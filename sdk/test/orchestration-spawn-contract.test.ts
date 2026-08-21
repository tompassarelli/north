import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { validateRoutingMetadata } from "../src/routing-metadata";
import { applyOrchestrationStaffing, loadOrchestrationStaffing } from "../src/orchestration-staffing";
import { bindSpawnTestRuntime } from "../src/internal/test-runtime";
import { wireTurnQuery } from "./support/wire-query";
import type { RoutedQueryArguments } from "../src/providers";

const north = resolve(import.meta.dir, "../..");
const orchestration = process.env.NORTH_ORCHESTRATION_HOME ?? resolve(north, "orchestration");
const compose = resolve(orchestration, "scripts/compose-routing.mjs");

function composed(...args: string[]): any {
  const result = spawnSync(process.execPath, [compose, ...args], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr);
  return JSON.parse(result.stdout);
}

const contract = JSON.stringify({
  responsibility: "reconstruct migration provenance", deliverable: "evidence-linked timeline",
  capabilities: ["filesystem.read", "filesystem.search", "shell.readonly"],
  mayDecide: ["read-only traces"], mustEscalate: ["destructive recovery"],
  doneWhen: ["every transition is sourced"], report: "timeline, contradictions, and gaps",
});

test("Orchestration composition survives North validation", () => {
  const request = composed("integrator", "--taskGrade", "staff", "--domain", "Nix,Beagle",
    "--tier", "frontier", "--deliberation", "xhigh", "--posture", "prune",
    "--override-reason", "cross-provider foundational contract");
  const metadata = validateRoutingMetadata(request);
  expect(metadata).toEqual({
    role: "integrator", taskGrade: "staff", domainRequirements: ["Nix", "Beagle"],
    topology: "worker", tier: "frontier", reasoning: "xhigh", posture: "prune",
    composition: { kind: "template", id: "integrator",
      overrides: ["taskGrade", "domainRequirements", "tier", "reasoning", "posture"],
      overrideReason: "cross-provider foundational contract" },
  });
});

test("spawn bootstrap derives the Codex turn deadline without replacing caller authority", async () => {
  const { applyCodexTurnDeadlineFromReasoning } = await import("../src/spawn");

  for (const [reasoning, deadline] of [
    ["low", "600000"],
    ["medium", "900000"],
    ["high", "1500000"],
    ["xhigh", "2400000"],
    ["max", "2400000"],
  ] as const) {
    const env: NodeJS.ProcessEnv = { AGENT_REASONING: reasoning };
    applyCodexTurnDeadlineFromReasoning(env);
    expect(env.NORTH_CODEX_TURN_DEADLINE_MS).toBe(deadline);
  }

  const explicit: NodeJS.ProcessEnv = {
    AGENT_REASONING: "xhigh",
    NORTH_CODEX_TURN_DEADLINE_MS: "1234567",
  };
  applyCodexTurnDeadlineFromReasoning(explicit);
  expect(explicit.NORTH_CODEX_TURN_DEADLINE_MS).toBe("1234567");
});

test("file-backed managed child bootstrap publishes identity before its hermetic provider boundary", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-child-catalog-startup-"));
  const log = join(directory, "north.log");
  const fakeNorth = join(directory, "north");
  const fakeBb = join(directory, "bb");
  const agentId = `lane-child-catalog-${process.pid}-${Date.now()}`;
  const keys = [
    "PATH", "NORTH_BIN", "NORTH_PEER_BB", "NORTH_IDENTITY_TEST_REDIRECT", "NORTH_STAFFING_SOURCE",
    "NORTH_STREAM_DIR", "NORTH_AGENT_LOGS_DIR", "NORTH_PORT",
    "AGENT_ID", "AGENT_ROLE", "AGENT_TASK_GRADE", "AGENT_DOMAIN_REQUIREMENTS",
    "AGENT_TOPOLOGY", "AGENT_TIER", "AGENT_REASONING", "AGENT_POSTURE",
    "AGENT_COMPOSITION", "AGENT_PROVIDER", "AGENT_TARGET", "AGENT_COORDINATOR",
    "AGENT_WORKTREE", "NORTH_ROUTING_POLICY", "NORTH_ENVELOPE_ACCOUNTING",
    "BEAGLE_STORE_HOME", "BEAGLE_STORE_BIN", "BEAGLE_STORE_OUT",
  ] as const;
  const saved = Object.fromEntries(keys.map((key) => [key, process.env[key]]));
  const route = applyOrchestrationStaffing({ role: "curator" });
  try {
    writeFileSync(fakeNorth, `#!/usr/bin/env bash
if [ "$1 $2" = "json show" ]; then printf '%s\\n' '[]'; exit 0; fi
printf '%s\\n' "$*" >> ${JSON.stringify(log)}
`);
    chmodSync(fakeNorth, 0o700);
    writeFileSync(fakeBb, "#!/usr/bin/env bash\nexit 0\n");
    chmodSync(fakeBb, 0o700);
    process.env.PATH = `${directory}:${saved.PATH ?? process.env.PATH ?? ""}`;
    process.env.NORTH_BIN = fakeNorth;
    process.env.NORTH_PEER_BB = fakeBb;
    process.env.NORTH_IDENTITY_TEST_REDIRECT = "1";
    process.env.NORTH_STAFFING_SOURCE = "file";
    process.env.NORTH_STREAM_DIR = directory;
    process.env.NORTH_AGENT_LOGS_DIR = directory;
    process.env.NORTH_PORT = "59999";
    process.env.BEAGLE_STORE_HOME = directory;
    process.env.BEAGLE_STORE_BIN = directory;
    process.env.BEAGLE_STORE_OUT = directory;
    process.env.AGENT_ID = agentId;
    process.env.AGENT_ROLE = route.role;
    process.env.AGENT_TASK_GRADE = route.taskGrade;
    process.env.AGENT_DOMAIN_REQUIREMENTS = JSON.stringify(route.domainRequirements);
    process.env.AGENT_TOPOLOGY = route.topology;
    process.env.AGENT_TIER = route.tier;
    process.env.AGENT_REASONING = route.reasoning;
    process.env.AGENT_POSTURE = route.posture;
    process.env.AGENT_COMPOSITION = JSON.stringify(route.composition);
    delete process.env.AGENT_PROVIDER;
    delete process.env.AGENT_TARGET;
    process.env.AGENT_COORDINATOR = "test-coordinator";
    process.env.AGENT_WORKTREE = "0";
    delete process.env.NORTH_ROUTING_POLICY;
    delete process.env.NORTH_ENVELOPE_ACCOUNTING;

    const { managedChildSpawnOptions, spawn } = await import("../src/spawn");
    const request = managedChildSpawnOptions("publish child startup identity");
    expect(request.routingMetadata).toMatchObject({ role: "curator", posture: "prune" });
    expect(process.env.NORTH_DELEGATE_THREAD_ID).toBeUndefined();
    // The executable entrypoint marks its caller authority as already checked
    // before it calls spawn(). This in-process fixture takes the same request
    // through the real execution path, so remove the inherited worker marker.
    delete process.env.AGENT_TOPOLOGY;
    let providerBoundaryCalls = 0;
    bindSpawnTestRuntime(request, {
      admitDispatchAuthority: () => {},
      publishLearningAssignment: async () => "recorded" as const,
      queryFn: (args: RoutedQueryArguments) => {
        providerBoundaryCalls++;
        return wireTurnQuery(args, { provider: "openai", output: "hermetic child result" });
      },
      worktreeAllocationWriter: { register: () => {}, event: () => {} },
      feedSubscriber: () => Object.assign(() => {}, {
        ready: Promise.resolve(), caughtUp: Promise.resolve(), replay: async () => {},
        drain: async () => {}, isArmed: () => true,
      }),
    });

    await spawn(request);
    const commands = readFileSync(log, "utf8");
    expect(commands).toContain(`tell agent:${agentId} kind lane`);
    expect(commands).toContain(`tell agent:${agentId} composition_id curator`);
    expect(commands).toContain(`tell agent:${agentId} display_name`);
    expect(providerBoundaryCalls).toBe(1);
  } finally {
    for (const key of keys) {
      const value = saved[key];
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
    rmSync(directory, { recursive: true, force: true });
  }
});

test("SDK presets inherit catalog axes while declared compatible overrides win independently", () => {
  const catalog = loadOrchestrationStaffing(resolve(orchestration, "staffing/catalog.json"));
  expect(() => applyOrchestrationStaffing({ role: "integrator", tier: "frontier" }, catalog))
    .toThrow("supply template composition.overrides");
  expect(applyOrchestrationStaffing({ role: "integrator", tier: "frontier", reasoning: "xhigh",
    composition: { kind: "template", id: "integrator", overrides: ["tier", "reasoning"],
      overrideReason: "cross-seam direction" } }, catalog)).toEqual({
    role: "integrator", taskGrade: "senior", domainRequirements: [], topology: "worker",
    tier: "frontier", reasoning: "xhigh", posture: "deliver",
    composition: { kind: "template", id: "integrator", overrides: ["tier", "reasoning"],
      overrideReason: "cross-seam direction" },
  });
  expect(applyOrchestrationStaffing({ role: "director" }, catalog)).toEqual({
    role: "director", taskGrade: "staff", domainRequirements: [], topology: "orchestrator",
    tier: "frontier", reasoning: "xhigh", posture: "deliver",
    composition: { kind: "template", id: "director", overrides: [] },
  });
  expect(applyOrchestrationStaffing({ role: "guardian" }, catalog)).toMatchObject({
    role: "guardian", topology: "worker", posture: "preserve",
  });
  expect(applyOrchestrationStaffing({ role: "curator" }, catalog)).toMatchObject({
    role: "curator", topology: "worker", posture: "prune",
  });
  expect(() => applyOrchestrationStaffing({
    role: "integrator", posture: "preserve",
    composition: { kind: "template", id: "integrator", overrides: ["posture"],
      overrideReason: "invalid preservation probe" },
  }, catalog)).toThrow("preserve posture requires a non-authoring capability boundary");
  // The retirement is the invariant; the successor list is naming churn. The
  // CLI-facing wording stays pinned in agents-cli-routing.test.ts.
  expect(() => applyOrchestrationStaffing({ role: "researcher" }, catalog))
    .toThrow("role researcher is retired");
});

test("North CLI reads staffing/catalog.json and carries authority-compatible independent overrides", () => {
  // --ad-hoc, deliberately: this probe resolves staffing and never becomes a
  // run, so there is no effort to attribute. The gate still fires on --dry-run
  // by design — a preview that accepted what the real spawn refuses would lie
  // about admissibility precisely where the preview is meant to be trusted.
  const result = spawnSync("bb", [resolve(north, "cli/agents-cli.clj"), "spawn", "executor", "settle completed work",
    "--dry-run", "--ad-hoc", "--taskGrade", "principal", "--domain", "computer-science",
    "--tier", "frontier", "--reasoning", "xhigh", "--posture", "prune",
    "--override-reason", "principal bounded retirement"], {
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1", ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("grade=principal tier=frontier reasoning=xhigh");
  expect(result.stdout).toContain("AGENT_DOMAIN_REQUIREMENTS=[\"computer-science\"]");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=worker");
  expect(result.stdout).toContain("AGENT_COMPOSITION={\"kind\":\"template\",\"id\":\"executor\",\"overrides\":[\"taskGrade\",\"domainRequirements\",\"tier\",\"reasoning\",\"posture\"],\"overrideReason\":\"principal bounded retirement\"}");
});

test("North rejects unlogged bespoke roles and composition identity mismatches", () => {
  const catalog = loadOrchestrationStaffing(resolve(orchestration, "staffing/catalog.json"));
  expect(() => applyOrchestrationStaffing({ role: "special" }, catalog))
    .toThrow("unknown Orchestration role special requires composition.kind=bespoke");
  expect(() => validateRoutingMetadata({
    role: "integrator", composition: { kind: "template", id: "scout", overrides: [] },
  })).toThrow("composition.id must match canonical role integrator");
  expect(() => applyOrchestrationStaffing({
    role: "special", taskGrade: "staff", domainRequirements: [], topology: "worker",
    tier: "frontier", reasoning: "xhigh", posture: "explore",
    composition: { kind: "bespoke", id: "special", nearestTemplate: "analyst",
      bespokeReason: "novel one-off", promotionCandidate: false, contract: JSON.parse(contract) },
  }, catalog)).not.toThrow();
});

test("bespoke Orchestration composition rationale survives North validation", () => {
  const request = composed("migration-forensics", "--rationale",
    "provenance tracing plus schema recovery", "--contract", contract, "--no-promotion-candidate",
    "--task-grade", "senior", "--topology", "worker", "--tier", "senior",
    "--reasoning", "high", "--posture", "explore");
  const metadata = validateRoutingMetadata(request);
  expect(metadata.composition).toMatchObject({
    kind: "bespoke", id: "migration-forensics",
    bespokeReason: "provenance tracing plus schema recovery", promotionCandidate: false,
  });
  expect(metadata.composition).toMatchObject({
    bespokeReason: "provenance tracing plus schema recovery",
    promotionCandidate: false,
  });
  expect(metadata.composition).not.toHaveProperty("nearestTemplate");
});

test("North MCP advertises the complete composition contract", () => {
  const request = `${JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list", params: {} })}\n`;
  const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], { input: request, encoding: "utf8" });
  expect(result.status).toBe(0);
  const response = JSON.parse(result.stdout.trim());
  const spawn = response.result.tools.find((tool: any) => tool.name === "spawn");
  for (const field of ["role", "taskGrade", "domainRequirements", "topology", "tier", "reasoning", "posture", "composition", "target"])
    expect(spawn.inputSchema.properties[field]).toBeDefined();
  const dispatch = response.result.tools.find((tool: any) => tool.name === "dispatch");
  expect(dispatch.inputSchema.properties.target).toBeDefined();
  expect(spawn.inputSchema.properties.tokenTarget).toMatchObject({
    type: "integer", minimum: 1, maximum: Number.MAX_SAFE_INTEGER,
  });
  expect(dispatch.inputSchema.properties.tokenTarget).toMatchObject({
    type: "integer", minimum: 1, maximum: Number.MAX_SAFE_INTEGER,
  });
  expect(spawn.inputSchema.required).toEqual([
    "prompt", "role", "taskGrade", "domainRequirements", "topology",
    "tier", "reasoning", "posture", "composition",
  ]);
  expect(dispatch.inputSchema.required).toEqual([
    "id", "role", "taskGrade", "domainRequirements", "topology",
    "tier", "reasoning", "posture", "composition",
  ]);
  expect(spawn.inputSchema.properties.reasoning.enum).toContain("xhigh");
  expect(spawn.inputSchema.properties.posture.enum).toEqual(
    ["explore", "deliver", "preserve", "prune", "evaluate"],
  );
  expect(dispatch.inputSchema.properties.posture.enum).toEqual(
    ["explore", "deliver", "preserve", "prune", "evaluate"],
  );
  expect(spawn.inputSchema.properties.composition.oneOf).toHaveLength(2);
});
