import { afterEach, beforeEach, expect, test } from "bun:test";
import {
  compileProviderAuthoritySurface, ProviderRetrySafeError, ProviderSelectionError,
  selectProvider, selectProviderFromAvailability,
} from "../src/providers";
import { balancedAllocationEstimates } from "../src/provider-routing";
import { RATE_LIMIT_WARNING_TTL_MS } from "../src/resource-policy";
import { markExecutionAdmission } from "../src/execution-admission";
import type {
  ProviderAvailability, ResourcePolicy, RoutingTarget,
} from "../src/providers/types";
import { ProviderCatalogFileCache, resolveTier } from "../src/providers/catalog";
import { anthropicProvider } from "../src/providers/anthropic";
import {
  READONLY_SHELL_SERVER, READONLY_SHELL_TOOL,
} from "../src/readonly-shell";
import { harnessOptions } from "../src/harness";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { providerCapabilityRejectionCode } from "../src/orchestration-capabilities";
import {
  WireEventWriter, wireEventId, wireRunId, type WireEvent, type WireQueryContext,
} from "../src/wire";

const MANAGED_ENV = [
  "NORTH_DISABLE_ANTHROPIC", "NORTH_DISABLE_OPENAI", "NORTH_PROVIDER_ORDER",
  "NORTH_ROUTING_POLICY", "NORTH_PROVIDER_OBSERVATIONS", "NORTH_ALLOCATION_MODE",
  "NORTH_PROVIDER_WEIGHTS", "NORTH_RESERVED_FRONTIER_PROVIDER",
  "NORTH_ANTHROPIC_ENTITLEMENT_PRESSURE", "NORTH_OPENAI_ENTITLEMENT_PRESSURE",
] as const;
const saved = Object.fromEntries(MANAGED_ENV.map((key) => [key, process.env[key]])) as Record<typeof MANAGED_ENV[number], string | undefined>;
const provedUnsent = (message: string, requestBytesPrepared = 0) =>
  ProviderRetrySafeError.provedUnsent(message, {
    mode: "managed",
    source: "adapter_preflight",
    requestBytesPrepared,
  });
const available: ProviderAvailability[] = [
  { provider: "anthropic", available: true, reason: "ready" },
  { provider: "openai", available: true, reason: "ready" },
];
const accountAvailability: ProviderAvailability[] = [
  { targetId: "claude-personal", provider: "anthropic", available: true, reason: "ready" },
  { targetId: "claude-work", provider: "anthropic", available: true, reason: "ready" },
  { targetId: "codex-personal", provider: "openai", available: true, reason: "ready" },
];
const policy = (overrides: Partial<ResourcePolicy> = {}): ResourcePolicy => ({
  mode: "preferential",
  providerOrder: ["anthropic", "openai"],
  targetPressures: { anthropic: "normal", openai: "normal" },
  ...overrides,
});
const accountPolicy = (overrides: Partial<ResourcePolicy> = {}): ResourcePolicy => policy({
  targets: [
    { id: "claude-personal", provider: "anthropic", authMode: "ambient" },
    { id: "claude-work", provider: "anthropic", authMode: "isolated", profile: "work" },
    { id: "codex-personal", provider: "openai", authMode: "ambient" },
  ],
  targetOrder: ["claude-personal", "claude-work", "codex-personal"],
  targetPressures: { "claude-personal": "normal", "claude-work": "normal", "codex-personal": "normal" },
  ...overrides,
});
function fableModelEvidence(target: RoutingTarget) {
  const observedAt = new Date();
  return {
    now: observedAt,
    store: {
      version: 1 as const,
      observations: [{
        provider: "anthropic" as const,
        targetId: target.id,
        authMode: target.authMode ?? "ambient" as const,
        ...(target.profile ? { profile: target.profile } : {}),
        observedAt: observedAt.toISOString(),
        source: "claude-agent-sdk:Query.supportedModels" as const,
        models: ["claude-fable-5"],
      }],
    },
  };
}
function codexModelEvidence(target: RoutingTarget) {
  const observedAt = new Date();
  return {
    now: observedAt,
    store: {
      version: 1 as const,
      observations: [{
        provider: "openai" as const,
        targetId: target.id,
        authMode: target.authMode ?? "ambient" as const,
        ...(target.profile ? { profile: target.profile } : {}),
        observedAt: observedAt.toISOString(),
        source: "codex-app-server:model-list" as const,
        models: ["gpt-5.6-sol"],
      }],
    },
  };
}
beforeEach(() => {
  for (const key of MANAGED_ENV) delete process.env[key];
  process.env.NORTH_ROUTING_POLICY = join(tmpdir(), `north-test-absent-policy-${process.pid}.json`);
  process.env.NORTH_PROVIDER_OBSERVATIONS = join(tmpdir(), `north-test-absent-observations-${process.pid}.json`);
});
afterEach(() => {
  for (const key of MANAGED_ENV) {
    if (saved[key] === undefined) delete process.env[key]; else process.env[key] = saved[key];
  }
});

test("explicit disabled provider fails loudly", () => {
  process.env.NORTH_DISABLE_ANTHROPIC = "1";
  expect(() => selectProvider("anthropic")).toThrow("provider anthropic unavailable: disabled");
});

test("auto order selects OpenAI when Anthropic is disabled", () => {
  process.env.NORTH_DISABLE_ANTHROPIC = "1";
  const decision = selectProvider("auto", policy(), {}, {
    probeAnthropic: () => ({ provider: "anthropic", available: false, reason: "disabled" }),
    probeOpenAI: () => ({ provider: "openai", available: true, reason: "ready" }),
  });
  expect(decision.provider).toBe("openai");
});

test("preferential allocation walks configured order and explains pressure", () => {
  const decision = selectProviderFromAvailability("auto", available,
    policy({ providerOrder: ["openai", "anthropic"], targetPressures: { openai: "plenty", anthropic: "normal" } }));
  expect(decision.provider).toBe("openai");
  expect(decision.selectionReason).toContain("mode=preferential");
  expect(decision.selectionReason).toContain("pressure=plenty");
});

test("automatic allocation avoids an exhausted entitlement", () => {
  const decision = selectProviderFromAvailability("auto", available,
    policy({ targetPressures: { anthropic: "exhausted", openai: "low" } }));
  expect(decision.provider).toBe("openai");
  expect(decision.selectionReason).toContain("pressure=low");

  try {
    selectProviderFromAvailability("auto", available,
      policy({ targetPressures: { anthropic: "exhausted", openai: "exhausted" } }));
    throw new Error("expected provider selection to fail");
  } catch (error) {
    expect(error).toBeInstanceOf(ProviderSelectionError);
    expect(error).toMatchObject({ kind: "no_provider_available", preSideEffect: true });
  }
});

test("explicit provider wins but exhausted explicit entitlement errors", () => {
  const decision = selectProviderFromAvailability("openai", available,
    policy({ targetPressures: { anthropic: "plenty", openai: "low" } }));
  expect(decision.provider).toBe("openai");
  expect(decision.selectionReason).toContain("explicit provider");
  expect(() => selectProviderFromAvailability("openai", available,
    policy({ targetPressures: { openai: "exhausted" } }))).toThrow("provider openai entitlement exhausted");
});

test("target pressure is independent and auto considers every configured account", () => {
  const decision = selectProviderFromAvailability("auto", accountAvailability, accountPolicy({
    targetPressures: { "claude-personal": "exhausted", "claude-work": "low", "codex-personal": "normal" },
  }));
  expect(decision.target).toBe("claude-work");
  expect(decision.provider).toBe("anthropic");
  expect(decision.entitlementPressure).toBe("low");
  expect(decision.targetEntitlementPressures["claude-personal"]).toBe("exhausted");
  expect(decision.fallbackTargets).toEqual(["codex-personal"]);
  expect(decision.fallbackProviders).toEqual(["openai"]);
});

test("exact target pin records the request and refuses sibling or provider fallback", () => {
  const healthy = selectProviderFromAvailability({ target: "claude-personal" }, accountAvailability, accountPolicy());
  expect(healthy).toMatchObject({
    requestedProvider: "auto", requestedTarget: "claude-personal",
    target: "claude-personal", provider: "anthropic",
    fallbackTargets: [], fallbackProviders: [], fallbackTargetPath: ["claude-personal"],
  });
  expect(() => selectProviderFromAvailability({ target: "claude-personal" }, accountAvailability, accountPolicy({
    targetPressures: { "claude-personal": "exhausted", "claude-work": "plenty", "codex-personal": "plenty" },
  }))).toThrow("routing target claude-personal entitlement exhausted");
  expect(() => selectProviderFromAvailability({ target: "claude-work", provider: "openai" }, accountAvailability, accountPolicy()))
    .toThrow("routing target claude-work belongs to anthropic, not requested provider openai");
});

test("probe scope follows the routing pin and isolates unrelated account failures", () => {
  let anthropicProbes = 0;
  let openAiProbes = 0;
  const dependencies = {
    probeAnthropic: () => {
      anthropicProbes++;
      return { provider: "anthropic" as const, available: true, reason: "ready" as const };
    },
    probeOpenAI: () => {
      openAiProbes++;
      throw new Error("hostile unrelated account bootstrap");
    },
  };
  const exact = selectProvider(
    { target: "claude-personal" }, accountPolicy(), { tier: "standard", reasoning: "medium" }, dependencies,
  );
  expect(exact.target).toBe("claude-personal");
  expect(anthropicProbes).toBe(1);
  expect(openAiProbes).toBe(0);

  anthropicProbes = 0;
  selectProvider("anthropic", accountPolicy(), { tier: "standard", reasoning: "medium" }, dependencies);
  expect(anthropicProbes).toBe(2);
  expect(openAiProbes).toBe(0);

  expect(selectProvider("auto", accountPolicy(),
    { tier: "standard", reasoning: "medium" }, dependencies).provider).toBe("anthropic");
  expect(openAiProbes).toBe(1);
});

test("provider pin filters cross-provider targets but retains same-provider siblings", () => {
  const decision = selectProviderFromAvailability("anthropic", accountAvailability, accountPolicy());
  expect(decision).toMatchObject({
    requestedProvider: "anthropic", target: "claude-personal", provider: "anthropic",
    fallbackTargets: ["claude-work"], fallbackProviders: ["anthropic"],
  });
  expect(decision.fallbackTargetPath).toEqual(["claude-personal"]);
});

test("same-provider target readiness is independent and isolated auth never borrows provider state", () => {
  const targetAware: ProviderAvailability[] = [
    { targetId: "claude-personal", provider: "anthropic", available: false, reason: "authentication_missing" },
    { targetId: "claude-work", provider: "anthropic", available: true, reason: "ready" },
    { targetId: "codex-personal", provider: "openai", available: true, reason: "ready" },
  ];
  expect(selectProviderFromAvailability("anthropic", targetAware, accountPolicy()).target).toBe("claude-work");

  const providerOnly = available;
  expect(() => selectProviderFromAvailability({ target: "claude-work" }, providerOnly, accountPolicy()))
    .toThrow("routing target claude-work unavailable through anthropic: unknown");
});

test("selection errors never interpolate untrusted availability detail", () => {
  const canary = "AVAILABILITY_CANARY_DO_NOT_EXPOSE";
  let caught: unknown;
  try {
    selectProviderFromAvailability("anthropic", [{
      provider: "anthropic", available: false, reason: "authentication_missing", detail: canary,
    }], policy({ providerOrder: ["anthropic"] }));
  } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ProviderSelectionError);
  expect((caught as Error).message).toBe("provider anthropic unavailable: authentication_missing");
  expect((caught as Error).message).not.toContain(canary);
});

test("balanced allocation is stable and distributes by entitlement-adjusted weights", () => {
  const balanced = policy({ mode: "balanced", targetWeights: { anthropic: 1, openai: 1 } });
  const first = selectProviderFromAvailability("auto", available, balanced, "standard", "lane-42");
  const second = selectProviderFromAvailability("auto", available, balanced, "standard", "lane-42");
  expect(second.provider).toBe(first.provider);

  const normalCounts = { anthropic: 0, openai: 0 };
  const lowAnthropicCounts = { anthropic: 0, openai: 0 };
  for (let i = 0; i < 500; i++) {
    normalCounts[selectProviderFromAvailability("auto", available, balanced, "standard", `lane-${i}`).provider]++;
    lowAnthropicCounts[selectProviderFromAvailability("auto", available,
      policy({ mode: "balanced", targetPressures: { anthropic: "low", openai: "normal" } }),
      "standard", `lane-${i}`).provider]++;
  }
  expect(normalCounts.anthropic).toBeGreaterThan(0);
  expect(normalCounts.openai).toBeGreaterThan(0);
  expect(lowAnthropicCounts.anthropic).toBeLessThan(normalCounts.anthropic);
});

test("balanced rendezvous is uniform for short target ids and honors weight ratios", () => {
  const targets = ["a", "b", "c"].map((id) => ({ id, provider: "anthropic" as const, authMode: "ambient" as const }));
  const availability = targets.map(({ id, provider }) => ({
    targetId: id, provider, available: true, reason: "ready" as const,
  }));
  const makePolicy = (targetWeights: Record<string, number>): ResourcePolicy => ({
    mode: "balanced", targets, targetOrder: ["a", "b", "c"], providerOrder: ["anthropic"],
    targetPressures: { anthropic: "plenty" },
    targetPressures: { a: "plenty", b: "plenty", c: "plenty" }, targetWeights,
  });
  const sample = (targetWeights: Record<string, number>) => {
    const counts = { a: 0, b: 0, c: 0 };
    const allocationPolicy = makePolicy(targetWeights);
    for (let index = 0; index < 30_000; index++)
      counts[selectProviderFromAvailability(
        "auto", availability, allocationPolicy, "standard", `k-${index}`, "medium",
      ).target as keyof typeof counts]++;
    return counts;
  };
  const equal = sample({ a: 1, b: 1, c: 1 });
  for (const count of Object.values(equal)) expect(count / 30_000).toBeWithin(0.313, 0.353);
  const weighted = sample({ a: 1, b: 2, c: 3 });
  expect(weighted.a / 30_000).toBeWithin(0.145, 0.19);
  expect(weighted.b / 30_000).toBeWithin(0.31, 0.355);
  expect(weighted.c / 30_000).toBeWithin(0.475, 0.525);
}, 15_000);

test("provider catalog cache reuses a stable snapshot and invalidates a same-path change", () => {
  let version = 1n;
  let reads = 0;
  const identity = () => ({
    dev: 1n, ino: version, size: version, mtimeNs: version, ctimeNs: version,
  });
  const cache = new ProviderCatalogFileCache<{ value: string }>({
    identity,
    read: () => {
      reads++;
      return JSON.stringify({ value: `v${version}` });
    },
  });
  const load = () => cache.load("/same/provider.json", JSON.parse);

  expect(load()).toEqual({ value: "v1" });
  expect(load()).toEqual({ value: "v1" });
  expect(reads).toBe(1);

  version = 2n;
  expect(load()).toEqual({ value: "v2" });
  expect(reads).toBe(2);
});

test("provider catalog cache fails closed when the file changes throughout both reads", () => {
  const versions = [1n, 2n, 2n, 3n];
  let identityCalls = 0;
  let reads = 0;
  const cache = new ProviderCatalogFileCache<{ value: string }>({
    identity: () => {
      const version = versions[identityCalls++]!;
      return { dev: 1n, ino: version, size: version, mtimeNs: version, ctimeNs: version };
    },
    read: () => {
      reads++;
      return JSON.stringify({ value: `attempt-${reads}` });
    },
  });

  expect(() => cache.load("/changing/provider.json", JSON.parse))
    .toThrow("Orchestration provider catalog changed while reading /changing/provider.json");
  expect(identityCalls).toBe(4);
  expect(reads).toBe(2);
});

test("balanced allocation uses each account's observed numeric headroom", () => {
  const resetsAt = "2099-01-01T00:00:00Z";
  const balanced = accountPolicy({
    mode: "balanced",
    automatedPressureObservationSets: {
      "claude-personal": [{
        targetId: "claude-personal", provider: "anthropic", observedAt: new Date().toISOString(),
        windows: [{ limitId: "claude:seven_day", usedPercent: 80, resetsAt }],
      }],
      "claude-work": [{
        targetId: "claude-work", provider: "anthropic", observedAt: new Date().toISOString(),
        windows: [{ limitId: "claude:seven_day", usedPercent: 50, resetsAt }],
      }],
      "codex-personal": [{
        targetId: "codex-personal", provider: "openai", observedAt: new Date().toISOString(),
        windows: [{ limitId: "codex:primary", usedPercent: 20, resetsAt }],
      }],
    },
  });
  const estimates = Object.fromEntries(balancedAllocationEstimates(
    accountAvailability, balanced, "standard", "medium",
  ).map((estimate) => [estimate.target, estimate]));
  expect(estimates["claude-personal"].effectiveWeight).toBeCloseTo(0.2);
  expect(estimates["claude-work"].effectiveWeight).toBeCloseTo(0.5);
  expect(estimates["codex-personal"].effectiveWeight).toBeCloseTo(0.8);
  expect(estimates["claude-personal"].approximateShare)
    .toBeLessThan(estimates["claude-work"].approximateShare);
  expect(estimates["claude-work"].approximateShare)
    .toBeLessThan(estimates["codex-personal"].approximateShare);
  expect(Object.values(estimates).reduce(
    (sum, estimate) => sum + estimate.approximateShare, 0,
  )).toBeCloseTo(1);
});

test("same-window Anthropic warning applies a routing-only floor without fabricating measured usage", () => {
  const now = new Date();
  const observedAt = now.toISOString();
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const calibrated = accountPolicy({
    mode: "balanced",
    automatedPressureObservationSets: {
      "claude-personal": [
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:usage-control-experimental", observedAt,
          windows: [{
            limitId: "claude:seven_day", usedPercent: 55,
            resetsAt: "2099-01-01T01:59:59.671Z",
          }],
        },
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:rate-limit-event", observedAt,
          categoricalSignals: [{
            kind: "warning", limitId: "seven_day", resetsAt: "2099-01-01T02:00:00.000Z",
          }],
        },
      ],
    },
  });
  const estimate = balancedAllocationEstimates(target, calibrated, "standard", "medium")[0]!;
  expect(estimate).toMatchObject({ pressure: "low", effectiveWeight: 0.2 });
  expect(estimate.allocationEvidence).toMatchObject({
    kind: "conservative-floor",
    source: "claude-agent-sdk:rate-limit-event",
    routingFloorPercent: 80,
    measuredUsedPercent: 55,
    measurementSource: "claude-agent-sdk:usage-control-experimental",
  });
  expect(estimate.allocationEvidence.usedPercent).toBeUndefined();
});

test("warning floors expire and do not absorb unlike provider windows", () => {
  const now = new Date();
  const staleWarningAt = new Date(now.getTime() - RATE_LIMIT_WARNING_TTL_MS - 1_000).toISOString();
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const stale = accountPolicy({
    mode: "balanced",
    automatedPressureObservationSets: {
      "claude-personal": [
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:usage-control-experimental", observedAt: now.toISOString(),
          windows: [{ limitId: "claude:seven_day", usedPercent: 55, resetsAt: "2099-01-01T02:00:00Z" }],
        },
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:rate-limit-event", observedAt: staleWarningAt,
          categoricalSignals: [{ kind: "warning", limitId: "seven_day", resetsAt: "2099-01-01T02:00:00Z" }],
        },
      ],
    },
  });
  expect(balancedAllocationEstimates(target, stale, "standard", "medium")[0]).toMatchObject({
    pressure: "normal",
    effectiveWeight: 0.45,
    allocationEvidence: {
      kind: "numeric-headroom", usedPercent: 55,
      source: "claude-agent-sdk:usage-control-experimental",
    },
  });

  const unlike = accountPolicy({
    mode: "balanced",
    automatedPressureObservationSets: {
      "claude-personal": [
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:usage-control-experimental", observedAt: now.toISOString(),
          windows: [{ limitId: "claude:five_hour", usedPercent: 55, resetsAt: "2099-01-01T02:00:00Z" }],
        },
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:rate-limit-event", observedAt: now.toISOString(),
          categoricalSignals: [{ kind: "warning", limitId: "seven_day", resetsAt: "2099-01-01T02:00:00Z" }],
        },
      ],
    },
  });
  const unlikeEvidence = balancedAllocationEstimates(target, unlike, "standard", "medium")[0]!.allocationEvidence;
  expect(unlikeEvidence).toMatchObject({ kind: "conservative-floor", routingFloorPercent: 80 });
  expect(unlikeEvidence.measuredUsedPercent).toBeUndefined();
});

test("model-scoped exhaustion constrains only the matching Anthropic route", () => {
  // Fable is reached by an explicit model pin plus target-scoped supportedModels
  // evidence; its pool window constrains only that route, never opus/sonnet.
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const scoped = accountPolicy({
    targetPressures: { "claude-personal": "exhausted", "claude-work": "unknown", "codex-personal": "unknown" },
    automatedPressureObservationSets: {
      "claude-personal": [{
        targetId: "claude-personal", provider: "anthropic", observedAt: new Date().toISOString(),
        windows: [
          { limitId: "claude:seven_day", usedPercent: 20, resetsAt: "2099-01-01T00:00:00Z" },
          { limitId: "claude:model:fable", usedPercent: 100, resetsAt: "2099-01-01T00:00:00Z" },
        ],
      }],
    },
  });
  const senior = selectProviderFromAvailability(
    { target: "claude-personal" }, target, scoped, "senior", "senior", "high",
  );
  expect(senior.entitlementPressure).toBe("plenty");
  expect(() => selectProviderFromAvailability(
    { target: "claude-personal" }, target, scoped, "frontier", "frontier", "xhigh", "fable",
    undefined, fableModelEvidence(accountPolicy().targets![0]),
  )).toThrow("routing target claude-personal entitlement exhausted");
});

test("route pressure combines every source after explicit-model family filtering", () => {
  const observedAt = new Date().toISOString();
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const combined = accountPolicy({
    mode: "balanced",
    automatedPressureObservationSets: {
      "claude-personal": [
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-agent-sdk:usage-control-experimental", observedAt,
          windows: [
            { limitId: "claude:seven_day", usedPercent: 20, resetsAt: "2099-01-01T00:00:00Z" },
            { limitId: "claude:model:fable", usedPercent: 100, resetsAt: "2099-01-01T00:00:00Z" },
          ],
        },
        {
          targetId: "claude-personal", provider: "anthropic",
          source: "claude-code:statusline", observedAt,
          windows: [{ limitId: "seven_day", usedPercent: 90, resetsAt: "2099-01-01T00:00:00Z" }],
        },
      ],
    },
  });
  const sonnet = balancedAllocationEstimates(
    target, combined, "standard", "medium", "claude-sonnet-5",
  )[0];
  expect(sonnet).toMatchObject({ pressure: "low", effectiveWeight: 0.1 });
  expect(sonnet.allocationEvidence).toMatchObject({
    source: "claude-code:statusline", limitId: "seven_day", usedPercent: 90,
  });
  const fable = balancedAllocationEstimates(
    target, combined, "frontier", "xhigh", "claude-fable-5",
  )[0];
  expect(fable).toMatchObject({ pressure: "exhausted", eligible: false });
});

test("explicit models constrain provider compatibility before observed-window pressure", () => {
  const observedAt = new Date().toISOString();
  const withWindows = accountPolicy({
    automatedPressureObservationSets: {
      "claude-personal": [{ targetId: "claude-personal", provider: "anthropic", observedAt,
        windows: [{ limitId: "claude:seven_day_opus", usedPercent: 100, resetsAt: "2099-01-01T00:00:00Z" }] }],
    },
  });
  expect(selectProviderFromAvailability(
    "auto", accountAvailability, withWindows, "senior", "openai-model", "high", "gpt-5.6-sol",
    undefined, codexModelEvidence(accountPolicy().targets![2]),
  ).provider).toBe("openai");
  expect(selectProviderFromAvailability(
    "openai", accountAvailability, withWindows, "senior", "openai-pin", "high", "gpt-5.6-sol",
    undefined, codexModelEvidence(accountPolicy().targets![2]),
  ).provider).toBe("openai");
});

test("fresh telemetry failure neither rewards stale headroom nor revives model-scoped exhaustion", () => {
  const observedAt = new Date().toISOString();
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const failed = accountPolicy({
    mode: "balanced",
    targetPressures: { "claude-personal": "exhausted", "claude-work": "unknown", "codex-personal": "unknown" },
    automatedPressureObservationSets: {
      "claude-personal": [{
        targetId: "claude-personal", provider: "anthropic", observedAt,
        windows: [{ limitId: "claude:model:fable", usedPercent: 100, resetsAt: "2099-01-01T00:00:00Z" }],
        collectionFailure: { observedAt, reason: "anthropic_usage_probe_timed_out" },
      }],
    },
  });

  const standard = balancedAllocationEstimates(target, failed, "standard", "medium")[0];
  expect(standard).toMatchObject({ eligible: true, pressure: "unknown", effectiveWeight: 0.5 });
  const frontier = balancedAllocationEstimates(target, failed, "frontier", "xhigh", "fable")[0];
  expect(frontier).toMatchObject({ eligible: false, pressure: "exhausted", effectiveWeight: 0 });
  expect(() => selectProviderFromAvailability(
    { target: "claude-personal" }, target, failed, "frontier", "failed-frontier", "xhigh", "fable",
    undefined, fableModelEvidence(accountPolicy().targets![0]),
  )).toThrow("routing target claude-personal entitlement exhausted");
});

test("reserved allocation preserves a frontier provider for non-frontier work", () => {
  const reserved = policy({ mode: "reserved", reservedFrontierProvider: "anthropic" });
  const normal = selectProviderFromAvailability("auto", available, reserved, "standard", "normal");
  const frontier = selectProviderFromAvailability("auto", available, reserved, "frontier", "frontier");
  expect(normal.provider).toBe("openai");
  expect(normal.selectionReason).toContain("preserving frontier reserve=anthropic");
  expect(frontier.provider).toBe("anthropic");
  expect(frontier.selectionReason).toContain("frontier reserve=anthropic");
});

test("reserved allocation exhausts non-reserve fallbacks before the frontier account", () => {
  const availability: ProviderAvailability[] = [
    { targetId: "reserve", provider: "anthropic", available: true, reason: "ready" },
    { targetId: "alt-a", provider: "openai", available: true, reason: "ready" },
    { targetId: "alt-b", provider: "openai", available: true, reason: "ready" },
  ];
  const reserved: ResourcePolicy = {
    mode: "reserved",
    targets: [
      { id: "reserve", provider: "anthropic", authMode: "ambient" },
      { id: "alt-a", provider: "openai", authMode: "ambient" },
      { id: "alt-b", provider: "openai", authMode: "ambient" },
    ],
    targetOrder: ["reserve", "alt-a", "alt-b"], providerOrder: ["anthropic", "openai"],
    targetPressures: { anthropic: "plenty", openai: "plenty" },
    targetPressures: { reserve: "plenty", "alt-a": "plenty", "alt-b": "plenty" },
    reservedFrontierTarget: "reserve", reservedFrontierProvider: "anthropic",
  };
  const decision = selectProviderFromAvailability(
    "auto", availability, reserved, "standard", "reserve-retry", "medium",
  );
  expect(decision.target).toBe("alt-a");
  expect(decision.fallbackTargets).toEqual(["alt-b", "reserve"]);
});

test("reserved allocation degrades gracefully when reserve or alternatives are unavailable", () => {
  const openAiUnavailable: ProviderAvailability[] = [
    available[0], { provider: "openai", available: false, reason: "disabled" },
  ];
  const reserved = policy({ mode: "reserved", reservedFrontierProvider: "anthropic" });
  expect(selectProviderFromAvailability("auto", openAiUnavailable, reserved, "standard", "x").provider).toBe("anthropic");

  const anthropicExhausted = policy({
    mode: "reserved", reservedFrontierProvider: "anthropic",
    targetPressures: { anthropic: "exhausted", openai: "normal" },
  });
  expect(selectProviderFromAvailability("auto", available, anthropicExhausted, "frontier", "x").provider).toBe("openai");
});

test("semantic tiers resolve independently per provider", () => {
  expect(resolveTier("anthropic", "senior")).toEqual({ tier: "senior", model: "claude-opus-5", effort: "high" });
  expect(resolveTier("openai", "frontier")).toEqual({ tier: "frontier", model: "gpt-5.6-sol", effort: "xhigh" });
});

test("OpenAI resolves the unpinned semantic ramp to its minimum-sufficient model and effort", () => {
  for (const { tier, model, defaultEffort } of [
    { tier: "economy" as const, model: "gpt-5.6-luna", defaultEffort: "low" as const },
    { tier: "standard" as const, model: "gpt-5.6-terra", defaultEffort: "medium" as const },
    { tier: "senior" as const, model: "gpt-5.6-sol", defaultEffort: "high" as const },
  ])
    expect(resolveTier("openai", tier)).toEqual({ tier, model, effort: defaultEffort });

  for (const { tier, model, defaultEffort } of [
    { tier: "economy" as const, model: "gpt-5.6-luna", defaultEffort: "xhigh" as const },
    { tier: "standard" as const, model: "gpt-5.6-terra", defaultEffort: "high" as const },
  ]) {
    expect(resolveTier("openai", tier, model)).toEqual({ tier, model, effort: defaultEffort });
    for (const effort of ["low", "medium", "high", "xhigh"] as const)
      expect(resolveTier("openai", tier, model, effort)).toEqual({ tier, model, effort });
    expect(() => resolveTier("openai", tier, model, "max"))
      .toThrow(`model ${model} does not support reasoning max at semantic tier ${tier}`);
  }
});

test("provider selection honors each catalog's explicit tier reasoning routes", () => {
  expect(resolveTier("anthropic", "senior", undefined, "medium"))
    .toEqual({ tier: "senior", model: "claude-opus-5", effort: "medium" });
  const decision = selectProviderFromAvailability(
    "auto",
    available,
    policy({ providerOrder: ["anthropic", "openai"] }),
    "senior",
    "asymmetric-route",
    "medium",
  );
  expect(decision.provider).toBe("anthropic");
  expect(decision.fallbackProviders).toEqual([]);
  expect(decision.selectionReason).toContain("route=senior/medium");
  expect(selectProviderFromAvailability(
    "anthropic", available, policy(), "senior", "exact-compatible", "medium",
  ).provider).toBe("anthropic");
});

test("provider selection filters incompatible tier reasoning before allocation", () => {
  expect(() => resolveTier("anthropic", "frontier", undefined, "max"))
    .toThrow("provider anthropic cannot resolve semantic tier frontier with reasoning max");
  const decision = selectProviderFromAvailability(
    "auto", available, policy({ providerOrder: ["anthropic", "openai"] }),
    "frontier", "asymmetric-incompatible-route", "max",
  );
  expect(decision.provider).toBe("openai");
  expect(decision.fallbackProviders).toEqual([]);
  expect(decision.selectionReason).toContain("route=frontier/max");
  try {
    selectProviderFromAvailability(
      "anthropic", available, policy(), "frontier", "exact-incompatible", "max",
    );
    throw new Error("expected route incompatibility");
  } catch (error) {
    expect(error).toMatchObject({ kind: "route_unresolvable", preSideEffect: true });
  }
});

test("provider selection filters unenforceable capability shapes before side effects", () => {
  const capabilities = ["filesystem.read"] as const;
  const decision = selectProviderFromAvailability(
    "auto", available, policy({ providerOrder: ["openai", "anthropic"] }),
    "senior", "capability-route", "high", undefined, capabilities,
  );
  expect(decision.provider).toBe("anthropic");
  expect(decision.fallbackProviders).toEqual([]);
  expect(() => selectProviderFromAvailability(
    "openai", available, policy(), "senior", "capability-pin", "high", undefined, capabilities,
  )).toThrow("cannot enforce the requested Orchestration capabilities");

  const webCapabilities = [
    "filesystem.read", "filesystem.search", "shell.readonly", "web",
  ] as const;
  const web = selectProviderFromAvailability(
    "auto", available, policy({ providerOrder: ["openai", "anthropic"] }),
    "senior", "web-route", "high", undefined, webCapabilities,
  );
  expect(web.provider).toBe("openai");
  expect(selectProviderFromAvailability(
    "openai", available, policy(), "senior", "web-pin", "high", undefined,
    webCapabilities,
  ).provider).toBe("openai");

  const orchestratorCapabilities = [
    "filesystem.read", "filesystem.search", "shell.readonly", "web", "coordination",
  ] as const;
  const orchestrator = selectProviderFromAvailability(
    "auto", available, policy({ providerOrder: ["openai", "anthropic"] }),
    "senior", "coordination-route", "high", undefined, orchestratorCapabilities,
  );
  expect(orchestrator.provider).toBe("openai");
  expect(selectProviderFromAvailability(
    "openai", available, policy(), "senior", "coordination-pin", "high", undefined,
    orchestratorCapabilities,
  ).provider).toBe("openai");
  const pinnedTarget = selectProviderFromAvailability(
    { provider: "auto", target: "codex-personal" },
    accountAvailability,
    accountPolicy(),
    "senior",
    "coordination-target-pin",
    "high",
    undefined,
    orchestratorCapabilities,
  );
  expect(pinnedTarget.provider).toBe("openai");
  expect(pinnedTarget.target).toBe("codex-personal");
});

test("provider effective-authority closure defense remains exact", () => {
  for (const provider of ["anthropic", "openai"] as const) {
    expect(providerCapabilityRejectionCode(
      provider, ["filesystem.search", "filesystem.write", "shell"],
    )).toBe(`${provider}_adapter_cannot_enforce_orchestration_capabilities`);
    expect(providerCapabilityRejectionCode(
      provider, ["filesystem.read", "filesystem.search", "shell"],
    )).toBe(`${provider}_adapter_cannot_enforce_orchestration_capabilities`);
    expect(providerCapabilityRejectionCode(
      provider, ["filesystem.search", "shell.readonly"],
    )).toBe(`${provider}_adapter_cannot_enforce_orchestration_capabilities`);
    expect(providerCapabilityRejectionCode(
      provider, ["filesystem.read", "shell.readonly"],
    )).toBe(`${provider}_adapter_cannot_enforce_orchestration_capabilities`);
    expect(providerCapabilityRejectionCode(
      provider, ["filesystem.read", "filesystem.search", "filesystem.write", "shell"],
    )).toBeUndefined();
    expect(providerCapabilityRejectionCode(
      provider, ["filesystem.read", "filesystem.search", "shell.readonly"],
    )).toBeUndefined();
  }
});

test("Anthropic frontier follows Orchestration's static route without a hidden time swap", () => {
  expect(resolveTier("anthropic", "frontier")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "xhigh" });
  expect(resolveTier("anthropic", "frontier", undefined, "high")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "high" });
  expect(resolveTier("anthropic", "frontier", undefined, "xhigh")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "xhigh" });
  expect(() => resolveTier("anthropic", "frontier", "sonnet", "xhigh"))
    .toThrow("model claude-sonnet-5 does not support reasoning xhigh");
  expect(() => resolveTier("openai", "frontier", "luna", "xhigh"))
    .toThrow("model gpt-5.6-luna does not support reasoning xhigh");
  expect(resolveTier("anthropic", "frontier", "opus", "xhigh")).toEqual({
    tier: "frontier", model: "claude-opus-5", effort: "xhigh",
  });
  expect(() => resolveTier("anthropic", "frontier", undefined, "max"))
    .toThrow("provider anthropic cannot resolve semantic tier frontier with reasoning max");
  expect(resolveTier("openai", "frontier")).toEqual({ tier: "frontier", model: "gpt-5.6-sol", effort: "xhigh" });
  delete process.env.NORTH_FABLE_NOW;
  expect(resolveTier("anthropic", "frontier")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "xhigh" });
  expect(resolveTier("anthropic", "frontier", "fable", "xhigh"))
    .toEqual({ tier: "frontier", model: "claude-fable-5", effort: "xhigh" });
  expect(resolveTier("anthropic", "frontier", "fable", "high"))
    .toEqual({ tier: "frontier", model: "claude-fable-5", effort: "high" });
  expect(() => resolveTier("anthropic", "frontier", "fable", "max"))
    .toThrow("model claude-fable-5 does not support reasoning max at semantic tier frontier");
});

let anthropicContextSequence = 0;

function anthropicTestContext(): WireQueryContext {
  const sequence = anthropicContextSequence++;
  const writer = new WireEventWriter({
    runId: wireRunId(`run:anthropic-admission:${sequence}`),
    eventId: (eventSequence) => wireEventId(
      `event:anthropic-admission:${sequence}:${eventSequence}`,
    ),
  });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "test" });
  return {
    writer,
    route: {
      model: { provider: "anthropic", tier: "senior", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    },
  };
}

async function wireEvents(query: AsyncIterable<WireEvent>): Promise<WireEvent[]> {
  const events: WireEvent[] = [];
  for await (const event of query) events.push(event);
  return events;
}

test("Anthropic managed admission rejects every omitted authority boundary before SDK side effects", async () => {
  let sequence = 0;
  const makeBase = () => harnessOptions({
    self: `anthropic-authority-probe-${sequence++}`,
    provider: "anthropic",
    model: "claude-opus-4-8",
    modelAvailability: { exactModelPinned: false, targetId: "anthropic" },
    routingMetadata: applyOrchestrationStaffing({ role: "designer" }),
    presenceRegistrar: false,
  }) as any;
  const changed = (mutate: (options: any) => void) => {
    const sealed = makeBase();
    const options = {
      ...sealed,
      env: { ...sealed.env },
      mcpServers: { ...sealed.mcpServers },
      tools: [...sealed.tools],
      allowedTools: [...sealed.allowedTools],
      disallowedTools: [...sealed.disallowedTools],
      settingSources: [...sealed.settingSources],
      northCapabilities: [...sealed.northCapabilities],
    };
    mutate(options);
    return options;
  };
  const withoutServer = (name: string) => {
    return changed((options) => { delete options.mcpServers[name]; });
  };
  const writableWithCanonicalGuards = harnessOptions({
    self: "anthropic-unrestricted-shell-without-guards",
    provider: "anthropic",
    model: "claude-opus-4-8",
    modelAvailability: { exactModelPinned: false, targetId: "anthropic" },
    routingMetadata: applyOrchestrationStaffing({ role: "integrator" }),
    presenceRegistrar: false,
  }) as any;
  const writableWithoutGuards = {
    ...writableWithCanonicalGuards,
    hooks: { ...writableWithCanonicalGuards.hooks, PreToolUse: [] },
  };
  const cases: Array<[any, string]> = [
    [
      withoutServer("north"),
      "anthropic_managed_north_mcp_contract_missing",
    ],
    [
      changed((options) => { options.settingSources = ["user"]; }),
      "anthropic_setting_sources_must_be_isolated",
    ],
    [
      changed((options) => { options.strictMcpConfig = false; }),
      "anthropic_strict_mcp_config_required",
    ],
    [
      changed((options) => {
        options.disallowedTools = options.disallowedTools.filter(
          (toolName: string) => toolName !== "Agent",
        );
      }),
      "anthropic_adapter_did_not_enforce_absent_native_agent_capability",
    ],
    [
      changed((options) => { options.northCapabilities = ["filesystem.search"]; }),
      "anthropic_adapter_did_not_enforce_absent_filesystem_read_capability",
    ],
    [
      changed((options) => { options.northCapabilities = ["filesystem.read"]; }),
      "anthropic_adapter_did_not_enforce_absent_filesystem_search_capability",
    ],
    [
      changed((options) => {
        options.allowedTools = options.allowedTools.filter(
          (toolName: string) => toolName !== READONLY_SHELL_TOOL,
        );
      }),
      "anthropic_adapter_did_not_apply_readonly_shell_capability",
    ],
    [
      changed((options) => {
        options.disallowedTools = options.disallowedTools.filter(
          (toolName: string) => toolName !== "Bash",
        );
      }),
      "anthropic_adapter_did_not_enforce_absent_shell_capability",
    ],
    [
      withoutServer(READONLY_SHELL_SERVER),
      "anthropic_readonly_shell_contract_missing",
    ],
    [
      changed((options) => { options.tools = [...options.tools, "Bash"]; }),
      "anthropic_builtin_tool_surface_contract_missing",
    ],
    [
      changed((options) => { options.allowedTools = [...options.allowedTools, "Bash"]; }),
      "anthropic_auto_approval_contract_missing",
    ],
    [
      changed((options) => {
        options.disallowedTools = options.disallowedTools.filter(
          (toolName: string) => toolName !== "mcp__north__linear_sync",
        );
      }),
      "anthropic_denied_tool_contract_missing",
    ],
    [
      changed((options) => { options.mcpServers.ambient = options.mcpServers.north; }),
      "anthropic_mcp_server_surface_contract_missing",
    ],
    [
      changed((options) => {
        options.mcpServers.north = { ...options.mcpServers.north };
      }),
      "anthropic_authoring_guard_contract_missing",
    ],
    [
      changed((options) => { options.env.AGENT_TOPOLOGY = undefined; }),
      "anthropic_managed_identity_topology_contract_missing",
    ],
    [
      writableWithoutGuards,
      "anthropic_authoring_guard_contract_missing",
    ],
  ];
  for (const [options] of cases) {
    let caught: unknown;
    try { await anthropicProvider.admit!({ options }); }
    catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(ProviderRetrySafeError);
    expect((caught as Error).message).toBe("anthropic_harness_authority_seal_missing");
  }
  await expect(wireEvents(anthropicProvider.query({
    input: "must not reach Claude",
    options: cases[0][0],
    context: anthropicTestContext(),
  }))).rejects.toThrow("anthropic_harness_authority_seal_missing");
  const base = makeBase();
  expect(compileProviderAuthoritySurface("anthropic", base).provider).toBe("anthropic");

  markExecutionAdmission("anthropic", base);
  const admitted = anthropicProvider.query({
    input: "must still not reach Claude",
    options: base,
    context: anthropicTestContext(),
  });
  base.disallowedTools = base.disallowedTools.filter(
    (toolName: string) => toolName !== "Agent",
  );
  await expect(wireEvents(admitted))
    .rejects.toThrow("anthropic_harness_authority_seal_missing");
});
