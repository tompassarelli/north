import { spawn } from "node:child_process";
import { connect, type Socket } from "node:net";
import { resolve } from "node:path";
import {
  bridgeSocketPath, parseBridgeLaunchRole, type BridgeLaunchRole, type BridgeRequest,
} from "./protocol";
import type { JournalRecord, TornTail } from "./journal";
import { markLaneConsumed, pendingLanes, type PendingLane } from "./pending";

type ServerMessage =
  | { type: "launched"; executionId: string }
  | { type: "controlled"; executionId: string; control: string; delivery: string }
  | { type: "event"; record: JournalRecord }
  | { type: "barrier"; executionId: string; cursor: number; tornTail?: TornTail }
  | { type: "error"; message: string };

function usage(): never {
  console.error(
    "usage: north bridge [app|tui] [--view-id ID]  (bare `north bridge` opens the app)"
    + " | north bridge [--role director|implementer] <prompt>"
    + " | north bridge dashboard [--once] [--ids] | north bridge accept"
    + " | north bridge pending [--json | --consume <execution-id>]"
    + " | north bridge attach <execution-id> [--cursor N]"
    + " | north bridge steer <execution-id> <text> | north bridge interrupt <execution-id>"
    + "\nlaunch role defaults to implementer",
  );
  process.exit(2);
}

export function parseBridgeLaunchArguments(args: string[]): {
  role: BridgeLaunchRole;
  promptArguments: string[];
} {
  if (args[0] !== "--role") return { role: "implementer", promptArguments: args };
  if (args.length < 2) throw new Error("bridge --role requires director or implementer");
  return {
    role: parseBridgeLaunchRole(args[1]),
    promptArguments: args.slice(2),
  };
}

async function runApp(args: string[]): Promise<number> {
  let viewId: string | undefined;
  if (args.length) {
    if (args.length !== 2 || args[0] !== "--view-id" || !args[1]) usage();
    viewId = args[1];
  }
  process.env.NORTH_BIN ??= resolve(import.meta.dir, "../../../bin/north");
  const appModule = new URL("./generated/north/bridge/app.js", import.meta.url).href;
  const { run_northbridge_app_bang } = await import(appModule) as {
    run_northbridge_app_bang(options: { viewId?: string }): Promise<unknown>;
  };
  await run_northbridge_app_bang({ viewId });
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
    const socket = connect(path);
    const onError = (error: Error) => { socket.destroy(); reject(error); };
    socket.once("error", onError);
    socket.once("connect", () => {
      socket.off("error", onError);
      resolveSocket(socket);
    });
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
  if (args.length === 0 || args[0] === "app" || args[0] === "tui") return runApp(args.slice(1));
  if (args[0] === "dashboard") return runDashboard(args.slice(1));
  if (args[0] === "pending") return runPending(args.slice(1));
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
  } else if (args[0] === "steer") {
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
    request = { op: "launch", prompt, cwd: process.cwd(), role: launch.role };
  }
  const socket = await connectedSocket(bridgeSocketPath());
  return runClient(socket, request);
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
