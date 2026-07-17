import { query } from "@anthropic-ai/claude-agent-sdk";
import {
  ProviderRetrySafeError, type AgentProvider, type AgentQuery, type ProviderAvailability,
} from "./types";
import { probeAnthropic } from "../provider-routing";
import { observeAnthropicQuery } from "./anthropic-observations";
import { providerEnvironmentForTarget } from "../accounts";
import { resolve } from "node:path";
import { requireGafferCapabilities } from "../gaffer-capabilities";

// Selection already proved a CLI-owned first-party Claude.ai session, and the
// target environment strips API-key, cloud, and alternate-endpoint transports.
// Claude Code Agent SDK 0.3.195 reports `none` for that subscription flow even
// though its current ApiKeySource declaration omits the runtime value.
const SUBSCRIPTION_SAFE_API_KEY_SOURCES = new Set(["oauth", "none"]);

function normalizedAnthropicMessage(message: any): any {
  if (!message || typeof message !== "object") return message;
  if (message.type === "system" && message.subtype === "init"
      && !SUBSCRIPTION_SAFE_API_KEY_SOURCES.has(message.apiKeySource)) {
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
    if (args.options && "northCapabilities" in args.options) {
      const capabilities = requireGafferCapabilities(
        (args.options as any).northCapabilities, "northCapabilities",
      );
      const denied = new Set(args.options.disallowedTools ?? []);
      const requireDenied = (tools: string[], capability: string) => {
        if (tools.some((toolName) => !denied.has(toolName)))
          throw new ProviderRetrySafeError(
            `anthropic_adapter_did_not_enforce_absent_${capability}_capability`,
          );
      };
      if (!capabilities.includes("filesystem.write"))
        requireDenied(["Edit", "Write", "MultiEdit", "NotebookEdit"], "filesystem_write");
      if (!capabilities.includes("shell") && !capabilities.includes("shell.readonly"))
        requireDenied(["Bash"], "shell");
      if (!capabilities.includes("web"))
        requireDenied(["WebSearch", "WebFetch"], "web");
      if (!capabilities.includes("coordination"))
        requireDenied(["mcp__north__spawn", "mcp__north__dispatch", "mcp__north-peer__command_peer"], "coordination");
      if (capabilities.includes("shell.readonly")) {
        const sandbox = args.options.sandbox;
        const cwd = resolve(args.options.cwd ?? process.cwd());
        if (sandbox?.enabled !== true || sandbox.failIfUnavailable !== true
            || sandbox.allowUnsandboxedCommands !== false
            || !sandbox.filesystem?.denyWrite?.map((path) => resolve(path)).includes(cwd)) {
          throw new ProviderRetrySafeError("anthropic_readonly_sandbox_contract_missing");
        }
      }
    }
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
