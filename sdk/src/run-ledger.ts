import * as path from "node:path";

import {
	beagleStoreBabashkaArguments,
	beagleStoreEnvironment,
	settleBeagleStoreCoordinatorChild,
} from "./beagle-store";
import {
	WIRE_MAX_EVENTS_PER_RUN,
	WIRE_VERSION,
	type WireEvent,
} from "./wire/events";
import type { WireRunId } from "./wire/ids";
import { encodeWireJsonlLine } from "./wire/jsonl";
import { reduceWireEvents, type WireRunSnapshot } from "./wire/reducer";

const REPO = path.resolve(import.meta.dir, "../..");
const CONTRACT_PATH = path.resolve(REPO, "contracts/agent-run-ledger-v2.json");
const INTERNAL_WRITER = path.resolve(REPO, "cli/run-event-internal.clj");
const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$/;
const WIRE_ID = /^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$/;
const ENTITY = /^@?[A-Za-z0-9][A-Za-z0-9_.:-]{0,255}$/;
const SHA256 = /^[a-f0-9]{64}$/;

interface WireLedgerContract {
	readonly version: string;
	readonly wireVersion: typeof WIRE_VERSION;
	readonly digest: Readonly<{
		algorithm: "sha256";
		eventInput: "utf8-canonical-wire-event-json";
		ledgerInput: "canonical-json-array-of-event-sha256";
	}>;
	readonly bounds: Readonly<{
		maxEventsPerRun: number;
		maxCanonicalEventBytes: number;
		maxBatchEvents: number;
		maxProjectionBatchBytes: number;
		maxTelemetryProjectionBytes: number;
	}>;
	readonly telemetry: Readonly<{
		estimateRatio: Readonly<{
			scale: 1_000_000;
			rounding: "nearest-half-up";
			trailingFractionZeros: "omit";
		}>;
	}>;
	readonly predicates: readonly string[];
}

export const AGENT_RUN_LEDGER_CONTRACT = Object.freeze(
	await Bun.file(CONTRACT_PATH).json() as WireLedgerContract,
);
export const AGENT_RUN_LEDGER_VERSION = AGENT_RUN_LEDGER_CONTRACT.version;

export type WireLedgerErrorCode =
	| "invalid_identity"
	| "invalid_event"
	| "invalid_batch"
	| "invalid_summary";

export class WireLedgerError extends Error {
	readonly code: WireLedgerErrorCode;

	constructor(code: WireLedgerErrorCode, message: string, options?: ErrorOptions) {
		super(message, options);
		this.name = "WireLedgerError";
		this.code = code;
	}
}

export interface WireRunLedgerIdentity {
	readonly thread: string;
	readonly agent: string;
	readonly parentThread?: string;
	readonly coordinator?: string;
}

export interface WireEventProjection {
	readonly subject: string;
	readonly facts: readonly (readonly [string, string])[];
}

export interface WireRunLedgerSummary {
	readonly version: string;
	readonly wireVersion: typeof WIRE_VERSION;
	readonly runId: WireRunId;
	readonly eventCount: number;
	readonly firstSequence: 0;
	readonly lastSequence: number;
	readonly terminalEventId: string;
	readonly digest: string;
}

export type WireLedgerPublicationStatus = "recorded" | "unavailable";
export type WireLedgerBatchWriter = (
	projections: readonly WireEventProjection[],
	timeoutMs: number,
) => Promise<WireLedgerPublicationStatus>;

/** Store-first publisher for one newly observed contiguous suffix. */
export interface WireEventStorePublisher {
	publish(events: readonly WireEvent[]): Promise<void>;
}

export interface WireEventStorePublisherOptions {
	readonly timeoutMs?: number;
	readonly writer?: WireLedgerBatchWriter;
}

function ledgerError(
	code: WireLedgerErrorCode,
	message: string,
	cause?: unknown,
): never {
	throw new WireLedgerError(code, message, cause === undefined ? undefined : { cause });
}

function canonicalEntity(value: string, label: string): string {
	if (!ENTITY.test(value)) ledgerError("invalid_identity", `invalid wire ledger ${label}`);
	return value.startsWith("@") ? value : `@${value}`;
}

export function wireRunLedgerIdentity(identity: WireRunLedgerIdentity): WireRunLedgerIdentity {
	if (!IDENTIFIER.test(identity.agent)) {
		ledgerError("invalid_identity", "invalid wire ledger agent");
	}
	const thread = identity.thread === "(ad-hoc)"
		? identity.thread
		: canonicalEntity(identity.thread, "thread");
	const parentThread = identity.parentThread === undefined
		? undefined : canonicalEntity(identity.parentThread, "parentThread");
	const coordinator = identity.coordinator?.replace(/^@agent:/u, "");
	if (coordinator !== undefined && !IDENTIFIER.test(coordinator)) {
		ledgerError("invalid_identity", "invalid wire ledger coordinator");
	}
	return Object.freeze({
		thread,
		agent: identity.agent,
		...(parentThread === undefined ? {} : { parentThread }),
		...(coordinator === undefined ? {} : { coordinator }),
	});
}

function sha256(value: string): string {
	return new Bun.CryptoHasher("sha256").update(value).digest("hex");
}

function canonicalStringArray(values: readonly string[]): string {
	return `[${values.map((value) => JSON.stringify(value)).join(",")}]`;
}

function canonicalEvent(event: WireEvent): { readonly event: WireEvent; readonly json: string } {
	try {
		const line = encodeWireJsonlLine(event, {
			maxLineBytes: AGENT_RUN_LEDGER_CONTRACT.bounds.maxCanonicalEventBytes,
		});
		return { event, json: line.slice(0, -1) };
	} catch (error) {
		return ledgerError("invalid_event", "wire ledger event is invalid or oversized", error);
	}
}

function eventSubject(runId: string, sequence: number): string {
	const digest = sha256(`north-wire-event-subject:v2\0${runId}\0${sequence}`);
	return `@run:wire-event-${digest}`;
}

export function wireEventFacts(
	identity: WireRunLedgerIdentity,
	event: WireEvent,
): WireEventProjection {
	const context = wireRunLedgerIdentity(identity);
	const canonical = canonicalEvent(event);
	const digest = sha256(canonical.json);
	const facts: Array<readonly [string, string]> = [
		["kind", "wire_event"],
		["wire_ledger_version", AGENT_RUN_LEDGER_VERSION],
		["wire_version", canonical.event.version],
		["wire_run_id", canonical.event.runId],
		["thread", context.thread],
		["agent", context.agent],
		["wire_event_id", canonical.event.id],
		["wire_event_sequence", String(canonical.event.sequence)],
		["wire_event_at", canonical.event.at],
		["wire_event_kind", canonical.event.kind],
		["wire_event_essential", String(canonical.event.essential)],
		["wire_event_json", canonical.json],
		["wire_event_sha256", digest],
	];
	if (context.parentThread !== undefined) facts.push(["parent_thread", context.parentThread]);
	if (context.coordinator !== undefined) facts.push(["run_coordinator", context.coordinator]);
	return Object.freeze({
		subject: eventSubject(canonical.event.runId, canonical.event.sequence),
		facts: Object.freeze(facts),
	});
}

function validateEventSlice(events: readonly WireEvent[]): void {
	if (events.length === 0) ledgerError("invalid_batch", "wire ledger batch must not be empty");
	if (events.length > AGENT_RUN_LEDGER_CONTRACT.bounds.maxBatchEvents) {
		ledgerError(
			"invalid_batch",
			`wire ledger batch exceeds ${AGENT_RUN_LEDGER_CONTRACT.bounds.maxBatchEvents} events`,
		);
	}
	const first = events[0]!;
	for (let index = 0; index < events.length; index += 1) {
		const event = events[index]!;
		canonicalEvent(event);
		if (event.runId !== first.runId || event.sequence !== first.sequence + index) {
			ledgerError("invalid_batch", "wire ledger batch must be one contiguous run slice");
		}
		if (index < events.length - 1 && event.kind === "run.terminated") {
			ledgerError("invalid_batch", "wire ledger batch cannot continue after run.terminated");
		}
	}
}

function validateBatch(events: readonly WireEvent[]): void {
	validateEventSlice(events);
	const first = events[0]!;
	if (first.sequence !== 0 || first.kind !== "run.started"
		|| events.at(-1)!.kind !== "run.terminated") {
		ledgerError("invalid_batch", "wire ledger publication requires one complete terminal run");
	}
	try {
		wireLedgerSummary(events);
	} catch (error) {
		ledgerError("invalid_batch", "wire ledger publication requires a reducible terminal run", error);
	}
}

export function wireLedgerSummary(events: readonly WireEvent[]): WireRunLedgerSummary {
	if (events.length === 0 || events.length > WIRE_MAX_EVENTS_PER_RUN) {
		return ledgerError("invalid_summary", "wire ledger summary requires a bounded event sequence");
	}
	let snapshot: WireRunSnapshot;
	try {
		snapshot = reduceWireEvents(events);
	} catch (error) {
		return ledgerError("invalid_summary", "wire ledger summary requires a valid event sequence", error);
	}
	const terminal = events.at(-1)!;
	if (terminal.kind !== "run.terminated"
		|| !["completed", "failed", "cancelled", "blocked"].includes(snapshot.lifecycle)) {
		return ledgerError("invalid_summary", "wire ledger summary requires run.terminated last");
	}
	const digests = events.map((event) => sha256(canonicalEvent(event).json));
	return Object.freeze({
		version: AGENT_RUN_LEDGER_VERSION,
		wireVersion: WIRE_VERSION,
		runId: snapshot.runId,
		eventCount: events.length,
		firstSequence: 0,
		lastSequence: terminal.sequence,
		terminalEventId: terminal.id,
		digest: sha256(canonicalStringArray(digests)),
	});
}

async function runWriter(
	projections: readonly WireEventProjection[],
	timeoutMs: number,
	env: NodeJS.ProcessEnv,
): Promise<WireLedgerPublicationStatus> {
	const payload = projectionPayload(projections);
	const child = Bun.spawn([
		"bb",
		...beagleStoreBabashkaArguments([
			INTERNAL_WRITER,
			env.NORTH_PORT ?? "7977",
		], env),
	], {
		env: beagleStoreEnvironment(env),
		stdin: "pipe",
		stdout: "ignore",
		stderr: "ignore",
	});
	child.stdin.write(payload);
	child.stdin.end();
	const outcome = await settleBeagleStoreCoordinatorChild(child, timeoutMs);
	return !outcome.timedOut && outcome.exitCode === 0 ? "recorded" : "unavailable";
}

function projectionPayload(projections: readonly WireEventProjection[]): string {
	const payload = JSON.stringify(projections);
	if (new TextEncoder().encode(payload).byteLength
		> AGENT_RUN_LEDGER_CONTRACT.bounds.maxProjectionBatchBytes) {
		ledgerError("invalid_batch", "wire ledger projection batch exceeds its byte bound");
	}
	return payload;
}

export async function recordWireEventProjections(
	projections: readonly WireEventProjection[],
	timeoutMs = 10_000,
	env: NodeJS.ProcessEnv = process.env,
): Promise<WireLedgerPublicationStatus> {
	if (projections.length === 0) {
		ledgerError("invalid_batch", "wire ledger projection batch must not be empty");
	}
	try {
		return await runWriter(projections, timeoutMs, env);
	} catch {
		return "unavailable";
	}
}

/**
 * Creates the Store acknowledgement barrier used while a provider is still
 * producing wire events. Callers must give this only the next contiguous
 * canonical suffix; an unavailable acknowledgement rejects rather than
 * allowing JSONL to become the publication barrier.
 */
export function createWireEventStorePublisher(
	identity: WireRunLedgerIdentity,
	options: WireEventStorePublisherOptions = {},
): WireEventStorePublisher {
	const context = wireRunLedgerIdentity(identity);
	const timeoutMs = options.timeoutMs ?? 10_000;
	const writer = options.writer ?? recordWireEventProjections;
	let nextSequence = 0;
	let poisoned: unknown;
	return Object.freeze({
		async publish(events: readonly WireEvent[]): Promise<void> {
			if (poisoned !== undefined) throw poisoned;
			try {
				validateEventSlice(events);
				if (events[0]!.sequence !== nextSequence) {
					ledgerError("invalid_batch", "wire event Store suffix does not begin at the next sequence");
				}
				const projections = Object.freeze(events.map((event) => wireEventFacts(context, event)));
				projectionPayload(projections);
				if (await writer(projections, timeoutMs) !== "recorded") {
					throw new WireLedgerError("invalid_batch", "wire event Store publication is unavailable");
				}
				nextSequence += events.length;
			} catch (error) {
				poisoned = error;
				throw error;
			}
		},
	});
}

export async function publishWireEvents(
	identity: WireRunLedgerIdentity,
	events: readonly WireEvent[],
	timeoutMs = 10_000,
	writer: WireLedgerBatchWriter = recordWireEventProjections,
): Promise<WireLedgerPublicationStatus> {
	validateBatch(events);
	const projections = Object.freeze(events.map((event) => wireEventFacts(identity, event)));
	projectionPayload(projections);
	return writer(projections, timeoutMs);
}

export function isWireRunLedgerSummary(value: unknown): value is WireRunLedgerSummary {
	if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
	const summary = value as Partial<WireRunLedgerSummary>;
	return summary.version === AGENT_RUN_LEDGER_VERSION
		&& summary.wireVersion === WIRE_VERSION
		&& typeof summary.runId === "string"
		&& WIRE_ID.test(summary.runId)
		&& Number.isSafeInteger(summary.eventCount) && summary.eventCount! > 0
		&& summary.firstSequence === 0
		&& Number.isSafeInteger(summary.lastSequence)
		&& summary.lastSequence === summary.eventCount! - 1
		&& typeof summary.terminalEventId === "string"
		&& WIRE_ID.test(summary.terminalEventId)
		&& typeof summary.digest === "string"
		&& SHA256.test(summary.digest);
}
