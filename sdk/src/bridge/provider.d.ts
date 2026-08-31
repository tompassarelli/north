export interface AbortController {}

export interface AbortSignal {}

export interface BridgeAttemptRouteAuthority {}

export interface BridgeAutomaticProviderSelection {
  role: BridgeLaunchRole;
  capabilityFloor?: CapabilityFloor;
  serviceClass?: ServiceClass;
  model?: string;
  reasoning?: Effort;
}

export type BridgeLaunchProvider = "anthropic" | "openai";

export type BridgeLaunchRole = "director" | "implementer";

export interface BridgeLaunchSelection {
  provider?: BridgeLaunchProvider;
  capabilityFloor?: CapabilityFloor;
  serviceClass?: ServiceClass;
  model?: string;
  reasoning?: Effort;
}

export interface BridgeProviderExecution {
  open: (arg0: BridgeProviderOpenContext) => Promise<BridgeProviderSession>;
}

export interface BridgeProviderOpenContext {
  executionId: string;
  prompt: string;
  cwd: string;
  role: BridgeLaunchRole;
  provider: BridgeLaunchProvider;
  capabilityFloor?: CapabilityFloor;
  serviceClass?: ServiceClass;
  model?: string;
  reasoning?: Effort;
  attemptRoute: BridgeAttemptRouteAuthority;
  signal: AbortSignal;
  writer: WireEventWriter;
}

export interface BridgeProviderSession {
  presentation?: BridgeSessionPresentation;
  submitInput: (arg0: string) => Promise<null>;
  interruptTurn: () => Promise<null>;
  terminateSession: () => Promise<null>;
  forceTerminateSession?: () => null;
  events: () => Record<string, unknown>;
}

export interface BridgeProviderTeardownTimeoutError {
  name: string;
  message: string;
}

export interface BridgeRouteResult {
  target?: RoutingTarget;
  receipt?: ProviderModelAdmissionReceipt;
}

export interface BridgeSessionPresentation {
  model?: string;
  effort?: string;
  cwd: string;
  permissionMode?: string;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export interface ProviderModelAdmissionReceipt {}

export interface ResolvedBridgeLaunchSelection {
  routingMetadata: Record<string, unknown>;
  resolved: Record<string, unknown>;
}

export interface RoutingTarget {}

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export interface WireEvent {}

export interface WireEventWriter {}

export interface WireQuery {}

export declare const BridgeProviderTeardownTimeoutError: {
  new(arg0: number): BridgeProviderTeardownTimeoutError;
};

export declare const BridgeWireSession: {
  new(arg0: WireQuery, arg1: AbortController, arg2: AbortSignal, arg3?: BridgeSessionPresentation): BridgeProviderSession;
};

export declare const bridgeProvider: BridgeProviderExecution;

export declare function bridgeProviderWithDependenciesForTest(arg0: Record<string, unknown>, arg1: Record<string, unknown>): BridgeProviderExecution;

export declare function bridgeRoute(arg0: Record<string, unknown>, arg1: string, arg2?: Record<string, unknown>): Promise<Record<string, unknown>>;

export declare function bridgeSystemPrompt(arg0: string): string;

export declare function resolveBridgeLaunchSelection(arg0: string, arg1: string, arg2: BridgeLaunchSelection): ResolvedBridgeLaunchSelection;

export declare function selectBridgeProvider(arg0?: BridgeAutomaticProviderSelection): Promise<string>;
