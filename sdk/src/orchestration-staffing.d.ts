export type AgentCompositionV3 = TemplateCompositionV3 | BespokeCompositionV3;

export interface BespokeCompositionV3 {
  kind: string;
  id: string;
  nearestTemplate: string | null;
  bespokeReason: string;
  promotionCandidate: boolean;
  contract: BespokeContractV3;
}

export interface BespokeContractV3 {
  responsibility: string;
  deliverable: string;
  capabilities: Array<string>;
  mayDecide: Array<string>;
  mustEscalate: Array<string>;
  doneWhen: Array<string>;
  report: string;
}

export interface CapabilityModuleV3 {
  orchestrationCapabilities: Array<string>;
  requireOrchestrationCapabilities: (arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: string) => Array<string>;
  validatePostureCapabilities: (arg0: string, arg1: Array<string>, arg2: string) => null;
  validateTopologyCapabilities: (arg0: string, arg1: Array<string>, arg2: string) => null;
}

export interface CatalogAxis {
  field: string;
  vocabularyField: string;
  expected: Array<string>;
}

export type ForeignValue = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export interface FsModuleV1 {
  readFileSync: (arg0: string, arg1: string) => string;
}

export interface GraphModuleV3 {
  projectStaffingCatalog: () => Record<string, unknown>;
  staffingSource: () => string;
  warnGraphCatalogFallback: (arg0: string, arg1: Record<string, unknown>) => null;
}

export interface JsonV1 {
  parse: (arg0: string) => Record<string, unknown>;
  stringify: (arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => string;
}

export interface PathModuleV1 {
  resolve2: (arg0: string, arg1: string) => string;
  resolve4: (arg0: string, arg1: string, arg2: string, arg3: string) => string;
}

export interface ProcessV1 {
  env: Map<string, string | null>;
}

export interface RoleModuleV3 {
  requireOrchestrationRoleId1: (arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null) => string;
  requireOrchestrationRoleId2: (arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: string) => string;
}

export type RouteValue = string | Array<string> | null;

export interface RoutingDraftV3 {
  role: string | null;
  taskGrade: string | null;
  domainRequirements: Array<string> | null;
  topology: string | null;
  capabilityFloor: string | null;
  serviceClass: string | null;
  reasoning: string | null;
  posture: string | null;
  composition: AgentCompositionV3 | null;
}

export interface RoutingRequestV3 {
  role: string;
  taskGrade: string;
  domainRequirements: Array<string>;
  topology: string;
  capabilityFloor: string;
  serviceClass: string;
  reasoning: string;
  posture: string;
  composition: AgentCompositionV3;
}

export interface StaffingCatalog {
  sourceVersion: number;
  vocabulary: StaffingVocabulary;
  defaults: StaffingDefaults;
  presets: Array<StaffingPreset>;
}

export interface StaffingDefaults {
  taskGrade: import("./routing-metadata.js").TaskGrade;
  capabilityFloor: import("./routing-metadata.js").CapabilityFloor;
  serviceClass: import("./routing-metadata.js").ServiceClass;
  deliberation: import("./routing-metadata.js").ReasoningLevel;
  topology: import("./routing-metadata.js").Topology;
  posture: import("./routing-metadata.js").Posture;
}

export interface StaffingPreset {
  name: string;
  taskGrade: import("./routing-metadata.js").TaskGrade;
  capabilityFloor: import("./routing-metadata.js").CapabilityFloor;
  serviceClass: import("./routing-metadata.js").ServiceClass;
  deliberation: import("./routing-metadata.js").ReasoningLevel;
  topology: import("./routing-metadata.js").Topology;
  posture: import("./routing-metadata.js").Posture;
  capabilities: Array<import("./routing-metadata.js").OrchestrationCapability>;
  tagline: string;
  description: string;
}

export interface StaffingVocabulary {
  taskGrades: Array<import("./routing-metadata.js").TaskGrade>;
  capabilityFloors: Array<import("./routing-metadata.js").CapabilityFloor>;
  serviceClasses: Array<import("./routing-metadata.js").ServiceClass>;
  deliberations: Array<import("./routing-metadata.js").ReasoningLevel>;
  topologies: Array<import("./routing-metadata.js").Topology>;
  postures: Array<import("./routing-metadata.js").Posture>;
  capabilities: Array<import("./routing-metadata.js").OrchestrationCapability>;
}

export interface TemplateCompositionV3 {
  kind: string;
  id: string;
  overrides: Array<string>;
  overrideReason: string | null;
}

export declare const DEFAULT_ORCHESTRATION_STAFFING_PATH: string;

export declare const ORCHESTRATION_STOCK_ROLE_IDS: Array<string>;

export declare function applyOrchestrationStaffing(arg0: import("./routing-metadata.js").RoutingDraft): import("./routing-metadata.js").RoutingRequest;
export declare function applyOrchestrationStaffing(arg0: import("./routing-metadata.js").RoutingDraft, arg1: StaffingCatalog): import("./routing-metadata.js").RoutingRequest;

export declare function canonicalStaffingRole(arg0: string | null): string | null;
export declare function canonicalStaffingRole(arg0: string | null, arg1: Record<string, unknown>): string | null;

export declare function loadOrchestrationStaffing(): StaffingCatalog;
export declare function loadOrchestrationStaffing(arg0: string): StaffingCatalog;

export declare function orchestrationCapabilities(arg0: Record<string, unknown>): Array<string>;
export declare function orchestrationCapabilities(arg0: Record<string, unknown>, arg1: Record<string, unknown>): Array<string>;

export declare function requireManagedOrchestrationSelection(arg0: import("./routing-metadata.js").RoutingDraft): import("./routing-metadata.js").RoutingRequest;
export declare function requireManagedOrchestrationSelection(arg0: import("./routing-metadata.js").RoutingDraft, arg1: string): import("./routing-metadata.js").RoutingRequest;
