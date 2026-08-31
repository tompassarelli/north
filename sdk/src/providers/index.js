import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, js_obj as $$bh$js_obj } from '../bridge/generated/beagle/host.js';

const anthropic_module = require("./anthropic");

const openai_module = require("./openai");

const types_module = require("./types");

const router_module = require("./internal-router");

const authority_module = require("./authority");

const provider_routing_module = require("../provider-routing");

const cancellation_module = require("../provider-cancellation");

const resource_policy_module = require("../resource-policy");

const observation_store_module = require("../provider-observation-store");

const availability_module = require("../account-availability");

const codex_entitlement_module = require("../codex-entitlement");

const anthropic_provider = anthropic_module.anthropicProvider;

const openai_provider = openai_module.openaiProvider;

const providers = Object.freeze($$bh$js_obj("anthropic", anthropic_provider, "openai", openai_provider));

function providerLiveInput(id) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(providers, id).liveInput;
}

const routed_query_with_registry = router_module.routedQueryWithRegistry;

function routed_query(decision, arguments$, route_class, before_fallback, on_route, on_route_attempt) {
  return routed_query_with_registry(decision, arguments$, route_class, providers, before_fallback, on_route, on_route_attempt);
}

function routedQuery(...$beagle$args) {
  if (arguments.length === 2) {
    const decision = $beagle$args[0];
    const arguments$ = $beagle$args[1];
    return routed_query(decision, arguments$, null, null, null, null);
  }
  if (arguments.length === 3) {
    const decision = $beagle$args[0];
    const arguments$ = $beagle$args[1];
    const route_class = $beagle$args[2];
    return routed_query(decision, arguments$, route_class, null, null, null);
  }
  if (arguments.length === 4) {
    const decision = $beagle$args[0];
    const arguments$ = $beagle$args[1];
    const route_class = $beagle$args[2];
    const before_fallback = $beagle$args[3];
    return routed_query(decision, arguments$, route_class, before_fallback, null, null);
  }
  if (arguments.length === 5) {
    const decision = $beagle$args[0];
    const arguments$ = $beagle$args[1];
    const route_class = $beagle$args[2];
    const before_fallback = $beagle$args[3];
    const on_route = $beagle$args[4];
    return routed_query(decision, arguments$, route_class, before_fallback, on_route, null);
  }
  if (arguments.length === 6) {
    const decision = $beagle$args[0];
    const arguments$ = $beagle$args[1];
    const route_class = $beagle$args[2];
    const before_fallback = $beagle$args[3];
    const on_route = $beagle$args[4];
    const on_route_attempt = $beagle$args[5];
    return routed_query(decision, arguments$, route_class, before_fallback, on_route, on_route_attempt);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const ProviderSelectionError = provider_routing_module.ProviderSelectionError;

const resourcePolicyFromEnv = provider_routing_module.resourcePolicyFromEnv;

const selectProvider = provider_routing_module.selectProvider;

const selectProviderForExecution = provider_routing_module.selectProviderForExecution;

const selectProviderFromAvailability = provider_routing_module.selectProviderFromAvailability;

const ProviderRefreshCancelledError = cancellation_module.ProviderRefreshCancelledError;

const applyProviderUsageObservations = resource_policy_module.applyProviderUsageObservations;

const automatedPressure = resource_policy_module.automatedPressure;

const effectivePressure = resource_policy_module.effectivePressure;

const loadProviderUsageObservations = resource_policy_module.loadProviderUsageObservations;

const loadResourcePolicy = resource_policy_module.loadResourcePolicy;

const parseProviderUsageObservations = resource_policy_module.parseProviderUsageObservations;

const parseResourcePolicy = resource_policy_module.parseResourcePolicy;

const pressureFromUsageWindows = resource_policy_module.pressureFromUsageWindows;

const mergeProviderUsageObservations = observation_store_module.mergeProviderUsageObservations;

const writeProviderUsageObservations = observation_store_module.writeProviderUsageObservations;

const accountAvailabilityBand = availability_module.accountAvailabilityBand;

const accountAvailabilityRowIsUsable = availability_module.accountAvailabilityRowIsUsable;

const normalizeAccountAvailability = availability_module.normalizeAccountAvailability;

const readAccountAvailability = availability_module.readAccountAvailability;

const normalizeCodexRateLimits = codex_entitlement_module.normalizeCodexRateLimits;

const observeCodexEntitlement = codex_entitlement_module.observeCodexEntitlement;

const readCodexEntitlementObservation = codex_entitlement_module.readCodexEntitlementObservation;

const refreshCodexEntitlementIfStale = codex_entitlement_module.refreshCodexEntitlementIfStale;

const shouldRefreshCodexEntitlement = codex_entitlement_module.shouldRefreshCodexEntitlement;

const isProvedUnsentPreacceptFailure = types_module.isProvedUnsentPreacceptFailure;

const providerPreacceptError = types_module.providerPreacceptError;

const providerRuntimeTelemetryValid = types_module.providerRuntimeTelemetryValid;

const providerRetrySafeTerminalDetail = types_module.providerRetrySafeTerminalDetail;

const ProviderEscalationUnsupportedError = types_module.ProviderEscalationUnsupportedError;

const ProviderRetrySafeError = types_module.ProviderRetrySafeError;

const ProviderRuntimeError = types_module.ProviderRuntimeError;

const compileProviderAuthoritySurface = authority_module.compileProviderAuthoritySurface;

const formatProviderAuthoritySurface = authority_module.formatProviderAuthoritySurface;

export { ProviderEscalationUnsupportedError as "ProviderEscalationUnsupportedError" };
export { ProviderRefreshCancelledError as "ProviderRefreshCancelledError" };
export { ProviderRetrySafeError as "ProviderRetrySafeError" };
export { ProviderRuntimeError as "ProviderRuntimeError" };
export { ProviderSelectionError as "ProviderSelectionError" };
export { accountAvailabilityBand as "accountAvailabilityBand" };
export { accountAvailabilityRowIsUsable as "accountAvailabilityRowIsUsable" };
export { applyProviderUsageObservations as "applyProviderUsageObservations" };
export { automatedPressure as "automatedPressure" };
export { compileProviderAuthoritySurface as "compileProviderAuthoritySurface" };
export { effectivePressure as "effectivePressure" };
export { formatProviderAuthoritySurface as "formatProviderAuthoritySurface" };
export { isProvedUnsentPreacceptFailure as "isProvedUnsentPreacceptFailure" };
export { loadProviderUsageObservations as "loadProviderUsageObservations" };
export { loadResourcePolicy as "loadResourcePolicy" };
export { mergeProviderUsageObservations as "mergeProviderUsageObservations" };
export { normalizeAccountAvailability as "normalizeAccountAvailability" };
export { normalizeCodexRateLimits as "normalizeCodexRateLimits" };
export { observeCodexEntitlement as "observeCodexEntitlement" };
export { parseProviderUsageObservations as "parseProviderUsageObservations" };
export { parseResourcePolicy as "parseResourcePolicy" };
export { pressureFromUsageWindows as "pressureFromUsageWindows" };
export { providerLiveInput as "providerLiveInput" };
export { providerPreacceptError as "providerPreacceptError" };
export { providerRetrySafeTerminalDetail as "providerRetrySafeTerminalDetail" };
export { providerRuntimeTelemetryValid as "providerRuntimeTelemetryValid" };
export { readAccountAvailability as "readAccountAvailability" };
export { readCodexEntitlementObservation as "readCodexEntitlementObservation" };
export { refreshCodexEntitlementIfStale as "refreshCodexEntitlementIfStale" };
export { resourcePolicyFromEnv as "resourcePolicyFromEnv" };
export { routedQuery as "routedQuery" };
export { selectProvider as "selectProvider" };
export { selectProviderForExecution as "selectProviderForExecution" };
export { selectProviderFromAvailability as "selectProviderFromAvailability" };
export { shouldRefreshCodexEntitlement as "shouldRefreshCodexEntitlement" };
export { writeProviderUsageObservations as "writeProviderUsageObservations" };
