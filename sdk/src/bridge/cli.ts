import { spawn } from "node:child_process";
import { Socket } from "node:net";
import { resolve } from "node:path";
import { acquireFileLease } from "../file-lease";
import {
  "run-northbridge-app!" as runNorthbridgeApp,
} from "./generated/north/bridge/app.js";
import {
  prepareManagedBridgeAppLaunch,
  type ManagedBridgeAppLaunch,
} from "./app-launch-reservation";
import { runBridgeAcceptance } from "./accept";
import type { WireEvent } from "../wire/events";
import {
  bridgeSocketPath, bridgeSourceIdentity, parseBridgeLaunchEffort,
  parseBridgeLaunchAttemptId,
  parseBridgeLaunchModel, parseBridgeLaunchProvider, parseBridgeLaunchRole,
  parseBridgeLaunchTier, pinningExecutions,
  type BridgeLaunchSelection,
  type BridgeHello, type BridgeLaunchProvider, type BridgeLaunchRole, type BridgeRequest,
  type BridgeServerMessage,
} from "./protocol";
import type { JournalRecord } from "./journal";
import { markLaneConsumed, pendingLanes, type PendingLane } from "./pending";

export interface BridgeLaunchArguments extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  attemptId: string;
  promptArguments: string[];
}

export interface BridgeAppLaunchArguments extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  promptArguments: string[];
  selectedThreadId: string;
}

interface ParsedBridgeRouteArguments extends BridgeLaunchSelection {
  role: BridgeLaunchRole;
  promptArguments: string[];
  attemptId?: string;
}

function usage(): never {
  console.error(
    "usage: north bridge [app|tui] [route flags] [--view-id ID]  (opens the app)"
    + " | north bridge --attempt @attempt:<sha256> [--role director|implementer] [route flags] <prompt>"
    + " | north bridge dashboard [--once] [--ids]"
    + " | north bridge accept <messaged-attempt-id> <interrupted-attempt-id>"
    + " | north bridge restart  (retire the control daemon now)"
    + " | north bridge pending [--json | --consume <execution-id>]"
    + " | north bridge attach <execution-id> [--cursor N]"
    + " | north bridge msg <execution-id> <text> | north bridge interrupt <execution-id>"
    + "\nroute flags: --provider anthropic|openai | --claude | --openai"
    + " --tier economy|standard|senior|frontier --model ID"
    + " --effort low|medium|high|xhigh|max"
    + "\nlaunch requires a reserved attempt id; role defaults to implementer",
  );
  process.exit(2);
}

function parseBridgeRouteArguments(
  args: string[],
  attemptMode: "required" | "forbidden",
): ParsedBridgeRouteArguments {
  let role: BridgeLaunchRole = "implementer";
  let provider: BridgeLaunchProvider | undefined;
  let tier: BridgeLaunchSelection["tier"];
  let model: string | undefined;
  let effort: BridgeLaunchSelection["effort"];
  let attemptId: string | undefined;
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
    if (argument === "--attempt") {
      if (attemptMode === "forbidden") {
        throw new Error("bridge app-launch reserves its own attempt");
      }
      if (index + 1 >= args.length) {
        throw new Error("bridge --attempt requires a canonical reserved attempt id");
      }
      attemptId = parseBridgeLaunchAttemptId(args[index + 1]);
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
    if (["--provider", "--tier", "--model", "--effort"].includes(argument ?? "")) {
      if (index + 1 >= args.length) throw new Error(`bridge ${argument} requires a value`);
      const value = args[index + 1];
      if (argument === "--provider") provider = parseBridgeLaunchProvider(value);
      else if (argument === "--tier") tier = parseBridgeLaunchTier(value);
      else if (argument === "--model") model = parseBridgeLaunchModel(value);
      else effort = parseBridgeLaunchEffort(value);
      index += 2;
      continue;
    }
    break;
  }
  if (attemptMode === "required" && attemptId === undefined) {
    throw new Error("bridge launch requires --attempt with a reserved attempt id");
  }
  return {
    role,
    ...(attemptId ? { attemptId } : {}),
    ...(provider ? { provider } : {}),
    ...(tier ? { tier } : {}),
    ...(model ? { model } : {}),
    ...(effort ? { effort } : {}),
    promptArguments: args.slice(index),
  };
}

export function parseBridgeLaunchArguments(args: string[]): BridgeLaunchArguments {
  const parsed = parseBridgeRouteArguments(args, "required");
  if (!parsed.attemptId) {
    throw new Error("bridge launch requires --attempt with a reserved attempt id");
  }
  return { ...parsed, attemptId: parsed.attemptId };
}

export function parseBridgeAppLaunchArguments(
  args: string[],
): BridgeAppLaunchArguments {
  let selectedThreadId: string | undefined;
  const launchArguments: string[] = [];
  const valuedFlags = new Set(["--role", "--provider", "--tier", "--model", "--effort"]);
  let index = 0;
  while (index < args.length) {
    const argument = args[index]!;
    if (argument === "--attempt") {
      throw new Error("bridge app-launch reserves its own attempt");
    }
    if (argument === "--thread") {
      const value = args[index + 1];
      if (!value) throw new Error("bridge app-launch --thread requires an exact thread id");
      if (selectedThreadId !== undefined) {
        throw new Error("bridge app-launch accepts exactly one --thread");
      }
      selectedThreadId = value;
      index += 2;
      continue;
    }
    if (argument === "--claude" || argument === "--anthropic"
      || argument === "--openai" || argument === "--codex") {
      launchArguments.push(argument);
      index += 1;
      continue;
    }
    if (valuedFlags.has(argument)) {
      const value = args[index + 1];
      if (!value) throw new Error(`bridge app-launch ${argument} requires a value`);
      launchArguments.push(argument, value);
      index += 2;
      continue;
    }
    launchArguments.push(...args.slice(index));
    break;
  }
  if (!selectedThreadId) {
    throw new Error("bridge app-launch requires --thread with an exact Store thread id");
  }
  const parsed = parseBridgeRouteArguments(launchArguments, "forbidden");
  if (parsed.promptArguments.length === 0) {
    throw new Error("bridge app-launch requires a prompt");
  }
  return {
    role: parsed.role,
    promptArguments: parsed.promptArguments,
    selectedThreadId,
    ...(parsed.provider ? { provider: parsed.provider } : {}),
    ...(parsed.tier ? { tier: parsed.tier } : {}),
    ...(parsed.model ? { model: parsed.model } : {}),
    ...(parsed.effort ? { effort: parsed.effort } : {}),
  };
}

async function runApp(args: string[]): Promise<number> {
  let viewId: string | undefined;
  const rest: string[] = [];
  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index]!;
    if (argument === "--claude" || argument === "--anthropic") {
      process.env.NORTH_BRIDGE_PROVIDER = "anthropic";
      continue;
    }
    if (argument === "--openai" || argument === "--codex") {
      process.env.NORTH_BRIDGE_PROVIDER = "openai";
      continue;
    }
    if (["--provider", "--tier", "--model", "--effort"].includes(argument)) {
      const value = args[index + 1];
      if (!value) usage();
      if (argument === "--provider") {
        process.env.NORTH_BRIDGE_PROVIDER = parseBridgeLaunchProvider(value)!;
      } else if (argument === "--tier") {
        process.env.NORTH_BRIDGE_TIER = parseBridgeLaunchTier(value)!;
      } else if (argument === "--model") {
        process.env.NORTH_BRIDGE_MODEL = parseBridgeLaunchModel(value)!;
      } else {
        process.env.NORTH_BRIDGE_EFFORT = parseBridgeLaunchEffort(value)!;
      }
      index += 1;
      continue;
    }
    rest.push(argument);
  }
  if (rest.length) {
    if (rest.length !== 2 || rest[0] !== "--view-id" || !rest[1]) usage();
    viewId = rest[1];
  }
  process.env.NORTH_BIN ??= resolve(import.meta.dir, "../../../bin/north");
  // Opening the control TUI supersedes a control session left behind by the
  // previous checkout. Replace a stale daemon before drawing the app so the
  // first thing on screen is startup progress, never a terminal stale error.
  const connection = await verifiedSocket(
    bridgeSocketPath(), consoleBridgeConnectionOutput, { replacePinned: true },
  );
  connection.socket.destroy();
  // The checkout the app is running from, which is the same identity the
  // staleness handshake is fought over. The banner prints its short form, so
  // "which North Bridge am I looking at" is answerable from the screen.
  await runNorthbridgeApp({ viewId, sourceIdentity: bridgeSourceIdentity() });
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
  const result = Promise.withResolvers<number>();
  child.once("error", result.reject);
  child.once("exit", (code) => result.resolve(code ?? 1));
  return result.promise;
}

function openSocket(path: string): Promise<Socket> {
  const result = Promise.withResolvers<Socket>();
  // Listeners first, then connect: a missing or dead socket path can fail
  // during the connect call itself, and the error belongs to the returned
  // promise rather than the process error channel.
  const socket = new Socket();
  const onError = (error: Error) => { socket.destroy(); result.reject(error); };
  socket.once("error", onError);
  socket.once("connect", () => {
    socket.off("error", onError);
    result.resolve(socket);
  });
  socket.connect(path);
  return result.promise;
}

async function connectedSocket(path: string): Promise<Socket> {
  try { return await openSocket(path); }
  catch { /* one client starts the shared daemon below */ }
  const lease = await acquireFileLease(`${path}.launch.lock`);
  try {
    // A concurrent client may have started the daemon while this client waited
    // for launch ownership. Recheck before creating another detached process.
    try { return await openSocket(path); }
    catch { /* this client owns the launch */ }
    const northd = resolve(import.meta.dir, "northd.ts");
    const child = spawn(process.execPath, [northd], {
      detached: true,
      stdio: "ignore",
      env: process.env,
    });
    child.unref();
    let lastError: unknown;
    for (let attempt = 0; attempt < 100; attempt++) {
      try { return await openSocket(path); }
      catch (error) { lastError = error; await Bun.sleep(20); }
    }
    throw new Error(`northd did not open ${path}`, { cause: lastError });
  } finally {
    await lease.release();
  }
}

export function readHello(socket: Socket, timeoutMs: number): Promise<BridgeHello | null> {
  const result = Promise.withResolvers<BridgeHello | null>();
  let buffer = "";
  let finished = false;
  const finish = (value: BridgeHello | null) => {
    if (finished) return;
    finished = true;
    clearTimeout(timer);
    socket.off("data", onData);
    socket.off("close", onClose);
    socket.off("error", onError);
    result.resolve(value);
  };
  const onClose = () => finish(null);
  const onError = () => finish(null);
  const onData = (chunk: string) => {
    buffer += chunk;
    const newline = buffer.indexOf("\n");
    if (newline < 0) return;
    try {
      const message = JSON.parse(buffer.slice(0, newline)) as BridgeServerMessage;
      finish(message.type === "hello" ? message : null);
    } catch { finish(null); }
  };
  const timer = setTimeout(() => finish(null), timeoutMs);
  socket.setEncoding("utf8");
  socket.on("data", onData);
  socket.once("close", onClose);
  socket.once("error", onError);
  if (socket.destroyed || socket.readableEnded) finish(null);
  return result.promise;
}

function socketClosed(socket: Socket): Promise<void> {
  const result = Promise.withResolvers<void>();
  socket.once("close", () => result.resolve());
  return result.promise;
}

export interface BridgeConnection {
  socket: Socket;
  hello: BridgeHello | null;
}

export interface BridgeConnectionOutput {
  info(message: string): void;
  error(message: string): void;
}

export interface VerifiedSocketOptions {
  replacePinned?: boolean;
}

const consoleBridgeConnectionOutput: BridgeConnectionOutput = {
  info: (message) => console.log(message),
  error: (message) => console.error(message),
};

function shortIdentity(identity: string | undefined): string {
  return identity ? identity.slice(0, 8) : "unknown";
}

const DAEMON_RETIRE_TIMEOUT_MS = 5_000;

function processAlive(pid: number): boolean {
  try { process.kill(pid, 0); return true; }
  catch (error) { return (error as NodeJS.ErrnoException).code === "EPERM"; }
}

/**
 * The staleness contract, client side: never talk to a daemon whose source
 * identity differs from the checkout. A stale daemon nothing is depending on
 * gets replaced here and now — retire it, spawn its successor, say so once —
 * because the operator asked for a session, not for a chore. Only the sessions
 * that genuinely hold it open (attached control, or a worker with work in
 * flight) turn that into a refusal they have to answer.
 */
export async function verifiedSocket(
  path: string,
  output: BridgeConnectionOutput = consoleBridgeConnectionOutput,
  options: VerifiedSocketOptions = {},
): Promise<BridgeConnection> {
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
        output.info(`northd: control daemon was stale — replaced (${shortIdentity(replacedFrom)}`
          + ` → ${shortIdentity(hello.identity)}); starting fresh`);
      return { socket, hello };
    }
    const pinning = hello === null ? 0 : pinningExecutions(hello);
    if (pinning > 0 && options.replacePinned !== true) {
      output.error(`north bridge: northd is stale with ${pinning} live session(s);`
        + " run 'north bridge restart' to replace it now, or new launches are refused"
        + " until it drains");
      return { socket, hello };
    }
    if (hello !== null) {
      replacedFrom = hello.identity;
      replaced = true;
      const closed = socketClosed(socket);
      socket.write(`${JSON.stringify({ op: "retire" })}\n`);
      await closed;
      if (!await daemonRetired(path, hello.pid)) {
        throw new Error(`northd ${hello.pid} did not finish retirement`);
      }
    } else {
      socket.destroy();
      if (attempt === 2) {
        output.error("north bridge: northd did not present the identity handshake;"
          + " reap the orphan with: pkill -f bridge/northd");
      }
      await Bun.sleep(50);
    }
  }
  throw new Error("northd did not present a fresh identity after replacement");
}

async function daemonRetired(path: string, retiringPid: number): Promise<boolean> {
  const deadline = Date.now() + DAEMON_RETIRE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (retiringPid !== process.pid && !processAlive(retiringPid)) return true;
    let socket: Socket;
    try { socket = await openSocket(path); }
    catch {
      // A detached daemon may release its listener before process shutdown has
      // released every process-owned resource. Hosted tests run in this client
      // process; for them, listener teardown is the completion boundary.
      if (retiringPid === process.pid) return true;
      await Bun.sleep(20);
      continue;
    }
    const hello = await readHello(socket, 100);
    socket.destroy();
    if (hello !== null && hello.pid !== retiringPid
      && (retiringPid === process.pid || !processAlive(retiringPid))) return true;
    await Bun.sleep(20);
  }
  return false;
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
    if (hello === null) {
      socket.destroy();
      console.error("north bridge: northd predates the identity handshake;"
        + " reap the orphan with: pkill -f bridge/northd");
      return 1;
    }
    retiredFrom = hello?.identity;
    const closed = socketClosed(socket);
    socket.write(`${JSON.stringify({ op: "retire" })}\n`);
    await closed;
    const gone = await daemonRetired(path, hello.pid);
    if (!gone) {
      console.error(`north bridge: the control daemon is still listening at ${path}`);
      return 1;
    }
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
  const data = ` ${JSON.stringify({ ...record.data, bridgeRecordAt: record.at })}`;
  return `[${record.seq}] ${record.kind}${data}`;
}

export function renderWireEvent(event: WireEvent): string {
  return `[${event.sequence + 1}] ${event.kind} ${JSON.stringify(event)}`;
}

interface BridgeClientOutcome {
  code: number;
  launched: boolean;
}

interface BridgeClientHooks {
  onDurableWireEvent?(event: WireEvent): void | Promise<void>;
}

function runClient(
  socket: Socket,
  request: BridgeRequest,
  hooks: BridgeClientHooks = {},
): Promise<BridgeClientOutcome> {
  const result = Promise.withResolvers<BridgeClientOutcome>();
  let buffer = "";
  let exitCode = 0;
  let launched = false;
  let observationTail = Promise.resolve();
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      const line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (!line) continue;
      const message = JSON.parse(line) as BridgeServerMessage;
      if (message.type === "hello") continue;
      if (message.type === "launched") {
        launched = true;
        console.log(`execution ${message.executionId}`);
      }
      else if (message.type === "controlled")
        console.log(`${message.executionId} ${message.delivery}`);
      else if (message.type === "event") console.log(renderRecord(message.record));
      else if (message.type === "wire") {
        console.log(renderWireEvent(message.event));
        observationTail = observationTail.then(async () => {
          await hooks.onDurableWireEvent?.(message.event);
        }).catch((error) => {
          console.error(
            `north bridge: ${error instanceof Error ? error.message : "wire settlement failed"}`,
          );
          exitCode = 1;
        });
      }
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
  socket.once("close", () => {
    void observationTail.then(() => result.resolve({ code: exitCode, launched }));
  });
  socket.write(`${JSON.stringify(request)}\n`);
  return result.promise;
}

export async function settleManagedAppLaunchClose(
  managed: ManagedBridgeAppLaunch,
  launched: boolean,
): Promise<void> {
  if (!launched && !managed.providerEffectObserved && !managed.settled) {
    await managed.proveUnsent("daemon-launch-refused");
  }
}

async function runManagedAppLaunch(launch: BridgeAppLaunchArguments): Promise<number> {
  let managed: ManagedBridgeAppLaunch | undefined;
  try {
    const prompt = launch.promptArguments.join(" ").trim();
    managed = await prepareManagedBridgeAppLaunch({
      role: launch.role,
      prompt,
      cwd: process.cwd(),
      selectedThreadId: launch.selectedThreadId,
      ...(launch.provider ? { provider: launch.provider } : {}),
      ...(launch.tier ? { tier: launch.tier } : {}),
      ...(launch.model ? { model: launch.model } : {}),
      ...(launch.effort ? { effort: launch.effort } : {}),
    });
    let socket: Socket;
    try {
      socket = (await verifiedSocket(bridgeSocketPath())).socket;
    } catch (error) {
      await managed.proveUnsent("daemon-not-contacted");
      throw error;
    }
    const outcome = await runClient(socket, {
      op: "launch",
      executionId: managed.executionId,
      attemptId: managed.attemptId,
      prompt,
      cwd: process.cwd(),
      role: launch.role,
      provider: managed.provider,
      model: managed.model,
      ...(launch.tier ? { tier: launch.tier } : {}),
      ...(launch.effort ? { effort: launch.effort } : {}),
    }, {
      onDurableWireEvent: (event) => managed!.observeDurableWireEvent(event),
    });
    await settleManagedAppLaunchClose(managed, outcome.launched);
    if (outcome.launched && !managed.settled) {
      console.error("north bridge: app launch stream closed before durable attempt settlement");
      return 1;
    }
    return outcome.code;
  } catch (error) {
    console.error(`north bridge: ${error instanceof Error ? error.message : "app launch failed"}`);
    return 1;
  }
}

async function main(args: string[]): Promise<number> {
  // A provider pin with no prompt is still an app launch: it selects the
  // supervisor the app will start, not a one-shot turn.
  const appFlags = new Set([
    "--claude", "--anthropic", "--openai", "--codex", "--view-id",
    "--provider", "--tier", "--model", "--effort",
  ]);
  const valuedAppFlags = new Set(["--view-id", "--provider", "--tier", "--model", "--effort"]);
  if (args.length === 0 || args[0] === "app" || args[0] === "tui")
    return runApp(args[0] === "app" || args[0] === "tui" ? args.slice(1) : args);
  if (args.every((argument, index) => appFlags.has(argument)
    || (index > 0 && valuedAppFlags.has(args[index - 1]!))))
    return runApp(args);
  if (args[0] === "dashboard") return runDashboard(args.slice(1));
  if (args[0] === "pending") return runPending(args.slice(1));
  if (args[0] === "restart") {
    if (args.length !== 1) usage();
    return runBridgeRestart(bridgeSocketPath());
  }
  if (args[0] === "accept") {
    if (args.length !== 3) usage();
    try {
      await runBridgeAcceptance({
        attemptIds: [
          parseBridgeLaunchAttemptId(args[1]),
          parseBridgeLaunchAttemptId(args[2]),
        ],
      });
      return 0;
    }
    catch { return 1; }
  }
  if (args[0] === "app-launch") {
    try {
      return await runManagedAppLaunch(parseBridgeAppLaunchArguments(args.slice(1)));
    } catch (error) {
      console.error(`north bridge: ${error instanceof Error ? error.message : "app launch failed"}`);
      return 1;
    }
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
    let launch: BridgeLaunchArguments;
    try { launch = parseBridgeLaunchArguments(args); }
    catch { usage(); }
    let prompt = launch.promptArguments.join(" ").trim();
    if (!prompt && !process.stdin.isTTY) prompt = (await Bun.stdin.text()).trim();
    if (!prompt) usage();
    request = {
      op: "launch", prompt, cwd: process.cwd(), role: launch.role,
      attemptId: launch.attemptId,
      ...(launch.provider ? { provider: launch.provider } : {}),
      ...(launch.tier ? { tier: launch.tier } : {}),
      ...(launch.model ? { model: launch.model } : {}),
      ...(launch.effort ? { effort: launch.effort } : {}),
    };
  }
  const { socket } = await verifiedSocket(bridgeSocketPath());
  return (await runClient(socket, request)).code;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
