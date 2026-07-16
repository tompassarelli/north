import { query } from "@anthropic-ai/claude-agent-sdk";
import type { AgentProvider, AgentQuery, ProviderAvailability } from "./types";
import { probeAnthropic } from "../provider-routing";
import { observeAnthropicQuery } from "./anthropic-observations";
import { providerEnvironmentForTarget } from "../accounts";

function normalizedAnthropicMessage(message: any): any {
  if (!message || typeof message !== "object") return message;
  if (message.type === "system" && message.subtype === "init" && message.apiKeySource !== "oauth") {
    throw new Error("anthropic_subscription_authentication_required");
  }
  if (message.type === "result" && message.subtype !== "success") {
    return { ...message, errors: ["anthropic_provider_execution_failed"] };
  }
  if (message.type === "assistant" && message.error && message.message && typeof message.message === "object") {
    return { ...message, message: { ...message.message, content: [] } };
  }
  if (message.type === "auth_status") {
    return {
      ...message,
      output: [],
      ...(message.error === undefined ? {} : { error: "anthropic_provider_authentication_failed" }),
    };
  }
  if (message.type === "system" && message.subtype === "mirror_error") {
    return { ...message, error: "anthropic_provider_execution_failed" };
  }
  if (message.type === "system" && message.subtype === "status" && message.compact_error !== undefined) {
    return { ...message, compact_error: "anthropic_provider_execution_failed" };
  }
  return message;
}

export function normalizeAnthropicQueryDiagnostics(source: AgentQuery): AgentQuery {
  const failed = () => new Error("anthropic_provider_execution_failed");
  return {
    interrupt: source.interrupt && (async () => {
      try { await source.interrupt!(); } catch { throw failed(); }
    }),
    setModel: source.setModel && (async (model) => {
      try { await source.setModel!(model); } catch { throw failed(); }
    }),
    applyFlagSettings: source.applyFlagSettings && (async (settings) => {
      try { await source.applyFlagSettings!(settings); } catch { throw failed(); }
    }),
    supportsInFlightEscalation: () => {
      try {
        return Boolean(source.setModel && source.applyFlagSettings && (source.supportsInFlightEscalation?.() ?? true));
      }
      catch { throw failed(); }
    },
    async *[Symbol.asyncIterator]() {
      try {
        for await (const message of source as AsyncIterable<any>) yield normalizedAnthropicMessage(message);
      } catch {
        throw failed();
      }
    },
  };
}

export const anthropicProvider: AgentProvider = {
  id: "anthropic",
  probe(target): ProviderAvailability {
    return probeAnthropic(target);
  },
  query(args) {
    const options = {
      ...args.options,
      env: providerEnvironmentForTarget("anthropic", args.target, { env: args.options.env }),
    };
    try {
      // The SDK exposes no typed proof that a failed request was never accepted,
      // so diagnostics are redacted without manufacturing retry safety.
      return observeAnthropicQuery(normalizeAnthropicQueryDiagnostics(query({ prompt: args.prompt, options })), {
        targetId: () => args.target?.id ?? "anthropic",
      });
    } catch {
      throw new Error("anthropic_provider_execution_failed");
    }
  },
};
