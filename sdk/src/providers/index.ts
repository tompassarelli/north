import { anthropicProvider } from "./anthropic";
import { openaiProvider } from "./openai";
import {
  ProviderEscalationUnsupportedError, ProviderRetrySafeError,
  type AgentProvider, type ProviderId, type ProviderPreference, type RoutingDecision,
} from "./types";
import type { AgentQuery } from "./types";
import type { Options } from "@anthropic-ai/claude-agent-sdk";
import { resolveTier, type SemanticTier } from "./catalog";
export {
  ProviderSelectionError, resourcePolicyFromEnv, selectProvider, selectProviderFromAvailability,
} from "../provider-routing";
export {
  applyProviderUsageObservations, automatedPressure, effectivePressure, loadProviderUsageObservations,
  loadResourcePolicy, parseProviderUsageObservations, parseResourcePolicy,
  pressureFromUsageWindows,
} from "../resource-policy";
export { mergeProviderUsageObservations, writeProviderUsageObservations } from "../provider-observation-store";
export {
  normalizeCodexRateLimits, observeCodexEntitlement, readCodexEntitlementObservation,
  refreshCodexEntitlementIfStale, shouldRefreshCodexEntitlement,
} from "../codex-entitlement";

const providers: Record<ProviderId, AgentProvider> = {
  anthropic: anthropicProvider,
  openai: openaiProvider,
};

export function providerFor(id: ProviderId): AgentProvider { return providers[id]; }

function replayablePrompt(prompt: string | AsyncIterable<any>): string | AsyncIterable<any> {
  if (typeof prompt === "string") return prompt;
  const source = prompt[Symbol.asyncIterator]();
  const cache: any[] = [];
  let done = false;
  let pending: Promise<IteratorResult<any>> | undefined;
  const readNext = async (): Promise<IteratorResult<any>> => {
    if (done) return { done: true, value: undefined };
    pending ??= source.next().finally(() => { pending = undefined; });
    const item = await pending;
    if (item.done) done = true;
    else cache.push(item.value);
    return item;
  };
  return {
    async *[Symbol.asyncIterator]() {
      let index = 0;
      while (true) {
        if (index < cache.length) { yield cache[index++]; continue; }
        const item = await readNext();
        if (item.done) return;
        index++;
        yield item.value;
      }
    },
  };
}

// Automatic fallback is intentionally proof-carrying and pre-side-effect only:
// the adapter must raise ProviderRetrySafeError from a typed condition that
// proves the request was never accepted. No event count or error prose can
// establish that proof. The production Claude adapter currently forwards SDK
// errors unchanged because that SDK exposes no such typed signal.
export function routedQuery(
  decision: RoutingDecision,
  args: { prompt: string | AsyncIterable<any>; options: Options },
  tier?: SemanticTier,
  providerRegistry: Record<ProviderId, AgentProvider> = providers,
  beforeFallback?: () => Promise<void>,
  onRoute?: (decision: RoutingDecision) => void,
): AgentQuery {
  let active: AgentQuery | undefined;
  const prompt = replayablePrompt(args.prompt);
  const optionsFor = (provider: ProviderId): Options => {
    const resolved = provider === decision.fallbackPath[0] ? undefined : resolveTier(provider, tier);
    const options = resolved
      ? { ...args.options, model: resolved.model, effort: resolved.effort }
      : args.options;
    decision.resolvedModel = options.model;
    decision.resolvedEffort = options.effort;
    return options;
  };
  return {
    interrupt: async () => { await active?.interrupt?.(); },
    supportsInFlightEscalation: () => Boolean(
      active?.setModel && active?.applyFlagSettings &&
      (active.supportsInFlightEscalation?.() ?? true),
    ),
    setModel: async (model: string) => {
      if (!active?.setModel) throw new ProviderEscalationUnsupportedError(
        `provider ${decision.provider} does not support in-flight model escalation`,
      );
      await active.setModel(model);
      decision.resolvedModel = model;
    },
    applyFlagSettings: async (settings) => {
      if (!active?.applyFlagSettings) throw new ProviderEscalationUnsupportedError(
        `provider ${decision.provider} does not support in-flight effort escalation`,
      );
      await active.applyFlagSettings(settings);
      if (settings.effortLevel !== undefined && settings.effortLevel !== null) {
        decision.resolvedEffort = settings.effortLevel;
      }
    },
    async *[Symbol.asyncIterator]() {
      let emitted = 0;
      while (true) {
        try {
          const options = optionsFor(decision.provider);
          onRoute?.(decision);
          active = providerRegistry[decision.provider].query({
            prompt,
            options,
            target: decision.routingTargets[decision.target],
          });
          for await (const event of active as AsyncIterable<any>) { emitted++; yield event; }
          return;
        } catch (err: any) {
          const fallbackTarget = decision.fallbackTargets[0];
          const fallbackProvider = decision.fallbackProviders[0];
          if (decision.requestedTarget === undefined && emitted === 0 && fallbackTarget && fallbackProvider
              && err instanceof ProviderRetrySafeError) {
            await beforeFallback?.();
            decision.fallbackTargets.shift();
            decision.fallbackProviders.shift();
            const previousTarget = decision.target;
            const previousProvider = decision.provider;
            decision.target = fallbackTarget;
            decision.provider = fallbackProvider;
            decision.entitlementPressure = decision.targetEntitlementPressures[fallbackTarget] ?? "unknown";
            decision.fallbackCount++;
            decision.fallbackTargetPath.push(fallbackTarget);
            decision.fallbackPath.push(fallbackProvider);
            decision.fallbackReasons.push(Object.freeze({
              sequence: decision.fallbackCount,
              reason: "provider_retry_safe_before_acceptance",
              fromTarget: previousTarget,
              fromProvider: previousProvider,
              toTarget: fallbackTarget,
              toProvider: fallbackProvider,
            }));
            continue;
          }
          throw err;
        }
      }
    },
  };
}
export { ProviderEscalationUnsupportedError, ProviderRetrySafeError } from "./types";
export type {
  AgentProvider, AllocationMode, EntitlementPressure, ProviderId, ProviderPreference,
  ResourcePolicy, RoutingDecision, RoutingFallbackReason, RoutingPreference, RoutingRequest,
} from "./types";
