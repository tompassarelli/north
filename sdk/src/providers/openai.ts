import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface } from "node:readline";
import type { AgentProvider, AgentQuery, ProviderAvailability } from "./types";
import { probeOpenAI } from "../provider-routing";

function command(): string { return process.env.NORTH_CODEX_BIN ?? "codex"; }

export function probeCodex(): ProviderAvailability {
  return probeOpenAI();
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

class CodexQuery implements AgentQuery {
  private child?: ChildProcessWithoutNullStreams;
  constructor(private prompt: string | AsyncIterable<any>, private options: any) {}

  async interrupt(): Promise<void> { this.child?.kill("SIGTERM"); }

  async *[Symbol.asyncIterator](): AsyncIterator<any> {
    const task = await initialPrompt(this.prompt);
    const prompt = this.options.systemPrompt
      ? `${this.options.systemPrompt}\n\n## Task\n${task}`
      : task;
    const args = ["exec", "--json", "--color", "never", "--skip-git-repo-check"];
    const model = modelForCodex(this.options.model);
    if (model) args.push("--model", model);
    if (this.options.effort) args.push("--config", `model_reasoning_effort=${JSON.stringify(this.options.effort)}`);
    if (this.options.cwd) args.push("--cd", this.options.cwd);
    args.push("-");
    const child = spawn(command(), args, { cwd: this.options.cwd ?? process.cwd(), env: process.env, stdio: ["pipe", "pipe", "pipe"] });
    this.child = child;
    child.stdin.end(prompt);
    let result = "";
    let usage: any = {};
    const stderr: string[] = [];
    child.stderr.on("data", (b) => stderr.push(String(b)));
    for await (const line of createInterface({ input: child.stdout })) {
      if (!line.trim()) continue;
      let event: any;
      try { event = JSON.parse(line); } catch { continue; }
      if (event.type === "item.completed" && event.item?.type === "agent_message") {
        const text = event.item.text ?? "";
        result = text || result;
        if (text) yield { type: "assistant", message: { role: "assistant", content: [{ type: "text", text }] } };
      }
      if (event.type === "turn.completed") usage = event.usage ?? usage;
      if (event.type === "error") throw new Error(event.message ?? JSON.stringify(event));
    }
    const code = await new Promise<number | null>((resolve) => child.once("close", resolve));
    if (code !== 0) throw new Error(`codex exec exited ${code}: ${stderr.join("").trim()}`);
    yield {
      type: "result", subtype: "success", result,
      duration_ms: 0, num_turns: 1,
      usage: { input_tokens: usage.input_tokens ?? 0, output_tokens: usage.output_tokens ?? 0 },
    };
  }
}

export const openaiProvider: AgentProvider = {
  id: "openai",
  probe: probeCodex,
  query: ({ prompt, options }) => new CodexQuery(prompt, options),
};
