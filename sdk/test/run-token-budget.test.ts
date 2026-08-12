import { expect, test } from "bun:test";
import {
  RUN_TOKEN_BUDGET_LIMITED_OUTCOME,
  classifyExecutionTerminal,
  wireTerminalDecision,
} from "../src/execution-outcome";
import type { HostTerminationParticipant } from "../src/host-termination";
import {
  ManagedQueryTermination,
  managedRunTokenBudgetHandoff,
  managedRunTokenTarget,
} from "../src/query-lifecycle";
import { wireRunProvenanceFacts } from "../src/run-provenance";
import { terminalNotificationCommand } from "../src/terminal-notification";
import {
  WireEventWriter,
  wireEventId,
  wireModelCallId,
  wireRunId,
  type WireModelCallUsageCoverage,
  type WireRunSnapshot,
} from "../src/wire";

function inertParticipant(): HostTerminationParticipant {
  return {
    signal: () => undefined,
    publicationSettled: () => {},
    cleanupSettled: () => {},
    release: () => {},
  };
}

function termination(
  targetTokens: number,
  onHardCapCancel: () => void = () => {},
): ManagedQueryTermination {
  return new ManagedQueryTermination(
    () => inertParticipant(),
    {
      agentId: "token-budget-lane",
      threadId: "token-budget-thread",
      goal: "exercise the inter-call token tripwire",
      repo: "/home/tom/code/north",
      tokenTarget: targetTokens,
      schedule: () => "hard-cap-not-fired",
      cancel: onHardCapCancel,
      writeHandoff: () => {
        throw new Error("hard cap was not fired");
      },
    },
  );
}

function completedCall(
  label: string,
  inputTokens: number,
  outputTokens: number,
  usageCoverage: WireModelCallUsageCoverage = "exact",
): WireRunSnapshot {
  const writer = new WireEventWriter({
    runId: wireRunId(`run:token-budget:${label}`),
    eventId: (sequence) => wireEventId(`event:token-budget:${label}:${sequence}`),
    now: () => "2026-08-12T00:00:00.000Z",
  });
  const modelCallId = wireModelCallId(`model-call:token-budget:${label}`);
  writer.append({ kind: "run.started", lifecycle: "running" });
  writer.append({
    kind: "model-call.started",
    modelCallId,
    model: { provider: "openai" },
    attempt: 1,
  });
  writer.append({
    kind: "model-call.completed",
    modelCallId,
    status: "succeeded",
    origin: "provider",
    usage: {
      lifetime: {
        inputTokens,
        outputTokens,
        cacheReadTokens: 0,
        cacheWriteTokens: 0,
        reasoningTokens: 0,
        modelCalls: 1,
      },
      context: { tokens: inputTokens + outputTokens },
    },
    usageCoverage,
  });
  return writer.snapshot()!;
}

test("the exact completed-call threshold latches one budget-limited transition", () => {
  let hardCapCancellations = 0;
  const managed = termination(100, () => { hardCapCancellations++; });
  let inputClosures = 0;
  let transitions = 0;
  managed.attachInput(() => { inputClosures++; });

  for (const snapshot of [completedCall("threshold", 80, 20), completedCall("repeat", 90, 30)]) {
    const wasAllowed = managed.continuationAllowed();
    const status = managed.observeCompletedCallUsage(snapshot);
    if (wasAllowed && status?.state === "budget_limited") transitions++;
  }

  expect(managed.tokenBudgetStatus()).toEqual({
    targetTokens: 100,
    observedTokens: 100,
    overshootTokens: 0,
    coverage: "exact",
    state: "budget_limited",
  });
  expect(transitions).toBe(1);
  expect(inputClosures).toBe(1);
  expect(hardCapCancellations).toBe(1);
  expect(managed.continuationAllowed()).toBe(false);
  expect(() => managed.throwIfContinuationBlocked()).toThrow("100-token target");
  expect(() => managed.throwIfTerminated()).toThrow("100-token target");
  let rejectedAttemptClosed = 0;
  expect(() => managed.attachQuery({
    forceClose: () => { rejectedAttemptClosed++; },
    async *[Symbol.asyncIterator]() {},
  })).toThrow("100-token target");
  expect(rejectedAttemptClosed).toBe(1);
  managed.release();
  expect(hardCapCancellations).toBe(1);
});

test("one completed call may overshoot before the inter-call tripwire stops continuation", () => {
  const managed = termination(100);
  expect(managed.observeCompletedCallUsage(completedCall("overshoot", 97, 28))).toEqual({
    targetTokens: 100,
    observedTokens: 125,
    overshootTokens: 25,
    coverage: "exact",
    state: "budget_limited",
  });
  managed.release();
});

test("exact usage below the target leaves North-owned continuations available", () => {
  const managed = termination(100);
  expect(managed.observeCompletedCallUsage(completedCall("within", 60, 20))).toEqual({
    targetTokens: 100,
    observedTokens: 80,
    coverage: "exact",
    state: "within_target",
  });
  expect(managed.continuationAllowed()).toBe(true);
  expect(() => managed.throwIfContinuationBlocked()).not.toThrow();
  managed.release();
});

test("unavailable cumulative usage is reported as unenforceable and never claims compliance", () => {
  const managed = termination(100);
  const snapshot = completedCall("unavailable", 0, 0, "unavailable");
  const status = managed.observeCompletedCallUsage(snapshot);
  expect(status).toEqual({
    targetTokens: 100,
    coverage: "unknown_incomplete_terminal",
    state: "unenforceable",
  });
  expect(managed.continuationAllowed()).toBe(true);
  const facts = new Map(wireRunProvenanceFacts({ tokenBudget: status }, 1));
  expect(facts.get("run_token_budget_status")).toBe("unenforceable");
  expect(facts.get("run_token_budget_coverage")).toBe("unknown_incomplete_terminal");
  expect(facts.has("run_token_observed")).toBe(false);
  expect(facts.has("run_token_overshoot")).toBe(false);
  expect(facts.has("run_token_budget_handoff")).toBe(false);
  managed.release();
});

test("partial cumulative usage also remains explicitly unenforceable", () => {
  const managed = termination(100);
  expect(managed.observeCompletedCallUsage(completedCall("partial", 80, 30, "partial"))).toEqual({
    targetTokens: 100,
    coverage: "partial",
    state: "unenforceable",
  });
  expect(managed.continuationAllowed()).toBe(true);
  managed.release();
});

test("budget-limited finalization is typed and carries exact handoff evidence", () => {
  const status = {
    targetTokens: 100,
    observedTokens: 125,
    overshootTokens: 25,
    coverage: "exact" as const,
    state: "budget_limited" as const,
  };
  expect(wireTerminalDecision(RUN_TOKEN_BUDGET_LIMITED_OUTCOME, undefined, undefined)).toEqual({
    lifecycle: "blocked",
    reason: { code: "blocked", detail: "blocked" },
  });
  const terminal = classifyExecutionTerminal(RUN_TOKEN_BUDGET_LIMITED_OUTCOME);
  expect(terminal).toMatchObject({
    processOutcome: "token_budget_limited",
    deliveryOutcome: "blocked",
    deliveryReason: "north_managed_run_token_budget_limited",
  });
  expect(wireRunProvenanceFacts({ tokenBudget: status }, 1)).toEqual([
    ["run_token_target", "100"],
    ["run_token_budget_status", "budget_limited"],
    ["run_token_budget_coverage", "exact"],
    ["run_token_observed", "125"],
    ["run_token_overshoot", "25"],
    ["run_token_budget_handoff", '{"reason":"managed_run_token_budget_limited","target":100,"observed":125,"overshoot":25,"coverage":"exact"}'],
  ]);
  expect(managedRunTokenBudgetHandoff(status)).toEqual({
    reason: "managed_run_token_budget_limited",
    target: 100,
    observed: 125,
    overshoot: 25,
    coverage: "exact",
  });
  expect(terminalNotificationCommand("lane", "root", {
    outcome: RUN_TOKEN_BUDGET_LIMITED_OUTCOME,
    terminal,
    terminalPublication: "committed",
    runPublication: "recorded",
    detail: "target=100 observed=125 overshoot=25 coverage=exact",
  })?.args).toContain("TOKEN TARGET");
});

test("managed run token targets reject non-positive and unsafe values", () => {
  for (const value of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, "100"]) {
    expect(() => managedRunTokenTarget(value)).toThrow(
      "managed run token target must be a positive safe integer",
    );
  }
  expect(managedRunTokenTarget(undefined)).toBeUndefined();
  expect(managedRunTokenTarget(1)).toBe(1);
});
