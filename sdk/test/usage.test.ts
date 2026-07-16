import { describe, expect, test } from "bun:test";
import { normalizeUsage, tokensOf } from "../src/usage";

const anthropicResult = (usage: Record<string, unknown>, subtype = "success") =>
  ({ type: "result", subtype, usage });

describe("provider-authoritative token telemetry", () => {
  test("one Anthropic terminal sums four disjoint categories, preserving observed zero", () => {
    const terminals = [anthropicResult({
      input_tokens: 101,
      output_tokens: 23,
      cache_creation_input_tokens: 0,
      cache_read_input_tokens: 59,
    })];
    expect(normalizeUsage(terminals, "anthropic")).toEqual({
      inputTokens: 101,
      outputTokens: 23,
      cacheCreateTokens: 0,
      cacheReadTokens: 59,
      total: 183,
      terminalCount: 1,
      terminalScope: "anthropic_result_terminal",
      totalStatus: "exact",
    });
    expect(tokensOf(terminals, "anthropic")).toBe(183);
  });

  test("zero terminals is unknown, not observed zero", () => {
    expect(normalizeUsage([], "anthropic")).toEqual({
      terminalCount: 0,
      terminalScope: "anthropic_result_terminal",
      totalStatus: "unknown_no_terminal",
    });
    expect(tokensOf([], "anthropic")).toBeUndefined();
  });

  test("an incomplete terminal preserves exact components but has no aggregate", () => {
    expect(normalizeUsage([anthropicResult({ input_tokens: 7 })], "anthropic")).toEqual({
      inputTokens: 7,
      terminalCount: 1,
      terminalScope: "anthropic_result_terminal",
      totalStatus: "unknown_incomplete_terminal",
    });
  });

  test("repeated Anthropic terminals retain count/scope but never sum or select", () => {
    const usage = { input_tokens: 10, output_tokens: 2,
      cache_creation_input_tokens: 0, cache_read_input_tokens: 0 };
    expect(normalizeUsage([
      anthropicResult(usage), anthropicResult({ ...usage, input_tokens: 20 }),
    ], "anthropic")).toEqual({
      terminalCount: 2,
      terminalScope: "anthropic_result_terminal",
      totalStatus: "unknown_repeated_terminal",
    });
  });

  test("Anthropic error terminals carry the same authoritative usage as success", () => {
    expect(normalizeUsage([anthropicResult({
      input_tokens: 11, output_tokens: 3,
      cache_creation_input_tokens: 2, cache_read_input_tokens: 5,
    }, "error_during_execution")], "anthropic")).toMatchObject({
      total: 21,
      terminalCount: 1,
      totalStatus: "exact",
    });
  });

  test("Codex trusts its adapter total and preserves cached/reasoning subsets once", () => {
    const terminal = {
      type: "result",
      usage: { input_tokens: 100, cached_input_tokens: 60,
        output_tokens: 20, reasoning_output_tokens: 7 },
      _north_usage: {
        provider: "openai" as const,
        terminal_count: 1,
        scope: "codex_fresh_invocation_thread_cumulative" as const,
        total_status: "exact" as const,
        total_tokens: 120,
      },
    };
    expect(normalizeUsage([terminal], "openai")).toEqual({
      inputTokens: 100,
      outputTokens: 20,
      cachedInputTokens: 60,
      reasoningOutputTokens: 7,
      total: 120,
      terminalCount: 1,
      terminalScope: "codex_fresh_invocation_thread_cumulative",
      totalStatus: "exact",
    });
  });

  test("an unannotated OpenAI result cannot invent adapter scope or a total", () => {
    expect(normalizeUsage([{ type: "result", usage: { input_tokens: 0 } }], "openai"))
      .toEqual({ inputTokens: 0, terminalCount: 1, totalStatus: "unknown_adapter_scope" });
  });
});
