import test from "node:test";
import assert from "node:assert/strict";
import {
  resolveExecutionPlan, summarizeSelectionEvidence,
} from "../scripts/model-selection.mjs";

const inventory = [
  { provider: "openai", model: "gpt-5.6-luna", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
  { provider: "openai", model: "gpt-5.6-terra", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
  { provider: "openai", model: "gpt-5.6-sol", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
];

function request(overrides = {}) {
  return {
    role: "implementer",
    taskGrade: "mid",
    domainRequirements: [],
    topology: "worker",
    capabilityFloor: "standard",
    serviceClass: "balanced",
    reasoning: "medium",
    posture: "deliver",
    composition: {
      kind: "template",
      id: "implementer",
      overrides: Object.keys(overrides).filter((field) =>
        ["taskGrade", "domainRequirements", "capabilityFloor", "serviceClass", "reasoning", "posture"].includes(field)),
      ...(Object.keys(overrides).length ? { overrideReason: "focused resolver fixture" } : {}),
    },
    ...overrides,
  };
}

test("one resolver preserves floor and effort, keeps Terra explicit-only, and reacts to evidence and inventory", () => {
  const baseline = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "low" }),
    inventory,
  });
  assert.deepEqual(baseline.selected, {
    provider: "openai",
    model: "gpt-5.6-luna",
    effort: "low",
    reason: "balanced:rework>intervention>pricePerQualityPass>latencyPerQualityPass>tokens>catalogPrior;evidence=prior;policy=model-selection-2026-08-30.2",
  });
  assert.equal(baseline.assignment.kind, "control");
  assert.deepEqual(baseline.baseline, baseline.selected);
  assert(baseline.excluded.some(({ actionId, reason }) =>
    actionId === "openai/gpt-5.6-terra@low" && reason === "explicit-only-model"));

  const advanced = resolveExecutionPlan({
    request: request({ capabilityFloor: "advanced", reasoning: "high" }),
    inventory,
  });
  assert.equal(advanced.selected.model, "gpt-5.6-sol");
  assert.equal(advanced.selected.effort, "high");

  const failedLuna = Array.from({ length: 8 }, () => ({
    provider: "openai", model: "gpt-5.6-luna", effort: "low",
    qualityPassed: false, processSucceeded: true,
    durationMs: 100, priceMicrousd: 10,
  }));
  const calibrated = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "low" }),
    inventory,
    evidence: failedLuna,
  });
  assert.equal(calibrated.selected.model, "gpt-5.6-sol");
  assert(calibrated.excluded.some(({ actionId, reason }) =>
    actionId === "openai/gpt-5.6-luna@low" && reason === "quality-floor"));

  const racedInventory = inventory.map((row) =>
    row.model === "gpt-5.6-luna" ? { ...row, available: false } : row);
  const replanned = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "low" }),
    inventory: racedInventory,
  });
  assert.equal(replanned.selected.model, "gpt-5.6-sol");
});

test("quality-gated price and latency objectives use only confident fit-for-purpose arms", () => {
  const observations = [
    ...Array.from({ length: 50 }, (_, index) => ({
      provider: "openai", model: "gpt-5.6-luna", effort: "low",
      qualityPassed: true, processSucceeded: true,
      reworkRequired: false, interventionRequired: false,
      durationMs: 200 + index, priceMicrousd: 10,
      inputTokens: 20, outputTokens: 10, reasoningTokens: 2,
      cacheReadTokens: 0, cacheWriteTokens: 0,
    })),
    ...Array.from({ length: 50 }, (_, index) => ({
      provider: "openai", model: "gpt-5.6-sol", effort: "low",
      qualityPassed: true, processSucceeded: true,
      reworkRequired: false, interventionRequired: false,
      durationMs: 100 + index, priceMicrousd: 20,
      inputTokens: 20, outputTokens: 10, reasoningTokens: 2,
      cacheReadTokens: 0, cacheWriteTokens: 0,
    })),
  ];
  const economy = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "low", serviceClass: "economy" }),
    inventory, evidence: observations,
  });
  assert.equal(economy.selected.model, "gpt-5.6-luna");
  assert.equal(economy.ranked[0].evidenceStatus, "eligible");
  const fast = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "low", serviceClass: "fast" }),
    inventory, evidence: observations,
  });
  assert.equal(fast.selected.model, "gpt-5.6-sol");
  assert.equal(fast.ranked[0].metrics.pricePerQualityPass, 20);
});

test("bounded model x effort exploration preserves the capability floor, share cap, and Terra exclusion", () => {
  const routing = request({ capabilityFloor: "baseline", reasoning: "low" });
  const explored = resolveExecutionPlan({
    request: routing,
    inventory,
    exploration: {
      enabled: true, episodeId: "episode-12", periodId: "2026-08-30",
      eligibleRuns: 9, explorationRuns: 0,
      minimumReasoning: "low", allowedEfforts: ["low", "medium"],
    },
  });
  assert.equal(explored.assignment.kind, "explore");
  assert.equal(explored.selected.model, "gpt-5.6-sol");
  assert.equal(explored.selected.effort, "low");
  assert(!explored.assignment.selectedActionId.includes("terra"));
  assert.deepEqual(explored.requirements, {
    capabilityFloor: "baseline", serviceClass: "balanced", reasoning: "low",
  });

  const capped = resolveExecutionPlan({
    request: routing,
    inventory,
    exploration: {
      enabled: true, episodeId: "episode-12", periodId: "2026-08-30",
      eligibleRuns: 10, explorationRuns: 1,
      minimumReasoning: "low", allowedEfforts: ["low", "medium"],
    },
  });
  assert.equal(capped.assignment.kind, "control");
  assert.equal(capped.assignment.reason, "exploration:share-bound");
});

test("daily and weekly calibration retains exact price, token categories, rework, and intervention coverage", () => {
  const evidence = [
    {
      provider: "openai", model: "gpt-5.6-sol", effort: "medium",
      at: "2026-08-24T12:00:00.000Z",
      route: {
        role: "implementer", taskGrade: "mid", topology: "worker",
        capabilityFloor: "standard", posture: "deliver", domainRequirements: ["typescript"],
      },
      qualityPassed: true, processSucceeded: true,
      reworkRequired: false, interventionRequired: false,
      durationMs: 120, priceMicrousd: 50,
      inputTokens: 100, outputTokens: 20, reasoningTokens: 10,
      cacheReadTokens: 5, cacheWriteTokens: 2,
    },
    {
      provider: "openai", model: "gpt-5.6-sol", effort: "medium",
      at: "2026-08-25T12:00:00.000Z",
      route: {
        role: "implementer", taskGrade: "mid", topology: "worker",
        capabilityFloor: "standard", posture: "deliver", domainRequirements: ["typescript"],
      },
      qualityPassed: false, processSucceeded: true,
      reworkRequired: true, interventionRequired: true,
      durationMs: 240, priceMicrousd: 70,
      inputTokens: 200, outputTokens: 40, reasoningTokens: 20,
      cacheReadTokens: 10, cacheWriteTokens: 4,
    },
  ];
  const daily = summarizeSelectionEvidence({ evidence, cadence: "daily" });
  assert.equal(daily.periods.length, 2);
  assert.equal(daily.population.included, 2);
  const weekly = summarizeSelectionEvidence({ evidence, cadence: "weekly" });
  assert.equal(weekly.periods.length, 1);
  const arm = weekly.periods[0].arms[0];
  assert.equal(arm.priceMicrousd.mean, 60);
  assert.equal(arm.reasoningTokens.total, 30);
  assert.equal(arm.rework.rate, 0.5);
  assert.equal(arm.intervention.rate, 0.5);
  assert.equal(arm.expectedPriceMicrousdPerQualityPass, 120);
});
