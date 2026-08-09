import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import { scanJournalFile } from "../src/bridge/journal";
import {
  bridgeSystemPrompt, type BridgeProviderExecution, type BridgeProviderSession,
  type NormalizedProviderEvent,
} from "../src/bridge/provider";

interface Client {
  socket: Socket;
  messages: any[];
  closed: Promise<void>;
}

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

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
      messages.push(JSON.parse(buffer.slice(0, newline)));
      buffer = buffer.slice(newline + 1);
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

class ControlledSession implements BridgeProviderSession {
  readonly effects: string[] = [];
  private queue: NormalizedProviderEvent[] = [];
  private wake?: () => void;
  private ended = false;

  constructor(
    private readonly onEffect?: (effect: string) => void,
    private readonly interruptFailure?: Error,
  ) {}

  private effect(value: string): void {
    this.effects.push(value);
    this.onEffect?.(value);
  }

  async submitInput(input: string): Promise<void> { this.effect(`submit:${input}`); }
  async interruptTurn(): Promise<void> {
    this.effect("interrupt");
    if (this.interruptFailure) throw this.interruptFailure;
  }
  async terminateSession(): Promise<void> {
    if (this.ended) return;
    this.effect("terminate");
    this.ended = true;
    this.wake?.();
    this.wake = undefined;
  }

  settle(result: string): void {
    this.queue.push({ kind: "result", data: { result }, turnTerminal: true });
    this.wake?.();
    this.wake = undefined;
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

async function fixture(
  onEffect?: (effect: string) => void,
  role?: "director" | "implementer",
  interruptFailure?: Error,
) {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-controls-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  const session = new ControlledSession(onEffect, interruptFailure);
  let opens = 0;
  const openContexts: Array<Parameters<BridgeProviderExecution["open"]>[0]> = [];
  const provider: BridgeProviderExecution = {
    async open(context) { opens += 1; openContexts.push(context); return session; },
  };
  const northd = new Northd({ socketPath, journalRoot, provider });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());
  const launched = await client(socketPath, {
    op: "launch", prompt: "first", cwd: root, ...(role ? { role } : {}),
  });
  await waitFor(() => launched.messages.some((message) => message.type === "launched"), "launch id");
  const executionId = launched.messages.find((message) => message.type === "launched").executionId;
  return {
    root, socketPath, journalRoot, session, launched, executionId,
    opens: () => opens, openContexts,
  };
}

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

  f.session.settle("first done");
  await waitFor(() => f.session.effects.includes("submit:second"), "second turn submission");
  expect(f.opens()).toBe(1);
  f.session.settle("second done");
  await waitFor(
    () => f.launched.messages.filter((message) => message.record?.kind === "session.idle").length === 2,
    "second idle boundary",
  );
  expect(f.launched.messages
    .filter((message) => message.record?.kind === "session.idle")
    .map((message) => message.record.data)).toEqual([
    { armed: true, disposition: "completed", pendingInputs: 1 },
    { armed: true, disposition: "completed", pendingInputs: 0 },
  ]);
});

test("interrupt settles only the active turn and leaves the session attachable", async () => {
  const f = await fixture();
  const interrupt = await client(f.socketPath, { op: "interruptTurn", executionId: f.executionId });
  await interrupt.closed;
  expect(f.session.effects).toEqual(["interrupt"]);

  f.session.settle("interrupted");
  await waitFor(
    () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
    "idle boundary",
  );
  expect(f.launched.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
    .toEqual({ armed: true, disposition: "interrupted", pendingInputs: 0 });
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

  f.session.settle("interrupted");
  await waitFor(() => f.session.effects.length === 2, "replacement submission");
  expect(f.session.effects).toEqual(["interrupt", "submit:replacement"]);
  expect(f.launched.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
    .toEqual({ armed: true, disposition: "interrupted", pendingInputs: 1 });
});

test("a rejected interrupt cannot mislabel a later normal terminal boundary", async () => {
  for (const request of [
    { op: "interruptTurn" },
    { op: "redirectNow", input: "replacement" },
  ] as const) {
    const f = await fixture(undefined, undefined, new Error("interrupt rejected"));
    const control = await client(f.socketPath, { ...request, executionId: f.executionId });
    await control.closed;
    expect(control.messages.at(-1)).toEqual({ type: "error", message: "interrupt rejected" });

    f.session.settle("completed normally");
    await waitFor(
      () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
      "normal idle boundary",
    );
    expect(f.launched.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
      .toEqual({ armed: true, disposition: "completed", pendingInputs: 0 });
  }
});

test("each control command is committed to the journal before its provider effect", async () => {
  let journalPath = "";
  const observed: Array<{ effect: string; kinds: string[] }> = [];
  const f = await fixture((effect) => {
    const records = scanJournalFile(journalPath).records;
    observed.push({ effect, kinds: records.map((record) => record.kind) });
  });
  journalPath = join(f.journalRoot, f.executionId, "events.log");

  const interrupt = await client(f.socketPath, { op: "interruptTurn", executionId: f.executionId });
  await interrupt.closed;
  f.session.settle("interrupted");
  await waitFor(
    () => f.launched.messages.some((message) => message.record?.kind === "session.idle"),
    "idle boundary",
  );

  const msg = await client(f.socketPath, {
    op: "submitInput", executionId: f.executionId, input: "next",
  });
  await msg.closed;
  f.session.settle("next done");
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
});
