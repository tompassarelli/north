export type AgentComposition = TemplateComposition | BespokeComposition;

export type AgentCompositionRecord = TemplateCompositionRecord | BespokeCompositionRecord;

export interface BespokeComposition {
  kind: "bespoke";
  id: string;
  nearestTemplate?: string;
  bespokeReason: string;
  promotionCandidate: boolean;
  contract: BespokeContract;
}

export interface BespokeCompositionRecord {
  kind: string;
  id: string;
  nearestTemplate: string | null;
  bespokeReason: string;
  promotionCandidate: boolean;
  contract: BespokeContractRecord;
}

export interface BespokeContract {
  responsibility: string;
  deliverable: string;
  capabilities: Array<OrchestrationCapability>;
  mayDecide: Array<string>;
  mustEscalate: Array<string>;
  doneWhen: Array<string>;
  report: string;
}

export interface BespokeContractRecord {
  responsibility: string;
  deliverable: string;
  capabilities: Array<string>;
  mayDecide: Array<string>;
  mustEscalate: Array<string>;
  doneWhen: Array<string>;
  report: string;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type CompositionKind = "template" | "bespoke";

export type ForeignValue = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export interface JsonV1 {
  parse: (arg0: string) => Record<string, unknown>;
}

export type OrchestrationCapability = "filesystem.read" | "filesystem.search" | "filesystem.write" | "shell" | "shell.readonly" | "web" | "coordination";

export type Posture = "explore" | "evaluate" | "deliver" | "preserve" | "prune";

export interface ProcessV1 {
  env: Map<string, string | null>;
}

export type ReasoningLevel = "low" | "medium" | "high" | "xhigh" | "max";

export interface RegExpV1 {
  test: (arg0: string) => boolean;
}

export interface RoutingDraft {
  role?: string;
  taskGrade?: TaskGrade;
  domainRequirements?: Array<string>;
  topology?: Topology;
  capabilityFloor?: CapabilityFloor;
  serviceClass?: ServiceClass;
  reasoning?: ReasoningLevel;
  posture?: Posture;
  composition?: AgentComposition;
}

export interface RoutingDraftRecord {
  role: string | null;
  taskGrade: string | null;
  domainRequirements: Array<string> | null;
  topology: string | null;
  capabilityFloor: string | null;
  serviceClass: string | null;
  reasoning: string | null;
  posture: string | null;
  composition: AgentCompositionRecord | null;
}

export type RoutingMetadata = RoutingDraft;

export type RoutingOverrideField = "taskGrade" | "domainRequirements" | "capabilityFloor" | "serviceClass" | "reasoning" | "posture";

export interface RoutingRequest {
  role: string;
  taskGrade: TaskGrade;
  domainRequirements: Array<string>;
  topology: Topology;
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  reasoning: ReasoningLevel;
  posture: Posture;
  composition: AgentComposition;
}

export interface RoutingRequestRecord {
  role: string;
  taskGrade: string;
  domainRequirements: Array<string>;
  topology: string;
  capabilityFloor: string;
  serviceClass: string;
  reasoning: string;
  posture: string;
  composition: AgentCompositionRecord;
}

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export type TaskGrade = "novice" | "junior" | "mid" | "senior" | "staff" | "principal" | "distinguished";

export interface TemplateComposition {
  kind: "template";
  id: string;
  overrides: Array<RoutingOverrideField>;
  overrideReason?: string;
}

export interface TemplateCompositionRecord {
  kind: string;
  id: string;
  overrides: Array<string>;
  overrideReason: string | null;
}

export type Topology = "worker" | "orchestrator";

export declare const CAPABILITY_FLOORS: Array<string>;

export declare const COMPOSITION_KINDS: Array<string>;

export declare const POSTURES: Array<string>;

export declare const REASONING_LEVELS: Array<string>;

export declare const ROUTING_OVERRIDE_FIELDS: Array<string>;

export declare const ROUTING_REQUEST_FIELDS: Array<string>;

export declare const SERVICE_CLASSES: Array<string>;

export declare const TASK_GRADES: Array<string>;

export declare const TOPOLOGIES: Array<string>;

export declare function canonicalRole(arg0?: string): string | undefined;

export declare function parseCompleteRoutingRequest(arg0: RoutingDraft): RoutingRequest;
export declare function parseCompleteRoutingRequest(arg0: RoutingDraft, arg1: string): RoutingRequest;

export declare function routingMetadataFromEnv(): RoutingMetadata;

export declare function validateRoutingMetadata(arg0: RoutingDraft): RoutingDraft;
