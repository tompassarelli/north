export interface BalancedAllocationEstimate {
  target: string;
  provider: ProviderId;
  eligible: boolean;
  pressure: EntitlementPressure;
  effectiveWeight: number;
  approximateShare: number;
  allocationEvidence: Record<string, unknown>;
}

export interface CachedTargetRouting {
  target: Record<string, unknown>;
  eligible: boolean;
  headroom: EntitlementPressure;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export type EntitlementPressure = "plenty" | "normal" | "low" | "exhausted" | "unknown";

export interface ExecutionRoutingDecision {}

export interface ProviderAvailability {
  targetId?: string;
  provider: ProviderId;
  installed: boolean;
  authenticated: boolean;
  available: boolean;
  reason: string;
  detail?: string;
}

export type ProviderId = "anthropic" | "openai";

export interface ProviderModelSelectionEvidence {}

export type ProviderPreference = "anthropic" | "openai" | "auto";

export type ProviderSelectionFailure = "provider_unavailable" | "entitlement_exhausted" | "route_unresolvable" | "blocked_preflight" | "no_provider_available";

export type RefreshRoutingFn = (arg0: RoutingPreference | null) => Promise<Record<string, unknown>>;

export interface ResourcePolicy {}

export interface RouteAxes {
  capabilityFloor: string;
  serviceClass: string;
  reasoning: string;
}

export interface RoutingDecision {}

export type RoutingPreference = ProviderPreference | RoutingRequest;

export interface RoutingRequest {
  provider?: ProviderPreference;
  target?: string;
}

export interface RoutingTarget {
  id: string;
  provider: ProviderId;
  authMode: string;
  profile?: string;
}

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export declare const BOOT_ROUTING_TIMEOUT_MS: number;

export declare const ProviderSelectionError: {
  new(arg0: string, arg1: string): Record<string, unknown>;
};

export declare function balancedAllocationEstimates(arg0: Array<Record<string, unknown>>, arg1: Record<string, unknown>): Array<Record<string, unknown>>;
export declare function balancedAllocationEstimates(arg0: Array<Record<string, unknown>>, arg1: Record<string, unknown>, arg2: string, arg3: string, arg4: string, arg5: string | null, arg6: Array<string> | null): Array<Record<string, unknown>>;

export declare function cachedAvailability(arg0: Record<string, unknown>): Record<string, unknown> | null;
export declare function cachedAvailability(arg0: Record<string, unknown>, arg1: number): Record<string, unknown> | null;
export declare function cachedAvailability(arg0: Record<string, unknown>, arg1: number, arg2: string): Record<string, unknown> | null;

export declare function cachedTargetRouting(): Array<Record<string, unknown>>;
export declare function cachedTargetRouting(arg0: Record<string, unknown>): Array<Record<string, unknown>>;
export declare function cachedTargetRouting(arg0: Record<string, unknown>, arg1: number): Array<Record<string, unknown>>;

export declare function collectExecutionModelRefreshAttempts(arg0: Array<Record<string, unknown>>, arg1: Array<Record<string, unknown>>, arg2: RoutingPreference): Promise<Array<Record<string, unknown>>>;
export declare function collectExecutionModelRefreshAttempts(arg0: Array<Record<string, unknown>>, arg1: Array<Record<string, unknown>>, arg2: RoutingPreference, arg3: Record<string, unknown>): Promise<Array<Record<string, unknown>>>;
export declare function collectExecutionModelRefreshAttempts(arg0: Array<Record<string, unknown>>, arg1: Array<Record<string, unknown>>, arg2: RoutingPreference, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null): Promise<Array<Record<string, unknown>>>;

export declare function configuredDefaultTarget(arg0: string): Record<string, unknown> | null;
export declare function configuredDefaultTarget(arg0: string, arg1: Record<string, unknown>): Record<string, unknown> | null;

export declare function probeAnthropic(): Record<string, unknown>;
export declare function probeAnthropic(arg0: Record<string, unknown>): Record<string, unknown>;

export declare function probeOpenAI(): Record<string, unknown>;
export declare function probeOpenAI(arg0: Record<string, unknown>): Record<string, unknown>;

export declare function refreshProviderRoutingInBackground(arg0: RoutingPreference | null): Promise<null>;
export declare function refreshProviderRoutingInBackground(arg0: RoutingPreference | null, arg1: (arg0: RoutingPreference | null) => Promise<Record<string, unknown>>): Promise<null>;

export declare function resourcePolicyFromEnv(): Record<string, unknown>;
export declare function resourcePolicyFromEnv(arg0: Record<string, unknown> | null): Record<string, unknown>;
export declare function resourcePolicyFromEnv(arg0: Record<string, unknown> | null, arg1: Record<string, unknown> | null): Record<string, unknown>;

export declare function selectProvider(): Record<string, unknown>;
export declare function selectProvider(arg0: RoutingPreference | null): Record<string, unknown>;
export declare function selectProvider(arg0: RoutingPreference | null, arg1: Record<string, unknown>): Record<string, unknown>;
export declare function selectProvider(arg0: RoutingPreference | null, arg1: Record<string, unknown>, arg2: Record<string, unknown>): Record<string, unknown>;
export declare function selectProvider(arg0: RoutingPreference | null, arg1: Record<string, unknown>, arg2: Record<string, unknown>, arg3: Record<string, unknown>): Record<string, unknown>;

export declare function selectProviderForExecution(): Promise<Record<string, unknown>>;
export declare function selectProviderForExecution(arg0: RoutingPreference | null): Promise<Record<string, unknown>>;
export declare function selectProviderForExecution(arg0: RoutingPreference | null, arg1: Record<string, unknown>): Promise<Record<string, unknown>>;
export declare function selectProviderForExecution(arg0: RoutingPreference | null, arg1: Record<string, unknown>, arg2: Record<string, unknown>): Promise<Record<string, unknown>>;
export declare function selectProviderForExecution(arg0: RoutingPreference | null, arg1: Record<string, unknown>, arg2: Record<string, unknown>, arg3: Record<string, unknown>): Promise<Record<string, unknown>>;

export declare function selectProviderFromAvailability(arg0: RoutingPreference, arg1: Array<Record<string, unknown>>, arg2: Record<string, unknown>): Record<string, unknown>;
export declare function selectProviderFromAvailability(arg0: RoutingPreference, arg1: Array<Record<string, unknown>>, arg2: Record<string, unknown>, arg3: string, arg4: string, arg5: string, arg6: string, arg7: string | null, arg8: Array<string> | null): Record<string, unknown>;
export declare function selectProviderFromAvailability(arg0: RoutingPreference, arg1: Array<Record<string, unknown>>, arg2: Record<string, unknown>, arg3: string, arg4: string, arg5: string, arg6: string, arg7: string | null, arg8: Array<string> | null, arg9: ProviderModelSelectionEvidence): Record<string, unknown>;

export declare function selectProviderFromCachedState(arg0: RoutingPreference | null): Promise<Record<string, unknown> | null>;
export declare function selectProviderFromCachedState(arg0: RoutingPreference | null, arg1: Record<string, unknown>): Promise<Record<string, unknown> | null>;
export declare function selectProviderFromCachedState(arg0: RoutingPreference | null, arg1: Record<string, unknown>, arg2: Record<string, unknown>): Promise<Record<string, unknown> | null>;
export declare function selectProviderFromCachedState(arg0: RoutingPreference | null, arg1: Record<string, unknown>, arg2: Record<string, unknown>, arg3: number): Promise<Record<string, unknown> | null>;
