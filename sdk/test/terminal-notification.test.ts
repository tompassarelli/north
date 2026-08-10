import { expect, test } from "bun:test";
import {
  classifyExecutionTerminal, describeProviderErrorTerminal,
  EMPTY_PROVIDER_ERROR_DETAIL, NO_PROVIDER_TERMINAL_DETAIL,
  PROVIDER_ERROR_DETAIL_MAX_LEN,
} from "../src/execution-outcome";
import {
  terminalNotificationCommand,
  terminalPublicationBudgetMs,
  TerminalPublicationBudget,
} from "../src/terminal-notification";
import { framBabashkaArguments } from "../src/fram-engine";
import {
  WireEventWriter,
  wireEventId,
  wireMessageId,
  wireModelCallId,
  wireRunId,
  type WireCompletionEvidence,
  type WireRunSnapshot,
} from "../src/wire";

function modelTerminalSnapshot(
  label: string,
  status: "succeeded" | "failed" | "cancelled",
  evidence?: WireCompletionEvidence,
): WireRunSnapshot {
  const writer = new WireEventWriter({
    runId: wireRunId(`run:terminal-${label}`),
    eventId: (sequence) => wireEventId(`event:terminal-${label}-${sequence}`),
    now: () => "2026-08-10T00:00:00.000Z",
  });
  const modelCallId = wireModelCallId(`model-call:terminal-${label}`);
  const messageId = wireMessageId(`message:terminal-${label}`);
  writer.append({ kind: "run.started", lifecycle: "running" });
  writer.append({
    kind: "model-call.started",
    modelCallId,
    model: { provider: "openai", tier: "standard" },
    attempt: 1,
  });
  writer.append({
    kind: "message.recorded",
    messageId,
    modelCallId,
    stage: "started",
    role: "assistant",
  });
  writer.append({
    kind: "message.recorded",
    messageId,
    modelCallId,
    stage: "delta",
    role: "assistant",
    content: "PROVIDER_PROSE_CANARY partial answer",
  });
  writer.append({
    kind: "message.recorded",
    messageId,
    modelCallId,
    stage: "completed",
    role: "assistant",
  });
  writer.append({
    kind: "model-call.completed",
    modelCallId,
    status,
    origin: "provider",
    usage: writer.snapshot()!.usage,
		usageCoverage: "exact",
    ...(status === "succeeded" ? {} : { errorCode: "provider_error" }),
    ...(evidence === undefined ? {} : { evidence }),
  });
  return writer.snapshot()!;
}

test("success completes while a preflight refusal reports an honest blocked terminal", () => {
  for (const [processOutcome, subject] of [
    ["ran", "AGENT COMPLETE"],
    ["blocked_preflight", "AGENT BLOCKED"],
  ]) {
    const terminal = classifyExecutionTerminal(processOutcome);
    const command = terminalNotificationCommand(
      "child",
      "coordinator",
      {
        outcome: processOutcome,
        terminal,
        terminalPublication: "recorded",
        runPublication: "recorded",
      },
    );
    expect(command?.args.slice(0, 4)).toEqual(framBabashkaArguments([
      expect.stringMatching(/\/cli\/msg-cli\.clj$/),
      expect.any(String),
    ]));
    expect(command?.args.slice(4)).toEqual([
      "send",
      "child",
      "coordinator",
      subject,
      `process=${processOutcome} — delivery=${terminal.deliveryOutcome} — terminal=recorded — run=recorded`,
    ]);
  }
});

test("death, watchdog, legacy stall, and turn-cap terminals retain one dedicated post-publication subject", () => {
  for (const [outcome, subject] of [
    ["died", "AGENT DEATH"],
    ["watchdog_aborted", "AGENT DEATH"],
    ["stalled", "AGENT DEATH"],
    ["max_turns", "TURN CAP"],
    ["capped", "TURN CAP"],
  ]) {
    const terminal = classifyExecutionTerminal(outcome);
    const command = terminalNotificationCommand(
      "child",
      "coordinator",
      {
        outcome,
        terminal,
        terminalPublication: "recorded",
        runPublication: "recorded",
        detail: " bounded\n detail ",
      },
    );
    expect(command?.args.at(-2)).toBe(subject);
    expect(command?.args.at(-1)).toBe(
      `bounded detail — process=${outcome} — delivery=${terminal.deliveryOutcome} — terminal=recorded — run=recorded`,
    );
  }
});

test("missing coordinators stay message-free and degraded publication is explicit", () => {
  const terminal = classifyExecutionTerminal("provider_error");
  expect(terminalNotificationCommand(
    "child",
    undefined,
    {
      outcome: "provider_error",
      terminal,
      terminalPublication: "recorded",
      runPublication: "recorded",
    },
  )).toBeUndefined();
  expect(terminalNotificationCommand(
    "child",
    "coordinator",
    {
      outcome: "provider_error",
      terminal,
      terminalPublication: "unavailable",
      runPublication: "unavailable",
    },
  )?.args.at(-1)).toEndWith("terminal=unavailable — run=unavailable");
});

test("one configurable wall-clock budget is split across both publications and the wake", () => {
  let now = 0;
  const budget = new TerminalPublicationBudget(1_000, () => now);
  expect(budget.publicationTimeout(2)).toBe(400);
  now = 400;
  expect(budget.publicationTimeout(1)).toBe(400);
  now = 800;
  expect(budget.notificationTimeout()).toBe(200);
  now = 1_500;
  expect(budget.notificationTimeout()).toBe(1);

  expect(terminalPublicationBudgetMs("50")).toBe(100);
  // A run record is ~200 facts and one coordinator round-trip each, so the
  // ceiling and default were raised to match what the write actually costs;
  // 90s is now an accepted override rather than clamped to the old 60s cap.
  expect(terminalPublicationBudgetMs("90000")).toBe(90_000);
  expect(terminalPublicationBudgetMs("400000")).toBe(300_000);
  expect(terminalPublicationBudgetMs("not-a-timeout")).toBe(90_000);
});

// thread 019f9cec: `provider_error` names a classification, not a cause. Three
// managed Codex lanes settled provider_error/blocked/turns=0 on 2026-07-26 with
// the provider's own account of the failure sitting in the terminal frame the
// message loop dropped. These pin the render that keeps it.
test("a failed model terminal renders typed evidence, never assistant prose", () => {
  const detail = describeProviderErrorTerminal(modelTerminalSnapshot("failure", "failed", {
    providerJoin: {
      version: "north-provider-join:v1",
      sessionKey: "a".repeat(64),
      turnKeys: ["b".repeat(64)],
      sessionPersistence: "ephemeral",
      coverage: "exact",
    },
    turns: { unit: "provider-turn", count: 1, toolItems: 4, comparable: false },
    failure: {
      landed: { completedTurns: 0, toolItems: 4, mcpCalls: 3, nativeCommands: 1 },
      detail: "provider turn error: stream disconnected before completion",
    },
  }));
  expect(detail).toContain("status=failed");
  expect(detail).toContain("code=provider_error");
  expect(detail).toContain("stream disconnected before completion");
  expect(detail).toContain(
    "landed=[0 completed turn(s), 4 tool item(s), 3 MCP call(s), 1 native command(s)]",
  );
  expect(detail).toContain(`provider_session=${"a".repeat(64)}`);
  expect(detail).not.toContain("PROVIDER_PROSE_CANARY");
  expect(detail.length).toBeLessThanOrEqual(PROVIDER_ERROR_DETAIL_MAX_LEN);
});

test("provider_error detail is bounded, single-line, and honest when there is nothing to say", () => {
  const huge = describeProviderErrorTerminal(modelTerminalSnapshot("huge", "failed", {
    failure: { detail: "large failure ".repeat(80).trim() },
  }));
  expect(huge.length).toBeLessThanOrEqual(PROVIDER_ERROR_DETAIL_MAX_LEN);
  expect(huge).not.toContain("\n");

  const noTerminal = new WireEventWriter({ runId: wireRunId("run:terminal-none") });
  noTerminal.append({ kind: "run.started", lifecycle: "running" });
  expect(describeProviderErrorTerminal(noTerminal.snapshot()!)).toBe(NO_PROVIDER_TERMINAL_DETAIL);
  expect(describeProviderErrorTerminal(modelTerminalSnapshot("success", "succeeded")))
    .toBe(EMPTY_PROVIDER_ERROR_DETAIL);
});
