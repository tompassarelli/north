import {
	expectedWireParentId,
	WIRE_EVENT_KINDS,
	WIRE_MAX_EVENTS_PER_RUN,
	WIRE_MAX_STREAM_BYTES,
	WIRE_REQUIRED_SEMANTICS,
	WIRE_VERSION,
	type WireAbortEvidence,
	type WireEvent,
	type WireEventDraft,
	type WireKnownEvent,
	type WireTerminalLifecycle,
	type WireTerminationReason,
} from "./events";
import { decodeWireEvent } from "./decode";
import { WireReductionError } from "./errors";
import {
	wireEventId,
	type WireEventId,
	type WireRunId,
} from "./ids";
import { WIRE_JSON_MAX_BYTES } from "./json";
import { reduceWireEvent, type WireRunSnapshot } from "./reducer";

export interface WireEventWriterOptions {
	runId: WireRunId;
	eventId?: (sequence: number) => WireEventId;
	now?: () => string;
	maxEvents?: number;
	maxBytes?: number;
}

export interface WireEventWriterRestoreOptions {
	eventId?: (sequence: number) => WireEventId;
	now?: () => string;
	maxEvents?: number;
	maxBytes?: number;
}

export interface WireTerminationInput {
	lifecycle: WireTerminalLifecycle;
	reason: WireTerminationReason;
	abort?: WireAbortEvidence;
}

const TEXT_ENCODER = new TextEncoder();
const MAX_WIRE_ID = "x".repeat(256);
const MAX_WIRE_INSTANT = "+275760-09-13T00:00:00.000Z";
const MAX_TERMINATION_CODE: WireTerminationReason["code"] = "provider_process_died";

interface WireTerminationReserve {
	events: number;
	bytes: number;
}

function encodedLineBytes(value: unknown): number {
	return TEXT_ENCODER.encode(JSON.stringify(value)).byteLength + 1;
}

function terminationDrafts(
	snapshot: WireRunSnapshot,
	input: WireTerminationInput,
): WireEventDraft[] {
	const drafts: WireEventDraft[] = [];
	if (input.lifecycle !== "completed") {
		const cancelled = input.reason.code === "cancelled" || input.reason.code === "aborted";
		for (const tool of Object.values(snapshot.toolCalls)) {
			if (tool.status !== "pending") continue;
			drafts.push({
				kind: "tool.terminal",
				toolCallId: tool.id,
				status: cancelled ? "cancelled" : "synthetic_failure",
				origin: "north",
				errorCode: input.reason.code,
			});
		}
		for (const message of Object.values(snapshot.messages)) {
			if (message.stage === "completed") continue;
			drafts.push({
				kind: "message.recorded",
				messageId: message.id,
				stage: "completed",
				role: message.role,
				...(message.modelCallId === undefined ? {} : { modelCallId: message.modelCallId }),
				...(message.parentToolCallId === undefined ? {} : {
					parentToolCallId: message.parentToolCallId,
				}),
			});
		}
		for (const modelCall of Object.values(snapshot.modelCalls)) {
			if (modelCall.status !== "running") continue;
			drafts.push({
				kind: "model-call.completed",
				modelCallId: modelCall.id,
				status: cancelled ? "cancelled" : "failed",
				origin: "north",
				usage: snapshot.usage,
				usageCoverage: "unavailable",
				errorCode: input.reason.code,
			});
		}
	}
	drafts.push({
		kind: "run.terminated",
		lifecycle: input.lifecycle,
		reason: input.reason,
		...(input.abort === undefined ? {} : { abort: input.abort }),
	});
	return drafts;
}

function syntheticClosureUpperBoundBytes(draft: WireEventDraft): number {
	return encodedLineBytes({
		...draft,
		version: WIRE_VERSION,
		id: MAX_WIRE_ID,
		runId: MAX_WIRE_ID,
		parentId: MAX_WIRE_ID,
		sequence: WIRE_MAX_EVENTS_PER_RUN - 1,
		at: MAX_WIRE_INSTANT,
		essential: true,
		requiredSemantics: WIRE_REQUIRED_SEMANTICS,
	});
}

function terminationReserve(snapshot: WireRunSnapshot): WireTerminationReserve {
	const drafts = terminationDrafts(snapshot, {
		lifecycle: "failed",
		reason: { code: MAX_TERMINATION_CODE },
	});
	const closureDrafts = drafts.filter((draft) => draft.kind !== "run.terminated");
	const closureBytes = closureDrafts.reduce(
		(total, draft) => total + syntheticClosureUpperBoundBytes(draft),
		0,
	);
	/*
	 * decodeWireEvent admits at most WIRE_JSON_MAX_BYTES for one event. Synthetic
	 * closures use only the bounded fields generated above, so their maximum
	 * envelope is counted directly. The caller-controlled run terminal keeps the
	 * full decoded-event bound. Each persisted JSONL record adds one LF byte.
	 */
	return {
		events: closureDrafts.length + 1,
		bytes: closureBytes + WIRE_JSON_MAX_BYTES + 1,
	};
}

function positiveBound(value: number | undefined, hardLimit: number, label: string): number {
	const resolved = value ?? hardLimit;
	if (!Number.isSafeInteger(resolved) || resolved < 1 || resolved > hardLimit) {
		throw new RangeError(`${label} must be a positive safe integer no greater than ${hardLimit}`);
	}
	return resolved;
}

export class WireEventWriter {
	readonly runId: WireRunId;
	#eventId: (sequence: number) => WireEventId;
	#now: () => string;
	#events: WireKnownEvent[] = [];
	#eventIds = new Set<WireEventId>();
	#snapshot?: WireRunSnapshot;
	#bytes = 0;
	#maxEvents: number;
	#maxBytes: number;

	constructor(options: WireEventWriterOptions) {
		this.runId = options.runId;
		this.#eventId = options.eventId ?? (() => wireEventId(`event:${crypto.randomUUID()}`));
		this.#now = options.now ?? (() => new Date().toISOString());
		this.#maxEvents = positiveBound(
			options.maxEvents,
			WIRE_MAX_EVENTS_PER_RUN,
			"wire writer maxEvents",
		);
		this.#maxBytes = positiveBound(
			options.maxBytes,
			WIRE_MAX_STREAM_BYTES,
			"wire writer maxBytes",
		);
	}

	/**
	 * Hydrate a writer from an exact canonical prefix so North can own any
	 * synthetic terminal events added during crash recovery. Provider adapters
	 * never use this path: their only admitted history is the writer they share
	 * with the live host.
	 */
	static restore(
		events: readonly WireEvent[],
		options: WireEventWriterRestoreOptions = {},
	): WireEventWriter {
		const first = events[0];
		if (!first) {
			throw new WireReductionError(
				"state_violation",
				"cannot restore a wire writer from an empty event stream",
			);
		}
		const decodedFirst = decodeWireEvent(first);
		if (decodedFirst.version !== WIRE_VERSION || decodedFirst.essential !== true
			|| !(WIRE_EVENT_KINDS as readonly string[]).includes(decodedFirst.kind)) {
			throw new WireReductionError(
				"state_violation",
				"wire writer recovery requires a canonical known event stream",
			);
		}
		const writer = new WireEventWriter({
			runId: decodedFirst.runId,
			...(options.eventId === undefined ? {} : { eventId: options.eventId }),
			...(options.now === undefined ? {} : { now: options.now }),
			...(options.maxEvents === undefined ? {} : { maxEvents: options.maxEvents }),
			...(options.maxBytes === undefined ? {} : { maxBytes: options.maxBytes }),
		});
		if (events.length > writer.#maxEvents) {
			throw new WireReductionError(
				"state_violation",
				`wire writer recovery exceeds its ${writer.#maxEvents}-event limit`,
				{ runId: writer.runId },
			);
		}

		const restored: WireKnownEvent[] = [];
		const eventIds = new Set<WireEventId>();
		let snapshot: WireRunSnapshot | undefined;
		let bytes = 0;
		for (const source of events) {
			const decoded = decodeWireEvent(source);
			if (decoded.version !== WIRE_VERSION || decoded.essential !== true
				|| !(WIRE_EVENT_KINDS as readonly string[]).includes(decoded.kind)) {
				throw new WireReductionError(
					"state_violation",
					"wire writer recovery requires a canonical known event stream",
					{ eventId: decoded.id, runId: decoded.runId, sequence: decoded.sequence },
				);
			}
			if (decoded.runId !== writer.runId) {
				throw new WireReductionError(
					"sequence_violation",
					"wire writer recovery cannot mix run identities",
					{ eventId: decoded.id, runId: decoded.runId, sequence: decoded.sequence },
				);
			}
			const event = decoded as WireKnownEvent;
			bytes += encodedLineBytes(event);
			if (bytes > writer.#maxBytes) {
				throw new WireReductionError(
					"state_violation",
					`wire writer recovery exceeds its ${writer.#maxBytes}-byte limit`,
					{ eventId: event.id, runId: event.runId, sequence: event.sequence },
				);
			}
			snapshot = reduceWireEvent(snapshot, event);
			restored.push(event);
			eventIds.add(event.id);
		}
		if (!snapshot) {
			throw new WireReductionError("state_violation", "wire writer recovery has no snapshot");
		}
		if (snapshot.lifecycle === "running" || snapshot.lifecycle === "waiting") {
			const reserve = terminationReserve(snapshot);
			if (restored.length + reserve.events > writer.#maxEvents
				|| reserve.bytes > WIRE_MAX_STREAM_BYTES
				|| bytes + reserve.bytes > writer.#maxBytes) {
				throw new WireReductionError(
					"state_violation",
					"wire writer recovery cannot reserve its terminal event suffix",
					{ runId: writer.runId },
				);
			}
		}
		writer.#events = restored;
		writer.#eventIds = eventIds;
		writer.#snapshot = snapshot;
		writer.#bytes = bytes;
		return writer;
	}

	#prepare(draft: WireEventDraft, sequence: number): WireKnownEvent {
		if ("parentId" in draft) {
			throw new WireReductionError(
				"state_violation",
				"wire event parentId is derived and cannot be supplied by a writer caller",
				{ runId: this.runId, sequence },
			);
		}
		const parentId = expectedWireParentId(draft, this.runId);
		const decoded = decodeWireEvent({
			...draft,
			version: WIRE_VERSION,
			id: this.#eventId(sequence),
			runId: this.runId,
			...(parentId === undefined ? {} : { parentId }),
			sequence,
			at: this.#now(),
			essential: true,
			requiredSemantics: WIRE_REQUIRED_SEMANTICS,
		});
		if (decoded.version !== WIRE_VERSION || !(WIRE_EVENT_KINDS as readonly string[]).includes(decoded.kind)) {
			throw new WireReductionError("state_violation", "wire writer produced an opaque event");
		}
		return decoded as WireKnownEvent;
	}

	append(draft: WireEventDraft): WireKnownEvent {
		return this.appendAll([draft])[0];
	}

	appendAll(drafts: readonly WireEventDraft[]): readonly WireKnownEvent[] {
		if (this.#events.length + drafts.length > this.#maxEvents) {
			throw new WireReductionError(
				"state_violation",
				`wire writer exceeds its ${this.#maxEvents}-event limit`,
				{ runId: this.runId },
			);
		}
		const emitted: WireKnownEvent[] = [];
		const emittedIds = new Set<WireEventId>();
		let snapshot = this.#snapshot;
		let bytes = this.#bytes;
		for (const draft of drafts) {
			const event = this.#prepare(draft, this.#events.length + emitted.length);
			bytes += encodedLineBytes(event);
			if (bytes > this.#maxBytes) {
				throw new WireReductionError(
					"state_violation",
					`wire writer exceeds its ${this.#maxBytes}-byte limit`,
					{ eventId: event.id, runId: event.runId, sequence: event.sequence },
				);
			}
			if (this.#eventIds.has(event.id) || emittedIds.has(event.id)) {
				throw new WireReductionError(
					"sequence_violation",
					`wire event id ${event.id} is duplicated`,
					{ eventId: event.id, runId: event.runId, sequence: event.sequence },
				);
			}
			snapshot = reduceWireEvent(snapshot, event);
			emitted.push(event);
			emittedIds.add(event.id);
		}
		if (snapshot && snapshot.lifecycle !== "completed" && snapshot.lifecycle !== "failed"
			&& snapshot.lifecycle !== "cancelled" && snapshot.lifecycle !== "blocked") {
			const reserve = terminationReserve(snapshot);
			const resultingEvents = this.#events.length + emitted.length;
			if (resultingEvents + reserve.events > this.#maxEvents) {
				throw new WireReductionError(
					"state_violation",
					`wire writer cannot reserve ${reserve.events} termination events within its ${this.#maxEvents}-event limit`,
					{ runId: this.runId },
				);
			}
			if (reserve.bytes > WIRE_MAX_STREAM_BYTES || bytes + reserve.bytes > this.#maxBytes) {
				throw new WireReductionError(
					"state_violation",
					`wire writer cannot reserve ${reserve.bytes} termination bytes within its ${this.#maxBytes}-byte limit`,
					{ runId: this.runId },
				);
			}
		}
		this.#snapshot = snapshot;
		this.#bytes = bytes;
		this.#events.push(...emitted);
		for (const eventId of emittedIds) this.#eventIds.add(eventId);
		return Object.freeze(emitted);
	}

	terminate(input: WireTerminationInput): readonly WireKnownEvent[] {
		if (!this.#snapshot) {
			throw new WireReductionError("state_violation", "wire run must start before termination");
		}
		return this.appendAll(terminationDrafts(this.#snapshot, input));
	}

	snapshot(): WireRunSnapshot | undefined {
		return this.#snapshot;
	}

	events(): readonly WireKnownEvent[] {
		return Object.freeze([...this.#events]);
	}
}
