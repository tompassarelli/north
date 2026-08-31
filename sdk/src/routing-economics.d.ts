export interface AdmittedRoutingEconomics {
  assessment?: RoutingAssessment;
  pinEvidence?: RoutingPinEvidence;
  receipt: RoutingAdmissionReceipt;
}

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type DecisionOwnership = "none" | "bounded" | "cross-boundary" | "system-shaping" | "open-solution-class";

export type DependencyShape = "atomic-cohesive" | "deterministic-workflow" | "parallel-breadth" | "dynamic-decomposition" | "tightly-coupled-sequential";

export type ErrorExposure = "contained-reversible" | "material-recoverable" | "high-or-hard-to-reverse";

export type ForeignValue = Record<string, unknown> | Array<unknown> | string | number | boolean | null;

export type FoundationalImpact = "none" | "implementation-only" | "invariant-decision-owned";

export type OracleStrength = "not-applicable" | "objective-local" | "objective-end-to-end" | "partial" | "judgment-only";

export type ReasoningLevel = "low" | "medium" | "high" | "xhigh" | "max";

export type ReasoningShape = "deterministic" | "bounded-branching" | "multi-hypothesis" | "system-synthesis" | "exceptional";

export interface RoutingAdmissionReceipt {
  version: number;
  routingRequestSha256: string;
  routingAssessmentSha256?: string;
  pinEvidenceSha256?: string;
  staffingCatalogSha256?: string;
  providerCatalogsSha256?: string;
  routingPolicySha256: string;
  stockAxes?: RoutingReceiptAxes;
  appliedAxes: RoutingReceiptAxes;
  overrideEvidence: RoutingOverrideEvidence;
  pinEvidenceStatus: string;
  orchestrationPolicyPinSha256?: string;
  orchestrationCatalogDigestSha256?: string;
  orchestrationCatalogVersion?: number;
  orchestrationCatalogTxVersion?: number;
}

export interface RoutingAssessment {
  $schema?: string;
  version: RoutingAssessmentPolicyVersion;
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

export type RoutingAssessmentPolicyVersion = "minimum-sufficient-v2";

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
}

export type RoutingComposition = RoutingTemplateComposition | RoutingBespokeComposition;

export interface RoutingEconomicsArgs {
  request: RoutingRequest;
  routingAssessment?: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  pinEvidence?: Record<string, unknown> | Array<unknown> | string | number | boolean | null;
  provider?: string;
  target?: string;
  model?: string;
  now?: JsDate;
  surface?: string;
}

export type RoutingExceptionCode = "explicit-human-floor" | "recent-lower-capability-failure" | "calibration-experiment" | "unmodeled-risk";

export interface RoutingOverrideEvidence {
  changedAxes: Array<RoutingOverrideField>;
  status: string;
  exceptionCode?: RoutingExceptionCode;
}

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

export interface RoutingReceiptAxes {
  taskGrade: string;
  topology: string;
  capabilityFloor: CapabilityFloor;
  serviceClass: ServiceClass;
  reasoning: string;
  posture: string;
}

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

export declare const MAX_PIN_LIFETIME_MS: number;

export declare const ROUTING_ASSESSMENT_POLICY_VERSION: string;

export declare const ROUTING_PIN_POLICY_VERSION: string;

export declare function admitRoutingAssessment(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: RoutingRequest): RoutingAssessment | undefined;
export declare function admitRoutingAssessment(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: RoutingRequest, arg2: string): RoutingAssessment | undefined;

export declare function admitRoutingEconomics(arg0: RoutingEconomicsArgs): AdmittedRoutingEconomics;

export declare function admitRoutingPinEvidence(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: Record<string, unknown>): RoutingPinEvidence | undefined;
export declare function admitRoutingPinEvidence(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: Record<string, unknown>, arg2: JsDate): RoutingPinEvidence | undefined;
export declare function admitRoutingPinEvidence(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null, arg1: Record<string, unknown>, arg2: JsDate, arg3: string): RoutingPinEvidence | undefined;

export declare function canonicalJson(arg0: Record<string, unknown> | Array<unknown> | string | number | boolean | null): string;

export declare function routingEconomicsFromEnv(arg0: RoutingRequest): AdmittedRoutingEconomics;
