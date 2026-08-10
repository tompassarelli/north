import { anthropicProvider } from "./anthropic";
import { openaiProvider } from "./openai";
import type {
  AgentProvider, ProviderFallbackTransition, ProviderId, RoutingDecision,
} from "./types";
import type { SemanticTier } from "./catalog";
import type { HarnessCompositionEvidence } from "../harness";
import type { ProviderAuthoritySurface } from "./authority";
import type { WireQuery } from "../wire/query";
import {
  routedQueryWithRegistry, type RoutedQueryArguments,
} from "./internal-router";
export {
  ProviderSelectionError, resourcePolicyFromEnv, selectProvider, selectProviderForExecution,
  selectProviderFromAvailability,
} from "../provider-routing";
export { ProviderRefreshCancelledError } from "../provider-cancellation";
export {
  applyProviderUsageObservations, automatedPressure, effectivePressure, loadProviderUsageObservations,
  loadResourcePolicy, parseProviderUsageObservations, parseResourcePolicy,
  pressureFromUsageWindows,
} from "../resource-policy";
export { mergeProviderUsageObservations, writeProviderUsageObservations } from "../provider-observation-store";
export {
  accountAvailabilityBand,
  accountAvailabilityRowIsUsable,
  normalizeAccountAvailability,
  readAccountAvailability,
} from "../account-availability";
export type {
  AccountAvailabilityBand,
  AccountAvailabilityRow,
  AccountAvailabilityRung,
  AccountAvailabilityThresholds,
  AccountAvailabilityWindowRung,
  NormalizeAccountAvailabilityOptions,
  ReadAccountAvailabilityOptions,
} from "../account-availability";
export {
  normalizeCodexRateLimits, observeCodexEntitlement, readCodexEntitlementObservation,
  refreshCodexEntitlementIfStale, shouldRefreshCodexEntitlement,
} from "../codex-entitlement";

const providers: Readonly<Record<ProviderId, AgentProvider>> = Object.freeze({
  anthropic: anthropicProvider,
  openai: openaiProvider,
});

/** Provider metadata only. The executable adapter registry remains private. */
export function providerLiveInput(id: ProviderId): AgentProvider["liveInput"] {
  return providers[id].liveInput;
}

// Automatic fallback is intentionally proof-carrying and pre-side-effect only:
// the adapter must raise ProviderRetrySafeError from a typed condition that
// proves the request was never accepted. No event count or error prose can
// establish that proof. The production Claude adapter currently forwards SDK
// errors unchanged because that SDK exposes no such typed signal.
export function routedQuery(
  decision: RoutingDecision,
  args: RoutedQueryArguments,
  tier?: SemanticTier,
  beforeFallback?: (
    transition: ProviderFallbackTransition,
  ) => Promise<void>,
  onRoute?: (
    decision: RoutingDecision,
    evidence: HarnessCompositionEvidence | undefined,
    authority: ProviderAuthoritySurface | undefined,
  ) => Promise<void> | void,
  onRouteAttempt?: (decision: RoutingDecision) => void,
): WireQuery {
  return routedQueryWithRegistry(
    decision,
    args,
    tier,
    providers,
    beforeFallback,
    onRoute,
    onRouteAttempt,
  );
}
export {
  isProvedUnsentPreacceptFailure, providerPreacceptError, providerRuntimeTelemetryValid,
  providerRetrySafeTerminalDetail,
  ProviderEscalationUnsupportedError, ProviderRetrySafeError, ProviderRuntimeError,
} from "./types";
export {
  compileProviderAuthoritySurface, formatProviderAuthoritySurface,
} from "./authority";
export type {
  AgentProvider, AgentProviderQuery, AllocationMode, EntitlementPressure, LiveInputCapability, ProviderId, ProviderPreference,
  ProviderFallbackTransition, ResourcePolicy, RoutingDecision, RoutingFallbackReason,
  RoutingPreference, RoutingRequest, ProviderRuntimeReason, ProviderRuntimeTelemetry, ProviderUnsentProof,
} from "./types";
export type { RoutedQueryArguments } from "./internal-router";
export type { ProviderAuthoritySurface } from "./authority";
