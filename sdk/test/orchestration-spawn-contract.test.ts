import { expect, test } from "bun:test";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { validateRoutingMetadata } from "../src/routing-metadata";
import { applyOrchestrationStaffing, loadOrchestrationStaffing } from "../src/orchestration-staffing";

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
    "--tier", "frontier", "--deliberation", "xhigh", "--posture", "preserve",
    "--override-reason", "cross-provider foundational contract");
  const metadata = validateRoutingMetadata(request);
  expect(metadata).toEqual({
    role: "integrator", taskGrade: "staff", domainRequirements: ["Nix", "Beagle"],
    topology: "worker", tier: "frontier", reasoning: "xhigh", posture: "preserve",
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
  // The retirement is the invariant; the successor list is naming churn. The
  // CLI-facing wording stays pinned in agents-cli-routing.test.ts.
  expect(() => applyOrchestrationStaffing({ role: "researcher" }, catalog))
    .toThrow("role researcher is retired");
});

test("North CLI reads staffing/catalog.json and carries independent overrides", () => {
  // --ad-hoc, deliberately: this probe resolves staffing and never becomes a
  // run, so there is no effort to attribute. The gate still fires on --dry-run
  // by design — a preview that accepted what the real spawn refuses would lie
  // about admissibility precisely where the preview is meant to be trusted.
  const result = spawnSync("bb", [resolve(north, "cli/agents-cli.clj"), "spawn", "scout", "contract probe",
    "--dry-run", "--ad-hoc", "--taskGrade", "principal", "--domain", "computer-science",
    "--tier", "frontier", "--reasoning", "xhigh", "--posture", "preserve",
    "--override-reason", "principal research direction"], {
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1", ORCHESTRATION_STAFFING_CATALOG: resolve(orchestration, "staffing/catalog.json") },
  });
  expect(result.status).toBe(0);
  expect(result.stdout).toContain("grade=principal tier=frontier reasoning=xhigh");
  expect(result.stdout).toContain("AGENT_DOMAIN_REQUIREMENTS=[\"computer-science\"]");
  expect(result.stdout).toContain("AGENT_TOPOLOGY=worker");
  expect(result.stdout).toContain("AGENT_COMPOSITION={\"kind\":\"template\",\"id\":\"scout\",\"overrides\":[\"taskGrade\",\"domainRequirements\",\"tier\",\"reasoning\",\"posture\"],\"overrideReason\":\"principal research direction\"}");
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
  expect(spawn.inputSchema.properties.composition.oneOf).toHaveLength(2);
});
