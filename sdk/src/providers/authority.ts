import { ProviderRetrySafeError, type ProviderId } from "./types";
import type { LiveInputCapability } from "./types";
import type { OrchestrationCapability } from "../orchestration-capabilities";
import { graphTextExperimentCapabilities } from "../orchestration-capabilities";
import { orchestrationCapabilities } from "../orchestration-staffing";
import { admitPinnedProvider } from "../execution-admission";
import { admitRoutingRequest } from "../routing-admission";
import {
  COORDINATION_TOOLS, ORCHESTRATION_TOOLS, hasCanonicalHarnessAuthority, managedToolPolicy,
} from "../harness";
import { FRAM_GRAPH_AUTHORING_CAPABILITY, FRAM_MCP_TOOLS } from "../fram-graph-authoring";
import { managedCodexNetworkPolicy } from "./codex-network-policy";

function bareNorthTool(toolName: string): string | undefined {
  const prefix = "mcp__north__";
  return toolName.startsWith(prefix) ? toolName.slice(prefix.length) : undefined;
}

export const CODEX_WORKER_NORTH_ENABLED_TOOLS = Object.freeze(
  COORDINATION_TOOLS.map(bareNorthTool).filter((name): name is string => Boolean(name)),
);
const CODEX_ORCHESTRATOR_NORTH_ENABLED_TOOLS = Object.freeze([
  ...CODEX_WORKER_NORTH_ENABLED_TOOLS,
  ...ORCHESTRATION_TOOLS.map(bareNorthTool).filter((name): name is string => Boolean(name)),
]);

interface AuthoritySurfaceBase {
  provider: ProviderId;
  capabilities: readonly OrchestrationCapability[];
  nativeMultiAgent: "disabled";
  liveInput: LiveInputCapability;
  northEnabledTools: readonly string[];
  authoringHooks: "harness-exact" | "managed-only";
}

export interface OpenAIAuthoritySurface extends AuthoritySurfaceBase {
  provider: "openai";
  sandbox: "read-only" | "workspace-write";
  web: "cached" | "disabled";
  /**
   * Every MCP tool the managed Codex session may call, fully qualified — the
   * same shape the Anthropic surface logs as `managedTools`. Codex enforces
   * per-server `enabled_tools`, so this is the union of the North grant and
   * (only under graph-authoring.fram) the sealed fram graph-edit verbs.
   */
  managedTools: readonly string[];
}

export interface AnthropicAuthoritySurface extends AuthoritySurfaceBase {
  provider: "anthropic";
  builtins: readonly string[];
  managedTools: readonly string[];
  web: "enabled" | "disabled";
}

export type ProviderAuthoritySurface = OpenAIAuthoritySurface | AnthropicAuthoritySurface;

/** Compile the exact provider authority from one sealed, admitted Orchestration request. */
export function compileProviderAuthoritySurface(
  provider: ProviderId,
  options: any,
): ProviderAuthoritySurface {
  if (!hasCanonicalHarnessAuthority(options, provider))
    throw new ProviderRetrySafeError(`${provider}_harness_authority_seal_missing`);
  const request = admitRoutingRequest(
    options.northRoutingRequest, `${provider} authority compiler`,
  );
  const expectedCapabilities = graphTextExperimentCapabilities(
    orchestrationCapabilities(request), options.northGraphTextExperiment,
  );
  const suppliedCapabilities = options.northCapabilities as readonly OrchestrationCapability[];
  if (JSON.stringify(expectedCapabilities) !== JSON.stringify(suppliedCapabilities))
    throw new ProviderRetrySafeError(`${provider}_graph_text_experiment_authority_mismatch`);
  const capabilities = Object.freeze([...suppliedCapabilities]);
  // A surface is evidence of executable authority, not a requested wish list.
  // Reject provider-inexpressible shapes before any caller can log or persist
  // a fictitious "effective" boundary.
  admitPinnedProvider(provider, capabilities);
  const nativeMultiAgent = "disabled" as const;
  if (provider === "openai") {
    const northEnabledTools = capabilities.includes("coordination")
      ? CODEX_ORCHESTRATOR_NORTH_ENABLED_TOOLS
      : CODEX_WORKER_NORTH_ENABLED_TOOLS;
    return Object.freeze({
      provider,
      capabilities,
      nativeMultiAgent,
      liveInput: "turn-framed",
      authoringHooks: "managed-only",
      sandbox: capabilities.includes("shell.readonly") ? "read-only" : "workspace-write",
      web: capabilities.includes("web") ? "cached" : "disabled",
      northEnabledTools,
      managedTools: Object.freeze([
        ...northEnabledTools.map((name) => `mcp__north__${name}`),
        ...(capabilities.includes(FRAM_GRAPH_AUTHORING_CAPABILITY) ? FRAM_MCP_TOOLS : []),
      ]),
    });
  }
  const policy = managedToolPolicy(capabilities);
  const managedTools = policy.allowedTools
    .filter((toolName) => toolName.startsWith("mcp__"));
  const northEnabledTools = managedTools
    .filter((toolName) => toolName.startsWith("mcp__north__"))
    .map((toolName) => toolName.slice("mcp__north__".length));
  return Object.freeze({
    provider,
    capabilities,
    nativeMultiAgent,
    liveInput: "streaming",
    authoringHooks: "harness-exact",
    builtins: Object.freeze(policy.tools),
    managedTools: Object.freeze(managedTools),
    northEnabledTools: Object.freeze(northEnabledTools),
    web: capabilities.includes("web") ? "enabled" : "disabled",
  });
}

export function formatProviderAuthoritySurface(surface: ProviderAuthoritySurface): string {
  const list = (values: readonly string[]) => values.length ? values.join(",") : "(none)";
  const base = `provider=${surface.provider}; capabilities=${list(surface.capabilities)}; `
    + `native-multi-agent=${surface.nativeMultiAgent}; `
    + `live-input=${surface.liveInput}; `
    + `authoring-hooks=${surface.authoringHooks}; `
    + `north enabled_tools=${list(surface.northEnabledTools)}`;
  return surface.provider === "openai"
    ? `${base}; sandbox=${surface.sandbox}; web=${surface.web}; `
      + `network=${managedCodexNetworkPolicy(surface).networkAccess ? "enabled" : "disabled"}; `
      + `mcp tools=${list(surface.managedTools)}`
    : `${base}; web=${surface.web}; sdk builtins=${list(surface.builtins)}; `
      + `mcp tools=${list(surface.managedTools)}`;
}

export function logProviderAuthoritySurface(
  operation: "spawn" | "dispatch",
  provider: ProviderId,
  options: any,
): ProviderAuthoritySurface {
  const surface = compileProviderAuthoritySurface(provider, options);
  console.log(`[${operation}] effective authority: ${formatProviderAuthoritySurface(surface)}`);
  return surface;
}
