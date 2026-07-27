import { spawn, type ChildProcess } from "node:child_process";
import {
  closeSync, copyFileSync, mkdirSync, openSync, readFileSync, writeFileSync,
} from "node:fs";
import { createServer, connect } from "node:net";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { expectedLog } from "./coord-wire";

export const FRAM_GRAPH_AUTHORING_CAPABILITY = "graph-authoring.fram" as const;
export const FRAM_MCP_SERVER = "fram" as const;
export const FRAM_MCP_TOOL_NAMES = Object.freeze([
  "tell",
  "retract",
  "show",
  "ask",
  "validate",
  "add-def",
  "set-body",
  "rename-def",
  "insert-after",
  "replace-in-body",
] as const);
export const FRAM_MCP_TOOLS = Object.freeze(
  FRAM_MCP_TOOL_NAMES.map((name) => `mcp__${FRAM_MCP_SERVER}__${name}`),
);

const MANAGED_LANE_DESCRIPTOR = "managed-code-coordinator.json";
const MANAGED_LANE_VERSION = 1;
const DEFAULT_BOOT_TIMEOUT_MS = 15 * 60_000;
const DEFAULT_TERM_MS = 2_000;
const DEFAULT_KILL_MS = 2_000;

interface ManagedFramLaneDescriptor {
  version: 1;
  active: boolean;
  sourceRoot: string;
  codeLog: string;
  codePort: string;
  canonicalSourceRoot: string;
  seededFrom: string;
  reboundTrackedPaths: number;
  pid: number;
}

export interface ManagedFramCoordinator {
  readonly sourceRoot: string;
  readonly codeLog: string;
  readonly codePort: string;
  readonly pid: number;
  close(): Promise<void>;
  forceClose(): void;
}

export interface PrepareManagedFramCoordinatorOptions {
  worktree: string;
  canonicalSourceRoot: string;
  bootTimeoutMs?: number;
  termMs?: number;
  killMs?: number;
  /** Hermetic unit seam; production owns the real port, child, and readiness probe. */
  runtime?: {
    allocatePort(): Promise<number>;
    launch(input: {
      framHome: string;
      codePort: string;
      codeLog: string;
      daemonLog: string;
    }): ChildProcess;
    ready(child: ChildProcess, port: number, log: string, timeoutMs: number): Promise<void>;
    waitForExit(child: ChildProcess, milliseconds: number): Promise<boolean>;
  };
}

// The fram and beagle roots are deployment facts, not source constants: the
// nix package purity gate rejects checkout paths baked into the installed
// CLI. Every path below derives lazily from these two roots, so composing
// the capability without them fails closed with a named error while every
// non-capability spawn never touches this resolution at all.
export function framGraphAuthoringRoots(): { framHome: string; beagleHome: string } {
  const framHome = process.env.NORTH_FRAM_HOME;
  const beagleHome = process.env.NORTH_BEAGLE_HOME;
  if (!framHome || !beagleHome) {
    const missing = [
      !framHome && "NORTH_FRAM_HOME",
      !beagleHome && "NORTH_BEAGLE_HOME",
    ].filter(Boolean).join(", ");
    throw new Error(
      "graph_authoring_fram_roots_unset: the graph-authoring.fram capability "
      + `requires NORTH_FRAM_HOME and NORTH_BEAGLE_HOME in the dispatching `
      + `environment (missing: ${missing})`,
    );
  }
  return { framHome: resolve(framHome), beagleHome: resolve(beagleHome) };
}

export function framMcpCommand(): string {
  return join(framGraphAuthoringRoots().framHome, "bin", "fram-mcp");
}

function descriptorPath(sourceRoot: string): string {
  return join(sourceRoot, ".fram", MANAGED_LANE_DESCRIPTOR);
}

function readManagedLaneDescriptor(sourceRoot: string): ManagedFramLaneDescriptor | undefined {
  let raw: string;
  try {
    raw = readFileSync(descriptorPath(sourceRoot), "utf8");
  } catch {
    return undefined;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (cause) {
    throw new Error("graph_authoring_fram_lane_descriptor_invalid: descriptor is not JSON", {
      cause,
    });
  }
  const value = parsed as Partial<ManagedFramLaneDescriptor>;
  const expectedLog = join(sourceRoot, ".fram", "code.log");
  if (value.version !== MANAGED_LANE_VERSION
      || value.active !== true
      || resolve(String(value.sourceRoot ?? "")) !== sourceRoot
      || resolve(String(value.codeLog ?? "")) !== expectedLog
      || typeof value.codePort !== "string"
      || !/^[1-9][0-9]*$/.test(value.codePort)
      || !Number.isSafeInteger(value.pid) || Number(value.pid) <= 1) {
    throw new Error(
      "graph_authoring_fram_lane_descriptor_invalid: managed worktree descriptor "
      + "does not name one active worktree-local coordinator",
    );
  }
  return value as ManagedFramLaneDescriptor;
}

function writeDescriptor(path: string, descriptor: ManagedFramLaneDescriptor): void {
  writeFileSync(path, `${JSON.stringify(descriptor, null, 2)}\n`, { mode: 0o600 });
}

function ignoreManagedFramState(sourceRoot: string): void {
  const info = join(sourceRoot, ".git", "info");
  const exclude = join(info, "exclude");
  mkdirSync(info, { recursive: true });
  let current = "";
  try { current = readFileSync(exclude, "utf8"); }
  catch { /* a fresh standalone clone may not have created it yet */ }
  if (current.split(/\r?\n/).includes("/.fram/")) return;
  const separator = current.length === 0 || current.endsWith("\n") ? "" : "\n";
  writeFileSync(exclude, `${current}${separator}/.fram/\n`);
}

/**
 * Seed from the canonical graph without re-parsing every Beagle module, then
 * rebind only the graph's tracked-view facts. Replacing the checkout prefix
 * globally would also rewrite ordinary source-string literals; restricting the
 * rewrite to `@module#root file` facts preserves the copied AST/history bytes.
 */
export function seedReboundFramCodeLog(options: {
  canonicalLog: string;
  laneLog: string;
  canonicalSourceRoot: string;
  laneSourceRoot: string;
}): { reboundTrackedPaths: number } {
  const canonicalRoot = resolve(options.canonicalSourceRoot);
  const laneRoot = resolve(options.laneSourceRoot);
  mkdirSync(resolve(options.laneLog, ".."), { recursive: true });
  copyFileSync(options.canonicalLog, options.laneLog);
  const copied = readFileSync(options.laneLog, "utf8");
  let reboundTrackedPaths = 0;
  const rebound = copied.replace(
    /(:l\s+"@[^"\n]+#root",\s+:p\s+"file",\s+:r\s+")([^"\n]+)(")/g,
    (line, prefix: string, trackedPath: string, suffix: string) => {
      if (trackedPath !== canonicalRoot && !trackedPath.startsWith(`${canonicalRoot}/`))
        return line;
      reboundTrackedPaths++;
      return `${prefix}${laneRoot}${trackedPath.slice(canonicalRoot.length)}${suffix}`;
    },
  );
  if (reboundTrackedPaths === 0) {
    throw new Error(
      "graph_authoring_fram_lane_seed_unrebound: canonical code log contains no "
      + `tracked source path under ${canonicalRoot}`,
    );
  }
  writeFileSync(options.laneLog, rebound);
  return { reboundTrackedPaths };
}

async function ephemeralPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  await new Promise<void>((resolveClose, reject) => {
    server.close((error) => error ? reject(error) : resolveClose());
  });
  if (!address || typeof address === "string" || !Number.isSafeInteger(address.port))
    throw new Error("graph_authoring_fram_lane_port_unresolved");
  return address.port;
}

function ednString(value: string): string {
  return JSON.stringify(value);
}

async function fencedVersion(port: number, log: string): Promise<boolean> {
  return new Promise<boolean>((resolveProbe) => {
    const socket = connect({ host: "127.0.0.1", port });
    let settled = false;
    let response = "";
    const settle = (value: boolean) => {
      if (settled) return;
      settled = true;
      socket.destroy();
      resolveProbe(value);
    };
    socket.setTimeout(1_000, () => settle(false));
    socket.once("error", () => settle(false));
    socket.on("data", (chunk) => {
      response += chunk.toString("utf8");
      if (response.includes("\n")) settle(/:version\s+[0-9]+/.test(response));
    });
    socket.once("connect", () => {
      socket.write(
        `{:op :for-log :expected-log ${ednString(log)} :request {:op :version}}\n`,
      );
    });
    socket.once("end", () => settle(/:version\s+[0-9]+/.test(response)));
  });
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
}

async function waitForCoordinator(
  child: ChildProcess,
  port: number,
  log: string,
  timeoutMs: number,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let backoff = 25;
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error(
        `graph_authoring_fram_lane_coordinator_exited: exit=${child.exitCode ?? "none"} `
        + `signal=${child.signalCode ?? "none"}`,
      );
    }
    if (await fencedVersion(port, log)) return;
    await delay(Math.min(backoff, Math.max(1, deadline - Date.now())));
    backoff = Math.min(500, backoff * 2);
  }
  throw new Error(
    `graph_authoring_fram_lane_coordinator_timeout: no fenced response on 127.0.0.1:${port}`,
  );
}

function waitForExit(child: ChildProcess, milliseconds: number): Promise<boolean> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
  return new Promise((resolveExit) => {
    let settled = false;
    const done = (result: boolean) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.off("exit", exited);
      child.off("error", exited);
      resolveExit(result);
    };
    const exited = () => done(true);
    const timer = setTimeout(() => done(false), milliseconds);
    child.once("exit", exited);
    child.once("error", exited);
  });
}

/**
 * Host-owned lifecycle for a lane's code coordinator. The daemon is a direct
 * child whose shell `exec`s the JVM, so reaping this PID reaps the listener.
 */
export async function prepareManagedFramCoordinator(
  options: PrepareManagedFramCoordinatorOptions,
): Promise<ManagedFramCoordinator> {
  const sourceRoot = resolve(options.worktree);
  const canonicalSourceRoot = resolve(options.canonicalSourceRoot);
  const { framHome } = framGraphAuthoringRoots();
  const canonicalLog = join(framHome, ".fram", "code.log");
  const localFram = join(sourceRoot, ".fram");
  const codeLog = join(localFram, "code.log");
  ignoreManagedFramState(sourceRoot);
  mkdirSync(localFram, { recursive: true });
  const { reboundTrackedPaths } = seedReboundFramCodeLog({
    canonicalLog,
    laneLog: codeLog,
    canonicalSourceRoot,
    laneSourceRoot: sourceRoot,
  });
  const runtime = options.runtime ?? {
    allocatePort: ephemeralPort,
    launch({ framHome, codePort, codeLog, daemonLog }: {
      framHome: string; codePort: string; codeLog: string; daemonLog: string;
    }): ChildProcess {
      const logFd = openSync(daemonLog, "a", 0o600);
      try {
        return spawn(
          join(framHome, "bin", "fram-daemon"),
          ["serve-flat", codePort, codeLog],
          {
            cwd: framHome,
            env: { ...process.env, FRAM_REQUIRE_LOG_FENCE: "1" },
            stdio: ["ignore", logFd, logFd],
          },
        );
      } finally {
        closeSync(logFd);
      }
    },
    ready: waitForCoordinator,
    waitForExit,
  };
  const codePort = String(await runtime.allocatePort());
  const daemonLog = join(localFram, `coord-${codePort}.log`);
  const child = runtime.launch({ framHome, codePort, codeLog, daemonLog });
  const pid = child.pid;
  if (!Number.isSafeInteger(pid) || pid === undefined || pid <= 1) {
    try { child.kill("SIGKILL"); } catch { /* invalid child is already unusable */ }
    throw new Error("graph_authoring_fram_lane_coordinator_pid_invalid");
  }
  try {
    await Promise.race([
      runtime.ready(
        child,
        Number(codePort),
        codeLog,
        options.bootTimeoutMs ?? DEFAULT_BOOT_TIMEOUT_MS,
      ),
      new Promise<never>((_resolve, reject) => {
        child.once("error", reject);
      }),
    ]);
  } catch (error) {
    try { child.kill("SIGKILL"); } catch { /* startup already failed */ }
    await runtime.waitForExit(child, options.killMs ?? DEFAULT_KILL_MS);
    throw error;
  }

  const path = descriptorPath(sourceRoot);
  const descriptor: ManagedFramLaneDescriptor = {
    version: MANAGED_LANE_VERSION,
    active: true,
    sourceRoot,
    codeLog,
    codePort,
    canonicalSourceRoot,
    seededFrom: canonicalLog,
    reboundTrackedPaths,
    pid,
  };
  writeDescriptor(path, descriptor);
  let closePromise: Promise<void> | undefined;
  let closed = false;
  const markClosed = () => {
    if (closed) return;
    closed = true;
    writeDescriptor(path, { ...descriptor, active: false });
  };
  const forceClose = () => {
    if (child.exitCode === null && child.signalCode === null) {
      try { child.kill("SIGKILL"); } catch { /* already terminal */ }
    }
    markClosed();
  };
  const close = () => closePromise ??= (async () => {
    if (!await runtime.waitForExit(child, 0)) {
      try { child.kill("SIGTERM"); } catch { /* already terminal */ }
      if (!await runtime.waitForExit(child, options.termMs ?? DEFAULT_TERM_MS)) {
        try { child.kill("SIGKILL"); } catch { /* already terminal */ }
        if (!await runtime.waitForExit(child, options.killMs ?? DEFAULT_KILL_MS))
          throw new Error("graph_authoring_fram_lane_coordinator_reap_failed");
      }
    }
    markClosed();
  })();
  return { sourceRoot, codeLog, codePort, pid, close, forceClose };
}

// FRAM_CODE_PORT names the WARM code coordinator for the fram checkout itself
// (framGraphAuthoringRoots().framHome, flipped by `fram-code-on ~/code/fram`),
// not the target repo a lane happens to be authoring in. `fram-code-on` writes
// that port into framHome/.mcp.json (mcpServers.fram.env.FRAM_CODE_PORT) on
// every flip/re-warm; `fram-code-status` reads the exact same field to report
// port=/coord=. Reading it here at spawn time — rather than baking in a
// literal — is the only way the value can track a coordinator that fram-code-on
// has moved three times in one evening. No .mcp.json (or no matching field)
// means the code coordinator was never flipped for this framHome: fail closed
// by name rather than pointing the MCP server at a port nothing listens on.
function framFlipEnvironment(framHome: string): Record<string, unknown> {
  const mcpJsonPath = join(framHome, ".mcp.json");
  let raw: string;
  try {
    raw = readFileSync(mcpJsonPath, "utf8");
  } catch (cause) {
    throw new Error(
      "graph_authoring_fram_code_port_unresolved: no "
      + `${mcpJsonPath} — run \`fram-code-on ${framHome}\` to boot the warm code `
      + "coordinator before composing the graph-authoring.fram capability",
      { cause },
    );
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (cause) {
    throw new Error(
      `graph_authoring_fram_code_port_unresolved: ${mcpJsonPath} is not valid JSON`,
      { cause },
    );
  }
  return (parsed as { mcpServers?: { fram?: { env?: Record<string, unknown> } } })
    ?.mcpServers?.fram?.env ?? {};
}

function resolveFramCodePort(flipEnv: Record<string, unknown>, framHome: string): string {
  const port = flipEnv.FRAM_CODE_PORT;
  if (typeof port !== "string" || !/^[1-9][0-9]*$/.test(port)) {
    throw new Error(
      `graph_authoring_fram_code_port_unresolved: ${join(framHome, ".mcp.json")} has no `
      + "mcpServers.fram.env.FRAM_CODE_PORT — the same field fram-code-status reads "
      + "to report the live coordinator port",
    );
  }
  return port;
}

// The interactive flip records how long an edit-prepare may take against ITS
// code log (a ~350k-fact log costs ~3s, far past fram.rt's 2000ms default read
// deadline; without the override every managed graph edit fails with a
// prepare-deadline REJECTED that reads like a semantic rejection — learning on
// thread 019f9cf1-d746). A managed lane must inherit the same budget the flip
// wrote, never a value baked in here; a framHome whose flip declares none keeps
// fram's own default rather than acquiring a synthetic one.
function resolveFramCoordReadTimeout(
  flipEnv: Record<string, unknown>,
): Readonly<Record<string, string>> {
  const timeout = flipEnv.FRAM_COORD_READ_TIMEOUT_MS;
  return typeof timeout === "string" && /^[1-9][0-9]*$/.test(timeout)
    ? { FRAM_COORD_READ_TIMEOUT_MS: timeout }
    : {};
}

// FRAM_LOG/FRAM_THREADS select the CORPUS a read (show/ask/validate) folds —
// bin/fram-mcp is explicit that both are required. The dispatching process
// (any invocation routed through bin/north) already exports both with the
// exact defaults and log-split logic reproduced here via expectedLog(); a
// managed lane's fram MCP must read the SAME corpus its own north MCP reads,
// never a hardcoded home path baked into this module.
function corpusFramEnv(): Readonly<Record<string, string>> {
  return Object.freeze({
    FRAM_LOG: expectedLog(),
    FRAM_THREADS: resolve(
      process.env.FRAM_THREADS ?? join(homedir(), ".local", "state", "north", "threads"),
    ),
  });
}

function staticFramMcpEnv(): Readonly<Record<string, string>> {
  const { framHome, beagleHome } = framGraphAuthoringRoots();
  const flipEnv = framFlipEnvironment(framHome);
  return Object.freeze({
    FRAM_FLIP: "1",
    FRAM_GRAPH_EDIT: "1",
    FRAM_CODE_PORT: resolveFramCodePort(flipEnv, framHome),
    ...resolveFramCoordReadTimeout(flipEnv),
    FRAM_CODE_LOG: join(framHome, ".fram", "code.log"),
    ...corpusFramEnv(),
    FRAM_OUT: join(framHome, "out"),
    FRAM_BIN: join(framHome, "bin"),
    FRAM_RESOLVE: join(framHome, "chartroom", "src", "resolve.clj"),
    FRAM_ROUNDTRIP: join(beagleHome, "beagle-lib", "private", "facts-roundtrip.rkt"),
    FRAM_CHECK_EMIT: join(beagleHome, "beagle-lib", "private", "facts-check-emit.rkt"),
    FRAM_BUILD_ALL: join(beagleHome, "bin", "beagle-build-all"),
    BEAGLE_HOME: beagleHome,
  });
}

export function framMcpEnvironment(
  cwd: string,
  requireManagedLane = false,
): Readonly<Record<string, string>> {
  const source = resolve(cwd);
  const lane = readManagedLaneDescriptor(source);
  if (requireManagedLane && !lane) {
    throw new Error(
      "graph_authoring_fram_lane_coordinator_unprepared: managed worktree graph "
      + "authoring requires a worktree-local code coordinator",
    );
  }
  return Object.freeze({
    ...staticFramMcpEnv(),
    FRAM_SRC: source,
    ...(lane ? {
      FRAM_CODE_LOG: lane.codeLog,
      FRAM_CODE_PORT: lane.codePort,
    } : {}),
  });
}

export function framMcpServer(cwd: string, requireManagedLane = false) {
  return Object.freeze({
    type: "stdio" as const,
    command: framMcpCommand(),
    args: Object.freeze([]) as unknown as string[],
    env: framMcpEnvironment(cwd, requireManagedLane),
  });
}

function exactStringMap(actual: unknown, expected: Readonly<Record<string, string>>): boolean {
  if (!actual || typeof actual !== "object" || Array.isArray(actual)) return false;
  const entries = Object.entries(actual);
  const expectedEntries = Object.entries(expected);
  return entries.length === expectedEntries.length
    && entries.every(([key, value], index) =>
      key === expectedEntries[index]?.[0] && value === expectedEntries[index]?.[1]);
}

export function hasCanonicalFramMcpServer(server: unknown, cwd: string): boolean {
  if (!server || typeof server !== "object" || Array.isArray(server)) return false;
  const raw = server as Record<string, unknown>;
  return Object.keys(raw).sort().join(",") === "args,command,env,type"
    && raw.type === "stdio"
    && raw.command === framMcpCommand()
    && Array.isArray(raw.args)
    && raw.args.length === 0
    && exactStringMap(raw.env, framMcpEnvironment(cwd));
}
