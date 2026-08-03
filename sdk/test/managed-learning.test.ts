import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { decideManagedLearning } from "../src/managed-learning";
import type { RoutingAssessment } from "../src/routing-economics";
import { presetRequest } from "./routing-fixtures";

const previousPolicy = process.env.NORTH_LEARNING_POLICY;
const directory = mkdtempSync(join(tmpdir(), "north-managed-learning-"));
const policyPath = join(directory, "policy.json");

const assessment: RoutingAssessment = {
  version: "minimum-sufficient-v1",
  signals: {
    decisionOwnership: "none", seamScope: "none",
    errorExposure: "contained-reversible", oracleStrength: "objective-local",
    foundationalImpact: "none", dependencyShape: "atomic-cohesive",
    reasoningShape: "deterministic",
  },
  derived: {
    minimumTier: "economy", minimumReasoning: "low",
    ruleCodes: ["reasoning-shape:deterministic"],
  },
  selected: { tier: "economy", reasoning: "low" },
};

beforeAll(() => {
  writeFileSync(policyPath, JSON.stringify({
    version: 1, mode: "learning", intensity: 1,
    axes: ["model-tier", "effort", "prompt", "authoring", "history"],
    maxTierDelta: 1, riskCeiling: "p1", seed: "managed-test", epoch: "1",
    evidenceMode: "evaluation", graphTextExperiment: "armed",
  }));
  process.env.NORTH_LEARNING_POLICY = policyPath;
});

afterAll(() => {
  if (previousPolicy === undefined) delete process.env.NORTH_LEARNING_POLICY;
  else process.env.NORTH_LEARNING_POLICY = previousPolicy;
  rmSync(directory, { recursive: true, force: true });
});

describe("managed learning integration", () => {
  test("an admitted low-risk episode applies exactly one provider-neutral route treatment", () => {
    const baseline = presetRequest("executor");
    const decision = decideManagedLearning({
      episodeId: "managed-route-treatment",
      taskSignature: { class: "fixture" }, taskSignatureCoverage: "exact",
      routingMetadata: baseline, routingAssessment: assessment,
    });
    expect(decision.assignment.arm).toBe("explore");
    expect(["model-tier", "effort"]).toContain(decision.assignment.axis);
    const changed = [
      decision.routingMetadata.tier !== baseline.tier,
      decision.routingMetadata.reasoning !== baseline.reasoning,
    ].filter(Boolean);
    expect(changed).toHaveLength(1);
    expect(decision.routingAssessment?.exception?.code).toBe("calibration-experiment");
    expect(decision.assignment.taskSignatureCoverage).toBe("exact");
  });

  test("a non-calibration route exception pins model tier and effort to control", () => {
    const baseline = presetRequest("executor");
    const decision = decideManagedLearning({
      episodeId: "managed-route-pinned",
      taskSignature: { class: "fixture" }, taskSignatureCoverage: "exact",
      routingMetadata: baseline,
      routingAssessment: {
        ...assessment,
        exception: { code: "unmodeled-risk", detail: "operator-owned route" },
      },
    });
    expect(decision.assignment.arm).toBe("control");
    expect(decision.assignment.narrowingReason).toBe("arms:none-eligible");
    expect(decision.routingMetadata).toEqual(baseline);
  });

  test("an eligible graph/text spawn changes no second learning axis", () => {
    const baseline = presetRequest("integrator");
    const decision = decideManagedLearning({
      episodeId: "managed-graph-text-treatment",
      taskSignature: { class: "fixture" }, taskSignatureCoverage: "exact",
      routingMetadata: baseline, routingAssessment: assessment,
      graphTextExperimentEligible: true,
    });
    expect(decision.assignment.graphTextExperiment.status).toBe("assigned");
    expect(["graph", "text"]).toContain(decision.assignment.graphTextExperiment.arm);
    expect(decision.assignment.arm).toBe("control");
    expect(decision.routingMetadata).toEqual(baseline);
  });
});
