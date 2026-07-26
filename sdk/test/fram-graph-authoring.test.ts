import { afterAll, afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
  FRAM_MCP_TOOL_NAMES, FRAM_MCP_TOOLS, framMcpCommand, framMcpEnvironment,
} from "../src/fram-graph-authoring";
import { expectedLog } from "../src/coord-wire";
import {
  compileProviderAuthoritySurface, formatProviderAuthoritySurface,
} from "../src/providers/authority";
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

afterEach(() => {
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

test("composing the capability without deployment roots fails closed by name", () => {
  delete process.env.NORTH_FRAM_HOME;
  delete process.env.NORTH_BEAGLE_HOME;
  expect(() => framMcpCommand()).toThrow(/graph_authoring_fram_roots_unset/);
  expect(() => framMcpCommand()).toThrow(/NORTH_FRAM_HOME, NORTH_BEAGLE_HOME/);
  process.env.NORTH_FRAM_HOME = framHome;
  expect(() => framMcpCommand()).toThrow(/NORTH_BEAGLE_HOME/);
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
  expect(FRAM_MCP_TOOL_NAMES).toHaveLength(10);
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
