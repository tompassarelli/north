import { spawnSync } from "node:child_process";
import type { ProviderAvailability, ProviderId, ProviderPreference, RoutingDecision } from "./providers/types";

export function probeAnthropic(): ProviderAvailability {
  if (process.env.NORTH_DISABLE_ANTHROPIC === "1")
    return { provider: "anthropic", available: false, reason: "disabled" };
  return { provider: "anthropic", available: true, reason: "ready" };
}

export function probeOpenAI(): ProviderAvailability {
  if (process.env.NORTH_DISABLE_OPENAI === "1")
    return { provider: "openai", available: false, reason: "disabled" };
  const command = process.env.NORTH_CODEX_BIN ?? "codex";
  const result = spawnSync(command, ["--version"], { encoding: "utf8", timeout: 3000 });
  if (result.error || result.status !== 0)
    return {
      provider: "openai",
      available: false,
      reason: "command_missing",
      detail: result.error?.message ?? result.stderr,
    };
  return { provider: "openai", available: true, reason: "ready", detail: result.stdout.trim() };
}

export function selectProvider(requested?: ProviderPreference): RoutingDecision {
  const preference = requested ?? (process.env.AGENT_PROVIDER as ProviderPreference | undefined) ?? "auto";
  const availability = [probeAnthropic(), probeOpenAI()];
  if (preference !== "auto") {
    const state = availability.find((entry) => entry.provider === preference)!;
    if (!state.available)
      throw new Error(`provider ${preference} unavailable: ${state.reason}${state.detail ? ` (${state.detail})` : ""}`);
    return { requested: preference, provider: preference, reason: "explicit provider", availability };
  }
  const order = (process.env.NORTH_PROVIDER_ORDER ?? "anthropic,openai")
    .split(",")
    .map((entry) => entry.trim())
    .filter((entry): entry is ProviderId => entry === "anthropic" || entry === "openai");
  const chosen = order.find((id) => availability.find((entry) => entry.provider === id)?.available);
  if (!chosen)
    throw new Error(`no agent provider available: ${availability.map((entry) => `${entry.provider}=${entry.reason}`).join(", ")}`);
  return { requested: "auto", provider: chosen, reason: `first available in ${order.join(" -> ")}`, availability };
}
