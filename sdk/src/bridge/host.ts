import { randomUUID } from "node:crypto";
import { chmodSync, existsSync, lstatSync, mkdirSync, unlinkSync } from "node:fs";
import { connect, createServer, type Server, type Socket } from "node:net";
import { dirname, join } from "node:path";
import { ExecutionJournal, type JournalRecord, type JournalScan } from "./journal";
import { codexBridgeProvider, type BridgeProviderExecution } from "./provider";
import {
  bridgeJournalRoot, bridgeSocketPath, parseBridgeRequest, type BridgeRequest,
} from "./protocol";

const MAX_REQUEST_BYTES = 1024 * 1024;
const TERMINAL_KINDS = new Set(["execution.completed", "execution.failed"]);

type BridgeMessage =
  | { type: "launched"; executionId: string }
  | { type: "event"; record: JournalRecord }
  | { type: "barrier"; executionId: string; cursor: number; tornTail?: JournalScan["tornTail"] }
  | { type: "error"; message: string };

export interface NorthdOptions {
  socketPath?: string;
  journalRoot?: string;
  provider?: BridgeProviderExecution;
}

interface ExecutionRuntime {
  executionId: string;
  journal: ExecutionJournal;
  subscribers: Set<Socket>;
  abort: AbortController;
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
  private readonly server: Server;
  private readonly runtimes = new Map<string, ExecutionRuntime>();
  private readonly sockets = new Set<Socket>();
  private readonly drives = new Set<Promise<void>>();
  private activeExecutionId?: string;

  constructor(options: NorthdOptions = {}) {
    this.socketPath = options.socketPath ?? bridgeSocketPath();
    this.journalRoot = options.journalRoot ?? bridgeJournalRoot();
    this.provider = options.provider ?? codexBridgeProvider;
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
    for (const runtime of this.runtimes.values()) runtime.abort.abort();
    for (const socket of this.sockets) socket.destroy();
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
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
    const runtime = {
      executionId, journal, subscribers: new Set<Socket>(), abort: new AbortController(), terminal,
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

  private async drive(runtime: ExecutionRuntime, prompt: string, cwd: string): Promise<void> {
    try {
      this.append(runtime, "provider.starting", { adapter: "codex-app-server" });
      await this.provider.execute(
        { executionId: runtime.executionId, prompt, cwd, signal: runtime.abort.signal },
        (event) => this.append(runtime, `provider.${event.kind}`, event.data),
      );
      this.append(runtime, "execution.completed", {});
    } catch (error) {
      this.append(runtime, "execution.failed", { message: errorMessage(error) });
    } finally {
      runtime.terminal = true;
      if (this.activeExecutionId === runtime.executionId) this.activeExecutionId = undefined;
      for (const subscriber of runtime.subscribers) subscriber.end();
      runtime.subscribers.clear();
    }
  }

  private launch(socket: Socket, request: Extract<BridgeRequest, { op: "launch" }>): void {
    if (this.activeExecutionId) throw new Error(`bridge execution ${this.activeExecutionId} is still active`);
    const executionId = randomUUID();
    const runtime: ExecutionRuntime = {
      executionId,
      journal: new ExecutionJournal(this.journalRoot, executionId),
      subscribers: new Set(),
      abort: new AbortController(),
      terminal: false,
    };
    this.runtimes.set(executionId, runtime);
    this.activeExecutionId = executionId;
    this.append(runtime, "execution.accepted", { prompt: request.prompt, cwd: request.cwd });
    wire(socket, { type: "launched", executionId });
    this.attach(socket, runtime, 0);
    const drive = this.drive(runtime, request.prompt, request.cwd);
    this.drives.add(drive);
    void drive.then(
      () => this.drives.delete(drive),
      () => this.drives.delete(drive),
    );
  }

  private dispatch(socket: Socket, request: BridgeRequest): void {
    if (request.op === "launch") this.launch(socket, request);
    else this.attach(socket, this.runtime(request.executionId), request.cursor);
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
      try {
        this.dispatch(socket, parseBridgeRequest(JSON.parse(buffer.slice(0, newline))));
      } catch (error) {
        wire(socket, { type: "error", message: errorMessage(error) });
        socket.end();
      }
    });
  }
}
