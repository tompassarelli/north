export interface AccountAvailabilityRow {
  account: string;
  provider: ProviderId;
  observedAt: string;
  stale: boolean;
  rungs: AccountAvailabilityRungs;
  verdict: string;
  usableModels: Array<string>;
}

export interface AccountAvailabilityRung {
  pct: number;
  resetsAt?: string;
  resetState?: "untouched";
}

export interface AccountAvailabilityRungs {
  window: AccountAvailabilityWindowRung | null;
  week: AccountAvailabilityRung | null;
  models: Record<string, unknown>;
}

export interface AccountAvailabilityWindowRung {
  name: string;
  pct: number;
  resetsAt?: string;
  resetState?: "untouched";
}

export interface ActiveSessionRoute {
  provider: ProviderId;
  account: string;
  model?: string;
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  reasoning: Effort;
}

export type AvailabilityRow = AccountAvailabilityRow;

export type AvailabilityRung = AvailabilityRungReset | AvailabilityRungUntouched;

export interface AvailabilityRungReset {
  pct: number;
  resetsAt: string;
  resetState?: Never;
}

export interface AvailabilityRungUntouched {
  pct: number;
  resetState: "untouched";
  resetsAt?: Never;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export interface Fact {
  predicate: string;
  value: string;
}

export interface FailoverBrief {
  path: string;
  sha256: string;
  content: string;
}

export interface FailoverCheck {
  threshold: number;
  classification: FailoverClassification;
  active: ActiveSessionRoute;
  unknownReason?: string;
  trigger?: RungTrigger;
  heir?: HeirRoute;
  receipts: FailoverReceipts;
}

export type FailoverClassification = "available" | "unknown" | "account-dead" | "window-dead" | "model-dead";

export interface FailoverCommand {
  executable: string;
  args: Array<string>;
}

export interface FailoverContextPackage {
  brief: FailoverBrief;
  threadMap: Array<ThreadMapEntry>;
}

export interface FailoverNotification {
  executable: string;
  args: Array<string>;
  target: string;
  subject: "PROVIDER FAILOVER FIRED";
  body: string;
}

export interface FailoverReceipts {
  active: AvailabilityRow;
  heir?: AvailabilityRow;
}

export interface FailoverRuntime {
  env?: Record<string, unknown>;
  now?: JsDate;
  northBin?: string;
  peerBb?: string;
  msgCli?: string;
  readBrief?: (arg0: string) => string;
  getFacts?: ((arg0: string) => Array<Fact>) | ((arg0: string, arg1: NorthReadOptions) => Array<Fact>);
  getChildren?: ((arg0: string) => Array<string>) | ((arg0: string, arg1: NorthReadOptions) => Array<string>);
  loadRows?: () => Array<AvailabilityRow>;
  run?: (arg0: string, arg1: Array<string>) => SpawnResult;
}

export interface FailoverSpawn {
  version: number;
  check: FailoverCheck;
  context: FailoverContextPackage;
  pinEvidence: RoutingPinEvidence;
  prompt: string;
  command: FailoverCommand;
  notification: FailoverNotification;
}

export interface FailoverWarning {
  version: number;
  thread?: string;
  threshold: number;
  active: ActiveSessionRoute;
  observedAt: string;
  crossing: RungTrigger;
  automaticFire: boolean;
}

export type ForeignValue = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export interface HeirRoute {
  provider: ProviderId;
  account: string;
  model: string;
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  reasoning: Effort;
  observedAt: string;
}

export interface NorthReadOptions {
  command?: string;
  timeoutMs?: number;
}

export type ProviderId = "anthropic" | "openai";

export interface RoutingPin {
  kind: "provider" | "account" | "model";
  value: string;
}

export interface RoutingPinEvidence {
  policyVersion: string;
  issuedAt: string;
  expiresAt: string;
  reasonCode: "provider-recovery";
  detail: string;
  pins: Array<RoutingPin>;
}

export interface RungTrigger {
  rung: "week" | "window" | "model";
  name: string;
  pct: number;
  resetsAt: string;
  model?: string;
}

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export interface SpawnResult {
  error?: Error;
  status: number | null;
  stderr?: string | Record<string, unknown>;
}

export interface ThreadMapEntry {
  id: string;
  title?: string;
  facts: Array<Fact>;
}

export declare function activeSessionIdentityFacts(arg0: string | null): Array<Record<string, unknown>>;
export declare function activeSessionIdentityFacts(arg0: string | null, arg1: FailoverRuntime): Array<Record<string, unknown>>;

export declare function activeSessionRoute(arg0: Array<Record<string, unknown>>, arg1: string | null): ActiveSessionRoute;
export declare function activeSessionRoute(arg0: Array<Record<string, unknown>>, arg1: string | null, arg2: Record<string, unknown>): ActiveSessionRoute;
export declare function activeSessionRoute(arg0: Array<Record<string, unknown>>, arg1: string | null, arg2: Record<string, unknown>, arg3: Array<Record<string, unknown>>): ActiveSessionRoute;

export declare function automaticFailoverFireEnabled(): boolean;
export declare function automaticFailoverFireEnabled(arg0: Record<string, unknown>): boolean;

export declare function availabilityForRoute(arg0: Array<Record<string, unknown>>, arg1: ActiveSessionRoute): AvailabilityRow;

export declare function checkFailover(arg0: Array<Record<string, unknown>>, arg1: ActiveSessionRoute): FailoverCheck;
export declare function checkFailover(arg0: Array<Record<string, unknown>>, arg1: ActiveSessionRoute, arg2: Record<string, unknown> | Array<unknown> | string | number | boolean | null): FailoverCheck;

export declare function composeFailoverSpawn(arg0: FailoverCheck, arg1: string, arg2: string, arg3: string): FailoverSpawn;
export declare function composeFailoverSpawn(arg0: FailoverCheck, arg1: string, arg2: string, arg3: string, arg4: FailoverRuntime): FailoverSpawn;

export declare function contextPackage(arg0: string, arg1: string): FailoverContextPackage;
export declare function contextPackage(arg0: string, arg1: string, arg2: FailoverRuntime): FailoverContextPackage;

export declare function failoverThreshold(): number;
export declare function failoverThreshold(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null): number;

export declare function failoverWarningCommands(arg0: FailoverWarning): Array<Record<string, unknown>>;
export declare function failoverWarningCommands(arg0: FailoverWarning, arg1: FailoverRuntime): Array<Record<string, unknown>>;

export declare function fireFailover(arg0: FailoverSpawn): null;
export declare function fireFailover(arg0: FailoverSpawn, arg1: FailoverRuntime): null;

export declare function loadAvailabilityRows(): Array<Record<string, unknown>>;
export declare function loadAvailabilityRows(arg0: string): Array<Record<string, unknown>>;
export declare function loadAvailabilityRows(arg0: string, arg1: (arg0: string, arg1: Array<string>, arg2: Record<string, unknown>) => string): Array<Record<string, unknown>>;

export declare function observeFailoverUsageSample(): Array<Record<string, unknown>>;
export declare function observeFailoverUsageSample(arg0: FailoverRuntime): Array<Record<string, unknown>>;

export declare function parseAvailabilityRows(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null): Array<Record<string, unknown>>;

export declare function recoveryPinEvidence(arg0: FailoverCheck): RoutingPinEvidence;
export declare function recoveryPinEvidence(arg0: FailoverCheck, arg1: JsDate): RoutingPinEvidence;

export declare function thresholdCrossings(arg0: AvailabilityRow): Array<Record<string, unknown>>;
export declare function thresholdCrossings(arg0: AvailabilityRow, arg1: Record<string, unknown> | Array<unknown> | string | number | boolean | null): Array<Record<string, unknown>>;
