import { afterEach, describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import { canonicalRole, routingMetadataFromEnv, validateRoutingMetadata } from "../src/routing-metadata";
import { newRunId } from "../src/telemetry";

const ENV_KEYS = [
  "AGENT_ROLE", "AGENT_TASK_GRADE", "AGENT_DOMAIN_REQUIREMENTS", "AGENT_TOPOLOGY",
  "AGENT_TIER", "AGENT_REASONING", "AGENT_POSTURE", "AGENT_COMPOSITION",
];
afterEach(() => { for (const key of ENV_KEYS) delete process.env[key]; });

const bespokeContract = {
  responsibility: "reconstruct migration provenance",
  deliverable: "an evidence-linked timeline",
  capabilities: ["filesystem.read", "filesystem.search", "shell.readonly", "coordination"] as const,
  mayDecide: ["which read-only traces to follow"],
  mustEscalate: ["destructive recovery"],
  doneWhen: ["every transition is sourced"],
  report: "timeline, contradictions, and gaps",
};

describe("Orchestration routing metadata boundary", () => {
  test("accepts and normalizes the complete composition payload", () => {
    process.env.AGENT_TASK_GRADE = "staff";
    process.env.AGENT_ROLE = "migration-forensics";
    process.env.AGENT_DOMAIN_REQUIREMENTS = JSON.stringify(["distributed-systems", "Nix"]);
    process.env.AGENT_TOPOLOGY = "orchestrator";
    process.env.AGENT_TIER = "frontier";
    process.env.AGENT_REASONING = "xhigh";
    process.env.AGENT_POSTURE = "explore";
    process.env.AGENT_COMPOSITION = JSON.stringify({
      kind: "bespoke", id: "migration-forensics", nearestPreset: "analyst",
      bespokeReason: "crosses provenance and schema recovery", promotionCandidate: true,
      contract: bespokeContract,
    });
    expect(routingMetadataFromEnv()).toEqual({
      role: "migration-forensics", taskGrade: "staff", domainRequirements: ["distributed-systems", "Nix"],
      topology: "orchestrator", tier: "frontier", reasoning: "xhigh", posture: "explore",
      composition: { kind: "bespoke", id: "migration-forensics", nearestPreset: "analyst",
        bespokeReason: "crosses provenance and schema recovery", promotionCandidate: true,
        contract: bespokeContract },
    });
  });

  test("rejects invalid grades, domains, topology, and unexplained bespoke roles", () => {
    expect(() => validateRoutingMetadata({ taskGrade: "guru" as any })).toThrow("taskGrade");
    expect(() => validateRoutingMetadata({ topology: "manager" as any })).toThrow("topology");
    expect(() => validateRoutingMetadata({ domainRequirements: [""] })).toThrow("domainRequirements");
    expect(() => validateRoutingMetadata({ domainRequirements: ["Nix", "Nix"] }))
      .toThrow("domainRequirements must not contain duplicates");
    expect(() => validateRoutingMetadata({ domainRequirements: ["Nix", " Nix "] }))
      .toThrow("domainRequirements must not contain duplicates");
    expect(() => validateRoutingMetadata({ topology: "verifier" as any })).toThrow("topology");
    expect(() => validateRoutingMetadata({ role: "x", composition: { kind: "bespoke", id: "x" } as any })).toThrow("bespokeReason");
    expect(() => validateRoutingMetadata({
      role: "x", composition: { kind: "bespoke", id: "x", nearestPreset: "analyst",
        bespokeReason: "one-off", promotionCandidate: false, contract: bespokeContract },
    })).toThrow("bespoke composition requires all routing axes");
  });

  test("retired role ids fail while explicit research functions remain canonical", () => {
    expect(() => canonicalRole("researcher")).toThrow("role researcher is retired");
    expect(() => canonicalRole("research-scientist")).toThrow("scientist");
    expect(() => canonicalRole("cs-researcher")).toThrow("scientist");
    expect(canonicalRole("scout")).toBe("scout");
    expect(canonicalRole("analyst")).toBe("analyst");
    expect(canonicalRole("scientist")).toBe("scientist");
    expect(canonicalRole("migration-forensics")).toBe("migration-forensics");
  });

  test("rejects unknown request fields and composition identity drift", () => {
    for (const field of ["provider", "invokedAs", "shape", "allocation"]) {
      expect(() => validateRoutingMetadata({ role: "integrator", [field]: "unexpected" } as any))
        .toThrow("routing metadata has unknown field");
    }
    expect(() => validateRoutingMetadata({
      role: "integrator", composition: { kind: "preset", id: "scout", overrides: [] },
    })).toThrow("composition.id must match canonical role integrator");
    expect(() => validateRoutingMetadata({
      role: "integrator", composition: { kind: "preset", id: "integrator", overrides: [], extra: true } as any,
    })).toThrow("composition has unknown field");
  });

  test("bespoke capability contracts reject widening, ambiguity, and missing authority", () => {
    const request = {
      role: "read-only-specialist", taskGrade: "senior", domainRequirements: [],
      topology: "worker", tier: "senior", reasoning: "high", posture: "preserve",
      composition: {
        kind: "bespoke", id: "read-only-specialist", bespokeReason: "no preset fits",
        promotionCandidate: false, contract: { ...bespokeContract },
      },
    } as const;
    expect(() => validateRoutingMetadata({
      ...request,
      composition: { ...request.composition, contract: { ...request.composition.contract, capabilities: [] } },
    } as any)).toThrow("capabilities must be a non-empty array");
    expect(() => validateRoutingMetadata({
      ...request,
      composition: { ...request.composition, contract: {
        ...request.composition.contract, capabilities: ["filesystem.read", "filesystem.read"],
      } },
    } as any)).toThrow("capabilities must not contain duplicates");
    expect(() => validateRoutingMetadata({
      ...request,
      composition: { ...request.composition, contract: {
        ...request.composition.contract, capabilities: ["filesystem.read", "root"],
      } },
    } as any)).toThrow("capabilities contain unknown values");
    expect(() => validateRoutingMetadata({
      ...request,
      composition: { ...request.composition, contract: {
        ...request.composition.contract, capabilities: ["filesystem.read", "shell", "shell.readonly"],
      } },
    } as any)).toThrow("shell and shell.readonly are mutually exclusive");
  });

  test("effective-authority closure rejects open shell capability sets", () => {
    const request = {
      role: "authority-probe", taskGrade: "senior", domainRequirements: [],
      topology: "worker", tier: "senior", reasoning: "high", posture: "preserve",
      composition: {
        kind: "bespoke", id: "authority-probe", bespokeReason: "closure probe",
        promotionCandidate: false, contract: { ...bespokeContract },
      },
    } as const;
    for (const [capabilities, diagnostic] of [
      [
        ["filesystem.search", "filesystem.write", "shell"],
        "composition.contract.capabilities: shell requires filesystem.read capability",
      ],
      [
        ["filesystem.read", "filesystem.search", "shell"],
        "composition.contract.capabilities: shell requires filesystem.write capability",
      ],
      [
        ["filesystem.search", "shell.readonly"],
        "composition.contract.capabilities: shell.readonly requires filesystem.read capability",
      ],
      [
        ["filesystem.read", "shell.readonly"],
        "composition.contract.capabilities: shell.readonly requires filesystem.search capability",
      ],
    ] as const) {
      expect(() => validateRoutingMetadata({
        ...request,
        composition: {
          ...request.composition,
          contract: { ...request.composition.contract, capabilities },
        },
      } as any)).toThrow(diagnostic);
    }
  });
});

test("North validates Orchestration's shared cross-harness routing fixtures", () => {
  const packagedPath = resolve(import.meta.dir, "fixtures/orchestration-routing-request.fixtures.json");
  const fixtures = JSON.parse(readFileSync(packagedPath, "utf8"));
  for (const fixture of fixtures.valid)
    expect(() => applyOrchestrationStaffing(validateRoutingMetadata(fixture.request))).not.toThrow();
  for (const fixture of fixtures.invalid)
    expect(() => applyOrchestrationStaffing(validateRoutingMetadata(fixture.request))).toThrow(fixture.errorContains);

  // Orchestration is canonical when present in a development workspace, but North's
  // packaged acceptance test never requires a sibling checkout.
  const orchestrationHome = process.env.NORTH_ORCHESTRATION_HOME ?? resolve(import.meta.dir, "../../orchestration");
  const canonicalPath = resolve(orchestrationHome, "contracts/routing-request.fixtures.json");
  if (existsSync(canonicalPath))
    expect(JSON.parse(readFileSync(canonicalPath, "utf8"))).toEqual(fixtures);
});

test("run ids remain distinct when the wall clock does not advance", () => {
  expect(newRunId("same-agent")).not.toBe(newRunId("same-agent"));
});
