import { afterEach, expect, test } from "bun:test";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
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

class IdleSession implements BridgeProviderSession {
  private ended = false;
  private wake?: () => void;

  async submitInput(): Promise<void> {}
  async interruptTurn(): Promise<void> {}
  async terminateSession(): Promise<void> {
    this.ended = true;
    this.wake?.();
    this.wake = undefined;
  }

  async *events(): AsyncIterable<NormalizedProviderEvent> {
    while (!this.ended) await new Promise<void>((resolve) => { this.wake = resolve; });
  }
}

async function fixture(identity: () => string | undefined) {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-staleness-"));
  const socketPath = join(root, "northd.sock");
  let opens = 0;
  const provider: BridgeProviderExecution = {
    async open() { opens += 1; return new IdleSession(); },
  };
  let retired = 0;
  const northd = new Northd({
    socketPath,
    journalRoot: join(root, "journal"),
    provider,
    sourceIdentity: identity,
    stalePollMs: 20,
    onRetire: () => { retired += 1; void northd.close(); },
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());
  return {
    socketPath,
    northd,
    openCount: () => opens,
    retireCount: () => retired,
  };
}

test("a fresh idle daemon never retires", async () => {
  const { retireCount } = await fixture(() => "rev-a");
  await Bun.sleep(120);
  expect(retireCount()).toBe(0);
});

test("an unknown identity disarms the watchdog and admits launches", async () => {
  const { socketPath, openCount, retireCount } = await fixture(() => undefined);
  const launched = await client(socketPath, { op: "launch", prompt: "go", cwd: "/" });
  await waitFor(() => openCount() === 1, "provider open");
  await Bun.sleep(120);
  expect(retireCount()).toBe(0);
  launched.socket.destroy();
});

test("a stale idle daemon retires and releases its socket", async () => {
  let disk = "rev-a";
  const { socketPath, retireCount } = await fixture(() => disk);
  disk = "rev-b";
  await waitFor(() => retireCount() === 1, "retirement");
  await waitFor(() => !existsSync(socketPath), "socket teardown");
});

test("live executions pin a stale daemon and new launches fail explicitly", async () => {
  let disk = "rev-a";
  const { socketPath, openCount, retireCount } = await fixture(() => disk);
  const live = await client(socketPath, { op: "launch", prompt: "stay", cwd: "/" });
  await waitFor(() => openCount() === 1, "provider open");
  disk = "rev-b";
  await Bun.sleep(120);
  expect(retireCount()).toBe(0);

  const refused = await client(socketPath, { op: "launch", prompt: "again", cwd: "/" });
  await refused.closed;
  const failed = refused.messages.find((message) =>
    message.type === "event" && message.record.kind === "execution.failed");
  expect(failed.record.data).toEqual({
    message: "bridge_daemon_source_stale", loaded: "rev-a", disk: "rev-b",
  });
  expect(openCount()).toBe(1);

  const launchedId = live.messages.find((message) => message.type === "launched").executionId;
  await client(socketPath, { op: "terminateSession", executionId: launchedId });
  await waitFor(() => retireCount() === 1, "retirement after termination");
});
