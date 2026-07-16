import { expect, test } from "bun:test";
import { activeLadder, decideEscalation, escalateInFlight } from "../src/ladder";
import { ProviderEscalationUnsupportedError } from "../src/providers/types";

test("escalation is a pure next-rung or ceiling decision", () => {
  const admittedLadder = activeLadder();
  expect(decideEscalation(0, admittedLadder)).toEqual({ kind: "escalate", toTier: 1 });
  expect(decideEscalation(admittedLadder.length - 1, admittedLadder)).toEqual({ kind: "struggle_ceiling" });
});

test("unsupported providers stop before either live dial or the continuation changes", async () => {
  const pushed: string[] = [];
  await expect(escalateInFlight({
    supportsInFlightEscalation: () => false,
    async *[Symbol.asyncIterator]() {},
  }, { push: (value) => pushed.push(value) }, { model: "opus", effort: "xhigh" }, "errors"))
    .rejects.toBeInstanceOf(ProviderEscalationUnsupportedError);
  expect(pushed).toEqual([]);
});

test("real live-control failures propagate and do not emit a false upgrade", async () => {
  const pushed: string[] = [];
  const failure = new Error("provider control rejected");
  await expect(escalateInFlight({
    setModel: async () => { throw failure; },
    applyFlagSettings: async () => {},
    async *[Symbol.asyncIterator]() {},
  }, { push: (value) => pushed.push(value) }, { model: "opus", effort: "xhigh" }, "errors"))
    .rejects.toBe(failure);
  expect(pushed).toEqual([]);
});

test("a second-control failure reports the already-applied model and preserves the error", async () => {
  const pushed: string[] = [];
  const applied: Array<{ model?: string; effort?: string }> = [];
  const failure = new Error("effort control rejected");
  await expect(escalateInFlight({
    setModel: async () => {},
    applyFlagSettings: async () => { throw failure; },
    async *[Symbol.asyncIterator]() {},
  }, { push: (value) => pushed.push(value) }, { model: "opus", effort: "xhigh" }, "errors",
  (route) => applied.push(route)))
    .rejects.toBe(failure);
  expect(applied).toEqual([{ model: "claude-opus-4-8" }]);
  expect(pushed).toEqual([]);
});
