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
    expect(command?.args.slice(2)).toEqual([
      "send",
      "child",
      "coordinator",
      subject,
      `process=${processOutcome} — delivery=${terminal.deliveryOutcome} — terminal=recorded — run=recorded`,
    ]);
  }
});

test("death, stall, and turn-cap terminals retain one dedicated post-publication subject", () => {
  for (const [outcome, subject] of [
    ["died", "AGENT DEATH"],
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
test("a provider_error terminal renders its payload, never the model's prose", () => {
  const detail = describeProviderErrorTerminal({
    type: "result",
    subtype: "error_during_execution",
    is_error: true,
    result: "PROVIDER_PROSE_CANARY partial answer",
    _north_harvest: {
      threadId: "th_abc",
      completedTurns: 0,
      mcp: { totalCalls: 3 },
      nativeCommands: { totalCommands: 1 },
      failure: "openai_provider_execution_failed <- cause: Codex completed turn"
        + " reported a provider-side turn error <- cause: provider turn error:"
        + " {\"message\":\"stream disconnected before completion\"}",
    },
  });
  expect(detail).toContain("subtype=error_during_execution");
  expect(detail).toContain("is_error=true");
  expect(detail).toContain("stream disconnected before completion");
  expect(detail).toContain("landed=[0 completed turn(s), 3 MCP call(s), 1 native command(s)]");
  expect(detail).toContain("provider_thread=th_abc");
  // The result text is model prose: recorded elsewhere, never a machine reason.
  expect(detail).not.toContain("PROVIDER_PROSE_CANARY");
  expect(detail.length).toBeLessThanOrEqual(PROVIDER_ERROR_DETAIL_MAX_LEN);
});

test("provider_error detail is bounded, single-line, and honest when there is nothing to say", () => {
  const huge = describeProviderErrorTerminal({
    subtype: "error",
    is_error: true,
    errors: Array.from({ length: 9 }, (_, index) => ({ message: `e${index} `.repeat(200) })),
  });
  expect(huge.length).toBeLessThanOrEqual(PROVIDER_ERROR_DETAIL_MAX_LEN);
  expect(huge).not.toContain("\n");
  expect(huge).toContain("+5 more");

  expect(describeProviderErrorTerminal(undefined)).toBe(NO_PROVIDER_TERMINAL_DETAIL);
  expect(describeProviderErrorTerminal({ type: "result", subtype: "success" }))
    .toBe(EMPTY_PROVIDER_ERROR_DETAIL);
});
