import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  agentRouteFacts, orchestrationProvenance, goalFromPrompt, providerTargetLabel, renderDisplayName, semanticHandle,
} from "../src/identity";
import type { ObservedAgentIdentity } from "../src/identity";

interface RosterFixture {
  name: string;
  id: string;
  facts: Record<string, string>;
  expected: {
    providerLabel: string;
    modelDisplay: string;
    effortDisplay: string;
    orchestrationProvenance: string;
    semanticHandle: string;
    displayName: string;
    primaryLine: string;
  };
}

const rosterFixtures = JSON.parse(readFileSync(
  new URL("./fixtures/agent-roster-contract.json", import.meta.url),
  "utf8",
)) as RosterFixture[];

function observedIdentity(facts: Record<string, string>): ObservedAgentIdentity {
  const promotion = facts.promotion_candidate === undefined
    ? undefined : facts.promotion_candidate === "true";
  return {
    kind: (facts.kind ?? "lane") as ObservedAgentIdentity["kind"],
    role: facts.role,
    model: facts.model,
    provider: facts.provider,
    providerTarget: facts.provider_target,
    effort: facts.effort,
    compositionKind: facts.composition_kind as ObservedAgentIdentity["compositionKind"],
    compositionId: facts.composition_id,
    compositionOverrides: facts.composition_overrides === undefined
      ? undefined : JSON.parse(facts.composition_overrides),
    compositionOverrideReason: facts.composition_override_reason,
    compositionNearestTemplate: facts.nearest_template,
    compositionBespokeReason: facts.bespoke_reason,
    compositionPromotionCandidate: promotion,
    compositionContractFingerprint: facts.composition_contract_sha256,
    compositionContractFingerprintVersion: facts.composition_contract_fingerprint_version,
    compositionContractFingerprintDomain: facts.composition_contract_fingerprint_domain,
    repo: facts.repo,
    goal: facts.goal,
  };
}

test("shared roster fixtures preserve semantic identity across provider adapters", () => {
  for (const fixture of rosterFixtures) {
    const identity = observedIdentity(fixture.facts);
    expect(providerTargetLabel(identity), fixture.name).toBe(fixture.expected.providerLabel);
    expect(orchestrationProvenance(identity), fixture.name).toBe(fixture.expected.orchestrationProvenance);
    expect(semanticHandle(fixture.id, identity), fixture.name).toBe(fixture.expected.semanticHandle);
    expect(renderDisplayName(fixture.id, identity), fixture.name).toBe(fixture.expected.displayName);
    expect(fixture.facts.display_name, fixture.name).not.toBe(fixture.expected.primaryLine);
  }
});

test("semantic handles keep provider-specific model families and stable control suffixes", () => {
  expect(semanticHandle("sdk-a205e9ce", {
    kind: "lane", provider: "openai", model: "gpt-5.6-sol", effort: "xhigh",
    role: "designer", compositionKind: "template", compositionId: "designer", compositionOverrides: [],
  })).toBe("openai-sol-xhigh-orchestration-designer-a205e9ce");
});

test("managed identity exposes the exact account target and Orchestration template", () => {
  const identity = {
    kind: "lane" as const, provider: "openai", providerTarget: "codex-work",
    model: "gpt-5.6-sol", effort: "xhigh", compositionKind: "template" as const,
    role: "designer", compositionId: "designer", compositionOverrides: [],
    goal: "Build the account-aware roster",
  };
  expect(providerTargetLabel(identity)).toBe("openai:codex-work");
  expect(renderDisplayName("lane-a205e9ce", identity))
    .toBe("openai:codex-work · sol · xhigh · orchestration:designer · Build the account-aware roster");
  expect(semanticHandle("lane-a205e9ce", identity)).toBe("openai-codex-work-sol-xhigh-orchestration-designer-a205e9ce");
  expect(providerTargetLabel({ kind: "lane", provider: "anthropic", providerTarget: "anthropic" }))
    .toBe("anthropic:ambient");
  expect(providerTargetLabel({ kind: "session", provider: "anthropic" })).toBe("anthropic");
});

test("fallback route facts replace provider target and refresh public identity", () => {
  const base = {
    kind: "lane" as const, model: "opus", effort: "high", compositionKind: "template" as const,
    role: "integrator", compositionId: "integrator", compositionOverrides: [], goal: "Integrate the change",
  };
  const initial = Object.fromEntries(agentRouteFacts("lane-route", {
    ...base, provider: "anthropic", providerTarget: "claude-personal",
  }));
  const fallback = Object.fromEntries(agentRouteFacts("lane-route", {
    ...base, provider: "openai", providerTarget: "codex-work", model: "gpt-5.6-sol", effort: "xhigh",
  }));
  expect(initial.provider_target).toBe("claude-personal");
  expect(fallback).toMatchObject({ provider: "openai", provider_target: "codex-work" });
  expect(fallback.display_name).toContain("openai:codex-work · sol · xhigh · orchestration:integrator");
});

test("Orchestration provenance distinguishes exact, overridden, bespoke, native, and legacy debt", () => {
  expect(orchestrationProvenance({
    kind: "lane", role: "designer", compositionKind: "template",
    compositionId: "designer", compositionOverrides: [],
  }))
    .toBe("orchestration:designer");
  expect(orchestrationProvenance({
    kind: "lane", role: "integrator", compositionKind: "template", compositionId: "integrator",
    compositionOverrides: ["tier", "reasoning"], compositionOverrideReason: "high leverage seam",
  })).toBe("orchestration:integrator+override(tier,reasoning)");
  expect(orchestrationProvenance({
    kind: "lane", role: "migration-forensics", compositionKind: "bespoke",
    compositionId: "migration-forensics", compositionBespokeReason: "one-off provenance analysis",
    compositionPromotionCandidate: false, compositionContractFingerprint: "a".repeat(64),
    compositionContractFingerprintVersion: "v1",
    compositionContractFingerprintDomain: "north:bespoke-contract:v1",
  }))
    .toBe("orchestration:bespoke:migration-forensics");
  expect(orchestrationProvenance({ kind: "session" }))
    .toBe("orchestration:not-selected");
  expect(orchestrationProvenance({ kind: "lane" })).toBe("orchestration:legacy-debt");
  expect(orchestrationProvenance({
    kind: "lane", compositionKind: "template", compositionId: "integrator",
    compositionOverrides: ["tier"],
  })).toBe("orchestration:legacy-debt");
});

test("managed missing composition and historical none are decode-only legacy debt", () => {
  expect(semanticHandle("lane-legacy", {
    kind: "lane", provider: "openai", providerTarget: "codex-work",
    model: "gpt-5.6-sol", effort: "high",
  })).toBe("openai-codex-work-sol-high-orchestration-legacy-debt-legacy");
  // Migration compatibility only: current native writers omit composition_kind.
  expect(semanticHandle("session-native", {
    kind: "session", provider: "openai", model: "gpt-5.6-sol", effort: "unobserved",
    compositionKind: "none",
  })).toBe("openai-sol-unobserved-orchestration-not-selected-native");
});

test("delegated prompt scaffolding yields the actual delegated task", () => {
  const prompt = `CONTEXT BRIEF:\n- prior context\n\nDELEGATE TASK: Implement the canonical agent roster.\n\nOPERATING CONTRACT: verify it`;
  expect(goalFromPrompt(prompt)).toBe("Implement the canonical agent roster.");
});
