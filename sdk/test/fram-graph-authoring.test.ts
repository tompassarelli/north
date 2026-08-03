import { afterAll, afterEach, expect, test } from "bun:test";
import { EventEmitter } from "node:events";
import type { ChildProcess } from "node:child_process";
import {
  mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  ORCHESTRATION_CAPABILITIES, hasAuthoringCapability,
} from "../src/orchestration-capabilities";
import {
  orchestrationCapabilities, loadOrchestrationStaffing,
} from "../src/orchestration-staffing";
import type { RoutingRequest } from "../src/routing-metadata";
import {
  hasCanonicalHarnessAuthority, harnessOptions, managedToolPolicy,
} from "../src/harness";
import {
  validateManagedExecutionEnvelope,
} from "../src/execution-admission";
import {
  FRAM_MCP_TOOL_NAMES, FRAM_MCP_TOOLS, framLaneSpaceId, framLaneStoreGeneration,
  framMcpCommand, framMcpEnvironment, graphAuthoringRoot,
  managedFramLaneSourceConfiguration, prepareFramLaneStore,
  prepareManagedFramCoordinator, seedReboundFramCodeLog,
} from "../src/fram-graph-authoring";
import { expectedLog } from "../src/coord-wire";
import {
  compileProviderAuthoritySurface, formatProviderAuthoritySurface,
} from "../src/providers/authority";
import { causeChain } from "../src/death";
import { HostTerminationError } from "../src/query-lifecycle";
import { eligibleForProviderProcessDeathRetry } from "../src/spawn";
import { presetRequest } from "./routing-fixtures";

const north = resolve(import.meta.dir, "../..");
const originalAgentLaws = process.env.AGENT_LAWS;
const originalFramThreads = process.env.FRAM_THREADS;

// Roots are deployment facts supplied by the dispatcher; tests provide
// hermetic stand-ins (with an executable fram-mcp stub so the admission
// X_OK preflight passes) instead of depending on host checkouts. FRAM_CODE_PORT
// is resolved at spawn time from framHome/.mcp.json — the exact field
// fram-code-status reads — so the fixture writes one too, standing in for a
// real `fram-code-on` flip.
const framHome = mkdtempSync(join(tmpdir(), "fram-home-"));
const beagleHome = mkdtempSync(join(tmpdir(), "beagle-home-"));
const framCodePort = "38213";
mkdirSync(join(framHome, "bin"), { recursive: true });
writeFileSync(join(framHome, "bin", "fram-mcp"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
writeFileSync(join(framHome, ".mcp.json"), JSON.stringify({
  mcpServers: { fram: { command: "x", args: [], env: { FRAM_CODE_PORT: framCodePort } } },
}));
process.env.NORTH_FRAM_HOME = framHome;
process.env.NORTH_BEAGLE_HOME = beagleHome;
process.env.FRAM_THREADS = join(framHome, "threads");

// Hermetic stand-in for fram's sealed bin/fram-migrate-triple-log: the seam only
// has to leave the one generation `fram-daemon serve` accepts.
const migrations: Array<{ source: string; spaceId: string; target: string }> = [];
const migrateFixture = async (input: {
  framHome: string; source: string; spaceId: string; target: string;
}) => {
  migrations.push({ source: input.source, spaceId: input.spaceId, target: input.target });
  writeFileSync(input.target, `FRAMLOG\u0000${readFileSync(input.source, "utf8")}`);
};

afterEach(() => {
  migrations.length = 0;
  if (originalAgentLaws === undefined) delete process.env.AGENT_LAWS;
  else process.env.AGENT_LAWS = originalAgentLaws;
  process.env.NORTH_FRAM_HOME = framHome;
  process.env.NORTH_BEAGLE_HOME = beagleHome;
  if (originalFramThreads === undefined) process.env.FRAM_THREADS = join(framHome, "threads");
  else process.env.FRAM_THREADS = originalFramThreads;
});

afterAll(() => {
  rmSync(framHome, { recursive: true, force: true });
  rmSync(beagleHome, { recursive: true, force: true });
  if (originalFramThreads === undefined) delete process.env.FRAM_THREADS;
  else process.env.FRAM_THREADS = originalFramThreads;
});

test("the flip's coordinator read budget rides into the managed lane, or nothing does", () => {
  // A ~350k-fact code log needs the flip's own FRAM_COORD_READ_TIMEOUT_MS: with
  // fram.rt's 2000ms default every managed graph edit dies as a prepare-deadline
  // REJECTED. Take the value the flip wrote; invent nothing when it wrote none.
  expect(framMcpEnvironment(north).FRAM_COORD_READ_TIMEOUT_MS).toBeUndefined();

  const budgetedFramHome = mkdtempSync(join(tmpdir(), "fram-home-budgeted-"));
  mkdirSync(join(budgetedFramHome, "bin"), { recursive: true });
  writeFileSync(join(budgetedFramHome, "bin", "fram-mcp"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  writeFileSync(join(budgetedFramHome, ".mcp.json"), JSON.stringify({
    mcpServers: { fram: { env: {
      FRAM_CODE_PORT: framCodePort, FRAM_COORD_READ_TIMEOUT_MS: "180000",
    } } },
  }));
  process.env.NORTH_FRAM_HOME = budgetedFramHome;
  try {
    expect(framMcpEnvironment(north).FRAM_COORD_READ_TIMEOUT_MS).toBe("180000");
  } finally {
    rmSync(budgetedFramHome, { recursive: true, force: true });
  }
});

test("a copied code log rebinds only tracked views, never ordinary source literals", () => {
  const root = mkdtempSync(join(tmpdir(), "fram-rebound-seed-"));
  const canonical = join(root, "canonical");
  const lane = join(root, "lane");
  const canonicalLog = join(canonical, ".fram", "code.log");
  const laneLog = join(lane, ".fram", "code.log");
  mkdirSync(join(canonical, ".fram"), { recursive: true });
  mkdirSync(lane, { recursive: true });
  const original = [
    `{:tx 1, :op "assert", :l "@src.demo#root", :p "file", :r "${canonical}/src/demo.bclj"}`,
    `{:tx 2, :op "assert", :l "@src.demo#9", :p "v", :r "${canonical}/literal-must-stay"}`,
    `{:tx 3, :op "assert", :l "@external#root", :p "file", :r "/other/repo/external.bclj"}`,
    "",
  ].join("\n");
  writeFileSync(canonicalLog, original);
  try {
    expect(seedReboundFramCodeLog({
      canonicalLog,
      laneLog,
      canonicalSourceRoot: canonical,
      laneSourceRoot: lane,
    })).toEqual({ reboundTrackedPaths: 1 });
    const seeded = readFileSync(laneLog, "utf8");
    expect(seeded).toContain(`:r "${lane}/src/demo.bclj"`);
    expect(seeded).toContain(`:r "${canonical}/literal-must-stay"`);
    expect(seeded).toContain(`:r "/other/repo/external.bclj"`);
    expect(readFileSync(canonicalLog, "utf8")).toBe(original);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("managed worktree preparation wires local env and reaps its coordinator", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-rebound-lifecycle-"));
  const canonical = join(root, "canonical");
  const lane = join(root, "lane");
  const localFramHome = join(root, "fram-home");
  mkdirSync(join(localFramHome, ".fram"), { recursive: true });
  mkdirSync(join(localFramHome, "bin"), { recursive: true });
  mkdirSync(join(canonical, "src"), { recursive: true });
  mkdirSync(join(lane, "src"), { recursive: true });
  writeFileSync(join(localFramHome, ".fram", "code.log"), [
    `{:tx 1, :op "assert", :l "@src.demo#root", :p "file", :r "${canonical}/src/demo.bclj"}`,
    "",
  ].join("\n"));
  writeFileSync(join(localFramHome, ".mcp.json"), JSON.stringify({
    mcpServers: { fram: { env: { FRAM_CODE_PORT: "39999" } } },
  }));
  writeFileSync(join(localFramHome, "bin", "fram-mcp"), "", { mode: 0o755 });
  const child = new EventEmitter() as ChildProcess;
  Object.assign(child, { pid: 4242, exitCode: null, signalCode: null });
  const signals: NodeJS.Signals[] = [];
  let unrefCalls = 0;
  (child as any).unref = () => { unrefCalls++; };
  (child as any).kill = (signal: NodeJS.Signals) => {
    signals.push(signal);
    (child as any).signalCode = signal;
    child.emit("exit", null, signal);
    return true;
  };
  let launchInput: Record<string, string> | undefined;
  process.env.NORTH_FRAM_HOME = localFramHome;
  try {
    const coordinator = await prepareManagedFramCoordinator({
      worktree: lane,
      canonicalSourceRoot: canonical,
      runtime: {
        allocatePort: async () => 45678,
        launch: (input) => {
          launchInput = input;
          return child;
        },
        ready: async (_child, port, log, _timeoutMs, daemonLog) => {
          expect(port).toBe(45678);
          expect(log).toBe(join(lane, ".fram", "code.log"));
          expect(daemonLog).toBe(join(lane, ".fram", "coord-45678.log"));
        },
        waitForExit: async (proc) =>
          proc.exitCode !== null || proc.signalCode !== null,
        migrate: migrateFixture,
      },
    });
    expect(launchInput).toMatchObject({
      framHome: localFramHome,
      codePort: "45678",
      codeLog: join(lane, ".fram", "code.log"),
      spaceId: framLaneSpaceId(join(lane, ".fram", "code.log")),
    });
    // The seeded flat log is converted once, in place, before the daemon boots.
    expect(migrations).toEqual([{
      source: join(lane, ".fram", "code.log.legacy-flat"),
      spaceId: framLaneSpaceId(join(lane, ".fram", "code.log")),
      target: join(lane, ".fram", "code.log"),
    }]);
    expect(framLaneStoreGeneration(join(lane, ".fram", "code.log"))).toBe("framlog");
    expect(unrefCalls).toBe(1);
    expect(readFileSync(join(lane, ".git", "info", "exclude"), "utf8"))
      .toContain("/.fram/");
    expect(framMcpEnvironment(lane, true)).toMatchObject({
      FRAM_SRC: lane,
      FRAM_CODE_LOG: join(lane, ".fram", "code.log"),
      FRAM_CODE_PORT: "45678",
    });
    process.env.AGENT_LAWS = "off";
    for (const provider of ["anthropic", "openai"] as const) {
      const options = harnessOptions({
        self: `${provider}-managed-fram-lane`,
        provider,
        cwd: lane,
        managedWorktree: true,
        presenceRegistrar: false,
        routingMetadata: graphAuthoringRequest,
      }) as any;
      expect(options.mcpServers.fram.env).toMatchObject({
        FRAM_SRC: lane,
        FRAM_CODE_LOG: join(lane, ".fram", "code.log"),
        FRAM_CODE_PORT: "45678",
      });
    }
    expect(readFileSync(coordinator.codeLog, "utf8"))
      .toContain(`:r "${lane}/src/demo.bclj"`);
    await coordinator.close();
    expect(signals).toEqual(["SIGTERM"]);
    expect(readFileSync(join(lane, ".fram", "managed-code-coordinator.json"), "utf8"))
      .toContain('"active": false');
    expect(readFileSync(coordinator.codeLog, "utf8"))
      .toContain(`:r "${lane}/src/demo.bclj"`);
    expect(() => framMcpEnvironment(lane, true))
      .toThrow("graph_authoring_fram_lane_descriptor_invalid");
  } finally {
    process.env.NORTH_FRAM_HOME = framHome;
    rmSync(root, { recursive: true, force: true });
  }
});

test("death-path reap kills only the descriptor PID that owns its recorded port", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-rebound-death-reap-"));
  const canonical = join(root, "canonical");
  const lane = join(root, "lane");
  const localFramHome = join(root, "fram-home");
  mkdirSync(join(localFramHome, ".fram"), { recursive: true });
  mkdirSync(join(canonical, "src"), { recursive: true });
  mkdirSync(join(lane, "src"), { recursive: true });
  writeFileSync(join(localFramHome, ".fram", "code.log"), [
    `{:tx 1, :op "assert", :l "@src.demo#root", :p "file", :r "${canonical}/src/demo.bclj"}`,
    "",
  ].join("\n"));
  const child = new EventEmitter() as ChildProcess;
  Object.assign(child, { pid: 5151, exitCode: null, signalCode: null });
  (child as any).unref = () => {};
  let ownerPid = 6161;
  let alive = true;
  let listening = true;
  const signals: Array<[number, NodeJS.Signals]> = [];
  process.env.NORTH_FRAM_HOME = localFramHome;
  try {
    const coordinator = await prepareManagedFramCoordinator({
      worktree: lane,
      canonicalSourceRoot: canonical,
      runtime: {
        allocatePort: async () => 45680,
        launch: () => child,
        ready: async () => {},
        waitForExit: async () => !alive,
        migrate: migrateFixture,
        pidAlive: () => alive,
        portListening: () => listening,
        pidOwnsPort: (pid, port) => pid === ownerPid && port === 45680,
        signalPid: (pid, signal) => {
          signals.push([pid, signal]);
          alive = false;
          listening = false;
        },
      },
    });
    await expect(coordinator.close()).rejects.toThrow(
      "graph_authoring_fram_lane_coordinator_pid_port_mismatch",
    );
    expect(signals).toEqual([]);
    expect(readFileSync(join(lane, ".fram", "managed-code-coordinator.json"), "utf8"))
      .toContain('"active": true');

    ownerPid = 5151;
    await coordinator.close();
    expect(signals).toEqual([[5151, "SIGTERM"]]);
    expect(readFileSync(join(lane, ".fram", "managed-code-coordinator.json"), "utf8"))
      .toContain('"active": false');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("reap accepts a coordinator exit between its final poll and ownership check", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-rebound-reap-race-"));
  const canonical = join(root, "canonical");
  const lane = join(root, "lane");
  const localFramHome = join(root, "fram-home");
  mkdirSync(join(localFramHome, ".fram"), { recursive: true });
  mkdirSync(join(canonical, "src"), { recursive: true });
  mkdirSync(join(lane, "src"), { recursive: true });
  writeFileSync(join(localFramHome, ".fram", "code.log"), [
    `{:tx 1, :op "assert", :l "@src.demo#root", :p "file", :r "${canonical}/src/demo.bclj"}`,
    "",
  ].join("\n"));
  const child = new EventEmitter() as ChildProcess;
  Object.assign(child, { pid: 5251, exitCode: null, signalCode: null });
  (child as any).unref = () => {};
  let alive = true;
  let listening = true;
  const signals: NodeJS.Signals[] = [];
  process.env.NORTH_FRAM_HOME = localFramHome;
  try {
    const coordinator = await prepareManagedFramCoordinator({
      worktree: lane,
      canonicalSourceRoot: canonical,
      termMs: 0,
      killMs: 0,
      runtime: {
        allocatePort: async () => 45681,
        launch: () => child,
        ready: async () => {},
        waitForExit: async () => !alive,
        migrate: migrateFixture,
        pidAlive: () => alive,
        portListening: () => listening,
        pidOwnsPort: () => alive && listening,
        signalPid: (_pid, signal) => {
          signals.push(signal);
          queueMicrotask(() => {
            alive = false;
            listening = false;
          });
        },
      },
    });

    await coordinator.close();
    expect(signals).toEqual(["SIGTERM"]);
    expect(readFileSync(join(lane, ".fram", "managed-code-coordinator.json"), "utf8"))
      .toContain('"active": false');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("reap grants checkpoint grace to a coordinator that released its port but lingers", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-rebound-reap-linger-"));
  const canonical = join(root, "canonical");
  const lane = join(root, "lane");
  const localFramHome = join(root, "fram-home");
  mkdirSync(join(localFramHome, ".fram"), { recursive: true });
  mkdirSync(join(canonical, "src"), { recursive: true });
  mkdirSync(join(lane, "src"), { recursive: true });
  writeFileSync(join(localFramHome, ".fram", "code.log"), [
    `{:tx 1, :op "assert", :l "@src.demo#root", :p "file", :r "${canonical}/src/demo.bclj"}`,
    "",
  ].join("\n"));
  const child = new EventEmitter() as ChildProcess;
  Object.assign(child, { pid: 5252, exitCode: null, signalCode: null });
  (child as any).unref = () => {};
  let alive = true;
  let listening = true;
  const signals: NodeJS.Signals[] = [];
  process.env.NORTH_FRAM_HOME = localFramHome;
  try {
    const coordinator = await prepareManagedFramCoordinator({
      worktree: lane,
      canonicalSourceRoot: canonical,
      termMs: 0,
      killMs: 0,
      runtime: {
        allocatePort: async () => 45682,
        launch: () => child,
        ready: async () => {},
        waitForExit: async () => !alive,
        migrate: migrateFixture,
        pidAlive: () => alive,
        portListening: () => listening,
        pidOwnsPort: () => alive && listening,
        // The JVM closes its listener at SIGTERM but flushes its shutdown
        // checkpoint before exiting; it dies only when the kill escalation
        // arrives. The old code threw pid_port_mismatch here instead.
        signalPid: (_pid, signal) => {
          signals.push(signal);
          listening = false;
          if (signal === "SIGKILL") alive = false;
        },
      },
    });

    await coordinator.close();
    expect(signals).toEqual(["SIGTERM", "SIGKILL"]);
    expect(readFileSync(join(lane, ".fram", "managed-code-coordinator.json"), "utf8"))
      .toContain('"active": false');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("host abort force-reaps a detached coordinator that has not finished booting", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-rebound-boot-abort-"));
  const canonical = join(root, "canonical");
  const lane = join(root, "lane");
  const localFramHome = join(root, "fram-home");
  mkdirSync(join(localFramHome, ".fram"), { recursive: true });
  mkdirSync(join(canonical, "src"), { recursive: true });
  mkdirSync(join(lane, "src"), { recursive: true });
  writeFileSync(join(localFramHome, ".fram", "code.log"), [
    `{:tx 1, :op "assert", :l "@src.demo#root", :p "file", :r "${canonical}/src/demo.bclj"}`,
    "",
  ].join("\n"));
  const child = new EventEmitter() as ChildProcess;
  Object.assign(child, { pid: 5252, exitCode: null, signalCode: null });
  (child as any).unref = () => {};
  const signals: NodeJS.Signals[] = [];
  (child as any).kill = (signal: NodeJS.Signals) => {
    signals.push(signal);
    (child as any).signalCode = signal;
    child.emit("exit", null, signal);
    return true;
  };
  const abort = new AbortController();
  process.env.NORTH_FRAM_HOME = localFramHome;
  try {
    const preparing = prepareManagedFramCoordinator({
      worktree: lane,
      canonicalSourceRoot: canonical,
      signal: abort.signal,
      runtime: {
        allocatePort: async () => 45681,
        launch: () => child,
        ready: () => new Promise<void>(() => {}),
        waitForExit: async (proc) =>
          proc.exitCode !== null || proc.signalCode !== null,
        migrate: migrateFixture,
      },
    });
    await Promise.resolve();
    abort.abort(new HostTerminationError("SIGTERM"));
    let bootError: unknown;
    try {
      await preparing;
    } catch (error) {
      bootError = error;
    }
    expect(causeChain(bootError)).toBe(
      "graph_authoring_fram_lane_coordinator_boot_aborted"
      + " <- cause: host termination requested (SIGTERM)",
    );
    expect(signals.length).toBeGreaterThanOrEqual(1);
    expect(signals.every((signal) => signal === "SIGKILL")).toBe(true);
  } finally {
    process.env.NORTH_FRAM_HOME = framHome;
    rmSync(root, { recursive: true, force: true });
  }
});

test("Beagle worktrees seed their repository log and rebind the self-host source tree", async () => {
  const root = mkdtempSync(join(tmpdir(), "beagle-rebound-lifecycle-"));
  const canonical = join(root, "beagle");
  const lane = join(root, "lane");
  const localFramHome = join(root, "fram-home");
  const canonicalSource = join(canonical, "self-host", "src", "selfhost");
  const laneSource = join(lane, "self-host", "src", "selfhost");
  mkdirSync(join(localFramHome, "bin"), { recursive: true });
  mkdirSync(join(canonical, ".fram"), { recursive: true });
  mkdirSync(canonicalSource, { recursive: true });
  mkdirSync(laneSource, { recursive: true });
  writeFileSync(join(canonical, ".fram", "code.log"), [
    `{:tx 1, :op "assert", :l "@selfhost.demo#root", :p "file", :r "${canonicalSource}/demo.bclj"}`,
    "",
  ].join("\n"));
  writeFileSync(join(localFramHome, ".mcp.json"), JSON.stringify({
    mcpServers: { fram: { env: { FRAM_CODE_PORT: "39998" } } },
  }));
  const child = new EventEmitter() as ChildProcess;
  Object.assign(child, { pid: 4343, exitCode: null, signalCode: null });
  (child as any).kill = (signal: NodeJS.Signals) => {
    (child as any).signalCode = signal;
    child.emit("exit", null, signal);
    return true;
  };
  process.env.NORTH_FRAM_HOME = localFramHome;
  process.env.NORTH_BEAGLE_HOME = canonical;
  try {
    const source = managedFramLaneSourceConfiguration(lane, canonical);
    expect(source).toEqual({
      canonicalSourceRoot: canonicalSource,
      canonicalCodeLog: join(canonical, ".fram", "code.log"),
      laneSourceRoot: laneSource,
    });
    const coordinator = await prepareManagedFramCoordinator({
      worktree: lane,
      ...source,
      runtime: {
        allocatePort: async () => 45679,
        launch: () => child,
        ready: async () => {},
        waitForExit: async (proc) =>
          proc.exitCode !== null || proc.signalCode !== null,
        migrate: migrateFixture,
      },
    });
    expect(framMcpEnvironment(lane, true)).toMatchObject({
      FRAM_SRC: laneSource,
      FRAM_CODE_LOG: join(lane, ".fram", "code.log"),
      FRAM_CODE_PORT: "45679",
    });
    expect(readFileSync(coordinator.codeLog, "utf8"))
      .toContain(`:r "${laneSource}/demo.bclj"`);
    await coordinator.close();
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("managed graph composition fails closed without a prepared lane coordinator", () => {
  process.env.AGENT_LAWS = "off";
  const unprepared = mkdtempSync(join(tmpdir(), "fram-unprepared-lane-"));
  try {
    expect(() => harnessOptions({
      self: "unprepared-fram-lane",
      provider: "openai",
      cwd: unprepared,
      managedWorktree: true,
      presenceRegistrar: false,
      routingMetadata: graphAuthoringRequest,
    })).toThrow("graph_authoring_fram_lane_coordinator_unprepared");
  } finally {
    rmSync(unprepared, { recursive: true, force: true });
  }
});

test("composing the capability without deployment roots fails closed by name", () => {
  const priorHome = process.env.HOME;
  // homedir() reads $HOME, so an empty layout proves the default cannot invent a root.
  const bareHome = mkdtempSync(join(tmpdir(), "fram-bare-home-"));
  process.env.HOME = bareHome;
  try {
    delete process.env.NORTH_FRAM_HOME;
    delete process.env.NORTH_BEAGLE_HOME;
    expect(() => framMcpCommand()).toThrow(/graph_authoring_fram_roots_unset/);
    expect(() => framMcpCommand()).toThrow(/NORTH_FRAM_HOME, NORTH_BEAGLE_HOME/);
    process.env.NORTH_FRAM_HOME = framHome;
    expect(() => framMcpCommand()).toThrow(/NORTH_BEAGLE_HOME/);
  } finally {
    if (priorHome === undefined) delete process.env.HOME;
    else process.env.HOME = priorHome;
    rmSync(bareHome, { recursive: true, force: true });
  }
});

test("an unset root defaults to the standard ~/code/<repo>/main checkout", () => {
  const priorHome = process.env.HOME;
  const home = mkdtempSync(join(tmpdir(), "fram-layout-home-"));
  const framMain = join(home, "code", "fram", "main");
  const beagleMain = join(home, "code", "beagle", "main");
  mkdirSync(join(framMain, ".git"), { recursive: true });
  mkdirSync(join(framMain, "bin"), { recursive: true });
  writeFileSync(join(framMain, "bin", "fram-mcp"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  mkdirSync(join(beagleMain, ".git"), { recursive: true });
  process.env.HOME = home;
  try {
    delete process.env.NORTH_FRAM_HOME;
    delete process.env.NORTH_BEAGLE_HOME;
    expect(graphAuthoringRoot("NORTH_FRAM_HOME")).toBe(framMain);
    // A checkout missing the repo's own marker is not a root; the wall stays up.
    expect(graphAuthoringRoot("NORTH_BEAGLE_HOME")).toBeUndefined();
    mkdirSync(join(beagleMain, "bin"), { recursive: true });
    writeFileSync(join(beagleMain, "bin", "beagle"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
    expect(graphAuthoringRoot("NORTH_BEAGLE_HOME")).toBe(beagleMain);
    expect(framMcpCommand()).toBe(join(framMain, "bin", "fram-mcp"));
    // An explicit export still wins over the layout default.
    process.env.NORTH_FRAM_HOME = framHome;
    expect(graphAuthoringRoot("NORTH_FRAM_HOME")).toBe(framHome);
  } finally {
    if (priorHome === undefined) delete process.env.HOME;
    else process.env.HOME = priorHome;
    rmSync(home, { recursive: true, force: true });
  }
});

test("a seeded legacy-flat lane store is migrated exactly once before serve", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-lane-store-"));
  const codeLog = join(root, "code.log");
  writeFileSync(codeLog, '{:tx 1, :op "assert"}\n');
  const spaceId = framLaneSpaceId(codeLog);
  try {
    expect(framLaneStoreGeneration(codeLog)).toBe("legacy");
    expect(await prepareFramLaneStore({
      framHome: "/unused", codeLog, spaceId, migrate: migrateFixture,
    })).toEqual({ generation: "legacy", migrated: true });
    expect(migrations).toEqual([
      { source: `${codeLog}.legacy-flat`, spaceId, target: codeLog },
    ]);
    expect(framLaneStoreGeneration(codeLog)).toBe("framlog");

    // Re-preparing an already-converted store must not migrate a second time.
    expect(await prepareFramLaneStore({
      framHome: "/unused", codeLog, spaceId, migrate: migrateFixture,
    })).toEqual({ generation: "framlog", migrated: false });
    expect(migrations).toHaveLength(1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a fresh lane store boots with no migration step at all", async () => {
  const root = mkdtempSync(join(tmpdir(), "fram-lane-fresh-"));
  const absent = join(root, "code.log");
  const empty = join(root, "empty.log");
  writeFileSync(empty, "");
  try {
    expect(framLaneStoreGeneration(absent)).toBe("absent");
    expect(framLaneStoreGeneration(empty)).toBe("absent");
    for (const codeLog of [absent, empty]) {
      expect(await prepareFramLaneStore({
        framHome: "/unused", codeLog, spaceId: framLaneSpaceId(codeLog),
        migrate: migrateFixture,
      })).toEqual({ generation: "absent", migrated: false });
    }
    expect(migrations).toEqual([]);
    // fram's boot parses any file that exists, so a zero-byte store is removed.
    expect(framLaneStoreGeneration(empty)).toBe("absent");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("a lane SpaceId is stable per store and distinct across lanes", () => {
  expect(framLaneSpaceId("/a/.fram/code.log")).toBe(framLaneSpaceId("/a/.fram/code.log"));
  expect(framLaneSpaceId("/a/.fram/code.log")).not.toBe(framLaneSpaceId("/b/.fram/code.log"));
  expect(framLaneSpaceId("/a/.fram/code.log")).toMatch(/^north-graph-lane-[0-9a-f]{16}$/);
});

test("composing the environment without a flipped code coordinator fails closed by name", () => {
  // A framHome that was never `fram-code-on`'d has no .mcp.json at all: the
  // capability must refuse to compose rather than fall back to a literal port
  // nothing listens on.
  const unflippedFramHome = mkdtempSync(join(tmpdir(), "fram-home-unflipped-"));
  mkdirSync(join(unflippedFramHome, "bin"), { recursive: true });
  writeFileSync(join(unflippedFramHome, "bin", "fram-mcp"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  process.env.NORTH_FRAM_HOME = unflippedFramHome;
  try {
    expect(() => framMcpEnvironment(north)).toThrow(/graph_authoring_fram_code_port_unresolved/);
    expect(() => framMcpEnvironment(north)).toThrow(/\.mcp\.json/);
  } finally {
    rmSync(unflippedFramHome, { recursive: true, force: true });
  }
});

test("composing the environment against a stale .mcp.json missing the port field fails closed", () => {
  const staleFramHome = mkdtempSync(join(tmpdir(), "fram-home-stale-"));
  mkdirSync(join(staleFramHome, "bin"), { recursive: true });
  writeFileSync(join(staleFramHome, "bin", "fram-mcp"), "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  writeFileSync(join(staleFramHome, ".mcp.json"), JSON.stringify({ mcpServers: {} }));
  process.env.NORTH_FRAM_HOME = staleFramHome;
  try {
    expect(() => framMcpEnvironment(north))
      .toThrow(/graph_authoring_fram_code_port_unresolved.*FRAM_CODE_PORT/);
  } finally {
    rmSync(staleFramHome, { recursive: true, force: true });
  }
});

const graphAuthoringRequest: RoutingRequest = {
  role: "beagle-graph-author",
  taskGrade: "senior",
  domainRequirements: ["Beagle graph authoring"],
  topology: "worker",
  tier: "senior",
  reasoning: "high",
  posture: "deliver",
  composition: {
    kind: "bespoke",
    id: "beagle-graph-author",
    bespokeReason: "Fram graph editing is a distinct sealed authority",
    promotionCandidate: false,
    contract: {
      responsibility: "author a graph-upstream Beagle module",
      deliverable: "a compiler-accepted graph edit",
      capabilities: [
        "filesystem.read",
        "filesystem.search",
        "shell.readonly",
        "graph-authoring.fram",
      ],
      mayDecide: ["which graph edit verb fits the requested change"],
      mustEscalate: ["any text edit to graph-upstream source"],
      doneWhen: ["the graph edit recompiles"],
      report: "edited definitions and compiler result",
    },
  },
};

test("graph-authoring.fram is bespoke-only and classed as mutation authority", () => {
  const catalog = loadOrchestrationStaffing();
  expect(ORCHESTRATION_CAPABILITIES).toContain("graph-authoring.fram");
  // The wire vocabulary admits the sealed capability so bespoke contracts can
  // request it; no stock preset may ever carry it.
  expect(catalog.vocabulary.capabilities).toContain("graph-authoring.fram");
  for (const preset of catalog.presets)
    expect(preset.capabilities).not.toContain("graph-authoring.fram");
  expect(orchestrationCapabilities(graphAuthoringRequest)).toContain("graph-authoring.fram");
  expect(hasAuthoringCapability(["graph-authoring.fram"])).toBe(true);
  expect(eligibleForProviderProcessDeathRetry(
    "openai_provider_execution_failed", "worker", ["graph-authoring.fram"],
  )).toBe(false);
});

test("managed providers compile the exact sealed Fram MCP only when explicitly requested", () => {
  process.env.AGENT_LAWS = "off";
  for (const provider of ["anthropic", "openai"] as const) {
    const options = harnessOptions({
      self: `${provider}-fram-graph-author`,
      provider,
      cwd: north,
      presenceRegistrar: false,
      routingMetadata: graphAuthoringRequest,
    }) as any;
    expect(hasCanonicalHarnessAuthority(options, provider)).toBe(true);
    expect(Object.keys(options.mcpServers)).toEqual([
      "north", "north-readonly-shell", "fram",
    ]);
    expect(options.mcpServers.fram).toEqual({
      type: "stdio",
      command: framMcpCommand(),
      args: [],
      env: framMcpEnvironment(north),
    });
    expect(Object.isFrozen(options.mcpServers.fram)).toBe(true);
    expect(Object.isFrozen(options.mcpServers.fram.env)).toBe(true);
    expect(options.mcpServers.fram.env.FRAM_SRC).toBe(north);
    // FRAM_CODE_LOG lives inside the fram checkout (framHome), never the cwd:
    // the lane worktree is a /tmp clone where .fram/ is git-excluded and never
    // exists, so a cwd-relative path could never resolve.
    expect(options.mcpServers.fram.env.FRAM_CODE_LOG).toBe(join(framHome, ".fram", "code.log"));
    expect(options.mcpServers.fram.env.FRAM_CODE_LOG).not.toBe(resolve(north, ".fram/code.log"));
    // FRAM_CODE_PORT is read from framHome/.mcp.json at spawn time — the same
    // field fram-code-status reads — never a hardcoded literal.
    expect(options.mcpServers.fram.env.FRAM_CODE_PORT).toBe(framCodePort);
    // FRAM_LOG/FRAM_THREADS select the real north corpus, derived the same way
    // bin/north exports them for the dispatching process — never a hardcoded
    // home path baked into this module.
    expect(options.mcpServers.fram.env.FRAM_LOG).toBe(expectedLog());
    expect(options.mcpServers.fram.env.FRAM_THREADS).toBe(resolve(process.env.FRAM_THREADS!));
    expect(options.allowedTools).toEqual(expect.arrayContaining([...FRAM_MCP_TOOLS]));
    expect(options.disallowedTools).not.toEqual(expect.arrayContaining([...FRAM_MCP_TOOLS]));
    expect(() => validateManagedExecutionEnvelope(
      provider, options.northCapabilities, options,
    )).not.toThrow();
    expect(compileProviderAuthoritySurface(provider, options).capabilities)
      .toContain("graph-authoring.fram");
    // The lane's own effective-authority log line must NAME the graph-edit verbs
    // on BOTH providers — an operator reading a codex lane log had no way to see
    // whether the fram grant actually mounted.
    const logged = formatProviderAuthoritySurface(
      compileProviderAuthoritySurface(provider, options),
    );
    for (const tool of FRAM_MCP_TOOLS) expect(logged).toContain(tool);

    const missingFram = {
      ...options,
      mcpServers: Object.fromEntries(
        Object.entries(options.mcpServers).filter(([name]) => name !== "fram"),
      ),
    };
    expect(() => validateManagedExecutionEnvelope(
      provider, missingFram.northCapabilities, missingFram,
    )).toThrow(`${provider}_managed_fram_mcp_contract_missing`);
  }

  const absentPolicy = managedToolPolicy(["filesystem.read"]);
  expect(absentPolicy.disallowedTools).toEqual(expect.arrayContaining([...FRAM_MCP_TOOLS]));
  expect(FRAM_MCP_TOOL_NAMES).toHaveLength(11);
});

// Negative control: an unselected composition (no graph-authoring.fram) must be
// wholly unaffected by the capability — byte-identical descriptor whether or not
// the Fram/Beagle deployment roots are present, and never a Fram MCP server.
test("unselected preset compositions are byte-identical and require no Fram roots", () => {
  process.env.AGENT_LAWS = "off";
  const route = presetRequest("integrator");
  const compose = () => harnessOptions({
    self: "anthropic-integrator",
    provider: "anthropic",
    cwd: north,
    presenceRegistrar: false,
    routingMetadata: route,
  }) as any;

  delete process.env.NORTH_FRAM_HOME;
  delete process.env.NORTH_BEAGLE_HOME;
  const withoutRoots = compose(); // must not throw despite absent roots

  process.env.NORTH_FRAM_HOME = framHome;
  process.env.NORTH_BEAGLE_HOME = beagleHome;
  const withRoots = compose();

  for (const options of [withoutRoots, withRoots]) {
    expect(Object.keys(options.mcpServers)).not.toContain("fram");
    expect(options.allowedTools).not.toEqual(expect.arrayContaining([...FRAM_MCP_TOOLS]));
    expect(compileProviderAuthoritySurface("anthropic", options).capabilities)
      .not.toContain("graph-authoring.fram");
  }

  // The only environment difference is the harness forwarding the two root vars
  // when the dispatcher happens to carry them; the capability surface itself is
  // wholly independent of them. Strip that incidental passthrough, then the
  // mcpServers, tools, routing, and environment are byte-identical.
  const descriptor = (options: any) => {
    const env = { ...(options.env ?? {}) };
    delete env.NORTH_FRAM_HOME;
    delete env.NORTH_BEAGLE_HOME;
    return JSON.stringify({
      mcpServers: options.mcpServers,
      allowedTools: options.allowedTools,
      disallowedTools: options.disallowedTools,
      northCapabilities: options.northCapabilities,
      env,
    });
  };
  expect(descriptor(withoutRoots)).toBe(descriptor(withRoots));
});

test("graph/text experiment mounts Fram only for the graph session arm", () => {
  process.env.AGENT_LAWS = "off";
  const route = presetRequest("integrator");
  const assignment = (arm: "graph" | "text") => ({
    version: "north-graph-text-assignment:v1" as const,
    status: "assigned" as const,
    arm,
    applied: true,
    reason: "deterministic-balanced-assignment",
    manifestSha256: "a".repeat(64),
  });
  const compose = (arm: "graph" | "text") => harnessOptions({
    self: `experiment-${arm}`,
    provider: "anthropic",
    cwd: north,
    presenceRegistrar: false,
    routingMetadata: route,
    graphTextExperiment: assignment(arm),
  }) as any;

  const graph = compose("graph");
  const text = compose("text");
  expect(Object.keys(graph.mcpServers)).toContain("fram");
  expect(graph.northCapabilities).toContain("graph-authoring.fram");
  expect(compileProviderAuthoritySurface("anthropic", graph).managedTools)
    .toEqual(expect.arrayContaining([...FRAM_MCP_TOOLS]));
  expect(Object.keys(text.mcpServers)).not.toContain("fram");
  expect(text.northCapabilities).not.toContain("graph-authoring.fram");
  expect(compileProviderAuthoritySurface("anthropic", text).managedTools)
    .not.toEqual(expect.arrayContaining([...FRAM_MCP_TOOLS]));
});
