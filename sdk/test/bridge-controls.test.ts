import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import {
  bridgeCommandArtifactLocator,
  bridgeCommandPayloadDigest,
  MemoryBridgeCommandReceipts,
} from "../src/bridge/command-receipts";
import {
  BridgeWireJournal,
  ExecutionJournal,
  readBridgeWireJournal,
  scanJournalFile,
  type JournalRecord,
} from "../src/bridge/journal";
import {
  bridgeSystemPrompt,
  type BridgeProviderExecution,
  type BridgeProviderOpenContext,
  type BridgeProviderSession,
} from "../src/bridge/provider";
import type {
  BridgeServerMessage,
} from "../src/bridge/generated/north/bridge/protocol.js";
import {
  decodeWireEvent,
  WireEventWriter,
  wireRunId,
  wireModelCallId,
  wireToolCallId,
  type WireEvent,
  type WireEventDraft,
  type WireModelCallId,
} from "../src/wire";
import { BridgeWireTestSession } from "./support/bridge-wire-session";

interface Client {
  socket: Socket;
  messages: BridgeServerMessage[];
  closed: Promise<void>;
}

const cleanups: Array<() => Promise<void> | void> = [];
const ATTEMPT_ID = `@attempt:${"a".repeat(64)}`;
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

async function client(socketPath: string, request: object): Promise<Client> {
  const socket = connect(socketPath);
  const connected = Promise.withResolvers<void>();
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
      messages.push(JSON.parse(buffer.slice(0, newline)) as BridgeServerMessage);
      buffer = buffer.slice(newline + 1);
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

async function fixture(
  onEffect?: (effect: string) => void,
  role?: "director" | "implementer",
  interruptFailure?: Error,
  terminationFailure?: Error,
  terminationNeverSettles = false,
  providerTeardownTimeoutMs?: number,
  controlJournal?: (root: string, executionId: string) => ExecutionJournal,
) {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-controls-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  let session: BridgeWireTestSession | undefined;
  let opens = 0;
  const openContexts: Array<Parameters<BridgeProviderExecution["open"]>[0]> = [];
  const provider: BridgeProviderExecution = {
    async open(context) {
      opens += 1;
      openContexts.push(context);
      session = new BridgeWireTestSession(context, {
        onEffect, interruptFailure, terminationFailure, terminationNeverSettles,
      });
      return session;
    },
  };
  const commandReceipts = new MemoryBridgeCommandReceipts([ATTEMPT_ID]);
  const northd = new Northd({
    socketPath, journalRoot, provider,
    commandReceipts,
    ...(providerTeardownTimeoutMs === undefined ? {} : { providerTeardownTimeoutMs }),
    ...(controlJournal === undefined ? {} : { controlJournal }),
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());
  const launched = await client(socketPath, {
    op: "launch", prompt: "first", cwd: root, attemptId: ATTEMPT_ID,
    ...(role ? { role } : {}),
  });
  await waitFor(() => launched.messages.some((message) => message.type === "launched"), "launch id");
  await waitFor(() => session !== undefined, "provider session");
  const launch = launched.messages.find((message) => message.type === "launched");
  if (!launch || launch.type !== "launched") throw new Error("launch id missing");
  const executionId = launch.executionId;
  return {
    root, socketPath, journalRoot, session: session!, launched, executionId,
    opens: () => opens, openContexts, commandReceipts,
  };
}

class AcceptanceFsyncFailureJournal extends ExecutionJournal {
  #failed = false;

  override append(kind: string, data: Record<string, unknown> = {}): JournalRecord {
    const record = super.append(kind, data);
    if (kind === "execution.accepted" && !this.#failed) {
      this.#failed = true;
      throw new Error("simulated control journal fsync failure");
    }
    return record;
  }
}

class TerminalControlFailureJournal extends ExecutionJournal {
  override append(kind: string, data: Record<string, unknown> = {}): JournalRecord {
    const record = super.append(kind, data);
    if (kind === "execution.terminated") {
      throw new Error("simulated terminal control journal fsync failure");
    }
    return record;
  }
}

class IdleControlFailureJournal extends ExecutionJournal {
  #failed = false;

  override append(kind: string, data: Record<string, unknown> = {}): JournalRecord {
    const record = super.append(kind, data);
    if (kind === "session.idle" && !this.#failed) {
      this.#failed = true;
      throw new Error("simulated idle control journal fsync failure");
    }
    return record;
  }
}

class GatedProviderReplacementSession implements BridgeProviderSession {
  readonly effects: string[] = [];
  interruptRequests = 0;
  replacementInterrupts = 0;
  #writer: WireEventWriter;
  #label: string;
  #queue: WireEvent[] = [];
  #waiting?: PromiseWithResolvers<void>;
  #pendingInterrupt?: PromiseWithResolvers<void>;
  #replacementInterrupted = false;
  #ended = false;
  #turn = 0;
  #phase: "initial" | "gap" | "replacement" | "idle" | "redirected" = "initial";
  #activeModelCall?: WireModelCallId;

  constructor(context: Pick<BridgeProviderOpenContext, "executionId" | "writer">) {
    this.#writer = context.writer;
    this.#label = context.executionId;
    this.#startModelCall("initial", 1);
  }

  #publish(draft: WireEventDraft): WireEvent {
    const event = this.#writer.append(draft);
    this.#queue.push(event);
    this.#waiting?.resolve();
    this.#waiting = undefined;
    return event;
  }

  #startModelCall(label: string, attempt: number): WireModelCallId {
    if (this.#activeModelCall) throw new Error("gated session already has an active model call");
    this.#turn += 1;
    const modelCallId = wireModelCallId(
      `model-call:bridge-gated:${this.#label}:${this.#turn}:${label}`,
    );
    this.#activeModelCall = modelCallId;
    this.#publish({
      kind: "model-call.started",
      modelCallId,
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt,
    });
    return modelCallId;
  }

  #completeActive(status: "succeeded" | "failed" | "cancelled", errorCode?: string): void {
    const modelCallId = this.#activeModelCall;
    if (!modelCallId) throw new Error("gated session has no active model call");
    this.#publish({
      kind: "model-call.completed",
      modelCallId,
      status,
      origin: "provider",
      usage: this.#writer.snapshot()!.usage,
      usageCoverage: "exact",
      ...(errorCode === undefined ? {} : { errorCode }),
    });
    this.#activeModelCall = undefined;
  }

  enterReplacementGap(): WireModelCallId {
    if (this.#phase !== "initial") throw new Error("gated session is not on its initial turn");
    const replacedModelCallId = this.#activeModelCall;
    if (!replacedModelCallId) throw new Error("gated session has no model call to replace");
    this.#publish({
      kind: "model-call.completed",
      modelCallId: replacedModelCallId,
      status: "failed",
      origin: "north",
      usage: this.#writer.snapshot()!.usage,
      usageCoverage: "unavailable",
      errorCode: "provider_session_replaced",
    });
    this.#activeModelCall = undefined;
    this.#phase = "gap";
    return replacedModelCallId;
  }

  startReplacement(): WireModelCallId {
    if (this.#phase !== "gap") throw new Error("gated session is not between provider sessions");
    const replacementModelCallId = this.#startModelCall("replacement", 2);
    this.#phase = "replacement";
    if (this.#pendingInterrupt) {
      this.#interruptReplacement();
      this.#pendingInterrupt.resolve();
      this.#pendingInterrupt = undefined;
    }
    return replacementModelCallId;
  }

  completeReplacement(): void {
    if (this.#phase !== "replacement") throw new Error("gated replacement is not active");
    this.#completeActive("cancelled");
    this.#phase = "idle";
  }

  completeRedirected(): void {
    if (this.#phase !== "redirected") throw new Error("gated redirected turn is not active");
    this.#completeActive("succeeded");
    this.#phase = "idle";
  }

  async submitInput(input: string): Promise<void> {
    if (this.#phase !== "idle") throw new Error("gated session received input before a real terminal");
    this.effects.push(`submit:${input}`);
    this.#startModelCall("redirected", 1);
    this.#phase = "redirected";
  }

  async interruptTurn(): Promise<void> {
    this.interruptRequests += 1;
    if (this.#phase === "gap") {
      this.#pendingInterrupt ??= Promise.withResolvers<void>();
      await this.#pendingInterrupt.promise;
      return;
    }
    if (this.#phase === "replacement") {
      this.#interruptReplacement();
      return;
    }
    throw new Error("RAW_PROVIDER_GATED_INTERRUPT_CANARY");
  }

  #interruptReplacement(): void {
    if (this.#replacementInterrupted) return;
    this.#replacementInterrupted = true;
    this.replacementInterrupts += 1;
    this.effects.push("interrupt:replacement");
  }

  async terminateSession(): Promise<void> {
    if (this.#ended) return;
    this.effects.push("terminate");
    this.#ended = true;
    this.#waiting?.resolve();
    this.#waiting = undefined;
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

test("restart observes an unresolved Store intent without replaying its provider effect", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-intent-restart-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  const executionId = "restart-intent";
  const receipts = new MemoryBridgeCommandReceipts([ATTEMPT_ID]);
  await receipts.bindExecution(executionId, ATTEMPT_ID, {});

  const journal = new ExecutionJournal(journalRoot, executionId);
  journal.append("execution.accepted", {
    prompt: "do not replay", cwd: root, role: "implementer", attemptId: ATTEMPT_ID,
  });
  journal.close();
  const wireJournal = await BridgeWireJournal.open(journalRoot, executionId);
  const writer = new WireEventWriter({ runId: wireRunId(`bridge:${executionId}`) });
  await wireJournal.append(writer.append({
    kind: "run.started", lifecycle: "running", owner: "bridge:implementer",
  }));
  await wireJournal.close();
  const command = await receipts.admit({
    executionId,
    attemptId: ATTEMPT_ID,
    kind: "submit-input",
    payloadDigest: bridgeCommandPayloadDigest("submit-input", "do not replay"),
    payloadArtifact: bridgeCommandArtifactLocator(executionId, 1),
    delivery: "queued-next-turn",
  });
  await receipts.commitIntent(command);

  let providerOpens = 0;
  const northd = new Northd({
    socketPath,
    journalRoot,
    commandReceipts: receipts,
    provider: {
      async open() {
        providerOpens += 1;
        throw new Error("restart must not construct a provider");
      },
    },
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());
  const attached = await client(socketPath, { op: "attach", executionId, cursor: 0 });
  await attached.closed;

  expect(providerOpens).toBe(0);
  expect(await receipts.reconcile(ATTEMPT_ID)).toEqual({
    pending: [], unresolvedIntents: [command],
  });
  expect((await readBridgeWireJournal(journalRoot, executionId)).events.at(-1))
    .toMatchObject({ kind: "run.terminated", reason: { code: "provider_process_died" } });
});

test("a durable wire start gets one failed terminal when control acceptance persistence fails", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-acceptance-fsync-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  let providerOpened = false;
  const northd = new Northd({
    socketPath,
    journalRoot,
    provider: {
      async open() {
        providerOpened = true;
        throw new Error("provider must not open after failed control acceptance");
      },
    },
    commandReceipts: new MemoryBridgeCommandReceipts([ATTEMPT_ID]),
    controlJournal: (journalPath, executionId) =>
      new AcceptanceFsyncFailureJournal(journalPath, executionId),
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const launch = await client(socketPath, {
    op: "launch", prompt: "first", cwd: root, attemptId: ATTEMPT_ID,
  });
  await launch.closed;
  expect(launch.messages.at(-1)).toEqual({
    type: "error", message: "simulated control journal fsync failure",
  });
  expect(providerOpened).toBe(false);

  const executions = readdirSync(journalRoot);
  expect(executions).toHaveLength(1);
  const replay = await readBridgeWireJournal(journalRoot, executions[0]!);
  expect(replay.events.map((event) => event.kind)).toEqual([
    "run.started", "run.terminated",
  ]);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated",
    lifecycle: "failed",
    reason: { code: "provider_error" },
  });
});

test("a forged event cannot hide a provider-owned run terminal behind the same ID and sequence", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-forged-wire-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  const northd = new Northd({
    socketPath,
    journalRoot,
    provider: {
      async open(context) {
        const terminal = context.writer.terminate({
          lifecycle: "completed",
          reason: { code: "completed" },
        }).at(-1);
        if (!terminal || terminal.kind !== "run.terminated") {
          throw new Error("forged-wire fixture terminal missing");
        }
        const forged = decodeWireEvent({
          version: terminal.version,
          id: terminal.id,
          runId: terminal.runId,
          parentId: terminal.parentId,
          sequence: terminal.sequence,
          at: terminal.at,
          essential: terminal.essential,
          requiredSemantics: terminal.requiredSemantics,
          kind: "run.progress",
          lifecycle: "running",
          progress: { currentAction: "forged terminal disguise" },
        });
        return {
          async *events() { yield forged; },
          async submitInput() {},
          async interruptTurn() {},
          async terminateSession() {},
        };
      },
    },
    commandReceipts: new MemoryBridgeCommandReceipts([ATTEMPT_ID]),
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const launched = await client(socketPath, {
    op: "launch", prompt: "first", cwd: root, attemptId: ATTEMPT_ID,
  });
  await waitFor(() => launched.socket.destroyed, "forged provider run failure terminal");
  await launched.closed;
  const executionId = launched.messages.find((message) => message.type === "launched")?.executionId;
  if (!executionId) throw new Error("launch id missing");
  const replay = await readBridgeWireJournal(journalRoot, executionId);
  expect(replay.events.map((event) => event.kind)).toEqual([
    "run.started", "run.terminated",
  ]);
  expect(replay.events.at(-1)).toMatchObject({
    kind: "run.terminated",
    lifecycle: "failed",
    reason: { code: "provider_error" },
  });
  expect(replay.events.some((event) =>
    event.kind === "run.progress" && event.progress.currentAction === "forged terminal disguise"))
    .toBe(false);
});

test("terminal control persistence failure cannot strand attached subscribers", async () => {
  const f = await fixture(
    undefined,
    undefined,
    undefined,
    undefined,
    false,
    undefined,
    (journalPath, executionId) => new TerminalControlFailureJournal(journalPath, executionId),
  );
  const attached = await client(f.socketPath, {
    op: "attach", executionId: f.executionId, cursor: 0,
  });
  await waitFor(() => attached.messages.some((message) => message.type === "barrier"), "attach barrier");

  const terminate = await client(f.socketPath, {
    op: "terminateSession", executionId: f.executionId,
  });
  await terminate.closed;
  expect(terminate.messages.at(-1)).toEqual({
    type: "error", message: "bridge terminal persistence failed",
  });
  await waitFor(() => attached.socket.destroyed, "attached terminal subscriber closure");
  await attached.closed;
  await f.launched.closed;

  const replay = await readBridgeWireJournal(f.journalRoot, f.executionId);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toEqual([
    expect.objectContaining({
      kind: "run.terminated",
      lifecycle: "cancelled",
      reason: expect.objectContaining({ code: "cancelled" }),
    }),
  ]);
});

test("session idle control failure leaves the canonical tail usable for a failed run terminal", async () => {
  const f = await fixture(
    undefined,
    undefined,
    undefined,
    undefined,
    false,
    undefined,
    (journalPath, executionId) => new IdleControlFailureJournal(journalPath, executionId),
  );
  f.session.complete("turn reached its canonical terminal");
  await waitFor(() => f.launched.socket.destroyed, "failed run terminal after idle projection failure");
  await f.launched.closed;

  const replay = await readBridgeWireJournal(f.journalRoot, f.executionId);
  expect(replay.events.filter((event) => event.kind === "model-call.completed")).toEqual([
    expect.objectContaining({ kind: "model-call.completed", status: "succeeded" }),
  ]);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toEqual([
    expect.objectContaining({
      kind: "run.terminated",
      lifecycle: "failed",
      reason: { code: "provider_error" },
    }),
  ]);
});

test("launch role defaults to implementer and an explicit director reaches the provider", async () => {
  const worker = await fixture();
  await waitFor(() => worker.openContexts.length === 1, "default worker provider open");
  expect(worker.openContexts[0]?.role).toBe("implementer");
  expect(scanJournalFile(join(worker.journalRoot, worker.executionId, "events.log")).records
    .find((record) => record.kind === "execution.accepted")?.data.role).toBe("implementer");

  const supervisor = await fixture(undefined, "director");
  await waitFor(() => supervisor.openContexts.length === 1, "director provider open");
  expect(supervisor.openContexts[0]?.role).toBe("director");
  expect(scanJournalFile(join(supervisor.journalRoot, supervisor.executionId, "events.log")).records
    .find((record) => record.kind === "execution.accepted")?.data.role).toBe("director");

  const invalid = await client(supervisor.socketPath, {
    op: "launch", prompt: "invalid", cwd: supervisor.root, role: "portfolio",
    attemptId: ATTEMPT_ID,
  });
  await invalid.closed;
  expect(invalid.messages.at(-1)).toEqual({
    type: "error", message: "bridge launch role must be director or implementer",
  });

  expect(bridgeSystemPrompt("director")).toContain("North MCP spawn and dispatch");
  expect(bridgeSystemPrompt("implementer")).toContain("do not spawn or delegate");
});

test("queued input becomes a second turn on the same provider session", async () => {
  const f = await fixture();
  const msg = await client(f.socketPath, {
    op: "submitInput", executionId: f.executionId, input: "second",
  });
  await msg.closed;
  expect(msg.messages.at(-1)).toMatchObject({
    type: "controlled", delivery: "queued-next-turn",
  });
  expect(f.session.effects).not.toContain("submit:second");

  f.session.complete("first done");
  await waitFor(() => f.session.effects.includes("submit:second"), "second turn submission");
  expect(f.opens()).toBe(1);
  f.session.complete("second done");
  await waitFor(
    () => f.launched.messages.filter((message) => message.record?.kind === "session.idle").length === 2,
    "second idle boundary",
  );
  expect(f.launched.messages
    .filter((message) => message.record?.kind === "session.idle")
    .map((message) => message.record.data)).toEqual([
    expect.objectContaining({ armed: true, disposition: "completed", pendingInputs: 1 }),
    expect.objectContaining({ armed: true, disposition: "completed", pendingInputs: 0 }),
  ]);
});

test("queued input waits through a provider-session replacement until its turn settles", async () => {
  const f = await fixture();
  const queued = await client(f.socketPath, {
    op: "submitInput", executionId: f.executionId, input: "after replacement",
  });
  await queued.closed;
  expect(f.session.effects).not.toContain("submit:after replacement");

  const replacementModelCallId = f.session.replaceProviderSession();
  await waitFor(
    () => f.launched.messages.some((message) =>
      message.type === "wire"
      && message.event.kind === "model-call.started"
      && message.event.modelCallId === replacementModelCallId),
    "replacement provider-session turn",
  );
  expect(f.launched.messages.filter((message) => message.record?.kind === "session.idle"))
    .toEqual([]);
  expect(f.session.effects).not.toContain("submit:after replacement");

  f.session.complete("replacement completed");
  await waitFor(
    () => f.session.effects.includes("submit:after replacement"),
    "queued input after replacement terminal",
  );
  expect(f.launched.messages.filter((message) => message.record?.kind === "session.idle"))
    .toEqual([
      expect.objectContaining({
        record: expect.objectContaining({
          data: expect.objectContaining({ pendingInputs: 1, disposition: "completed" }),
        }),
      }),
    ]);

  await f.session.terminateSession();
  await f.launched.closed;
  const failures = scanJournalFile(
    join(f.journalRoot, f.executionId, "events.log"), f.executionId,
  ).records.filter((record) => record.kind === "execution.failure");
  expect(failures).toHaveLength(1);
  expect(failures[0]?.data.code).toBe("provider_process_died");
  expect(failures[0]?.data.evidence).toBeUndefined();
});

test("interrupt and redirect stay gated across provider-session replacement preflight", async () => {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-replacement-controls-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  let session: GatedProviderReplacementSession | undefined;
  const provider: BridgeProviderExecution = {
    async open(context) {
      session = new GatedProviderReplacementSession(context);
      return session;
    },
  };
  const northd = new Northd({
    socketPath,
    journalRoot,
    provider,
    commandReceipts: new MemoryBridgeCommandReceipts([ATTEMPT_ID]),
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const launched = await client(socketPath, {
    op: "launch", prompt: "first", cwd: root, attemptId: ATTEMPT_ID,
  });
  await waitFor(() => session !== undefined, "gated provider session");
  await waitFor(
    () => launched.messages.some((message) =>
      message.type === "wire" && message.event.kind === "model-call.started"),
    "initial model call",
  );
  const launch = launched.messages.find((message) => message.type === "launched");
  if (!launch || launch.type !== "launched") throw new Error("launch id missing");
  const executionId = launch.executionId;
  const attached = await client(socketPath, { op: "attach", executionId, cursor: 0 });
  await waitFor(() => attached.messages.some((message) => message.type === "barrier"), "attach barrier");

  const replacedModelCallId = session!.enterReplacementGap();
  await waitFor(
    () => attached.messages.some((message) =>
      message.type === "wire"
      && message.event.kind === "model-call.completed"
      && message.event.modelCallId === replacedModelCallId),
    "provider replacement preflight terminal",
  );

  const interrupt = await client(socketPath, { op: "interruptTurn", executionId });
  const redirect = await client(socketPath, {
    op: "redirectNow", executionId, input: "redirect after replacement",
  });
  await waitFor(() => session!.interruptRequests === 2, "both gated interrupt requests");
  await Bun.sleep(20);
  for (const control of [interrupt, redirect]) {
    expect(control.socket.destroyed).toBe(false);
    expect(control.messages.some((message) =>
      message.type === "controlled" || message.type === "error")).toBe(false);
  }
  expect(session!.replacementInterrupts).toBe(0);
  expect(session!.effects).not.toContain("submit:redirect after replacement");
  expect(attached.messages.some((message) => message.record?.kind === "session.idle")).toBe(false);

  const replacementModelCallId = session!.startReplacement();
  await waitFor(
    () => attached.messages.some((message) =>
      message.type === "wire"
      && message.event.kind === "model-call.started"
      && message.event.modelCallId === replacementModelCallId),
    "replacement model call",
  );
  await Promise.all([interrupt.closed, redirect.closed]);
  expect(interrupt.messages.at(-1)).toMatchObject({
    type: "controlled", control: "interruptTurn", delivery: "active-turn",
  });
  expect(redirect.messages.at(-1)).toMatchObject({
    type: "controlled", control: "redirectNow", delivery: "interrupt-and-redirect",
  });
  expect([...interrupt.messages, ...redirect.messages]
    .some((message) => message.type === "error")).toBe(false);
  expect(session!.replacementInterrupts).toBe(1);
  expect(session!.effects).toEqual(["interrupt:replacement"]);

  session!.completeReplacement();
  await waitFor(
    () => session!.effects.includes("submit:redirect after replacement"),
    "redirect delivery after replacement terminal",
  );
  await waitFor(
    () => attached.messages.filter((message) =>
      message.type === "wire" && message.event.kind === "model-call.started").length === 3,
    "redirected model call",
  );
  const firstIdle = attached.messages.find((message) => message.record?.kind === "session.idle");
  expect(firstIdle?.record.data).toEqual(expect.objectContaining({
    armed: true,
    disposition: "interrupted",
    pendingInputs: 1,
  }));

  session!.completeRedirected();
  await waitFor(
    () => attached.messages.filter((message) => message.record?.kind === "session.idle").length === 2,
    "redirected idle boundary",
  );
  const terminate = await client(socketPath, { op: "terminateSession", executionId });
  await terminate.closed;
  await launched.closed;
  await attached.closed;

  const replay = await readBridgeWireJournal(journalRoot, executionId);
  const attachedWire = attached.messages
    .filter((message) => message.type === "wire")
    .map((message) => message.event);
  expect(attachedWire).toEqual(replay.events);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toEqual([
    expect.objectContaining({ kind: "run.terminated", lifecycle: "completed" }),
  ]);

  const records = scanJournalFile(join(journalRoot, executionId, "events.log"), executionId).records;
  const attachedRecords = attached.messages
    .filter((message) => message.type === "event")
    .map((message) => message.record);
  expect(attachedRecords).toEqual(records);
  expect(records.filter((record) => record.kind === "control.interrupt_turn")).toHaveLength(1);
  expect(records.filter((record) => record.kind === "control.redirect_now")).toHaveLength(1);
  expect(records.filter((record) => record.kind === "session.idle")).toHaveLength(2);
  expect(records.filter((record) => record.kind === "control.input_delivered")).toHaveLength(1);

  const replacementTerminal = replay.events.find((event) =>
    event.kind === "model-call.completed" && event.modelCallId === replacementModelCallId);
  if (!replacementTerminal) throw new Error("replacement terminal missing");
  const redirectedStart = replay.events.find((event) =>
    event.kind === "model-call.started"
    && event.modelCallId !== replacedModelCallId
    && event.modelCallId !== replacementModelCallId);
  if (!redirectedStart) throw new Error("redirected model call missing");
  const preflightTerminalIndex = replay.events.findIndex((event) =>
    event.kind === "model-call.completed" && event.modelCallId === replacedModelCallId);
  const replacementStartIndex = replay.events.findIndex((event) =>
    event.kind === "model-call.started" && event.modelCallId === replacementModelCallId);
  expect(preflightTerminalIndex).toBeGreaterThanOrEqual(0);
  expect(replacementStartIndex).toBeGreaterThan(preflightTerminalIndex);
  expect(replacementTerminal.status).toBe("cancelled");
  expect(replacementTerminal.sequence).toBeLessThan(redirectedStart.sequence);

  const redirectRecord = records.find((record) => record.kind === "control.redirect_now");
  const idleRecord = records.find((record) => record.kind === "session.idle");
  const deliveryRecord = records.find((record) => record.kind === "control.input_delivered");
  if (!redirectRecord || !idleRecord || !deliveryRecord) {
    throw new Error("replacement control journal projection missing");
  }
  expect(redirectRecord.seq).toBeLessThan(idleRecord.seq);
  expect(idleRecord.seq).toBeLessThan(deliveryRecord.seq);
  expect(idleRecord.data.wireCursor).toBe(replacementTerminal.sequence + 1);
  expect(deliveryRecord.data.commandSeq).toBe(redirectRecord.seq);
});

test("interrupt settles only the active turn and leaves the session attachable", async () => {
  const f = await fixture();
  const interrupt = await client(f.socketPath, { op: "interruptTurn", executionId: f.executionId });
  await interrupt.closed;
  expect(f.session.effects).toEqual(["interrupt"]);

  f.session.complete("interrupted", "cancelled");
  await waitFor(
    () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
    "idle boundary",
  );
  expect(f.launched.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
    .toEqual(expect.objectContaining({ armed: true, disposition: "interrupted", pendingInputs: 0 }));
  const attached = await client(f.socketPath, { op: "attach", executionId: f.executionId, cursor: 0 });
  await waitFor(() => attached.messages.some((message) => message.type === "barrier"), "attach barrier");
  expect(attached.socket.destroyed).toBe(false);
  attached.socket.destroy();
  await attached.closed;
});

test("redirect-now interrupts, awaits the terminal boundary, then submits replacement input", async () => {
  const f = await fixture();
  const redirect = await client(f.socketPath, {
    op: "redirectNow", executionId: f.executionId, input: "replacement",
  });
  await redirect.closed;
  expect(redirect.messages.at(-1)).toMatchObject({
    type: "controlled", delivery: "interrupt-and-redirect",
  });
  expect(f.session.effects).toEqual(["interrupt"]);

  f.session.complete("interrupted", "cancelled");
  await waitFor(() => f.session.effects.length === 2, "replacement submission");
  expect(f.session.effects).toEqual(["interrupt", "submit:replacement"]);
  expect(f.launched.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
    .toEqual(expect.objectContaining({ armed: true, disposition: "interrupted", pendingInputs: 1 }));
});

test("a rejected interrupt cannot mislabel a later normal terminal boundary", async () => {
  for (const request of [
    { op: "interruptTurn" },
    { op: "redirectNow", input: "replacement" },
  ] as const) {
    const providerCanary = `RAW_PROVIDER_CONTROL_CANARY_${request.op}`;
    const f = await fixture(undefined, undefined, new Error(providerCanary));
    const control = await client(f.socketPath, { ...request, executionId: f.executionId });
    await control.closed;
    expect(control.messages.at(-1)).toEqual({
      type: "error", message: "bridge provider turn control failed",
    });

    f.session.complete("completed normally");
    await waitFor(
      () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
      "normal idle boundary",
    );
    expect(f.launched.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
      .toEqual(expect.objectContaining({ armed: true, disposition: "completed", pendingInputs: 0 }));
    const records = scanJournalFile(
      join(f.journalRoot, f.executionId, "events.log"), f.executionId,
    ).records;
    expect(JSON.stringify({ control: control.messages, launch: f.launched.messages, records }))
      .not.toContain(providerCanary);
  }
});

test("each control command has a Store intent before its provider effect and a receipt after", async () => {
  let journalPath = "";
  let commandReceipts: MemoryBridgeCommandReceipts | undefined;
  const observed: Array<{
    effect: string;
    kinds: string[];
    intents: number;
    receipts: number;
  }> = [];
  const f = await fixture((effect) => {
    const records = scanJournalFile(journalPath).records;
    observed.push({
      effect,
      kinds: records.map((record) => record.kind),
      intents: commandReceipts?.intents.length ?? 0,
      receipts: commandReceipts?.receipts.length ?? 0,
    });
  });
  commandReceipts = f.commandReceipts;
  journalPath = join(f.journalRoot, f.executionId, "events.log");

  const interrupt = await client(f.socketPath, { op: "interruptTurn", executionId: f.executionId });
  await interrupt.closed;
  f.session.complete("interrupted", "cancelled");
  await waitFor(
    () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
    "idle boundary",
  );

  const msg = await client(f.socketPath, {
    op: "submitInput", executionId: f.executionId, input: "next",
  });
  await msg.closed;
  f.session.complete("next done");
  await waitFor(
    () => f.launched.messages.filter((message) => message.record?.kind === "session.idle").length === 2,
    "second idle boundary",
  );
  expect(f.launched.messages
    .filter((message) => message.record?.kind === "session.idle")
    .map((message) => message.record.data.disposition))
    .toEqual(["interrupted", "completed"]);

  const terminate = await client(f.socketPath, { op: "terminateSession", executionId: f.executionId });
  await terminate.closed;

  expect(observed.find(({ effect }) => effect === "interrupt")?.kinds.at(-1))
    .toBe("control.interrupt_turn");
  expect(observed.find(({ effect }) => effect === "submit:next")?.kinds.at(-1))
    .toBe("control.submit_input");
  expect(observed.find(({ effect }) => effect === "terminate")?.kinds.at(-1))
    .toBe("control.terminate_session");
  expect(observed.map(({ effect, intents, receipts }) => ({ effect, intents, receipts })))
    .toEqual([
      { effect: "interrupt", intents: 1, receipts: 0 },
      { effect: "submit:next", intents: 2, receipts: 1 },
      { effect: "terminate", intents: 3, receipts: 2 },
    ]);
  expect(f.commandReceipts.receipts).toEqual(f.commandReceipts.commands.map((command) => ({
    commandId: command.commandId,
    outcome: "succeeded",
  })));
});

test("a provider close failure still commits exactly one host-owned run terminal", async () => {
  const f = await fixture(undefined, undefined, undefined, new Error("close rejected"));
  const terminate = await client(f.socketPath, {
    op: "terminateSession", executionId: f.executionId,
  });
  await terminate.closed;
  expect(terminate.messages.at(-1)).toEqual({
    type: "error", message: "provider session teardown failed",
  });
  await f.launched.closed;

  const replay = await readBridgeWireJournal(f.journalRoot, f.executionId);
  const terminals = replay.events.filter((event) => event.kind === "run.terminated");
  expect(terminals).toHaveLength(1);
  expect(terminals[0]).toMatchObject({
    kind: "run.terminated",
    lifecycle: "failed",
    reason: { code: "provider_error" },
  });
  const failures = scanJournalFile(
    join(f.journalRoot, f.executionId, "events.log"), f.executionId,
  ).records.filter((record) => record.kind === "execution.failure");
  expect(failures).toHaveLength(1);
});

test("idle termination synthetically closes background work before one outer terminal", async () => {
  const f = await fixture();
  const backgroundTask = wireToolCallId(`tool-call:bridge-background:${f.executionId}`);
  f.session.publish({
    kind: "tool.admitted",
    toolCallId: backgroundTask,
    name: "background-task",
    schema: {
      status: "unavailable",
      reason: "provider background task has no callable schema",
    },
    argumentPreview: "task_started",
  });
  f.session.complete("foreground turn done");
  await waitFor(
    () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
    "idle boundary with background work",
  );

  const terminate = await client(f.socketPath, {
    op: "terminateSession", executionId: f.executionId,
  });
  await terminate.closed;
  expect(terminate.messages.at(-1)).toMatchObject({
    type: "controlled", delivery: "session-terminated",
  });
  await f.launched.closed;

  const replay = await readBridgeWireJournal(f.journalRoot, f.executionId);
  expect(replay.events.filter((event) => event.kind === "run.terminated")).toEqual([
    expect.objectContaining({ kind: "run.terminated", lifecycle: "cancelled" }),
  ]);
  expect(replay.events.filter((event) =>
    event.kind === "tool.terminal" && event.toolCallId === backgroundTask)).toEqual([
    expect.objectContaining({
      kind: "tool.terminal",
      status: "cancelled",
      origin: "north",
      errorCode: "cancelled",
    }),
  ]);
});

test("a provider teardown that never settles is force-closed behind a durable terminal", async () => {
  const f = await fixture(undefined, undefined, undefined, undefined, true, 25);
  const terminate = await client(f.socketPath, {
    op: "terminateSession", executionId: f.executionId,
  });
  const deadline = Promise.withResolvers<never>();
  const timer = setTimeout(
    () => deadline.reject(new Error("terminate control exceeded its bounded teardown deadline")),
    500,
  );
  try { await Promise.race([terminate.closed, deadline.promise]); }
  finally { clearTimeout(timer); }

  expect(terminate.messages.at(-1)).toEqual({
    type: "error", message: "provider session teardown timed out",
  });
  expect(f.session.effects).toEqual(["terminate", "force-terminate"]);
  await f.launched.closed;

  const replay = await readBridgeWireJournal(f.journalRoot, f.executionId);
  const terminals = replay.events.filter((event) => event.kind === "run.terminated");
  expect(terminals).toHaveLength(1);
  expect(terminals[0]).toMatchObject({
    kind: "run.terminated",
    lifecycle: "failed",
    reason: { code: "provider_error" },
  });
  const failures = scanJournalFile(
    join(f.journalRoot, f.executionId, "events.log"), f.executionId,
  ).records.filter((record) => record.kind === "execution.failure");
  expect(failures).toEqual([
    expect.objectContaining({
      data: expect.objectContaining({
        code: "provider_teardown_failed",
        classification: "provider_teardown_timeout",
        phase: "provider_teardown",
      }),
    }),
  ]);
});
