export interface AgentProvider {}

export interface AgentProviderQuery {}

export type AllocationMode = "preferential" | "balanced" | "reserved";

export type EntitlementPressure = "plenty" | "normal" | "low" | "exhausted" | "unknown";

export interface ExecutionRoutingDecision {}

export type LiveInputCapability = "streaming" | "turn-messages" | "unsupported";

export interface ProviderAuthoritySurface {}

export interface ProviderFallbackTransition {}

export type ProviderId = "anthropic" | "openai";

export type ProviderPreference = "anthropic" | "openai" | "auto";

export type ProviderRuntimeReason = "proved_unsent_preaccept" | "retry_proof_missing" | "openai_provider_acceptance_ambiguous" | "openai_provider_failed_after_acceptance" | "exact_checkpoint_successor_granted" | "delivery_reservation_publication_unverified" | "provider_terminal_settled";

export interface ProviderRuntimeTelemetry {}

export interface ProviderUnsentProof {}

export interface ResourcePolicy {}

export interface RoutedQueryArguments {}

export interface RoutingDecision {}

export interface RoutingFallbackReason {}

export type RoutingPreference = ProviderPreference | RoutingRequest;

export interface RoutingRequest {}

export declare const ProviderEscalationUnsupportedError: Record<string, unknown>;

export declare const ProviderRefreshCancelledError: Record<string, unknown>;

export declare const ProviderRetrySafeError: Record<string, unknown>;

export declare const ProviderRuntimeError: Record<string, unknown>;

export declare const ProviderSelectionError: Record<string, unknown>;

export declare const accountAvailabilityBand: Record<string, unknown>;

export declare const accountAvailabilityRowIsUsable: Record<string, unknown>;

export declare const applyProviderUsageObservations: Record<string, unknown>;

export declare const automatedPressure: Record<string, unknown>;

export declare const compileProviderAuthoritySurface: Record<string, unknown>;

export declare const effectivePressure: Record<string, unknown>;

export declare const formatProviderAuthoritySurface: Record<string, unknown>;

export declare const isProvedUnsentPreacceptFailure: Record<string, unknown>;

export declare const loadProviderUsageObservations: Record<string, unknown>;

export declare const loadResourcePolicy: Record<string, unknown>;

export declare const mergeProviderUsageObservations: Record<string, unknown>;

export declare const normalizeAccountAvailability: Record<string, unknown>;

export declare const normalizeCodexRateLimits: Record<string, unknown>;

export declare const observeCodexEntitlement: Record<string, unknown>;

export declare const parseProviderUsageObservations: Record<string, unknown>;

export declare const parseResourcePolicy: Record<string, unknown>;

export declare const pressureFromUsageWindows: Record<string, unknown>;

export declare function providerLiveInput(arg0: string): string;

export declare const providerPreacceptError: Record<string, unknown>;

export declare const providerRetrySafeTerminalDetail: Record<string, unknown>;

export declare const providerRuntimeTelemetryValid: Record<string, unknown>;

export declare const readAccountAvailability: Record<string, unknown>;

export declare const readCodexEntitlementObservation: Record<string, unknown>;

export declare const refreshCodexEntitlementIfStale: Record<string, unknown>;

export declare const resourcePolicyFromEnv: Record<string, unknown>;

export declare function routedQuery(arg0: Record<string, unknown>, arg1: Record<string, unknown>): Record<string, unknown>;
export declare function routedQuery(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: string | null): Record<string, unknown>;
export declare function routedQuery(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: string | null, arg3: Record<string, unknown> | null): Record<string, unknown>;
export declare function routedQuery(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: string | null, arg3: Record<string, unknown> | null, arg4: Record<string, unknown> | null): Record<string, unknown>;
export declare function routedQuery(arg0: Record<string, unknown>, arg1: Record<string, unknown>, arg2: string | null, arg3: Record<string, unknown> | null, arg4: Record<string, unknown> | null, arg5: Record<string, unknown> | null): Record<string, unknown>;

export declare const selectProvider: Record<string, unknown>;

export declare const selectProviderForExecution: Record<string, unknown>;

export declare const selectProviderFromAvailability: Record<string, unknown>;

export declare const shouldRefreshCodexEntitlement: Record<string, unknown>;

export declare const writeProviderUsageObservations: Record<string, unknown>;
