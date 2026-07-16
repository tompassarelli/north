// Provider-authoritative token telemetry. A total exists only when the active
// provider's terminal scope makes its formula exact; absence and ambiguity stay
// unknown instead of becoming zero.
export interface TerminalTokenUsage {
  input_tokens?: unknown;
  output_tokens?: unknown;
  cache_creation_input_tokens?: unknown;
  cache_read_input_tokens?: unknown;
  cached_input_tokens?: unknown;
  reasoning_output_tokens?: unknown;
}

export type TokenUsageScope =
  | "anthropic_result_terminal"
  | "codex_fresh_invocation_thread_cumulative";

export type TokenTotalStatus =
  | "exact"
  | "unknown_no_terminal"
  | "unknown_repeated_terminal"
  | "unknown_incomplete_terminal"
  | "unknown_adapter_scope";

export interface AdapterUsageMetadata {
  provider: "openai";
  terminal_count: number;
  scope: "codex_fresh_invocation_thread_cumulative";
  total_status: TokenTotalStatus;
  total_tokens?: number;
}

export interface TerminalUsageMessage {
  type?: unknown;
  usage?: TerminalTokenUsage;
  _north_usage?: AdapterUsageMetadata;
}

export interface NormalizedTokenUsage {
  inputTokens?: number;
  outputTokens?: number;
  cacheCreateTokens?: number;
  cacheReadTokens?: number;
  cachedInputTokens?: number;
  reasoningOutputTokens?: number;
  total?: number;
  terminalCount: number;
  terminalScope?: TokenUsageScope;
  totalStatus: TokenTotalStatus;
}

function tokenCount(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : undefined;
}

function components(usage?: TerminalTokenUsage): Omit<NormalizedTokenUsage,
  "total" | "terminalCount" | "terminalScope" | "totalStatus"> {
  const inputTokens = tokenCount(usage?.input_tokens);
  const outputTokens = tokenCount(usage?.output_tokens);
  const cacheCreateTokens = tokenCount(usage?.cache_creation_input_tokens);
  const cacheReadTokens = tokenCount(usage?.cache_read_input_tokens);
  const cachedInputTokens = tokenCount(usage?.cached_input_tokens);
  const reasoningOutputTokens = tokenCount(usage?.reasoning_output_tokens);
  return {
    ...(inputTokens !== undefined ? { inputTokens } : {}),
    ...(outputTokens !== undefined ? { outputTokens } : {}),
    ...(cacheCreateTokens !== undefined ? { cacheCreateTokens } : {}),
    ...(cacheReadTokens !== undefined ? { cacheReadTokens } : {}),
    ...(cachedInputTokens !== undefined ? { cachedInputTokens } : {}),
    ...(reasoningOutputTokens !== undefined ? { reasoningOutputTokens } : {}),
  };
}

function normalizeOpenAI(terminals: readonly TerminalUsageMessage[]): NormalizedTokenUsage {
  const terminal = terminals.at(-1);
  const metadata = terminal?._north_usage;
  if (!metadata || metadata.provider !== "openai") {
    return {
      ...components(terminal?.usage),
      terminalCount: terminals.length,
      totalStatus: terminals.length === 0 ? "unknown_no_terminal" : "unknown_adapter_scope",
    };
  }
  const total = tokenCount(metadata.total_tokens);
  const exact = metadata.total_status === "exact" && total !== undefined;
  return {
    ...components(terminal.usage),
    ...(exact ? { total } : {}),
    terminalCount: metadata.terminal_count,
    terminalScope: metadata.scope,
    totalStatus: exact ? "exact" : metadata.total_status,
  };
}

function normalizeAnthropic(terminals: readonly TerminalUsageMessage[]): NormalizedTokenUsage {
  const terminalCount = terminals.length;
  if (terminalCount === 0) return {
    terminalCount,
    terminalScope: "anthropic_result_terminal",
    totalStatus: "unknown_no_terminal",
  };
  if (terminalCount > 1) {
    return {
      terminalCount,
      terminalScope: "anthropic_result_terminal",
      totalStatus: "unknown_repeated_terminal",
    };
  }

  const observed = components(terminals[0].usage);
  // Anthropic's terminal result reports four disjoint billed categories. All
  // four must be present (zero is present) before their sum is authoritative.
  const exact = observed.inputTokens !== undefined && observed.outputTokens !== undefined &&
    observed.cacheCreateTokens !== undefined && observed.cacheReadTokens !== undefined;
  return {
    ...observed,
    ...(exact ? { total: observed.inputTokens! + observed.outputTokens! +
      observed.cacheCreateTokens! + observed.cacheReadTokens! } : {}),
    terminalCount,
    terminalScope: "anthropic_result_terminal",
    totalStatus: exact ? "exact" : "unknown_incomplete_terminal",
  };
}

export function normalizeUsage(
  terminalMessages: readonly TerminalUsageMessage[],
  provider: string,
): NormalizedTokenUsage {
  const terminals = terminalMessages.filter((message) => message?.type === "result");
  return provider === "openai" ? normalizeOpenAI(terminals) : normalizeAnthropic(terminals);
}

export function tokensOf(
  terminalMessages: readonly TerminalUsageMessage[],
  provider: string,
): number | undefined {
  return normalizeUsage(terminalMessages, provider).total;
}
