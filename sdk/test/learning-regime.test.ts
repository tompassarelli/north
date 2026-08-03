import { describe, expect, test } from "bun:test";
import {
  assignLearningEpisode, DEFAULT_LEARNING_POLICY, learningAssignmentFacts,
  graphTextExperimentAssignment, validateLearningPolicy, type LearningAssignmentInput,
} from "../src/learning-regime";
import {
  buildEnvironmentReceipt, buildPromptReceipt, buildRunEnvelope, sha256Bytes,
} from "../src/composition-receipt";

const digest = (value: string) => sha256Bytes(value);

function assignmentInput(episodeId = "episode-1"): LearningAssignmentInput {
  return {
    episodeId,
    taskSignatureSha256: digest("task-shape"),
    taskSignatureCoverage: "exact",
    risk: "p1",
    baseline: {
      modelTier: "standard", effort: "medium",
      prompt: "baseline", authoring: "text", history: "git",
    },
    hardFloor: { modelTier: "standard", effort: "medium" },
    eligibleArms: {
      "model-tier": ["economy", "senior", "frontier"],
      effort: ["low", "high", "xhigh"],
      prompt: ["concise"],
      // No declared authoring/history alternate means those axes are control.
    },
  };
}

describe("learning regime assignment", () => {
  test("frozen is the safe default and deterministic", () => {
    const left = assignLearningEpisode(DEFAULT_LEARNING_POLICY, assignmentInput());
    const right = assignLearningEpisode(DEFAULT_LEARNING_POLICY, assignmentInput());
    expect(left).toEqual(right);
    expect(left.arm).toBe("control");
    expect(left.narrowingReason).toBe("mode:frozen");
    expect(left.propensity.assigned).toBe(1);
  });

  test("learning changes at most one eligible axis without crossing floors or delta", () => {
    const policy = validateLearningPolicy({
      ...DEFAULT_LEARNING_POLICY, mode: "learning", intensity: 1,
      axes: ["model-tier", "effort"], maxTierDelta: 1,
    });
    const assignment = assignLearningEpisode(policy, assignmentInput());
    expect(assignment.arm).toBe("explore");
    expect(["model-tier", "effort"]).toContain(assignment.axis);
    expect(Object.keys(assignment.options)).not.toContain("authoring");
    expect(assignment.options["model-tier"]).toEqual(["senior"]);
    expect(assignment.options.effort).toEqual(["high"]);
    expect(learningAssignmentFacts(assignment)).toContainEqual([
      "learning_assignment_sha256", assignment.manifestSha256,
    ]);
  });

  test("unknown or above-ceiling risk narrows to control", () => {
    const policy = validateLearningPolicy({
      ...DEFAULT_LEARNING_POLICY, mode: "learning", intensity: 1,
    });
    expect(assignLearningEpisode(policy, { ...assignmentInput(), risk: undefined }).narrowingReason)
      .toBe("risk:unknown");
    expect(assignLearningEpisode(policy, { ...assignmentInput(), risk: "p3" }).narrowingReason)
      .toBe("risk:above-ceiling");
  });

  test("armed graph/text assignment is deterministic and balanced independently of generic axes", () => {
    const policy = validateLearningPolicy({
      ...DEFAULT_LEARNING_POLICY, graphTextExperiment: "armed",
    });
    const left = assignLearningEpisode(policy, assignmentInput("fleet-episode"), "eligible");
    const right = assignLearningEpisode(policy, assignmentInput("fleet-episode"), "eligible");
    expect(left.graphTextExperiment).toEqual(right.graphTextExperiment);
    expect(["graph", "text"]).toContain(left.graphTextExperiment.arm);
    expect(left.graphTextExperiment.status).toBe("assigned");
    expect(left.graphTextExperiment.applied).toBe(true);
    expect(learningAssignmentFacts(left)).toContainEqual([
      "graph_text_experiment_arm", left.graphTextExperiment.arm,
    ]);
    expect(graphTextExperimentAssignment(policy, "pinned", "pinned-graph"))
      .toMatchObject({ status: "pinned", arm: "graph", applied: false,
        reason: "operator-pinned-authoring-surface" });
  });
});

describe("content-addressed composition receipts", () => {
  const source = digest("module-source");

  test("module order, parameters, and branch decisions are manifest-visible", () => {
    const one = buildPromptReceipt({
      coverage: "exact", wirePrompt: "AB",
      modules: [
        { id: "a", schemaVersion: "v1", position: 0, sourceSha256: source, rendered: "A" },
        { id: "b", schemaVersion: "v1", position: 1, dependencies: ["a"], sourceSha256: source,
          rendered: "B", safeParameters: { style: "short" }, parameterDigests: { task: digest("task") } },
      ],
      branches: [{ ruleId: "constitution", conditionId: "provider", inputDigest: digest("route"), branch: "included" }],
    });
    const reordered = buildPromptReceipt({
      coverage: "exact", wirePrompt: "AB",
      modules: [
        { id: "b", schemaVersion: "v1", position: 0, sourceSha256: source, rendered: "A" },
        { id: "a", schemaVersion: "v1", position: 1, sourceSha256: source, rendered: "B" },
      ],
    });
    expect(one.wireBytesSha256).toBe(reordered.wireBytesSha256);
    expect(one.manifestSha256).not.toBe(reordered.manifestSha256);
    expect(one.modules[1]?.safeParameters).toEqual({ style: "short" });
    expect(one.branches[0]?.branch).toBe("included");
    expect(JSON.stringify(one)).not.toContain("module-source");
    expect(() => buildPromptReceipt({
      coverage: "exact", wirePrompt: "x",
      modules: [{ id: "x", schemaVersion: "v1", position: 0, sourceSha256: source,
        rendered: "x", safeParameters: { api_token: "do-not-record" } }],
    })).toThrow("secret-shaped");
  });

  test("environment changes do not alter prompt receipt and activated closure differs from catalog", () => {
    const prompt = buildPromptReceipt({
      coverage: "exact", wirePrompt: "A",
      modules: [{ id: "a", schemaVersion: "v1", position: 0, sourceSha256: source, rendered: "A" }],
    });
    const environment = (hook: string, activated: boolean) => buildEnvironmentReceipt({
      availableSkills: [{ id: "skill-a", sha256: digest("catalog"), coverage: "exact" }],
      activatedResources: activated
        ? [{ id: "skill-a-resource", sha256: digest("resource"), coverage: "exact" }]
        : [],
      tools: [{ id: "read", sha256: digest("tool"), coverage: "exact" }],
      hooks: [{ id: "guard", sha256: digest(hook), coverage: "exact" }],
      configs: [], executables: [], instructions: [],
    });
    const left = environment("hook-v1", false);
    const right = environment("hook-v2", true);
    expect(left.manifestSha256).not.toBe(right.manifestSha256);
    expect(prompt.manifestSha256).toBe(prompt.manifestSha256);
    expect(right.availableSkillCatalogSha256).not.toBe(right.activatedResourceClosureSha256);
    const run = buildRunEnvelope({
      promptReceipt: prompt, environmentReceipt: right, assignmentSha256: digest("assignment"),
      tier: "standard", effort: "medium", providerAdapterVersion: "adapter-v1",
      providerRuntimeVersion: "runtime-v1",
    });
    expect(run.promptReceipt.manifestSha256).toBe(prompt.manifestSha256);
    expect(run.environmentReceipt.manifestSha256).toBe(right.manifestSha256);
  });

  test("missing activated closure remains unknown for evaluation exclusion", () => {
    const receipt = buildEnvironmentReceipt({
      availableSkills: [{ id: "skill-a", sha256: digest("catalog"), coverage: "exact" }],
      activatedResources: [{ id: "activation-observation", coverage: "unknown" }],
      tools: [], hooks: [], configs: [], executables: [], instructions: [],
      coverageReason: "activated-resource-observation-unavailable",
    });
    expect(receipt.coverage).toBe("unknown");
  });

  test("receipt builders reject ambiguous artifact identities and invalid runtime coverage", () => {
    expect(() => buildEnvironmentReceipt({
      availableSkills: [
        { id: "skill-a", sha256: digest("one"), coverage: "exact" },
        { id: "skill-a", sha256: digest("two"), coverage: "exact" },
      ],
      activatedResources: [], tools: [], hooks: [], configs: [], executables: [], instructions: [],
    })).toThrow("artifact ids must be unique");
    expect(() => buildPromptReceipt({
      coverage: "invented" as any, wirePrompt: "A",
      modules: [{ id: "a", schemaVersion: "v1", position: 0, sourceSha256: source, rendered: "A" }],
    })).toThrow("invalid prompt receipt coverage");
  });
});
