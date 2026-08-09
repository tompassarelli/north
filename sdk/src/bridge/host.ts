import { randomUUID } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, unlinkSync } from "node:fs";
import { createServer, Socket, type Server } from "node:net";
import { dirname, join, resolve } from "node:path";
import { ExecutionJournal, type JournalRecord, type JournalScan } from "./journal";
import {
  bridgeProvider, selectBridgeProvider, type BridgeProviderExecution,
  type BridgeProviderSession, type NormalizedProviderEvent,
} from "./provider";
import {
  bridgeJournalRoot, bridgeSocketPath, bridgeSourceIdentity, parseBridgeRequest,
  type BridgeLaunchProvider, type BridgeRequest,
} from "./protocol";

const MAX_REQUEST_BYTES = 1024 * 1024;
const TERMINAL_KINDS = new Set(["execution.completed", "execution.failed"]);
const STALE_POLL_MS = 15_000;

type BridgeMessage =
  | { type: "hello"; identity?: string; liveExecutions: number; pid: number }
  | { type: "launched"; executionId: string }
  | { type: "controlled"; executionId: string; control: string; delivery: string }
  | { type: "event"; record: JournalRecord }
  | { type: "barrier"; executionId: string; cursor: number; tornTail?: JournalScan["tornTail"] }
  | { type: "error"; message: string };

export interface NorthdOptions {
  socketPath?: string;
  journalRoot?: string;
  provider?: BridgeProviderExecution;
  providerAdapter?: string;
  /** Test injection. Production selects by entitlement headroom. */
  selectProvider?: () => Promise<BridgeLaunchProvider>;
  /** Test injection. Production reads this checkout's HEAD. */
  sourceIdentity?: () => string | undefined;
  stalePollMs?: number;
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
  journal: ExecutionJournal;
  subscribers: Set<Socket>;
  abort: AbortController;
  pendingInputs: QueuedInput[];
  session?: BridgeProviderSession;
  activeTurn: boolean;
  turnDisposition: TurnDisposition;
  terminating: boolean;
  terminal: boolean;
  /**
   * The one fact retirement turns on: this daemon owns a provider drive that
   * has not settled. Derived liveness — a session handle, an active turn, a
   * missing terminal record — all outlive the provider in some path, and every
   * one of those paths pinned a corpse daemon against replacement. This is set
   * exactly once when the drive is dispatched and cleared exactly once when it
   * settles, success or failure.
   */
  live: boolean;
}

function wire(socket: Socket, message: BridgeMessage): void {
  if (!socket.destroyed) socket.write(`${JSON.stringify(message)}\n`);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Provider preaccept codes are deliberately opaque; without the chain the journal names a stage, not a defect. */
function failureData(error: unknown): Record<string, unknown> {
  const causes: string[] = [];
  let cause: unknown = error instanceof Error ? error.cause : undefined;
  while (cause !== undefined && causes.length < 8) {
    causes.push(errorMessage(cause));
    cause = cause instanceof Error ? cause.cause : undefined;
  }
  return { message: errorMessage(error), ...(causes.length ? { causes } : {}) };
}

function liveSocket(path: string): Promise<boolean> {
  return new Promise((resolve, reject) => {
    // Listeners first, then connect: probing an abandoned socket can fail
    // during the connect call itself, and the answer to "is anyone there" must
    // arrive as this promise, never as an uncaught error event.
    const socket = new Socket();
    socket.once("connect", () => { socket.destroy(); resolve(true); });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      socket.destroy();
      if (error.code === "ECONNREFUSED" || error.code === "ENOENT") resolve(false);
      else reject(error);
    });
    socket.connect(path);
  });
}

export class Northd {
  readonly socketPath: string;
  readonly journalRoot: string;
  private readonly provider: BridgeProviderExecution;
  private readonly providerAdapter: string;
  private readonly selectProvider: () => Promise<BridgeLaunchProvider>;
  private readonly server: Server;
  private readonly runtimes = new Map<string, ExecutionRuntime>();
  private readonly sockets = new Set<Socket>();
  private readonly drives = new Set<Promise<void>>();
  private readonly sourceIdentity: () => string | undefined;
  private readonly stalePollMs: number;
  private readonly onRetire: () => void;
  private loadedIdentity?: string;
  private staleTimer?: ReturnType<typeof setInterval>;
  private retiring = false;

  constructor(options: NorthdOptions = {}) {
    this.socketPath = options.socketPath ?? bridgeSocketPath();
    this.journalRoot = options.journalRoot ?? bridgeJournalRoot();
    this.provider = options.provider ?? bridgeProvider;
    this.providerAdapter = options.providerAdapter ?? "codex-app-server";
    this.selectProvider = options.selectProvider ?? selectBridgeProvider;
    this.sourceIdentity = options.sourceIdentity ?? bridgeSourceIdentity;
    this.stalePollMs = options.stalePollMs ?? STALE_POLL_MS;
    this.onRetire = options.onRetire ?? (() => { void this.close(); });
    this.server = createServer((socket) => this.accept(socket));
  }

  async listen(): Promise<void> {
    mkdirSync(dirname(this.socketPath), { recursive: true, mode: 0o700 });
    mkdirSync(this.journalRoot, { recursive: true, mode: 0o700 });
    chmodSync(dirname(this.socketPath), 0o700);
    chmodSync(this.journalRoot, 0o700);
    if (existsSync(this.socketPath)) {
      const info = lstatSync(this.socketPath);
      if (!info.isSocket()) throw new Error(`refusing to replace non-socket ${this.socketPath}`);
      if (await liveSocket(this.socketPath)) throw new Error(`northd is already listening at ${this.socketPath}`);
      unlinkSync(this.socketPath);
    }
    await new Promise<void>((resolve, reject) => {
      const onError = (error: Error) => { this.server.off("listening", onListening); reject(error); };
      const onListening = () => { this.server.off("error", onError); resolve(); };
      this.server.once("error", onError);
      this.server.once("listening", onListening);
      this.server.listen(this.socketPath);
    });
    chmodSync(this.socketPath, 0o600);
    this.loadedIdentity = this.sourceIdentity();
    if (this.loadedIdentity !== undefined)
      this.staleTimer = setInterval(() => this.retireWhenStale(), this.stalePollMs);
  }

  /** Stale means the checkout moved under a live daemon; unknown identity never retires. */
  private stale(): boolean {
    if (this.loadedIdentity === undefined) return false;
    const disk = this.sourceIdentity();
    return disk !== undefined && disk !== this.loadedIdentity;
  }

  /**
   * `except` is the runtime asking the question. A refused launch counts itself
   * as live, so reporting the sessions that actually hold retirement open means
   * excluding the asker.
   */
  private liveExecutions(except?: ExecutionRuntime): number {
    let live = 0;
    for (const runtime of this.runtimes.values()) {
      if (runtime === except) continue;
      if (runtime.live) live += 1;
    }
    return live;
  }

  private retireWhenStale(): void {
    if (this.retiring || this.liveExecutions() > 0 || !this.stale()) return;
    this.retiring = true;
    this.onRetire();
  }

  async close(): Promise<void> {
    if (this.staleTimer !== undefined) clearInterval(this.staleTimer);
    this.staleTimer = undefined;
    const terminations: Promise<void>[] = [];
    for (const runtime of this.runtimes.values()) {
      runtime.terminating = true;
      runtime.live = false;
      runtime.abort.abort();
      if (runtime.session) terminations.push(runtime.session.terminateSession());
    }
    for (const socket of this.sockets) socket.destroy();
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
    await Promise.allSettled(terminations);
    await Promise.allSettled([...this.drives]);
    for (const runtime of this.runtimes.values()) runtime.journal.close();
    if (existsSync(this.socketPath) && lstatSync(this.socketPath).isSocket()) unlinkSync(this.socketPath);
  }

  private runtime(executionId: string): ExecutionRuntime {
    const existing = this.runtimes.get(executionId);
    if (existing) return existing;
    const journalPath = join(this.journalRoot, executionId, "events.log");
    if (!existsSync(journalPath)) throw new Error(`unknown bridge execution ${executionId}`);
    const journal = new ExecutionJournal(this.journalRoot, executionId);
    const scan = journal.scan();
    const terminal = scan.records.some((record) => TERMINAL_KINDS.has(record.kind));
    const runtime: ExecutionRuntime = {
      executionId,
      journal,
      subscribers: new Set<Socket>(),
      abort: new AbortController(),
      pendingInputs: [],
      activeTurn: false,
      turnDisposition: "completed",
      terminating: false,
      terminal,
      // Replayed from the journal: this daemon drives nothing for it, whatever
      // the log's last record says.
      live: false,
    };
    this.runtimes.set(executionId, runtime);
    return runtime;
  }

  private append(runtime: ExecutionRuntime, kind: string, data: Record<string, unknown>): JournalRecord {
    const record = runtime.journal.append(kind, data);
    for (const subscriber of runtime.subscribers) wire(subscriber, { type: "event", record });
    return record;
  }

  private attach(socket: Socket, runtime: ExecutionRuntime, cursor: number): void {
    const scan = runtime.journal.scan();
    for (const record of scan.records) {
      if (record.seq > cursor) wire(socket, { type: "event", record });
    }
    const committedCursor = scan.records.at(-1)?.seq ?? 0;
    wire(socket, {
      type: "barrier", executionId: runtime.executionId, cursor: committedCursor,
      ...(scan.tornTail ? { tornTail: scan.tornTail } : {}),
    });
    if (runtime.terminal || scan.tornTail) {
      socket.end();
      return;
    }
    runtime.subscribers.add(socket);
    socket.once("close", () => runtime.subscribers.delete(socket));
  }

  private finish(runtime: ExecutionRuntime, kind: "execution.completed" | "execution.failed", data: Record<string, unknown>): void {
    if (runtime.terminal) return;
    this.append(runtime, kind, data);
    runtime.terminal = true;
    runtime.live = false;
    runtime.activeTurn = false;
    runtime.turnDisposition = "completed";
    for (const subscriber of runtime.subscribers) subscriber.end();
    runtime.subscribers.clear();
  }

  private async dispatchNext(runtime: ExecutionRuntime): Promise<void> {
    if (runtime.terminal || runtime.terminating || runtime.activeTurn || !runtime.session) return;
    const next = runtime.pendingInputs.shift();
    if (!next) return;
    runtime.activeTurn = true;
    runtime.turnDisposition = "completed";
    try {
      await runtime.session.submitInput(next.input);
      this.append(runtime, "control.input_delivered", {
        commandSeq: next.commandSeq,
        delivery: next.delivery,
      });
    } catch (error) {
      this.finish(runtime, "execution.failed", failureData(error));
    }
  }

  private providerEvent(runtime: ExecutionRuntime, event: NormalizedProviderEvent): void {
    this.append(runtime, `provider.${event.kind}`, event.data);
    if (!event.turnTerminal || runtime.terminal) return;
    const disposition = runtime.turnDisposition;
    runtime.activeTurn = false;
    runtime.turnDisposition = "completed";
    this.append(runtime, "session.idle", {
      armed: true,
      disposition,
      pendingInputs: runtime.pendingInputs.length,
    });
    void this.dispatchNext(runtime);
  }

  private async drive(
    runtime: ExecutionRuntime,
    prompt: string,
    cwd: string,
    role: Extract<BridgeRequest, { op: "launch" }>["role"],
    pinned: BridgeLaunchProvider | undefined,
  ): Promise<void> {
    try {
      const provider = pinned ?? await this.selectProvider();
      this.append(runtime, "provider.starting", {
        adapter: this.providerAdapter,
        provider,
        selection: pinned ? "pinned" : "headroom",
      });
      const session = await this.provider.open({
        executionId: runtime.executionId,
        prompt,
        cwd,
        role,
        provider,
        signal: runtime.abort.signal,
      });
      runtime.session = session;
      if (runtime.terminating || runtime.terminal) {
        await session.terminateSession();
        return;
      }
      for await (const event of session.events()) this.providerEvent(runtime, event);
      if (!runtime.terminating && !runtime.terminal)
        this.finish(runtime, "execution.failed", { message: "provider session closed before termination" });
    } catch (error) {
      if (!runtime.terminating)
        this.finish(runtime, "execution.failed", failureData(error));
    } finally {
      // The provider query has terminated — completed, failed, refused before
      // it ever opened, or torn down under a terminating runtime. Every one of
      // those is an execution that no longer holds this daemon open, including
      // the paths above that deliberately write no terminal record.
      runtime.live = false;
      runtime.activeTurn = false;
    }
  }

  private launch(socket: Socket, request: Extract<BridgeRequest, { op: "launch" }>): void {
    const executionId = randomUUID();
    const runtime: ExecutionRuntime = {
      executionId,
      journal: new ExecutionJournal(this.journalRoot, executionId),
      subscribers: new Set(),
      abort: new AbortController(),
      pendingInputs: [],
      activeTurn: true,
      turnDisposition: "completed",
      terminating: false,
      terminal: false,
      live: true,
    };
    this.runtimes.set(executionId, runtime);
    try {
      this.append(runtime, "execution.accepted", {
        prompt: request.prompt, cwd: request.cwd, role: request.role,
        ...(request.provider ? { provider: request.provider } : {}),
      });
    } catch (error) {
      // A launch that cannot even record its acceptance drives nothing. It was
      // already in the runtime map when the journal refused, and leaving it
      // there counted a session that never existed against retirement.
      runtime.live = false;
      runtime.activeTurn = false;
      runtime.terminal = true;
      throw error;
    }
    wire(socket, { type: "launched", executionId });
    this.attach(socket, runtime, 0);
    if (this.stale()) {
      this.finish(runtime, "execution.failed", {
        message: "bridge_daemon_source_stale",
        loaded: this.loadedIdentity,
        disk: this.sourceIdentity(),
        // The sessions still holding this daemon open — what a client needs to
        // say something more useful than the code.
        live: this.liveExecutions(runtime),
      });
      return;
    }
    const drive = this.drive(
      runtime, request.prompt, request.cwd, request.role, request.provider,
    );
    this.drives.add(drive);
    void drive
      // The drive writes its own terminal record; reaching here means that
      // write itself failed, so the client's socket is the last place left to
      // say so. Dropping it silently left an unhandled rejection behind.
      .catch((error) => wire(socket, { type: "error", message: errorMessage(error) }))
      .finally(() => this.drives.delete(drive));
  }

  private async control(
    socket: Socket,
    runtime: ExecutionRuntime,
    request: Exclude<BridgeRequest, { op: "launch" | "attach" }>,
  ): Promise<void> {
    if (runtime.terminal) throw new Error(`bridge execution ${runtime.executionId} is terminal`);
    if (request.op === "submitInput") {
      const command = this.append(runtime, "control.submit_input", {
        input: request.input,
        delivery: "queued-next-turn",
      });
      runtime.pendingInputs.push({
        input: request.input,
        delivery: "queued-next-turn",
        commandSeq: command.seq,
      });
      await this.dispatchNext(runtime);
      wire(socket, {
        type: "controlled", executionId: runtime.executionId,
        control: request.op, delivery: "queued-next-turn",
      });
    } else if (request.op === "interruptTurn") {
      this.append(runtime, "control.interrupt_turn", { delivery: "active-turn" });
      if (!runtime.activeTurn || !runtime.session)
        throw new Error(`bridge execution ${runtime.executionId} has no active turn`);
      runtime.turnDisposition = "interrupted";
      try { await runtime.session.interruptTurn(); }
      catch (error) {
        if (!runtime.terminal && runtime.turnDisposition === "interrupted")
          runtime.turnDisposition = "completed";
        throw error;
      }
      wire(socket, {
        type: "controlled", executionId: runtime.executionId,
        control: request.op, delivery: "active-turn",
      });
    } else if (request.op === "redirectNow") {
      const command = this.append(runtime, "control.redirect_now", {
        input: request.input,
        delivery: "interrupt-and-redirect",
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
          if (!runtime.terminal && runtime.turnDisposition === "interrupted")
            runtime.turnDisposition = "completed";
          throw error;
        }
      } else {
        await this.dispatchNext(runtime);
      }
      wire(socket, {
        type: "controlled", executionId: runtime.executionId,
        control: request.op, delivery: "interrupt-and-redirect",
      });
    } else {
      this.append(runtime, "control.terminate_session", {});
      runtime.terminating = true;
      runtime.abort.abort();
      await runtime.session?.terminateSession();
      this.finish(runtime, "execution.completed", {});
      wire(socket, {
        type: "controlled", executionId: runtime.executionId,
        control: request.op, delivery: "session-terminated",
      });
    }
    socket.end();
  }

  private async dispatch(socket: Socket, request: BridgeRequest): Promise<void> {
    if (request.op === "retire") {
      wire(socket, {
        type: "controlled", executionId: "northd", control: "retire", delivery: "accepted",
      });
      socket.end();
      // `north bridge restart` is allowed to race the watchdog and itself; the
      // acknowledgement is owed either way, the teardown runs once.
      if (this.retiring) return;
      this.retiring = true;
      this.onRetire();
      return;
    }
    if (request.op === "launch") this.launch(socket, request);
    else if (request.op === "attach") this.attach(socket, this.runtime(request.executionId), request.cursor);
    else await this.control(socket, this.runtime(request.executionId), request);
  }

  private accept(socket: Socket): void {
    this.sockets.add(socket);
    socket.once("close", () => this.sockets.delete(socket));
    // The freshness contract: every client learns who it reached before asking
    // anything, so a stale daemon can be detected and replaced at connect.
    wire(socket, {
      type: "hello",
      ...(this.loadedIdentity !== undefined ? { identity: this.loadedIdentity } : {}),
      liveExecutions: this.liveExecutions(),
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
        wire(socket, { type: "error", message: "bridge request is too large" });
        socket.end();
        return;
      }
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      handled = true;
      let request: BridgeRequest;
      try { request = parseBridgeRequest(JSON.parse(buffer.slice(0, newline))); }
      catch (error) {
        wire(socket, { type: "error", message: errorMessage(error) });
        socket.end();
        return;
      }
      void this.dispatch(socket, request).catch((error) => {
        wire(socket, { type: "error", message: errorMessage(error) });
        socket.end();
      });
    });
  }
}
