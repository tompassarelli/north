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

async function fixture(
  identity: () => string | undefined,
  open?: () => Promise<BridgeProviderSession>,
) {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-staleness-"));
  const socketPath = join(root, "northd.sock");
  let opens = 0;
  const provider: BridgeProviderExecution = {
    async open() { opens += 1; return open ? open() : new IdleSession(); },
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
  // `live` counts the sessions actually pinning the daemon, never the refused
  // launch itself, so a client can name why the daemon has not retired yet.
  expect(failed.record.data).toEqual({
    message: "bridge_daemon_source_stale", loaded: "rev-a", disk: "rev-b", live: 1,
  });
  expect(openCount()).toBe(1);

  const launchedId = live.messages.find((message) => message.type === "launched").executionId;
  await client(socketPath, { op: "terminateSession", executionId: launchedId });
  await waitFor(() => retireCount() === 1, "retirement after termination");
});

test("every connection opens with an identity hello", async () => {
  const { socketPath } = await fixture(() => "rev-a");
  const session = await client(socketPath, { op: "attach", executionId: "missing", cursor: 0 });
  await waitFor(() => session.messages.length > 0, "hello");
  const hello = session.messages[0];
  expect(hello.type).toBe("hello");
  expect(hello.identity).toBe("rev-a");
  expect(hello.liveExecutions).toBe(0);
  expect(hello.pid).toBe(process.pid);
  session.socket.destroy();
});

async function liveExecutions(socketPath: string): Promise<number> {
  const probe = await client(socketPath, { op: "attach", executionId: "missing", cursor: 0 });
  await waitFor(() => probe.messages.length > 0, "hello");
  const hello = probe.messages[0];
  probe.socket.destroy();
  await probe.closed;
  expect(hello.type).toBe("hello");
  return hello.liveExecutions;
}

// Tonight's real failures — anthropic_harness_authority_seal_missing,
// bwrap_executable_unavailable — all land here: the provider refuses before
// there is anything to drive. An execution that never started must not be one
// of the sessions the daemon is waiting on.
test("a session whose provider refuses at admit stops holding the daemon open", async () => {
  let disk = "rev-a";
  const { socketPath, retireCount } = await fixture(() => disk, async () => {
    throw new Error("bwrap_executable_unavailable");
  });
  const launched = await client(socketPath, { op: "launch", prompt: "go", cwd: "/" });
  await waitFor(
    () => launched.messages.some((message) =>
      message.type === "event" && message.record.kind === "execution.failed"),
    "provider refusal",
  );
  expect(await liveExecutions(socketPath)).toBe(0);

  // And with nothing live, the checkout moving under the daemon retires it —
  // the idle-stale path a phantom session used to hold shut forever.
  disk = "rev-b";
  await waitFor(() => retireCount() === 1, "retirement after a refused session");
  await waitFor(() => !existsSync(socketPath), "socket teardown");
});

// The failure the daemon cannot even write down: the provider's error is larger
// than a journal record, so the terminal record fails too. Liveness follows the
// provider, never the journal, or an execution nobody can finish pins the
// daemon against replacement.
test("a failure the journal cannot record still releases the daemon", async () => {
  let disk = "rev-a";
  const { socketPath, retireCount } = await fixture(() => disk, async () => {
    throw new Error("x".repeat(9 * 1024 * 1024));
  });
  const launched = await client(socketPath, { op: "launch", prompt: "go", cwd: "/" });
  await waitFor(
    () => launched.messages.some((message) => message.type === "error"),
    "unjournalable failure reported to the client",
  );
  expect(await liveExecutions(socketPath)).toBe(0);
  launched.socket.destroy();

  disk = "rev-b";
  await waitFor(() => retireCount() === 1, "retirement after an unjournalable failure");
});

test("retire op retires an idle daemon on demand", async () => {
  const { socketPath, retireCount } = await fixture(() => "rev-a");
  const session = await client(socketPath, { op: "retire" });
  await session.closed;
  const accepted = session.messages.find((message) => message.type === "controlled");
  expect(accepted.control).toBe("retire");
  await waitFor(() => retireCount() === 1, "retire on demand");
});
