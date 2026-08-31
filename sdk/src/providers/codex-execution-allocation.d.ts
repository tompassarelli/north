export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export interface CodexAccountAuthority {
  role: string;
  executionEligible: boolean;
  receipt: StoreAccountAuthorityReceipt;
}

export interface CodexExecutionAllocation {
  target: RoutingTarget;
  model: string;
  effort?: Effort;
  receipt: CodexExecutionReceipt;
}

export interface CodexExecutionAllocatorDependencies {
  readAuthority?: (arg0: RoutingTarget) => Promise<CodexAccountAuthority | null>;
  loadUsage?: (arg0: RoutingTarget) => Promise<StoreObservationSnapshot | null>;
}

export interface CodexExecutionReceipt {
  accountAuthority: StoreAccountAuthorityReceipt;
  usage: StoreObservationSnapshot;
}

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export interface ProviderAvailability {}

export type ProviderId = "anthropic" | "openai";

export interface ProviderUsageObservation {
  provider: ProviderId;
  source: string;
  windows?: Array<ProviderUsageWindow>;
}

export interface ProviderUsageWindow {
  limitId?: string;
  resetsAt?: string;
  usedPercent: number;
}

export interface ResolvedRoute {
  model?: string;
  effort?: Effort;
}

export interface RoutingTarget {
  id: string;
  provider: ProviderId;
  authMode: string;
  profile?: string;
}

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export interface StoreAccountAuthorityReceipt {}

export interface StoreObservationSnapshot {
  observation: ProviderUsageObservation;
}

export declare function allocateCodexExecutionAccount(arg0: Array<RoutingTarget>, arg1: Array<ProviderAvailability>, arg2: string, arg3: string, arg4: string | null, arg5: CodexExecutionAllocatorDependencies | null): Promise<CodexExecutionAllocation | null>;
