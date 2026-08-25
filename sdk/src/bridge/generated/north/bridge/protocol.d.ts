import type { WireEvent } from "../../../../wire/events";
import type { JournalRecord, TornTail } from "../../../journal";
import type { ReasoningLevel, RoutingTier } from "../../../../routing-metadata";

export interface BridgeHello {
  type: "hello";
  identity?: string;
  liveExecutions: number;
  pinningExecutions?: number;
  pid: number;
}

export type BridgeServerMessage =
  | BridgeHello
  | { type: "launched"; executionId: string }
  | { type: "controlled"; executionId: string; control: string; delivery: string }
  | { type: "event"; record: JournalRecord }
  | { type: "wire"; event: WireEvent }
  | { type: "barrier"; executionId: string; cursor: number; tornTail?: TornTail }
  | { type: "error"; message: string };

export type BridgeLaunchRole = "director" | "implementer";
export type BridgeLaunchProvider = "anthropic" | "openai";

export interface BridgeLaunchSelection {
  provider?: BridgeLaunchProvider;
  tier?: RoutingTier;
  model?: string;
  effort?: ReasoningLevel;
}

export type BridgeRequest =
  | { op: "launch"; prompt: string; cwd: string; role: BridgeLaunchRole;
      attemptId: string; executionId?: string; provider?: BridgeLaunchProvider;
      tier?: RoutingTier; model?: string; effort?: ReasoningLevel }
  | { op: "attach"; executionId: string; cursor: number }
  | { op: "submitInput"; executionId: string; input: string }
  | { op: "interruptTurn"; executionId: string }
  | { op: "redirectNow"; executionId: string; input: string }
  | { op: "terminateSession"; executionId: string }
  | { op: "retire" };

declare function bridgeSourceIdentity(): string | undefined;

declare function pinningExecutions(hello: BridgeHello): number;

declare function parseBridgeLaunchProvider(value: unknown): BridgeLaunchProvider | undefined;

declare function parseBridgeLaunchTier(value: unknown): RoutingTier | undefined;

declare function parseBridgeLaunchModel(value: unknown): string | undefined;

declare function parseBridgeLaunchEffort(value: unknown): ReasoningLevel | undefined;

declare function parseBridgeLaunchRole(value: unknown): BridgeLaunchRole;

declare function bridgeStateDirectory(env?: NodeJS.ProcessEnv): string;

declare function bridgeSocketPath(env?: NodeJS.ProcessEnv): string;

declare function bridgeJournalRoot(env?: NodeJS.ProcessEnv): string;

declare function parseBridgeRequest(value: unknown): BridgeRequest;

declare function parseBridgeLaunchAttemptId(value: unknown): string;

declare function parseBridgeLaunchExecutionId(value: unknown): string;

export {
  bridgeJournalRoot as "bridge-journal-root",
  bridgeSocketPath as "bridge-socket-path",
  bridgeSourceIdentity as "bridge-source-identity",
  bridgeStateDirectory as "bridge-state-directory",
  parseBridgeLaunchAttemptId as "parse-bridge-launch-attempt-id!",
  parseBridgeLaunchEffort as "parse-bridge-launch-effort!",
  parseBridgeLaunchExecutionId as "parse-bridge-launch-execution-id!",
  parseBridgeLaunchModel as "parse-bridge-launch-model!",
  parseBridgeLaunchProvider as "parse-bridge-launch-provider!",
  parseBridgeLaunchRole as "parse-bridge-launch-role!",
  parseBridgeLaunchTier as "parse-bridge-launch-tier!",
  parseBridgeRequest as "parse-bridge-request!",
  pinningExecutions as "pinning-executions",
};
