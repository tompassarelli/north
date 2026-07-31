import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  chmodSync, closeSync, fsyncSync, linkSync, lstatSync, mkdirSync, openSync,
  readdirSync, readFileSync, unlinkSync, writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import type { AgentQuery } from "./providers/types";
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
  schedule?: (callback: () => void, delayMs: number) => unknown;
  cancel?: (timer: unknown) => void;
  writeHandoff?: (document: SessionHardCapDocument) => SessionHardCapWriteResult;
  replayHandoffs?: () => number;
}

export interface SessionHardCapStatus {
  hardCapMs: number;
  handoffPath?: string;
  indexed: boolean;
  spooled: boolean;
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

const MAX_HANDOFF_REPLAY_RECORDS = 8;
const MAX_HANDOFF_RECORD_BYTES = 64 * 1024;

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
export async function interruptAgentQuery(
  query: AgentQuery | undefined,
  timeoutMs = 1_000,
): Promise<void> {
  if (!query?.interrupt) return;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      Promise.resolve(query.interrupt()).catch(() => undefined),
      new Promise<void>((resolve) => { timer = setTimeout(resolve, timeoutMs); }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
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
  private query: AgentQuery | undefined;
  private closeInput: (() => void) | undefined;
  private closePromise: Promise<void> | undefined;
  private signalled: HostTerminationSignal | undefined;
  private readonly participant: HostTerminationParticipant;
  private readonly resources = new Set<ManagedOwnedResource>();
  private readonly hardCapOptions: ManagedSessionHardCapOptions | undefined;
  private readonly cancelHardCapTimer: (timer: unknown) => void;
  private hardCapTimer: unknown;
  private hardCapActive = false;
  private hardCapFrozen = false;
  private hardCapReached: SessionHardCapStatus | undefined;
  private hardCapError: SessionHardCapError | undefined;
  private released = false;

  constructor(
    register: HostTerminationRegistrar = registerHostTerminationParticipant,
    hardCapOptions?: ManagedSessionHardCapOptions,
  ) {
    if (hardCapOptions) {
      const hardCapMs = hardCapOptions.hardCapMs
        ?? DEFAULT_MANAGED_SESSION_HARD_CAP_MS;
      if (!Number.isSafeInteger(hardCapMs) || hardCapMs <= 0)
        throw new Error("managed session hard cap must be a positive safe integer");
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
      ?? ((timer) => clearTimeout(timer as ReturnType<typeof setTimeout>));
    if (hardCapOptions) {
      const hardCapMs = hardCapOptions.hardCapMs
        ?? DEFAULT_MANAGED_SESSION_HARD_CAP_MS;
      const schedule = hardCapOptions.schedule ?? ((callback, delayMs) => {
        const timer = setTimeout(callback, delayMs);
        timer.unref();
        return timer;
      });
      this.hardCapActive = true;
      this.hardCapTimer = schedule(() => this.reachHardCap(), hardCapMs);
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

  throwIfTerminated(): void {
    if (this.hardCapError) throw this.hardCapError;
    const signal = this.hostSignal();
    if (signal) throw new HostTerminationError(signal);
  }

  attachInput(close: () => void): void {
    this.closeInput = close;
    if (this.hardCapFrozen || this.hostSignal()) this.closeInputSafely();
  }

  attachQuery(query: AgentQuery): void {
    if (this.hardCapError) {
      query.forceClose?.();
      throw this.hardCapError;
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

  attachResource(resource: ManagedOwnedResource): void {
    if (this.closePromise) {
      resource.forceClose();
      throw new Error("managed_query_resource_attached_after_close");
    }
    this.resources.add(resource);
    if (this.hardCapError || this.hostSignal()) resource.forceClose();
  }

  close(): Promise<void> {
    this.cancelSessionHardCap();
    return this.closePromise ??= (async () => {
      this.closeInputSafely();
      const failures: unknown[] = [];
      if (this.query) {
        try {
          await interruptAgentQuery(this.query);
          await this.query.close?.();
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
    this.participant.release();
  }

  private reachHardCap(): void {
    if (!this.hardCapActive || this.released || this.hardCapReached
        || !this.hardCapOptions) return;
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
      const document = sessionHardCapDocument(context, hardCapMs, new Date());
      handoff = this.hardCapOptions.writeHandoff
        ? this.hardCapOptions.writeHandoff(document)
        : writeSessionHardCapHandoff(context, hardCapMs);
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
    this.hardCapReached = {
      hardCapMs,
      handoffPath: handoff.path,
      indexed: handoff.indexed,
      spooled: handoff.spooled ?? false,
    };
    this.hardCapError = new SessionHardCapError(hardCapMs, handoff.path || undefined);
    this.abortController.abort(this.hardCapError);
    try { this.forceClose(); }
    catch (error) {
      console.error(
        `[session-cap] @agent:${this.hardCapOptions.agentId} force-close reported: `
        + (error instanceof Error ? error.message : String(error)),
      );
    }
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
    try { this.closeInput?.(); }
    catch { /* idempotent input close is best-effort */ }
  }
}
