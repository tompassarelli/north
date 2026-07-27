import { spawnSync } from "node:child_process";
import {
  appendFileSync,
  closeSync,
  mkdirSync,
  openSync,
  readFileSync,
  statSync,
  unlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { dirname } from "node:path";
import { parseStrictJson } from "./strict-json";

export const DEFAULT_COOKED_THRESHOLD = 98;
export const DEFAULT_HEARTBEAT_STALE_MS = 90 * 60 * 1_000;
export const DEFAULT_COMMAND_TIMEOUT_MS = 2_000;
export const DEFAULT_COORDINATOR_MODEL = "claude-fable-5";

export interface AvailabilityRung {
  pct: number;
  resetsAt: string;
  observedAt: string;
}

export interface ModelAvailabilityRung extends AvailabilityRung {
  model?: string;
}

export interface AvailabilityAccount {
  accountId: string;
  provider: string;
  eligible?: boolean;
  stale: boolean;
  rungs: {
    window?: AvailabilityRung | null;
    week?: AvailabilityRung | null;
    models?: Record<string, AvailabilityRung> | ModelAvailabilityRung[];
  };
}

export interface AvailabilityDocument {
  schemaVersion?: number;
  accounts: AvailabilityAccount[];
}

export type HeartbeatSource = "graph" | "file" | "missing";

export interface HeartbeatEvidence {
  source: HeartbeatSource;
  observedAt?: string;
  ageMs?: number;
  stale: boolean;
  daemonReachable: boolean;
}

export interface AccountCookedDecision {
  accountId: string;
  cooked: boolean;
  rung?: "week" | "window" | "model";
  pct?: number;
  model?: string;
}

export interface SuccessionDecision {
  action: "fire" | "hold";
  reason:
    | "all-anthropic-accounts-cooked"
    | "anthropic-account-not-cooked"
    | "stale-evidence-heartbeat-stale"
    | "stale-evidence-heartbeat-fresh";
  evidenceStale: boolean;
  threshold: number;
  coordinatorModel: string;
  accounts: AccountCookedDecision[];
  heartbeat: HeartbeatEvidence;
}

export interface CommandResult {
  status: number | null;
  stdout: string;
  stderr: string;
  timedOut: boolean;
}

export type CommandRunner = (
  command: string,
  args: string[],
  timeoutMs: number,
) => CommandResult;

function object(value: unknown, label: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value as Record<string, unknown>;
}

function string(value: unknown, label: string): string {
  if (typeof value !== "string" || !value) throw new Error(`${label} must be a nonempty string`);
  return value;
}

function finitePercent(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 100)
    throw new Error(`${label} must be a percentage from 0 through 100`);
  return value;
}

function rung(value: unknown, label: string): AvailabilityRung | null | undefined {
  if (value === undefined || value === null) return value;
  const row = object(value, label);
  return {
    pct: finitePercent(row.pct, `${label}.pct`),
    resetsAt: string(row.resetsAt, `${label}.resetsAt`),
    observedAt: string(row.observedAt, `${label}.observedAt`),
  };
}

function account(value: unknown, index: number): AvailabilityAccount {
  const row = object(value, `availability.accounts[${index}]`);
  const rungs = object(row.rungs, `availability.accounts[${index}].rungs`);
  const modelsValue = rungs.models;
  let models: AvailabilityAccount["rungs"]["models"];
  if (Array.isArray(modelsValue)) {
    models = modelsValue.map((entry, modelIndex) => {
      const modelRow = object(entry, `availability.accounts[${index}].rungs.models[${modelIndex}]`);
      return {
        ...rung(modelRow, `availability.accounts[${index}].rungs.models[${modelIndex}]`)!,
        model: string(modelRow.model, `availability.accounts[${index}].rungs.models[${modelIndex}].model`),
      };
    });
  } else if (modelsValue !== undefined) {
    const modelRows = object(modelsValue, `availability.accounts[${index}].rungs.models`);
    models = Object.fromEntries(Object.entries(modelRows).map(([model, value]) => [
      model,
      rung(value, `availability.accounts[${index}].rungs.models.${model}`)!,
    ]));
  }
  if (typeof row.stale !== "boolean")
    throw new Error(`availability.accounts[${index}].stale must be boolean`);
  if (row.eligible !== undefined && typeof row.eligible !== "boolean")
    throw new Error(`availability.accounts[${index}].eligible must be boolean`);
  return {
    accountId: string(row.accountId ?? row.id, `availability.accounts[${index}].accountId`),
    provider: string(row.provider, `availability.accounts[${index}].provider`),
    ...(row.eligible === undefined ? {} : { eligible: row.eligible }),
    stale: row.stale,
    rungs: {
      window: rung(rungs.window, `availability.accounts[${index}].rungs.window`),
      week: rung(rungs.week, `availability.accounts[${index}].rungs.week`),
      ...(models === undefined ? {} : { models }),
    },
  };
}

export function parseAvailabilityDocument(text: string): AvailabilityDocument {
  const parsed = parseStrictJson(text, "account availability", {
    maxBytes: 512 * 1_024,
    maxDepth: 32,
    maxNodes: 20_000,
  });
  const root = Array.isArray(parsed) ? { accounts: parsed } : object(parsed, "account availability");
  if (!Array.isArray(root.accounts)) throw new Error("account availability accounts must be an array");
  return {
    ...(typeof root.schemaVersion === "number" ? { schemaVersion: root.schemaVersion } : {}),
    accounts: root.accounts.map(account),
  };
}

function modelRung(
  models: AvailabilityAccount["rungs"]["models"],
  coordinatorModel: string,
): AvailabilityRung | undefined {
  if (!models) return undefined;
  if (Array.isArray(models))
    return models.find((entry) => entry.model === coordinatorModel);
  return models[coordinatorModel];
}

export function classifyAccountCooked(
  row: AvailabilityAccount,
  threshold: number,
  coordinatorModel: string,
): AccountCookedDecision {
  if (row.rungs.week && row.rungs.week.pct >= threshold)
    return { accountId: row.accountId, cooked: true, rung: "week", pct: row.rungs.week.pct };
  if (row.rungs.window && row.rungs.window.pct >= threshold)
    return { accountId: row.accountId, cooked: true, rung: "window", pct: row.rungs.window.pct };
  const model = modelRung(row.rungs.models, coordinatorModel);
  if (model && model.pct >= threshold)
    return { accountId: row.accountId, cooked: true, rung: "model", pct: model.pct, model: coordinatorModel };
  return { accountId: row.accountId, cooked: false };
}

export function decideSuccession(
  availability: AvailabilityDocument,
  heartbeat: HeartbeatEvidence,
  threshold = DEFAULT_COOKED_THRESHOLD,
  coordinatorModel = DEFAULT_COORDINATOR_MODEL,
): SuccessionDecision {
  if (!Number.isFinite(threshold) || threshold < 0 || threshold > 100)
    throw new Error("cooked threshold must be a percentage from 0 through 100");
  const eligible = availability.accounts.filter((row) =>
    row.provider === "anthropic" && row.eligible !== false);
  const evidenceStale = eligible.length === 0 || eligible.some((row) => row.stale);
  const accounts = eligible.map((row) => classifyAccountCooked(row, threshold, coordinatorModel));
  if (evidenceStale) {
    return {
      action: heartbeat.stale ? "fire" : "hold",
      reason: heartbeat.stale
        ? "stale-evidence-heartbeat-stale"
        : "stale-evidence-heartbeat-fresh",
      evidenceStale,
      threshold,
      coordinatorModel,
      accounts,
      heartbeat,
    };
  }
  const allCooked = accounts.length > 0 && accounts.every((row) => row.cooked);
  return {
    action: allCooked ? "fire" : "hold",
    reason: allCooked ? "all-anthropic-accounts-cooked" : "anthropic-account-not-cooked",
    evidenceStale,
    threshold,
    coordinatorModel,
    accounts,
    heartbeat,
  };
}

export const productionCommandRunner: CommandRunner = (command, args, timeoutMs) => {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    timeout: Math.max(1, Math.floor(timeoutMs)),
    maxBuffer: 1024 * 1024,
  });
  return {
    status: result.status,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    timedOut: (result.error as NodeJS.ErrnoException | undefined)?.code === "ETIMEDOUT",
  };
};

function timestampFromGraphShow(text: string): string | undefined {
  const parsed = parseStrictJson(text, "north json show", {
    maxBytes: 256 * 1_024,
    maxDepth: 16,
    maxNodes: 10_000,
  });
  if (!Array.isArray(parsed)) throw new Error("north json show must return an array");
  const values = parsed.flatMap((value) => {
    const fact = object(value, "north json show fact");
    return fact.predicate === "coordinator_pulse" && typeof fact.value === "string"
      ? [fact.value]
      : [];
  });
  return values.sort((left, right) => Date.parse(right) - Date.parse(left))[0];
}

function evidenceFromTimestamp(
  source: HeartbeatSource,
  observedAt: string | undefined,
  daemonReachable: boolean,
  now: Date,
  staleMs: number,
): HeartbeatEvidence {
  const timestamp = observedAt ? Date.parse(observedAt) : Number.NaN;
  if (!Number.isFinite(timestamp)) return { source: "missing", stale: true, daemonReachable };
  const ageMs = Math.max(0, now.getTime() - timestamp);
  return { source, observedAt, ageMs, stale: ageMs >= staleMs, daemonReachable };
}

export function readHeartbeatEvidence(options: {
  northBin: string;
  thread: string;
  fallbackFile: string;
  now?: Date;
  staleMs?: number;
  timeoutMs?: number;
  run?: CommandRunner;
}): HeartbeatEvidence {
  const now = options.now ?? new Date();
  const staleMs = options.staleMs ?? DEFAULT_HEARTBEAT_STALE_MS;
  const run = options.run ?? productionCommandRunner;
  const result = run(
    options.northBin,
    ["json", "show", options.thread.replace(/^@/, "")],
    options.timeoutMs ?? DEFAULT_COMMAND_TIMEOUT_MS,
  );
  if (result.status === 0 && !result.timedOut) {
    try {
      return evidenceFromTimestamp("graph", timestampFromGraphShow(result.stdout), true, now, staleMs);
    } catch {
      return { source: "missing", stale: true, daemonReachable: true };
    }
  }
  try {
    const stat = statSync(options.fallbackFile);
    return evidenceFromTimestamp("file", stat.mtime.toISOString(), false, now, staleMs);
  } catch {
    return { source: "missing", stale: true, daemonReachable: false };
  }
}

export function recordPulse(options: {
  northBin: string;
  thread: string;
  fallbackFile: string;
  now?: Date;
  timeoutMs?: number;
  run?: CommandRunner;
}): void {
  const now = options.now ?? new Date();
  mkdirSync(dirname(options.fallbackFile), { recursive: true, mode: 0o700 });
  closeSync(openSync(options.fallbackFile, "a", 0o600));
  utimesSync(options.fallbackFile, now, now);
  const run = options.run ?? productionCommandRunner;
  const timeout = options.timeoutMs ?? DEFAULT_COMMAND_TIMEOUT_MS;
  const schema = run(options.northBin, ["tell", "coordinator_pulse", "cardinality", "single"], timeout);
  if (schema.status !== 0 || schema.timedOut)
    throw new Error(`coordinator pulse schema write unavailable: ${schema.stderr.trim() || "timeout"}`);
  const pulse = run(options.northBin, [
    "tell", options.thread.replace(/^@/, ""), "coordinator_pulse", now.toISOString(),
  ], timeout);
  if (pulse.status !== 0 || pulse.timedOut)
    throw new Error(`coordinator pulse fact write unavailable: ${pulse.stderr.trim() || "timeout"}`);
}

export function claimFire(markerFile: string, decision: SuccessionDecision, now = new Date()): boolean {
  mkdirSync(dirname(markerFile), { recursive: true, mode: 0o700 });
  try {
    const fd = openSync(markerFile, "wx", 0o600);
    try {
      writeFileSync(fd, `${JSON.stringify({ claimedAt: now.toISOString(), decision })}\n`);
    } finally {
      closeSync(fd);
    }
    return true;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "EEXIST") return false;
    throw error;
  }
}

export function appendDecisionFact(options: {
  northBin: string;
  thread: string;
  predicate: "succession_decision" | "succession_fire";
  value: unknown;
  timeoutMs?: number;
  run?: CommandRunner;
}): void {
  const result = (options.run ?? productionCommandRunner)(
    options.northBin,
    [
      "tell",
      options.thread.replace(/^@/, ""),
      options.predicate,
      JSON.stringify(options.value),
    ],
    options.timeoutMs ?? DEFAULT_COMMAND_TIMEOUT_MS,
  );
  if (result.status !== 0 || result.timedOut)
    throw new Error(`${options.predicate} fact write unavailable: ${result.stderr.trim() || "timeout"}`);
}

interface PendingFact {
  predicate: "succession_decision" | "succession_fire";
  value: unknown;
}

function validPendingFact(value: unknown): PendingFact {
  const row = object(value, "pending succession fact");
  if (row.predicate !== "succession_decision" && row.predicate !== "succession_fire")
    throw new Error("pending succession fact predicate is invalid");
  return { predicate: row.predicate, value: row.value };
}

export function spoolDecisionFact(path: string, fact: PendingFact): void {
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  appendFileSync(path, `${JSON.stringify(fact)}\n`, { encoding: "utf8", mode: 0o600 });
}

export function flushDecisionSpool(options: {
  path: string;
  northBin: string;
  thread: string;
  timeoutMs?: number;
  run?: CommandRunner;
}): number {
  let text: string;
  try {
    text = readFileSync(options.path, "utf8");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return 0;
    throw error;
  }
  const pending = text.split("\n").filter(Boolean).map((line) =>
    validPendingFact(parseStrictJson(line, "pending succession fact", {
      maxBytes: 256 * 1_024,
      maxDepth: 32,
      maxNodes: 20_000,
    })));
  let flushed = 0;
  for (let index = 0; index < pending.length; index++) {
    const fact = pending[index]!;
    try {
      appendDecisionFact({
        northBin: options.northBin,
        thread: options.thread,
        predicate: fact.predicate,
        value: fact.value,
        timeoutMs: options.timeoutMs,
        run: options.run,
      });
    } catch {
      writeFileSync(
        options.path,
        `${pending.slice(index).map((entry) => JSON.stringify(entry)).join("\n")}\n`,
        { encoding: "utf8", mode: 0o600 },
      );
      return flushed;
    }
    flushed++;
  }
  unlinkSync(options.path);
  return flushed;
}
