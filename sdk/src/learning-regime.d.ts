export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type ForeignValue = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export interface LearningArmOptions {
  capabilityFloor?: Array<string>;
  serviceClass?: Array<string>;
  reasoning?: Array<string>;
  prompt?: Array<string>;
  authoring?: Array<string>;
  history?: Array<string>;
}

export interface LearningAssignment {
  version: string;
  policyVersion: string;
  policySha256: string;
  mode: LearningMode;
  evidenceMode: LearningEvidenceMode;
  experimentId: string;
  episodeId: string;
  taskSignatureSha256: string;
  taskSignatureCoverage: ReceiptCoverage;
  risk: LearningRisk | "unknown";
  arm: "control" | "explore";
  axis: "control" | LearningAxis;
  armId: string;
  baseline: LearningBaseline;
  options: LearningArmOptions;
  propensity: LearningPropensity;
  narrowingReason: string;
  manifestSha256: string;
}

export interface LearningAssignmentInput {
  episodeId: string;
  experimentId?: string;
  taskSignatureSha256: string;
  taskSignatureCoverage: ReceiptCoverage;
  risk?: LearningRisk;
  baseline: LearningBaseline;
  eligibleArms?: LearningArmOptions;
  hardFloor?: LearningHardFloor;
  pinnedAxes?: Array<LearningAxis>;
}

export type LearningAxis = "capabilityFloor" | "serviceClass" | "reasoning" | "prompt" | "authoring" | "history";

export interface LearningBaseline {
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  reasoning: ReasoningLevel;
  prompt: string;
  authoring: string;
  history: string;
}

export type LearningEvidenceMode = "discovery" | "evaluation";

export type LearningFact = [string, string];

export interface LearningHardFloor {
  capabilityFloor: CapabilityFloor;
  reasoning: ReasoningLevel;
}

export type LearningMode = "frozen" | "learning";

export interface LearningPolicy {
  version: number;
  mode: LearningMode;
  intensity: number;
  axes: Array<LearningAxis>;
  maxAxisDelta: number;
  riskCeiling: LearningRisk;
  seed: string;
  epoch: string;
  evidenceMode: LearningEvidenceMode;
}

export interface LearningPropensity {
  assigned: number;
  explore: number;
  axis: number;
  arm: number;
}

export type LearningRisk = "p0" | "p1" | "p2" | "p3";

export type ProcessEnv = Record<string, unknown>;

export type ReasoningLevel = "low" | "medium" | "high" | "xhigh" | "max";

export type ReceiptCoverage = "exact" | "partial" | "unknown";

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export declare const DEFAULT_LEARNING_POLICY: LearningPolicy;

export declare const LEARNING_ASSIGNMENT_VERSION: string;

export declare const LEARNING_AXES: Array<LearningAxis>;

export declare const LEARNING_POLICY_VERSION: string;

export declare function assignLearningEpisode(arg0: LearningPolicy, arg1: LearningAssignmentInput): LearningAssignment;

export declare function learningAssignmentFacts(arg0: LearningAssignment): Array<[string, string]>;

export declare function learningPolicyPath(): string;
export declare function learningPolicyPath(arg0: Record<string, unknown>): string;

export declare function learningPolicySha256(arg0: LearningPolicy): string;

export declare function loadLearningPolicy(): LearningPolicy;
export declare function loadLearningPolicy(arg0: string): LearningPolicy;
export declare function loadLearningPolicy(arg0: string, arg1: (arg0: string, arg1: string) => string): LearningPolicy;

export declare function validateLearningPolicy(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null): LearningPolicy;
