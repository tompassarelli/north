import { randomUUID } from "node:crypto";
import {
  chmodSync, mkdirSync, readFileSync, renameSync, unlinkSync, writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { validAgentEntity } from "./delivery-verification";

export interface PresenceFence {
  resource: string;
  holder: string;
  epoch: number;
}

const FENCE_KEYS = ["epoch", "holder", "resource"] as const;

function bareAgentId(agentId: string): string {
  const bare = agentId.replace(/^@?agent:/, "");
  if (!validAgentEntity(`@agent:${bare}`))
    throw new Error("presence fence requires a valid bare agent id");
  return bare;
}

export function canonicalPresenceFence(
  value: unknown,
  expectedAgentId?: string,
): PresenceFence {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    throw new Error("presence fence must be an object");
  const record = value as Record<string, unknown>;
  if (Object.keys(record).sort().join("\0") !== FENCE_KEYS.join("\0"))
    throw new Error("presence fence must contain exactly resource, holder, and epoch");
  if (typeof record.resource !== "string" || typeof record.holder !== "string"
      || !Number.isSafeInteger(record.epoch) || (record.epoch as number) <= 0)
    throw new Error("presence fence fields are malformed");
  const fence: PresenceFence = {
    resource: record.resource,
    holder: record.holder,
    epoch: record.epoch as number,
  };
  if (expectedAgentId !== undefined) {
    const bare = bareAgentId(expectedAgentId);
    if (fence.resource !== `session:${bare}` || fence.holder !== bare)
      throw new Error("presence fence does not belong to the managed agent");
  }
  return fence;
}

export function parsePresenceFence(raw: string, expectedAgentId?: string): PresenceFence {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw.trim());
  } catch {
    throw new Error("presence fence is not canonical JSON");
  }
  return canonicalPresenceFence(parsed, expectedAgentId);
}

export function presenceFenceJson(fence: PresenceFence): string {
  return JSON.stringify(canonicalPresenceFence(fence));
}

export function presenceFencePath(
  agentId: string,
  env: NodeJS.ProcessEnv = process.env,
): string {
  const bare = bareAgentId(agentId);
  const directory = env.NORTH_AGENT_LOGS_DIR
    ?? (env.NORTH_IDENTITY_TEST_REDIRECT === "1" && env.NORTH_STREAM_DIR
      ? env.NORTH_STREAM_DIR : undefined)
    ?? join(env.HOME || homedir(), ".local/state/north/agents");
  return join(directory, `${bare}.presence-fence.json`);
}

export function persistPresenceFence(
  agentId: string,
  fence: PresenceFence,
  env: NodeJS.ProcessEnv = process.env,
): PresenceFence {
  const canonical = canonicalPresenceFence(fence, agentId);
  const target = presenceFencePath(agentId, env);
  const directory = dirname(target);
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const temporary = `${target}.${randomUUID()}.tmp`;
  try {
    writeFileSync(temporary, `${presenceFenceJson(canonical)}\n`, {
      encoding: "utf8",
      mode: 0o600,
    });
    chmodSync(temporary, 0o600);
    renameSync(temporary, target);
  } catch (error) {
    try { unlinkSync(temporary); } catch { /* the temporary may not exist */ }
    throw error;
  }
  return canonical;
}

export function loadPresenceFence(
  agentId: string,
  env: NodeJS.ProcessEnv = process.env,
): PresenceFence {
  return parsePresenceFence(readFileSync(presenceFencePath(agentId, env), "utf8"), agentId);
}

export function removePresenceFence(
  agentId: string,
  expected: PresenceFence,
  env: NodeJS.ProcessEnv = process.env,
): boolean {
  const canonical = canonicalPresenceFence(expected, agentId);
  let current: PresenceFence;
  try {
    current = loadPresenceFence(agentId, env);
  } catch (error) {
    return (error as NodeJS.ErrnoException)?.code === "ENOENT";
  }
  if (presenceFenceJson(current) !== presenceFenceJson(canonical)) return false;
  try {
    unlinkSync(presenceFencePath(agentId, env));
    return true;
  } catch {
    return false;
  }
}
