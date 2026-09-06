import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import Ajv2020 from "ajv/dist/2020.js";
import {
  loadModelSelectionCatalog, validateModelSelectionCatalog, resolveExecutionPlan, summarizeSelectionEvidence,
} from "../scripts/model-selection.mjs";
import { deriveSelectionAssessment, SELECTION_ASSESSMENT_VERSION } from "../scripts/selection-assessment.mjs";
import { effectivePreset } from "../scripts/routing-request.mjs";
import { loadStaffingCatalog } from "../scripts/staffing-catalog.mjs";

const inventory = [
  { provider: "openai", model: "gpt-6-astra", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
  { provider: "openai", model: "gpt-5.6-luna", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
  { provider: "openai", model: "gpt-5.6-terra", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
  { provider: "openai", model: "gpt-5.6-sol", available: true, efforts: ["low", "medium", "high", "xhigh", "max"] },
];

function request(overrides = {}) {
  const defaults = { taskGrade: "mid", domainRequirements: [], capabilityFloor: "standard", serviceClass: "balanced", reasoning: "medium", posture: "deliver" };
  const changed = Object.keys(overrides).filter((field) =>
    Object.hasOwn(defaults, field) && JSON.stringify(overrides[field]) !== JSON.stringify(defaults[field]));
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
      overrides: changed,
      ...(changed.length ? { overrideReason: "focused resolver fixture" } : {}),
    },
    ...overrides,
  };
}

test("the staffing catalog has matching structural and semantic model-effort constraints", () => {
  const catalog = loadModelSelectionCatalog();
  const schema = JSON.parse(readFileSync(new URL("../selection/catalog.schema.json", import.meta.url), "utf8"));
  const validate = new Ajv2020({ strict: false }).compile(schema);
  assert.equal(validate(catalog), true, JSON.stringify(validate.errors));
  assert.match(catalog.staffingPolicy.operatorPrior, /operator prior, not benchmark evidence/);
  const invalid = structuredClone(catalog);
  invalid.providers[0].models[0].effortPolicy.experiment.push("max");
  assert.equal(validate(invalid), true);
  assert.throws(() => validateModelSelectionCatalog(invalid), /maximum effort/);
  const wrongFloor = structuredClone(catalog);
  wrongFloor.providers[0].models.find(({ id }) => id === "gpt-5.6-luna").effortPolicy.capabilityFloors.low.push("frontier");
  assert.throws(() => validateModelSelectionCatalog(wrongFloor), /capabilityFloors/);
});

test("one resolver preserves floor and effort, keeps Terra explicit-only, and reacts to evidence and inventory", () => {
  const baseline = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "xhigh" }),
    inventory,
  });
  assert.deepEqual(baseline.selected, {
    provider: "openai",
    model: "gpt-5.6-luna",
    effort: "xhigh",
    reason: "balanced:rework>intervention>pricePerQualityPass>latencyPerQualityPass>tokens>catalogPrior;evidence=prior;policy=model-selection-2026-09-06.1",
  });
  assert.equal(baseline.assignment.kind, "control");
  assert.deepEqual(baseline.baseline, baseline.selected);
  assert(baseline.excluded.some(({ actionId, reason }) =>
    actionId === "openai/gpt-5.6-terra@xhigh" && reason === "explicit-only-model"));

  const advanced = resolveExecutionPlan({
    request: request({ capabilityFloor: "advanced", reasoning: "high" }),
    inventory,
  });
  assert.equal(advanced.selected.model, "gpt-6-astra");
  assert.equal(advanced.selected.effort, "high");

  const failedLuna = Array.from({ length: 8 }, () => ({
    provider: "openai", model: "gpt-5.6-luna", effort: "xhigh",
    qualityPassed: false, processSucceeded: true,
    durationMs: 100, priceMicrousd: 10,
  }));
  const calibrated = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "xhigh" }),
    inventory,
    evidence: failedLuna,
  });
  assert.equal(calibrated.selected.model, "gpt-6-astra");
  assert(calibrated.excluded.some(({ actionId, reason }) =>
    actionId === "openai/gpt-5.6-luna@xhigh" && reason === "quality-floor"));

  const racedInventory = inventory.map((row) =>
    row.model === "gpt-5.6-luna" ? { ...row, available: false } : row);
  const replanned = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "xhigh" }),
    inventory: racedInventory,
  });
  assert.equal(replanned.selected.model, "gpt-6-astra");
});

test("quality-gated price and latency objectives use only confident fit-for-purpose arms", () => {
  const observations = [
    ...Array.from({ length: 50 }, (_, index) => ({
      provider: "openai", model: "gpt-5.6-luna", effort: "xhigh",
      qualityPassed: true, processSucceeded: true,
      reworkRequired: false, interventionRequired: false,
      durationMs: 200 + index, priceMicrousd: 10,
      inputTokens: 20, outputTokens: 10, reasoningTokens: 2,
      cacheReadTokens: 0, cacheWriteTokens: 0,
    })),
    ...Array.from({ length: 50 }, (_, index) => ({
      provider: "openai", model: "gpt-6-astra", effort: "xhigh",
      qualityPassed: true, processSucceeded: true,
      reworkRequired: false, interventionRequired: false,
      durationMs: 100 + index, priceMicrousd: 20,
      inputTokens: 20, outputTokens: 10, reasoningTokens: 2,
      cacheReadTokens: 0, cacheWriteTokens: 0,
    })),
  ];
  const economy = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "xhigh", serviceClass: "economy" }),
    inventory, evidence: observations,
  });
  assert.equal(economy.selected.model, "gpt-5.6-luna");
  assert.equal(economy.ranked[0].evidenceStatus, "eligible");
  const fast = resolveExecutionPlan({
    request: request({ capabilityFloor: "baseline", reasoning: "xhigh", serviceClass: "fast" }),
    inventory, evidence: observations,
  });
  assert.equal(fast.selected.model, "gpt-6-astra");
  assert.equal(fast.ranked[0].metrics.pricePerQualityPass, 20);
});

test("bounded model x effort exploration preserves the capability floor, share cap, and Terra exclusion", () => {
  const routing = request();
  const treatments = new Set();
  let explorationRuns = 0;
  for (let eligibleRuns = 0; eligibleRuns < 1000; eligibleRuns++) {
    const input = {
      request: routing, inventory,
      exploration: {
        enabled: true, episodeId: `episode-${eligibleRuns}`, periodId: "2026-09-05",
        eligibleRuns, explorationRuns,
        minimumReasoning: "low", allowedEfforts: ["low", "medium", "high", "xhigh", "max"],
      },
    };
    const plan = resolveExecutionPlan(input);
    assert.deepEqual(resolveExecutionPlan(input), plan);
    assert.equal(plan.baseline.model, "gpt-6-astra");
    assert.equal(plan.baseline.effort, "medium");
    if (plan.assignment.kind === "explore") {
      explorationRuns++;
      treatments.add(plan.assignment.selectedActionId);
      assert(plan.assignment.propensity > 0 && plan.assignment.propensity <= 0.1);
      assert(!plan.assignment.selectedActionId.includes("terra"));
      assert(!plan.assignment.selectedActionId.includes("luna"));
      assert.notEqual(plan.selected.effort, "max");
    }
    assert(explorationRuns / (eligibleRuns + 1) <= 0.1);
  }
  assert(explorationRuns > 0);
  assert(treatments.has("openai/gpt-6-astra@low"));
  assert(treatments.has("openai/gpt-5.6-sol@high"));
  assert(treatments.has("openai/gpt-5.6-sol@xhigh"));

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

test("ordinary authoring defaults to Astra medium across service objectives and full inventory", () => {
  const staffing = loadStaffingCatalog();
  const authored = effectivePreset(staffing.presets.find(({ name }) => name === "implementer"), staffing);
  assert.equal(authored.reasoning, "medium");
  const fullInventory = loadModelSelectionCatalog().providers.flatMap(provider =>
    provider.models.map(model => ({ provider: provider.id, model: model.id, available: true, efforts: model.efforts })));
  for (const serviceClass of ["economy", "fast", "balanced", "premium"]) {
    const plan = resolveExecutionPlan({ request: request({ ...authored, serviceClass }), inventory: fullInventory });
    assert.equal(plan.selected.model, "gpt-6-astra");
    assert.equal(plan.selected.effort, "medium");
    assert(plan.excluded.some(({ actionId, reason }) =>
      actionId === "openai/gpt-5.6-sol@medium" && reason === "worker-default-policy"));
  }
  const providerChoice = resolveExecutionPlan({ request: request(), inventory: fullInventory, constraints: { provider: "anthropic" } });
  assert.equal(providerChoice.selected.provider, "anthropic");
  const openaiChoice = resolveExecutionPlan({ request: request({ serviceClass: "fast" }), inventory: fullInventory, constraints: { provider: "openai" } });
  assert.equal(openaiChoice.selected.model, "gpt-6-astra");
  assert.throws(() => resolveExecutionPlan({ request: request(), inventory: fullInventory.filter(row => row.model !== "gpt-6-astra") }), /no live model/);
  for (const reasoning of ["low", "high", "xhigh"]) {
    const plan = resolveExecutionPlan({ request: request({ reasoning }), inventory: fullInventory, constraints: { effort: reasoning } });
    assert.equal(plan.selected.model, "gpt-6-astra");
    assert.equal(plan.selected.effort, reasoning);
  }
});

test("worker priors, exact pins, model-local floors, and live effort availability are enforced", () => {
  for (const [floor, effort, model] of [
    ["baseline", "xhigh", "gpt-5.6-luna"], ["baseline", "max", "gpt-5.6-luna"],
    ["standard", "low", "gpt-6-astra"], ["standard", "medium", "gpt-6-astra"],
    ["advanced", "low", "gpt-6-astra"], ["frontier", "medium", "gpt-6-astra"],
    ["advanced", "medium", "gpt-6-astra"], ["advanced", "high", "gpt-6-astra"],
    ["frontier", "xhigh", "gpt-6-astra"],
  ]) {
    const plan = resolveExecutionPlan({ request: request({ capabilityFloor: floor, reasoning: effort }), inventory });
    assert.equal(plan.selected.model, model);
    assert.equal(plan.selected.effort, effort);
  }
  const sol = resolveExecutionPlan({ request: request(), inventory, constraints: { model: "gpt-5.6-sol", effort: "medium" } });
  assert.equal(sol.selected.model, "gpt-5.6-sol");
  assert.equal(sol.selected.effort, "medium");
  const terra = resolveExecutionPlan({ request: request(), inventory, constraints: { model: "gpt-5.6-terra" } });
  assert.equal(terra.selected.model, "gpt-5.6-terra");
  assert.throws(() => resolveExecutionPlan({ request: request(), inventory, constraints: { effort: "high" } }), /effort pin/);
  assert.throws(() => resolveExecutionPlan({
    request: request({ capabilityFloor: "frontier", reasoning: "low" }), inventory,
    constraints: { model: "gpt-6-astra" },
  }), /no live model/);
  assert.throws(() => resolveExecutionPlan({
    request: request({ capabilityFloor: "advanced", reasoning: "medium" }),
    inventory: inventory.map(row => row.model === "gpt-6-astra" ? { ...row, efforts: ["high"] } : row),
  }), /no live model/);
});

test("supervisors and named costly-to-reverse decisions never downshift", () => {
  const exploration = {
    enabled: true, episodeId: "protected", periodId: "2026-09-05", eligibleRuns: 99, explorationRuns: 0,
    minimumReasoning: "low", allowedEfforts: ["low", "medium", "high", "xhigh", "max"],
  };
  for (const reasoning of ["high", "xhigh"]) {
    const primary = resolveExecutionPlan({ request: request({ reasoning }), inventory, exploration, context: { supervisory: true } });
    assert.equal(primary.selected.model, "gpt-6-astra");
    assert.equal(primary.selected.effort, reasoning);
    assert.equal(primary.assignment.reason, "exploration:supervisor-excluded");
    const director = { ...request({ reasoning }), role: "team-lead", taskGrade: "staff", capabilityFloor: "advanced", topology: "orchestrator",
      composition: { kind: "template", id: "team-lead", overrides: reasoning === "high" ? [] : ["reasoning"], ...(reasoning === "xhigh" ? { overrideReason: "higher complexity" } : {}) } };
    const plan = resolveExecutionPlan({ request: director, inventory, exploration });
    assert.equal(plan.selected.model, "gpt-6-astra");
    assert.equal(plan.assignment.reason, "exploration:supervisor-excluded");
  }
  assert.throws(() => resolveExecutionPlan({ request: request(), inventory, context: { supervisory: true } }), /no live model/);
  assert.throws(() => resolveExecutionPlan({ request: request({ reasoning: "high" }), inventory,
    context: { supervisory: true }, constraints: { model: "gpt-5.6-sol" } }), /no live model/);
  const maximum = request({ capabilityFloor: "frontier", reasoning: "max" });
  assert.throws(() => resolveExecutionPlan({ request: maximum, inventory }), /no live model/);
  const decision = resolveExecutionPlan({ request: maximum, inventory, exploration,
    context: { loadBearingDecision: "Choose the durable data representation; changing it requires rewriting the owned archive." } });
  assert.equal(decision.selected.model, "gpt-6-astra");
  assert.equal(decision.selected.effort, "max");
  assert.equal(decision.assignment.reason, "exploration:load-bearing-excluded");
  const exceptionalPrimary = resolveExecutionPlan({ request: maximum, inventory,
    context: { supervisory: true, loadBearingDecision: "Choose archive representation; reversal requires rewriting the durable archive." } });
  assert.equal(exceptionalPrimary.selected.effort, "max");
  for (const constraints of [{ model: "gpt-5.6-sol" }, { effort: "medium" }]) {
    const pinned = resolveExecutionPlan({ request: request(), inventory, exploration, constraints });
    assert.equal(pinned.assignment.kind, "control");
    assert.match(pinned.assignment.reason, /pinned/);
    assert.equal(pinned.selected.effort, "medium");
  }
});

test("experiments exclude measured failures and cannot trade away competence", () => {
  const evidence = Array.from({ length: 8 }, () => ({ provider: "openai", model: "gpt-5.6-sol", effort: "high", qualityPassed: false, processSucceeded: true }));
  for (let i = 0; i < 200; i++) {
    const exploration = { enabled: true, episodeId: `quality-${i}`, periodId: "2026-09-05", eligibleRuns: 99, explorationRuns: 0,
      minimumReasoning: "low", allowedEfforts: ["low", "medium", "high", "xhigh", "max"] };
    const plan = resolveExecutionPlan({ request: request(), inventory, evidence, exploration });
    assert.notEqual(plan.assignment.selectedActionId, "openai/gpt-5.6-sol@high");
    const advanced = resolveExecutionPlan({ request: request({ capabilityFloor: "advanced", reasoning: "high" }), inventory, exploration });
    const model = loadModelSelectionCatalog().providers[0].models.find(({ id }) => id === advanced.selected.model);
    assert(model.effortPolicy.capabilityFloors[advanced.selected.effort].includes("advanced"));
    assert.notEqual(advanced.selected.model, "gpt-5.6-luna");
  }
});

test("assessment through authoring and resolution admits model-local effort without lowering competence", () => {
  for (const [capabilityFloor, reasoning, model, decisionOwnership] of [
    ["baseline", "max", "gpt-5.6-luna", "none"],
    ["advanced", "low", "gpt-6-astra", "cross-boundary"],
    ["frontier", "medium", "gpt-6-astra", "system-shaping"],
  ]) {
    const signals = { decisionOwnership, seamScope: "none", errorExposure: "contained-reversible",
      oracleStrength: "objective-end-to-end", foundationalImpact: "implementation-only",
      dependencyShape: "atomic-cohesive", reasoningShape: "deterministic" };
    const assessment = { version: SELECTION_ASSESSMENT_VERSION, signals,
      derived: deriveSelectionAssessment(signals), selected: { capabilityFloor, reasoning } };
    const result = spawnSync(process.execPath, [new URL("../scripts/compose-routing.mjs", import.meta.url).pathname,
      "implementer", "--assessment", JSON.stringify(assessment), "--override-reason", "model-local competence fixture"], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const plan = resolveExecutionPlan({ request: JSON.parse(result.stdout), inventory });
    assert.equal(plan.selected.model, model);
    assert.equal(plan.selected.effort, reasoning);
  }
});

test("one task-local Luna failure can escalate to Astra without banning Luna globally", () => {
  const routing = request({ capabilityFloor: "baseline", reasoning: "xhigh" });
  const evidence = [{ provider: "openai", model: "gpt-5.6-luna", effort: "xhigh",
    taskSignature: "mechanical-failed-check", qualityPassed: false, processSucceeded: true }];
  const escalated = resolveExecutionPlan({ request: routing, inventory, evidence, constraints: { model: "gpt-6-astra" } });
  assert.equal(escalated.selected.model, "gpt-6-astra");
  assert.equal(resolveExecutionPlan({ request: routing, inventory, evidence }).selected.model, "gpt-5.6-luna");
});

test("mechanical comparisons admit Luna max and model-local lower labels across models", () => {
  const treatments = new Set();
  for (let index = 0; index < 400; index++) {
    const plan = resolveExecutionPlan({
      request: request({ capabilityFloor: "baseline", reasoning: "max" }), inventory,
      exploration: { enabled: true, episodeId: `mechanical-${index}`, periodId: "2026-09-05",
        eligibleRuns: 99, explorationRuns: 0, minimumReasoning: "xhigh",
        allowedEfforts: ["low", "medium", "xhigh", "max"] },
    });
    assert.equal(plan.baseline.model, "gpt-5.6-luna");
    assert.equal(plan.baseline.effort, "max");
    if (plan.assignment.kind === "explore") treatments.add(plan.assignment.selectedActionId);
    assert.notEqual(plan.assignment.selectedActionId, "openai/gpt-6-astra@max");
  }
  assert(treatments.has("openai/gpt-6-astra@low"));
  assert(treatments.has("openai/gpt-5.6-sol@low"));
  assert(treatments.has("openai/gpt-5.6-luna@xhigh"));
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
