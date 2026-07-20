import { resourcePolicyFromEnv, selectProviderForExecution } from "./provider-routing";
import type {
  ProviderPreference,
  ResourcePolicy,
  RoutingDecision,
  RoutingPreference,
} from "./providers/types";
import type { SemanticTier } from "./providers/catalog";
import type { Effort } from "./harness";
import { refreshAccountUsages } from "./account-usage";
import { refreshCodexEntitlementsIfStale } from "./codex-entitlement";
import { listProviderAccounts, type ProviderAccount } from "./accounts";

const PROVIDERS = new Set<ProviderPreference>(["auto", "anthropic", "openai"]);

export interface McpRoutePin {
  target: string;
  provider?: ProviderPreference;
  tier?: SemanticTier;
  reasoning?: Effort;
  model?: string;
}

export function validateConfiguredRoutePin(
  request: McpRoutePin,
  policy: ResourcePolicy = resourcePolicyFromEnv(),
): { target: string; provider: "anthropic" | "openai" } {
  const requestedProvider = request.provider ?? "auto";
  if (!PROVIDERS.has(requestedProvider)) throw new Error(`invalid provider: ${requestedProvider}`);
  const target = policy.targets?.find(({ id }) => id === request.target);
  if (!target) throw new Error(`routing target ${request.target} is not configured`);
  if (requestedProvider !== "auto" && target.provider !== requestedProvider)
    throw new Error(
      `routing target ${request.target} belongs to ${target.provider}, not requested provider ${requestedProvider}`,
    );
  return { target: target.id, provider: target.provider };
}

/** Fail an exact MCP pin before the background worker is acknowledged. */
export async function preflightMcpRoutePin(
  request: McpRoutePin,
  dependencies: {
    policy?: ResourcePolicy;
    select?: (requested: RoutingPreference, policy?: ResourcePolicy,
      context?: { tier?: SemanticTier; reasoning?: Effort; model?: string; stableKey?: string }) =>
        RoutingDecision | Promise<RoutingDecision>;
  } = {},
): Promise<RoutingDecision> {
  const policy = dependencies.policy ?? resourcePolicyFromEnv();
  validateConfiguredRoutePin(request, policy);
  // The execution selector owns the authoritative usage + exact-model refresh.
  // Its persisted observations are then cache hits when the admitted worker
  // resolves the same exact target, so cold/stale MCP launch performs one warm
  // provider Query rather than one usage probe plus a second model probe.
  const decision = await (dependencies.select ?? selectProviderForExecution)(
    { provider: request.provider ?? "auto", target: request.target },
    policy,
    { tier: request.tier, reasoning: request.reasoning, model: request.model,
      stableKey: `mcp-preflight:${request.target}` },
  );
  if (decision.target !== request.target || decision.fallbackTargets.length !== 0)
    throw new Error(`exact target preflight violated pin contract for ${request.target}`);
  return decision;
}

/** Refresh only the exact pin, sharing model evidence when the route names one. */
export async function refreshMcpRoutePinUsage(
  request: McpRoutePin,
  dependencies: {
    policy?: ResourcePolicy;
    accounts?: () => ProviderAccount[];
    refreshAccounts?: typeof refreshAccountUsages;
    refreshCodex?: typeof refreshCodexEntitlementsIfStale;
  } = {},
): Promise<void> {
  const policy = dependencies.policy ?? resourcePolicyFromEnv();
  const configured = validateConfiguredRoutePin(request, policy);
  let isolated: ProviderAccount | undefined;
  try {
    isolated = (dependencies.accounts ?? listProviderAccounts)()
      .find(({ id, provider }) => id === configured.target && provider === configured.provider);
  } catch {
    // Account-registry diagnostics are advisory at this boundary; canonical
    // selection below remains the authority on whether the target can run.
  }
  try {
    if (isolated) {
      await (dependencies.refreshAccounts ?? refreshAccountUsages)({
        accounts: [isolated],
        observeAnthropicModels: configured.provider === "anthropic" && request.model !== undefined,
      });
    } else if (configured.provider === "openai") {
      await (dependencies.refreshCodex ?? refreshCodexEntitlementsIfStale)({ requested: request });
    }
  } catch {
    // Failed usage collection persists unknown when it can, but must never
    // turn observability into an execution dependency.
  }
}

if (import.meta.main) {
  const [target, provider = "auto", rawTier, rawReasoning, rawModel] = process.argv.slice(2);
  if (!target) {
    console.error("usage: bun run mcp-route-preflight.ts <target> [provider] [tier|-] [reasoning|-] [model|-]");
    process.exit(2);
  }
  try {
    const request = {
      target,
      provider: provider as ProviderPreference,
      tier: (rawTier && rawTier !== "-") ? rawTier as SemanticTier : undefined,
      reasoning: (rawReasoning && rawReasoning !== "-") ? rawReasoning as Effort : undefined,
      model: (rawModel && rawModel !== "-") ? rawModel : undefined,
    };
    // Validation happens before the execution selector can probe. Its shared
    // model + usage evidence makes the worker's subsequent refresh a cache hit.
    const decision = await preflightMcpRoutePin(request);
    console.log(JSON.stringify({ target: decision.target, provider: decision.provider, reason: decision.selectionReason }));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(2);
  }
}
