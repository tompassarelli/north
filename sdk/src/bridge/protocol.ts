import { homedir } from "node:os";
import { join, resolve } from "node:path";

export type BridgeRequest =
  | { op: "launch"; prompt: string; cwd: string }
  | { op: "attach"; executionId: string; cursor: number }
  | { op: "submitInput"; executionId: string; input: string }
  | { op: "interruptTurn"; executionId: string }
  | { op: "redirectNow"; executionId: string; input: string }
  | { op: "terminateSession"; executionId: string };

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
    return { op: "launch", prompt: request.prompt, cwd: request.cwd };
  }
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
