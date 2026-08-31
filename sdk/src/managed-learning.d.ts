export interface BespokeContract {
  responsibility: string;
  deliverable: string;
  capabilities: Array<string>;
  mayDecide: Array<string>;
  mustEscalate: Array<string>;
  doneWhen: Array<string>;
  report: string;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type DecisionOwnership = "none" | "bounded" | "cross-boundary" | "system-shaping" | "open-solution-class";

export type DependencyShape = "atomic-cohesive" | "deterministic-workflow" | "parallel-breadth" | "dynamic-decomposition" | "tightly-coupled-sequential";

export type ErrorExposure = "contained-reversible" | "material-recoverable" | "high-or-hard-to-reverse";

export type ForeignValue = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export type FoundationalImpact = "none" | "implementation-only" | "invariant-decision-owned";

export interface LearningAssignment {
  version: "north-learning-assignment:v1";
  policyVersion: "north-learning-policy:v1";
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
  options: LearningOptions;
  propensity: LearningPropensity;
  narrowingReason: string;
  manifestSha256: string;
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

export type LearningMode = "frozen" | "learning";

export interface LearningOptions {
  capabilityFloor?: Array<string>;
  serviceClass?: Array<string>;
  reasoning?: Array<string>;
  prompt?: Array<string>;
  authoring?: Array<string>;
  history?: Array<string>;
}

export interface LearningPropensity {
  assigned: number;
  explore: number;
  axis: number;
  arm: number;
}

export type LearningRisk = "p0" | "p1" | "p2" | "p3";

export interface ManagedLearningDecision {
  assignment: LearningAssignment;
  routingMetadata: RoutingRequest;
  routingAssessment?: RoutingAssessment;
}

export interface ManagedLearningInput {
  episodeId: string;
  taskSignature: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  taskSignatureCoverage: ReceiptCoverage;
  routingMetadata: RoutingRequest;
  routingAssessment?: RoutingAssessment;
  pinEvidence?: RoutingPinEvidence;
  promptArms?: Array<string>;
  authoringArms?: Array<string>;
  historyArms?: Array<string>;
}

export type OracleStrength = "not-applicable" | "objective-local" | "objective-end-to-end" | "partial" | "judgment-only";

export type ReasoningLevel = "low" | "medium" | "high" | "xhigh" | "max";

export type ReasoningShape = "deterministic" | "bounded-branching" | "multi-hypothesis" | "system-synthesis" | "exceptional";

export type ReceiptCoverage = "exact" | "partial" | "unknown";

export interface RoutingAssessment {
  $schema?: string;
  version: "minimum-sufficient-v2";
  signals: RoutingAssessmentSignals;
  derived: RoutingAssessmentDerived;
  selected: RoutingAssessmentSelected;
  exception?: RoutingAssessmentException;
  exceptionalDeliberation?: string;
}

export interface RoutingAssessmentDerived {
  minimumCapabilityFloor: CapabilityFloor;
  minimumReasoning: ReasoningLevel;
  ruleCodes: Array<string>;
}

export interface RoutingAssessmentException {
  code: RoutingExceptionCode;
  detail: string;
}

export interface RoutingAssessmentSelected {
  capabilityFloor: CapabilityFloor;
  reasoning: ReasoningLevel;
}

export interface RoutingAssessmentSignals {
  decisionOwnership: DecisionOwnership;
  seamScope: SeamScope;
  errorExposure: ErrorExposure;
  oracleStrength: OracleStrength;
  foundationalImpact: FoundationalImpact;
  dependencyShape: DependencyShape;
  reasoningShape: ReasoningShape;
}

export interface RoutingBespokeComposition {
  kind: "bespoke";
  id: string;
  nearestTemplate?: string;
  bespokeReason: string;
  promotionCandidate: boolean;
  contract: BespokeContract;
}

export type RoutingComposition = RoutingTemplateComposition | RoutingBespokeComposition;

export type RoutingExceptionCode = "explicit-human-floor" | "recent-lower-capability-failure" | "calibration-experiment" | "unmodeled-risk";

export type RoutingOverrideField = "taskGrade" | "domainRequirements" | "capabilityFloor" | "serviceClass" | "reasoning" | "posture";

export interface RoutingPin {
  kind: RoutingPinKind;
  value: string;
}

export interface RoutingPinEvidence {
  policyVersion: RoutingPinPolicyVersion;
  issuedAt: string;
  expiresAt: string;
  reasonCode: RoutingPinReasonCode;
  detail: string;
  pins: Array<RoutingPin>;
}

export type RoutingPinKind = "provider" | "account" | "model";

export type RoutingPinPolicyVersion = "north-routing-pin-v1";

export type RoutingPinReasonCode = "explicit-human-request" | "provider-recovery" | "capability-requirement" | "calibration-experiment";

export interface RoutingRequest {
  role: string;
  taskGrade: string;
  domainRequirements: Array<string>;
  topology: string;
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  reasoning: ReasoningLevel;
  posture: string;
  composition: RoutingComposition;
}

export interface RoutingTemplateComposition {
  kind: "template";
  id: string;
  overrides: Array<RoutingOverrideField>;
  overrideReason?: string;
}

export type SeamScope = "none" | "established" | "consequential" | "system-wide";

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export declare function decideManagedLearning(arg0: ManagedLearningInput): ManagedLearningDecision;

export declare function learningRiskFromAssessment(): string | undefined;
export declare function learningRiskFromAssessment(arg0?: RoutingAssessment): string | undefined;
