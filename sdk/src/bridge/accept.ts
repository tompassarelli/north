import { mkdtempSync, rmSync } from "node:fs";
import { createServer, connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  wireMessageId,
  wireModelCallId,
  type WireEvent,
  type WireEventWriter,
  type WireKnownEvent,
  type WireModelCallId,
} from "../wire";
import { Northd } from "./host";
import {
  readBridgeWireJournal,
  scanJournalFile,
  type JournalRecord,
} from "./journal";
import type { BridgeProviderExecution, BridgeProviderSession } from "./provider";
import type { BridgeServerMessage } from "./protocol";

interface Client {
  socket: Socket;
  messages: BridgeServerMessage[];
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

function knownWireEvent(event: WireEvent): event is WireKnownEvent {
  return event.version === "north:wire:v2" && event.essential;
}

class AcceptanceSession implements BridgeProviderSession {
  readonly effects: string[] = [];
  #writer: WireEventWriter;
  #executionId: string;
  #queue: WireEvent[] = [];
  #waiting?: PromiseWithResolvers<void>;
  #ended = false;
  #turn = 0;
  #activeModelCall?: WireModelCallId;

  constructor(executionId: string, prompt: string, writer: WireEventWriter) {
    this.#executionId = executionId;
    this.#writer = writer;
    this.#startTurn();
    this.emitAssistant(`started: ${prompt}`);
  }

  #publish(events: readonly WireEvent[]): void {
    this.#queue.push(...events);
    this.#waiting?.resolve();
    this.#waiting = undefined;
  }

  #startTurn(): void {
    this.#turn += 1;
    const modelCallId = wireModelCallId(`model-call:bridge:${this.#executionId}:${this.#turn}`);
    this.#activeModelCall = modelCallId;
    this.#publish([this.#writer.append({
      kind: "model-call.started",
      modelCallId,
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    })]);
  }

  async submitInput(input: string): Promise<void> {
    this.effects.push(`submit:${input}`);
    this.#startTurn();
  }

  async interruptTurn(): Promise<void> {
    this.effects.push("interrupt");
  }

  async terminateSession(): Promise<void> {
    if (this.#ended) return;
    this.effects.push("terminate");
    this.#ended = true;
    this.#waiting?.resolve();
    this.#waiting = undefined;
  }

  emitAssistant(text: string): void {
    const modelCallId = this.#activeModelCall;
    if (!modelCallId) throw new Error("acceptance session has no active model call");
    const messageId = wireMessageId(
      `message:bridge:${this.#executionId}:${this.#turn}:${crypto.randomUUID()}`,
    );
    this.#publish(this.#writer.appendAll([
      {
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "started",
        role: "assistant",
      },
      {
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "delta",
        role: "assistant",
        content: text,
      },
      {
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "completed",
        role: "assistant",
      },
    ]));
  }

  settle(result: string): void {
    const modelCallId = this.#activeModelCall;
    if (!modelCallId) throw new Error("acceptance session has no active model call");
    this.emitAssistant(result);
    this.#publish([this.#writer.append({
      kind: "model-call.completed",
      modelCallId,
      status: "succeeded",
      origin: "provider",
      usage: {
        lifetime: {
          inputTokens: this.#turn,
          outputTokens: this.#turn,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          modelCalls: this.#turn,
        },
        context: { tokens: this.#turn * 2 },
      },
      usageCoverage: "exact",
    })]);
    this.#activeModelCall = undefined;
  }

  async *events(): AsyncGenerator<WireEvent, void, unknown> {
    while (true) {
      const event = this.#queue.shift();
      if (event) {
        yield event;
        continue;
      }
      if (this.#ended) return;
      this.#waiting = Promise.withResolvers<void>();
      await this.#waiting.promise;
      this.#waiting = undefined;
    }
  }
}

async function client(socketPath: string, request: object): Promise<Client> {
  const connected = Promise.withResolvers<void>();
  const socket = connect(socketPath);
  socket.once("connect", () => connected.resolve());
  socket.once("error", connected.reject);
  await connected.promise;
  const messages: BridgeServerMessage[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (line) messages.push(JSON.parse(line) as BridgeServerMessage);
    }
  });
  const closed = Promise.withResolvers<void>();
  socket.once("close", () => closed.resolve());
  socket.write(`${JSON.stringify(request)}\n`);
  return { socket, messages, closed: closed.promise };
}

async function waitFor(condition: () => boolean, label: string): Promise<void> {
  const deadline = Date.now() + 2_000;
  while (!condition() && Date.now() < deadline) await Bun.sleep(5);
  if (!condition()) throw new Error(`timed out waiting for ${label}`);
}

function executionId(clientConnection: Client): string {
  const launched = clientConnection.messages.find((message) => message.type === "launched");
  if (!launched || launched.type !== "launched") {
    throw new Error("launch response omitted execution id");
  }
  return launched.executionId;
}

function state(controls: JournalRecord[], wire: readonly WireEvent[]): HistoryState {
  const accepted = controls.find((record) => record.kind === "execution.accepted");
  const terminal = wire.findLast((event) =>
    knownWireEvent(event) && event.kind === "run.terminated");
  return {
    prompt: String(accepted?.data.prompt ?? ""),
    turns: wire.filter((event) => event.kind === "model-call.completed").length,
    queuedInputs: controls.filter((record) => record.kind === "control.submit_input").length,
    interrupts: controls.filter((record) => record.kind === "control.interrupt_turn").length,
    status: terminal && knownWireEvent(terminal) && terminal.kind === "run.terminated"
      ? terminal.lifecycle
      : "active",
  };
}

function explain(controls: JournalRecord[], wire: readonly WireEvent[]): string {
  const summary = state(controls, wire);
  const actions = controls.flatMap((record) => {
    if (record.kind === "control.submit_input") return [`queued ${JSON.stringify(record.data.input)}`];
    if (record.kind === "control.interrupt_turn") return ["interrupted active turn"];
    if (record.kind === "control.terminate_session") return ["terminated session"];
    return [];
  });
  const results = wire.flatMap((event) =>
    knownWireEvent(event)
      && event.kind === "message.recorded"
      && event.role === "assistant"
      && event.stage === "delta"
      && typeof event.content === "string"
      ? [`result ${JSON.stringify(event.content)}`]
      : []);
  return `prompt ${JSON.stringify(summary.prompt)} -> ${[...actions, ...results].join(" -> ")}`;
}

async function closedPort(): Promise<number> {
  const server = createServer();
  const listening = Promise.withResolvers<void>();
  server.once("error", listening.reject);
  server.listen(0, "127.0.0.1", () => listening.resolve());
  await listening.promise;
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("could not allocate dead coordinator port");
  const closed = Promise.withResolvers<void>();
  server.close((error) => error ? closed.reject(error) : closed.resolve());
  await closed.promise;
  return address.port;
}

async function portIsDead(port: number): Promise<boolean> {
  const result = Promise.withResolvers<boolean>();
  const socket = connect({ host: "127.0.0.1", port });
  socket.once("connect", () => { socket.destroy(); result.resolve(false); });
  socket.once("error", (error: NodeJS.ErrnoException) => {
    socket.destroy();
    if (error.code === "ECONNREFUSED") result.resolve(true);
    else result.reject(error);
  });
  return result.promise;
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
      const session = new AcceptanceSession(context.executionId, context.prompt, context.writer);
      sessions.set(context.executionId, session);
      return session;
    },
  };
  const northd = new Northd({ socketPath, journalRoot, provider });
  let hostOpen = false;
  const previousNorthPort = process.env.NORTH_PORT;

  try {
    emit("INFO provider=mock-provider (deterministic fixture; live admission is probed separately)");
    await northd.listen();
    hostOpen = true;
    const messagedLaunch = await client(socketPath, {
      op: "launch", prompt: "alpha initial", cwd: root,
    });
    const interruptedLaunch = await client(socketPath, {
      op: "launch", prompt: "beta initial", cwd: root,
    });
    await waitFor(
      () => messagedLaunch.messages.some((message) =>
        message.type === "wire" && message.event.kind === "message.recorded")
        && interruptedLaunch.messages.some((message) =>
          message.type === "wire" && message.event.kind === "message.recorded"),
      "both provider executions",
    );
    const messagedId = executionId(messagedLaunch);
    const interruptedId = executionId(interruptedLaunch);
    if (messagedId === interruptedId || sessions.size !== 2) {
      throw new Error("two launches did not produce independent provider sessions");
    }
    pass("launch-two-provider-executions", "2 independent sessions are active");

    messagedLaunch.socket.destroy();
    interruptedLaunch.socket.destroy();
    await Promise.all([messagedLaunch.closed, interruptedLaunch.closed]);
    pass("kill-ui-client", "both launch clients disconnected while provider sessions stayed active");

    const messagedAttach = await client(socketPath, { op: "attach", executionId: messagedId, cursor: 0 });
    const interruptedAttach = await client(socketPath, {
      op: "attach", executionId: interruptedId, cursor: 0,
    });
    await waitFor(
      () => messagedAttach.messages.some((message) => message.type === "barrier")
        && interruptedAttach.messages.some((message) => message.type === "barrier"),
      "both reattach barriers",
    );
    pass("reattach-to-both", "both wire projections replayed and resumed live tails");

    const messagedSession = sessions.get(messagedId)!;
    const interruptedSession = sessions.get(interruptedId)!;
    const msg = await client(socketPath, {
      op: "submitInput", executionId: messagedId, input: "alpha follow-up",
    });
    await msg.closed;
    const msgResult = msg.messages.at(-1);
    if (msgResult?.type !== "controlled" || msgResult.delivery !== "queued-next-turn"
        || messagedSession.effects.includes("submit:alpha follow-up")) {
      throw new Error("msg was not held at the active-turn boundary");
    }
    messagedSession.settle("alpha initial complete");
    await waitFor(
      () => messagedSession.effects.includes("submit:alpha follow-up"),
      "queued follow-up delivery",
    );
    messagedSession.emitAssistant("alpha follow-up running");
    messagedSession.settle("alpha follow-up complete");
    pass("msg-one-queued-next-turn", "follow-up stayed queued until model-call.completed");

    const interrupt = await client(socketPath, { op: "interruptTurn", executionId: interruptedId });
    await interrupt.closed;
    const interruptResult = interrupt.messages.at(-1);
    if (interruptResult?.type !== "controlled" || interruptResult.delivery !== "active-turn"
        || interruptedSession.effects.at(-1) !== "interrupt") {
      throw new Error("interrupt did not reach the other active provider turn");
    }
    interruptedSession.settle("beta interrupted");
    pass("interrupt-the-other", "active turn interrupted without terminating its provider session");

    await waitFor(
      () => messagedAttach.messages.filter((message) =>
        message.type === "event" && message.record.kind === "session.idle").length === 2
        && interruptedAttach.messages.some((message) =>
          message.type === "event" && message.record.kind === "session.idle"),
      "turn terminal boundaries",
    );
    const messagedTerminate = await client(socketPath, {
      op: "terminateSession", executionId: messagedId,
    });
    const interruptedTerminate = await client(socketPath, {
      op: "terminateSession", executionId: interruptedId,
    });
    await Promise.all([messagedTerminate.closed, interruptedTerminate.closed]);
    await Promise.all([messagedAttach.closed, interruptedAttach.closed]);

    await northd.close();
    hostOpen = false;
    const deadPort = await closedPort();
    process.env.NORTH_PORT = String(deadPort);
    if (!await portIsDead(deadPort)) {
      throw new Error(`coordinator probe unexpectedly connected to ${deadPort}`);
    }
    pass("coordinator-down-for-replay-explain", `NORTH_PORT=${deadPort} refused connections`);

    const messagedRecords = scanJournalFile(
      join(journalRoot, messagedId, "events.log"), messagedId,
    ).records;
    const interruptedRecords = scanJournalFile(
      join(journalRoot, interruptedId, "events.log"), interruptedId,
    ).records;
    const messagedWire = (await readBridgeWireJournal(journalRoot, messagedId)).events;
    const interruptedWire = (await readBridgeWireJournal(journalRoot, interruptedId)).events;
    const messagedState = state(messagedRecords, messagedWire);
    const interruptedState = state(interruptedRecords, interruptedWire);
    emit("STATE DIFF (control + canonical wire projection)");
    emit(`  messaged: turns=${messagedState.turns} queued=${messagedState.queuedInputs} interrupts=${messagedState.interrupts} status=${messagedState.status}`);
    emit(`  interrupted: turns=${interruptedState.turns} queued=${interruptedState.queuedInputs} interrupts=${interruptedState.interrupts} status=${interruptedState.status}`);
    if (messagedState.turns !== 2 || messagedState.queuedInputs !== 1 || messagedState.interrupts !== 0
        || interruptedState.turns !== 1 || interruptedState.queuedInputs !== 0
        || interruptedState.interrupts !== 1) {
      throw new Error("replayed session states did not preserve the expected difference");
    }
    pass("render-state-diff", "messaged and interrupted histories remain distinguishable");

    emit("JOURNAL EXPLANATIONS (northd closed; coordinator unreachable)");
    emit(`  messaged: ${explain(messagedRecords, messagedWire)}`);
    emit(`  interrupted: ${explain(interruptedRecords, interruptedWire)}`);
    if (!explain(messagedRecords, messagedWire).includes("queued \"alpha follow-up\"")
        || !explain(interruptedRecords, interruptedWire).includes("interrupted active turn")) {
      throw new Error("replay explanation omitted a control history");
    }
    pass("explain-both-from-journal-alone", "controls and exact wire results were reconstructed");
    emit("ACCEPTANCE PASS 8/8");
    return lines;
  } catch (error) {
    emit(`FAIL acceptance: ${errorMessage(error)}`);
    throw error;
  } finally {
    if (hostOpen) await northd.close().catch(() => {});
    if (previousNorthPort === undefined) delete process.env.NORTH_PORT;
    else process.env.NORTH_PORT = previousNorthPort;
    rmSync(root, { recursive: true, force: true });
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

if (import.meta.main) {
  try { await runBridgeAcceptance(); }
  catch { process.exitCode = 1; }
}
