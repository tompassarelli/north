import { randomUUID } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, unlinkSync } from "node:fs";
import { createServer, Socket, type Server } from "node:net";
import { dirname, join } from "node:path";
import {
  BridgeWireJournal,
  ExecutionJournal,
  readBridgeWireJournal,
  type JournalRecord,
} from "./journal";
import {
  BridgeProviderTeardownTimeoutError,
  bridgeProvider,
  selectBridgeProvider,
  type BridgeProviderExecution,
  type BridgeProviderSession,
} from "./provider";
import {
  ProviderEscalationUnsupportedError,
  ProviderRuntimeError,
} from "../providers/types";
import {
  bridgeJournalRoot,
  bridgeSocketPath,
  bridgeSourceIdentity,
  parseBridgeLaunchRole,
  parseBridgeRequest,
  type BridgeLaunchProvider,
  type BridgeLaunchRole,
  type BridgeRequest,
  type BridgeServerMessage,
} from "./protocol";
import {
  encodeWireJsonlLine,
  isIntermediateProviderSessionReplacement,
  isProviderNeutralWireErrorCode,
  WireEventWriter,
  wireRunId,
  type WireAbortEvidence,
  type WireEvent,
  type WireTerminalLifecycle,
  type WireTerminationReason,
} from "../wire";

const MAX_REQUEST_BYTES = 1024 * 1024;
const STALE_POLL_MS = 15_000;
const PROVIDER_TEARDOWN_TIMEOUT_MS = 2_000;

class HostProviderTeardownTimeoutError extends Error {
  constructor() {
    super("provider session teardown timed out");
    this.name = "HostProviderTeardownTimeoutError";
  }
}

class BridgeProviderTurnControlError extends Error {
  constructor(cause: unknown) {
    super("bridge provider turn control failed", { cause });
    this.name = "BridgeProviderTurnControlError";
  }
}

export interface NorthdOptions {
  socketPath?: string;
  journalRoot?: string;
  provider?: BridgeProviderExecution;
  /** Test injection. Production selects by entitlement headroom. */
  selectProvider?: () => Promise<BridgeLaunchProvider>;
  /** Test injection. Production reads this checkout's HEAD. */
  sourceIdentity?: () => string | undefined;
  stalePollMs?: number;
  /** Test injection. Production bounds provider teardown to two seconds. */
  providerTeardownTimeoutMs?: number;
  /** Test injection for control-journal persistence failures. */
  controlJournal?: (root: string, executionId: string) => ExecutionJournal;
  /** Invoked once when the daemon is stale and idle; owns process teardown. */
  onRetire?: () => void;
}

interface QueuedInput {
  input: string;
  delivery: "queued-next-turn" | "interrupt-and-redirect";
  commandSeq: number;
}

type TurnDisposition = "completed" | "interrupted";

interface ExecutionRuntime {
  executionId: string;
  role: BridgeLaunchRole;
  journal: ExecutionJournal;
  wireJournal?: BridgeWireJournal;
  writer?: WireEventWriter;
  wireEvents: WireEvent[];
  wireTail: Promise<void>;
  subscribers: Set<Socket>;
  abort: AbortController;
  pendingInputs: QueuedInput[];
  session?: BridgeProviderSession;
  activeTurn: boolean;
  pendingInterrupt: boolean;
  turnDisposition: TurnDisposition;
  terminating: boolean;
  terminal: boolean;
  replayOnly: boolean;
  live: boolean;
  finishing?: Promise<void>;
  teardown?: Promise<void>;
  teardownFailureRecorded: boolean;
}

interface WireIdleProjection {
  disposition: TurnDisposition;
  pendingInputs: number;
  wireCursor: number;
}

interface WirePersistence {
  events: WireEvent[];
  idle: WireIdleProjection[];
}

function send(socket: Socket, message: BridgeServerMessage): void {
  if (socket.destroyed || socket.writableEnded || !socket.writable) return;
  try { socket.write(`${JSON.stringify(message)}\n`); }
  catch { socket.destroy(); }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function providerFailureClassification(error: unknown): string {
  if (error instanceof ProviderEscalationUnsupportedError
    && isProviderNeutralWireErrorCode(error.code)) {
    return error.code;
  }
  if (error instanceof ProviderRuntimeError) return "provider_runtime_failure";
  return "provider_failure";
}

function teardownFailureClassification(error: unknown): string {
  return error instanceof HostProviderTeardownTimeoutError
    || error instanceof BridgeProviderTeardownTimeoutError
    ? "provider_teardown_timeout"
    : "provider_teardown_failed";
}

function adapterFailureEvidence(runtime: ExecutionRuntime): Record<string, unknown> | undefined {
  const terminal = [...(runtime.writer?.events() ?? [])].reverse().find(
    (event) => event.kind === "model-call.completed"
      && event.status !== "succeeded"
      && !isIntermediateProviderSessionReplacement(event),
  );
  if (!terminal || terminal.kind !== "model-call.completed") return undefined;
  const detail = terminal.evidence?.failure?.detail;
  return {
    status: terminal.status,
    origin: terminal.origin,
    ...(terminal.errorCode === undefined ? {} : { errorCode: terminal.errorCode }),
    ...(detail === undefined ? {} : { detail }),
  };
}

function failureData(
  runtime: ExecutionRuntime,
  code: "provider_error" | "provider_process_died" | "provider_teardown_failed",
  classification: string,
): Record<string, unknown> {
  const evidence = adapterFailureEvidence(runtime);
  return {
    code,
    classification,
    ...(evidence === undefined ? {} : { evidence }),
  };
}

function liveSocket(path: string): Promise<boolean> {
  const result = Promise.withResolvers<boolean>();
  const socket = new Socket();
  socket.once("connect", () => { socket.destroy(); result.resolve(true); });
  socket.once("error", (error: NodeJS.ErrnoException) => {
    socket.destroy();
    if (error.code === "ECONNREFUSED" || error.code === "ENOENT") result.resolve(false);
    else result.reject(error);
  });
  socket.connect(path);
  return result.promise;
}

function serverClosed(server: Server): Promise<void> {
  const result = Promise.withResolvers<void>();
  server.close((error) => error ? result.reject(error) : result.resolve());
  return result.promise;
}

function wireTerminal(event: WireEvent | undefined): boolean {
  return event?.kind === "run.terminated";
}

function hasOpenWireLifecycle(runtime: ExecutionRuntime): boolean {
  const snapshot = runtime.writer?.snapshot();
  if (!snapshot) return false;
  return Object.values(snapshot.modelCalls).some((modelCall) => modelCall.status === "running")
    || Object.values(snapshot.messages).some((message) => message.stage !== "completed")
    || Object.values(snapshot.toolCalls).some((tool) => tool.status === "pending");
}

export class Northd {
  readonly socketPath: string;
  readonly journalRoot: string;
  #provider: BridgeProviderExecution;
  #selectProvider: () => Promise<BridgeLaunchProvider>;
  #server: Server;
  #runtimes = new Map<string, ExecutionRuntime>();
  #runtimeLoads = new Map<string, Promise<ExecutionRuntime>>();
  #sockets = new Set<Socket>();
  #drives = new Set<Promise<void>>();
  #sourceIdentity: () => string | undefined;
  #stalePollMs: number;
  #providerTeardownTimeoutMs: number;
  #controlJournal: (root: string, executionId: string) => ExecutionJournal;
  #onRetire: () => void;
  #loadedIdentity?: string;
  #staleTimer?: NodeJS.Timeout;
  #retiring = false;
  #closePromise?: Promise<void>;

  constructor(options: NorthdOptions = {}) {
    this.socketPath = options.socketPath ?? bridgeSocketPath();
    this.journalRoot = options.journalRoot ?? bridgeJournalRoot();
    this.#provider = options.provider ?? bridgeProvider;
    this.#selectProvider = options.selectProvider ?? selectBridgeProvider;
    this.#sourceIdentity = options.sourceIdentity ?? bridgeSourceIdentity;
    this.#stalePollMs = options.stalePollMs ?? STALE_POLL_MS;
    this.#providerTeardownTimeoutMs = options.providerTeardownTimeoutMs
      ?? PROVIDER_TEARDOWN_TIMEOUT_MS;
    this.#controlJournal = options.controlJournal
      ?? ((root, executionId) => new ExecutionJournal(root, executionId));
    this.#onRetire = options.onRetire ?? (() => { void this.close(); });
    this.#server = createServer((socket) => this.#accept(socket));
  }

  async listen(): Promise<void> {
    mkdirSync(dirname(this.socketPath), { recursive: true, mode: 0o700 });
    mkdirSync(this.journalRoot, { recursive: true, mode: 0o700 });
    chmodSync(dirname(this.socketPath), 0o700);
    chmodSync(this.journalRoot, 0o700);
    if (existsSync(this.socketPath)) {
      const info = lstatSync(this.socketPath);
      if (!info.isSocket()) throw new Error(`refusing to replace non-socket ${this.socketPath}`);
      if (await liveSocket(this.socketPath)) {
        throw new Error(`northd is already listening at ${this.socketPath}`);
      }
      unlinkSync(this.socketPath);
    }
    this.#loadedIdentity = this.#sourceIdentity();
    const listening = Promise.withResolvers<void>();
    const onError = (error: Error) => {
      this.#server.off("listening", onListening);
      listening.reject(error);
    };
    const onListening = () => {
      this.#server.off("error", onError);
      listening.resolve();
    };
    this.#server.once("error", onError);
    this.#server.once("listening", onListening);
    this.#server.listen(this.socketPath);
    await listening.promise;
    chmodSync(this.socketPath, 0o600);
    if (this.#loadedIdentity !== undefined) {
      this.#staleTimer = setInterval(() => this.#retireWhenStale(), this.#stalePollMs);
    }
  }

  #stale(): boolean {
    if (this.#loadedIdentity === undefined) return false;
    const disk = this.#sourceIdentity();
    return disk !== undefined && disk !== this.#loadedIdentity;
  }

  #liveExecutions(except?: ExecutionRuntime): number {
    let live = 0;
    for (const runtime of this.#runtimes.values()) {
      if (runtime !== except && runtime.live) live += 1;
    }
    return live;
  }

  #pinning(runtime: ExecutionRuntime): boolean {
    if (!runtime.live) return false;
    return !(runtime.role === "director" && runtime.subscribers.size === 0);
  }

  #pinningExecutions(except?: ExecutionRuntime): number {
    let pinning = 0;
    for (const runtime of this.#runtimes.values()) {
      if (runtime !== except && this.#pinning(runtime)) pinning += 1;
    }
    return pinning;
  }

  #retireWhenStale(): void {
    if (this.#retiring || this.#pinningExecutions() > 0 || !this.#stale()) return;
    this.#retiring = true;
    this.#onRetire();
  }

  close(): Promise<void> {
    if (this.#closePromise) return this.#closePromise;
    this.#closePromise = this.#close();
    return this.#closePromise;
  }

  async #close(): Promise<void> {
    if (this.#staleTimer !== undefined) clearInterval(this.#staleTimer);
    this.#staleTimer = undefined;
    const terminations: Promise<void>[] = [];
    for (const runtime of this.#runtimes.values()) {
      if (runtime.terminal || runtime.replayOnly) continue;
      runtime.terminating = true;
      runtime.abort.abort();
      terminations.push((async () => {
        let teardownFailure: unknown;
        let evidenceFailure: unknown;
        let finishFailure: unknown;
        try {
          await this.#teardown(runtime);
        } catch (error) {
          teardownFailure = error;
          try { this.#recordTeardownFailure(runtime, error); }
          catch (failure) { evidenceFailure = failure; }
        } finally {
          try {
            await this.#finish(runtime, teardownFailure === undefined ? {
              lifecycle: "cancelled",
              reason: { code: "aborted" },
              abort: {
                requestedAt: new Date().toISOString(),
                source: "runtime",
                reason: "bridge daemon closed",
              },
            } : {
              lifecycle: "failed",
              reason: { code: "provider_error" },
            });
          } catch (error) {
            finishFailure = error;
          }
        }
        const failures = [teardownFailure, evidenceFailure, finishFailure]
          .filter((failure) => failure !== undefined);
        if (failures.length === 1) throw failures[0];
        if (failures.length > 1) throw new AggregateError(failures);
      })());
    }
    for (const socket of this.#sockets) socket.destroy();
    if (this.#server.listening) await serverClosed(this.#server);
    await Promise.allSettled(terminations);
    for (const runtime of this.#runtimes.values()) {
      runtime.journal.close();
      await runtime.wireJournal?.close().catch(() => {});
    }
    if (existsSync(this.socketPath) && lstatSync(this.socketPath).isSocket()) {
      unlinkSync(this.socketPath);
    }
  }

  async #runtime(executionId: string): Promise<ExecutionRuntime> {
    const existing = this.#runtimes.get(executionId);
    if (existing) return existing;
    const pending = this.#runtimeLoads.get(executionId);
    if (pending) return pending;
    const loading = this.#loadRuntime(executionId);
    this.#runtimeLoads.set(executionId, loading);
    try { return await loading; }
    finally {
      if (this.#runtimeLoads.get(executionId) === loading) {
        this.#runtimeLoads.delete(executionId);
      }
    }
  }

  async #loadRuntime(executionId: string): Promise<ExecutionRuntime> {
    const journalPath = join(this.journalRoot, executionId, "events.log");
    if (!existsSync(journalPath)) throw new Error(`unknown bridge execution ${executionId}`);
    const journal = this.#controlJournal(this.journalRoot, executionId);
    let wireJournal: BridgeWireJournal | undefined;
    try {
      const controls = journal.scan();
      const accepted = controls.records.find((record) => record.kind === "execution.accepted");
      const role = parseBridgeLaunchRole(
        typeof accepted?.data.role === "string" ? accepted.data.role : undefined,
      );
      const replay = await readBridgeWireJournal(this.journalRoot, executionId);
      let events = [...replay.events];
      if (wireTerminal(events.at(-1))) {
        const runtime: ExecutionRuntime = {
          executionId,
          role,
          journal,
          wireEvents: events,
          wireTail: Promise.resolve(),
          subscribers: new Set<Socket>(),
          abort: new AbortController(),
          pendingInputs: [],
          activeTurn: false,
          pendingInterrupt: false,
          turnDisposition: "completed",
          terminating: false,
          terminal: true,
          replayOnly: true,
          live: false,
          teardownFailureRecorded: false,
        };
        this.#runtimes.set(executionId, runtime);
        return runtime;
      }

      // The append-only JSONL writer lock is the recovery serialization point.
      // Re-read while holding it so a concurrent live writer either wins first
      // or makes this restart fail closed without a duplicate terminal suffix.
      wireJournal = await BridgeWireJournal.open(this.journalRoot, executionId);
      events = [...wireJournal.replay().events];
      if (wireTerminal(events.at(-1))) {
        await wireJournal.close();
        wireJournal = undefined;
        const runtime: ExecutionRuntime = {
          executionId,
          role,
          journal,
          wireEvents: events,
          wireTail: Promise.resolve(),
          subscribers: new Set<Socket>(),
          abort: new AbortController(),
          pendingInputs: [],
          activeTurn: false,
          pendingInterrupt: false,
          turnDisposition: "completed",
          terminating: false,
          terminal: true,
          replayOnly: true,
          live: false,
          teardownFailureRecorded: false,
        };
        this.#runtimes.set(executionId, runtime);
        return runtime;
      }
      const writer = WireEventWriter.restore(events);
      if (writer.runId !== wireRunId(`bridge:${executionId}`)) {
        throw new Error("bridge wire replay belongs to another run");
      }
      const runtime: ExecutionRuntime = {
        executionId,
        role,
        journal,
        wireJournal,
        writer,
        wireEvents: events,
        wireTail: Promise.resolve(),
        subscribers: new Set<Socket>(),
        abort: new AbortController(),
        pendingInputs: [],
        activeTurn: false,
        pendingInterrupt: false,
        turnDisposition: "completed",
        terminating: true,
        terminal: false,
        replayOnly: true,
        live: false,
        teardownFailureRecorded: false,
      };
      await this.#finish(runtime, {
        lifecycle: "failed",
        reason: { code: "provider_process_died" },
      });
      this.#runtimes.set(executionId, runtime);
      return runtime;
    } catch (error) {
      journal.close();
      await wireJournal?.close().catch(() => {});
      throw error;
    }
  }

  #appendControl(
    runtime: ExecutionRuntime,
    kind: string,
    data: Record<string, unknown>,
  ): JournalRecord {
    const record = runtime.journal.append(kind, data);
    for (const subscriber of runtime.subscribers) send(subscriber, { type: "event", record });
    return record;
  }

  #restoreWriterFromDurablePrefix(runtime: ExecutionRuntime): void {
    const writer = WireEventWriter.restore(runtime.wireEvents);
    if (writer.runId !== wireRunId(`bridge:${runtime.executionId}`)) {
      throw new Error("bridge wire replay belongs to another run");
    }
    runtime.writer = writer;
  }

  #persistWire(
    runtime: ExecutionRuntime,
    event: WireEvent,
    allowTerminal = false,
  ): Promise<WirePersistence> {
    const append = runtime.wireTail.then(async () => {
      if (!runtime.wireJournal || !runtime.writer) {
        throw new Error("bridge wire writer is unavailable");
      }
      if (runtime.terminal) throw new Error("bridge run is already terminal");
      let encoded: string;
      try { encoded = encodeWireJsonlLine(event); }
      catch (error) {
        this.#restoreWriterFromDurablePrefix(runtime);
        throw error;
      }
      const owned = runtime.writer.events()[event.sequence];
      if (!owned || encodeWireJsonlLine(owned) !== encoded) {
        this.#restoreWriterFromDurablePrefix(runtime);
        throw new Error("bridge provider event did not exactly match the shared writer");
      }
      if (event.kind === "run.terminated" && !allowTerminal) {
        this.#restoreWriterFromDurablePrefix(runtime);
        throw new Error("provider adapter attempted to terminate a Bridge run");
      }
      const persistedEvents: WireEvent[] = [];
      const idle: WireIdleProjection[] = [];
      if (event.sequence < runtime.wireEvents.length) {
        const prior = runtime.wireEvents[event.sequence];
        if (!prior || encodeWireJsonlLine(prior) !== encoded) {
          this.#restoreWriterFromDurablePrefix(runtime);
          throw new Error("bridge wire replay diverged from its writer");
        }
        return { events: persistedEvents, idle };
      }
      while (runtime.wireEvents.length <= event.sequence) {
        const next = runtime.writer.events()[runtime.wireEvents.length];
        if (!next) throw new Error("bridge wire writer has a sequence gap");
        const persisted = await runtime.wireJournal.append(next);
        if (encodeWireJsonlLine(persisted) !== encodeWireJsonlLine(next)) {
          throw new Error("bridge wire journal changed a canonical event");
        }
        runtime.wireEvents.push(persisted);
        persistedEvents.push(persisted);
        if (persisted.kind !== "model-call.completed"
          || allowTerminal
          || isIntermediateProviderSessionReplacement(persisted)) continue;
        const disposition = runtime.turnDisposition;
        runtime.activeTurn = false;
        runtime.turnDisposition = "completed";
        idle.push({
          disposition,
          pendingInputs: runtime.pendingInputs.length,
          wireCursor: persisted.sequence + 1,
        });
      }
      return { events: persistedEvents, idle };
    });
    runtime.wireTail = append.then(() => {}, () => {});
    return append;
  }

  #projectWire(runtime: ExecutionRuntime, persistence: WirePersistence): void {
    for (const event of persistence.events) {
      for (const subscriber of runtime.subscribers) {
        send(subscriber, { type: "wire", event });
      }
    }
    for (const idle of persistence.idle) {
      this.#appendControl(runtime, "session.idle", { armed: true, ...idle });
      void this.#dispatchNext(runtime);
    }
  }

  async #appendWire(
    runtime: ExecutionRuntime,
    event: WireEvent,
    allowTerminal = false,
  ): Promise<void> {
    this.#projectWire(runtime, await this.#persistWire(runtime, event, allowTerminal));
  }

  #attach(socket: Socket, runtime: ExecutionRuntime, cursor: number): void {
    const controls = runtime.journal.scan();
    for (const record of controls.records) send(socket, { type: "event", record });
    // Bridge cursors are one-based counts. Wire sequence is zero-based, so a
    // cursor N resumes with event.sequence >= N and commits as sequence + 1.
    for (const event of runtime.wireEvents) {
      if (event.sequence >= cursor) send(socket, { type: "wire", event });
    }
    const committedCursor = runtime.wireEvents.length;
    send(socket, {
      type: "barrier",
      executionId: runtime.executionId,
      cursor: committedCursor,
      ...(controls.tornTail ? { tornTail: controls.tornTail } : {}),
    });
    if (runtime.terminal || runtime.replayOnly || controls.tornTail) {
      socket.end();
      return;
    }
    runtime.subscribers.add(socket);
    socket.once("close", () => runtime.subscribers.delete(socket));
  }

  #finish(
    runtime: ExecutionRuntime,
    input: {
      lifecycle: WireTerminalLifecycle;
      reason: WireTerminationReason;
      abort?: WireAbortEvidence;
    },
  ): Promise<void> {
    if (runtime.finishing) return runtime.finishing;
    if (runtime.terminal) return Promise.resolve();
    runtime.finishing = (async () => {
      if (!runtime.writer) throw new Error("bridge run writer is unavailable");
      const terminalEvents = runtime.writer.terminate(input);
      const persistence: WirePersistence = { events: [], idle: [] };
      for (const event of terminalEvents) {
        const appended = await this.#persistWire(runtime, event, true);
        persistence.events.push(...appended.events);
      }
      runtime.terminal = true;
      runtime.live = false;
      runtime.activeTurn = false;
      runtime.turnDisposition = "completed";
      const failures: unknown[] = [];
      try {
        try { this.#projectWire(runtime, persistence); }
        catch (error) { failures.push(error); }
        try {
          this.#appendControl(runtime, "execution.terminated", {
            lifecycle: input.lifecycle,
            reason: input.reason.code,
            wireCursor: runtime.wireEvents.length,
          });
        } catch (error) { failures.push(error); }
      } finally {
        for (const subscriber of runtime.subscribers) {
          try { subscriber.end(); }
          catch { subscriber.destroy(); }
        }
        runtime.subscribers.clear();
      }
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1) throw new AggregateError(failures);
    })();
    return runtime.finishing;
  }

  #recordTeardownFailure(runtime: ExecutionRuntime, error: unknown): void {
    if (runtime.teardownFailureRecorded) return;
    runtime.teardownFailureRecorded = true;
    this.#appendControl(runtime, "execution.failure", {
      ...failureData(
        runtime,
        "provider_teardown_failed",
        teardownFailureClassification(error),
      ),
      phase: "provider_teardown",
    });
  }

  #teardown(runtime: ExecutionRuntime): Promise<void> {
    if (runtime.teardown) return runtime.teardown;
    const session = runtime.session;
    if (!session) return Promise.resolve();
    runtime.teardown = (async () => {
      const timeout = Promise.withResolvers<never>();
      const timer = setTimeout(
        () => timeout.reject(new HostProviderTeardownTimeoutError()),
        this.#providerTeardownTimeoutMs,
      );
      try {
        await Promise.race([
          Promise.resolve().then(() => session.terminateSession()),
          timeout.promise,
        ]);
      } catch (error) {
        if (error instanceof HostProviderTeardownTimeoutError) {
          try { session.forceTerminateSession?.(); }
          catch (forceError) { throw new AggregateError([error, forceError]); }
        }
        throw error;
      } finally {
        clearTimeout(timer);
      }
    })();
    return runtime.teardown;
  }

  async #fail(
    runtime: ExecutionRuntime,
    error: unknown,
    code: "provider_error" | "provider_process_died" = "provider_error",
  ): Promise<void> {
    if (runtime.terminal) return;
    runtime.live = false;
    this.#restoreWriterFromDurablePrefix(runtime);
    const classification = code === "provider_process_died"
      ? "provider_process_died"
      : providerFailureClassification(error);
    const failures: unknown[] = [];
    try {
      this.#appendControl(
        runtime,
        "execution.failure",
        failureData(runtime, code, classification),
      );
    } catch (failure) { failures.push(failure); }
    try {
      await this.#finish(runtime, {
        lifecycle: "failed",
        reason: { code },
      });
    } catch (failure) { failures.push(failure); }
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures);
  }

  async #dispatchNext(runtime: ExecutionRuntime): Promise<void> {
    if (runtime.terminal || runtime.terminating || runtime.activeTurn || !runtime.session) return;
    const next = runtime.pendingInputs.shift();
    if (!next) return;
    runtime.activeTurn = true;
    runtime.turnDisposition = "completed";
    try {
      await runtime.session.submitInput(next.input);
      this.#appendControl(runtime, "control.input_delivered", {
        commandSeq: next.commandSeq,
        delivery: next.delivery,
      });
    } catch (error) {
      await this.#fail(runtime, error);
    }
  }

  async #drive(
    runtime: ExecutionRuntime,
    prompt: string,
    cwd: string,
    role: Extract<BridgeRequest, { op: "launch" }>["role"],
    pinned: BridgeLaunchProvider | undefined,
  ): Promise<void> {
    try {
      const provider = pinned ?? await this.#selectProvider();
      const session = await this.#provider.open({
        executionId: runtime.executionId,
        prompt,
        cwd,
        role,
        provider,
        signal: runtime.abort.signal,
        writer: runtime.writer!,
      });
      runtime.session = session;
      if (runtime.terminating || runtime.terminal) {
        try { await this.#teardown(runtime); }
        catch (error) {
          if (!runtime.terminal) {
            this.#recordTeardownFailure(runtime, error);
            await this.#finish(runtime, {
              lifecycle: "failed",
              reason: { code: "provider_error" },
            });
          }
        }
        return;
      }
      if (runtime.pendingInterrupt) {
        runtime.pendingInterrupt = false;
        try { await session.interruptTurn(); }
        catch (error) {
          if (runtime.turnDisposition === "interrupted") {
            runtime.turnDisposition = "completed";
          }
          throw new BridgeProviderTurnControlError(error);
        }
      }
      for await (const event of session.events()) {
        if (event.version !== "north:wire:v2" || event.essential !== true) {
          throw new Error("Bridge provider emitted a noncanonical wire event");
        }
        await this.#appendWire(runtime, event);
      }
      if (!runtime.terminating && !runtime.terminal) {
        await this.#fail(runtime, new Error("provider session closed before termination"), "provider_process_died");
      }
    } catch (error) {
      if (!runtime.terminating) await this.#fail(runtime, error);
    } finally {
      if (runtime.terminal || runtime.terminating) runtime.live = false;
      runtime.activeTurn = false;
    }
  }

  async #launch(
    socket: Socket,
    request: Extract<BridgeRequest, { op: "launch" }>,
  ): Promise<void> {
    const executionId = request.executionId ?? randomUUID();
    if (this.#runtimes.has(executionId)
      || this.#runtimeLoads.has(executionId)
      || existsSync(join(this.journalRoot, executionId))) {
      throw new Error(`bridge execution ${executionId} already exists`);
    }
    const admission = Promise.withResolvers<ExecutionRuntime>();
    void admission.promise.catch(() => {});
    this.#runtimeLoads.set(executionId, admission.promise);
    try {
      const journal = this.#controlJournal(this.journalRoot, executionId);
      const wireJournal = await BridgeWireJournal.open(this.journalRoot, executionId);
      const writer = new WireEventWriter({ runId: wireRunId(`bridge:${executionId}`) });
      const started = writer.append({
        kind: "run.started",
        lifecycle: "running",
        owner: `bridge:${request.role}`,
      });
      const persisted = await wireJournal.append(started);
      const runtime: ExecutionRuntime = {
        executionId,
        role: request.role,
        journal,
        wireJournal,
        writer,
        wireEvents: [persisted],
        wireTail: Promise.resolve(),
        subscribers: new Set(),
        abort: new AbortController(),
        pendingInputs: [],
        activeTurn: true,
        pendingInterrupt: false,
        turnDisposition: "completed",
        terminating: false,
        terminal: false,
        replayOnly: false,
        live: true,
        teardownFailureRecorded: false,
      };
      try {
        this.#appendControl(runtime, "execution.accepted", {
          prompt: request.prompt,
          cwd: request.cwd,
          role: request.role,
          wireCursor: runtime.wireEvents.length,
        });
      } catch (error) {
        runtime.live = false;
        runtime.activeTurn = false;
        runtime.terminating = true;
        let terminalFailure: unknown;
        try {
          await this.#finish(runtime, {
            lifecycle: "failed",
            reason: { code: "provider_error" },
          });
        } catch (failure) {
          terminalFailure = failure;
        }
        let controlCloseFailure: unknown;
        let wireCloseFailure: unknown;
        try { journal.close(); }
        catch (failure) { controlCloseFailure = failure; }
        try { await wireJournal.close(); }
        catch (failure) { wireCloseFailure = failure; }
        const failures = [error, terminalFailure, controlCloseFailure, wireCloseFailure]
          .filter((failure) => failure !== undefined);
        if (failures.length > 1) {
          throw new AggregateError(
            failures,
            "bridge launch control acceptance and terminal persistence failed",
          );
        }
        throw error;
      }
      this.#runtimes.set(executionId, runtime);
      admission.resolve(runtime);
      send(socket, { type: "launched", executionId });
      this.#attach(socket, runtime, 0);
      if (this.#stale()) {
        this.#appendControl(runtime, "execution.failure", {
          message: "bridge_daemon_source_stale",
          loaded: this.#loadedIdentity,
          disk: this.#sourceIdentity(),
          live: this.#pinningExecutions(runtime),
        });
        await this.#finish(runtime, {
          lifecycle: "failed",
          reason: { code: "provider_error" },
        });
        return;
      }
      const drive = this.#drive(
        runtime, request.prompt, request.cwd, request.role, request.provider,
      );
      this.#drives.add(drive);
      void drive
        .catch((error) => send(socket, { type: "error", message: errorMessage(error) }))
        .finally(() => this.#drives.delete(drive));
    } catch (error) {
      admission.reject(error);
      throw error;
    } finally {
      if (this.#runtimeLoads.get(executionId) === admission.promise) {
        this.#runtimeLoads.delete(executionId);
      }
    }
  }

  async #control(
    socket: Socket,
    runtime: ExecutionRuntime,
    request: Exclude<BridgeRequest, { op: "launch" | "attach" }>,
  ): Promise<void> {
    if (runtime.terminal || runtime.replayOnly) {
      throw new Error(`bridge execution ${runtime.executionId} is terminal`);
    }
    if (request.op === "submitInput") {
      const command = this.#appendControl(runtime, "control.submit_input", {
        input: request.input,
        delivery: "queued-next-turn",
        wireCursor: runtime.wireEvents.length,
      });
      runtime.pendingInputs.push({
        input: request.input,
        delivery: "queued-next-turn",
        commandSeq: command.seq,
      });
      await this.#dispatchNext(runtime);
      send(socket, {
        type: "controlled",
        executionId: runtime.executionId,
        control: request.op,
        delivery: "queued-next-turn",
      });
    } else if (request.op === "interruptTurn") {
      this.#appendControl(runtime, "control.interrupt_turn", {
        delivery: "active-turn",
        wireCursor: runtime.wireEvents.length,
      });
      if (!runtime.activeTurn) {
        throw new Error(`bridge execution ${runtime.executionId} has no active turn`);
      }
      runtime.turnDisposition = "interrupted";
      if (runtime.session) {
        try { await runtime.session.interruptTurn(); }
        catch (error) {
          if (!runtime.terminal && runtime.turnDisposition === "interrupted") {
            runtime.turnDisposition = "completed";
          }
          throw new BridgeProviderTurnControlError(error);
        }
      } else {
        runtime.pendingInterrupt = true;
      }
      send(socket, {
        type: "controlled",
        executionId: runtime.executionId,
        control: request.op,
        delivery: "active-turn",
      });
    } else if (request.op === "redirectNow") {
      const command = this.#appendControl(runtime, "control.redirect_now", {
        input: request.input,
        delivery: "interrupt-and-redirect",
        wireCursor: runtime.wireEvents.length,
      });
      runtime.pendingInputs.unshift({
        input: request.input,
        delivery: "interrupt-and-redirect",
        commandSeq: command.seq,
      });
      if (runtime.activeTurn) {
        if (!runtime.session) {
          runtime.pendingInputs.shift();
          throw new Error(`bridge execution ${runtime.executionId} provider is still starting`);
        }
        runtime.turnDisposition = "interrupted";
        try { await runtime.session.interruptTurn(); }
        catch (error) {
          runtime.pendingInputs = runtime.pendingInputs
            .filter((pending) => pending.commandSeq !== command.seq);
          if (!runtime.terminal && runtime.turnDisposition === "interrupted") {
            runtime.turnDisposition = "completed";
          }
          throw new BridgeProviderTurnControlError(error);
        }
      } else {
        await this.#dispatchNext(runtime);
      }
      send(socket, {
        type: "controlled",
        executionId: runtime.executionId,
        control: request.op,
        delivery: "interrupt-and-redirect",
      });
    } else {
      this.#appendControl(runtime, "control.terminate_session", {});
      const wasActive = runtime.activeTurn;
      runtime.terminating = true;
      runtime.abort.abort();
      let closeFailure: unknown;
      let evidenceFailure: unknown;
      let finishFailure: unknown;
      try {
        await this.#teardown(runtime);
      }
      catch (error) {
        closeFailure = error;
        try { this.#recordTeardownFailure(runtime, error); }
        catch (failure) { evidenceFailure = failure; }
      } finally {
        const hasOpenWork = hasOpenWireLifecycle(runtime);
        try {
          await this.#finish(runtime, closeFailure !== undefined ? {
            lifecycle: "failed",
            reason: { code: "provider_error" },
          } : wasActive || hasOpenWork ? {
            lifecycle: "cancelled",
            reason: { code: "cancelled" },
            abort: {
              requestedAt: new Date().toISOString(),
              source: "operator",
              reason: "Bridge session terminated",
            },
          } : {
            lifecycle: "completed",
            reason: { code: "completed" },
          });
        } catch (error) {
          finishFailure = error;
        }
      }
      if (closeFailure !== undefined || evidenceFailure !== undefined || finishFailure !== undefined) {
        if (finishFailure !== undefined) throw new Error("bridge terminal persistence failed");
        if (evidenceFailure !== undefined) {
          throw new Error("bridge failure evidence persistence failed");
        }
        throw new Error(teardownFailureClassification(closeFailure) === "provider_teardown_timeout"
          ? "provider session teardown timed out"
          : "provider session teardown failed");
      }
      send(socket, {
        type: "controlled",
        executionId: runtime.executionId,
        control: request.op,
        delivery: "session-terminated",
      });
    }
    socket.end();
  }

  async #dispatch(socket: Socket, request: BridgeRequest): Promise<void> {
    if (request.op === "retire") {
      send(socket, {
        type: "controlled",
        executionId: "northd",
        control: "retire",
        delivery: "accepted",
      });
      socket.end();
      if (this.#retiring) return;
      this.#retiring = true;
      this.#onRetire();
      return;
    }
    if (request.op === "launch") await this.#launch(socket, request);
    else if (request.op === "attach") {
      this.#attach(socket, await this.#runtime(request.executionId), request.cursor);
    } else {
      await this.#control(socket, await this.#runtime(request.executionId), request);
    }
  }

  #accept(socket: Socket): void {
    this.#sockets.add(socket);
    socket.once("close", () => this.#sockets.delete(socket));
    socket.on("error", () => socket.destroy());
    send(socket, {
      type: "hello",
      ...(this.#loadedIdentity !== undefined ? { identity: this.#loadedIdentity } : {}),
      liveExecutions: this.#liveExecutions(),
      pinningExecutions: this.#pinningExecutions(),
      pid: process.pid,
    });
    let buffer = "";
    let handled = false;
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      if (handled) return;
      buffer += chunk;
      if (Buffer.byteLength(buffer, "utf8") > MAX_REQUEST_BYTES) {
        handled = true;
        send(socket, { type: "error", message: "bridge request is too large" });
        socket.end();
        return;
      }
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      handled = true;
      let request: BridgeRequest;
      try { request = parseBridgeRequest(JSON.parse(buffer.slice(0, newline))); }
      catch (error) {
        send(socket, { type: "error", message: errorMessage(error) });
        socket.end();
        return;
      }
      void this.#dispatch(socket, request).catch((error) => {
        send(socket, { type: "error", message: errorMessage(error) });
        socket.end();
      });
    });
  }
}
