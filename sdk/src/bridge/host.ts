import { randomUUID } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, unlinkSync } from "node:fs";
import { connect, createServer, type Server, type Socket } from "node:net";
import { dirname, join } from "node:path";
import { ExecutionJournal, type JournalRecord, type JournalScan } from "./journal";
import {
  codexBridgeProvider, type BridgeProviderExecution, type BridgeProviderSession,
  type NormalizedProviderEvent,
} from "./provider";
import {
  bridgeJournalRoot, bridgeSocketPath, parseBridgeRequest, type BridgeRequest,
} from "./protocol";

const MAX_REQUEST_BYTES = 1024 * 1024;
const TERMINAL_KINDS = new Set(["execution.completed", "execution.failed"]);

type BridgeMessage =
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
}

interface QueuedInput {
  input: string;
  delivery: "queued-next-turn" | "interrupt-and-redirect";
  commandSeq: number;
}

interface ExecutionRuntime {
  executionId: string;
  journal: ExecutionJournal;
  subscribers: Set<Socket>;
  abort: AbortController;
  pendingInputs: QueuedInput[];
  session?: BridgeProviderSession;
  activeTurn: boolean;
  terminating: boolean;
  terminal: boolean;
}

function wire(socket: Socket, message: BridgeMessage): void {
  if (!socket.destroyed) socket.write(`${JSON.stringify(message)}\n`);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function liveSocket(path: string): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const socket = connect(path);
    socket.once("connect", () => { socket.destroy(); resolve(true); });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      socket.destroy();
      if (error.code === "ECONNREFUSED" || error.code === "ENOENT") resolve(false);
      else reject(error);
    });
  });
}

export class Northd {
  readonly socketPath: string;
  readonly journalRoot: string;
  private readonly provider: BridgeProviderExecution;
  private readonly providerAdapter: string;
  private readonly server: Server;
  private readonly runtimes = new Map<string, ExecutionRuntime>();
  private readonly sockets = new Set<Socket>();
  private readonly drives = new Set<Promise<void>>();

  constructor(options: NorthdOptions = {}) {
    this.socketPath = options.socketPath ?? bridgeSocketPath();
    this.journalRoot = options.journalRoot ?? bridgeJournalRoot();
    this.provider = options.provider ?? codexBridgeProvider;
    this.providerAdapter = options.providerAdapter ?? "codex-app-server";
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
  }

  async close(): Promise<void> {
    const terminations: Promise<void>[] = [];
    for (const runtime of this.runtimes.values()) {
      runtime.terminating = true;
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
      terminating: false,
      terminal,
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
    runtime.activeTurn = false;
    for (const subscriber of runtime.subscribers) subscriber.end();
    runtime.subscribers.clear();
  }

  private async dispatchNext(runtime: ExecutionRuntime): Promise<void> {
    if (runtime.terminal || runtime.terminating || runtime.activeTurn || !runtime.session) return;
    const next = runtime.pendingInputs.shift();
    if (!next) return;
    runtime.activeTurn = true;
    try {
      await runtime.session.submitInput(next.input);
      this.append(runtime, "control.input_delivered", {
        commandSeq: next.commandSeq,
        delivery: next.delivery,
      });
    } catch (error) {
      this.finish(runtime, "execution.failed", { message: errorMessage(error) });
    }
  }

  private providerEvent(runtime: ExecutionRuntime, event: NormalizedProviderEvent): void {
    this.append(runtime, `provider.${event.kind}`, event.data);
    if (!event.turnTerminal || runtime.terminal) return;
    runtime.activeTurn = false;
    this.append(runtime, "session.idle", {
      armed: true,
      pendingInputs: runtime.pendingInputs.length,
    });
    void this.dispatchNext(runtime);
  }

  private async drive(runtime: ExecutionRuntime, prompt: string, cwd: string): Promise<void> {
    try {
      this.append(runtime, "provider.starting", { adapter: this.providerAdapter });
      const session = await this.provider.open({
        executionId: runtime.executionId,
        prompt,
        cwd,
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
        this.finish(runtime, "execution.failed", { message: errorMessage(error) });
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
      terminating: false,
      terminal: false,
    };
    this.runtimes.set(executionId, runtime);
    this.append(runtime, "execution.accepted", { prompt: request.prompt, cwd: request.cwd });
    wire(socket, { type: "launched", executionId });
    this.attach(socket, runtime, 0);
    const drive = this.drive(runtime, request.prompt, request.cwd);
    this.drives.add(drive);
    void drive.finally(() => this.drives.delete(drive));
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
      await runtime.session.interruptTurn();
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
        try { await runtime.session.interruptTurn(); }
        catch (error) {
          runtime.pendingInputs = runtime.pendingInputs
            .filter((pending) => pending.commandSeq !== command.seq);
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
    if (request.op === "launch") this.launch(socket, request);
    else if (request.op === "attach") this.attach(socket, this.runtime(request.executionId), request.cursor);
    else await this.control(socket, this.runtime(request.executionId), request);
  }

  private accept(socket: Socket): void {
    this.sockets.add(socket);
    socket.once("close", () => this.sockets.delete(socket));
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
