import {
	WIRE_PROVIDER_JOIN_VERSION,
	type WireProviderJoinEvidence,
} from "../wire/events";
import type { ProviderId } from "./types";

export const PROVIDER_JOIN_KEY_VERSION = WIRE_PROVIDER_JOIN_VERSION;

const OPAQUE_PROVIDER_ID = /^[A-Za-z0-9._:-]+$/;

function opaqueProviderId(value: unknown, label: string): string {
	if (typeof value !== "string" || !value || value !== value.trim()
		|| Buffer.byteLength(value, "utf8") > 512 || !OPAQUE_PROVIDER_ID.test(value)) {
		throw new Error(`${label} is not a bounded opaque provider identifier`);
	}
	return value;
}

function digest(parts: readonly string[]): string {
	const hash = new Bun.CryptoHasher("sha256");
	for (const part of parts) hash.update(part);
	return hash.digest("hex");
}

/**
 * This deliberately reuses the deployed native actor session-key domain.
 * Raw provider identifiers remain adapter-private; only this digest crosses
 * the wire boundary.
 */
export function providerSessionKey(providerSessionId: unknown): string {
	const id = opaqueProviderId(providerSessionId, "provider session id");
	return digest(["north-actor-key-v1\0session\0", id]);
}

export function providerTurnKey(provider: ProviderId, providerTurnId: unknown): string {
	const id = opaqueProviderId(providerTurnId, "provider turn id");
	return digest(["north-provider-turn-key-v1\0", provider, "\0", id]);
}

/** Build wire-safe join evidence at the adapter's raw-identity boundary. */
export function providerJoinEvidence(
	provider: ProviderId,
	input: {
		sessionId?: unknown;
		turnIds?: readonly unknown[];
		sessionPersistence: WireProviderJoinEvidence["sessionPersistence"];
	},
): WireProviderJoinEvidence {
	const sessionKey = input.sessionId === undefined
		? undefined : providerSessionKey(input.sessionId);
	const turnKeys = Object.freeze([...new Set((input.turnIds ?? [])
		.map((id) => providerTurnKey(provider, id)))].sort());
	return Object.freeze({
		version: WIRE_PROVIDER_JOIN_VERSION,
		...(sessionKey === undefined ? {} : { sessionKey }),
		turnKeys,
		sessionPersistence: input.sessionPersistence,
		coverage: sessionKey && turnKeys.length > 0 ? "exact"
			: sessionKey || turnKeys.length > 0 ? "partial" : "unknown",
	});
}

/** Fold validated completion evidence belonging to one North run. */
export function foldProviderJoinEvidence(
	evidence: readonly WireProviderJoinEvidence[],
): WireProviderJoinEvidence | undefined {
	if (evidence.length === 0) return undefined;
	const sessions = new Set(evidence
		.map((entry) => entry.sessionKey)
		.filter((sessionKey): sessionKey is string => sessionKey !== undefined));
	const persistences = new Set(evidence.map((entry) => entry.sessionPersistence));
	const sessionKey = sessions.size === 1 ? sessions.values().next().value : undefined;
	const sessionPersistence = persistences.size === 1
		? evidence[0]!.sessionPersistence : "unknown";
	const turnKeys = Object.freeze([...new Set(evidence.flatMap((entry) => entry.turnKeys))].sort());
	const coverage: WireProviderJoinEvidence["coverage"] = evidence.some(
		(entry) => entry.coverage === "unknown",
	) ? "unknown"
		: sessions.size <= 1 && persistences.size <= 1
			&& sessionKey !== undefined && turnKeys.length > 0
			&& evidence.every((entry) => entry.coverage === "exact")
			? "exact"
			: sessionKey !== undefined || turnKeys.length > 0 ? "partial" : "unknown";
	return Object.freeze({
		version: WIRE_PROVIDER_JOIN_VERSION,
		...(sessionKey === undefined ? {} : { sessionKey }),
		turnKeys,
		sessionPersistence,
		coverage,
	});
}

function providerId(value: unknown): ProviderId {
	if (value !== "anthropic" && value !== "openai") {
		throw new Error("provider turn entry requires anthropic|openai");
	}
	return value;
}

function record(value: unknown, label: string): Readonly<Record<string, unknown>> {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new Error(`${label} must be an object`);
	}
	return value as Readonly<Record<string, unknown>>;
}

// Babashka usage reporting delegates raw-ID hashing here in one bounded batch.
// Input and output are positional; raw IDs never reach the graph.
if (import.meta.main) {
	const input = record(JSON.parse(await Bun.stdin.text()), "provider join input");
	const sessions = Array.isArray(input.sessions)
		? input.sessions.map(providerSessionKey) : [];
	const turns = Array.isArray(input.turns)
		? input.turns.map((entry) => {
			const turn = record(entry, "provider turn entry");
			return providerTurnKey(providerId(turn.provider), turn.id);
		})
		: [];
	process.stdout.write(`${JSON.stringify({ version: PROVIDER_JOIN_KEY_VERSION, sessions, turns })}\n`);
}
