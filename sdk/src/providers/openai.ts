import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import { ProviderRetrySafeError, type AgentProvider, type AgentQuery, type ProviderAvailability } from "./types";
import type { RoutingTarget } from "./types";
import { probeOpenAI } from "../provider-routing";
import type { AdapterUsageMetadata, TerminalTokenUsage, TokenTotalStatus } from "../usage";
import { codexConfigArguments, providerEnvironmentForTarget } from "../accounts";

function command(env: NodeJS.ProcessEnv): string { return env.NORTH_CODEX_BIN ?? "codex"; }

export function probeCodex(target?: RoutingTarget): ProviderAvailability {
  return probeOpenAI(target);
}

async function initialPrompt(value: string | AsyncIterable<any>): Promise<string> {
  if (typeof value === "string") return value;
  const it = value[Symbol.asyncIterator]();
  const first = await it.next();
  if (first.done) return "";
  const v = first.value;
  if (typeof v === "string") return v;
  if (v?.type === "user" && typeof v.message?.content === "string") return v.message.content;
  if (v?.type === "user" && Array.isArray(v.message?.content))
    return v.message.content.map((x: any) => x.text ?? "").join("\n");
  return String(v?.text ?? v?.content ?? v ?? "");
}

function modelForCodex(model?: string): string | undefined {
  // Anthropic aliases have no valid cross-provider meaning. An explicit OpenAI
  // model is honored; semantic/default aliases defer to the user's Codex config.
  if (!model || /^(sonnet|opus|haiku|fable|economy|standard|senior|frontier)/.test(model)) return undefined;
  return model;
}

function waitForClose(child: ChildProcessWithoutNullStreams): Promise<number | null> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(child.exitCode);
  return new Promise((resolve) => child.once("close", resolve));
}

function observedToken(value: unknown): number | undefined {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
}

function codexUsage(raw: any, terminalCount: number): {
  usage: TerminalTokenUsage;
  metadata: AdapterUsageMetadata;
} {
  const inputTokens = observedToken(raw?.input_tokens);
  const outputTokens = observedToken(raw?.output_tokens);
  const cachedInputTokens = observedToken(raw?.cached_input_tokens);
  const reasoningOutputTokens = observedToken(raw?.reasoning_output_tokens);
  const usage: TerminalTokenUsage = {
    ...(inputTokens !== undefined ? { input_tokens: inputTokens } : {}),
    ...(outputTokens !== undefined ? { output_tokens: outputTokens } : {}),
    ...(cachedInputTokens !== undefined ? { cached_input_tokens: cachedInputTokens } : {}),
    ...(reasoningOutputTokens !== undefined ? { reasoning_output_tokens: reasoningOutputTokens } : {}),
  };
  // Codex says cached_input_tokens is a subset of input_tokens and
  // reasoning_output_tokens is a subset of output_tokens. The adapter owns this
  // formula so the provider-neutral recorder can never add either subset twice.
  const totalStatus: TokenTotalStatus = terminalCount === 0
    ? "unknown_no_terminal"
    : inputTokens === undefined || outputTokens === undefined
      ? "unknown_incomplete_terminal"
      : "exact";
  return {
    usage,
    metadata: {
      provider: "openai",
      terminal_count: terminalCount,
      scope: "codex_fresh_invocation_thread_cumulative",
      total_status: totalStatus,
      ...(totalStatus === "exact" ? { total_tokens: inputTokens! + outputTokens! } : {}),
    },
  };
}

class CodexQuery implements AgentQuery {
  private child?: ChildProcessWithoutNullStreams;
  constructor(
    private prompt: string | AsyncIterable<any>,
    private options: any,
    private target?: RoutingTarget,
  ) {}

  supportsInFlightEscalation(): boolean { return false; }

  async interrupt(): Promise<void> {
    const child = this.child;
    if (!child || child.exitCode !== null || child.signalCode !== null) return;
    child.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => { if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL"); }, 1_000);
      child.once("close", () => { clearTimeout(timer); resolve(); });
    });
  }

  async *[Symbol.asyncIterator](): AsyncIterator<any> {
    const task = await initialPrompt(this.prompt);
    const prompt = this.options.systemPrompt
      ? `${this.options.systemPrompt}\n\n## Task\n${task}`
      : task;
    const env = providerEnvironmentForTarget("openai", this.target, { env: this.options.env });
    const args = ["exec", ...codexConfigArguments(env), "--json", "--color", "never", "--skip-git-repo-check"];
    const model = modelForCodex(this.options.model);
    if (model) args.push("--model", model);
    if (this.options.effort) args.push("--config", `model_reasoning_effort=${JSON.stringify(this.options.effort)}`);
    if (this.options.cwd) args.push("--cd", this.options.cwd);
    args.push("-");
    const child = spawn(command(env), args, { cwd: this.options.cwd ?? process.cwd(), env, stdio: ["pipe", "pipe", "pipe"] });
    this.child = child;
    const launched = new Promise<void>((resolve, reject) => {
      child.once("spawn", resolve);
      child.once("error", reject);
    });
    child.stdin.on("error", () => { /* child process error is classified below */ });
    try { await launched; }
    catch {
      this.child = undefined;
      throw new ProviderRetrySafeError("openai_provider_executable_unavailable_before_acceptance");
    }
    child.stdin.end(prompt);
    // Once the process has spawned, silence is not proof of non-acceptance: the
    // provider may have accepted work before the CLI emitted a recognized event.
    // Every subsequent failure therefore remains the original provider error.
    let result = "";
    let usage: any;
    let usageTerminalCount = 0;
    child.stderr.resume();
    try {
      for await (const line of createInterface({ input: child.stdout })) {
        if (!line.trim()) continue;
        let event: any;
        try { event = JSON.parse(line); } catch { continue; }
        if (event.type === "item.completed" && event.item?.type === "agent_message") {
          const text = event.item.text ?? "";
          result = text || result;
          if (text) yield { type: "assistant", message: { role: "assistant", content: [{ type: "text", text }] } };
        }
        if (event.type === "turn.completed") {
          usageTerminalCount++;
          usage = event.usage;
        }
        if (event.type === "error") throw new Error("openai_provider_execution_failed");
      }
      const code = await waitForClose(child);
      if (code !== 0) throw new Error("openai_provider_execution_failed");
    } catch (error) {
      try { await this.interrupt(); } catch { /* cleanup must not replace the provider error */ }
      throw error;
    } finally {
      this.child = undefined;
    }
    const normalizedUsage = codexUsage(usage, usageTerminalCount);
    yield {
      type: "result", subtype: "success", result,
      duration_ms: 0, num_turns: 1,
      usage: normalizedUsage.usage,
      _north_usage: normalizedUsage.metadata,
    };
  }
}

export const openaiProvider: AgentProvider = {
  id: "openai",
  probe: probeCodex,
  query: ({ prompt, options, target }) => new CodexQuery(prompt, options, target),
};
