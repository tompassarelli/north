export type ProviderPreference = "anthropic" | "openai" | "auto";

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
  routingMetadata: Record<string, unknown>;
  projectProfile?: Record<string, unknown>;
  routingAssessment?: Record<string, unknown>;
  pinEvidence?: Record<string, unknown>;
  project?: string;
  sessionId?: string;
  worktree?: boolean;
  setupCmd?: string;
  tokenTarget?: number;
}

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
