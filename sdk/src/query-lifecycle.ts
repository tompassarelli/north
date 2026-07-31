import { createHash, randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  linkSync, mkdirSync, readFileSync, unlinkSync, writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
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
}

export interface SessionHardCapStatus {
  hardCapMs: number;
  handoffPath?: string;
  indexed: boolean;
}

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
  const document = sessionHardCapDocument(
    context,
    hardCapMs,
    (runtime.now ?? (() => new Date()))(),
  );
  const directory = runtime.stateDirectory
    ?? resolve(homedir(), ".local/state/north/session-handoffs");
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const path = resolve(directory, safeHandoffFilename(context.agentId));
  const serialized = `${JSON.stringify(document, null, 2)}\n`;
  const temporary = `${path}.${process.pid}.${randomUUID()}.tmp`;
  try {
    writeFileSync(temporary, serialized, {
      encoding: "utf8", flag: "wx", mode: 0o600,
    });
    linkSync(temporary, path);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "EEXIST"
        || readFileSync(path, "utf8") !== serialized) {
      throw error;
    }
  } finally {
    try { unlinkSync(temporary); }
    catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
  }

  let indexed = false;
  if (context.threadId) {
    const value = [
      "session_hard_cap",
      `agent=${context.agentId}`,
      `artifact=${path}`,
      "completion_claimed=false",
    ].join(" | ");
    try {
      (runtime.indexHandoff ?? defaultIndexHandoff)(context.threadId, value);
      indexed = true;
    } catch (error) {
      console.error(
        `[session-cap] @agent:${context.agentId} handoff artifact written at ${path}; `
        + `thread index unavailable: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  return { path, indexed };
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
    if (this.hardCapError || this.hostSignal()) this.closeInputSafely();
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
    const hardCapMs = this.hardCapOptions.hardCapMs
      ?? DEFAULT_MANAGED_SESSION_HARD_CAP_MS;
    let handoff: SessionHardCapWriteResult = { path: "", indexed: false };
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
    this.hardCapReached = {
      hardCapMs,
      ...(handoff.path ? { handoffPath: handoff.path } : {}),
      indexed: handoff.indexed,
    };
    this.hardCapError = new SessionHardCapError(hardCapMs, handoff.path || undefined);
    this.abortController.abort(this.hardCapError);
    this.closeInputSafely();
    try { this.forceClose(); }
    catch (error) {
      console.error(
        `[session-cap] @agent:${this.hardCapOptions.agentId} force-close reported: `
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
