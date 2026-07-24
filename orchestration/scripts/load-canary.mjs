#!/usr/bin/env node
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { loadStaffingCatalog, validateStaffingCatalog } from "./staffing-catalog.mjs";
import {
  PROVIDER_NAMES, loadProviderCatalog, resolvePinnedModelRoute, validateProviderCatalog,
} from "./provider-catalog.mjs";
import { validateRoutingRequest } from "./routing-request.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ADMISSIONS_PER_SAMPLE = 5_000;
const REFERENCE_BUDGET_MS = 10_000;
const fixtures = JSON.parse(readFileSync(
  resolve(root, "contracts/routing-request.fixtures.json"), "utf8",
));

// The canary envelope follows the algorithm, not a particular workstation:
// every admission walks eight fixed routing fields, one bounded composition,
// and two small provider catalogs. Five thousand such admissions get a very
// generous ten-second budget. The tail and retained-heap limits allow another
// order of magnitude of headroom while still catching accidental quadratic
// scans, synchronous stalls, or per-admission retention.
const ENVELOPE = Object.freeze({
  samples: 3,
  warmupAdmissions: 500,
  admissionsPerSample: ADMISSIONS_PER_SAMPLE,
  referenceBudgetMs: REFERENCE_BUDGET_MS,
  minThroughputPerSecond: ADMISSIONS_PER_SAMPLE * 1_000 / REFERENCE_BUDGET_MS,
  maxP99Ms: 10,
  maxRetainedHeapBytes: 64 * 1024 * 1024,
  complexity: "O(8 routing fields + bounded composition + 2 provider catalogs)",
});

if (typeof global.gc !== "function") {
  console.error("load canary requires: node --expose-gc scripts/load-canary.mjs");
  process.exit(2);
}

function loadValidatedCatalogs() {
  const started = performance.now();
  const staffing = validateStaffingCatalog(loadStaffingCatalog());
  const providers = PROVIDER_NAMES.map((name) =>
    validateProviderCatalog(loadProviderCatalog(name), name));
  return { staffing, providers, elapsedMs: performance.now() - started };
}

const initial = loadValidatedCatalogs();
const presetCases = initial.staffing.presets.map((preset) => ({
  name: `stock:${preset.name}`,
  valid: true,
  request: {
    role: preset.name,
    taskGrade: preset.taskGrade,
    domainRequirements: [],
    topology: preset.topology,
    tier: preset.tier,
    reasoning: preset.deliberation,
    posture: preset.posture,
    composition: { kind: "preset", id: preset.name, overrides: [] },
  },
}));
const cases = [
  ...presetCases,
  ...fixtures.valid.map((fixture) => ({ ...fixture, valid: true })),
  ...fixtures.invalid.map((fixture) => ({ ...fixture, valid: false })),
];

function candidateRoute(catalog, request) {
  const tier = catalog.tiers[request.tier];
  const supported = (tier?.efforts ?? tier?.reasoning ?? []).includes(request.reasoning);
  try {
    const resolved = resolvePinnedModelRoute(catalog, {
      model: tier?.model, tier: request.tier, reasoning: request.reasoning,
    });
    return supported && resolved.provider === catalog.provider &&
      resolved.tier === request.tier && resolved.reasoning === request.reasoning;
  } catch {
    return !supported;
  }
}

function exercise(testCase, staffing, providers, violations) {
  try {
    const admitted = validateRoutingRequest(testCase.request, staffing);
    if (!testCase.valid) {
      violations.push(`${testCase.name}: invalid request was admitted`);
      return;
    }
    if (admitted !== testCase.request)
      violations.push(`${testCase.name}: validator changed request identity`);
    if (!providers.every((catalog) => candidateRoute(catalog, admitted)))
      violations.push(`${testCase.name}: provider candidate resolution drifted`);
  } catch (error) {
    if (testCase.valid) {
      violations.push(`${testCase.name}: valid request failed: ${error.message}`);
    } else if (!error.message.includes(testCase.errorContains)) {
      violations.push(`${testCase.name}: wrong rejection: ${error.message}`);
    }
  }
}

function percentile(sorted, fraction) {
  return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)];
}

function rounded(value) {
  return Math.round(value * 1_000) / 1_000;
}

function runSample(index) {
  const { staffing, providers, elapsedMs: catalogValidationMs } = loadValidatedCatalogs();
  const violations = [];
  global.gc();
  const heapBefore = process.memoryUsage().heapUsed;
  const latencies = new Array(ENVELOPE.admissionsPerSample);
  const started = performance.now();
  for (let admission = 0; admission < ENVELOPE.admissionsPerSample; admission++) {
    const operationStarted = performance.now();
    exercise(cases[admission % cases.length], staffing, providers, violations);
    latencies[admission] = performance.now() - operationStarted;
  }
  const elapsedMs = performance.now() - started;
  latencies.sort((left, right) => left - right);
  const latencyMs = {
    p50: rounded(percentile(latencies, 0.50)),
    p95: rounded(percentile(latencies, 0.95)),
    p99: rounded(percentile(latencies, 0.99)),
  };
  latencies.length = 0;
  global.gc();
  const heapAfter = process.memoryUsage().heapUsed;
  return {
    sample: index,
    catalogValidationMs: rounded(catalogValidationMs),
    elapsedMs: rounded(elapsedMs),
    throughputPerSecond: rounded(ENVELOPE.admissionsPerSample * 1_000 / elapsedMs),
    latencyMs,
    retainedHeapBytes: Math.max(0, heapAfter - heapBefore),
    correctnessViolations: violations.length,
    violationExamples: violations.slice(0, 5),
  };
}

// Warm the module/JIT caches before the measured samples. The same deterministic
// corpus is used, but warm-up results cannot contribute to a passing verdict.
for (let admission = 0; admission < ENVELOPE.warmupAdmissions; admission++)
  exercise(cases[admission % cases.length], initial.staffing, initial.providers, []);

const samples = Array.from({ length: ENVELOPE.samples }, (_, index) => runSample(index + 1));
const aggregate = {
  minThroughputPerSecond: Math.min(...samples.map((sample) => sample.throughputPerSecond)),
  maxP50Ms: Math.max(...samples.map((sample) => sample.latencyMs.p50)),
  maxP95Ms: Math.max(...samples.map((sample) => sample.latencyMs.p95)),
  maxP99Ms: Math.max(...samples.map((sample) => sample.latencyMs.p99)),
  maxRetainedHeapBytes: Math.max(...samples.map((sample) => sample.retainedHeapBytes)),
  correctnessViolations: samples.reduce((sum, sample) => sum + sample.correctnessViolations, 0),
};
const verdicts = {
  throughput: aggregate.minThroughputPerSecond >= ENVELOPE.minThroughputPerSecond,
  tailLatency: aggregate.maxP99Ms <= ENVELOPE.maxP99Ms,
  retainedHeap: aggregate.maxRetainedHeapBytes <= ENVELOPE.maxRetainedHeapBytes,
  correctness: aggregate.correctnessViolations === 0,
};
const receipt = {
  version: "gaffer-load-canary:v1",
  workload: {
    samples: ENVELOPE.samples,
    warmupAdmissions: ENVELOPE.warmupAdmissions,
    admissionsPerSample: ENVELOPE.admissionsPerSample,
    cases: cases.length,
    validCases: cases.filter(({ valid }) => valid).length,
    invalidCases: cases.filter(({ valid }) => !valid).length,
    providerCatalogs: PROVIDER_NAMES.length,
    shape: "North preflight burst across provider candidates",
    validationSurface: [
      "staffing catalog/schema", "provider catalog/schema", "routing request",
      "preset composition", "bespoke composition", "exact candidate route",
    ],
  },
  envelope: ENVELOPE,
  samples,
  aggregate,
  verdicts,
};

console.log(`GAFFER_LOAD_RECEIPT ${JSON.stringify(receipt)}`);
if (Object.values(verdicts).every(Boolean)) {
  console.log("gaffer load canary: PASS");
} else {
  console.error(`gaffer load canary: FAIL ${Object.entries(verdicts)
    .filter(([, passed]) => !passed).map(([name]) => name).join(",")}`);
  process.exitCode = 1;
}
