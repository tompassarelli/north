import { expect, test } from "bun:test";
import { activeLadder, baseLadder, decideEscalation, escalateInFlight, tierIndexOf } from "../src/ladder";
import { ProviderEscalationUnsupportedError } from "../src/providers/types";

test("escalation is a pure next-rung or ceiling decision", () => {
  const admittedLadder = activeLadder("anthropic");
  expect(decideEscalation(0, admittedLadder)).toEqual({ kind: "escalate", toTier: 1 });
  expect(decideEscalation(admittedLadder.length - 1, admittedLadder)).toEqual({ kind: "struggle_ceiling" });
});

test("provider ladders are concrete projections of Gaffer, with no invented aliases", () => {
  expect(baseLadder("anthropic").map(({ model, effort }) => [model, effort])).toEqual([
    ["claude-sonnet-5", "low"],
    ["claude-sonnet-5", "medium"],
    ["claude-opus-4-8", "high"],
    ["claude-opus-4-8", "xhigh"],
  ]);
  expect(baseLadder("openai").map(({ model, effort }) => [model, effort])).toEqual([
    ["gpt-5.6-luna", "low"],
    ["gpt-5.6-terra", "low"],
    ["gpt-5.6-terra", "medium"],
    ["gpt-5.6-sol", "medium"],
    ["gpt-5.6-sol", "high"],
    ["gpt-5.6-sol", "xhigh"],
    ["gpt-5.6-sol", "max"],
  ]);
});

test("unknown or transport-unsettable admitted routes never fall back to a cheaper rung", () => {
  const ladder = baseLadder("anthropic");
  expect(tierIndexOf("anthropic", "claude-opus-4-8", "max", ladder)).toBe(ladder.length - 1);
  expect(tierIndexOf("anthropic", "future-model", "high", ladder)).toBe(ladder.length - 1);
});

test("unsupported providers stop before either live dial or the continuation changes", async () => {
  const pushed: string[] = [];
  await expect(escalateInFlight("openai", {
    supportsInFlightEscalation: () => false,
    async *[Symbol.asyncIterator]() {},
  }, { push: (value) => pushed.push(value) }, {
    provider: "openai", tier: "frontier", model: "gpt-5.6-sol", effort: "xhigh",
  }, "errors"))
    .rejects.toBeInstanceOf(ProviderEscalationUnsupportedError);
  expect(pushed).toEqual([]);
});

test("real live-control failures propagate and do not emit a false upgrade", async () => {
  const pushed: string[] = [];
  const failure = new Error("provider control rejected");
  await expect(escalateInFlight("anthropic", {
    setModel: async () => { throw failure; },
    applyFlagSettings: async () => {},
    async *[Symbol.asyncIterator]() {},
  }, { push: (value) => pushed.push(value) }, {
    provider: "anthropic", tier: "senior", model: "claude-opus-4-8", effort: "xhigh",
  }, "errors"))
    .rejects.toBe(failure);
  expect(pushed).toEqual([]);
});

test("a second-control failure reports the already-applied model and preserves the error", async () => {
  const pushed: string[] = [];
  const applied: Array<{ model?: string; effort?: string }> = [];
  const failure = new Error("effort control rejected");
  await expect(escalateInFlight("anthropic", {
    setModel: async () => {},
    applyFlagSettings: async () => { throw failure; },
    async *[Symbol.asyncIterator]() {},
  }, { push: (value) => pushed.push(value) }, {
    provider: "anthropic", tier: "senior", model: "claude-opus-4-8", effort: "xhigh",
  }, "errors",
  (route) => applied.push(route)))
    .rejects.toBe(failure);
  expect(applied).toEqual([{ model: "claude-opus-4-8" }]);
  expect(pushed).toEqual([]);
});
