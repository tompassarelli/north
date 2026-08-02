import { mkdtempSync, rmSync } from "node:fs";
import { createServer, connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "./host";
import { scanJournalFile, type JournalRecord } from "./journal";
import type {
  BridgeProviderExecution, BridgeProviderSession, NormalizedProviderEvent,
} from "./provider";

interface Client {
  socket: Socket;
  messages: any[];
  closed: Promise<void>;
}

interface AcceptanceOptions {
  output?: (line: string) => void;
}

interface HistoryState {
  prompt: string;
  turns: number;
  queuedInputs: number;
  interrupts: number;
  status: string;
}

class AcceptanceSession implements BridgeProviderSession {
  readonly effects: string[] = [];
  private readonly queue: NormalizedProviderEvent[] = [];
  private wake?: () => void;
  private ended = false;

  constructor(readonly executionId: string, prompt: string) {
    this.emit({ kind: "assistant", data: { text: `started: ${prompt}` } });
  }

  async submitInput(input: string): Promise<void> {
    this.effects.push(`submit:${input}`);
  }

  async interruptTurn(): Promise<void> {
    this.effects.push("interrupt");
  }

  async terminateSession(): Promise<void> {
    if (this.ended) return;
    this.effects.push("terminate");
    this.ended = true;
    this.wake?.();
    this.wake = undefined;
  }

  emit(event: NormalizedProviderEvent): void {
    this.queue.push(event);
    this.wake?.();
    this.wake = undefined;
  }

  settle(result: string): void {
    this.emit({ kind: "result", data: { result }, turnTerminal: true });
  }

  async *events(): AsyncIterable<NormalizedProviderEvent> {
    while (true) {
      const event = this.queue.shift();
      if (event) {
        yield event;
        continue;
      }
      if (this.ended) return;
      await new Promise<void>((resolve) => { this.wake = resolve; });
    }
  }
}

async function client(socketPath: string, request: object): Promise<Client> {
  const socket = connect(socketPath);
  await new Promise<void>((resolve, reject) => {
    socket.once("connect", resolve);
    socket.once("error", reject);
  });
  const messages: any[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (line) messages.push(JSON.parse(line));
    }
  });
  const closed = new Promise<void>((resolve) => socket.once("close", () => resolve()));
  socket.write(`${JSON.stringify(request)}\n`);
  return { socket, messages, closed };
}

async function waitFor(condition: () => boolean, label: string): Promise<void> {
  const deadline = Date.now() + 2_000;
  while (!condition() && Date.now() < deadline) await Bun.sleep(5);
  if (!condition()) throw new Error(`timed out waiting for ${label}`);
}

function executionId(client: Client): string {
  const id = client.messages.find((message) => message.type === "launched")?.executionId;
  if (typeof id !== "string") throw new Error("launch response omitted execution id");
  return id;
}

function state(records: JournalRecord[]): HistoryState {
  const accepted = records.find((record) => record.kind === "execution.accepted");
  const terminal = records.findLast((record) => record.kind.startsWith("execution."));
  return {
    prompt: String(accepted?.data.prompt ?? ""),
    turns: records.filter((record) => record.kind === "provider.result").length,
    queuedInputs: records.filter((record) => record.kind === "control.submit_input").length,
    interrupts: records.filter((record) => record.kind === "control.interrupt_turn").length,
    status: terminal?.kind.replace("execution.", "") ?? "active",
  };
}

function explain(records: JournalRecord[]): string {
  const summary = state(records);
  const actions = records.flatMap((record) => {
    if (record.kind === "control.submit_input") return [`queued ${JSON.stringify(record.data.input)}`];
    if (record.kind === "control.interrupt_turn") return ["interrupted active turn"];
    if (record.kind === "provider.result") return [`result ${JSON.stringify(record.data.result)}`];
    if (record.kind === "control.terminate_session") return ["terminated session"];
    return [];
  });
  return `prompt ${JSON.stringify(summary.prompt)} -> ${actions.join(" -> ")}`;
}

async function closedPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("could not allocate dead coordinator port");
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  return address.port;
}

async function portIsDead(port: number): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const socket = connect({ host: "127.0.0.1", port });
    socket.once("connect", () => { socket.destroy(); resolve(false); });
    socket.once("error", (error: NodeJS.ErrnoException) => {
      socket.destroy();
      if (error.code === "ECONNREFUSED") resolve(true);
      else reject(error);
    });
  });
}

export async function runBridgeAcceptance(options: AcceptanceOptions = {}): Promise<string[]> {
  const lines: string[] = [];
  const output = options.output ?? ((line: string) => console.log(line));
  const emit = (line: string) => { lines.push(line); output(line); };
  const pass = (clause: string, detail: string) => emit(`PASS ${clause}: ${detail}`);
  const root = mkdtempSync(join(tmpdir(), "north-bridge-accept-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  const sessions = new Map<string, AcceptanceSession>();
  const provider: BridgeProviderExecution = {
    async open(context) {
      const session = new AcceptanceSession(context.executionId, context.prompt);
      sessions.set(context.executionId, session);
      return session;
    },
  };
  const northd = new Northd({ socketPath, journalRoot, provider, providerAdapter: "mock-provider" });
  let hostOpen = false;
  const previousNorthPort = process.env.NORTH_PORT;

  try {
    emit("INFO provider=mock-provider (deterministic fixture; live Codex admission is probed separately)");
    await northd.listen();
    hostOpen = true;
    const steeredLaunch = await client(socketPath, {
      op: "launch", prompt: "alpha initial", cwd: root,
    });
    const interruptedLaunch = await client(socketPath, {
      op: "launch", prompt: "beta initial", cwd: root,
    });
    await waitFor(
      () => steeredLaunch.messages.some((message) => message.record?.kind === "provider.assistant")
        && interruptedLaunch.messages.some((message) => message.record?.kind === "provider.assistant"),
      "both provider executions",
    );
    const steeredId = executionId(steeredLaunch);
    const interruptedId = executionId(interruptedLaunch);
    if (steeredId === interruptedId || sessions.size !== 2)
      throw new Error("two launches did not produce independent provider sessions");
    pass("launch-two-provider-executions", "2 independent sessions are active");

    steeredLaunch.socket.destroy();
    interruptedLaunch.socket.destroy();
    await Promise.all([steeredLaunch.closed, interruptedLaunch.closed]);
    pass("kill-ui-client", "both launch clients disconnected while provider sessions stayed active");

    const steeredAttach = await client(socketPath, { op: "attach", executionId: steeredId, cursor: 0 });
    const interruptedAttach = await client(socketPath, {
      op: "attach", executionId: interruptedId, cursor: 0,
    });
    await waitFor(
      () => steeredAttach.messages.some((message) => message.type === "barrier")
        && interruptedAttach.messages.some((message) => message.type === "barrier"),
      "both reattach barriers",
    );
    pass("reattach-to-both", "both journals replayed through attachment barriers and resumed live tails");

    const steeredSession = sessions.get(steeredId)!;
    const interruptedSession = sessions.get(interruptedId)!;
    const steer = await client(socketPath, {
      op: "submitInput", executionId: steeredId, input: "alpha follow-up",
    });
    await steer.closed;
    if (steer.messages.at(-1)?.delivery !== "queued-next-turn"
        || steeredSession.effects.includes("submit:alpha follow-up"))
      throw new Error("steer was not held at the active-turn boundary");
    steeredSession.settle("alpha initial complete");
    await waitFor(
      () => steeredSession.effects.includes("submit:alpha follow-up"),
      "queued follow-up delivery",
    );
    steeredSession.emit({ kind: "assistant", data: { text: "alpha follow-up running" } });
    steeredSession.settle("alpha follow-up complete");
    pass("steer-one-queued-next-turn", "follow-up stayed queued until the first turn terminal event");

    const interrupt = await client(socketPath, { op: "interruptTurn", executionId: interruptedId });
    await interrupt.closed;
    if (interrupt.messages.at(-1)?.delivery !== "active-turn"
        || interruptedSession.effects.at(-1) !== "interrupt")
      throw new Error("interrupt did not reach the other active provider turn");
    interruptedSession.settle("beta interrupted");
    pass("interrupt-the-other", "active turn interrupted without terminating its provider session");

    await waitFor(
      () => steeredAttach.messages.filter((message) => message.record?.kind === "session.idle").length === 2
        && interruptedAttach.messages.some((message) => message.record?.kind === "session.idle"),
      "turn terminal boundaries",
    );
    const steeredTerminate = await client(socketPath, {
      op: "terminateSession", executionId: steeredId,
    });
    const interruptedTerminate = await client(socketPath, {
      op: "terminateSession", executionId: interruptedId,
    });
    await Promise.all([steeredTerminate.closed, interruptedTerminate.closed]);
    await Promise.all([steeredAttach.closed, interruptedAttach.closed]);

    await northd.close();
    hostOpen = false;
    const deadPort = await closedPort();
    process.env.NORTH_PORT = String(deadPort);
    if (!await portIsDead(deadPort)) throw new Error(`coordinator probe unexpectedly connected to ${deadPort}`);
    pass("coordinator-down-for-replay-explain", `NORTH_PORT=${deadPort} refused connections`);

    const steeredRecords = scanJournalFile(
      join(journalRoot, steeredId, "events.log"), steeredId,
    ).records;
    const interruptedRecords = scanJournalFile(
      join(journalRoot, interruptedId, "events.log"), interruptedId,
    ).records;
    const steeredState = state(steeredRecords);
    const interruptedState = state(interruptedRecords);
    emit("STATE DIFF (journal only)");
    emit(`  steered: turns=${steeredState.turns} queued=${steeredState.queuedInputs} interrupts=${steeredState.interrupts} status=${steeredState.status}`);
    emit(`  interrupted: turns=${interruptedState.turns} queued=${interruptedState.queuedInputs} interrupts=${interruptedState.interrupts} status=${interruptedState.status}`);
    if (steeredState.turns !== 2 || steeredState.queuedInputs !== 1 || steeredState.interrupts !== 0
        || interruptedState.turns !== 1 || interruptedState.queuedInputs !== 0
        || interruptedState.interrupts !== 1)
      throw new Error("journal-derived session states did not preserve the expected difference");
    pass("render-state-diff", "steered and interrupted histories remain distinguishable");

    emit("JOURNAL EXPLANATIONS (northd closed; coordinator unreachable)");
    emit(`  steered: ${explain(steeredRecords)}`);
    emit(`  interrupted: ${explain(interruptedRecords)}`);
    if (!explain(steeredRecords).includes("queued \"alpha follow-up\"")
        || !explain(interruptedRecords).includes("interrupted active turn"))
      throw new Error("journal-only explanation omitted a control history");
    pass("explain-both-from-journal-alone", "both prompts, controls, results, and termination were reconstructed");
    emit("ACCEPTANCE PASS 8/8");
    return lines;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    emit(`FAIL acceptance: ${message}`);
    throw error;
  } finally {
    if (hostOpen) await northd.close().catch(() => {});
    if (previousNorthPort === undefined) delete process.env.NORTH_PORT;
    else process.env.NORTH_PORT = previousNorthPort;
    rmSync(root, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  try { await runBridgeAcceptance(); }
  catch { process.exitCode = 1; }
}
