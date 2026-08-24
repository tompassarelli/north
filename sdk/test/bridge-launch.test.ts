import { afterAll, afterEach, beforeEach, expect, test } from "bun:test";
import { spawn } from "node:child_process";
import {
  chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { createServer, Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readHello, runBridgeRestart, verifiedSocket } from "../src/bridge/cli";
import {
  "handle-local-command!" as handleLocalCommand,
  "palette-options" as paletteOptions,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "->Agent" as Agent,
  "make-model" as makeModel,
  "snapshot" as snapshot,
  "upsert-agent" as upsertAgent,
} from "../src/bridge/generated/north/bridge/model.js";
import { Northd } from "../src/bridge/host";
import { MemoryBridgeCommandReceipts } from "../src/bridge/command-receipts";
import {
  bridgeJournalRoot, bridgeSocketPath, bridgeSourceIdentity,
} from "../src/bridge/protocol";
import type {
  BridgeProviderExecution, BridgeProviderOpenContext, BridgeProviderSession,
} from "../src/bridge/provider";
import type { BridgeServerMessage } from "../src/bridge/protocol";
import { BridgeWireTestSession } from "./support/bridge-wire-session";

// The whole daemon lifecycle, through the entrypoints an operator reaches: the
// client spawns a real northd process, the source identity moves under it, and
// the next connect must reach a daemon built from the checkout on disk — idle,
// pinned by a failed session, or pinned by a live one and forced out by
// `north bridge restart`. Identity is fixtured at its input (a stand-in git
// that prints a chosen revision) rather than by moving the checkout.

const REV_A = "a".repeat(40);
const REV_B = "b1".repeat(20);
const REV_C = "c2".repeat(20);
const ATTEMPT_ID = `@attempt:${"a".repeat(64)}`;

const cleanups: Array<() => Promise<void> | void> = [];
let root: string;
let socketPath: string;
const previous = new Map<string, string | undefined>();

// The stand-in git is written once, before any test runs anything, and reads a
// fixed file. Writing an executable and exec'ing it moments later races the
// write out of the file system's hands often enough to make the identity probe
// fail intermittently, and a failed probe reads as "identity unknown" — which
// is the one answer that disarms the staleness handshake, so the flake looked
// like the daemon refusing to be replaced. The revision is data the script
// reads at run time; the script itself never changes.
const toolRoot = mkdtempSync(join(tmpdir(), "north-bridge-launch-tools-"));
const revisionFile = join(toolRoot, "revision");
const gitStandIn = join(toolRoot, "git");
writeFileSync(gitStandIn, `#!/bin/sh\nexec cat ${JSON.stringify(revisionFile)}\n`);
chmodSync(gitStandIn, 0o755);

function environment(key: string, value: string): void {
  if (!previous.has(key)) previous.set(key, process.env[key]);
  process.env[key] = value;
}

function revision(value: string): void {
  writeFileSync(revisionFile, `${value}\n`);
}

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "north-bridge-launch-"));
  revision(REV_A);
  environment("NORTH_GIT_BIN", gitStandIn);
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

afterAll(() => { rmSync(toolRoot, { recursive: true, force: true }); });

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

interface Wire {
  messages: BridgeServerMessage[];
  socket: Socket;
}

function wired(socket: Socket): Wire {
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
  cleanups.push(() => { socket.destroy(); });
  return { messages, socket };
}

async function opened(): Promise<Wire> {
  const socket = new Socket();
  const connected = Promise.withResolvers<void>();
  socket.once("connect", () => connected.resolve());
  socket.once("error", connected.reject);
  socket.connect(socketPath);
  await connected.promise;
  return wired(socket);
}

/** What the daemon says about itself, without asking the client to judge it. */
async function probe(): Promise<Probe> {
  const wire = await opened();
  await waitFor(() => wire.messages.length > 0, "hello");
  const hello = wire.messages[0];
  wire.socket.destroy();
  expect(hello.type).toBe("hello");
  return {
    identity: hello.identity,
    live: hello.liveExecutions,
    pinning: hello.pinningExecutions,
    pid: hello.pid,
  };
}

/**
 * Restart leaves a successor running, and a successor nobody read a hello from
 * is a process this suite would leak on every run. Ask who it is, then own it.
 */
async function restart(): Promise<number> {
  const code = await runBridgeRestart(socketPath);
  if (existsSync(socketPath)) reap((await probe()).pid);
  return code;
}

type Probe = { identity?: string; live: number; pinning: number; pid: number };

async function probeUntil(
  condition: (state: Probe) => boolean,
  label: string,
): Promise<Probe> {
  const deadline = Date.now() + 5_000;
  let state = await probe();
  while (!condition(state) && Date.now() < deadline) {
    await Bun.sleep(10);
    state = await probe();
  }
  if (!condition(state)) throw new Error(`timed out waiting for ${label}`);
  return state;
}

/** The client path an operator takes: connect, handshake, keep or replace. */
async function connectVerified(): Promise<{
  pid: number; identity?: string; live: number; socket: Socket;
}> {
  const { socket, hello } = await verifiedSocket(socketPath);
  cleanups.push(() => { socket.destroy(); });
  if (hello === null) throw new Error("northd answered no hello");
  reap(hello.pid);
  return { pid: hello.pid, identity: hello.identity, live: hello.liveExecutions, socket };
}

/** The provider is open and the daemon holds its session, not merely the launch. */
function started(wire: Wire): boolean {
  return wire.messages.some((message) =>
    message.type === "wire" && message.event.kind === "model-call.started");
}

async function launch(prompt: string, role = "implementer"): Promise<Wire> {
  const wire = await opened();
  wire.socket.write(`${JSON.stringify({
    op: "launch", prompt, cwd: root, role, attemptId: ATTEMPT_ID,
  })}\n`);
  return wire;
}

/** Everything the client says on each stream, so a calm line can be told from a chore. */
async function saying<T>(body: () => Promise<T>): Promise<{
  value: T; out: string; err: string;
}> {
  const out: string[] = [];
  const err: string[] = [];
  const consoleLog = console.log;
  const consoleError = console.error;
  console.log = (line: string) => { out.push(String(line)); };
  console.error = (line: string) => { err.push(String(line)); };
  try {
    const value = await body();
    return { value, out: out.join("\n"), err: err.join("\n") };
  } finally {
    console.log = consoleLog;
    console.error = consoleError;
  }
}

class IdleSession extends BridgeWireTestSession implements BridgeProviderSession {
  terminated = false;

  constructor(context: BridgeProviderOpenContext) {
    super(context);
  }

  async terminateSession(): Promise<void> {
    this.terminated = true;
    await super.terminateSession();
  }
}

class BlockingIdleSession extends IdleSession {
  readonly terminationStarted = Promise.withResolvers<void>();
  readonly releaseTermination = Promise.withResolvers<void>();

  async terminateSession(): Promise<void> {
    this.terminationStarted.resolve();
    await this.releaseTermination.promise;
    await super.terminateSession();
  }
}

/**
 * A daemon on the same socket the real one uses, with a provider under test
 * control. The identity still comes from disk, so it goes stale exactly the way
 * the spawned process does.
 */
async function hostedDaemon(
  open: (context: BridgeProviderOpenContext) => Promise<BridgeProviderSession>,
) {
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
    provider: { async open(context) { return open(context); } } satisfies BridgeProviderExecution,
    // Pinned, so a launch reaches its provider immediately: headroom selection
    // is a real probe with real latency, and a session still being selected is
    // a session this test would race.
    commandReceipts: new MemoryBridgeCommandReceipts([ATTEMPT_ID]),
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

test("a hello wait ends when its peer closes without answering", async () => {
  const path = join(root, "no-hello.sock");
  const server = createServer((socket) => { void Bun.sleep(10).then(() => socket.end()); });
  cleanups.push(() => {
    if (!server.listening) return;
    const closed = Promise.withResolvers<void>();
    server.close((error) => error ? closed.reject(error) : closed.resolve());
    return closed.promise;
  });
  const listening = Promise.withResolvers<void>();
  server.once("error", listening.reject);
  server.listen(path, listening.resolve);
  await listening.promise;
  const peer = new Socket();
  const connected = Promise.withResolvers<void>();
  peer.once("connect", connected.resolve);
  peer.once("error", connected.reject);
  peer.connect(path);
  await connected.promise;

  const outcome = await Promise.race([
    readHello(peer, 2_000),
    Bun.sleep(250).then(() => "still-waiting" as const),
  ]);
  expect(outcome).toBeNull();
  peer.destroy();
  const closed = Promise.withResolvers<void>();
  server.close((error) => error ? closed.reject(error) : closed.resolve());
  await closed.promise;
});

test("socket disappearance does not outrun retirement of its owning process", async () => {
  revision(REV_B);
  const fixture = join(import.meta.dir, "fixtures/bridge-retiring-northd.ts");
  const child = spawn(process.execPath, [fixture], {
    env: { ...process.env, NORTH_BRIDGE_RETIRE_DELAY_MS: "2200" },
    stdio: "ignore",
  });
  if (child.pid === undefined) throw new Error("retiring daemon fixture did not start");
  reap(child.pid);
  await waitFor(() => existsSync(socketPath), "fixture listener");

  const startedAt = Date.now();
  const fresh = await connectVerified();
  expect(Date.now() - startedAt).toBeGreaterThanOrEqual(2_000);
  expect(fresh.identity).toBe(REV_B);
  expect(fresh.pid).not.toBe(child.pid);
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
  const hosted = await hostedDaemon(async () => {
    throw new Error("anthropic_harness_authority_seal_missing");
  });
  const failed = await launch("supervisor");
  await waitFor(
    () => failed.messages.some((message) =>
      message.type === "event" && message.record.kind === "execution.failure"),
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

// The session an operator abandons by closing the window: still live, still
// driving a provider, and nobody attached to it. It must not cost them a daemon
// they cannot replace — this is the case that made the whole ceremony appear.
test("an abandoned control session is replaced in place, with one calm line", async () => {
  let session: IdleSession | undefined;
  const hosted = await hostedDaemon(async (context) => {
    session = new IdleSession(context);
    return session;
  });
  const control = await launch("supervisor", "director");
  await waitFor(() => started(control), "the control session");
  expect(await probe()).toMatchObject({ live: 1, pinning: 1 });

  // Closing the window is exactly this: the launch client goes away and the
  // session keeps running with nobody to drain for.
  control.socket.destroy();
  const abandoned = await probeUntil(
    (state) => state.pinning === 0, "the control session to read as abandoned",
  );
  expect(abandoned.live).toBe(1);

  revision(REV_B);
  const { value: fresh, out, err } = await saying(() => connectVerified());
  expect(hosted.retireCount()).toBe(1);
  await waitFor(() => session?.terminated === true, "the abandoned session to be torn down");
  expect(fresh.identity).toBe(REV_B);
  expect(fresh.pid).not.toBe(process.pid);
  // One line, on stdout, where the app reads it as a system note. Nothing on
  // stderr, and nothing for the operator to type.
  expect(out).toBe(
    `northd: control daemon was stale — replaced (${REV_A.slice(0, 8)}`
    + ` → ${REV_B.slice(0, 8)}); starting fresh`,
  );
  expect(err).toBe("");

  // And the connection the client handed back is the fresh daemon's, usable
  // without reconnecting: the replacement happened inside this one connect.
  const answered = wired(fresh.socket);
  fresh.socket.write(`${JSON.stringify({ op: "attach", executionId: "missing", cursor: 0 })}\n`);
  await waitFor(() => answered.messages.some((message) => message.type === "error"), "a live successor");
});

test("concurrent clients wait for stale-daemon teardown before sharing its successor", async () => {
  let session: BlockingIdleSession | undefined;
  const hosted = await hostedDaemon(async (context) => {
    session = new BlockingIdleSession(context);
    return session;
  });
  const control = await launch("supervisor", "director");
  await waitFor(() => started(control), "the control session");
  control.socket.destroy();
  await probeUntil((state) => state.pinning === 0, "the abandoned control session");
  if (!session) throw new Error("provider session missing");
  cleanups.push(() => session?.releaseTermination.resolve());

  revision(REV_B);
  const info: string[] = [];
  const errors: string[] = [];
  const output = {
    info(message: string) { info.push(message); },
    error(message: string) { errors.push(message); },
  };
  let firstResolved = false;
  let secondResolved = false;
  const first = verifiedSocket(socketPath, output).then((connection) => {
    firstResolved = true;
    return connection;
  });
  await session.terminationStarted.promise;
  const second = verifiedSocket(socketPath, output).then((connection) => {
    secondResolved = true;
    return connection;
  });
  await Bun.sleep(50);
  expect(firstResolved).toBe(false);
  expect(secondResolved).toBe(false);

  session.releaseTermination.resolve();
  const [one, two] = await Promise.all([first, second]);
  cleanups.push(() => one.socket.destroy());
  cleanups.push(() => two.socket.destroy());
  if (one.hello === null || two.hello === null) throw new Error("fresh hello missing");
  reap(one.hello.pid);
  reap(two.hello.pid);
  expect(one.hello.identity).toBe(REV_B);
  expect(two.hello.identity).toBe(REV_B);
  expect(one.hello.pid).toBe(two.hello.pid);
  expect(hosted.retireCount()).toBe(1);
  expect(errors).toEqual([]);
  expect(info).toHaveLength(2);
  expect(info.every((message) => message.includes("starting fresh"))).toBe(true);
});

test("an attached control session still pins a stale daemon", async () => {
  const hosted = await hostedDaemon(async (context) => new IdleSession(context));
  const control = await launch("supervisor", "director");
  await waitFor(() => started(control), "the control session");
  revision(REV_C);

  const { value: pinned, out, err } = await saying(() => connectVerified());
  expect(pinned.identity).toBe(REV_A);
  expect(hosted.retireCount()).toBe(0);
  expect(err).toContain("north bridge restart");
  expect(out).toBe("");
});

test("opening the control app replaces an attached stale control session", async () => {
  let session: IdleSession | undefined;
  const hosted = await hostedDaemon(async (context) => {
    session = new IdleSession(context);
    return session;
  });
  const control = await launch("supervisor", "director");
  await waitFor(() => started(control), "the control session");
  revision(REV_C);

  const { value: fresh, out, err } = await saying(() =>
    verifiedSocket(socketPath, undefined, { replacePinned: true }));
  expect(hosted.retireCount()).toBe(1);
  await waitFor(() => session?.terminated === true, "the stale control session to stop");
  if (fresh.hello === null) throw new Error("fresh daemon hello missing");
  cleanups.push(() => fresh.socket.destroy());
  reap(fresh.hello.pid);
  expect(fresh.hello.identity).toBe(REV_C);
  expect(fresh.hello.pid).not.toBe(process.pid);
  expect(out).toContain("starting fresh");
  expect(err).toBe("");
});

// Detaching from work in flight is the attach contract, not abandonment: a
// worker holds the daemon whether or not anyone is watching it.
test("a detached worker still pins a stale daemon", async () => {
  const hosted = await hostedDaemon(async (context) => new IdleSession(context));
  const worker = await launch("do the work", "implementer");
  await waitFor(() => started(worker), "the worker session");
  worker.socket.destroy();
  const detached = await probe();
  expect(detached.live).toBe(1);
  expect(detached.pinning).toBe(1);

  revision(REV_C);
  const { value: pinned, err } = await saying(() => connectVerified());
  expect(pinned.identity).toBe(REV_A);
  expect(hosted.retireCount()).toBe(0);
  expect(err).toContain("north bridge restart");
});

test("restart replaces a pinned daemon in place and leaves a live successor", async () => {
  let session: IdleSession | undefined;
  const hosted = await hostedDaemon(async (context) => {
    session = new IdleSession(context);
    return session;
  });
  const worker = await launch("stay");
  await waitFor(() => started(worker), "the live session");
  revision(REV_C);

  const { value: code, out } = await saying(() => restart());
  expect(code).toBe(0);
  expect(hosted.retireCount()).toBe(1);
  await waitFor(() => session?.terminated === true, "the pinned session to be torn down");
  // Replaced, not merely retired: the successor is already up and named.
  expect(out).toBe(`control daemon replaced (${REV_A.slice(0, 8)} → ${REV_C.slice(0, 8)})`);
  expect(existsSync(socketPath)).toBe(true);

  const fresh = await probe();
  expect(fresh.identity).toBe(REV_C);
  expect(fresh.live).toBe(0);
});

test("restart with nothing listening starts the daemon rather than a chore", async () => {
  expect(existsSync(socketPath)).toBe(false);
  const { value: code, out } = await saying(() => restart());
  expect(code).toBe(0);
  expect(out).toBe(`control daemon started (${REV_A.slice(0, 8)})`);
  const started = await probe();
  expect(started.identity).toBe(REV_A);
});

test("/restart is in both command sets and restores the session in place", async () => {
  const marker = join(root, "restart-argv");
  const north = join(root, "north");
  writeFileSync(
    north,
    `#!/bin/sh\nprintf '%s ' "$@" >> ${JSON.stringify(marker)}\nprintf '\\n' >> ${JSON.stringify(marker)}\n`
    + "echo 'control daemon replaced (aaaaaaaa → b1b1b1b1)'\n",
  );
  chmodSync(north, 0o755);
  environment("NORTH_BIN", north);
  for (const snapshot of ["agents", "threads"]) {
    expect(paletteOptions(snapshot, "/restart").map((option) => option.name))
      .toEqual(["/restart"]);
  }

  // A window that already has a control session: the row on the roster, the
  // execution the app routes messages to, and the id it calls its supervisor.
  const dead = "dead-control-session";
  const runtime = {
    disposed: false,
    conversation: [] as Array<{ body: string }>,
    itemSequence: 0,
    working: false,
    workingLabel: "",
    workingSince: 0,
    spinnerTimer: null,
    spinnerIndex: 0,
    agentIndex: 3,
    supervisorId: dead,
    controlThreadId: "bridge-control-thread",
    bridgeExecutions: new Set([dead]),
    model: upsertAgent(
      makeModel("list"),
      Agent(
        dead, "Main", "ready", "Northbridge control session",
        "", "", "", "", "", "", "", "", "",
      ),
    ),
    render() {},
    renderConversation() {},
  };
  expect(handleLocalCommand(runtime, {}, "/restart")).toBe(true);
  await waitFor(
    () => existsSync(marker) && readFileSync(marker, "utf8").includes("--role"),
    "the restart verb and the session it restores",
  );
  const argv = readFileSync(marker, "utf8").trim().split("\n").map((line) => line.trim());
  // Retire, then reopen the control session here rather than telling someone to.
  expect(argv[0]).toBe("bridge restart");
  expect(argv[1]).toContain(
    "bridge app-launch --thread bridge-control-thread --role director",
  );
  expect(runtime.conversation.map((item) => item.body))
    .toContain("control daemon replaced; session restored");

  // One Main, not a corpse beside its successor: the session that died with the
  // daemon leaves the roster, the routing set, and the supervisor binding.
  expect(snapshot(runtime.model).agents.map((agent) => agent.id)).toEqual([]);
  expect(runtime.bridgeExecutions.has(dead)).toBe(false);
  expect(runtime.supervisorId).toBe("");
  expect(runtime.agentIndex).toBe(0);
});

test("/mcp is discoverable from both bridge views", () => {
  for (const snapshot of ["agents", "threads"]) {
    expect(paletteOptions(snapshot, "/mcp").map((option) => option.name))
      .toEqual(["/mcp"]);
  }
});

test("the hello a spawned daemon presents is its own identity and pid", async () => {
  const daemon = await connectVerified();
  const wire = await opened();
  const hello = await readHello(wire.socket, 2_000);
  wire.socket.destroy();
  expect(hello).not.toBeNull();
  expect(hello!.type).toBe("hello");
  expect(hello!.identity).toBe(bridgeSourceIdentity());
  expect(hello!.pid).toBe(daemon.pid);
});
