import { spawn } from "node:child_process";
import {
  chmodSync, mkdirSync, readFileSync, renameSync, writeFileSync,
} from "node:fs";
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

export const FLEET_STATE_VERSION = "north:fleet-dispatch-state:v2" as const;
export const FLEET_ASSIGNMENT_VERSION = "north:fleet-dispatch-assignment:v2" as const;
export const FLEET_CALIBRATION_VERSION = "north:fleet-token-percent-calibration:v1" as const;
export const FLEET_PROJECTED_USAGE_TIE_BAND = 1;
export const FLEET_LIVE_ASSIGNMENT_CAP = 8;
export const FLEET_RESERVATION_TTL_MS = 24 * 60 * 60 * 1_000;
export const FLEET_FALLBACK_TOKENS_PER_PERCENT = 10_000;
const FLEET_MIN_TOKENS_PER_PERCENT = 1_000;
const FLEET_MAX_TOKENS_PER_PERCENT = 1_000_000;

const SAFE_ASSIGNMENT_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;

export interface FleetAccountEvidence {
  accountId: string;
  usedPercent: number;
  headroom: EntitlementPressure;
  liveAssignments: number;
  totalTokens: number;
  windowId: string;
  windowResetsAt: string;
}

export interface FleetTokenCalibration {
  version: typeof FLEET_CALIBRATION_VERSION;
  source: "empirical" | "conservative-fallback";
  tokensPerPercent: number;
  samples: number;
  uncertainty: "medium" | "high";
}

export type FleetReservationStatus = "reserved" | "reconciled" | "cancelled" | "expired";

export interface FleetDispatchAssignment {
  version: typeof FLEET_ASSIGNMENT_VERSION;
  assignmentId: string;
  accountId: string;
  status: FleetReservationStatus;
  reservedAt: string;
  updatedAt: string;
  expiresAt: string;
  estimatedTokens: number;
  actualTokens?: number;
  observedUsedPercent: number;
  reconciledUsedPercent?: number;
  observedTotalTokens: number;
  windowId: string;
  windowResetsAt: string;
  projectedUsedPercent: number;
  postReservationProjectedUsedPercent: number;
  liveAssignments: number;
  calibration: FleetTokenCalibration;
  reconciliationUncertainty?: "isolated-account-window"
    | "overlapping-account-window"
    | "usage-refresh-unavailable";
  pid?: number;
}

export interface FleetDispatchState {
  version: typeof FLEET_STATE_VERSION;
  assignments: FleetDispatchAssignment[];
}

export interface FleetDispatchCandidate extends FleetAccountEvidence {
  remainingHeadroom: number;
  outstandingEstimatedTokens: number;
  projectedUsedPercent: number;
  postReservationProjectedUsedPercent: number;
  calibration: FleetTokenCalibration;
}

export interface FleetDispatchExclusion extends FleetAccountEvidence {
  reason: "low-headroom" | "usage-unavailable" | "live-cap";
}

export interface FleetDispatchSelection {
  scope: "commander/operator-live-only";
  estimatedTokens: number;
  selected: FleetDispatchCandidate;
  closeCandidates: FleetDispatchCandidate[];
  eligible: FleetDispatchCandidate[];
  excluded: FleetDispatchExclusion[];
}

export interface FleetDispatchOptions {
  assignmentId?: string;
  estimatedTokens: number;
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

interface BindingUsage {
  usedPercent: number;
  windowId: string;
  windowResetsAt: string;
}

function activeWindow(window: ProviderUsageWindow, now: Date): boolean {
  return window.resetState === "untouched" || Date.parse(window.resetsAt) > now.getTime();
}

function bindingUsage(report: AccountUsageReport, now: Date): BindingUsage | undefined {
  const binding = report.observation.windows
    ?.filter((window) => activeWindow(window, now)
      && Number.isFinite(window.usedPercent)
      && window.usedPercent >= 0
      && window.usedPercent <= 100)
    .sort((left, right) => right.usedPercent - left.usedPercent)[0];
  if (!binding) return undefined;
  const windowResetsAt = binding.resetState === "untouched" ? "untouched" : binding.resetsAt;
  return {
    usedPercent: binding.usedPercent,
    windowId: `${binding.limitId ?? "codex:binding"}@${windowResetsAt}`,
    windowResetsAt,
  };
}

function lastSelectionByAccount(assignments: readonly FleetDispatchAssignment[]): Map<string, number> {
  const result = new Map<string, number>();
  assignments.forEach(({ accountId }, index) => result.set(accountId, index));
  return result;
}

export function emptyFleetDispatchState(): FleetDispatchState {
  return { version: FLEET_STATE_VERSION, assignments: [] };
}

function validCalibration(value: unknown): value is FleetTokenCalibration {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const row = value as Partial<FleetTokenCalibration>;
  return row.version === FLEET_CALIBRATION_VERSION
    && (row.source === "empirical" || row.source === "conservative-fallback")
    && typeof row.tokensPerPercent === "number" && Number.isFinite(row.tokensPerPercent)
    && typeof row.samples === "number" && Number.isInteger(row.samples) && row.samples >= 0
    && (row.uncertainty === "medium" || row.uncertainty === "high");
}

function validAssignment(value: unknown): value is FleetDispatchAssignment {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const row = value as Partial<FleetDispatchAssignment>;
  return row.version === FLEET_ASSIGNMENT_VERSION
    && typeof row.assignmentId === "string" && SAFE_ASSIGNMENT_ID.test(row.assignmentId)
    && typeof row.accountId === "string" && row.accountId.length > 0
    && ["reserved", "reconciled", "cancelled", "expired"].includes(row.status ?? "")
    && typeof row.reservedAt === "string" && Number.isFinite(Date.parse(row.reservedAt))
    && typeof row.updatedAt === "string" && Number.isFinite(Date.parse(row.updatedAt))
    && typeof row.expiresAt === "string" && Number.isFinite(Date.parse(row.expiresAt))
    && typeof row.estimatedTokens === "number" && Number.isSafeInteger(row.estimatedTokens)
    && row.estimatedTokens > 0
    && typeof row.observedUsedPercent === "number" && Number.isFinite(row.observedUsedPercent)
    && typeof row.observedTotalTokens === "number" && Number.isFinite(row.observedTotalTokens)
    && typeof row.windowId === "string" && typeof row.windowResetsAt === "string"
    && typeof row.projectedUsedPercent === "number" && Number.isFinite(row.projectedUsedPercent)
    && typeof row.postReservationProjectedUsedPercent === "number"
    && Number.isFinite(row.postReservationProjectedUsedPercent)
    && typeof row.liveAssignments === "number" && Number.isInteger(row.liveAssignments)
    && validCalibration(row.calibration);
}

export function readFleetDispatchState(path: string): FleetDispatchState {
  let contents: string;
  try { contents = readFileSync(path, "utf8"); }
  catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return emptyFleetDispatchState();
    throw error;
  }
  let value: unknown;
  try { value = JSON.parse(contents); }
  catch { throw new Error("fleet dispatch state has malformed JSON"); }
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("fleet dispatch state is invalid");
  const state = value as Partial<FleetDispatchState>;
  if (state.version !== FLEET_STATE_VERSION || !Array.isArray(state.assignments)
      || !state.assignments.every(validAssignment))
    throw new Error("fleet dispatch state is invalid");
  const ids = new Set<string>();
  for (const assignment of state.assignments) {
    if (ids.has(assignment.assignmentId))
      throw new Error(`fleet dispatch state repeats assignment ${assignment.assignmentId}`);
    ids.add(assignment.assignmentId);
  }
  return state as FleetDispatchState;
}

function writeFleetDispatchState(path: string, state: FleetDispatchState): void {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const candidate = `${path}.candidate.${process.pid}`;
  writeFileSync(candidate, `${JSON.stringify(state, null, 2)}\n`, {
    encoding: "utf8", mode: 0o600,
  });
  renameSync(candidate, path);
  chmodSync(path, 0o600);
}

export function expireFleetDispatchReservations(
  state: FleetDispatchState,
  now: Date,
): { state: FleetDispatchState; expired: number } {
  let expired = 0;
  const assignments = state.assignments.map((assignment) => {
    if (assignment.status !== "reserved" || Date.parse(assignment.expiresAt) > now.getTime())
      return assignment;
    expired += 1;
    return { ...assignment, status: "expired" as const, updatedAt: now.toISOString() };
  });
  return { state: { ...state, assignments }, expired };
}

function calibrationFor(
  account: FleetAccountEvidence,
  state: FleetDispatchState,
): FleetTokenCalibration {
  const samples = state.assignments.flatMap((assignment) => {
    if (assignment.status !== "reconciled"
        || assignment.accountId !== account.accountId
        || assignment.windowId !== account.windowId
        || assignment.reconciliationUncertainty !== "isolated-account-window"
        || assignment.actualTokens === undefined
        || assignment.reconciledUsedPercent === undefined) return [];
    const percentDelta = assignment.reconciledUsedPercent - assignment.observedUsedPercent;
    if (percentDelta <= 0 || assignment.actualTokens <= 0) return [];
    return [assignment.actualTokens / percentDelta];
  }).sort((left, right) => left - right);
  if (!samples.length) {
    return {
      version: FLEET_CALIBRATION_VERSION,
      source: "conservative-fallback",
      tokensPerPercent: FLEET_FALLBACK_TOKENS_PER_PERCENT,
      samples: 0,
      uncertainty: "high",
    };
  }
  const median = samples[Math.floor(samples.length / 2)]!;
  const conservative = Math.max(
    FLEET_MIN_TOKENS_PER_PERCENT,
    Math.min(FLEET_MAX_TOKENS_PER_PERCENT, median * 0.75),
  );
  return {
    version: FLEET_CALIBRATION_VERSION,
    source: "empirical",
    tokensPerPercent: conservative,
    samples: samples.length,
    uncertainty: samples.length >= 3 ? "medium" : "high",
  };
}

function requireEstimatedTokens(estimatedTokens: number): void {
  if (!Number.isSafeInteger(estimatedTokens) || estimatedTokens <= 0)
    throw new Error("fleet dispatch requires a positive integer estimated-token budget");
}

/**
 * Rank live subscription accounts for an explicit commander/operator dispatch.
 * These observations and reservations never become Store routing authority.
 */
export function selectFleetDispatchAccount(
  evidence: readonly FleetAccountEvidence[],
  state: FleetDispatchState,
  estimatedTokens: number,
  now: Date,
): FleetDispatchSelection {
  requireEstimatedTokens(estimatedTokens);
  const excluded: FleetDispatchExclusion[] = [];
  const active = state.assignments.filter((assignment) =>
    assignment.status === "reserved" && Date.parse(assignment.expiresAt) > now.getTime());
  const rankable = evidence.flatMap((row) => {
    if (!Number.isFinite(row.usedPercent) || row.usedPercent < 0 || row.usedPercent > 100
        || !Number.isInteger(row.liveAssignments) || row.liveAssignments < 0
        || !Number.isFinite(row.totalTokens) || row.totalTokens < 0
        || !row.windowId || !row.windowResetsAt || row.headroom === "unknown") {
      excluded.push({ ...row, reason: "usage-unavailable" });
      return [];
    }
    if (row.headroom === "low" || row.headroom === "exhausted") {
      excluded.push({ ...row, reason: "low-headroom" });
      return [];
    }
    if (row.liveAssignments >= FLEET_LIVE_ASSIGNMENT_CAP) {
      excluded.push({ ...row, reason: "live-cap" });
      return [];
    }
    const calibration = calibrationFor(row, state);
    const outstandingEstimatedTokens = active
      .filter(({ accountId, windowId }) => accountId === row.accountId && windowId === row.windowId)
      .reduce((sum, assignment) => sum + assignment.estimatedTokens, 0);
    const projectedUsedPercent = row.usedPercent
      + outstandingEstimatedTokens / calibration.tokensPerPercent;
    return [{
      ...row,
      remainingHeadroom: 100 - row.usedPercent,
      outstandingEstimatedTokens,
      projectedUsedPercent,
      postReservationProjectedUsedPercent:
        projectedUsedPercent + estimatedTokens / calibration.tokensPerPercent,
      calibration,
    }];
  });
  if (!rankable.length) throw new Error("no Codex account has usable live headroom");

  const eligible = rankable.sort((left, right) =>
    left.projectedUsedPercent - right.projectedUsedPercent
    || left.accountId.localeCompare(right.accountId));
  const bestProjectedUsage = eligible[0]!.projectedUsedPercent;
  const closeCandidates = eligible.filter(({ projectedUsedPercent }) =>
    projectedUsedPercent <= bestProjectedUsage + FLEET_PROJECTED_USAGE_TIE_BAND);
  const lastSelected = lastSelectionByAccount(state.assignments);
  const selected = [...closeCandidates].sort((left, right) => {
    const leftLast = lastSelected.get(left.accountId) ?? -1;
    const rightLast = lastSelected.get(right.accountId) ?? -1;
    return leftLast - rightLast
      || left.projectedUsedPercent - right.projectedUsedPercent
      || left.accountId.localeCompare(right.accountId);
  })[0]!;
  return {
    scope: "commander/operator-live-only",
    estimatedTokens,
    selected,
    closeCandidates,
    eligible,
    excluded: excluded.sort((left, right) => left.accountId.localeCompare(right.accountId)),
  };
}

export function reserveFleetDispatchAccount(
  evidence: readonly FleetAccountEvidence[],
  state: FleetDispatchState,
  assignmentId: string,
  estimatedTokens: number,
  now: Date,
): { state: FleetDispatchState; selection: FleetDispatchSelection; assignment: FleetDispatchAssignment } {
  if (!SAFE_ASSIGNMENT_ID.test(assignmentId))
    throw new Error("fleet dispatch requires --assignment with a portable assignment id");
  if (state.assignments.some((assignment) => assignment.assignmentId === assignmentId))
    throw new Error(`fleet assignment already exists: ${assignmentId}`);
  const selection = selectFleetDispatchAccount(evidence, state, estimatedTokens, now);
  const selected = selection.selected;
  const timestamp = now.toISOString();
  const assignment: FleetDispatchAssignment = {
    version: FLEET_ASSIGNMENT_VERSION,
    assignmentId,
    accountId: selected.accountId,
    status: "reserved",
    reservedAt: timestamp,
    updatedAt: timestamp,
    expiresAt: new Date(now.getTime() + FLEET_RESERVATION_TTL_MS).toISOString(),
    estimatedTokens,
    observedUsedPercent: selected.usedPercent,
    observedTotalTokens: selected.totalTokens,
    windowId: selected.windowId,
    windowResetsAt: selected.windowResetsAt,
    projectedUsedPercent: selected.projectedUsedPercent,
    postReservationProjectedUsedPercent: selected.postReservationProjectedUsedPercent,
    liveAssignments: selected.liveAssignments,
    calibration: selected.calibration,
  };
  return {
    selection,
    assignment,
    state: { ...state, assignments: [...state.assignments, assignment] },
  };
}

function assignmentPath(dependencies: FleetDispatchDependencies): string {
  if (dependencies.assignmentPath) return dependencies.assignmentPath;
  const env = dependencies.env ?? process.env;
  const home = dependencies.home ?? env.HOME ?? homedir();
  const state = env.XDG_STATE_HOME ?? join(home, ".local/state");
  return join(state, "north/fleet-dispatch/reservations.json");
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
    const binding = report ? bindingUsage(report, now) : undefined;
    const headroom = report ? automatedPressure(report.observation, now) ?? "unknown" : "unknown";
    const activity = activities.get(account.id);
    return {
      accountId: account.id,
      usedPercent: binding?.usedPercent ?? Number.NaN,
      headroom,
      liveAssignments: activity?.live ?? 0,
      totalTokens: activity?.totalTokens ?? Number.NaN,
      windowId: binding?.windowId ?? "",
      windowResetsAt: binding?.windowResetsAt ?? "",
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

async function updateAssignment(
  path: string,
  assignmentId: string,
  update: (assignment: FleetDispatchAssignment, state: FleetDispatchState) => FleetDispatchAssignment,
): Promise<FleetDispatchAssignment> {
  return withFileLease(`${path}.lock`, async () => {
    const state = readFleetDispatchState(path);
    const index = state.assignments.findIndex((assignment) => assignment.assignmentId === assignmentId);
    if (index < 0) throw new Error(`fleet assignment is missing: ${assignmentId}`);
    const assignment = update(state.assignments[index]!, state);
    const assignments = [...state.assignments];
    assignments[index] = assignment;
    writeFleetDispatchState(path, { ...state, assignments });
    return assignment;
  });
}

async function cancelAssignment(
  path: string,
  assignmentId: string,
  now: Date,
): Promise<FleetDispatchAssignment> {
  return updateAssignment(path, assignmentId, (assignment) => {
    if (assignment.status !== "reserved") return assignment;
    return { ...assignment, status: "cancelled", updatedAt: now.toISOString() };
  });
}

async function reconcileAssignment(
  path: string,
  assignmentId: string,
  after: FleetAccountEvidence | undefined,
  usageRefreshAvailable: boolean,
  now: Date,
): Promise<FleetDispatchAssignment> {
  return updateAssignment(path, assignmentId, (assignment, state) => {
    if (assignment.status !== "reserved") return assignment;
    const overlaps = state.assignments.some((other) =>
      other.assignmentId !== assignmentId
      && other.accountId === assignment.accountId
      && other.windowId === assignment.windowId
      && Date.parse(other.reservedAt) <= now.getTime()
      && (other.status === "reserved"
        || Date.parse(other.updatedAt) >= Date.parse(assignment.reservedAt)));
    const sameWindow = after?.windowId === assignment.windowId;
    const actualTokens = sameWindow
      ? Math.max(0, after.totalTokens - assignment.observedTotalTokens)
      : 0;
    return {
      ...assignment,
      status: "reconciled",
      updatedAt: now.toISOString(),
      actualTokens,
      ...(sameWindow ? { reconciledUsedPercent: after.usedPercent } : {}),
      reconciliationUncertainty: !usageRefreshAvailable
        ? "usage-refresh-unavailable"
        : overlaps || assignment.liveAssignments > 0
          ? "overlapping-account-window"
          : "isolated-account-window",
    };
  });
}

export async function executeFleetDispatch(
  options: FleetDispatchOptions,
  dependencies: FleetDispatchDependencies = {},
): Promise<FleetDispatchResult> {
  requireEstimatedTokens(options.estimatedTokens);
  const evidence = await collectFleetEvidence(dependencies);
  const path = assignmentPath(dependencies);
  const now = (dependencies.now ?? (() => new Date()))();
  if (options.dryRun) {
    return selectFleetDispatchAccount(
      evidence,
      readFleetDispatchState(path),
      options.estimatedTokens,
      now,
    );
  }
  if (!options.assignmentId || !SAFE_ASSIGNMENT_ID.test(options.assignmentId))
    throw new Error("fleet dispatch requires --assignment with a portable assignment id");
  if (options.codexArgs[0] !== "exec")
    throw new Error("fleet dispatch launches a bounded Codex exec; pass -- exec [arguments]");

  const reserved = await withFileLease(`${path}.lock`, async () => {
    const current = readFleetDispatchState(path);
    const expired = expireFleetDispatchReservations(current, now);
    if (expired.expired > 0) writeFleetDispatchState(path, expired.state);
    const reservation = reserveFleetDispatchAccount(
      evidence,
      expired.state,
      options.assignmentId!,
      options.estimatedTokens,
      now,
    );
    writeFleetDispatchState(path, reservation.state);
    return reservation;
  });
  const launchEnv = { ...(dependencies.env ?? process.env) };
  delete launchEnv.CODEX_HOME;
  delete launchEnv.CODEX_SQLITE_HOME;
  delete launchEnv.CODEX_PROFILE;

  let child: FleetChild;
  try {
    child = await (dependencies.startCodex ?? defaultStartCodex)(
      reserved.selection.selected.accountId,
      options.codexArgs,
      launchEnv,
    );
  } catch (error) {
    await cancelAssignment(path, reserved.assignment.assignmentId, now);
    throw error;
  }
  if (child.pid !== undefined) {
    await updateAssignment(path, reserved.assignment.assignmentId, (assignment) => ({
      ...assignment, pid: child.pid,
    }));
  }

  let exitCode: number;
  try { exitCode = await child.completed; }
  catch (error) {
    await cancelAssignment(
      path,
      reserved.assignment.assignmentId,
      (dependencies.now ?? (() => new Date()))(),
    );
    throw error;
  }
  const completedAt = (dependencies.now ?? (() => new Date()))();
  let after: FleetAccountEvidence | undefined;
  let usageRefreshAvailable = true;
  try {
    after = (await collectFleetEvidence(dependencies))
      .find(({ accountId }) => accountId === reserved.assignment.accountId);
    if (!after) usageRefreshAvailable = false;
  } catch {
    usageRefreshAvailable = false;
    const account = (dependencies.accounts ?? listProviderAccounts)()
      .find(({ id }) => id === reserved.assignment.accountId);
    const activity: OpenAISessionActivity | undefined = account
      ? await (dependencies.readActivity ?? readOpenAISessionActivity)({
        accountRoot: account.root,
        now: completedAt,
      }).catch(() => undefined)
      : undefined;
    after = {
      ...reserved.selection.selected,
      liveAssignments: activity?.live ?? reserved.selection.selected.liveAssignments,
      totalTokens: activity?.totalTokens ?? reserved.selection.selected.totalTokens,
    };
  }
  const assignment = await reconcileAssignment(
    path,
    reserved.assignment.assignmentId,
    after,
    usageRefreshAvailable,
    completedAt,
  );
  return { ...reserved.selection, assignment, exitCode };
}
