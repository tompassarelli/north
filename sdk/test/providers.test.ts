import { afterEach, beforeEach, expect, test } from "bun:test";
import {
  ProviderRetrySafeError, ProviderSelectionError, routedQuery, selectProvider, selectProviderFromAvailability,
} from "../src/providers";
import { balancedAllocationEstimates } from "../src/provider-routing";
import type { AgentProvider, ProviderAvailability, ProviderId, ResourcePolicy } from "../src/providers/types";
import { resolveTier } from "../src/providers/catalog";
import { anthropicProvider, normalizeAnthropicQueryDiagnostics } from "../src/providers/anthropic";
import { agentRouteFacts } from "../src/identity";
import { join } from "node:path";
import { tmpdir } from "node:os";

const MANAGED_ENV = [
  "NORTH_DISABLE_ANTHROPIC", "NORTH_DISABLE_OPENAI", "NORTH_PROVIDER_ORDER",
  "NORTH_ROUTING_POLICY", "NORTH_PROVIDER_OBSERVATIONS", "NORTH_ALLOCATION_MODE",
  "NORTH_PROVIDER_WEIGHTS", "NORTH_RESERVED_FRONTIER_PROVIDER",
  "NORTH_FABLE_NOW",
  "NORTH_ANTHROPIC_ENTITLEMENT_PRESSURE", "NORTH_OPENAI_ENTITLEMENT_PRESSURE",
] as const;
const saved = Object.fromEntries(MANAGED_ENV.map((key) => [key, process.env[key]])) as Record<typeof MANAGED_ENV[number], string | undefined>;
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
  pressures: { anthropic: "normal", openai: "normal" },
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
  const decision = selectProvider("auto");
  expect(decision.provider).toBe("openai");
});

test("preferential allocation walks configured order and explains pressure", () => {
  const decision = selectProviderFromAvailability("auto", available,
    policy({ providerOrder: ["openai", "anthropic"], pressures: { openai: "plenty", anthropic: "normal" } }));
  expect(decision.provider).toBe("openai");
  expect(decision.reason).toContain("mode=preferential");
  expect(decision.reason).toContain("pressure=plenty");
});

test("automatic allocation avoids an exhausted entitlement", () => {
  const decision = selectProviderFromAvailability("auto", available,
    policy({ pressures: { anthropic: "exhausted", openai: "low" } }));
  expect(decision.provider).toBe("openai");
  expect(decision.reason).toContain("pressure=low");

  try {
    selectProviderFromAvailability("auto", available,
      policy({ pressures: { anthropic: "exhausted", openai: "exhausted" } }));
    throw new Error("expected provider selection to fail");
  } catch (error) {
    expect(error).toBeInstanceOf(ProviderSelectionError);
    expect(error).toMatchObject({ kind: "no_provider_available", preSideEffect: true });
  }
});

test("explicit provider wins but exhausted explicit entitlement errors", () => {
  const decision = selectProviderFromAvailability("openai", available,
    policy({ pressures: { anthropic: "plenty", openai: "low" } }));
  expect(decision.provider).toBe("openai");
  expect(decision.reason).toContain("explicit provider");
  expect(() => selectProviderFromAvailability("openai", available,
    policy({ pressures: { openai: "exhausted" } }))).toThrow("provider openai entitlement exhausted");
});

test("target pressure is independent and auto considers every configured account", () => {
  const decision = selectProviderFromAvailability("auto", accountAvailability, accountPolicy({
    targetPressures: { "claude-personal": "exhausted", "claude-work": "low", "codex-personal": "normal" },
    pressures: { anthropic: "exhausted", openai: "normal" },
  }));
  expect(decision.target).toBe("claude-work");
  expect(decision.provider).toBe("anthropic");
  expect(decision.entitlementPressure).toBe("low");
  expect(decision.targetEntitlementPressures["claude-personal"]).toBe("exhausted");
  expect(decision.entitlementPressures.anthropic).toBe("exhausted");
  expect(decision.fallbackTargets).toEqual(["codex-personal"]);
  expect(decision.fallbackProviders).toEqual(["openai"]);
});

test("exact target pin records the request and refuses sibling or provider fallback", () => {
  const healthy = selectProviderFromAvailability({ target: "claude-personal" }, accountAvailability, accountPolicy());
  expect(healthy).toMatchObject({
    requested: "auto", requestedProvider: "auto", requestedTarget: "claude-personal",
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
  const balanced = policy({ mode: "balanced", weights: { anthropic: 1, openai: 1 } });
  const first = selectProviderFromAvailability("auto", available, balanced, "standard", "lane-42");
  const second = selectProviderFromAvailability("auto", available, balanced, "standard", "lane-42");
  expect(second.provider).toBe(first.provider);

  const normalCounts = { anthropic: 0, openai: 0 };
  const lowAnthropicCounts = { anthropic: 0, openai: 0 };
  for (let i = 0; i < 500; i++) {
    normalCounts[selectProviderFromAvailability("auto", available, balanced, "standard", `lane-${i}`).provider]++;
    lowAnthropicCounts[selectProviderFromAvailability("auto", available,
      policy({ mode: "balanced", pressures: { anthropic: "low", openai: "normal" } }),
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
    pressures: { anthropic: "plenty" },
    targetPressures: { a: "plenty", b: "plenty", c: "plenty" }, targetWeights,
  });
  const sample = (targetWeights: Record<string, number>) => {
    const counts = { a: 0, b: 0, c: 0 };
    for (let index = 0; index < 30_000; index++)
      counts[selectProviderFromAvailability(
        "auto", availability, makePolicy(targetWeights), "standard", `k-${index}`, "medium",
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

test("balanced allocation uses each account's observed numeric headroom", () => {
  const resetsAt = "2099-01-01T00:00:00Z";
  const balanced = accountPolicy({
    mode: "balanced",
    automatedPressureObservations: {
      "claude-personal": {
        targetId: "claude-personal", provider: "anthropic", observedAt: new Date().toISOString(),
        windows: [{ limitId: "claude:seven_day", usedPercent: 80, resetsAt }],
      },
      "claude-work": {
        targetId: "claude-work", provider: "anthropic", observedAt: new Date().toISOString(),
        windows: [{ limitId: "claude:seven_day", usedPercent: 50, resetsAt }],
      },
      "codex-personal": {
        targetId: "codex-personal", provider: "openai", observedAt: new Date().toISOString(),
        windows: [{ limitId: "codex:primary", usedPercent: 20, resetsAt }],
      },
    },
  });
  const counts = { "claude-personal": 0, "claude-work": 0, "codex-personal": 0 };
  for (let index = 0; index < 2_000; index++) {
    const decision = selectProviderFromAvailability(
      "auto", accountAvailability, balanced, "standard", `run-${index}`, "medium",
    );
    counts[decision.target as keyof typeof counts]++;
  }
  expect(counts["claude-personal"]).toBeLessThan(counts["claude-work"]);
  expect(counts["claude-work"]).toBeLessThan(counts["codex-personal"]);
  expect(Object.values(counts).reduce((sum, count) => sum + count, 0)).toBe(2_000);
});

test("model-scoped exhaustion constrains only the matching Anthropic route", () => {
  process.env.NORTH_FABLE_NOW = "2026-07-19T00:00:00Z";
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const scoped = accountPolicy({
    targetPressures: { "claude-personal": "exhausted", "claude-work": "unknown", "codex-personal": "unknown" },
    automatedPressureObservations: {
      "claude-personal": {
        targetId: "claude-personal", provider: "anthropic", observedAt: new Date().toISOString(),
        windows: [
          { limitId: "claude:seven_day", usedPercent: 20, resetsAt: "2099-01-01T00:00:00Z" },
          { limitId: "claude:model:fable", usedPercent: 100, resetsAt: "2099-01-01T00:00:00Z" },
        ],
      },
    },
  });
  const senior = selectProviderFromAvailability(
    { target: "claude-personal" }, target, scoped, "senior", "senior", "high",
  );
  expect(senior.entitlementPressure).toBe("plenty");
  expect(() => selectProviderFromAvailability(
    { target: "claude-personal" }, target, scoped, "frontier", "frontier", "xhigh",
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
  ).provider).toBe("openai");
  expect(selectProviderFromAvailability(
    "openai", accountAvailability, withWindows, "senior", "openai-pin", "high", "gpt-5.6-sol",
  ).provider).toBe("openai");
});

test("fresh telemetry failure neither rewards stale headroom nor revives model-scoped exhaustion", () => {
  process.env.NORTH_FABLE_NOW = "2026-07-19T00:00:00Z";
  const observedAt = new Date().toISOString();
  const target = accountAvailability.filter(({ targetId }) => targetId === "claude-personal");
  const failed = accountPolicy({
    mode: "balanced",
    targetPressures: { "claude-personal": "exhausted", "claude-work": "unknown", "codex-personal": "unknown" },
    automatedPressureObservations: {
      "claude-personal": {
        targetId: "claude-personal", provider: "anthropic", observedAt,
        windows: [{ limitId: "claude:model:fable", usedPercent: 100, resetsAt: "2099-01-01T00:00:00Z" }],
        collectionFailure: { observedAt, reason: "anthropic_usage_probe_timed_out" },
      },
    },
  });

  const standard = balancedAllocationEstimates(target, failed, "standard", "medium")[0];
  expect(standard).toMatchObject({ eligible: true, pressure: "unknown", effectiveWeight: 0.5 });
  const frontier = balancedAllocationEstimates(target, failed, "frontier", "xhigh")[0];
  expect(frontier).toMatchObject({ eligible: false, pressure: "exhausted", effectiveWeight: 0 });
  expect(() => selectProviderFromAvailability(
    { target: "claude-personal" }, target, failed, "frontier", "failed-frontier", "xhigh",
  )).toThrow("routing target claude-personal entitlement exhausted");
});

test("reserved allocation preserves a frontier provider for non-frontier work", () => {
  const reserved = policy({ mode: "reserved", reservedFrontierProvider: "anthropic" });
  const normal = selectProviderFromAvailability("auto", available, reserved, "standard", "normal");
  const frontier = selectProviderFromAvailability("auto", available, reserved, "frontier", "frontier");
  expect(normal.provider).toBe("openai");
  expect(normal.reason).toContain("preserving frontier reserve=anthropic");
  expect(frontier.provider).toBe("anthropic");
  expect(frontier.reason).toContain("frontier reserve=anthropic");
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
    pressures: { anthropic: "plenty", openai: "plenty" },
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
    pressures: { anthropic: "exhausted", openai: "normal" },
  });
  expect(selectProviderFromAvailability("auto", available, anthropicExhausted, "frontier", "x").provider).toBe("openai");
});

test("semantic tiers resolve independently per provider", () => {
  expect(resolveTier("anthropic", "senior")).toEqual({ tier: "senior", model: "claude-opus-4-8", effort: "high" });
  expect(resolveTier("openai", "frontier")).toEqual({ tier: "frontier", model: "gpt-5.6-sol", effort: "xhigh" });
});

test("provider selection filters tier and reasoning before allocation", () => {
  expect(() => resolveTier("openai", "senior", undefined, "xhigh"))
    .toThrow("provider openai cannot resolve semantic tier senior with reasoning xhigh");
  const decision = selectProviderFromAvailability(
    "auto",
    available,
    policy({ providerOrder: ["openai", "anthropic"] }),
    "senior",
    "asymmetric-route",
    "xhigh",
  );
  expect(decision.provider).toBe("anthropic");
  expect(decision.fallbackProviders).toEqual([]);
  expect(decision.selectionReason).toContain("route=senior/xhigh");
  try {
    selectProviderFromAvailability("openai", available, policy(), "senior", "exact-incompatible", "xhigh");
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
  )).toThrow("no eligible provider resolves");
});

test("temporary Fable promotion is Anthropic-only at the semantic frontier", () => {
  process.env.NORTH_FABLE_NOW = "2026-07-19T00:00:00Z";
  expect(resolveTier("anthropic", "frontier")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "xhigh" });
  expect(resolveTier("anthropic", "frontier", undefined, "high")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "high" });
  expect(resolveTier("anthropic", "frontier", undefined, "xhigh")).toEqual({ tier: "frontier", model: "claude-fable-5", effort: "xhigh" });
  expect(resolveTier("anthropic", "frontier", "opus", "xhigh")).toEqual({ tier: "frontier", model: "claude-opus-4-8", effort: "xhigh" });
  expect(resolveTier("anthropic", "frontier", undefined, "max")).toEqual({ tier: "frontier", model: "claude-opus-4-8", effort: "max" });
  expect(resolveTier("openai", "frontier")).toEqual({ tier: "frontier", model: "gpt-5.6-sol", effort: "xhigh" });
  process.env.NORTH_FABLE_NOW = "2026-07-20T04:00:00Z";
  expect(resolveTier("anthropic", "frontier")).toEqual({ tier: "frontier", model: "claude-opus-4-8", effort: "xhigh" });
});

function fakeProvider(id: ProviderId, query: AgentProvider["query"]): AgentProvider {
  return { id, probe: () => ({ provider: id, available: true, reason: "ready" }), query };
}

async function eventsOf(query: AsyncIterable<any>): Promise<any[]> {
  const events: any[] = [];
  for await (const event of query) events.push(event);
  return events;
}

test("Anthropic adapter diagnostics redact SDK failures across stream and controls", async () => {
  const canary = "ANTHROPIC_SDK_CANARY_DO_NOT_EXPOSE";
  const diagnosticEvents = await eventsOf(normalizeAnthropicQueryDiagnostics({ async *[Symbol.asyncIterator]() {
    yield { type: "result", subtype: "error_during_execution", errors: [canary] };
    yield { type: "assistant", error: "server_error", message: { content: [{ type: "text", text: canary }] } };
    yield { type: "auth_status", output: [canary], error: canary };
    yield { type: "system", subtype: "mirror_error", error: canary };
    yield { type: "system", subtype: "status", compact_error: canary };
  }}));
  expect(JSON.stringify(diagnosticEvents)).not.toContain(canary);
  expect(diagnosticEvents[0].errors).toEqual(["anthropic_provider_execution_failed"]);
  expect(diagnosticEvents[1].message.content).toEqual([]);
  expect(diagnosticEvents[2]).toMatchObject({ output: [], error: "anthropic_provider_authentication_failed" });
  expect(diagnosticEvents[3].error).toBe("anthropic_provider_execution_failed");
  expect(diagnosticEvents[4].compact_error).toBe("anthropic_provider_execution_failed");

  for (const apiKeySource of ["user", "project", "org", "temporary", "unknown", undefined]) {
    const nonSubscription = normalizeAnthropicQueryDiagnostics({ async *[Symbol.asyncIterator]() {
      yield { type: "system", subtype: "init", apiKeySource };
    }});
    try {
      await eventsOf(nonSubscription);
      throw new Error(`expected ${String(apiKeySource)} to be rejected`);
    } catch (error) {
      expect(error).not.toBeInstanceOf(ProviderRetrySafeError);
      expect((error as Error).message).toBe("anthropic_provider_execution_failed");
    }
  }
  for (const apiKeySource of ["oauth", "none"]) {
    expect(await eventsOf(normalizeAnthropicQueryDiagnostics({ async *[Symbol.asyncIterator]() {
      yield { type: "system", subtype: "init", apiKeySource };
    }}))).toEqual([{ type: "system", subtype: "init", apiKeySource }]);
  }

  const source = {
    interrupt: async () => { throw new Error(canary); },
    setModel: async () => { throw new Error(canary); },
    applyFlagSettings: async () => { throw new Error(canary); },
    supportsInFlightEscalation: () => { throw new Error(canary); },
    async *[Symbol.asyncIterator]() { throw new Error(canary); },
  };
  const query = normalizeAnthropicQueryDiagnostics(source);
  await expect(eventsOf(query)).rejects.toThrow("anthropic_provider_execution_failed");
  await expect(query.interrupt!()).rejects.toThrow("anthropic_provider_execution_failed");
  await expect(query.setModel!("opus")).rejects.toThrow("anthropic_provider_execution_failed");
  await expect(query.applyFlagSettings!({ effortLevel: "high" })).rejects.toThrow("anthropic_provider_execution_failed");
  expect(() => query.supportsInFlightEscalation!()).toThrow("anthropic_provider_execution_failed");
  for (const action of [
    () => eventsOf(query),
    () => query.interrupt!(),
    () => query.setModel!("opus"),
    () => query.applyFlagSettings!({ effortLevel: "high" }),
  ]) {
    try { await action(); } catch (error) { expect(String(error)).not.toContain(canary); }
  }
});

test("Anthropic readonly capability requires an exact hard sandbox before SDK side effects", () => {
  const base = {
    northCapabilities: ["filesystem.read", "filesystem.search", "shell.readonly"],
    disallowedTools: [
      "Edit", "Write", "MultiEdit", "NotebookEdit", "WebSearch", "WebFetch",
      "mcp__north__spawn", "mcp__north__dispatch", "mcp__north-peer__command_peer",
    ],
    cwd: "/tmp/north-anthropic-readonly",
  } as any;
  for (const sandbox of [
    undefined,
    { enabled: true, failIfUnavailable: false, allowUnsandboxedCommands: false,
      filesystem: { denyWrite: [base.cwd] } },
    { enabled: true, failIfUnavailable: true, allowUnsandboxedCommands: true,
      filesystem: { denyWrite: [base.cwd] } },
    { enabled: true, failIfUnavailable: true, allowUnsandboxedCommands: false,
      filesystem: { denyWrite: ["/tmp/other"] } },
  ]) {
    let caught: unknown;
    try { anthropicProvider.query({ prompt: "x", options: { ...base, sandbox } }); }
    catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(ProviderRetrySafeError);
    expect((caught as Error).message).toBe("anthropic_readonly_sandbox_contract_missing");
  }
});

test("unsafe Anthropic init is redacted and never manufactures retry-safe fallback", async () => {
  const decision = selectProviderFromAvailability("auto", accountAvailability, accountPolicy(), "standard");
  let anthropicCalls = 0;
  let openAiCalls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", () => {
      anthropicCalls++;
      return normalizeAnthropicQueryDiagnostics({ async *[Symbol.asyncIterator]() {
        yield { type: "system", subtype: "init", apiKeySource: "user" };
      }});
    }),
    openai: fakeProvider("openai", () => {
      openAiCalls++;
      return { async *[Symbol.asyncIterator]() { yield { type: "result", result: "must not run" }; } };
    }),
  };

  try {
    await eventsOf(routedQuery(decision, { prompt: "x", options: {} as any }, "standard", registry));
    throw new Error("expected unsafe init rejection");
  } catch (error) {
    expect(error).not.toBeInstanceOf(ProviderRetrySafeError);
    expect((error as Error).message).toBe("anthropic_provider_execution_failed");
  }
  expect(anthropicCalls).toBe(1);
  expect(openAiCalls).toBe(0);
  expect(decision.fallbackCount).toBe(0);
  expect(decision.fallbackTargetPath).toEqual(["claude-personal"]);
  expect(decision.fallbackReasons).toEqual([]);
});

test("concurrent auto routes accept CLI-owned subscription init with honest identity and provenance", async () => {
  const decisions = Array.from({ length: 4 }, (_, index) =>
    selectProviderFromAvailability("auto", accountAvailability, accountPolicy(), "standard", `lane-${index}`));
  const activated: string[][] = decisions.map(() => []);
  let openAiCalls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", (args) => normalizeAnthropicQueryDiagnostics({
      async *[Symbol.asyncIterator]() {
        await Promise.resolve();
        yield { type: "system", subtype: "init", apiKeySource: "none" };
        yield { type: "result", subtype: "success", result: args.target?.id };
      },
    })),
    openai: fakeProvider("openai", () => {
      openAiCalls++;
      return { async *[Symbol.asyncIterator]() { yield { type: "result", result: "must not run" }; } };
    }),
  };

  const results = await Promise.all(decisions.map((decision, index) => eventsOf(routedQuery(
    decision,
    { prompt: `task-${index}`, options: { model: "sonnet", effort: "medium" } as any },
    "standard",
    registry,
    undefined,
    (route) => activated[index].push(`${route.target}/${route.provider}`),
  ))));

  expect(results).toEqual(Array.from({ length: 4 }, () => [
    { type: "system", subtype: "init", apiKeySource: "none" },
    { type: "result", subtype: "success", result: "claude-personal" },
  ]));
  expect(openAiCalls).toBe(0);
  for (const [index, decision] of decisions.entries()) {
    expect(decision).toMatchObject({
      target: "claude-personal", provider: "anthropic", resolvedModel: "sonnet", resolvedEffort: "medium",
      fallbackCount: 0, fallbackTargetPath: ["claude-personal"], fallbackReasons: [],
    });
    expect(decision.selectionReason).toBe(
      "mode=preferential; target=claude-personal; pressure=normal; "
      + "order=claude-personal -> claude-work -> codex-personal",
    );
    expect(decision.reason).toBe(decision.selectionReason);
    expect(activated[index]).toEqual(["claude-personal/anthropic"]);
    const identity = Object.fromEntries(agentRouteFacts(`lane-${index}`, {
      kind: "lane", role: "implementer", provider: decision.provider, providerTarget: decision.target,
      model: decision.resolvedModel, effort: decision.resolvedEffort,
      compositionKind: "preset", compositionId: "implementer", compositionOverrides: [], goal: `task-${index}`,
    }));
    expect(identity).toMatchObject({ provider: "anthropic", provider_target: "claude-personal" });
    expect(identity.display_name).toContain("anthropic:claude-personal · sonnet · medium · gaffer:implementer");
  }
});

test("an explicitly retry-safe synthetic Anthropic failure re-resolves the tier on OpenAI", async () => {
  const decision = selectProviderFromAvailability("auto", available, policy(), "frontier");
  const initialReason = decision.selectionReason;
  const calls: Array<{ provider: ProviderId; args: any }> = [];
  const prompt = "preserve this prompt";
  const activated: string[] = [];
  const registry = {
    anthropic: fakeProvider("anthropic", (args) => ({ async *[Symbol.asyncIterator]() {
      calls.push({ provider: "anthropic", args });
      throw new ProviderRetrySafeError("subscription usage limit reached; bearer secret-must-not-leak");
    }})),
    openai: fakeProvider("openai", (args) => ({ async *[Symbol.asyncIterator]() {
      calls.push({ provider: "openai", args });
      yield { type: "result", result: "ok" };
    }})),
  };

  expect(await eventsOf(routedQuery(decision, {
    prompt, options: { model: "fable", effort: "xhigh", systemPrompt: "keep system" } as any,
  }, "frontier", registry, undefined,
  (route) => activated.push(`${route.provider}/${route.resolvedModel}/${route.resolvedEffort}`))))
    .toEqual([{ type: "result", result: "ok" }]);
  expect(calls.map((call) => call.provider)).toEqual(["anthropic", "openai"]);
  expect(calls[1].args.prompt).toBe(prompt);
  expect(calls[1].args.options.systemPrompt).toBe("keep system");
  expect(calls[1].args.options.model).toBe("gpt-5.6-sol");
  expect(calls[1].args.options.effort).toBe("xhigh");
  expect(decision.provider).toBe("openai");
  expect(decision.fallbackCount).toBe(1);
  expect(decision.fallbackPath).toEqual(["anthropic", "openai"]);
  expect(decision.fallbackTargetPath).toEqual(["anthropic", "openai"]);
  expect(decision.reason).toBe(initialReason);
  expect(decision.selectionReason).toBe(initialReason);
  expect(() => { (decision as any).reason = "rewritten"; }).toThrow();
  expect(decision.reason).toBe(initialReason);
  expect(initialReason).toContain("mode=preferential");
  expect(initialReason).toContain("pressure=normal");
  expect(initialReason).toContain("order=anthropic -> openai");
  expect(decision.fallbackReasons).toEqual([{
    sequence: 1,
    reason: "provider_retry_safe_before_acceptance",
    fromTarget: "anthropic", fromProvider: "anthropic",
    toTarget: "openai", toProvider: "openai",
  }]);
  expect(JSON.stringify(decision.fallbackReasons)).not.toContain("secret-must-not-leak");
  expect(decision.resolvedModel).toBe(calls[1].args.options.model);
  expect(decision.resolvedEffort).toBe(calls[1].args.options.effort);
  expect(activated).toEqual(["anthropic/fable/xhigh", "openai/gpt-5.6-sol/xhigh"]);
});

test("provider-pinned retry-safe failure advances to a sibling target only", async () => {
  const decision = selectProviderFromAvailability("anthropic", accountAvailability, accountPolicy(), "senior");
  let calls = 0;
  const routes: string[] = [];
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({ async *[Symbol.asyncIterator]() {
      calls++;
      if (calls === 1) throw new ProviderRetrySafeError("account unavailable before acceptance");
      yield { type: "result", result: "ok" };
    }})),
    openai: fakeProvider("openai", () => { throw new Error("cross-provider fallback must remain filtered"); }),
  };
  expect(await eventsOf(routedQuery(decision, { prompt: "x", options: { model: "opus", effort: "high" } as any },
    "senior", registry, undefined, (route) => routes.push(`${route.target}/${route.provider}`))))
    .toEqual([{ type: "result", result: "ok" }]);
  expect(routes).toEqual(["claude-personal/anthropic", "claude-work/anthropic"]);
  expect(decision.fallbackTargetPath).toEqual(["claude-personal", "claude-work"]);
  expect(decision.fallbackPath).toEqual(["anthropic", "anthropic"]);
  expect(decision.target).toBe("claude-work");
  expect(decision.fallbackTargets).toEqual([]);
});

test("multiple retry-safe fallbacks append redacted structured provenance", async () => {
  const decision = selectProviderFromAvailability("auto", accountAvailability, accountPolicy(), "standard");
  const selected = decision.selectionReason;
  const registry = {
    anthropic: fakeProvider("anthropic", (args) => ({ async *[Symbol.asyncIterator]() {
      throw new ProviderRetrySafeError(`private failure for ${args.target?.id}`);
    }})),
    openai: fakeProvider("openai", () => ({ async *[Symbol.asyncIterator]() {
      yield { type: "result", result: "ok" };
    }})),
  };

  expect(await eventsOf(routedQuery(decision, { prompt: "x", options: {} as any },
    "standard", registry))).toEqual([{ type: "result", result: "ok" }]);
  expect(decision.selectionReason).toBe(selected);
  expect(decision.fallbackTargetPath).toEqual(["claude-personal", "claude-work", "codex-personal"]);
  expect(decision.fallbackPath).toEqual(["anthropic", "anthropic", "openai"]);
  expect(decision.fallbackReasons).toEqual([
    { sequence: 1, reason: "provider_retry_safe_before_acceptance",
      fromTarget: "claude-personal", fromProvider: "anthropic",
      toTarget: "claude-work", toProvider: "anthropic" },
    { sequence: 2, reason: "provider_retry_safe_before_acceptance",
      fromTarget: "claude-work", fromProvider: "anthropic",
      toTarget: "codex-personal", toProvider: "openai" },
  ]);
  expect(JSON.stringify(decision.fallbackReasons)).not.toContain("private failure");
});

test("retry-safe execution failure on an exact target pin still does not fall back", async () => {
  const decision = selectProviderFromAvailability({ target: "claude-personal" }, accountAvailability, accountPolicy(), "standard");
  let calls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({ async *[Symbol.asyncIterator]() {
      calls++;
      throw new ProviderRetrySafeError("target unavailable before acceptance");
    }})),
    openai: fakeProvider("openai", () => { throw new Error("must not be called"); }),
  };
  await expect(eventsOf(routedQuery(decision, { prompt: "x", options: {} as any }, "standard", registry)))
    .rejects.toThrow("target unavailable");
  expect(calls).toBe(1);
  expect(decision.fallbackCount).toBe(0);
  expect(decision.fallbackTargetPath).toEqual(["claude-personal"]);
});

test("automatic fallback re-resolves the provider while preserving requested reasoning", async () => {
  const decision = selectProviderFromAvailability("auto", available,
    policy({ providerOrder: ["openai", "anthropic"] }), "senior", "fallback", "high");
  let fallbackArgs: any;
  const registry = {
    openai: fakeProvider("openai", () => ({ async *[Symbol.asyncIterator]() {
      throw new ProviderRetrySafeError("authentication required before acceptance");
    }})),
    anthropic: fakeProvider("anthropic", (args) => ({ async *[Symbol.asyncIterator]() {
      fallbackArgs = args;
      yield { type: "result", result: "ok" };
    }})),
  };

  await eventsOf(routedQuery(decision, {
    prompt: "x", options: { model: "gpt-5.6-sol", effort: "high", systemPrompt: "system" } as any,
  }, "senior", registry));
  expect(fallbackArgs.options.model).toBe("claude-opus-4-8");
  expect(fallbackArgs.options.effort).toBe("high");
  expect(fallbackArgs.options.systemPrompt).toBe("system");
  expect(decision.fallbackPath).toEqual(["openai", "anthropic"]);
});

test("routed query preserves both live controls and records only successful changes", async () => {
  const decision = selectProviderFromAvailability("anthropic", available, policy(), "senior");
  const changes: string[] = [];
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({
      setModel: async (model) => { changes.push(`model:${model}`); },
      applyFlagSettings: async ({ effortLevel }) => { changes.push(`effort:${effortLevel}`); },
      async *[Symbol.asyncIterator]() { yield { type: "result", result: "ok" }; },
    })),
    openai: fakeProvider("openai", () => ({ async *[Symbol.asyncIterator]() {} })),
  };
  const query = routedQuery(decision, {
    prompt: "x", options: { model: "opus", effort: "high" } as any,
  }, "senior", registry);

  await eventsOf(query);
  expect(query.supportsInFlightEscalation?.()).toBe(true);
  await query.setModel?.("claude-opus-4-8");
  await query.applyFlagSettings?.({ effortLevel: "xhigh" });

  expect(changes).toEqual(["model:claude-opus-4-8", "effort:xhigh"]);
  expect(decision.resolvedModel).toBe("claude-opus-4-8");
  expect(decision.resolvedEffort).toBe("xhigh");
});

test("routed query leaves a resolved dial unchanged when its live control fails", async () => {
  const decision = selectProviderFromAvailability("anthropic", available, policy(), "senior");
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({
      setModel: async () => { throw new Error("model control failed"); },
      applyFlagSettings: async () => { throw new Error("effort control failed"); },
      async *[Symbol.asyncIterator]() { yield { type: "result", result: "ok" }; },
    })),
    openai: fakeProvider("openai", () => ({ async *[Symbol.asyncIterator]() {} })),
  };
  const query = routedQuery(decision, {
    prompt: "x", options: { model: "opus", effort: "high" } as any,
  }, "senior", registry);

  await eventsOf(query);
  await expect(query.setModel!("claude-opus-4-8")).rejects.toThrow("model control failed");
  expect(decision.resolvedModel).toBe("opus");
  await expect(query.applyFlagSettings!({ effortLevel: "xhigh" })).rejects.toThrow("effort control failed");
  expect(decision.resolvedEffort).toBe("high");
});

test("routed query preserves an applied model when the following effort control fails", async () => {
  const decision = selectProviderFromAvailability("anthropic", available, policy(), "senior");
  const effortFailure = new Error("effort control rejected");
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({
      setModel: async () => {},
      applyFlagSettings: async () => { throw effortFailure; },
      async *[Symbol.asyncIterator]() { yield { type: "result", result: "ok" }; },
    })),
    openai: fakeProvider("openai", () => ({ async *[Symbol.asyncIterator]() {} })),
  };
  const query = routedQuery(decision, {
    prompt: "x", options: { model: "opus", effort: "high" } as any,
  }, "senior", registry);

  await eventsOf(query);
  await query.setModel!("claude-opus-4-8");
  await expect(query.applyFlagSettings!({ effortLevel: "xhigh" })).rejects.toBe(effortFailure);
  expect(decision.resolvedModel).toBe("claude-opus-4-8");
  expect(decision.resolvedEffort).toBe("high");
});

test("fallback replays a streaming prompt consumed by the failed provider", async () => {
  const decision = selectProviderFromAvailability("auto", available, policy(), "standard");
  const received: Record<string, string[]> = { anthropic: [], openai: [] };
  const consumeOne = async (provider: ProviderId, prompt: string | AsyncIterable<any>) => {
    if (typeof prompt === "string") return prompt;
    const item = await prompt[Symbol.asyncIterator]().next();
    received[provider].push(item.value.message.content);
  };
  const registry = {
    anthropic: fakeProvider("anthropic", (args) => ({ async *[Symbol.asyncIterator]() {
      await consumeOne("anthropic", args.prompt);
      throw new ProviderRetrySafeError("capacity unavailable before acceptance");
    }})),
    openai: fakeProvider("openai", (args) => ({ async *[Symbol.asyncIterator]() {
      await consumeOne("openai", args.prompt);
      yield { type: "result", result: "ok" };
    }})),
  };
  const prompt = { async *[Symbol.asyncIterator]() {
    yield { type: "user", message: { content: "same payload" } };
  }};

  await eventsOf(routedQuery(decision, { prompt, options: {} as any }, "standard", registry));
  expect(received).toEqual({ anthropic: ["same payload"], openai: ["same payload"] });
});

test("automatic routing never retries after the first emitted event", async () => {
  const decision = selectProviderFromAvailability("auto", available, policy(), "standard");
  let fallbackCalls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({ async *[Symbol.asyncIterator]() {
      yield { type: "assistant", text: "observable" };
      throw new Error("capacity exhausted");
    }})),
    openai: fakeProvider("openai", () => { fallbackCalls++; return { async *[Symbol.asyncIterator]() {} }; }),
  };

  const seen: any[] = [];
  await expect(async () => {
    for await (const event of routedQuery(decision, { prompt: "x", options: {} as any }, "standard", registry)) seen.push(event);
  }).toThrow("capacity exhausted");
  expect(seen).toHaveLength(1);
  expect(fallbackCalls).toBe(0);
  expect(decision.fallbackCount).toBe(0);
  expect(decision.fallbackPath).toEqual(["anthropic"]);
});

test("automatic routing never infers retry safety from matching error prose", async () => {
  const decision = selectProviderFromAvailability("auto", available, policy(), "standard");
  let fallbackCalls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({ async *[Symbol.asyncIterator]() {
      throw new Error("authentication required: capacity unavailable");
    }})),
    openai: fakeProvider("openai", () => { fallbackCalls++; return { async *[Symbol.asyncIterator]() {} }; }),
  };
  await expect(eventsOf(routedQuery(decision, { prompt: "x", options: {} as any }, "standard", registry)))
    .rejects.toThrow("authentication required");
  expect(fallbackCalls).toBe(0);
  expect(decision.fallbackCount).toBe(0);
});

test("an explicit provider never receives a fallback route", async () => {
  const decision = selectProviderFromAvailability("anthropic", available, policy(), "standard");
  let fallbackCalls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({ async *[Symbol.asyncIterator]() {
      throw new Error("rate limit reached");
    }})),
    openai: fakeProvider("openai", () => { fallbackCalls++; return { async *[Symbol.asyncIterator]() {} }; }),
  };

  expect(decision.fallbackProviders).toEqual([]);
  await expect(eventsOf(routedQuery(decision, { prompt: "x", options: {} as any }, "standard", registry)))
    .rejects.toThrow("rate limit reached");
  expect(fallbackCalls).toBe(0);
  expect(decision.fallbackCount).toBe(0);
  expect(decision.fallbackPath).toEqual(["anthropic"]);
});

test("fallback admission runs before the fallback provider has side effects", async () => {
  const decision = selectProviderFromAvailability("auto", available, policy(), "standard");
  let fallbackCalls = 0;
  const registry = {
    anthropic: fakeProvider("anthropic", () => ({ async *[Symbol.asyncIterator]() {
      throw new ProviderRetrySafeError("capacity unavailable before acceptance");
    }})),
    openai: fakeProvider("openai", () => { fallbackCalls++; return { async *[Symbol.asyncIterator]() {} }; }),
  };
  await expect(eventsOf(routedQuery(
    decision, { prompt: "x", options: {} as any }, "standard", registry,
    async () => { throw new Error("resource envelope month:2026-07 exhausted: retries 1/1"); },
  ))).rejects.toThrow("retries 1/1");
  expect(fallbackCalls).toBe(0);
  expect(decision.fallbackCount).toBe(0);
  expect(decision.fallbackPath).toEqual(["anthropic"]);
});
