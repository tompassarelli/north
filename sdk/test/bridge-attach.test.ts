import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import {
  readBridgeWireJournal,
} from "../src/bridge/journal";
import type { BridgeProviderExecution } from "../src/bridge/provider";
import type { BridgeServerMessage } from "../src/bridge/protocol";
import { BridgeWireTestSession } from "./support/bridge-wire-session";

interface Client {
  socket: Socket;
  messages: BridgeServerMessage[];
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

function launchedId(connection: Client): string {
  const message = connection.messages.find((candidate) => candidate.type === "launched");
  if (!message || message.type !== "launched") throw new Error("launch response missing");
  return message.executionId;
}

test("attach maps its one-based cursor to exact zero-based wire replay, then tails live events", async () => {
  const paths = fixture();
  const releaseLive = Promise.withResolvers<void>();
  const provider: BridgeProviderExecution = {
    async open(context) {
      const session = new BridgeWireTestSession(context, { initialAssistant: "history" });
      void (async () => {
        await releaseLive.promise;
        session.assistant("live");
        session.complete("done");
      })();
      return session;
    },
  };
  const northd = new Northd({ ...paths, provider });
  await northd.listen();
  cleanups.push(() => northd.close());

  const launched = await client(paths.socketPath, { op: "launch", prompt: "test", cwd: paths.root });
  await waitFor(
    () => launched.messages.some((message) =>
      message.type === "wire"
      && message.event.kind === "message.recorded"
      && message.event.stage === "delta"),
    "first wire message",
  );
  const executionId = launchedId(launched);
  launched.socket.destroy();
  await launched.closed;

  const attached = await client(paths.socketPath, { op: "attach", executionId, cursor: 0 });
  await waitFor(() => attached.messages.some((message) => message.type === "barrier"), "attach barrier");
  const barrierIndex = attached.messages.findIndex((message) => message.type === "barrier");
  const replay = attached.messages.slice(0, barrierIndex)
    .filter((message) => message.type === "wire")
    .map((message) => message.type === "wire" ? message.event : undefined)
    .filter((event) => event !== undefined);
  expect(replay.map((event) => event.sequence)).toEqual(replay.map((_, index) => index));
  const barrier = attached.messages[barrierIndex];
  expect(barrier?.type).toBe("barrier");
  if (!barrier || barrier.type !== "barrier") throw new Error("barrier missing");
  expect(barrier.cursor).toBe(replay.length);

  releaseLive.resolve();
  await waitFor(
    () => attached.messages.some((message) =>
      message.type === "event" && message.record.kind === "session.idle"),
    "idle session",
  );
  const idle = attached.messages.find((message) =>
    message.type === "event" && message.record.kind === "session.idle");
  expect(idle?.type === "event" ? idle.record.data : undefined).toMatchObject({
    armed: true,
    disposition: "completed",
    pendingInputs: 0,
  });
  const liveIndex = attached.messages.findIndex((message) =>
    message.type === "wire"
    && message.event.kind === "message.recorded"
    && message.event.stage === "delta"
    && message.event.content === "live");
  expect(liveIndex).toBeGreaterThan(barrierIndex);
  expect(attached.socket.destroyed).toBe(false);
  attached.socket.destroy();
  await attached.closed;
});

test("a restarted northd replays terminal wire bytes without provider or Fram", async () => {
  const paths = fixture();
  const provider: BridgeProviderExecution = {
    async open(context) {
      const session = new BridgeWireTestSession(context, { initialAssistant: "journal only" });
      session.complete("done");
      return session;
    },
  };
  const first = new Northd({ ...paths, provider });
  await first.listen();
  const launched = await client(paths.socketPath, { op: "launch", prompt: "offline", cwd: paths.root });
  await waitFor(
    () => launched.messages.some((message) =>
      message.type === "event" && message.record.kind === "session.idle"),
    "idle session",
  );
  const executionId = launchedId(launched);
  const terminated = await client(paths.socketPath, { op: "terminateSession", executionId });
  await terminated.closed;
  await launched.closed;
  const original = launched.messages
    .filter((message) => message.type === "wire")
    .map((message) => message.type === "wire" ? message.event : undefined)
    .filter((event) => event !== undefined);
  expect(original.at(-1)?.kind).toBe("run.terminated");
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
    .filter((message) => message.type === "wire")
    .map((message) => message.type === "wire" ? message.event : undefined)
    .filter((event) => event !== undefined);

  expect(replayed).toEqual(original);
  expect(providerCalls).toBe(0);
  expect(attached.messages.at(-1)).toMatchObject({
    type: "barrier", executionId, cursor: original.length,
  });
});

test("restart recovery closes an exact incomplete run once under concurrent and repeated attach", async () => {
  const paths = fixture();
  const child = Bun.spawn([
    process.execPath,
    join(import.meta.dir, "fixtures/bridge-crash-northd.ts"),
    paths.socketPath,
    paths.journalRoot,
    paths.root,
  ], {
    stdout: "pipe",
    stderr: "inherit",
  });
  cleanups.push(() => {
    if (child.exitCode === null) child.kill("SIGKILL");
  });
  const output = (child.stdout as ReadableStream<Uint8Array>).getReader();
  const ready = await output.read();
  output.releaseLock();
  expect(new TextDecoder().decode(ready.value)).toContain("ready");
  const launched = await client(paths.socketPath, {
    op: "launch",
    prompt: "crash before provider completion",
    cwd: paths.root,
  });
  await waitFor(
    () => launched.messages.some((message) =>
      message.type === "wire" && message.event.kind === "tool.admitted"),
    "crash fixture tool admission",
  );
  const executionId = launchedId(launched);
  child.kill("SIGKILL");
  await child.exited;
  await launched.closed;

  const crashed = await readBridgeWireJournal(paths.journalRoot, executionId);
  const crashedPrefix = crashed.events;
  const modelStarted = crashedPrefix.find((event) => event.kind === "model-call.started");
  const toolAdmitted = crashedPrefix.find((event) => event.kind === "tool.admitted");
  if (!modelStarted || modelStarted.kind !== "model-call.started") {
    throw new Error("crash prefix has no model call");
  }
  if (!toolAdmitted || toolAdmitted.kind !== "tool.admitted") {
    throw new Error("crash prefix has no tool admission");
  }
  const modelCallId = modelStarted.modelCallId;
  const toolCallId = toolAdmitted.toolCallId;
  expect(await Bun.file(`${join(paths.journalRoot, executionId, "wire.jsonl")}.lock`).exists())
    .toBe(true);

  let providerCalls = 0;
  const unavailableProvider: BridgeProviderExecution = {
    async open() {
      providerCalls += 1;
      throw new Error("provider must not reopen during crash recovery");
    },
  };
  const first = new Northd({ ...paths, provider: unavailableProvider });
  await first.listen();
  cleanups.push(() => first.close());
  const [left, right] = await Promise.all([
    client(paths.socketPath, { op: "attach", executionId, cursor: 0 }),
    client(paths.socketPath, { op: "attach", executionId, cursor: 0 }),
  ]);
  await Promise.all([left.closed, right.closed]);

  const recovered = await readBridgeWireJournal(paths.journalRoot, executionId);
  expect(recovered.events.slice(0, crashedPrefix.length)).toEqual(crashedPrefix);
  expect(recovered.events.filter((event) =>
    event.kind === "tool.terminal" && event.toolCallId === toolCallId)).toEqual([
    expect.objectContaining({
      kind: "tool.terminal",
      status: "synthetic_failure",
      origin: "north",
      errorCode: "provider_process_died",
    }),
  ]);
  expect(recovered.events.filter((event) =>
    event.kind === "model-call.completed" && event.modelCallId === modelCallId)).toEqual([
    expect.objectContaining({
      kind: "model-call.completed",
      status: "failed",
      origin: "north",
      errorCode: "provider_process_died",
    }),
  ]);
  expect(recovered.events.filter((event) => event.kind === "run.terminated")).toEqual([
    expect.objectContaining({
      kind: "run.terminated",
      lifecycle: "failed",
      reason: { code: "provider_process_died" },
    }),
  ]);
  const leftWire = left.messages
    .filter((message) => message.type === "wire")
    .map((message) => message.type === "wire" ? message.event : undefined)
    .filter((event) => event !== undefined);
  const rightWire = right.messages
    .filter((message) => message.type === "wire")
    .map((message) => message.type === "wire" ? message.event : undefined)
    .filter((event) => event !== undefined);
  expect(leftWire).toEqual(recovered.events);
  expect(rightWire).toEqual(recovered.events);
  expect(providerCalls).toBe(0);

  await first.close();
  const second = new Northd({ ...paths, provider: unavailableProvider });
  await second.listen();
  cleanups.push(() => second.close());
  const repeated = await client(paths.socketPath, { op: "attach", executionId, cursor: 0 });
  await repeated.closed;
  const replayed = repeated.messages
    .filter((message) => message.type === "wire")
    .map((message) => message.type === "wire" ? message.event : undefined)
    .filter((event) => event !== undefined);
  expect(replayed).toEqual(recovered.events);
  expect((await readBridgeWireJournal(paths.journalRoot, executionId)).events)
    .toEqual(recovered.events);
  expect(providerCalls).toBe(0);
});
