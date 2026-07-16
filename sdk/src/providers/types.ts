import type { Options } from "@anthropic-ai/claude-agent-sdk";
import type { Effort } from "../harness";

export type ProviderId = "anthropic" | "openai";
export type ProviderPreference = ProviderId | "auto";
export interface RoutingRequest {
  provider?: ProviderPreference;
  /** Exact target pin. Exact pins never fall back to another target. */
  target?: string;
}
export type RoutingPreference = ProviderPreference | RoutingRequest;
export type EntitlementPressure = "plenty" | "normal" | "low" | "exhausted" | "unknown";
export type AllocationMode = "preferential" | "balanced" | "reserved";
export type TargetAuthMode = "ambient" | "isolated";

export interface RoutingTarget {
  id: string;
  provider: ProviderId;
  /** Missing means ambient for policies written before per-target auth existed. */
  authMode?: TargetAuthMode;
  profile?: string;
}

export interface PressureObservation {
  state: EntitlementPressure;
  observedAt: string;
  until?: string;
}

export interface ProviderUsageWindow {
  limitId?: string;
  usedPercent: number;
  resetsAt: string;
}

export interface ProviderUsageObservation {
  targetId: string;
  provider: ProviderId;
  observedAt: string;
  until?: string;
  /** Adapter-normalized state when the provider does not expose numeric windows. */
  state?: EntitlementPressure;
  windows?: ProviderUsageWindow[];
}

export interface ProviderUsageObservationStore {
  version: 1;
  observations: ProviderUsageObservation[];
}

export interface EnvelopeLimits {
  runs?: number;
  frontierRuns?: number;
  retries?: number;
  parallelism?: number;
}

export interface ResourceEnvelopes {
  default?: EnvelopeLimits;
  month?: EnvelopeLimits;
  week?: EnvelopeLimits;
  projects?: Record<string, EnvelopeLimits>;
  sessions?: Record<string, EnvelopeLimits>;
}

export interface ResourcePolicy {
  version?: 1;
  mode: AllocationMode;
  targets?: RoutingTarget[];
  targetOrder?: string[];
  providerOrder: ProviderId[];
  pressures: Partial<Record<ProviderId, EntitlementPressure>>;
  weights?: Partial<Record<ProviderId, number>>;
  /** Executable pressure keyed by routing target; provider pressures are a compatibility projection. */
  targetPressures?: Record<string, EntitlementPressure>;
  pressureObservations?: Record<string, PressureObservation>;
  automatedPressureObservations?: Record<string, ProviderUsageObservation>;
  targetWeights?: Record<string, number>;
  reservedFrontierTarget?: string;
  reservedFrontierProvider?: ProviderId;
  envelopes?: ResourceEnvelopes;
}

export interface ProviderAvailability {
  provider: ProviderId;
  /** Present when the probe was run in a routing target's authentication context. */
  targetId?: string;
  installed?: boolean;
  authenticated?: boolean;
  available: boolean;
  reason: "ready" | "command_missing" | "authentication_missing" | "disabled" | "unknown";
  detail?: string;
}

// Deliberately query-shaped while North migrates its mature supervision loop to
// normalized events. Both adapters satisfy this boundary; provider SDK imports do
// not escape their adapter directory.
export interface AgentQuery {
  [Symbol.asyncIterator](): AsyncIterator<any>;
  interrupt?(): Promise<void>;
  setModel?(model: string): Promise<void> | void;
  applyFlagSettings?(settings: { effortLevel?: Effort | null }): Promise<void> | void;
  /** True only when both model and effort can be changed on the active run. */
  supportsInFlightEscalation?(): boolean;
}

export class ProviderEscalationUnsupportedError extends Error {
  readonly code = "provider_escalation_unsupported";
  constructor(message: string) {
    super(message);
    this.name = "ProviderEscalationUnsupportedError";
  }
}

/**
 * A provider may raise this only when it can prove the request was not accepted
 * and no externally observable tool/model side effect occurred. North never
 * infers retry safety from error text or from an empty output stream.
 */
export class ProviderRetrySafeError extends Error {
  readonly retrySafeBeforeAcceptance = true;
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "ProviderRetrySafeError";
  }
}

export interface RoutingFallbackReason {
  sequence: number;
  reason: "provider_retry_safe_before_acceptance";
  fromTarget: string;
  fromProvider: ProviderId;
  toTarget: string;
  toProvider: ProviderId;
}

export interface AgentProvider {
  id: ProviderId;
  probe(target?: RoutingTarget): ProviderAvailability;
  query(args: { prompt: string | AsyncIterable<any>; options: Options; target?: RoutingTarget }): AgentQuery;
}

export interface RoutingDecision {
  /** Compatibility alias for requestedProvider. */
  requested: ProviderPreference;
  requestedProvider: ProviderPreference;
  requestedTarget?: string;
  target: string;
  provider: ProviderId;
  /** Immutable target metadata used by provider adapters for target-scoped auth. */
  routingTargets: Record<string, RoutingTarget>;
  /** Initial allocation explanation. Never replaced by execution fallback detail. */
  readonly selectionReason: string;
  /** Compatibility alias for selectionReason. */
  readonly reason: string;
  availability: ProviderAvailability[];
  /** Remaining eligible target IDs, in retry order. */
  fallbackTargets: string[];
  /** Append-only target IDs actually attempted, beginning with the selected target. */
  readonly fallbackTargetPath: string[];
  /** Provider projection of fallbackTargets; duplicate providers are meaningful. */
  fallbackProviders: ProviderId[];
  fallbackCount: number;
  /** Append-only provider projection of fallbackTargetPath. */
  readonly fallbackPath: ProviderId[];
  /** Append-only redacted, structured fallback decisions in occurrence order. */
  readonly fallbackReasons: RoutingFallbackReason[];
  allocationMode: AllocationMode;
  entitlementPressure: EntitlementPressure;
  targetEntitlementPressures: Record<string, EntitlementPressure>;
  entitlementPressures: Partial<Record<ProviderId, EntitlementPressure>>;
  /** Actual model/effort used by the currently active provider route. */
  resolvedModel?: string;
  resolvedEffort?: string;
}
