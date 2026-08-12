import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync, closeSync, fsyncSync, linkSync, lstatSync, mkdirSync, openSync,
  readdirSync, readFileSync, unlinkSync, writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import type { WireQuery } from "./wire/query";
import type { WireRunSnapshot } from "./wire/reducer";
import { normalizeUsage, tokensOf, type TokenTotalStatus } from "./usage";
import {
  registerHostTerminationParticipant,
  type HostTerminationParticipant,
  type HostTerminationParticipantOptions,
  type HostTerminationSignal,
} from "./host-termination";

const REPO = resolve(import.meta.dir, "..", "..");
const northBin = () => process.env.NORTH_BIN ?? `${REPO}/bin/north`;

export const DEFAULT_MANAGED_SESSION_HARD_CAP_MS = 60 * 60_000;

export interface SessionHardCapContext {
  agentId: string;
  threadId?: string;
  goal: string;
  repo: string;
  worktree?: string;
  branch?: string;
}

export interface SessionHardCapDocument extends SessionHardCapContext {
  version: 1;
  reason: "session_hard_cap";
  writtenAt: string;
  hardCapMs: number;
  nextAction: string;
  completionClaimed: false;
}

export interface SessionHardCapWriteResult {
  path: string;
  indexed: boolean;
  spooled?: boolean;
  idempotencyKey?: string;
}

export interface SessionHardCapWriterRuntime {
  stateDirectory?: string;
  now?: () => Date;
  indexHandoff?: (threadId: string, value: string) => void;
}

export interface ManagedSessionHardCapOptions extends SessionHardCapContext {
  /** Test-only injection. Production callers omit this fixed code-owned bound. */
  hardCapMs?: number;
  schedule?: (callback: () => void | Promise<void>, delayMs: number) => unknown;
  cancel?: (timer: unknown) => void;
  writeHandoff?: (document: SessionHardCapDocument) => SessionHardCapWriteResult;
  replayHandoffs?: () => number;
  /** Test seam; production stores the deadline under the normal North state root. */
  stateDirectory?: string;
  /** Test seam for deterministic restart/recovery deadlines. */
  now?: () => Date;
  /** Caller-supplied inter-call target; this is not a no-overshoot ceiling. */
  tokenTarget?: number;
}

export interface SessionHardCapStatus {
  hardCapMs: number;
  handoffPath?: string;
  indexed: boolean;
  spooled: boolean;
}

export type ManagedRunTokenBudgetState =
  | "within_target"
  | "budget_limited"
  | "unenforceable";

export interface ManagedRunTokenBudgetStatus {
  targetTokens: number;
  coverage: TokenTotalStatus;
  state: ManagedRunTokenBudgetState;
  observedTokens?: number;
  overshootTokens?: number;
}

export interface ManagedRunTokenBudgetHandoff {
  reason: "managed_run_token_budget_limited";
  target: number;
  observed: number;
  overshoot: number;
  coverage: "exact";
}

export function managedRunTokenBudgetHandoff(
  status: ManagedRunTokenBudgetStatus,
): ManagedRunTokenBudgetHandoff {
  if (status.state !== "budget_limited" || status.coverage !== "exact"
      || status.observedTokens === undefined || status.overshootTokens === undefined) {
    throw new Error("managed run token budget handoff requires exact limited usage");
  }
  return Object.freeze({
    reason: "managed_run_token_budget_limited",
    target: status.targetTokens,
    observed: status.observedTokens,
    overshoot: status.overshootTokens,
    coverage: status.coverage,
  });
}

export function managedRunTokenTarget(value: unknown): number | undefined {
  if (value === undefined) return undefined;
  if (!Number.isSafeInteger(value) || (value as number) <= 0) {
    throw new Error("managed run token target must be a positive safe integer");
  }
  return value as number;
}

interface SessionHardCapIndexRecord {
  version: 1;
  type: "session_hard_cap_handoff";
  idempotencyKey: string;
  threadId: string;
  predicate: "handoff";
  value: string;
  artifactPath: string;
  artifactSha256: string;
}

interface SessionHardCapDeadlineDocument extends SessionHardCapContext {
  version: 1;
  type: "managed_session_deadline";
  startedAt: string;
  deadlineAt: string;
  hardCapMs: number;
}

interface SessionHardCapDeadline {
  remainingMs: number;
  deadlineAtMs: number;
  path?: string;
  serialized?: string;
}

const MAX_HANDOFF_REPLAY_RECORDS = 8;
const MAX_HANDOFF_RECORD_BYTES = 64 * 1024;
// Both Codex transports own bounded process-tree teardown below 3.5s.
const HARD_CAP_QUERY_INTERRUPT_TIMEOUT_MS = 5_000;

function sessionHardCapDocument(
  context: SessionHardCapContext,
  hardCapMs: number,
  now: Date,
): SessionHardCapDocument {
  return {
    version: 1,
    reason: "session_hard_cap",
    writtenAt: now.toISOString(),
    hardCapMs,
    ...context,
    nextAction: "Resume only this deliverable; inspect the named thread, worktree, branch, and session transcript before editing.",
    completionClaimed: false,
  };
}

function safeHandoffFilename(agentId: string): string {
  const label = agentId.replace(/[^A-Za-z0-9._-]+/g, "-").slice(0, 80) || "agent";
  const digest = createHash("sha256").update(agentId).digest("hex").slice(0, 12);
  return `${label}-${digest}.json`;
}

function validNow(runtime: Pick<SessionHardCapWriterRuntime, "now">): Date {
  const now = (runtime.now ?? (() => new Date()))();
  if (!Number.isFinite(now.getTime()))
    throw new Error("managed session hard-cap clock returned an invalid date");
  return now;
}

function sessionHardCapDeadlineDocument(
  context: SessionHardCapContext,
  hardCapMs: number,
  startedAt: Date,
): SessionHardCapDeadlineDocument {
  return {
    version: 1,
    type: "managed_session_deadline",
    startedAt: startedAt.toISOString(),
    deadlineAt: new Date(startedAt.getTime() + hardCapMs).toISOString(),
    hardCapMs,
    ...context,
  };
}

function sha256(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

function handoffStateDirectory(runtime: SessionHardCapWriterRuntime): string {
  return runtime.stateDirectory
    ?? resolve(homedir(), ".local/state/north/session-handoffs");
}

function ensurePrivateDirectory(path: string): void {
  mkdirSync(path, { recursive: true, mode: 0o700 });
  const stat = lstatSync(path);
  if (!stat.isDirectory() || stat.isSymbolicLink())
    throw new Error(`session handoff state path is not a private directory: ${path}`);
  chmodSync(path, 0o700);
  fsyncDirectory(path);
  if (dirname(path) !== path) fsyncDirectory(dirname(path));
}

function fsyncDirectory(path: string): void {
  const fd = openSync(path, "r");
  try { fsyncSync(fd); }
  finally { closeSync(fd); }
}

function atomicWriteOnce(path: string, serialized: string): boolean {
  const directory = dirname(path);
  ensurePrivateDirectory(directory);
  const temporary = `${path}.${process.pid}.${randomUUID()}.tmp`;
  const fd = openSync(temporary, "wx", 0o600);
  try {
    writeFileSync(fd, serialized, { encoding: "utf8" });
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
  try {
    try {
      linkSync(temporary, path);
      fsyncDirectory(directory);
      return true;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") {
        throw error;
      }
      const stat = lstatSync(path);
      if (!stat.isFile() || stat.isSymbolicLink()
          || readFileSync(path, "utf8") !== serialized) throw error;
      return false;
    }
  } finally {
    try {
      unlinkSync(temporary);
      fsyncDirectory(directory);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
  }
}

function removeDurably(path: string): void {
  try {
    unlinkSync(path);
    fsyncDirectory(dirname(path));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
}

function existingHandoffDocument(
  path: string,
  context: SessionHardCapContext,
  hardCapMs: number,
): SessionHardCapDocument {
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_HANDOFF_RECORD_BYTES)
    throw new Error(`session handoff artifact is unsafe: ${path}`);
  const parsed = JSON.parse(readFileSync(path, "utf8")) as SessionHardCapDocument;
  if (typeof parsed.writtenAt !== "string")
    throw new Error(`session handoff artifact has no writtenAt: ${path}`);
  const expected = sessionHardCapDocument(context, hardCapMs, new Date(parsed.writtenAt));
  if (`${JSON.stringify(parsed, null, 2)}\n` !== `${JSON.stringify(expected, null, 2)}\n`)
    throw new Error(`session handoff artifact conflicts with its idempotency identity: ${path}`);
  return parsed;
}

function durableHandoffDocument(
  path: string,
  context: SessionHardCapContext,
  hardCapMs: number,
  now: Date,
): SessionHardCapDocument {
  try {
    return existingHandoffDocument(path, context, hardCapMs);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  const document = sessionHardCapDocument(context, hardCapMs, now);
  const serialized = `${JSON.stringify(document, null, 2)}\n`;
  try {
    atomicWriteOnce(path, serialized);
    return document;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    return existingHandoffDocument(path, context, hardCapMs);
  }
}

function existingSessionHardCapDeadline(
  path: string,
  context: SessionHardCapContext,
  hardCapMs: number,
): SessionHardCapDeadlineDocument {
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_HANDOFF_RECORD_BYTES)
    throw new Error(`managed session deadline is unsafe: ${path}`);
  const parsed = JSON.parse(readFileSync(path, "utf8")) as SessionHardCapDeadlineDocument;
  const startedAt = new Date(parsed.startedAt);
  const expected = sessionHardCapDeadlineDocument(context, hardCapMs, startedAt);
  if (!Number.isFinite(startedAt.getTime())
      || Date.parse(parsed.deadlineAt) - startedAt.getTime() !== hardCapMs
      || `${JSON.stringify(parsed, null, 2)}\n` !== `${JSON.stringify(expected, null, 2)}\n`) {
    throw new Error(`managed session deadline conflicts with its lane identity: ${path}`);
  }
  return parsed;
}

function acquireSessionHardCapDeadline(
  context: SessionHardCapContext,
  hardCapMs: number,
  runtime: SessionHardCapWriterRuntime,
  persist: boolean,
): SessionHardCapDeadline {
  const now = validNow(runtime);
  if (!persist) {
    return {
      remainingMs: hardCapMs,
      deadlineAtMs: now.getTime() + hardCapMs,
    };
  }
  const directory = handoffStateDirectory(runtime);
  const path = resolve(directory, "deadlines", safeHandoffFilename(context.agentId));
  let document: SessionHardCapDeadlineDocument;
  try {
    document = existingSessionHardCapDeadline(path, context, hardCapMs);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    const candidate = sessionHardCapDeadlineDocument(context, hardCapMs, now);
    const serialized = `${JSON.stringify(candidate, null, 2)}\n`;
    try {
      atomicWriteOnce(path, serialized);
      document = candidate;
    } catch (writeError) {
      if ((writeError as NodeJS.ErrnoException).code !== "EEXIST") throw writeError;
      document = existingSessionHardCapDeadline(path, context, hardCapMs);
    }
  }
  const deadlineAt = Date.parse(document.deadlineAt);
  return {
    remainingMs: Math.max(0, Math.min(hardCapMs, deadlineAt - now.getTime())),
    deadlineAtMs: deadlineAt,
    path,
    serialized: `${JSON.stringify(document, null, 2)}\n`,
  };
}

function settleSessionHardCapDeadline(deadline: SessionHardCapDeadline): void {
  if (!deadline.path || !deadline.serialized) return;
  try {
    if (readFileSync(deadline.path, "utf8") !== deadline.serialized)
      throw new Error(`managed session deadline changed before settlement: ${deadline.path}`);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return;
    throw error;
  }
  removeDurably(deadline.path);
}

function handoffIdempotencyKey(document: SessionHardCapDocument): string {
  const { writtenAt: _writtenAt, ...identity } = document;
  return sha256(JSON.stringify(identity));
}

function handoffIndexRecord(
  document: SessionHardCapDocument,
  artifactPath: string,
  serializedArtifact: string,
): SessionHardCapIndexRecord {
  if (!document.threadId)
    throw new Error("session hard cap cannot terminate without an exact thread handoff target");
  const idempotencyKey = handoffIdempotencyKey(document);
  const value = [
    "session_hard_cap",
    `agent=${document.agentId}`,
    `artifact=${artifactPath}`,
    `idempotency_key=${idempotencyKey}`,
    "completion_claimed=false",
  ].join(" | ");
  return {
    version: 1,
    type: "session_hard_cap_handoff",
    idempotencyKey,
    threadId: document.threadId,
    predicate: "handoff",
    value,
    artifactPath,
    artifactSha256: sha256(serializedArtifact),
  };
}

function validateIndexRecord(value: unknown, path: string): SessionHardCapIndexRecord {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`session handoff outbox record is not an object: ${path}`);
  const record = value as Record<string, unknown>;
  const keys = [
    "artifactPath", "artifactSha256", "idempotencyKey", "predicate",
    "threadId", "type", "value", "version",
  ];
  if (Object.keys(record).sort().join("\0") !== keys.sort().join("\0")
      || record.version !== 1
      || record.type !== "session_hard_cap_handoff"
      || record.predicate !== "handoff"
      || ![record.idempotencyKey, record.threadId, record.value,
        record.artifactPath, record.artifactSha256].every(
        (field) => typeof field === "string" && field.length > 0,
      )
      || !/^[0-9a-f]{64}$/.test(record.idempotencyKey as string)
      || !/^[0-9a-f]{64}$/.test(record.artifactSha256 as string)) {
    throw new Error(`session handoff outbox record is malformed: ${path}`);
  }
  return record as unknown as SessionHardCapIndexRecord;
}

function readIndexRecord(path: string): SessionHardCapIndexRecord {
  const stat = lstatSync(path);
  if (!stat.isFile() || stat.isSymbolicLink() || stat.size > MAX_HANDOFF_RECORD_BYTES)
    throw new Error(`session handoff outbox record is unsafe: ${path}`);
  return validateIndexRecord(JSON.parse(readFileSync(path, "utf8")), path);
}

function recordPaths(directory: string, idempotencyKey: string): {
  pending: string;
  settled: string;
} {
  return {
    pending: resolve(directory, "outbox", `${idempotencyKey}.json`),
    settled: resolve(directory, "settled", `${idempotencyKey}.json`),
  };
}

function settleIndexRecord(
  paths: { pending: string; settled: string },
  serialized: string,
): void {
  atomicWriteOnce(paths.settled, serialized);
  removeDurably(paths.pending);
}

function defaultIndexHandoff(threadId: string, value: string): void {
  execFileSync(northBin(), ["tell", threadId, "handoff", value], {
    encoding: "utf8",
    timeout: 2_000,
    stdio: ["ignore", "ignore", "pipe"],
  });
}

export function writeSessionHardCapHandoff(
  context: SessionHardCapContext,
  hardCapMs = DEFAULT_MANAGED_SESSION_HARD_CAP_MS,
  runtime: SessionHardCapWriterRuntime = {},
): SessionHardCapWriteResult {
  if (!context.threadId)
    throw new Error("session hard cap cannot terminate without an exact thread handoff target");
  const directory = handoffStateDirectory(runtime);
  ensurePrivateDirectory(directory);
  const path = resolve(directory, safeHandoffFilename(context.agentId));
  const document = durableHandoffDocument(
    path,
    context,
    hardCapMs,
    (runtime.now ?? (() => new Date()))(),
  );
  const serializedArtifact = `${JSON.stringify(document, null, 2)}\n`;
  const record = handoffIndexRecord(document, path, serializedArtifact);
  const serializedRecord = `${JSON.stringify(record)}\n`;
  const paths = recordPaths(directory, record.idempotencyKey);

  try {
    const settled = readIndexRecord(paths.settled);
    if (`${JSON.stringify(settled)}\n` !== serializedRecord)
      throw new Error(`session handoff settlement conflicts with ${record.idempotencyKey}`);
    return {
      path,
      indexed: true,
      spooled: false,
      idempotencyKey: record.idempotencyKey,
    };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }

  atomicWriteOnce(paths.pending, serializedRecord);
  try {
    (runtime.indexHandoff ?? defaultIndexHandoff)(record.threadId, record.value);
    settleIndexRecord(paths, serializedRecord);
    return {
      path,
      indexed: true,
      spooled: false,
      idempotencyKey: record.idempotencyKey,
    };
  } catch (error) {
    console.error(
      `[session-cap] @agent:${context.agentId} handoff artifact written at ${path}; `
      + `thread index deferred to durable outbox: ${error instanceof Error ? error.message : String(error)}`,
    );
    return {
      path,
      indexed: false,
      spooled: true,
      idempotencyKey: record.idempotencyKey,
    };
  }
}

export function replaySessionHardCapHandoffs(
  runtime: SessionHardCapWriterRuntime = {},
): number {
  const directory = handoffStateDirectory(runtime);
  const outbox = resolve(directory, "outbox");
  let entries: string[];
  try {
    entries = readdirSync(outbox)
      .filter((entry) => /^[0-9a-f]{64}\.json$/.test(entry))
      .sort()
      .slice(0, MAX_HANDOFF_REPLAY_RECORDS);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return 0;
    throw error;
  }
  let replayed = 0;
  for (const entry of entries) {
    const pending = resolve(outbox, entry);
    const record = readIndexRecord(pending);
    const serialized = `${JSON.stringify(record)}\n`;
    const artifactStat = lstatSync(record.artifactPath);
    const artifact = readFileSync(record.artifactPath);
    if (!artifactStat.isFile() || artifactStat.isSymbolicLink()
        || artifact.length > MAX_HANDOFF_RECORD_BYTES
        || sha256(artifact) !== record.artifactSha256) {
      throw new Error(`session handoff artifact does not match outbox record: ${record.artifactPath}`);
    }
    const paths = recordPaths(directory, record.idempotencyKey);
    try {
      const settled = readIndexRecord(paths.settled);
      if (`${JSON.stringify(settled)}\n` !== serialized)
        throw new Error(`session handoff settlement conflicts with ${record.idempotencyKey}`);
      removeDurably(pending);
      continue;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    try {
      (runtime.indexHandoff ?? defaultIndexHandoff)(record.threadId, record.value);
    } catch {
      continue;
    }
    settleIndexRecord(paths, serialized);
    replayed++;
  }
  return replayed;
}

/** Bound turn-level interruption so process cleanup cannot be held hostage by a control request. */
export async function interruptWireQuery(
  query: WireQuery | undefined,
  timeoutMs = 1_000,
): Promise<void> {
  if (!query?.interrupt) return;
  const timeout = Promise.withResolvers<void>();
  const timer = setTimeout(timeout.resolve, timeoutMs);
  try {
    await Promise.race([
      Promise.resolve(query.interrupt()).catch(() => undefined),
      timeout.promise,
    ]);
  } finally {
    clearTimeout(timer);
  }
}

export class HostTerminationError extends Error {
  constructor(readonly signal: HostTerminationSignal) {
    super(`host termination requested (${signal})`);
    this.name = "HostTerminationError";
  }
}

export class SessionHardCapError extends Error {
  readonly code = "NORTH_MANAGED_SESSION_HARD_CAP";

  constructor(
    readonly hardCapMs: number,
    readonly handoffPath?: string,
  ) {
    super(`managed session reached its ${hardCapMs}ms hard cap`);
    this.name = "SessionHardCapError";
  }
}

export class ManagedRunTokenBudgetError extends Error {
  readonly code = "NORTH_MANAGED_RUN_TOKEN_BUDGET_LIMITED";

  constructor(readonly status: ManagedRunTokenBudgetStatus) {
    super(
      `managed run reached its ${status.targetTokens}-token target at `
      + `${status.observedTokens} tokens (${status.overshootTokens} overshoot)`,
    );
    this.name = "ManagedRunTokenBudgetError";
  }
}

export type HostTerminationRegistrar = (
  options: HostTerminationParticipantOptions,
) => HostTerminationParticipant;

export interface ManagedOwnedResource {
  close(): Promise<void>;
  forceClose(): void;
}

/**
 * One lifecycle from outer admission through terminal publication and all
 * outer cleanup. It exists before any awaited preflight, so the first signal
 * becomes a sticky abort even when no provider query has been constructed.
 */
export class ManagedQueryTermination {
  readonly abortController = new AbortController();
  private query: WireQuery | undefined;
  readonly #queryClosures = new WeakMap<WireQuery, Promise<void>>();
  private closeInput: (() => void) | undefined;
  private inputClosed = false;
  private closePromise: Promise<void> | undefined;
  private signalled: HostTerminationSignal | undefined;
  private readonly participant: HostTerminationParticipant;
  private readonly resources = new Set<ManagedOwnedResource>();
  private readonly hardCapOptions: ManagedSessionHardCapOptions | undefined;
  private readonly hardCapDeadline: SessionHardCapDeadline | undefined;
  private readonly now: () => Date;
  private readonly cancelHardCapTimer: (timer: unknown) => void;
  private hardCapTimer: unknown;
  private hardCapActive = false;
  private hardCapFrozen = false;
  private hardCapReached: SessionHardCapStatus | undefined;
  private hardCapError: SessionHardCapError | undefined;
  #tokenBudget: ManagedRunTokenBudgetStatus | undefined;
  private released = false;

  constructor(
    register: HostTerminationRegistrar = registerHostTerminationParticipant,
    hardCapOptions?: ManagedSessionHardCapOptions,
  ) {
    this.now = hardCapOptions?.now ?? (() => new Date());
    if (hardCapOptions) {
      const hardCapMs = hardCapOptions.hardCapMs
        ?? DEFAULT_MANAGED_SESSION_HARD_CAP_MS;
      if (!Number.isSafeInteger(hardCapMs) || hardCapMs <= 0)
        throw new Error("managed session hard cap must be a positive safe integer");
      managedRunTokenTarget(hardCapOptions.tokenTarget);
    }
    this.participant = register({
      onSignal: (signal) => {
        this.signalled = signal;
        this.abortController.abort(new HostTerminationError(signal));
        this.closeInputSafely();
      },
      close: () => this.close(),
      forceClose: () => this.forceClose(),
    });
    this.hardCapOptions = hardCapOptions;
    this.cancelHardCapTimer = hardCapOptions?.cancel
      ?? ((timer) => clearTimeout(timer as NodeJS.Timeout));
    if (hardCapOptions) {
      const hardCapMs = hardCapOptions.hardCapMs
        ?? DEFAULT_MANAGED_SESSION_HARD_CAP_MS;
      const deadline = acquireSessionHardCapDeadline(
        {
          agentId: hardCapOptions.agentId,
          goal: hardCapOptions.goal,
          repo: hardCapOptions.repo,
          ...(hardCapOptions.threadId ? { threadId: hardCapOptions.threadId } : {}),
          ...(hardCapOptions.worktree ? { worktree: hardCapOptions.worktree } : {}),
          ...(hardCapOptions.branch ? { branch: hardCapOptions.branch } : {}),
        },
        hardCapMs,
        {
          ...(hardCapOptions.stateDirectory
            ? { stateDirectory: hardCapOptions.stateDirectory } : {}),
          ...(hardCapOptions.now ? { now: hardCapOptions.now } : {}),
        },
        hardCapOptions.schedule === undefined
          || hardCapOptions.stateDirectory !== undefined,
      );
      this.hardCapDeadline = deadline;
      const schedule = hardCapOptions.schedule ?? ((callback, delayMs) => {
        const timer = setTimeout(() => { void callback(); }, delayMs);
        timer.unref();
        return timer;
      });
      this.hardCapActive = true;
      this.hardCapTimer = schedule(() => this.reachHardCap(), deadline.remainingMs);
      this.replayHandoffsSafely();
    }
  }

  get signal(): AbortSignal {
    return this.abortController.signal;
  }

  hostSignal(): HostTerminationSignal | undefined {
    return this.signalled ?? this.participant.signal();
  }

  hardCapStatus(): SessionHardCapStatus | undefined {
    return this.hardCapReached;
  }

  tokenBudgetStatus(): ManagedRunTokenBudgetStatus | undefined {
    return this.#tokenBudget;
  }

  observeCompletedCallUsage(snapshot: WireRunSnapshot): ManagedRunTokenBudgetStatus | undefined {
    const targetTokens = this.hardCapOptions?.tokenTarget;
    if (targetTokens === undefined) return undefined;
    if (this.#tokenBudget?.state === "budget_limited") return this.#tokenBudget;

    const observedTokens = tokensOf(snapshot);
    const coverage = normalizeUsage(snapshot).totalStatus;
    if (coverage !== "exact" || observedTokens === undefined) {
      this.#tokenBudget = Object.freeze({
        targetTokens,
        coverage,
        state: "unenforceable",
      });
      return this.#tokenBudget;
    }

    if (observedTokens < targetTokens) {
      this.#tokenBudget = Object.freeze({
        targetTokens,
        observedTokens,
        coverage,
        state: "within_target",
      });
      return this.#tokenBudget;
    }

    this.#tokenBudget = Object.freeze({
      targetTokens,
      observedTokens,
      overshootTokens: observedTokens - targetTokens,
      coverage,
      state: "budget_limited",
    });
    this.cancelSessionHardCap();
    this.closeInputSafely();
    return this.#tokenBudget;
  }

  continuationAllowed(): boolean {
    return this.#tokenBudget?.state !== "budget_limited";
  }

  /** Read-only gate for the one successful-empty corrective turn. */
  emptyResultRepairAllowed(): boolean {
    if (!this.continuationAllowed() || this.hardCapFrozen || this.closePromise
        || this.released || this.hostSignal()) return false;
    return this.hardCapDeadline === undefined
      || validNow({ now: this.now }).getTime() < this.hardCapDeadline.deadlineAtMs;
  }

  throwIfContinuationBlocked(): void {
    if (this.#tokenBudget?.state === "budget_limited") {
      throw new ManagedRunTokenBudgetError(this.#tokenBudget);
    }
  }

  throwIfTerminated(): void {
    this.throwIfContinuationBlocked();
    if (this.hardCapError) throw this.hardCapError;
    const signal = this.hostSignal();
    if (signal) throw new HostTerminationError(signal);
  }

  attachInput(close: () => void): void {
    this.closeInput = close;
    if (this.hardCapFrozen || !this.continuationAllowed() || this.hostSignal()) {
      this.closeInputSafely();
    }
  }

  attachQuery(query: WireQuery): void {
    if (this.closePromise || this.released || this.#queryClosures.has(query)) {
      query.forceClose?.();
      throw new Error("managed_query_attached_after_close");
    }
    if (this.query && this.query !== query) {
      query.forceClose?.();
      throw new Error("managed_query_replaced_before_close");
    }
    if (this.hardCapError) {
      query.forceClose?.();
      throw this.hardCapError;
    }
    if (!this.continuationAllowed()) {
      query.forceClose?.();
      this.throwIfContinuationBlocked();
    }
    const signalledBeforeAttach = this.hostSignal();
    if (signalledBeforeAttach) {
      query.forceClose?.();
      throw new HostTerminationError(signalledBeforeAttach);
    }
    this.query = query;
    // JavaScript cannot deliver a signal during the synchronous assignment,
    // but retain the postcondition explicitly for alternate registrars/tests.
    if (this.hostSignal()) {
      query.forceClose?.();
      throw new HostTerminationError(this.hostSignal()!);
    }
  }

  /** Close and detach one provider attempt without ending the shared managed session. */
  closeQuery(
    query: WireQuery | undefined = this.query,
    interruptTimeoutMs = 1_000,
  ): Promise<void> {
    if (!query) return Promise.resolve();
    const existing = this.#queryClosures.get(query);
    if (existing) return existing;
    if (this.query !== query) {
      return Promise.reject(new Error("managed_query_close_requires_attached_query"));
    }
    const closing = (async () => {
      try {
        await interruptWireQuery(query, interruptTimeoutMs);
        await query.close?.();
      } finally {
        if (this.query === query) this.query = undefined;
      }
    })();
    this.#queryClosures.set(query, closing);
    return closing;
  }

  attachResource(resource: ManagedOwnedResource): void {
    if (this.closePromise) {
      resource.forceClose();
      throw new Error("managed_query_resource_attached_after_close");
    }
    this.resources.add(resource);
    if (this.hardCapError || this.hostSignal()) resource.forceClose();
  }

  close(): Promise<void> {
    return this.closeWithInterruptTimeout();
  }

  private closeWithInterruptTimeout(
    interruptTimeoutMs = 1_000,
  ): Promise<void> {
    this.cancelSessionHardCap();
    return this.closePromise ??= (async () => {
      this.closeInputSafely();
      const failures: unknown[] = [];
      if (this.query) {
        try {
          await this.closeQuery(this.query, interruptTimeoutMs);
        } catch (error) {
          failures.push(error);
        }
      }
      for (const resource of this.resources) {
        try { await resource.close(); }
        catch (error) { failures.push(error); }
      }
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1)
        throw new AggregateError(failures, "managed query resources failed to close");
    })();
  }

  forceClose(): void {
    const failures: unknown[] = [];
    try { this.query?.forceClose?.(); }
    catch (error) { failures.push(error); }
    for (const resource of this.resources) {
      try { resource.forceClose(); }
      catch (error) { failures.push(error); }
    }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1)
      throw new AggregateError(failures, "managed query resources failed to force-close");
  }

  publicationSettled(): void {
    this.participant.publicationSettled();
  }

  cleanupSettled(): void {
    this.participant.cleanupSettled();
    this.replayHandoffsSafely();
  }

  release(): void {
    this.released = true;
    this.cancelSessionHardCap();
    try {
      if (!this.hardCapFrozen && this.hardCapDeadline)
        settleSessionHardCapDeadline(this.hardCapDeadline);
    } finally {
      this.participant.release();
    }
  }

  private async reachHardCap(): Promise<void> {
    if (!this.hardCapActive || this.released || this.hardCapReached
        || !this.hardCapOptions) return;
    // closeQuery detaches the attempt after graceful teardown so a retry may
    // attach its successor. Retain only this expiring attempt for the hard-cap
    // force-close fallback below; it is never made attachable again.
    const expiringQuery = this.query;
    this.hardCapActive = false;
    this.hardCapFrozen = true;
    this.closeInputSafely();
    const hardCapMs = this.hardCapOptions.hardCapMs
      ?? DEFAULT_MANAGED_SESSION_HARD_CAP_MS;
    let handoff: SessionHardCapWriteResult = {
      path: "", indexed: false, spooled: false,
    };
    try {
      const context: SessionHardCapContext = {
        agentId: this.hardCapOptions.agentId,
        goal: this.hardCapOptions.goal,
        repo: this.hardCapOptions.repo,
        ...(this.hardCapOptions.threadId
          ? { threadId: this.hardCapOptions.threadId } : {}),
        ...(this.hardCapOptions.worktree
          ? { worktree: this.hardCapOptions.worktree } : {}),
        ...(this.hardCapOptions.branch
          ? { branch: this.hardCapOptions.branch } : {}),
      };
      const document = sessionHardCapDocument(
        context,
        hardCapMs,
        validNow({ ...(this.hardCapOptions.now ? { now: this.hardCapOptions.now } : {}) }),
      );
      handoff = this.hardCapOptions.writeHandoff
        ? this.hardCapOptions.writeHandoff(document)
        : writeSessionHardCapHandoff(context, hardCapMs, {
          ...(this.hardCapOptions.stateDirectory
            ? { stateDirectory: this.hardCapOptions.stateDirectory } : {}),
          ...(this.hardCapOptions.now ? { now: this.hardCapOptions.now } : {}),
        });
    } catch (error) {
      console.error(
        `[session-cap] @agent:${this.hardCapOptions.agentId} terminal handoff write failed: `
        + (error instanceof Error ? error.message : String(error)),
      );
    }
    if (!handoff.path || (!handoff.indexed && !handoff.spooled)) {
      console.error(
        `[session-cap] @agent:${this.hardCapOptions.agentId} provider retained: `
        + "no durable handoff index or outbox record exists",
      );
      return;
    }
    this.hardCapError = new SessionHardCapError(hardCapMs, handoff.path || undefined);
    this.abortController.abort(this.hardCapError);
    try { await this.closeWithInterruptTimeout(HARD_CAP_QUERY_INTERRUPT_TIMEOUT_MS); }
    catch (error) {
      console.error(
        `[session-cap] @agent:${this.hardCapOptions.agentId} graceful teardown reported: `
        + (error instanceof Error ? error.message : String(error)),
      );
    }
    try {
      expiringQuery?.forceClose?.();
      this.forceClose();
    }
    catch (error) {
      console.error(
        `[session-cap] @agent:${this.hardCapOptions.agentId} force-close reported: `
        + (error instanceof Error ? error.message : String(error)),
      );
    }
    this.hardCapReached = {
      hardCapMs,
      handoffPath: handoff.path,
      indexed: handoff.indexed,
      spooled: handoff.spooled ?? false,
    };
  }

  private replayHandoffsSafely(): void {
    if (!this.hardCapOptions) return;
    const replay = this.hardCapOptions.replayHandoffs
      ?? (this.hardCapOptions.writeHandoff ? undefined : replaySessionHardCapHandoffs);
    if (!replay) return;
    try { replay(); }
    catch (error) {
      console.error(
        `[session-cap] pending handoff replay unavailable: `
        + (error instanceof Error ? error.message : String(error)),
      );
    }
  }

  private cancelSessionHardCap(): void {
    this.hardCapActive = false;
    if (this.hardCapTimer === undefined) return;
    try { this.cancelHardCapTimer(this.hardCapTimer); }
    finally { this.hardCapTimer = undefined; }
  }

  private closeInputSafely(): void {
    if (this.inputClosed || !this.closeInput) return;
    this.inputClosed = true;
    try { this.closeInput(); }
    catch { /* idempotent input close is best-effort */ }
  }
}
