import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

// HEAD is the identity: main is never dirty by policy, so committed state is
// live state. Client and daemon both compute this; equality at connect is the
// freshness contract that makes a silently stale daemon unreachable.
export function bridgeSourceIdentity(): string | undefined {
  const repo = resolve(import.meta.dir, "../../..");
  const git = process.env.NORTH_GIT_BIN ?? "git";
  const result = spawnSync(git, ["-C", repo, "rev-parse", "HEAD"], { encoding: "utf8" });
  if (result.status !== 0 || typeof result.stdout !== "string") return undefined;
  const revision = result.stdout.trim();
  return /^[0-9a-f]{40}(?:[0-9a-f]{24})?$/.test(revision) ? revision : undefined;
}

export interface BridgeHello {
  type: "hello";
  identity?: string;
  liveExecutions: number;
  pid: number;
}

export const BRIDGE_LAUNCH_ROLES = ["director", "implementer"] as const;
export type BridgeLaunchRole = typeof BRIDGE_LAUNCH_ROLES[number];

export const BRIDGE_LAUNCH_PROVIDERS = ["anthropic", "openai"] as const;
export type BridgeLaunchProvider = typeof BRIDGE_LAUNCH_PROVIDERS[number];

/** Absent means the daemon selects by headroom; a pin is always honored. */
export function parseBridgeLaunchProvider(value: unknown): BridgeLaunchProvider | undefined {
  if (value === undefined) return undefined;
  if (value === "anthropic" || value === "openai") return value;
  throw new Error("bridge launch provider must be anthropic or openai");
}

export function parseBridgeLaunchRole(value: unknown): BridgeLaunchRole {
  if (value === undefined) return "implementer";
  if (value === "director" || value === "implementer") return value;
  throw new Error("bridge launch role must be director or implementer");
}

export type BridgeRequest =
  | {
    op: "launch"; prompt: string; cwd: string; role: BridgeLaunchRole;
    provider?: BridgeLaunchProvider;
  }
  | { op: "attach"; executionId: string; cursor: number }
  | { op: "submitInput"; executionId: string; input: string }
  | { op: "interruptTurn"; executionId: string }
  | { op: "redirectNow"; executionId: string; input: string }
  | { op: "terminateSession"; executionId: string }
  | { op: "retire" };

export function bridgeStateDirectory(env: NodeJS.ProcessEnv = process.env): string {
  const configured = env.NORTH_BRIDGE_STATE_DIR?.trim();
  return configured ? resolve(configured) : join(homedir(), ".local/state/north/bridge");
}

export function bridgeSocketPath(env: NodeJS.ProcessEnv = process.env): string {
  return join(bridgeStateDirectory(env), "northd.sock");
}

export function bridgeJournalRoot(env: NodeJS.ProcessEnv = process.env): string {
  return join(bridgeStateDirectory(env), "journal");
}

export function parseBridgeRequest(value: unknown): BridgeRequest {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("bridge request must be an object");
  const request = value as Record<string, unknown>;
  if (request.op === "launch") {
    if (typeof request.prompt !== "string" || !request.prompt.trim())
      throw new Error("bridge launch requires a non-empty prompt");
    if (typeof request.cwd !== "string" || !request.cwd)
      throw new Error("bridge launch requires cwd");
    const provider = parseBridgeLaunchProvider(request.provider);
    return {
      op: "launch", prompt: request.prompt, cwd: request.cwd,
      role: parseBridgeLaunchRole(request.role),
      ...(provider ? { provider } : {}),
    };
  }
  if (request.op === "retire") return { op: "retire" };
  if (request.op === "attach") {
    if (typeof request.executionId !== "string" || !request.executionId)
      throw new Error("bridge attach requires an execution id");
    if (!Number.isSafeInteger(request.cursor) || (request.cursor as number) < 0)
      throw new Error("bridge attach cursor must be a non-negative integer");
    return { op: "attach", executionId: request.executionId, cursor: request.cursor as number };
  }
  if (["submitInput", "interruptTurn", "redirectNow", "terminateSession"].includes(String(request.op))) {
    if (typeof request.executionId !== "string" || !request.executionId)
      throw new Error(`bridge ${String(request.op)} requires an execution id`);
    if (request.op === "submitInput" || request.op === "redirectNow") {
      if (typeof request.input !== "string" || !request.input.trim())
        throw new Error(`bridge ${request.op} requires non-empty input`);
      return { op: request.op, executionId: request.executionId, input: request.input };
    }
    if (request.op === "interruptTurn")
      return { op: request.op, executionId: request.executionId };
    return { op: "terminateSession", executionId: request.executionId };
  }
  throw new Error("unknown bridge request");
}
