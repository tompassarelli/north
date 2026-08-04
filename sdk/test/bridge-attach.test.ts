import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
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

function fixture(): { root: string; socketPath: string; journalRoot: string } {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-host-"));
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  return { root, socketPath: join(root, "northd.sock"), journalRoot: join(root, "journal") };
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

function providerSession(
  stream: (terminated: Promise<void>) => AsyncGenerator<NormalizedProviderEvent>,
): BridgeProviderExecution {
  return {
    async open() {
      let terminate!: () => void;
      const terminated = new Promise<void>((resolve) => { terminate = resolve; });
      const session: BridgeProviderSession = {
        async submitInput() {},
        async interruptTurn() {},
        async terminateSession() { terminate(); },
        events: () => stream(terminated),
      };
      return session;
    },
  };
}

test("attach replays a committed prefix, emits its barrier, then tails later records", async () => {
  const paths = fixture();
  let releaseLive!: () => void;
  const liveGate = new Promise<void>((resolve) => { releaseLive = resolve; });
  const provider = providerSession(async function* (terminated) {
    yield { kind: "assistant", data: { text: "history" } };
    await liveGate;
    yield { kind: "assistant", data: { text: "live" } };
    yield { kind: "result", data: { result: "done" }, turnTerminal: true };
    await terminated;
  });
  const northd = new Northd({ ...paths, provider });
  await northd.listen();
  cleanups.push(() => northd.close());

  const launched = await client(paths.socketPath, { op: "launch", prompt: "test", cwd: paths.root });
  await waitFor(
    () => launched.messages.some((message) => message.record?.data?.text === "history"),
    "first provider event",
  );
  const executionId = launched.messages.find((message) => message.type === "launched").executionId;
  launched.socket.destroy();
  await launched.closed;

  const attached = await client(paths.socketPath, { op: "attach", executionId, cursor: 0 });
  await waitFor(() => attached.messages.some((message) => message.type === "barrier"), "attach barrier");
  const barrierIndex = attached.messages.findIndex((message) => message.type === "barrier");
  const replay = attached.messages.slice(0, barrierIndex).filter((message) => message.type === "event");
  expect(replay.map((message) => message.record.seq)).toEqual([1, 2, 3]);
  expect(replay.at(-1).record.data.text).toBe("history");

  releaseLive();
  await waitFor(
    () => attached.messages.some((message) => message.record?.kind === "session.idle"),
    "idle session",
  );
  expect(attached.messages.find((message) => message.record?.kind === "session.idle")?.record.data)
    .toEqual({ armed: true, disposition: "completed", pendingInputs: 0 });
  const liveIndex = attached.messages.findIndex((message) => message.record?.data?.text === "live");
  expect(liveIndex).toBeGreaterThan(barrierIndex);
  expect(attached.messages[liveIndex].record.seq).toBe(4);
  expect(attached.socket.destroyed).toBe(false);
  attached.socket.destroy();
  await attached.closed;
});

test("a restarted northd replays terminal history from the journal without provider or Fram", async () => {
  const paths = fixture();
  const provider = providerSession(async function* (terminated) {
    yield { kind: "assistant", data: { text: "journal only" } };
    yield { kind: "result", data: { result: "done" }, turnTerminal: true };
    await terminated;
  });
  const first = new Northd({ ...paths, provider });
  await first.listen();
  const launched = await client(paths.socketPath, { op: "launch", prompt: "offline", cwd: paths.root });
  await waitFor(
    () => launched.messages.some((message) => message.record?.kind === "session.idle"),
    "idle session",
  );
  const executionId = launched.messages.find((message) => message.type === "launched").executionId;
  const terminated = await client(paths.socketPath, { op: "terminateSession", executionId });
  await terminated.closed;
  await launched.closed;
  const original = launched.messages
    .filter((message) => message.type === "event")
    .map((message) => message.record);
  expect(original.find((record) => record.kind === "session.idle")?.data)
    .toEqual({ armed: true, disposition: "completed", pendingInputs: 0 });
  await first.close();

  let providerCalls = 0;
  const replayOnly: BridgeProviderExecution = {
    async open() { providerCalls += 1; throw new Error("provider must stay down during replay"); },
  };
  const restarted = new Northd({ ...paths, provider: replayOnly });
  await restarted.listen();
  cleanups.push(() => restarted.close());
  const attached = await client(paths.socketPath, { op: "attach", executionId, cursor: 0 });
  await attached.closed;
  const replayed = attached.messages
    .filter((message) => message.type === "event")
    .map((message) => message.record);

  expect(replayed).toEqual(original);
  expect(providerCalls).toBe(0);
  expect(attached.messages.at(-1)).toMatchObject({
    type: "barrier", executionId, cursor: original.at(-1).seq,
  });
});
