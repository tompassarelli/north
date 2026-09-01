import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validateRoutingRequest } from "./routing-request.mjs";
import {
  "expected-cost-per-pass" as expectedCostPerPass,
  "exploration-share-allows?" as explorationShareAllows,
  "wilson-lower-bound" as wilsonLowerBound,
  "wilson-upper-bound" as wilsonUpperBound,
} from "./selection-statistics.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const MODEL_SELECTION_CATALOG_SCHEMA_ID =
  "urn:agent-machinery:schema:model-selection-catalog:v2";
export const MODEL_SELECTION_PLAN_VERSION = "agent-machinery-execution-plan:v2";
export const MODEL_CALIBRATION_REPORT_VERSION = "agent-machinery-model-calibration:v1";
export const MODEL_SELECTION_CATALOG_PATH = resolve(ROOT, "selection/catalog.json");

const CAPABILITY_FLOORS = ["baseline", "standard", "advanced", "frontier"];
const SERVICE_CLASSES = ["economy", "fast", "balanced", "premium"];
const REASONING_LEVELS = ["low", "medium", "high", "xhigh", "max"];
const METRICS = [
  "quality", "success", "rework", "intervention", "latency", "price", "tokens",
  "pricePerQualityPass", "latencyPerQualityPass", "catalogPrior",
];
const OBSERVATION_METRICS = [
  "durationMs", "priceMicrousd", "inputTokens", "outputTokens", "reasoningTokens",
  "cacheReadTokens", "cacheWriteTokens",
];

function object(value, label) {
  if (value == null || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value;
}

function exactKeys(value, required, label) {
  const actual = Object.keys(object(value, label));
  const unknown = actual.filter((key) => !required.includes(key));
  const missing = required.filter((key) => !Object.hasOwn(value, key));
  if (unknown.length) throw new Error(`${label} has unknown field(s): ${unknown.join(", ")}`);
  if (missing.length) throw new Error(`${label} is missing field(s): ${missing.join(", ")}`);
}

function uniqueStrings(value, allowed, label) {
  if (!Array.isArray(value) || value.length === 0 ||
      value.some((item) => typeof item !== "string" || !allowed.includes(item)) ||
      new Set(value).size !== value.length)
    throw new Error(`${label} must contain unique values from: ${allowed.join(", ")}`);
  return value;
}

function probability(value, label) {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1)
    throw new Error(`${label} must be between 0 and 1`);
}

function positiveInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 1) throw new Error(`${label} must be a positive integer`);
}

export function validateModelSelectionCatalog(value) {
  const catalog = object(value, "model selection catalog");
  exactKeys(catalog,
    ["$schema", "version", "policyRevision", "qualityPolicy", "explorationPolicy", "serviceObjectives", "providers"],
    "model selection catalog");
  if (catalog.$schema !== MODEL_SELECTION_CATALOG_SCHEMA_ID)
    throw new Error(`model selection catalog.$schema must be ${MODEL_SELECTION_CATALOG_SCHEMA_ID}`);
  if (catalog.version !== 2) throw new Error("model selection catalog.version must be 2");
  if (typeof catalog.policyRevision !== "string" || !catalog.policyRevision.trim())
    throw new Error("model selection catalog.policyRevision must be non-empty");

  const quality = object(catalog.qualityPolicy, "model selection catalog.qualityPolicy");
  exactKeys(quality,
    ["minimumQuality", "minimumSuccess", "minimumObservations", "nicheMinimumObservations"],
    "model selection catalog.qualityPolicy");
  probability(quality.minimumQuality, "minimumQuality");
  probability(quality.minimumSuccess, "minimumSuccess");
  positiveInteger(quality.minimumObservations, "minimumObservations");
  positiveInteger(quality.nicheMinimumObservations, "nicheMinimumObservations");
  if (quality.nicheMinimumObservations < quality.minimumObservations)
    throw new Error("nicheMinimumObservations must not be below minimumObservations");

  const exploration = object(catalog.explorationPolicy,
    "model selection catalog.explorationPolicy");
  exactKeys(exploration,
    ["maximumShare", "minimumEligibleRuns", "maximumEffortDistance", "seed"],
    "model selection catalog.explorationPolicy");
  probability(exploration.maximumShare, "explorationPolicy.maximumShare");
  if (exploration.maximumShare === 0)
    throw new Error("explorationPolicy.maximumShare must be greater than zero");
  positiveInteger(exploration.minimumEligibleRuns, "explorationPolicy.minimumEligibleRuns");
  if (!Number.isSafeInteger(exploration.maximumEffortDistance)
      || exploration.maximumEffortDistance < 0
      || exploration.maximumEffortDistance >= REASONING_LEVELS.length)
    throw new Error("explorationPolicy.maximumEffortDistance is invalid");
  if (typeof exploration.seed !== "string" || !/^[A-Za-z0-9][A-Za-z0-9_.:/-]*$/.test(exploration.seed))
    throw new Error("explorationPolicy.seed must be a portable identifier");

  const objectives = object(catalog.serviceObjectives, "model selection catalog.serviceObjectives");
  exactKeys(objectives, SERVICE_CLASSES, "model selection catalog.serviceObjectives");
  for (const serviceClass of SERVICE_CLASSES)
    uniqueStrings(objectives[serviceClass], METRICS, `service objective ${serviceClass}`);

  if (!Array.isArray(catalog.providers) || catalog.providers.length === 0)
    throw new Error("model selection catalog.providers must be non-empty");
  const actions = new Set();
  for (const provider of catalog.providers) {
    exactKeys(provider, ["id", "models"], "model selection provider");
    if (typeof provider.id !== "string" || !provider.id.trim())
      throw new Error("model selection provider.id must be non-empty");
    if (!Array.isArray(provider.models) || provider.models.length === 0)
      throw new Error(`${provider.id}.models must be non-empty`);
    for (const model of provider.models) {
      exactKeys(model,
        ["id", "capabilityFloors", "efforts", "automaticEligible", "niche", "catalogPrior"],
        `model ${provider.id}/${model?.id ?? "<unknown>"}`);
      if (typeof model.id !== "string" || !model.id.trim())
        throw new Error(`${provider.id} model.id must be non-empty`);
      const action = `${provider.id}/${model.id}`;
      if (actions.has(action)) throw new Error(`duplicate model selection action ${action}`);
      actions.add(action);
      uniqueStrings(model.capabilityFloors, CAPABILITY_FLOORS, `${action}.capabilityFloors`);
      uniqueStrings(model.efforts, REASONING_LEVELS, `${action}.efforts`);
      if (typeof model.automaticEligible !== "boolean" || typeof model.niche !== "boolean")
        throw new Error(`${action} automaticEligible and niche must be boolean`);
      const prior = object(model.catalogPrior, `${action}.catalogPrior`);
      exactKeys(prior, SERVICE_CLASSES, `${action}.catalogPrior`);
      for (const serviceClass of SERVICE_CLASSES)
        if (!Number.isSafeInteger(prior[serviceClass]) || prior[serviceClass] < 0)
          throw new Error(`${action}.catalogPrior.${serviceClass} must be a nonnegative integer`);
    }
  }
  return catalog;
}

export function loadModelSelectionCatalog(path = MODEL_SELECTION_CATALOG_PATH) {
  return validateModelSelectionCatalog(JSON.parse(readFileSync(path, "utf8")));
}

function safeNonnegative(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}

function mean(values) {
  const known = values.flatMap((value) => {
    const safe = safeNonnegative(value);
    return safe === undefined ? [] : [safe];
  });
  return known.length === 0 ? undefined : known.reduce((sum, value) => sum + value, 0) / known.length;
}

function rate(observations, field) {
  const known = observations.flatMap((observation) =>
    typeof observation[field] === "boolean" ? [observation[field]] : []);
  if (known.length === 0) return { known: 0 };
  const successes = known.filter(Boolean).length;
  return {
    known: known.length,
    successes,
    estimate: successes / known.length,
    lower: wilsonLowerBound(successes, known.length),
    upper: wilsonUpperBound(successes, known.length),
  };
}

const OBSERVATION_FIELDS = new Set([
  "provider", "model", "effort", "at", "route", "taskSignature",
  "qualityPassed", "processSucceeded", "reworkRequired", "interventionRequired",
  ...OBSERVATION_METRICS,
]);
const ROUTE_CONTEXT_FIELDS = new Set([
  "role", "taskGrade", "topology", "capabilityFloor", "posture", "domainRequirements",
]);

function validateObservation(value, index) {
  const observation = object(value, `model selection evidence[${index}]`);
  const unknown = Object.keys(observation).filter((field) => !OBSERVATION_FIELDS.has(field));
  if (unknown.length)
    throw new Error(`model selection evidence[${index}] has unknown field(s): ${unknown.join(", ")}`);
  for (const field of ["provider", "model"])
    if (typeof observation[field] !== "string" || !observation[field].trim())
      throw new Error(`model selection evidence[${index}].${field} must be non-empty`);
  if (!REASONING_LEVELS.includes(observation.effort))
    throw new Error(`model selection evidence[${index}].effort is invalid`);
  for (const field of ["qualityPassed", "processSucceeded", "reworkRequired", "interventionRequired"])
    if (observation[field] !== undefined && typeof observation[field] !== "boolean")
      throw new Error(`model selection evidence[${index}].${field} must be boolean when known`);
  for (const field of OBSERVATION_METRICS)
    if (observation[field] !== undefined && safeNonnegative(observation[field]) === undefined)
      throw new Error(`model selection evidence[${index}].${field} must be nonnegative when known`);
  if (observation.at !== undefined &&
      (typeof observation.at !== "string" || !Number.isFinite(Date.parse(observation.at))))
    throw new Error(`model selection evidence[${index}].at must be an ISO timestamp when known`);
  if (observation.taskSignature !== undefined &&
      (typeof observation.taskSignature !== "string" || !observation.taskSignature.trim()))
    throw new Error(`model selection evidence[${index}].taskSignature must be non-empty when known`);
  if (observation.route !== undefined) {
    const route = object(observation.route, `model selection evidence[${index}].route`);
    const unknownRoute = Object.keys(route).filter((field) => !ROUTE_CONTEXT_FIELDS.has(field));
    if (unknownRoute.length)
      throw new Error(`model selection evidence[${index}].route has unknown field(s): ${unknownRoute.join(", ")}`);
    for (const field of ["role", "taskGrade", "topology", "capabilityFloor", "posture"])
      if (typeof route[field] !== "string" || !route[field].trim())
        throw new Error(`model selection evidence[${index}].route.${field} must be non-empty`);
    if (!Array.isArray(route.domainRequirements) ||
        route.domainRequirements.some((domain) => typeof domain !== "string" || !domain.trim()))
      throw new Error(`model selection evidence[${index}].route.domainRequirements is invalid`);
  }
  return observation;
}

export function validateModelSelectionEvidence(value) {
  if (!Array.isArray(value)) throw new Error("model selection evidence must be an array");
  return value.map(validateObservation);
}

function routeContextMatches(observation, request) {
  if (observation.route === undefined) return true;
  const route = observation.route;
  return route.role === request.role
    && route.taskGrade === request.taskGrade
    && route.topology === request.topology
    && route.capabilityFloor === request.capabilityFloor
    && route.posture === request.posture
    && JSON.stringify([...route.domainRequirements].sort()) ===
      JSON.stringify([...request.domainRequirements].sort());
}

function evidenceFor(evidence, provider, model, effort, request) {
  return evidence.filter((observation) => observation.provider === provider &&
    observation.model === model && observation.effort === effort &&
    routeContextMatches(observation, request));
}

function tokenTotal(observation) {
  const values = ["inputTokens", "outputTokens", "reasoningTokens", "cacheReadTokens", "cacheWriteTokens"]
    .map((field) => safeNonnegative(observation[field]));
  return values.every((value) => value === undefined)
    ? undefined : values.reduce((sum, value) => sum + (value ?? 0), 0);
}

function candidateEstimate(provider, model, effort, request, evidence, policy) {
  const observations = evidenceFor(evidence, provider.id, model.id, effort, request);
  const quality = rate(observations, "qualityPassed");
  const success = rate(observations, "processSucceeded");
  const rework = rate(observations, "reworkRequired");
  const intervention = rate(observations, "interventionRequired");
  const required = model.niche ? policy.nicheMinimumObservations : policy.minimumObservations;
  const belowFloor = (quality.estimate !== undefined && quality.known >= required &&
      quality.estimate < policy.minimumQuality) ||
    (success.estimate !== undefined && success.known >= required &&
      success.estimate < policy.minimumSuccess);
  const evidenceStatus = belowFloor ? "quality-floor" :
    observations.length < required || quality.known < required || success.known < required ?
      (model.niche ? "niche-prior" : "prior") :
      quality.lower < policy.minimumQuality || success.lower < policy.minimumSuccess ?
        "insufficient-evidence" : "eligible";
  const meanPrice = mean(observations.map((observation) => observation.priceMicrousd));
  const meanLatency = mean(observations.map((observation) => observation.durationMs));
  const pricePerQualityPass = meanPrice === undefined || quality.successes === undefined
    ? -1 : expectedCostPerPass(meanPrice, quality.successes, quality.known);
  const latencyPerQualityPass = meanLatency === undefined || quality.successes === undefined
    ? -1 : expectedCostPerPass(meanLatency, quality.successes, quality.known);
  return {
    provider: provider.id,
    model: model.id,
    effort,
    actionId: `${provider.id}/${model.id}@${effort}`,
    evidenceStatus,
    observationCount: observations.length,
    metrics: {
      quality: quality.lower,
      success: success.lower,
      rework: rework.known ? rework.upper : undefined,
      intervention: intervention.known ? intervention.upper : undefined,
      latency: meanLatency,
      price: meanPrice,
      tokens: mean(observations.map(tokenTotal)),
      pricePerQualityPass: pricePerQualityPass < 0 ? undefined : pricePerQualityPass,
      latencyPerQualityPass: latencyPerQualityPass < 0 ? undefined : latencyPerQualityPass,
      catalogPrior: model.catalogPrior[request.serviceClass],
    },
  };
}

function direction(metric) {
  return metric === "quality" || metric === "success" ? "max" : "min";
}

function compareMetric(left, right, metric) {
  const a = left.metrics[metric];
  const b = right.metrics[metric];
  if (a === undefined && b === undefined) return 0;
  if (a === undefined) return 1;
  if (b === undefined) return -1;
  return direction(metric) === "max" ? b - a : a - b;
}

function inventoryRows(value) {
  if (!Array.isArray(value)) throw new Error("model selection inventory must be an array");
  return value.map((row, index) => {
    const item = object(row, `model selection inventory[${index}]`);
    for (const field of ["provider", "model"])
      if (typeof item[field] !== "string" || !item[field].trim())
        throw new Error(`model selection inventory[${index}].${field} must be non-empty`);
    if (typeof item.available !== "boolean")
      throw new Error(`model selection inventory[${index}].available must be boolean`);
    uniqueStrings(item.efforts, REASONING_LEVELS, `model selection inventory[${index}].efforts`);
    return item;
  });
}

function nonnegativeInteger(value, label) {
  if (!Number.isSafeInteger(value) || value < 0)
    throw new Error(`${label} must be a nonnegative integer`);
  return value;
}

function portableId(value, label) {
  if (typeof value !== "string" || !/^[A-Za-z0-9][A-Za-z0-9_.:/-]*$/.test(value))
    throw new Error(`${label} must be a portable identifier`);
  return value;
}

function explorationInput(value, request) {
  if (value === undefined) return undefined;
  const input = object(value, "model selection exploration");
  exactKeys(input, [
    "enabled", "episodeId", "periodId", "eligibleRuns", "explorationRuns",
    "minimumReasoning", "allowedEfforts",
  ], "model selection exploration");
  if (typeof input.enabled !== "boolean")
    throw new Error("model selection exploration.enabled must be boolean");
  portableId(input.episodeId, "model selection exploration.episodeId");
  portableId(input.periodId, "model selection exploration.periodId");
  nonnegativeInteger(input.eligibleRuns, "model selection exploration.eligibleRuns");
  nonnegativeInteger(input.explorationRuns, "model selection exploration.explorationRuns");
  if (!REASONING_LEVELS.includes(input.minimumReasoning))
    throw new Error("model selection exploration.minimumReasoning is invalid");
  uniqueStrings(input.allowedEfforts, REASONING_LEVELS,
    "model selection exploration.allowedEfforts");
  if (REASONING_LEVELS.indexOf(input.minimumReasoning) > REASONING_LEVELS.indexOf(request.reasoning))
    throw new Error("model selection exploration minimum cannot exceed requested reasoning");
  return input;
}

function stableUnit(value) {
  const sample = createHash("sha256").update(value).digest().readBigUInt64BE(0) >> 11n;
  return Number(sample) / Number(0x20_0000_0000_0000n);
}

function modelCandidates({ request, rows, evidence, constraints, catalog, efforts, excluded }) {
  const candidates = [];
  for (const provider of catalog.providers) for (const model of provider.models) for (const effort of efforts) {
    const actionId = `${provider.id}/${model.id}@${effort}`;
    const inventory = rows.find((row) => row.provider === provider.id && row.model === model.id);
    const explicit = constraints.provider === provider.id && constraints.model === model.id;
    let reason;
    if (constraints.provider && constraints.provider !== provider.id) reason = "provider-constraint";
    else if (constraints.model && constraints.model !== model.id) reason = "model-constraint";
    else if (!model.automaticEligible && !explicit) reason = "explicit-only-model";
    else if (!model.capabilityFloors.includes(request.capabilityFloor)) reason = "capability-floor";
    else if (!model.efforts.includes(effort)) reason = "catalog-effort";
    else if (!inventory?.available) reason = "unavailable";
    else if (!inventory.efforts.includes(effort)) reason = "inventory-effort";
    if (reason) {
      excluded?.push({ actionId, reason });
      continue;
    }
    const estimate = candidateEstimate(provider, model, effort, request, evidence, catalog.qualityPolicy);
    if (estimate.evidenceStatus === "quality-floor" && !explicit) {
      excluded?.push({ actionId, reason: "quality-floor" });
      continue;
    }
    candidates.push(estimate);
  }
  return candidates;
}

function rankCandidates(candidates, objective) {
  return candidates.sort((left, right) => {
    const leftEligible = left.evidenceStatus === "eligible";
    const rightEligible = right.evidenceStatus === "eligible";
    if (leftEligible !== rightEligible) return leftEligible ? -1 : 1;
    if (!leftEligible && !rightEligible) {
      const prior = compareMetric(left, right, "catalogPrior");
      return prior || left.actionId.localeCompare(right.actionId);
    }
    for (const metric of objective) {
      const compared = compareMetric(left, right, metric);
      if (compared) return compared;
    }
    return left.actionId.localeCompare(right.actionId);
  });
}

function explorationAssignment(input, request, rows, evidence, constraints, catalog, baseline) {
  const control = (reason, propensity = 1) => Object.freeze({
    kind: "control", reason, propensity, baselineActionId: baseline.actionId,
    selectedActionId: baseline.actionId,
    ...(input ? { episodeId: input.episodeId, periodId: input.periodId } : {}),
  });
  if (!input?.enabled) return control("exploration:disabled");
  if (constraints.model) return control("exploration:model-pinned");
  if (input.eligibleRuns < catalog.explorationPolicy.minimumEligibleRuns)
    return control("exploration:minimum-runs");
  if (!explorationShareAllows(
    input.eligibleRuns, input.explorationRuns, catalog.explorationPolicy.maximumShare,
  )) return control("exploration:share-bound");
  const requestedIndex = REASONING_LEVELS.indexOf(request.reasoning);
  const minimumIndex = REASONING_LEVELS.indexOf(input.minimumReasoning);
  const efforts = input.allowedEfforts.filter((effort) => {
    const index = REASONING_LEVELS.indexOf(effort);
    return index >= minimumIndex && Math.abs(index - requestedIndex) <=
      catalog.explorationPolicy.maximumEffortDistance;
  });
  const treatments = modelCandidates({
    request, rows, evidence, constraints, catalog, efforts,
  }).filter(({ actionId }) => actionId !== baseline.actionId);
  if (treatments.length === 0) return control("exploration:no-treatment");
  const exploreDraw = stableUnit([
    catalog.explorationPolicy.seed, catalog.policyRevision, input.periodId,
    input.episodeId, "explore",
  ].join(":"));
  if (exploreDraw >= catalog.explorationPolicy.maximumShare)
    return control("exploration:control-draw", 1 - catalog.explorationPolicy.maximumShare);
  const fewest = Math.min(...treatments.map(({ observationCount }) => observationCount));
  const leastObserved = treatments.filter(({ observationCount }) => observationCount === fewest)
    .sort((left, right) => left.actionId.localeCompare(right.actionId));
  const armDraw = stableUnit([
    catalog.explorationPolicy.seed, catalog.policyRevision, input.periodId,
    input.episodeId, "arm",
  ].join(":"));
  const selected = leastObserved[Math.min(
    leastObserved.length - 1, Math.floor(armDraw * leastObserved.length),
  )];
  return Object.freeze({
    kind: "explore",
    reason: "exploration:bounded-model-effort",
    propensity: catalog.explorationPolicy.maximumShare / leastObserved.length,
    episodeId: input.episodeId,
    periodId: input.periodId,
    baselineActionId: baseline.actionId,
    selectedActionId: selected.actionId,
    treatment: selected,
  });
}

export function resolveExecutionPlan({
  request, inventory, evidence = [], constraints = {}, exploration,
  catalog: catalogValue,
}) {
  const catalog = validateModelSelectionCatalog(catalogValue ?? loadModelSelectionCatalog());
  validateRoutingRequest(request);
  const rows = inventoryRows(inventory);
  const observations = validateModelSelectionEvidence(evidence);
  const constraintObject = object(constraints, "model selection constraints");
  const unknownConstraints = Object.keys(constraintObject)
    .filter((field) => field !== "provider" && field !== "model");
  if (unknownConstraints.length)
    throw new Error(`model selection constraints has unknown field(s): ${unknownConstraints.join(", ")}`);
  for (const field of ["provider", "model"])
    if (constraintObject[field] !== undefined &&
        (typeof constraintObject[field] !== "string" || !constraintObject[field].trim()))
      throw new Error(`model selection constraints.${field} must be non-empty when supplied`);
  const calibratedExploration = explorationInput(exploration, request);
  const serviceClass = request.serviceClass;
  const objective = catalog.serviceObjectives[serviceClass];
  const excluded = [];
  const candidates = rankCandidates(modelCandidates({
    request, rows, evidence: observations, constraints: constraintObject, catalog,
    efforts: [request.reasoning], excluded,
  }), objective);
  if (candidates.length === 0)
    throw new Error(`no live model satisfies ${request.capabilityFloor}/${request.reasoning}/${request.serviceClass}`);
  const ranked = candidates.map((candidate, index) => ({
    ...candidate,
    rank: index + 1,
    reason: `${serviceClass}:${objective.join(">")};evidence=${candidate.evidenceStatus};policy=${catalog.policyRevision}`,
  }));
  const baseline = ranked[0];
  const assignment = explorationAssignment(
    calibratedExploration, request, rows, observations, constraintObject, catalog, baseline,
  );
  const chosen = assignment.kind === "explore" ? assignment.treatment : baseline;
  const selectedReason = assignment.kind === "explore"
    ? `${serviceClass}:${objective.join(">")};${assignment.reason};policy=${catalog.policyRevision}`
    : baseline.reason;
  return Object.freeze({
    version: MODEL_SELECTION_PLAN_VERSION,
    policyRevision: catalog.policyRevision,
    requirements: Object.freeze({
      capabilityFloor: request.capabilityFloor,
      serviceClass,
      reasoning: request.reasoning,
    }),
    baseline: Object.freeze({
      provider: baseline.provider, model: baseline.model, effort: baseline.effort,
      reason: baseline.reason,
    }),
    selected: Object.freeze({
      provider: chosen.provider, model: chosen.model, effort: chosen.effort,
      reason: selectedReason,
    }),
    assignment,
    ranked: Object.freeze(ranked),
    excluded: Object.freeze(excluded.sort((left, right) => left.actionId.localeCompare(right.actionId))),
  });
}

function metricSummary(observations, field) {
  const values = observations.flatMap((observation) => {
    const value = safeNonnegative(observation[field]);
    return value === undefined ? [] : [value];
  });
  return Object.freeze({
    known: values.length,
    unknown: observations.length - values.length,
    mean: values.length ? mean(values) : undefined,
    total: values.length ? values.reduce((sum, value) => sum + value, 0) : undefined,
  });
}

function rateSummary(observations, field) {
  const value = rate(observations, field);
  return Object.freeze({
    known: value.known,
    unknown: observations.length - value.known,
    ...(value.estimate === undefined ? {} : {
      count: value.successes, rate: value.estimate,
      lower95: value.lower, upper95: value.upper,
    }),
  });
}

function isoWeek(timestamp) {
  const date = new Date(timestamp);
  const day = date.getUTCDay() || 7;
  date.setUTCDate(date.getUTCDate() + 4 - day);
  const year = date.getUTCFullYear();
  const start = new Date(Date.UTC(year, 0, 1));
  const week = Math.ceil((((date - start) / 86_400_000) + 1) / 7);
  return `${year}-W${String(week).padStart(2, "0")}`;
}

function periodId(timestamp, cadence) {
  return cadence === "daily" ? new Date(timestamp).toISOString().slice(0, 10) : isoWeek(timestamp);
}

function routeSignature(observation) {
  if (!observation.route) return "global";
  const route = observation.route;
  return [
    route.role, route.taskGrade, route.topology, route.capabilityFloor, route.posture,
    [...route.domainRequirements].sort().join("+"),
  ].join("/");
}

function armSummary(actionId, stratum, observations, policy) {
  const quality = rateSummary(observations, "qualityPassed");
  const success = rateSummary(observations, "processSucceeded");
  const price = metricSummary(observations, "priceMicrousd");
  const latency = metricSummary(observations, "durationMs");
  const qualityStatus = quality.known < policy.minimumObservations ||
      success.known < policy.minimumObservations
    ? "insufficient-observations"
    : quality.rate < policy.minimumQuality || success.rate < policy.minimumSuccess
      ? "below-quality-floor"
      : quality.lower95 < policy.minimumQuality || success.lower95 < policy.minimumSuccess
        ? "insufficient-confidence" : "eligible";
  const expectedPrice = price.mean === undefined || quality.count === undefined
    ? -1 : expectedCostPerPass(price.mean, quality.count, quality.known);
  const expectedLatency = latency.mean === undefined || quality.count === undefined
    ? -1 : expectedCostPerPass(latency.mean, quality.count, quality.known);
  return Object.freeze({
    actionId, stratum, observations: observations.length, qualityStatus,
    quality, success,
    rework: rateSummary(observations, "reworkRequired"),
    intervention: rateSummary(observations, "interventionRequired"),
    durationMs: latency,
    priceMicrousd: price,
    inputTokens: metricSummary(observations, "inputTokens"),
    outputTokens: metricSummary(observations, "outputTokens"),
    reasoningTokens: metricSummary(observations, "reasoningTokens"),
    cacheReadTokens: metricSummary(observations, "cacheReadTokens"),
    cacheWriteTokens: metricSummary(observations, "cacheWriteTokens"),
    expectedPriceMicrousdPerQualityPass: expectedPrice < 0 ? undefined : expectedPrice,
    expectedDurationMsPerQualityPass: expectedLatency < 0 ? undefined : expectedLatency,
  });
}

export function summarizeSelectionEvidence({
  evidence, cadence = "daily", since, until, catalog: catalogValue,
}) {
  const catalog = validateModelSelectionCatalog(catalogValue ?? loadModelSelectionCatalog());
  const observations = validateModelSelectionEvidence(evidence);
  if (cadence !== "daily" && cadence !== "weekly")
    throw new Error("model calibration cadence must be daily or weekly");
  const lower = since === undefined ? -Infinity : Date.parse(since);
  const upper = until === undefined ? Infinity : Date.parse(until);
  if (Number.isNaN(lower) || Number.isNaN(upper) || lower > upper)
    throw new Error("model calibration time window is invalid");
  const included = observations.filter((observation) => observation.at !== undefined &&
    Date.parse(observation.at) >= lower && Date.parse(observation.at) < upper);
  const groups = new Map();
  for (const observation of included) {
    const period = periodId(observation.at, cadence);
    const stratum = routeSignature(observation);
    const actionId = `${observation.provider}/${observation.model}@${observation.effort}`;
    const key = `${period}\u0000${stratum}\u0000${actionId}`;
    const group = groups.get(key) ?? { period, stratum, actionId, observations: [] };
    group.observations.push(observation);
    groups.set(key, group);
  }
  const periods = new Map();
  for (const group of groups.values()) {
    const arms = periods.get(group.period) ?? [];
    arms.push(armSummary(
      group.actionId, group.stratum, group.observations, catalog.qualityPolicy,
    ));
    periods.set(group.period, arms);
  }
  const missingTimestamp = observations.filter(({ at }) => at === undefined).length;
  return Object.freeze({
    version: MODEL_CALIBRATION_REPORT_VERSION,
    policyRevision: catalog.policyRevision,
    cadence,
    window: Object.freeze({ since: since ?? null, until: until ?? null }),
    population: Object.freeze({
      supplied: observations.length,
      included: included.length,
      excludedMissingTimestamp: missingTimestamp,
      excludedOutsideWindow: observations.length - included.length - missingTimestamp,
    }),
    periods: Object.freeze([...periods.entries()]
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([period, arms]) => Object.freeze({
        period,
        arms: Object.freeze(arms.sort((left, right) =>
          left.stratum.localeCompare(right.stratum) || left.actionId.localeCompare(right.actionId))),
      }))),
  });
}
