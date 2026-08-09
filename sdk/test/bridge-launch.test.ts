import { afterEach, beforeEach, expect, test } from "bun:test";
import {
  chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readHello, runBridgeRestart, verifiedSocket } from "../src/bridge/cli";
import { Northd } from "../src/bridge/host";
import {
  bridgeJournalRoot, bridgeSocketPath, bridgeSourceIdentity,
} from "../src/bridge/protocol";
import type {
  BridgeProviderExecution, BridgeProviderSession, NormalizedProviderEvent,
} from "../src/bridge/provider";

// The whole daemon lifecycle, through the entrypoints an operator reaches: the
// client spawns a real northd process, the source identity moves under it, and
// the next connect must reach a daemon built from the checkout on disk — idle,
// pinned by a failed session, or pinned by a live one and forced out by
// `north bridge restart`. Identity is fixtured at its input (a stand-in git
// that prints a chosen revision) rather than by moving the checkout.

const REV_A = "a".repeat(40);
const REV_B = "b1".repeat(20);
const REV_C = "c2".repeat(20);

const cleanups: Array<() => Promise<void> | void> = [];
let root: string;
let socketPath: string;
let revisionFile: string;
const previous = new Map<string, string | undefined>();

function environment(key: string, value: string): void {
  if (!previous.has(key)) previous.set(key, process.env[key]);
  process.env[key] = value;
}

function revision(value: string): void {
  writeFileSync(revisionFile, `${value}\n`);
}

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "north-bridge-launch-"));
  revisionFile = join(root, "revision");
  revision(REV_A);
  const git = join(root, "git");
  writeFileSync(git, `#!/bin/sh\nexec cat ${JSON.stringify(revisionFile)}\n`);
  chmodSync(git, 0o755);
  environment("NORTH_GIT_BIN", git);
  environment("NORTH_BRIDGE_STATE_DIR", join(root, "state"));
  socketPath = bridgeSocketPath();
  // Never the operator's own daemon: everything below spawns real processes.
  expect(socketPath.startsWith(root)).toBe(true);
  expect(bridgeSourceIdentity()).toBe(REV_A);
});

afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
  for (const [key, value] of previous) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  previous.clear();
  rmSync(root, { recursive: true, force: true });
});

function alive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch { return false; }
}

/**
 * Daemons this test spawned are real OS processes; none may outlive it. A
 * hosted daemon answers with this process's own pid and is closed, not killed.
 */
function reap(pid: number): void {
  if (pid === process.pid) return;
  cleanups.push(() => { try { process.kill(pid, "SIGKILL"); } catch { /* gone */ } });
}

async function waitFor(condition: () => boolean, label: string): Promise<void> {
  const deadline = Date.now() + 5_000;
  while (!condition() && Date.now() < deadline) await Bun.sleep(10);
  if (!condition()) throw new Error(`timed out waiting for ${label}`);
}

/** The client path an operator takes: connect, handshake, keep or replace. */
async function connectVerified(): Promise<{ pid: number; identity?: string; live: number }> {
  const { socket, hello } = await verifiedSocket(socketPath);
  socket.destroy();
  if (hello === null) throw new Error("northd answered no hello");
  reap(hello.pid);
  return { pid: hello.pid, identity: hello.identity, live: hello.liveExecutions };
}

async function launch(prompt: string): Promise<{ messages: any[]; socket: Socket }> {
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
  socket.write(`${JSON.stringify({ op: "launch", prompt, cwd: root })}\n`);
  cleanups.push(() => { socket.destroy(); });
  return { messages, socket };
}

class IdleSession implements BridgeProviderSession {
  terminated = false;
  private wake?: () => void;

  async submitInput(): Promise<void> {}
  async interruptTurn(): Promise<void> {}
  async terminateSession(): Promise<void> {
    this.terminated = true;
    this.wake?.();
    this.wake = undefined;
  }

  async *events(): AsyncIterable<NormalizedProviderEvent> {
    while (!this.terminated) await new Promise<void>((resolve) => { this.wake = resolve; });
  }
}

/**
 * A daemon on the same socket the real one uses, with a provider under test
 * control. The identity still comes from disk, so it goes stale exactly the way
 * the spawned process does.
 */
async function hostedDaemon(open: () => Promise<BridgeProviderSession>) {
  let retired = 0;
  let closed = false;
  // Teardown runs exactly once, the way the daemon entrypoint guards it: a
  // second close never completes, and cleanup must not wait on it.
  const close = async () => {
    if (closed) return;
    closed = true;
    await northd.close();
  };
  const northd = new Northd({
    socketPath,
    journalRoot: bridgeJournalRoot(),
    provider: { async open() { return open(); } } satisfies BridgeProviderExecution,
    onRetire: () => { retired += 1; void close(); },
  });
  await northd.listen();
  cleanups.push(close);
  return { northd, retireCount: () => retired };
}

test("a first connect spawns a real northd and reaches a fresh identity", async () => {
  expect(existsSync(socketPath)).toBe(false);
  const daemon = await connectVerified();
  expect(daemon.identity).toBe(REV_A);
  expect(daemon.live).toBe(0);
  expect(daemon.pid).not.toBe(process.pid);
  expect(alive(daemon.pid)).toBe(true);
  expect(existsSync(socketPath)).toBe(true);

  // A second connect is the same daemon: freshness is not a reason to respawn.
  expect((await connectVerified()).pid).toBe(daemon.pid);
});

test("source movement replaces an idle daemon at the next connect", async () => {
  const stale = await connectVerified();
  revision(REV_B);

  const fresh = await connectVerified();
  expect(fresh.identity).toBe(REV_B);
  expect(fresh.pid).not.toBe(stale.pid);
  await waitFor(() => !alive(stale.pid), "the stale daemon to exit");
});

test("a session that failed at admit does not pin a stale daemon", async () => {
  // Take the socket back from any spawned daemon so the provider is ours.
  expect(await runBridgeRestart(socketPath)).toBe(0);
  const hosted = await hostedDaemon(async () => {
    throw new Error("anthropic_harness_authority_seal_missing");
  });
  const failed = await launch("supervisor");
  await waitFor(
    () => failed.messages.some((message) =>
      message.type === "event" && message.record.kind === "execution.failed"),
    "the admit failure",
  );
  expect((await connectVerified()).live).toBe(0);

  // The phantom is gone, so the moved checkout replaces this daemon at the
  // handshake instead of waiting on a session that will never drain.
  revision(REV_B);
  const fresh = await connectVerified();
  expect(hosted.retireCount()).toBe(1);
  expect(fresh.identity).toBe(REV_B);
  expect(fresh.pid).not.toBe(process.pid);
});

test("a live session pins a stale daemon until restart retires it", async () => {
  expect(await runBridgeRestart(socketPath)).toBe(0);
  const session = new IdleSession();
  const hosted = await hostedDaemon(async () => session);
  const live = await launch("stay");
  await waitFor(
    () => live.messages.some((message) => message.type === "launched"),
    "the live session",
  );
  revision(REV_C);

  const warnings: string[] = [];
  const consoleError = console.error;
  console.error = (line: string) => { warnings.push(String(line)); };
  let pinned: Awaited<ReturnType<typeof connectVerified>>;
  try { pinned = await connectVerified(); }
  finally { console.error = consoleError; }
  // Tolerated, not replaced — and the message names the way out.
  expect(pinned.identity).toBe(REV_A);
  expect(pinned.live).toBe(1);
  expect(warnings.join("\n")).toContain("north bridge restart");
  expect(hosted.retireCount()).toBe(0);

  expect(await runBridgeRestart(socketPath)).toBe(0);
  expect(hosted.retireCount()).toBe(1);
  expect(session.terminated).toBe(true);
  expect(existsSync(socketPath)).toBe(false);

  const fresh = await connectVerified();
  expect(fresh.identity).toBe(REV_C);
  expect(fresh.live).toBe(0);
});

test("restart on a socket nobody is listening at starts nothing", async () => {
  expect(existsSync(socketPath)).toBe(false);
  expect(await runBridgeRestart(socketPath)).toBe(0);
  expect(existsSync(socketPath)).toBe(false);
});

test("/restart is in both command sets and runs the restart verb", async () => {
  const marker = join(root, "restart-argv");
  const north = join(root, "north");
  writeFileSync(
    north,
    `#!/bin/sh\nprintf '%s ' "$@" > ${JSON.stringify(marker)}\n`
    + "echo 'control daemon retired; the next north bridge command starts a fresh one'\n",
  );
  chmodSync(north, 0o755);
  // NORTH_BIN is read once when the app module loads, so it is pinned first.
  environment("NORTH_BIN", north);
  const app = await import("../src/bridge/generated/north/bridge/app.js") as {
    handle_local_command_bang(runtime: unknown, ui: unknown, input: string): boolean;
    palette_options(frame: string, query: string): Array<{ name: string }>;
  };
  for (const frame of ["agents", "threads"]) {
    expect(app.palette_options(frame, "/restart").map((option) => option.name))
      .toEqual(["/restart"]);
  }

  const runtime = {
    disposed: false,
    conversation: [] as Array<{ body: string }>,
    itemSequence: 0,
    render() {},
  };
  expect(app.handle_local_command_bang(runtime, {}, "/restart")).toBe(true);
  await waitFor(() => existsSync(marker), "the restart verb to run");
  expect(readFileSync(marker, "utf8").trim()).toBe("bridge restart");
  await waitFor(() => runtime.conversation.length > 0, "the restart line");
  expect(runtime.conversation[0]!.body).toContain("control daemon retired");
});

test("the hello a spawned daemon presents is its own identity and pid", async () => {
  const daemon = await connectVerified();
  const socket = connect(socketPath);
  await new Promise<void>((resolve, reject) => {
    socket.once("connect", resolve);
    socket.once("error", reject);
  });
  const hello = await readHello(socket, 2_000);
  socket.destroy();
  expect(hello).not.toBeNull();
  expect(hello!.type).toBe("hello");
  expect(hello!.identity).toBe(bridgeSourceIdentity());
  expect(hello!.pid).toBe(daemon.pid);
});
