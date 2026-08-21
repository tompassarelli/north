import { spawn } from "node:child_process";
import { appendFileSync, chmodSync, mkdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { listProviderAccounts, type ProviderAccount } from "./accounts";
import { refreshAccountUsages, type AccountUsageReport } from "./account-usage";
import { withFileLease } from "./file-lease";
import {
  readOpenAISessionActivity, type OpenAISessionActivity,
} from "./openai-session-activity";
import { automatedPressure } from "./resource-policy";
import type { EntitlementPressure, ProviderUsageWindow } from "./providers/types";

export const FLEET_ASSIGNMENT_VERSION = "north:fleet-dispatch-assignment:v1";
export const FLEET_ASSIGNMENT_PENALTY = 8;
export const FLEET_CLOSE_SCORE_DELTA = 8;
export const FLEET_OVERSUBSCRIPTION_DELTA = 3;

const SAFE_ASSIGNMENT_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;

export interface FleetAccountEvidence {
  accountId: string;
  usedPercent: number;
  headroom: EntitlementPressure;
  liveAssignments: number;
}

export interface FleetDispatchAssignment {
  version: typeof FLEET_ASSIGNMENT_VERSION;
  assignmentId: string;
  accountId: string;
  selectedAt: string;
  usedPercent: number;
  remainingHeadroom: number;
  liveAssignments: number;
  score: number;
  pid?: number;
}

export interface FleetDispatchCandidate extends FleetAccountEvidence {
  remainingHeadroom: number;
  score: number;
}

export interface FleetDispatchExclusion extends FleetAccountEvidence {
  reason: "low-headroom" | "usage-unavailable" | "materially-oversubscribed";
}

export interface FleetDispatchSelection {
  scope: "commander/operator-live-only";
  selected: FleetDispatchCandidate;
  closeCandidates: FleetDispatchCandidate[];
  eligible: FleetDispatchCandidate[];
  excluded: FleetDispatchExclusion[];
}

export interface FleetDispatchOptions {
  assignmentId?: string;
  codexArgs: string[];
  dryRun?: boolean;
}

interface FleetChild {
  pid?: number;
  completed: Promise<number>;
}

export interface FleetDispatchDependencies {
  accounts?: () => ProviderAccount[];
  refreshUsage?: typeof refreshAccountUsages;
  readActivity?: typeof readOpenAISessionActivity;
  now?: () => Date;
  assignmentPath?: string;
  startCodex?: (
    accountId: string, codexArgs: string[], env: NodeJS.ProcessEnv,
  ) => Promise<FleetChild>;
  env?: NodeJS.ProcessEnv;
  home?: string;
}

export interface FleetDispatchResult extends FleetDispatchSelection {
  assignment?: FleetDispatchAssignment;
  exitCode?: number;
}

function activeWindow(window: ProviderUsageWindow, now: Date): boolean {
  return window.resetState === "untouched" || Date.parse(window.resetsAt) > now.getTime();
}

function bindingUsedPercent(report: AccountUsageReport, now: Date): number | undefined {
  const used = report.observation.windows
    ?.filter((window) => activeWindow(window, now))
    .map(({ usedPercent }) => usedPercent)
    .filter((value) => Number.isFinite(value) && value >= 0 && value <= 100);
  return used?.length ? Math.max(...used) : undefined;
}

function lastSelectionByAccount(assignments: readonly FleetDispatchAssignment[]): Map<string, number> {
  const result = new Map<string, number>();
  assignments.forEach(({ accountId }, index) => result.set(accountId, index));
  return result;
}

/**
 * Rank live subscription accounts for an explicit commander/operator dispatch.
 * These session and usage observations never become Store routing authority.
 */
export function selectFleetDispatchAccount(
  evidence: readonly FleetAccountEvidence[],
  assignments: readonly FleetDispatchAssignment[] = [],
): FleetDispatchSelection {
  const excluded: FleetDispatchExclusion[] = [];
  const rankable = evidence.flatMap((row) => {
    if (!Number.isFinite(row.usedPercent) || row.usedPercent < 0 || row.usedPercent > 100
        || !Number.isInteger(row.liveAssignments) || row.liveAssignments < 0
        || row.headroom === "unknown") {
      excluded.push({ ...row, reason: "usage-unavailable" });
      return [];
    }
    if (row.headroom === "low" || row.headroom === "exhausted") {
      excluded.push({ ...row, reason: "low-headroom" });
      return [];
    }
    return [row];
  });
  if (!rankable.length) throw new Error("no Codex account has usable live headroom");

  const minimumAssignments = Math.min(...rankable.map(({ liveAssignments }) => liveAssignments));
  const balanced = rankable.filter((row) => {
    if (row.liveAssignments < minimumAssignments + FLEET_OVERSUBSCRIPTION_DELTA) return true;
    excluded.push({ ...row, reason: "materially-oversubscribed" });
    return false;
  });
  if (!balanced.length) throw new Error("all usable Codex accounts are materially oversubscribed");

  const eligible = balanced.map((row): FleetDispatchCandidate => {
    const remainingHeadroom = 100 - row.usedPercent;
    return {
      ...row,
      remainingHeadroom,
      score: remainingHeadroom - FLEET_ASSIGNMENT_PENALTY * row.liveAssignments,
    };
  }).sort((left, right) => right.score - left.score || left.accountId.localeCompare(right.accountId));
  const bestScore = eligible[0]!.score;
  const closeCandidates = eligible.filter(({ score }) => score >= bestScore - FLEET_CLOSE_SCORE_DELTA);
  const lastSelected = lastSelectionByAccount(assignments);
  const selected = [...closeCandidates].sort((left, right) => {
    const leftLast = lastSelected.get(left.accountId) ?? -1;
    const rightLast = lastSelected.get(right.accountId) ?? -1;
    return leftLast - rightLast || right.score - left.score || left.accountId.localeCompare(right.accountId);
  })[0]!;
  return {
    scope: "commander/operator-live-only",
    selected,
    closeCandidates,
    eligible,
    excluded: excluded.sort((left, right) => left.accountId.localeCompare(right.accountId)),
  };
}

function assignmentPath(dependencies: FleetDispatchDependencies): string {
  if (dependencies.assignmentPath) return dependencies.assignmentPath;
  const env = dependencies.env ?? process.env;
  const home = dependencies.home ?? env.HOME ?? homedir();
  const state = env.XDG_STATE_HOME ?? join(home, ".local/state");
  return join(state, "north/fleet-dispatch/assignments.jsonl");
}

export function readFleetDispatchAssignments(path: string): FleetDispatchAssignment[] {
  let contents: string;
  try { contents = readFileSync(path, "utf8"); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
    throw error;
  }
  return contents.split("\n").filter(Boolean).map((line, index) => {
    let value: unknown;
    try { value = JSON.parse(line); }
    catch { throw new Error(`fleet assignment ledger has malformed JSON on line ${index + 1}`); }
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw new Error(`fleet assignment ledger has an invalid record on line ${index + 1}`);
    const record = value as Partial<FleetDispatchAssignment>;
    if (record.version !== FLEET_ASSIGNMENT_VERSION
        || typeof record.assignmentId !== "string" || !SAFE_ASSIGNMENT_ID.test(record.assignmentId)
        || typeof record.accountId !== "string" || typeof record.selectedAt !== "string"
        || typeof record.score !== "number")
      throw new Error(`fleet assignment ledger has an invalid record on line ${index + 1}`);
    return record as FleetDispatchAssignment;
  });
}

function appendFleetDispatchAssignment(path: string, assignment: FleetDispatchAssignment): void {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  appendFileSync(path, `${JSON.stringify(assignment)}\n`, { encoding: "utf8", mode: 0o600 });
  chmodSync(path, 0o600);
}

async function collectFleetEvidence(
  dependencies: FleetDispatchDependencies,
): Promise<FleetAccountEvidence[]> {
  const accounts = (dependencies.accounts ?? listProviderAccounts)()
    .filter(({ provider }) => provider === "openai");
  if (!accounts.length) throw new Error("no isolated Codex accounts are configured");
  const now = (dependencies.now ?? (() => new Date()))();
  const reports = await (dependencies.refreshUsage ?? refreshAccountUsages)({
    accounts,
    force: true,
    allowLiveUsageWithoutPersistence: true,
  });
  const activities = new Map(await Promise.all(accounts.map(async (account) => [
    account.id,
    await (dependencies.readActivity ?? readOpenAISessionActivity)({
      accountRoot: account.root,
      now,
    }),
  ] as const)));
  return accounts.map((account): FleetAccountEvidence => {
    const report = reports.find(({ accountId }) => accountId === account.id);
    const usedPercent = report ? bindingUsedPercent(report, now) : undefined;
    const headroom = report ? automatedPressure(report.observation, now) ?? "unknown" : "unknown";
    return {
      accountId: account.id,
      usedPercent: usedPercent ?? Number.NaN,
      headroom,
      liveAssignments: activities.get(account.id)?.live ?? 0,
    };
  });
}

async function defaultStartCodex(
  accountId: string,
  codexArgs: string[],
  sourceEnv: NodeJS.ProcessEnv,
): Promise<FleetChild> {
  const env = { ...sourceEnv };
  delete env.CODEX_HOME;
  delete env.CODEX_SQLITE_HOME;
  delete env.CODEX_PROFILE;
  const command = env.NORTH_FLEET_CODEX_COMMAND ?? "codex";
  const child = spawn(command, ["as", accountId, ...codexArgs], {
    env,
    stdio: "inherit",
    detached: false,
  });
  await new Promise<void>((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", reject);
  });
  return {
    pid: child.pid,
    completed: new Promise<number>((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => {
        if (signal) reject(new Error(`Codex fleet assignment terminated by ${signal}`));
        else resolve(code ?? 1);
      });
    }),
  };
}

export async function executeFleetDispatch(
  options: FleetDispatchOptions,
  dependencies: FleetDispatchDependencies = {},
): Promise<FleetDispatchResult> {
  const evidence = await collectFleetEvidence(dependencies);
  const path = assignmentPath(dependencies);
  if (options.dryRun) {
    return selectFleetDispatchAccount(evidence, readFleetDispatchAssignments(path));
  }
  if (!options.assignmentId || !SAFE_ASSIGNMENT_ID.test(options.assignmentId))
    throw new Error("fleet dispatch requires --assignment with a portable assignment id");
  if (options.codexArgs[0] !== "exec")
    throw new Error("fleet dispatch launches a bounded Codex exec; pass -- exec [arguments]");

  const selectedAt = (dependencies.now ?? (() => new Date()))().toISOString();
  const launch = await withFileLease(`${path}.lock`, async () => {
    const existing = readFleetDispatchAssignments(path);
    if (existing.some(({ assignmentId }) => assignmentId === options.assignmentId))
      throw new Error(`fleet assignment already exists: ${options.assignmentId}`);
    const selection = selectFleetDispatchAccount(evidence, existing);
    const launchEnv = { ...(dependencies.env ?? process.env) };
    delete launchEnv.CODEX_HOME;
    delete launchEnv.CODEX_SQLITE_HOME;
    delete launchEnv.CODEX_PROFILE;
    const child = await (dependencies.startCodex ?? defaultStartCodex)(
      selection.selected.accountId,
      options.codexArgs,
      launchEnv,
    );
    const assignment: FleetDispatchAssignment = {
      version: FLEET_ASSIGNMENT_VERSION,
      assignmentId: options.assignmentId!,
      accountId: selection.selected.accountId,
      selectedAt,
      usedPercent: selection.selected.usedPercent,
      remainingHeadroom: selection.selected.remainingHeadroom,
      liveAssignments: selection.selected.liveAssignments,
      score: selection.selected.score,
      ...(child.pid === undefined ? {} : { pid: child.pid }),
    };
    appendFleetDispatchAssignment(path, assignment);
    return { selection, assignment, child };
  });
  const exitCode = await launch.child.completed;
  return { ...launch.selection, assignment: launch.assignment, exitCode };
}
