export interface ActiveSkillCandidate {
  name: string;
  description: string;
  path: string;
}

export interface ActiveSkillCatalog {
  root: string;
  roots: Array<string>;
  candidates: Array<ActiveSkillCandidate>;
  appendix: string;
}

export type AgentCompositionDeclaration = TemplateCompositionDeclaration | BespokeCompositionDeclaration;

export interface BespokeCompositionDeclaration {
  kind: "bespoke";
  id: string;
  nearestTemplate?: string;
  bespokeReason: string;
  promotionCandidate: boolean;
  contract: BespokeContractDeclaration;
}

export interface BespokeContractDeclaration {
  responsibility: string;
  deliverable: string;
  capabilities: Array<string>;
  mayDecide: Array<string>;
  mustEscalate: Array<string>;
  doneWhen: Array<string>;
  report: string;
}

export interface CanonicalGlobalAgents {
  path: string;
  realpath: string;
  bytes: Record<string, unknown>;
  text: string;
}

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export type EnvironmentArtifact = Record<string, unknown>;

export type EnvironmentReceipt = Record<string, unknown>;

export interface HarnessCompositionEvidence {
  roleKind?: string;
  roleId?: string;
  bespokeContractHash?: string;
  bespokeContractFingerprintVersion?: string;
  bespokeContractFingerprintDomain?: string;
  templateOverrides?: Array<string>;
  templateOverrideReasonHash?: string;
  capabilities?: Array<string>;
  commsContractHash?: string;
  taskGrade?: string;
  domainRequirements?: Array<string>;
  topology?: string;
  capabilityFloor?: string;
  serviceClass?: string;
  reasoning?: string;
  posture?: string;
  modelDelta?: ModelDeltaEvidence;
  promptEconomics?: PromptEconomicsEvidence;
  promptReceipt?: Record<string, unknown>;
  environmentReceipt?: Record<string, unknown>;
}

export interface HarnessModelAvailabilityBinding {
  required: boolean;
  targetId: string;
  model?: string;
  receipt?: Record<string, unknown>;
  observationPath: string;
}

export interface HarnessOpts {
  self: string;
  extraTools?: Array<string>;
  model?: string;
  systemPrompt?: string;
  maxTurns?: number;
  abortController?: Record<string, unknown>;
  provider?: string;
  routingMetadata?: RoutingRequestDeclaration;
  projectProfile?: Record<string, unknown>;
  omitModelDeltaReason?: string;
  modelAvailability?: Record<string, unknown>;
  cwd?: string;
  deliveryRun?: Record<string, unknown>;
  artifactDirectory?: string;
  presenceRegistrar?: Record<string, unknown>;
  presenceRenewer?: Record<string, unknown>;
  activatedResources?: Array<Record<string, unknown>>;
  availableSkills?: Array<Record<string, unknown>>;
  dataOnly?: boolean;
  outputFormat?: Record<string, unknown>;
  persistSession?: boolean;
}

export interface HarnessRouteApplication {
  options: Record<string, unknown>;
  evidence?: HarnessCompositionEvidence;
}

export interface HarnessRouteSeed {
  provider?: string;
  model?: string;
}

export type InvocationObservationReceipt = Record<string, unknown>;

export interface ManagedToolPolicyDeclaration {
  tools: Array<string>;
  allowedTools: Array<string>;
  disallowedTools: Array<string>;
}

export interface ModelDeltaEvidence {
  provider?: string;
  model?: string;
  kind: string;
  path?: string;
  reason?: string;
}

export type Options = Record<string, unknown>;

export interface OrchestrationAppendixResult {
  appendix: string;
  evidence: HarnessCompositionEvidence;
}

export type OrchestrationCapability = string;

export type PeerOperation = string;

export type ProcessEnv = Record<string, unknown>;

export interface ProjectSkillTarget {
  id: string;
  gitRoot: string;
}

export interface PromptEconomicsEvidence {
  compositionVersion: string;
  compositionDigest: string;
  capabilityClass: string;
  capabilityCount: number;
  stablePrefixBytes: number;
  uniqueTailBytes: number;
  totalBytes: number;
  byteMeasurementSource: string;
  stablePrefixTokens?: number;
  uniqueTailTokens?: number;
  totalCompositionTokens?: number;
  tokenMeasurementStatus: string;
  tokenMeasurementSource: string;
  providerContextWindowTokens?: number;
  contextWindowEffectiveFrom?: string;
  contextWindowStatus: string;
  contextWindowSource: string;
  effectiveContextBudgetTokens?: number;
  contextBudgetStatus: string;
  contextBudgetSource: string;
  compactionPolicy: string;
  compactionPolicyVersion: string;
}

export type PromptReceipt = Record<string, unknown>;

export type ProviderId = string;

export type ProviderModelAdmissionReceipt = Record<string, unknown>;

export type ResolvedProjectExposureProfile = Record<string, unknown>;

export type RoutingDraft = Record<string, unknown>;

export type RoutingOverrideField = string;

export type RoutingRequest = Record<string, unknown>;

export interface RoutingRequestDeclaration {
  role: string;
  taskGrade: string;
  domainRequirements: Array<string>;
  topology: string;
  capabilityFloor: string;
  serviceClass: string;
  reasoning: string;
  posture: string;
  composition: AgentCompositionDeclaration;
}

export interface TemplateCompositionDeclaration {
  kind: "template";
  id: string;
  overrides: Array<string>;
  overrideReason?: string;
}

export type Topology = string;

export declare const COMPACTION_POLICY_VERSION: string;

export declare const COORDINATION_TOOLS: Array<string>;

export declare const DEFAULT_SYSTEM_PROMPT: string;

export declare const GLOBAL_AGENTS_MAX_BYTES: number;

export declare const NATIVE_AGENT_TOOLS: Array<string>;

export declare const NORTH_MCP_TOOL_NAMES: Array<string>;

export declare const ORCHESTRATION_TOOLS: Array<string>;

export declare const PROJECT_AGENTS_MAX_BYTES: number;

export declare const PROMPT_COMPOSITION_VERSION: string;

export declare function activeSkillCatalog(): ActiveSkillCatalog;
export declare function activeSkillCatalog(arg0: Record<string, unknown>): ActiveSkillCatalog;
export declare function activeSkillCatalog(arg0: Record<string, unknown>, arg1?: string): ActiveSkillCatalog;

export declare function applyHarnessRoute(arg0: Record<string, unknown>, arg1: string): HarnessRouteApplication;
export declare function applyHarnessRoute(arg0: Record<string, unknown>, arg1: string, arg2?: string): HarnessRouteApplication;
export declare function applyHarnessRoute(arg0: Record<string, unknown>, arg1: string, arg2?: string, arg3?: string): HarnessRouteApplication;
export declare function applyHarnessRoute(arg0: Record<string, unknown>, arg1: string, arg2?: string, arg3?: string, arg4?: Record<string, unknown>): HarnessRouteApplication;

export declare function canonicalGlobalAgents(): CanonicalGlobalAgents | undefined;
export declare function canonicalGlobalAgents(arg0: Record<string, unknown>): CanonicalGlobalAgents | undefined;

export declare function canonicalHarnessModelAvailability(arg0: Record<string, unknown>, arg1: string): HarnessModelAvailabilityBinding | undefined;

export declare function domainSkillsDir(): string;
export declare function domainSkillsDir(arg0: Record<string, unknown>): string;

export declare function globalLawsPath(): string;
export declare function globalLawsPath(arg0: Record<string, unknown>): string;

export declare function harnessCompositionEvidence(arg0: Record<string, unknown>): HarnessCompositionEvidence | undefined;

export declare function harnessOptions(arg0: HarnessOpts): Record<string, unknown>;

export declare function harnessRouteSeed(arg0: Record<string, unknown>): HarnessRouteSeed | undefined;

export declare function hasCanonicalAuthoringHooks(arg0: Record<string, unknown>): boolean;

export declare function hasCanonicalHarnessAuthority(arg0: Record<string, unknown>, arg1: string): boolean;

export declare function managedToolPolicy(arg0: Array<string>): ManagedToolPolicyDeclaration;

export declare function orchestrationAppendix(arg0: Record<string, unknown> | null): OrchestrationAppendixResult;
export declare function orchestrationAppendix(arg0: Record<string, unknown> | null, arg1: string): OrchestrationAppendixResult;
export declare function orchestrationAppendix(arg0: Record<string, unknown> | null, arg1: string, arg2: Record<string, unknown>): OrchestrationAppendixResult;

export declare function peerCommandServer(arg0: string): Record<string, unknown>;

export declare function praxisAppendix(): string;
export declare function praxisAppendix(arg0: string | null): string;
export declare function praxisAppendix(arg0: string | null, arg1: string | null): string;
export declare function praxisAppendix(arg0: string | null, arg1: string | null, arg2: string | null): string;

export declare function projectAgentsAppendix(arg0: string): string;
export declare function projectAgentsAppendix(arg0: string, arg1: Record<string, unknown>): string;

export declare function projectSkillTarget(arg0: string): ProjectSkillTarget | undefined;
export declare function projectSkillTarget(arg0: string, arg1: Record<string, unknown>): ProjectSkillTarget | undefined;

export declare function renewHarnessPresence(arg0: Record<string, unknown>): null;

export declare function sendPeerCommand(arg0: string, arg1: string, arg2: string, arg3: Record<string, unknown>): string;

export declare function validatePeerCommandArgs(arg0: string, arg1: Record<string, unknown>): null;
