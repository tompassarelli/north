import { createHash } from "node:crypto";
import type { ProviderId } from "./types";

export const PROVIDER_JOIN_KEY_VERSION = "north-provider-join:v1" as const;

export type ProviderSessionPersistence = "persisted" | "ephemeral" | "unknown";

/**
 * Privacy-bounded provider execution identity. Raw provider session and turn
 * identifiers remain inside the adapter process and are never written to the
 * North graph.
 */
export interface ProviderJoinEvidence {
  version: typeof PROVIDER_JOIN_KEY_VERSION;
  sessionKey?: string;
  turnKeys: string[];
  sessionPersistence: ProviderSessionPersistence;
  coverage: "exact" | "partial" | "unknown";
}

export interface ProviderJoinTerminalMessage {
  _north_provider_join?: ProviderJoinEvidence;
}

const OPAQUE_PROVIDER_ID = /^[A-Za-z0-9._:-]+$/;
const SHA256 = /^[a-f0-9]{64}$/;

function opaqueProviderId(value: unknown, label: string): string {
  if (typeof value !== "string" || !value || value !== value.trim()
      || Buffer.byteLength(value, "utf8") > 512 || !OPAQUE_PROVIDER_ID.test(value))
    throw new Error(`${label} is not a bounded opaque provider identifier`);
  return value;
}

function digest(parts: readonly string[]): string {
  const hash = createHash("sha256");
  for (const part of parts) hash.update(part);
  return hash.digest("hex");
}

/**
 * This deliberately reuses the deployed native actor session-key domain.
 * Existing @agent:native-<sha256> rows can therefore join provider logs without
 * exposing the provider's raw session identifier or requiring a migration.
 */
export function providerSessionKey(providerSessionId: unknown): string {
  const id = opaqueProviderId(providerSessionId, "provider session id");
  return digest(["north-actor-key-v1\0session\0", id]);
}

export function providerTurnKey(provider: ProviderId, providerTurnId: unknown): string {
  const id = opaqueProviderId(providerTurnId, "provider turn id");
  return digest(["north-provider-turn-key-v1\0", provider, "\0", id]);
}

export function providerJoinEvidence(
  provider: ProviderId,
  input: {
    sessionId?: unknown;
    turnIds?: readonly unknown[];
    sessionPersistence: ProviderSessionPersistence;
  },
): ProviderJoinEvidence {
  const sessionKey = input.sessionId === undefined
    ? undefined : providerSessionKey(input.sessionId);
  const turnKeys = [...new Set((input.turnIds ?? [])
    .map((id) => providerTurnKey(provider, id)))].sort();
  return Object.freeze({
    version: PROVIDER_JOIN_KEY_VERSION,
    ...(sessionKey ? { sessionKey } : {}),
    turnKeys: Object.freeze(turnKeys) as unknown as string[],
    sessionPersistence: input.sessionPersistence,
    coverage: sessionKey && turnKeys.length ? "exact"
      : sessionKey || turnKeys.length ? "partial" : "unknown",
  });
}

function validEvidence(value: unknown): value is ProviderJoinEvidence {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const evidence = value as ProviderJoinEvidence;
  return evidence.version === PROVIDER_JOIN_KEY_VERSION
    && (evidence.sessionKey === undefined || SHA256.test(evidence.sessionKey))
    && Array.isArray(evidence.turnKeys)
    && evidence.turnKeys.every((key) => typeof key === "string" && SHA256.test(key))
    && new Set(evidence.turnKeys).size === evidence.turnKeys.length
    && ["persisted", "ephemeral", "unknown"].includes(evidence.sessionPersistence)
    && ["exact", "partial", "unknown"].includes(evidence.coverage);
}

/** Fold every provider terminal belonging to one North run into one join set. */
export function collectProviderJoinEvidence(
  terminals: readonly ProviderJoinTerminalMessage[],
): ProviderJoinEvidence | undefined {
  const evidence = terminals.map((terminal) => terminal?._north_provider_join)
    .filter(validEvidence);
  if (!evidence.length) return undefined;
  const sessions = new Set(evidence.map((entry) => entry.sessionKey).filter(Boolean));
  const persistences = new Set(evidence.map((entry) => entry.sessionPersistence));
  if (sessions.size > 1 || persistences.size > 1) return undefined;
  const sessionKey = sessions.values().next().value as string | undefined;
  const turnKeys = [...new Set(evidence.flatMap((entry) => entry.turnKeys))].sort();
  return Object.freeze({
    version: PROVIDER_JOIN_KEY_VERSION,
    ...(sessionKey ? { sessionKey } : {}),
    turnKeys: Object.freeze(turnKeys) as unknown as string[],
    sessionPersistence: evidence[0]!.sessionPersistence,
    coverage: sessionKey && turnKeys.length ? "exact"
      : sessionKey || turnKeys.length ? "partial" : "unknown",
  });
}

// The Babashka usage report delegates raw-ID hashing here in one bounded batch,
// so managed adapters and provider-log ingestion cannot drift into two key
// algorithms. Input and output are positional; raw IDs never reach the graph.
if (import.meta.main) {
  const input = JSON.parse(await Bun.stdin.text()) as {
    sessions?: unknown[];
    turns?: Array<{ provider: ProviderId; id: unknown }>;
  };
  const sessions = Array.isArray(input.sessions)
    ? input.sessions.map(providerSessionKey) : [];
  const turns = Array.isArray(input.turns)
    ? input.turns.map(({ provider, id }) => providerTurnKey(provider, id)) : [];
  process.stdout.write(`${JSON.stringify({ version: PROVIDER_JOIN_KEY_VERSION, sessions, turns })}\n`);
}
