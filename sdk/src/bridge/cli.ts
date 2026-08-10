import { spawn } from "node:child_process";
import { unlinkSync } from "node:fs";
import { Socket } from "node:net";
import { resolve } from "node:path";
import {
  bridgeSocketPath, bridgeSourceIdentity, parseBridgeLaunchRole, pinningExecutions,
  type BridgeHello, type BridgeLaunchProvider, type BridgeLaunchRole,
  type BridgeRequest,
} from "./protocol";
import type { JournalRecord, TornTail } from "./journal";
import { markLaneConsumed, pendingLanes, type PendingLane } from "./pending";

type ServerMessage =
  | BridgeHello
  | { type: "launched"; executionId: string }
  | { type: "controlled"; executionId: string; control: string; delivery: string }
  | { type: "event"; record: JournalRecord }
  | { type: "barrier"; executionId: string; cursor: number; tornTail?: TornTail }
  | { type: "error"; message: string };

function usage(): never {
  console.error(
    "usage: north bridge [app|tui] [--claude|--openai] [--view-id ID]  (opens the app)"
    + " | north bridge [--role director|implementer] [--claude|--openai] <prompt>"
    + " | north bridge dashboard [--once] [--ids] | north bridge accept"
    + " | north bridge restart  (retire the control daemon now)"
    + " | north bridge pending [--json | --consume <execution-id>]"
    + " | north bridge attach <execution-id> [--cursor N]"
    + " | north bridge msg <execution-id> <text> | north bridge interrupt <execution-id>"
    + "\nlaunch role defaults to implementer",
  );
  process.exit(2);
}

export function parseBridgeLaunchArguments(args: string[]): {
  role: BridgeLaunchRole;
  provider?: BridgeLaunchProvider;
  promptArguments: string[];
} {
  let role: BridgeLaunchRole = "implementer";
  let provider: BridgeLaunchProvider | undefined;
  let index = 0;
  while (index < args.length) {
    const argument = args[index];
    if (argument === "--role") {
      if (index + 1 >= args.length)
        throw new Error("bridge --role requires director or implementer");
      role = parseBridgeLaunchRole(args[index + 1]);
      index += 2;
      continue;
    }
    if (argument === "--claude" || argument === "--anthropic") {
      provider = "anthropic";
      index += 1;
      continue;
    }
    if (argument === "--openai" || argument === "--codex") {
      provider = "openai";
      index += 1;
      continue;
    }
    break;
  }
  return { role, ...(provider ? { provider } : {}), promptArguments: args.slice(index) };
}

async function runApp(args: string[]): Promise<number> {
  let viewId: string | undefined;
  const rest: string[] = [];
  for (const argument of args) {
    if (argument === "--claude" || argument === "--anthropic") {
      process.env.NORTH_BRIDGE_PROVIDER = "anthropic";
      continue;
    }
    if (argument === "--openai" || argument === "--codex") {
      process.env.NORTH_BRIDGE_PROVIDER = "openai";
      continue;
    }
    rest.push(argument);
  }
  if (rest.length) {
    if (rest.length !== 2 || rest[0] !== "--view-id" || !rest[1]) usage();
    viewId = rest[1];
  }
  process.env.NORTH_BIN ??= resolve(import.meta.dir, "../../../bin/north");
  const appModule = new URL("./generated/north/bridge/app.js", import.meta.url).href;
  const { run_northbridge_app_bang } = await import(appModule) as {
    run_northbridge_app_bang(
      options: { viewId?: string; sourceIdentity?: string },
    ): Promise<unknown>;
  };
  // The checkout the app is running from, which is the same identity the
  // staleness handshake is fought over. The banner prints its short form, so
  // "which North Bridge am I looking at" is answerable from the screen.
  await run_northbridge_app_bang({ viewId, sourceIdentity: bridgeSourceIdentity() });
  return 0;
}

function pendingValue(record: JournalRecord | undefined, key: string): string | undefined {
  const value = record?.data[key];
  return typeof value === "string" && value ? value : undefined;
}

function renderPendingLane(lane: PendingLane): string {
  const processOutcome = pendingValue(lane.terminal, "processOutcome") ?? "unknown";
  const deliveryOutcome = pendingValue(lane.terminal, "deliveryOutcome") ?? "unknown";
  const branch = pendingValue(lane.harvest, "branch");
  const sha = pendingValue(lane.harvest, "sha");
  return [
    lane.executionId,
    `process=${processOutcome}`,
    `delivery=${deliveryOutcome}`,
    ...(branch ? [`branch=${branch}`] : []),
    ...(sha ? [`sha=${sha}`] : []),
  ].join(" ");
}

function runPending(args: string[]): number {
  if (args.length === 0) {
    for (const lane of pendingLanes()) console.log(renderPendingLane(lane));
    return 0;
  }
  if (args.length === 1 && args[0] === "--json") {
    console.log(JSON.stringify(pendingLanes()));
    return 0;
  }
  if (args.length === 2 && (args[0] === "--consume" || args[0] === "consume")) {
    const created = markLaneConsumed(args[1]!);
    console.log(`${created ? "consumed" : "already consumed"} ${args[1]}`);
    return 0;
  }
  usage();
}

function runDashboard(args: string[]): Promise<number> {
  const dashboard = resolve(import.meta.dir, "../../../cli/dashboard-cli.clj");
  const child = spawn(process.env.NORTH_BB ?? "bb", [dashboard, "dashboard", ...args], {
    stdio: "inherit",
    env: process.env,
  });
  return new Promise((resolveExit, reject) => {
    child.once("error", reject);
    child.once("exit", (code) => resolveExit(code ?? 1));
  });
}

function openSocket(path: string): Promise<Socket> {
  return new Promise((resolveSocket, reject) => {
    // Listeners first, then connect: a missing or dead socket path can fail
    // during the connect call itself, and an error emitted before anything is
    // listening is an uncaught error rather than this promise's rejection.
    const socket = new Socket();
    const onError = (error: Error) => { socket.destroy(); reject(error); };
    socket.once("error", onError);
    socket.once("connect", () => {
      socket.off("error", onError);
      resolveSocket(socket);
    });
    socket.connect(path);
  });
}

async function connectedSocket(path: string): Promise<Socket> {
  try { return await openSocket(path); }
  catch {
    const northd = resolve(import.meta.dir, "northd.ts");
    const child = spawn(process.execPath, [northd], {
      detached: true,
      stdio: "ignore",
      env: process.env,
    });
    child.unref();
  }
  let lastError: unknown;
  for (let attempt = 0; attempt < 100; attempt++) {
    try { return await openSocket(path); }
    catch (error) { lastError = error; await Bun.sleep(20); }
  }
  throw new Error(`northd did not open ${path}`, { cause: lastError });
}

export function readHello(socket: Socket, timeoutMs: number): Promise<BridgeHello | null> {
  return new Promise((resolveHello) => {
    let buffer = "";
    const finish = (value: BridgeHello | null) => {
      clearTimeout(timer);
      socket.off("data", onData);
      resolveHello(value);
    };
    const timer = setTimeout(() => finish(null), timeoutMs);
    const onData = (chunk: string) => {
      buffer += chunk;
      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      try {
        const message = JSON.parse(buffer.slice(0, newline)) as ServerMessage;
        finish(message.type === "hello" ? message : null);
      } catch { finish(null); }
    };
    socket.setEncoding("utf8");
    socket.on("data", onData);
  });
}

export interface BridgeConnection {
  socket: Socket;
  hello: BridgeHello | null;
}

function shortIdentity(identity: string | undefined): string {
  return identity ? identity.slice(0, 8) : "unknown";
}

/**
 * The staleness contract, client side: never talk to a daemon whose source
 * identity differs from the checkout. A stale daemon nothing is depending on
 * gets replaced here and now — retire it, spawn its successor, say so once —
 * because the operator asked for a session, not for a chore. Only the sessions
 * that genuinely hold it open (attached control, or a worker with work in
 * flight) turn that into a refusal they have to answer.
 */
export async function verifiedSocket(path: string): Promise<BridgeConnection> {
  let replacedFrom: string | undefined;
  let replaced = false;
  for (let attempt = 0; attempt < 3; attempt++) {
    const socket = await connectedSocket(path);
    const hello = await readHello(socket, 750);
    const disk = bridgeSourceIdentity();
    const fresh = hello !== null
      && (hello.identity === undefined || disk === undefined || hello.identity === disk);
    if (fresh) {
      // One calm line, on the stream the app reads as a system note: what
      // happened, which checkout won, and that the session is starting now.
      if (replaced)
        console.log(`northd: control daemon was stale — replaced (${shortIdentity(replacedFrom)}`
          + ` → ${shortIdentity(hello.identity)}); starting fresh`);
      return { socket, hello };
    }
    const pinning = hello === null ? 0 : pinningExecutions(hello);
    if (pinning > 0) {
      console.error(`north bridge: northd is stale with ${pinning} live session(s);`
        + " run 'north bridge restart' to replace it now, or new launches are refused"
        + " until it drains");
      return { socket, hello };
    }
    if (hello !== null) {
      replacedFrom = hello.identity;
      replaced = true;
      socket.write(`${JSON.stringify({ op: "retire" })}\n`);
      await new Promise<void>((resolveClose) => socket.once("close", () => resolveClose()));
    } else {
      socket.destroy();
      try { unlinkSync(path); } catch { /* replaced concurrently */ }
      console.error("north bridge: replacing a northd that predates the identity handshake;"
        + " reap the orphan with: pkill -f bridge/northd");
    }
    await Bun.sleep(50);
  }
  throw new Error("northd did not present a fresh identity after replacement");
}

async function daemonListening(path: string): Promise<boolean> {
  try {
    const socket = await openSocket(path);
    socket.destroy();
    return true;
  } catch { return false; }
}

/**
 * The deliberate force path, for the one case the handshake will not resolve on
 * its own: a daemon genuinely pinned by sessions that are still someone's. It
 * drains nothing — the daemon goes now — and it does not hand the operator a
 * chore afterwards: the successor is up before this returns, so a caller can
 * reopen its session immediately and in place.
 */
export async function runBridgeRestart(path: string): Promise<number> {
  let retiredFrom: string | undefined;
  let socket: Socket | undefined;
  try { socket = await openSocket(path); }
  catch { socket = undefined; }
  if (socket !== undefined) {
    const hello = await readHello(socket, 750);
    retiredFrom = hello?.identity;
    const closed = new Promise<void>((resolveClose) => socket.once("close", () => resolveClose()));
    socket.write(`${JSON.stringify({ op: "retire" })}\n`);
    await closed;
    let gone = false;
    for (let attempt = 0; attempt < 100 && !gone; attempt++) {
      if (await daemonListening(path)) await Bun.sleep(20);
      else gone = true;
    }
    if (!gone) {
      console.error(`north bridge: the control daemon is still listening at ${path}`);
      return 1;
    }
    // A daemon that predates the retire op answers nothing and leaves its
    // socket behind; the file is dead either way once nothing accepts on it.
    try { unlinkSync(path); } catch { /* already reaped */ }
  }
  const successor = await verifiedSocket(path);
  successor.socket.destroy();
  const now = shortIdentity(successor.hello?.identity);
  console.log(retiredFrom === undefined
    ? `control daemon started (${now})`
    : `control daemon replaced (${shortIdentity(retiredFrom)} → ${now})`);
  return 0;
}

function renderRecord(record: JournalRecord): string {
  const data = Object.keys(record.data).length ? ` ${JSON.stringify(record.data)}` : "";
  return `[${record.seq}] ${record.kind}${data}`;
}

function runClient(socket: Socket, request: BridgeRequest): Promise<number> {
  return new Promise((resolveExit) => {
    let buffer = "";
    let exitCode = 0;
    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      while (true) {
        const newline = buffer.indexOf("\n");
        if (newline < 0) break;
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (!line) continue;
        const message = JSON.parse(line) as ServerMessage;
        if (message.type === "hello") continue;
        if (message.type === "launched") console.log(`execution ${message.executionId}`);
        else if (message.type === "controlled")
          console.log(`${message.executionId} ${message.delivery}`);
        else if (message.type === "event") console.log(renderRecord(message.record));
        else if (message.type === "barrier") {
          console.log(`attached ${message.executionId} at ${message.cursor}`);
          if (message.tornTail) {
            console.error(
              `torn journal tail at byte ${message.tornTail.offset}: `
              + `${message.tornTail.availableBytes}/${message.tornTail.requiredBytes} bytes`,
            );
            exitCode = 1;
          }
        } else {
          console.error(`north bridge: ${message.message}`);
          exitCode = 1;
        }
      }
    });
    socket.once("error", (error) => {
      console.error(`north bridge: ${error.message}`);
      exitCode = 1;
    });
    socket.once("close", () => resolveExit(exitCode));
    socket.write(`${JSON.stringify(request)}\n`);
  });
}

async function main(args: string[]): Promise<number> {
  // A provider pin with no prompt is still an app launch: it selects the
  // supervisor the app will start, not a one-shot turn.
  const appFlags = new Set(["--claude", "--anthropic", "--openai", "--codex", "--view-id"]);
  if (args.length === 0 || args[0] === "app" || args[0] === "tui")
    return runApp(args[0] === "app" || args[0] === "tui" ? args.slice(1) : args);
  if (args.every((argument, index) =>
    appFlags.has(argument) || (index > 0 && args[index - 1] === "--view-id")))
    return runApp(args);
  if (args[0] === "dashboard") return runDashboard(args.slice(1));
  if (args[0] === "pending") return runPending(args.slice(1));
  if (args[0] === "restart") {
    if (args.length !== 1) usage();
    return runBridgeRestart(bridgeSocketPath());
  }
  if (args[0] === "accept") {
    if (args.length !== 1) usage();
    const { runBridgeAcceptance } = await import("./accept");
    try { await runBridgeAcceptance(); return 0; }
    catch { return 1; }
  }
  let request: BridgeRequest;
  if (args[0] === "attach") {
    const executionId = args[1];
    if (!executionId || (args.length !== 2 && args.length !== 4)) usage();
    let cursor = 0;
    if (args.length === 4) {
      if (args[2] !== "--cursor" || !/^[0-9]+$/.test(args[3]!)) usage();
      cursor = Number(args[3]);
      if (!Number.isSafeInteger(cursor)) usage();
    }
    request = { op: "attach", executionId, cursor };
  } else if (args[0] === "msg") {
    const executionId = args[1];
    const input = args.slice(2).join(" ").trim();
    if (!executionId || !input) usage();
    request = { op: "submitInput", executionId, input };
  } else if (args[0] === "interrupt") {
    const executionId = args[1];
    if (!executionId || args.length !== 2) usage();
    request = { op: "interruptTurn", executionId };
  } else {
    let launch: ReturnType<typeof parseBridgeLaunchArguments>;
    try { launch = parseBridgeLaunchArguments(args); }
    catch { usage(); }
    let prompt = launch.promptArguments.join(" ").trim();
    if (!prompt && !process.stdin.isTTY) prompt = (await Bun.stdin.text()).trim();
    if (!prompt) usage();
    request = {
      op: "launch", prompt, cwd: process.cwd(), role: launch.role,
      ...(launch.provider ? { provider: launch.provider } : {}),
    };
  }
  const { socket } = await verifiedSocket(bridgeSocketPath());
  return runClient(socket, request);
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
