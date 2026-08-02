import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import { scanJournalFile } from "../src/bridge/journal";
import type {
  BridgeProviderExecution, BridgeProviderSession, NormalizedProviderEvent,
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

  constructor(private readonly onEffect?: (effect: string) => void) {}

  private effect(value: string): void {
    this.effects.push(value);
    this.onEffect?.(value);
  }

  async submitInput(input: string): Promise<void> { this.effect(`submit:${input}`); }
  async interruptTurn(): Promise<void> { this.effect("interrupt"); }
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

async function fixture(onEffect?: (effect: string) => void) {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-controls-"));
  const socketPath = join(root, "northd.sock");
  const journalRoot = join(root, "journal");
  const session = new ControlledSession(onEffect);
  let opens = 0;
  const provider: BridgeProviderExecution = {
    async open() { opens += 1; return session; },
  };
  const northd = new Northd({ socketPath, journalRoot, provider });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());
  const launched = await client(socketPath, { op: "launch", prompt: "first", cwd: root });
  await waitFor(() => launched.messages.some((message) => message.type === "launched"), "launch id");
  const executionId = launched.messages.find((message) => message.type === "launched").executionId;
  return { root, socketPath, journalRoot, session, launched, executionId, opens: () => opens };
}

test("queued input becomes a second turn on the same provider session", async () => {
  const f = await fixture();
  const steer = await client(f.socketPath, {
    op: "submitInput", executionId: f.executionId, input: "second",
  });
  await steer.closed;
  expect(steer.messages.at(-1)).toMatchObject({
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

  const steer = await client(f.socketPath, {
    op: "submitInput", executionId: f.executionId, input: "next",
  });
  await steer.closed;
  f.session.settle("next done");
  await waitFor(
    () => f.launched.messages.filter((message) => message.record?.kind === "session.idle").length === 2,
    "second idle boundary",
  );

  const terminate = await client(f.socketPath, { op: "terminateSession", executionId: f.executionId });
  await terminate.closed;

  expect(observed.find(({ effect }) => effect === "interrupt")?.kinds.at(-1))
    .toBe("control.interrupt_turn");
  expect(observed.find(({ effect }) => effect === "submit:next")?.kinds.at(-1))
    .toBe("control.submit_input");
  expect(observed.find(({ effect }) => effect === "terminate")?.kinds.at(-1))
    .toBe("control.terminate_session");
});
