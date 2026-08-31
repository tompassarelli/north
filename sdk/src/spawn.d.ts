export type AgentComposition = TemplateComposition | BespokeComposition;

export interface BespokeComposition {
  kind: "bespoke";
  id: string;
  nearestTemplate?: string;
  bespokeReason: string;
  promotionCandidate: boolean;
  contract: BespokeContract;
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

export type CapabilityFloor = "baseline" | "standard" | "advanced" | "frontier";

export type OrchestrationCapability = "filesystem.read" | "filesystem.search" | "filesystem.write" | "shell" | "shell.readonly" | "web" | "coordination";

export type Posture = "explore" | "evaluate" | "deliver" | "preserve" | "prune";

export type ProviderPreference = "anthropic" | "openai" | "auto";

export type ReasoningLevel = "low" | "medium" | "high" | "xhigh" | "max";

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

export type ServiceClass = "economy" | "fast" | "balanced" | "premium";

export interface SpawnOptions {
  prompt: string;
  agentId?: string;
  model?: string;
  tools?: Array<string>;
  systemPrompt?: string;
  maxTurns?: number;
  thread?: string;
  coordinator?: string;
  provider?: ProviderPreference;
  target?: string;
  routingMetadata: RoutingRequest;
  projectProfile?: Record<string, unknown>;
  routingAssessment?: Record<string, unknown>;
  pinEvidence?: Record<string, unknown>;
  project?: string;
  sessionId?: string;
  worktree?: boolean;
  setupCmd?: string;
  tokenTarget?: number;
}

export type TaskGrade = "novice" | "junior" | "mid" | "senior" | "staff" | "principal" | "distinguished";

export interface TemplateComposition {
  kind: "template";
  id: string;
  overrides: Array<RoutingOverrideField>;
  overrideReason?: string;
}

export type Topology = "worker" | "orchestrator";

export declare function RecursiveChildBindingError(arg0: string): Record<string, unknown>;

export declare function appendSpawnTerminalLine(arg0: string): null;
export declare function appendSpawnTerminalLine(arg0: string, arg1: Record<string, unknown> | null): null;

export declare function applyCodexTurnDeadlineFromReasoning(): null;
export declare function applyCodexTurnDeadlineFromReasoning(arg0: Record<string, unknown>): null;

export declare function createSpawnAgentId(): string;
export declare function createSpawnAgentId(arg0: number): string;
export declare function createSpawnAgentId(arg0: number, arg1: string): string;

export declare function eligibleForLaneStartProviderRetry(arg0: string, arg1: string | null, arg2: number | null, arg3: string | null): boolean;

export declare function eligibleForProviderProcessDeathRetry(arg0: string, arg1: string | null, arg2: Array<string>): boolean;

export declare function installSpawnTerminalHandlers(): null;

export declare function managedChildSpawnOptions(arg0: string): SpawnOptions;

export declare function spawn(arg0: SpawnOptions): Promise<string>;

export declare function spawnParallel(arg0: Array<SpawnOptions>): Promise<Array<string>>;
