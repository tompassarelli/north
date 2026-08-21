import {
	closeSync,
	fstatSync,
	lstatSync,
	openSync,
	readFileSync,
	readSync,
} from "node:fs";
import { join } from "node:path";

import {
	publishWireEvents,
	recordWireEventProjections,
	wireLedgerSummary,
	type WireLedgerBatchWriter,
} from "./run-ledger";
import {
	recordWireRunTelemetry,
	recordWireRunTelemetryProjection,
	type WireRunTelemetryWriter,
} from "./telemetry";
import type { WireTerminationInput } from "./wire/writer";
import { wireEventId, wireRunId } from "./wire/ids";
import { WireEventWriter } from "./wire/writer";

const OPAQUE_PROVIDER_ID = /^[A-Za-z0-9._:-]+$/u;
const NORTH_AGENT_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,511}$/u;
const MAX_PROVIDER_ID_BYTES = 512;
const MAX_HOOK_BYTES = 64 * 1024;
const MAX_TRANSCRIPT_EDGE_BYTES = 8 * 1024 * 1024;
const DEFAULT_PUBLICATION_TIMEOUT_MS = 800;
const INTERRUPTED_REASONS = new Set(["cancelled", "interrupted", "user_cancelled"]);

type JsonRecord = Readonly<Record<string, unknown>>;

export type ProviderNativeTerminalStatus =
	| Readonly<{ status: "recorded"; runId: string; processOutcome: string }>
	| Readonly<{ status: "unknown"; reason: "identity" | "terminal" }>
	| Readonly<{ status: "bypassed"; reason: "managed" }>
	| Readonly<{ status: "unavailable" }>;

export interface ProviderNativeTerminalDependencies {
	readonly env?: NodeJS.ProcessEnv;
	readonly runtimeDir?: string;
	readonly ledgerTimeoutMs?: number;
	readonly telemetryTimeoutMs?: number;
	readonly ledgerWriter?: WireLedgerBatchWriter;
	readonly telemetryWriter?: WireRunTelemetryWriter;
}

interface CodexHookInput {
	readonly event: "SessionEnd" | "SubagentStop";
	readonly sessionId: string;
	readonly actorNamespace: "session" | "agent";
	readonly actorId: string;
	readonly transcriptPath: string;
}

interface TerminalEvidence {
	readonly startedAt: string;
	readonly terminatedAt: string;
	readonly termination: WireTerminationInput;
	readonly processOutcome: "ran" | "aborted" | "provider_error";
}

function record(value: unknown): JsonRecord | undefined {
	return value !== null && typeof value === "object" && !Array.isArray(value)
		? value as JsonRecord : undefined;
}

function boundedOpaqueId(value: unknown): string | undefined {
	return typeof value === "string"
		&& Buffer.byteLength(value, "utf8") <= MAX_PROVIDER_ID_BYTES
		&& OPAQUE_PROVIDER_ID.test(value)
		? value : undefined;
}

function instant(value: unknown): string | undefined {
	if (typeof value !== "string") return undefined;
	const milliseconds = Date.parse(value);
	return Number.isFinite(milliseconds) ? new Date(milliseconds).toISOString() : undefined;
}

function sha256(parts: readonly string[]): string {
	const hash = new Bun.CryptoHasher("sha256");
	for (const part of parts) hash.update(part);
	return hash.digest("hex");
}

export function providerNativeActorKey(
	namespace: "session" | "agent",
	actorId: string,
): string {
	const id = boundedOpaqueId(actorId);
	if (id === undefined) throw new TypeError("provider-native actor id is invalid");
	return sha256(["north-actor-key-v1\0", namespace, "\0", id]);
}

function parseHookInput(value: unknown): CodexHookInput | undefined {
	const input = record(value);
	if (input === undefined) return undefined;
	const event = input.hook_event_name;
	const sessionId = boundedOpaqueId(input.session_id);
	if ((event !== "SessionEnd" && event !== "SubagentStop")
		|| sessionId === undefined) {
		return undefined;
	}
	const transcriptPath = event === "SubagentStop"
		? input.agent_transcript_path : input.transcript_path;
	if (typeof transcriptPath !== "string"
		|| !transcriptPath.startsWith("/")
		|| Buffer.byteLength(transcriptPath, "utf8") > 4096
		|| /[\0\n\r\t]/u.test(transcriptPath)) {
		return undefined;
	}
	if (event === "SessionEnd") {
		return { event, sessionId, actorNamespace: "session", actorId: sessionId, transcriptPath };
	}
	const agentId = boundedOpaqueId(input.agent_id);
	return agentId === undefined ? undefined : {
		event,
		sessionId,
		actorNamespace: "agent",
		actorId: agentId,
		transcriptPath,
	};
}

function managedLane(env: NodeJS.ProcessEnv): boolean {
	return env.NORTH_MANAGED_LANE === "1"
		&& (env.AGENT_TOPOLOGY === "worker" || env.AGENT_TOPOLOGY === "orchestrator")
		&& NORTH_AGENT_ID.test(env.AGENT_ID ?? "");
}

function parseJsonLine(line: string): JsonRecord | undefined {
	try {
		return record(JSON.parse(line));
	} catch {
		return undefined;
	}
}

function transcriptLines(path: string): { readonly first: readonly string[]; readonly tail: readonly string[] } {
	const info = lstatSync(path);
	if (!info.isFile() || info.isSymbolicLink()) throw new Error("transcript is not a regular file");
	const fd = openSync(path, "r");
	try {
		const size = fstatSync(fd).size;
		if (size < 1) throw new Error("transcript is empty");
		const firstBytes = Math.min(size, 256 * 1024);
		const firstBuffer = Buffer.alloc(firstBytes);
		readSync(fd, firstBuffer, 0, firstBytes, 0);
		const tailBytes = Math.min(size, MAX_TRANSCRIPT_EDGE_BYTES);
		const tailOffset = size - tailBytes;
		const tailBuffer = Buffer.alloc(tailBytes);
		readSync(fd, tailBuffer, 0, tailBytes, tailOffset);
		const first = firstBuffer.toString("utf8").split("\n").slice(0, 32);
		const tail = tailBuffer.toString("utf8").split("\n");
		if (tailOffset > 0) tail.shift();
		return { first, tail };
	} finally {
		closeSync(fd);
	}
}

function transcriptEvidence(input: CodexHookInput): TerminalEvidence | undefined {
	const lines = transcriptLines(input.transcriptPath);
	const firstRecords = lines.first.map(parseJsonLine).filter((entry): entry is JsonRecord => entry !== undefined);
	const startedAt = firstRecords.map((entry) => instant(entry.timestamp)).find(
		(value): value is string => value !== undefined,
	);
	const matchingMeta = firstRecords.some((entry) => {
		if (entry.type !== "session_meta") return false;
		const payload = record(entry.payload);
		return payload?.id === input.actorId;
	});
	if (startedAt === undefined || !matchingMeta) return undefined;

	let boundary: JsonRecord | undefined;
	for (const line of lines.tail) {
		const entry = parseJsonLine(line);
		const payload = record(entry?.payload);
		if (entry?.type !== "event_msg" || payload === undefined) continue;
		if (["task_started", "task_complete", "task_failed", "turn_aborted"].includes(
			String(payload.type),
		)) {
			boundary = entry;
		}
	}
	const payload = record(boundary?.payload);
	if (payload === undefined) return undefined;
	const kind = payload.type;
	const terminatedAt = instant(boundary?.timestamp);
	if (terminatedAt === undefined || Date.parse(terminatedAt) < Date.parse(startedAt)) return undefined;
	if (kind === "task_complete") {
		return {
			startedAt,
			terminatedAt,
			termination: { lifecycle: "completed", reason: { code: "completed" } },
			processOutcome: "ran",
		};
	}
	if (kind === "turn_aborted") {
		const reason = typeof payload.reason === "string" ? payload.reason : "";
		if (INTERRUPTED_REASONS.has(reason)) {
			return {
				startedAt,
				terminatedAt,
				termination: {
					lifecycle: "cancelled",
					reason: { code: "aborted" },
					abort: {
						requestedAt: terminatedAt,
						source: "provider",
						reason: "provider reported an interrupted direct turn",
					},
				},
				processOutcome: "aborted",
			};
		}
		return {
			startedAt,
			terminatedAt,
			termination: { lifecycle: "failed", reason: { code: "provider_error" } },
			processOutcome: "provider_error",
		};
	}
	if (kind === "task_failed") {
		return {
			startedAt,
			terminatedAt,
			termination: { lifecycle: "failed", reason: { code: "provider_error" } },
			processOutcome: "provider_error",
		};
	}
	return undefined;
}

function positiveTimeout(value: number | undefined): number {
	return Number.isSafeInteger(value) && value! > 0
		? value! : DEFAULT_PUBLICATION_TIMEOUT_MS;
}

function exactCachedIdentity(runtimeDir: string, actorKey: string): string | undefined {
	const path = join(runtimeDir, "north-agent-ids", actorKey);
	try {
		const info = lstatSync(path);
		if (!info.isFile() || info.isSymbolicLink() || info.size > 513) return undefined;
		const id = readFileSync(path, "utf8").trimEnd();
		return NORTH_AGENT_ID.test(id) ? id : undefined;
	} catch {
		return undefined;
	}
}

export async function recordCodexProviderNativeTerminal(
	value: unknown,
	dependencies: ProviderNativeTerminalDependencies = {},
): Promise<ProviderNativeTerminalStatus> {
	const env = dependencies.env ?? process.env;
	if (managedLane(env)) return { status: "bypassed", reason: "managed" };
	try {
		const input = parseHookInput(value);
		if (input === undefined) return { status: "unknown", reason: "terminal" };
		const actorKey = providerNativeActorKey(input.actorNamespace, input.actorId);
		const runtimeDir = dependencies.runtimeDir ?? env.XDG_RUNTIME_DIR ?? "/tmp";
		const agent = exactCachedIdentity(runtimeDir, actorKey);
		if (agent === undefined) return { status: "unknown", reason: "identity" };
		const evidence = transcriptEvidence(input);
		if (evidence === undefined) return { status: "unknown", reason: "terminal" };

		const runId = wireRunId(`run:native-${actorKey}`);
		const instants = [evidence.startedAt, evidence.terminatedAt];
		let instantIndex = 0;
		const writer = new WireEventWriter({
			runId,
			eventId: (sequence) => wireEventId(`event:native-${actorKey}:${sequence}`),
			now: () => instants[Math.min(instantIndex++, instants.length - 1)]!,
		});
		writer.append({ kind: "run.started", lifecycle: "running", owner: agent });
		writer.terminate(evidence.termination);
		const events = writer.events();
		const identity = { thread: "(ad-hoc)", agent } as const;
		const ledgerStatus = await publishWireEvents(
			identity,
			events,
			positiveTimeout(dependencies.ledgerTimeoutMs),
			dependencies.ledgerWriter ?? recordWireEventProjections,
		);
		if (ledgerStatus !== "recorded") return { status: "unavailable" };
		const snapshot = writer.snapshot();
		if (snapshot === undefined) return { status: "unavailable" };
		const telemetryStatus = await recordWireRunTelemetry(
			identity,
			snapshot,
			{ status: "recorded", summary: wireLedgerSummary(events) },
			{
				provider: "openai",
				processOutcome: evidence.processOutcome,
				executionSource: "provider-native",
			},
			positiveTimeout(dependencies.telemetryTimeoutMs),
			dependencies.telemetryWriter ?? recordWireRunTelemetryProjection,
		);
		return telemetryStatus === "recorded"
			? { status: "recorded", runId, processOutcome: evidence.processOutcome }
			: { status: "unavailable" };
	} catch {
		return { status: "unavailable" };
	}
}

async function main(): Promise<void> {
	let raw: string;
	try {
		raw = await Bun.stdin.text();
	} catch {
		return;
	}
	if (Buffer.byteLength(raw, "utf8") > MAX_HOOK_BYTES) return;
	let input: unknown;
	try {
		input = JSON.parse(raw);
	} catch {
		return;
	}
	await recordCodexProviderNativeTerminal(input);
}

if (import.meta.main) await main();
