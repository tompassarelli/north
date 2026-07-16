import { expect, test } from "bun:test";
import {
  agentRouteFacts, gafferProvenance, goalFromPrompt, providerTargetLabel, renderDisplayName, semanticHandle,
} from "../src/identity";

test("semantic handles keep provider-specific model families and stable control suffixes", () => {
  expect(semanticHandle("sdk-a205e9ce", {
    kind: "lane", provider: "openai", model: "gpt-5.6-sol", effort: "xhigh",
    compositionKind: "preset", compositionId: "designer",
  })).toBe("openai-sol-xhigh-designer-a205e9ce");
});

test("managed identity exposes the exact account target and Gaffer template", () => {
  const identity = {
    kind: "lane" as const, provider: "openai", providerTarget: "codex-work",
    model: "gpt-5.6-sol", effort: "xhigh", compositionKind: "preset" as const,
    compositionId: "designer", goal: "Build the account-aware roster",
  };
  expect(providerTargetLabel(identity)).toBe("openai:codex-work");
  expect(renderDisplayName("lane-a205e9ce", identity))
    .toBe("openai:codex-work · sol · xhigh · gaffer:designer · Build the account-aware roster");
  expect(semanticHandle("lane-a205e9ce", identity)).toBe("openai-codex-work-sol-xhigh-designer-a205e9ce");
  expect(providerTargetLabel({ kind: "lane", provider: "anthropic", providerTarget: "anthropic" }))
    .toBe("anthropic:ambient");
  expect(providerTargetLabel({ kind: "session", provider: "anthropic" })).toBe("anthropic");
});

test("fallback route facts replace provider target and refresh public identity", () => {
  const base = {
    kind: "lane" as const, model: "opus", effort: "high", compositionKind: "preset" as const,
    compositionId: "integrator", goal: "Integrate the change",
  };
  const initial = Object.fromEntries(agentRouteFacts("lane-route", {
    ...base, provider: "anthropic", providerTarget: "claude-personal",
  }));
  const fallback = Object.fromEntries(agentRouteFacts("lane-route", {
    ...base, provider: "openai", providerTarget: "codex-work", model: "gpt-5.6-sol", effort: "xhigh",
  }));
  expect(initial.provider_target).toBe("claude-personal");
  expect(fallback).toMatchObject({ provider: "openai", provider_target: "codex-work" });
  expect(fallback.display_name).toContain("openai:codex-work · sol · xhigh · gaffer:integrator");
});

test("Gaffer provenance distinguishes preset, bespoke, native, and unknown", () => {
  expect(gafferProvenance({ kind: "lane", compositionKind: "preset", compositionId: "designer" }))
    .toBe("gaffer:designer");
  expect(gafferProvenance({ kind: "lane", compositionKind: "bespoke", compositionId: "migration-forensics" }))
    .toBe("gaffer:bespoke(migration-forensics)");
  expect(gafferProvenance({ kind: "session", compositionKind: "none" })).toBe("gaffer:none");
  expect(gafferProvenance({ kind: "lane" })).toBe("gaffer:unknown");
});

test("delegated prompt scaffolding yields the actual delegated task", () => {
  const prompt = `CONTEXT BRIEF:\n- prior context\n\nDELEGATE TASK: Implement the canonical agent roster.\n\nOPERATING CONTRACT: verify it`;
  expect(goalFromPrompt(prompt)).toBe("Implement the canonical agent roster.");
});
