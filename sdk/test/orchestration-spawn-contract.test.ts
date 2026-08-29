import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { validateRoutingMetadata } from "../src/routing-metadata";
import { applyOrchestrationStaffing, loadOrchestrationStaffing } from "../src/orchestration-staffing";
import { bindSpawnTestRuntime } from "../src/internal/test-runtime";
import { wireTurnQuery } from "./support/wire-query";
import type { RoutedQueryArguments } from "../src/providers";
import { researchProjectProfile } from "./routing-fixtures";

const north = resolve(import.meta.dir, "../..");
const agentMachinery = process.env.AGENT_MACHINERY_HOME ?? "/home/tom/code/agent-machinery/main";
const compose = resolve(agentMachinery, "scripts/compose-routing.mjs");
const researchProfileJson = JSON.stringify(researchProjectProfile());

function composed(...args: string[]): any {
  const result = spawnSync(process.execPath, [
    compose, ...args, "--project-profile", researchProfileJson,
  ], { encoding: "utf8" });
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
    "NORTH_STREAM_DIR", "NORTH_AGENT_LOGS_DIR", "NORTH_PORT", "NORTH_STORE_HOST",
    "AGENT_ID", "AGENT_ROLE", "AGENT_TASK_GRADE", "AGENT_DOMAIN_REQUIREMENTS",
    "AGENT_TOPOLOGY", "AGENT_TIER", "AGENT_REASONING", "AGENT_POSTURE",
    "AGENT_COMPOSITION", "AGENT_PROJECT_PROFILE", "AGENT_PROVIDER", "AGENT_TARGET", "AGENT_COORDINATOR",
    "AGENT_WORKTREE", "NORTH_ROUTING_POLICY", "NORTH_ROUTING_PIN_EVIDENCE", "NORTH_ENVELOPE_ACCOUNTING",
    "BEAGLE_STORE_HOME", "BEAGLE_STORE_BIN", "BEAGLE_STORE_OUT",
  ] as const;
  const saved = Object.fromEntries(keys.map((key) => [key, process.env[key]]));
  const route = applyOrchestrationStaffing({ role: "guardian" });
  try {
    writeFileSync(fakeNorth, `#!/usr/bin/env bash
if [ "$1 $2" = "json show" ]; then printf '%s\\n' '[]'; exit 0; fi
printf '%s\\n' "$*" >> ${JSON.stringify(log)}
`);
    chmodSync(fakeNorth, 0o700);
    writeFileSync(fakeBb, "#!/usr/bin/env bash\nexit 0\n");
    chmodSync(fakeBb, 0o700);
    const routingPolicy = join(directory, "routing-policy.json");
    writeFileSync(routingPolicy, JSON.stringify({
      version: 1, mode: "preferential", providerOrder: ["openai"],
      targets: [{ id: "codex-explicit-fixture", provider: "openai", authMode: "isolated", profile: "fixture" }],
      targetOrder: ["codex-explicit-fixture"],
    }));
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
    process.env.AGENT_PROJECT_PROFILE = JSON.stringify(researchProjectProfile());
    process.env.AGENT_PROVIDER = "openai";
    process.env.AGENT_TARGET = "codex-explicit-fixture";
    process.env.AGENT_MODEL = "gpt-5.6-sol";
    process.env.AGENT_COORDINATOR = "test-coordinator";
    process.env.AGENT_WORKTREE = "0";
    process.env.NORTH_ROUTING_POLICY = routingPolicy;
    const issuedAt = new Date();
    process.env.NORTH_ROUTING_PIN_EVIDENCE = JSON.stringify({
      policyVersion: "north-routing-pin-v1", issuedAt: issuedAt.toISOString(),
      expiresAt: new Date(issuedAt.getTime() + 60 * 60 * 1000).toISOString(),
      reasonCode: "explicit-human-request", detail: "hermetic exact child route",
      pins: [
        { kind: "provider", value: "openai" },
        { kind: "account", value: "codex-explicit-fixture" },
        { kind: "model", value: "gpt-5.6-sol" },
      ],
    });
    delete process.env.NORTH_ENVELOPE_ACCOUNTING;

    const { managedChildSpawnOptions, spawn } = await import("../src/spawn");
    const request = managedChildSpawnOptions("publish child startup identity");
    expect(request).toMatchObject({ provider: "openai", target: "codex-explicit-fixture",
      model: "gpt-5.6-sol", routingMetadata: { role: "guardian", posture: "preserve" } });
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
      executionSelection: true,
      worktreeAllocationWriter: { register: () => {}, event: () => {} },
      feedSubscriber: () => Object.assign(() => {}, {
        ready: Promise.resolve(), caughtUp: Promise.resolve(), replay: async () => {},
        drain: async () => {}, isArmed: () => true,
      }),
    });

    await spawn(request);
    const commands = readFileSync(log, "utf8");
    expect(commands).toContain(`tell agent:${agentId} kind lane`);
    expect(commands).toContain(`tell agent:${agentId} composition_id guardian`);
    expect(commands).toContain(`tell agent:${agentId} display_name`);
    expect(providerBoundaryCalls).toBe(1);

    const truncatedStore = createServer((socket) => socket.destroy());
    await new Promise<void>((resolve, reject) => {
      truncatedStore.once("error", reject);
      truncatedStore.listen(0, "127.0.0.1", resolve);
    });
    const storePort = (truncatedStore.address() as AddressInfo).port;
    process.env.NORTH_PORT = String(storePort);
    process.env.NORTH_STORE_HOST = "127.0.0.1";
    try {
      process.env.AGENT_PROVIDER = "openai";
      delete process.env.AGENT_TARGET;
      delete process.env.AGENT_MODEL;
      process.env.AGENT_TOPOLOGY = route.topology;
      process.env.NORTH_ROUTING_PIN_EVIDENCE = JSON.stringify({
        policyVersion: "north-routing-pin-v1", issuedAt: issuedAt.toISOString(),
        expiresAt: new Date(issuedAt.getTime() + 60 * 60 * 1000).toISOString(),
        reasonCode: "explicit-human-request", detail: "partial provider pin",
        pins: [{ kind: "provider", value: "openai" }],
      });
      const partial = managedChildSpawnOptions("partial pin must refuse before provider");
      delete process.env.AGENT_TOPOLOGY;
      bindSpawnTestRuntime(partial, {
        admitDispatchAuthority: () => {}, publishLearningAssignment: async () => "recorded" as const,
        queryFn: (args: RoutedQueryArguments) => {
          providerBoundaryCalls++;
          return wireTurnQuery(args, { provider: "openai", output: "must not run" });
        },
        executionSelection: true,
      });
      await expect(spawn(partial)).rejects.toMatchObject({ code: "rpc-truncated" });

      delete process.env.AGENT_PROVIDER;
      delete process.env.NORTH_ROUTING_PIN_EVIDENCE;
      process.env.AGENT_TOPOLOGY = route.topology;
      const automatic = managedChildSpawnOptions("automatic route must refuse before provider");
      delete process.env.AGENT_TOPOLOGY;
      bindSpawnTestRuntime(automatic, {
        admitDispatchAuthority: () => {}, publishLearningAssignment: async () => "recorded" as const,
        queryFn: (args: RoutedQueryArguments) => {
          providerBoundaryCalls++;
          return wireTurnQuery(args, { provider: "openai", output: "must not run" });
        },
        executionSelection: true,
      });
      await expect(spawn(automatic)).rejects.toMatchObject({ code: "rpc-truncated" });
      expect(providerBoundaryCalls).toBe(1);
    } finally {
      await new Promise<void>((resolve) => truncatedStore.close(() => resolve()));
    }
  } finally {
    for (const key of keys) {
      const value = saved[key];
      if (value === undefined) delete process.env[key]; else process.env[key] = value;
    }
    rmSync(directory, { recursive: true, force: true });
  }
});

test("SDK presets inherit catalog axes while declared compatible overrides win independently", () => {
  const catalog = loadOrchestrationStaffing(resolve(agentMachinery, "staffing/catalog.json"));
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
    env: {
      ...process.env,
      AGENT_PROJECT_PROFILE: researchProfileJson,
      NO_COLOR: "1",
      ORCHESTRATION_STAFFING_CATALOG: resolve(agentMachinery, "staffing/catalog.json"),
    },
  });
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("grade=principal tier=frontier reasoning=xhigh");
  expect(result.stdout).toContain("AGENT_DOMAIN_REQUIREMENTS=[\"computer-science\"]");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=worker");
  expect(result.stdout).toContain("AGENT_COMPOSITION={\"kind\":\"template\",\"id\":\"executor\",\"overrides\":[\"taskGrade\",\"domainRequirements\",\"tier\",\"reasoning\",\"posture\"],\"overrideReason\":\"principal bounded retirement\"}");

  const provenance = spawnSync("bb", [resolve(north, "cli/agents-cli.clj"), "spawn", "migration-scout", "trace the boundary",
    "--dry-run", "--ad-hoc", "--composition", JSON.stringify({ kind: "template", id: "scout", overrides: [] })], {
    encoding: "utf8",
    env: {
      ...process.env,
      AGENT_PROJECT_PROFILE: researchProfileJson,
      NO_COLOR: "1",
      ORCHESTRATION_STAFFING_CATALOG: resolve(agentMachinery, "staffing/catalog.json"),
    },
  });
  expect(provenance.status).toBe(0);
  expect(provenance.stdout).toContain("AGENT_ROLE=migration-scout");
  expect(provenance.stdout).toContain('AGENT_COMPOSITION={"kind":"template","id":"scout","overrides":[]}');
});

test("North SDK admits role and composition independently at both catalog boundaries", () => {
  const catalog = loadOrchestrationStaffing(resolve(agentMachinery, "staffing/catalog.json"));
  expect(() => applyOrchestrationStaffing({
    role: "special", composition: { kind: "template", id: "missing-template", overrides: [] },
  }, catalog)).toThrow("unknown stock template missing-template");
  expect(validateRoutingMetadata({
    role: "migration-scout", composition: { kind: "template", id: "scout", overrides: [] },
  })).toEqual({
    role: "migration-scout", composition: { kind: "template", id: "scout", overrides: [] },
  });
  expect(validateRoutingMetadata({
    role: "reviewer",
    taskGrade: "senior",
    domainRequirements: ["migration evidence"],
    topology: "worker",
    tier: "senior",
    reasoning: "high",
    posture: "evaluate",
    composition: { kind: "bespoke", id: "migration-forensics",
      bespokeReason: "one-off migration evidence review", promotionCandidate: false,
      contract: JSON.parse(contract) },
  })).toEqual({
    role: "reviewer",
    taskGrade: "senior",
    domainRequirements: ["migration evidence"],
    topology: "worker",
    tier: "senior",
    reasoning: "high",
    posture: "evaluate",
    composition: { kind: "bespoke", id: "migration-forensics",
      bespokeReason: "one-off migration evidence review", promotionCandidate: false,
      contract: JSON.parse(contract) },
  });
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
  expect(spawn.inputSchema.properties.taskGrade.enum).toEqual(
    ["novice", "junior", "mid", "senior", "staff", "principal", "distinguished"],
  );
  expect(dispatch.inputSchema.properties.taskGrade.enum).toEqual(
    ["novice", "junior", "mid", "senior", "staff", "principal", "distinguished"],
  );
  expect(spawn.inputSchema.properties.posture.enum).toEqual(
    ["explore", "deliver", "preserve", "prune", "evaluate"],
  );
  expect(dispatch.inputSchema.properties.posture.enum).toEqual(
    ["explore", "deliver", "preserve", "prune", "evaluate"],
  );
  expect(spawn.inputSchema.properties.composition.oneOf).toHaveLength(2);
});
