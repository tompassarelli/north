import { expect, test } from "bun:test";
import { runFacts } from "../src/telemetry";
import {
  assignLearningEpisode, DEFAULT_LEARNING_POLICY,
} from "../src/learning-regime";
import {
  buildEnvironmentReceipt, buildPromptReceipt, buildRunEnvelope, sha256Bytes,
} from "../src/composition-receipt";

test("terminal telemetry carries immutable assignment and receipt identities, never raw prompt", () => {
  const source = sha256Bytes("source");
  const assignment = assignLearningEpisode(DEFAULT_LEARNING_POLICY, {
    episodeId: "telemetry-episode",
    taskSignatureSha256: sha256Bytes("task"),
    taskSignatureCoverage: "exact",
    risk: "p1",
    baseline: {
      modelTier: "standard", effort: "medium", prompt: "baseline",
      authoring: "text", history: "git",
    },
  });
  const promptReceipt = buildPromptReceipt({
    coverage: "exact",
    wirePrompt: "PRIVATE RAW PROMPT SENTINEL",
    modules: [{
      id: "core", schemaVersion: "v1", position: 0, sourceSha256: source,
      rendered: "PRIVATE RAW PROMPT SENTINEL",
    }],
  });
  const environmentReceipt = buildEnvironmentReceipt({
    availableSkills: [], activatedResources: [], tools: [], hooks: [],
    configs: [], executables: [], instructions: [],
  });
  const runEnvelopeReceipt = buildRunEnvelope({
    promptReceipt, environmentReceipt,
    assignmentSha256: assignment.manifestSha256,
    tier: "standard", effort: "medium",
    providerAdapterVersion: "adapter-v1", providerRuntimeVersion: "runtime-v1",
  });
  const facts = runFacts({
    thread: "@telemetry-learning", agent: "learning-lane", durationMs: 1,
    posture: "spawn", outcome: "ran", processOutcome: "ran",
    learningAssignment: assignment, promptReceipt, environmentReceipt, runEnvelopeReceipt,
    mcpActivity: {
      source: "fixture", coverage: "exact", tools: [],
      operationReceipts: [
        { tool: "fram/show", operation: "reasoning.inspect", durationMs: 2, resultSize: 1, outcome: "ok" },
        { tool: "fram/show", operation: "reasoning.inspect", durationMs: 3, resultSize: 1, outcome: "ok" },
        { tool: "fram/show", operation: "reasoning.inspect", durationMs: 4, resultSize: 1, outcome: "typed_failure" },
      ],
      operationAggregates: [{
        operation: "reasoning.inspect", count: 3, totalDurationMs: 9,
        meanDurationMs: 3, failureCount: 1,
      }],
    },
  });

  expect(facts).toContainEqual(["learning_assignment_sha256", assignment.manifestSha256]);
  expect(facts).toContainEqual(["prompt_receipt_sha256", promptReceipt.manifestSha256]);
  expect(facts).toContainEqual(["prompt_wire_sha256", promptReceipt.wireBytesSha256]);
  expect(facts).toContainEqual([
    "activated_resource_closure_sha256", environmentReceipt.activatedResourceClosureSha256,
  ]);
  expect(facts).toContainEqual(["run_envelope_sha256", runEnvelopeReceipt.manifestSha256]);
  expect(facts).toContainEqual(["graph_text_experiment_arm", "none"]);
  expect(facts).toContainEqual([
    "mcp_operation_aggregate",
    JSON.stringify({
      operation: "reasoning.inspect", count: 3, totalDurationMs: 9,
      meanDurationMs: 3, failureCount: 1,
    }),
  ]);
  expect(JSON.stringify(facts)).not.toContain("PRIVATE RAW PROMPT SENTINEL");
});
