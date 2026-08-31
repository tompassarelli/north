export type RunProvenanceFact = [string, string];

export interface ShadowReviewerExecutionProvenance {
  version: string;
  targetId: string;
  sourceRunId: string;
  sourceFromSequence: number;
  sourceThroughSequence: number;
  privacyOmittedEvents: number;
  capacityOmittedEvents: number;
  inputSha256: string;
}

export interface WireModelAvailabilityReceipt {
  provider: string;
  targetId: string;
  observedAt: string;
  source: string;
  observationDigest: string;
}

export interface WireRunProvenance {
  posture?: string;
  role?: string;
  provider?: string;
  providerTarget?: string;
  providerReason?: string;
  modelAvailability?: WireModelAvailabilityReceipt;
  requestedProvider?: string;
  requestedTarget?: string;
  requestedCapabilityFloor?: string;
  requestedServiceClass?: string;
  requestedEffort?: string;
  routingMetadata?: Record<string, unknown>;
  routingAssessment?: Record<string, unknown>;
  routingAdmissionReceipt?: Record<string, unknown>;
  routingPinEvidence?: Record<string, unknown>;
  promptComposition?: Record<string, unknown>;
  learningAssignment?: Record<string, unknown>;
  promptReceipt?: Record<string, unknown>;
  environmentReceipt?: Record<string, unknown>;
  runEnvelopeReceipt?: Record<string, unknown>;
  mcpActivity?: Record<string, unknown>;
  nativeCommandActivity?: Record<string, unknown>;
  effectiveAuthority?: Record<string, unknown>;
  allocationMode?: string;
  entitlementPressure?: string;
  allocationEvidence?: Record<string, unknown>;
  fallbackCount?: number;
  fallbackPath?: Array<string>;
  fallbackTargetPath?: Array<string>;
  fallbackReasons?: Array<Record<string, unknown>>;
  envelopeScopes?: Array<string>;
  envelopeRetries?: number;
  envelopeAdvisories?: Array<string>;
  processOutcome?: string;
  deliveryOutcome?: string;
  deliveryReason?: string;
  deliveryProof?: Record<string, unknown>;
  retryOfRun?: string;
  retryAttempt?: number;
  executionSource?: string;
  executionTransport?: string;
  executionObservation?: Record<string, unknown>;
  runEstimate?: Record<string, unknown>;
  judgmentGrade?: Record<string, unknown>;
  struggleObservation?: Record<string, unknown>;
  tokenBudget?: Record<string, unknown>;
  shadowReviewerSummary?: Record<string, unknown>;
  shadowReviewerExecution?: ShadowReviewerExecutionProvenance;
}

export declare function wireModelAvailabilityReceipt(arg0: Record<string, unknown>): WireModelAvailabilityReceipt;

export declare function wireRunProvenanceFacts(arg0: WireRunProvenance, arg1: number): Array<[string, string]>;
