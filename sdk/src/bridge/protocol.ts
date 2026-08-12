import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import type { WireEvent } from "../wire/events";
import type { JournalRecord, TornTail } from "./journal";

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
  /**
   * The sessions that hold retirement open — live minus the abandoned control
   * sessions nobody is attached to. Optional on the wire: a daemon from before
   * this field answers with liveExecutions alone, and the client falls back to
   * it rather than replacing a daemon on a count it did not send.
   */
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

/** What the replacement gate asks a hello. */
export function pinningExecutions(hello: BridgeHello): number {
  return hello.pinningExecutions ?? hello.liveExecutions;
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
    executionId?: string;
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

function parseExecutionId(value: unknown, operation: string): string {
  if (typeof value !== "string"
    || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/.test(value)
    || value === "." || value === "..") {
    throw new Error(`bridge ${operation} requires a safe execution id`);
  }
  return value;
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
    const executionId = request.executionId === undefined
      ? undefined
      : parseBridgeLaunchExecutionId(request.executionId);
    return {
      op: "launch", prompt: request.prompt, cwd: request.cwd,
      role: parseBridgeLaunchRole(request.role),
      ...(executionId ? { executionId } : {}),
      ...(provider ? { provider } : {}),
    };
  }
  if (request.op === "retire") return { op: "retire" };
  if (request.op === "attach") {
    const executionId = parseExecutionId(request.executionId, "attach");
    if (!Number.isSafeInteger(request.cursor) || (request.cursor as number) < 0)
      throw new Error("bridge attach cursor must be a non-negative integer");
    return { op: "attach", executionId, cursor: request.cursor as number };
  }
  if (["submitInput", "interruptTurn", "redirectNow", "terminateSession"].includes(String(request.op))) {
    const executionId = parseExecutionId(request.executionId, String(request.op));
    if (request.op === "submitInput" || request.op === "redirectNow") {
      if (typeof request.input !== "string" || !request.input.trim())
        throw new Error(`bridge ${request.op} requires non-empty input`);
      return { op: request.op, executionId, input: request.input };
    }
    if (request.op === "interruptTurn")
      return { op: request.op, executionId };
    return { op: "terminateSession", executionId };
  }
  throw new Error("unknown bridge request");
}

export function parseBridgeLaunchExecutionId(value: unknown): string {
  if (typeof value !== "string"
    || !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(value)) {
    throw new Error("bridge launch execution id must be a UUIDv4");
  }
  return value.toLowerCase();
}
