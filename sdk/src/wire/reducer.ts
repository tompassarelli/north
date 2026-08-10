import {
	expectedWireParentId,
	WIRE_EVENT_KINDS,
	WIRE_MAX_ACCUMULATED_RECORDS,
	WIRE_MAX_ENTITIES_PER_KIND,
	WIRE_MAX_EVENTS_PER_RUN,
	WIRE_MAX_MESSAGE_CONTENT_PARTS,
	WIRE_MAX_STREAM_BYTES,
	WIRE_VERSION,
	type WireAbortEvidence,
	type WireArtifactPublishedEvent,
	type WireBranchState,
	type WireCompletionEvidence,
	type WireEvent,
	type WireEffort,
	type WireFallbackState,
	type WireLifetimeUsage,
	type WireMessageRecordedEvent,
	type WireModelCallCompletedEvent,
	type WireModelCallUsageCoverage,
	type WireModelSelection,
	type WireNestedProgress,
	type WireKnownEvent,
	type WireOpaqueEvent,
	type WireOutputReference,
	type WirePatchState,
	type WireRecentTool,
	type WireRetryState,
	type WireRunLifecycle,
	type WireTerminationReason,
	type WireToolAdmittedEvent,
	type WireToolSchemaProvenance,
	type WireUsageSnapshot,
} from "./events";
import { decodeWireEvent } from "./decode";
import { WireReductionError } from "./errors";
import type {
	WireArtifactId,
	WireEventId,
	WireMessageId,
	WireModelCallId,
	WireParentId,
	WireResourceId,
	WireRunId,
	WireToolCallId,
} from "./ids";
import type { JsonObject, JsonValue } from "./json";

export const WIRE_RECENT_TOOL_LIMIT = 32;
const WIRE_KNOWN_EVENT_KIND_SET = new Set<string>(WIRE_EVENT_KINDS);
const WIRE_EVENT_ID_INDEX = Symbol("north.wire.event-id-index");
const WIRE_ENTITY_ID_INDEX = Symbol("north.wire.entity-id-index");
const WIRE_ENTITY_KIND_COUNTS = Symbol("north.wire.entity-kind-counts");
const WIRE_EVENT_BYTES = Symbol("north.wire.event-bytes");
const TEXT_ENCODER = new TextEncoder();

interface WireEventIdIndex {
	readonly id: WireEventId;
	readonly height: number;
	readonly left?: WireEventIdIndex;
	readonly right?: WireEventIdIndex;
}

type WireEntityKind = "run" | "resource" | "artifact" | "message" | "model-call" | "tool-call";
type WireEntityKindCounts = Readonly<Record<WireEntityKind, number>>;

interface WireEntityIdIndex {
	readonly id: string;
	readonly kind: WireEntityKind;
	readonly height: number;
	readonly left?: WireEntityIdIndex;
	readonly right?: WireEntityIdIndex;
}

type IndexedWireRunSnapshot = WireRunSnapshot & {
	readonly [WIRE_EVENT_ID_INDEX]: WireEventIdIndex;
	readonly [WIRE_ENTITY_ID_INDEX]: WireEntityIdIndex;
	readonly [WIRE_ENTITY_KIND_COUNTS]: WireEntityKindCounts;
	readonly [WIRE_EVENT_BYTES]: number;
};

const EMPTY_ENTITY_KIND_COUNTS: WireEntityKindCounts = Object.freeze({
	run: 0,
	resource: 0,
	artifact: 0,
	message: 0,
	"model-call": 0,
	"tool-call": 0,
});

function indexHeight(index: WireEventIdIndex | undefined): number {
	return index?.height ?? 0;
}

function eventIdIndex(
	id: WireEventId,
	left?: WireEventIdIndex,
	right?: WireEventIdIndex,
): WireEventIdIndex {
	return Object.freeze({
		id,
		height: Math.max(indexHeight(left), indexHeight(right)) + 1,
		...optional("left", left),
		...optional("right", right),
	});
}

function rotateEventIdIndexLeft(root: WireEventIdIndex): WireEventIdIndex {
	const pivot = root.right;
	if (!pivot) return root;
	return eventIdIndex(
		pivot.id,
		eventIdIndex(root.id, root.left, pivot.left),
		pivot.right,
	);
}

function rotateEventIdIndexRight(root: WireEventIdIndex): WireEventIdIndex {
	const pivot = root.left;
	if (!pivot) return root;
	return eventIdIndex(
		pivot.id,
		pivot.left,
		eventIdIndex(root.id, pivot.right, root.right),
	);
}

interface EventIdInsertResult {
	readonly index: WireEventIdIndex;
	readonly inserted: boolean;
}

function insertEventId(
	root: WireEventIdIndex | undefined,
	id: WireEventId,
): EventIdInsertResult {
	if (!root) return { index: eventIdIndex(id), inserted: true };
	if (id === root.id) return { index: root, inserted: false };
	const insertLeft = id < root.id;
	const inserted = insertEventId(insertLeft ? root.left : root.right, id);
	if (!inserted.inserted) return inserted;
	let next = insertLeft
		? eventIdIndex(root.id, inserted.index, root.right)
		: eventIdIndex(root.id, root.left, inserted.index);
	const balance = indexHeight(next.left) - indexHeight(next.right);
	if (balance > 1 && next.left) {
		if (id > next.left.id) {
			next = eventIdIndex(next.id, rotateEventIdIndexLeft(next.left), next.right);
		}
		return { index: rotateEventIdIndexRight(next), inserted: true };
	}
	if (balance < -1 && next.right) {
		if (id < next.right.id) {
			next = eventIdIndex(next.id, next.left, rotateEventIdIndexRight(next.right));
		}
		return { index: rotateEventIdIndexLeft(next), inserted: true };
	}
	return { index: next, inserted: true };
}

function entityIndexHeight(index: WireEntityIdIndex | undefined): number {
	return index?.height ?? 0;
}

function entityIdIndex(
	id: string,
	kind: WireEntityKind,
	left?: WireEntityIdIndex,
	right?: WireEntityIdIndex,
): WireEntityIdIndex {
	return Object.freeze({
		id,
		kind,
		height: Math.max(entityIndexHeight(left), entityIndexHeight(right)) + 1,
		...optional("left", left),
		...optional("right", right),
	});
}

function rotateEntityIdIndexLeft(root: WireEntityIdIndex): WireEntityIdIndex {
	const pivot = root.right;
	if (!pivot) return root;
	return entityIdIndex(
		pivot.id,
		pivot.kind,
		entityIdIndex(root.id, root.kind, root.left, pivot.left),
		pivot.right,
	);
}

function rotateEntityIdIndexRight(root: WireEntityIdIndex): WireEntityIdIndex {
	const pivot = root.left;
	if (!pivot) return root;
	return entityIdIndex(
		pivot.id,
		pivot.kind,
		pivot.left,
		entityIdIndex(root.id, root.kind, pivot.right, root.right),
	);
}

interface EntityIdInsertResult {
	readonly index: WireEntityIdIndex;
	readonly existingKind?: WireEntityKind;
}

function insertEntityId(
	root: WireEntityIdIndex | undefined,
	id: string,
	kind: WireEntityKind,
): EntityIdInsertResult {
	if (!root) return { index: entityIdIndex(id, kind) };
	if (id === root.id) return { index: root, existingKind: root.kind };
	const insertLeft = id < root.id;
	const inserted = insertEntityId(insertLeft ? root.left : root.right, id, kind);
	if (inserted.existingKind !== undefined) {
		return { index: root, existingKind: inserted.existingKind };
	}
	let next = insertLeft
		? entityIdIndex(root.id, root.kind, inserted.index, root.right)
		: entityIdIndex(root.id, root.kind, root.left, inserted.index);
	const balance = entityIndexHeight(next.left) - entityIndexHeight(next.right);
	if (balance > 1 && next.left) {
		if (id > next.left.id) {
			next = entityIdIndex(
				next.id,
				next.kind,
				rotateEntityIdIndexLeft(next.left),
				next.right,
			);
		}
		return { index: rotateEntityIdIndexRight(next) };
	}
	if (balance < -1 && next.right) {
		if (id < next.right.id) {
			next = entityIdIndex(
				next.id,
				next.kind,
				next.left,
				rotateEntityIdIndexRight(next.right),
			);
		}
		return { index: rotateEntityIdIndexLeft(next) };
	}
	return { index: next };
}

function indexedSnapshot(
	snapshot: WireRunSnapshot,
	index: WireEventIdIndex,
	entityIndex: WireEntityIdIndex,
	entityKindCounts: WireEntityKindCounts,
	bytes: number,
): WireRunSnapshot {
	const indexed = { ...snapshot } as IndexedWireRunSnapshot;
	Object.defineProperty(indexed, WIRE_EVENT_ID_INDEX, {
		value: index,
		enumerable: false,
		configurable: false,
		writable: false,
	});
	Object.defineProperty(indexed, WIRE_EVENT_BYTES, {
		value: bytes,
		enumerable: false,
		configurable: false,
		writable: false,
	});
	Object.defineProperty(indexed, WIRE_ENTITY_ID_INDEX, {
		value: entityIndex,
		enumerable: false,
		configurable: false,
		writable: false,
	});
	Object.defineProperty(indexed, WIRE_ENTITY_KIND_COUNTS, {
		value: entityKindCounts,
		enumerable: false,
		configurable: false,
		writable: false,
	});
	return Object.freeze(indexed);
}

function snapshotEventIdIndex(
	snapshot: WireRunSnapshot,
	event: WireEvent,
): WireEventIdIndex {
	const index = (snapshot as Partial<IndexedWireRunSnapshot>)[WIRE_EVENT_ID_INDEX];
	if (!index) {
		return reductionError(
			event,
			"state_violation",
			"wire snapshot was not produced by the incremental reducer",
		);
	}
	return index;
}

function snapshotEntityIdIndex(
	snapshot: WireRunSnapshot,
	event: WireEvent,
): WireEntityIdIndex {
	const index = (snapshot as Partial<IndexedWireRunSnapshot>)[WIRE_ENTITY_ID_INDEX];
	if (!index) {
		return reductionError(
			event,
			"state_violation",
			"wire snapshot has no cross-kind entity identity index",
		);
	}
	return index;
}

function snapshotEntityKindCounts(
	snapshot: WireRunSnapshot,
	event: WireEvent,
): WireEntityKindCounts {
	const counts = (snapshot as Partial<IndexedWireRunSnapshot>)[WIRE_ENTITY_KIND_COUNTS];
	if (!counts) {
		return reductionError(
			event,
			"state_violation",
			"wire snapshot has no cross-kind entity capacity accounting",
		);
	}
	return counts;
}

function snapshotEventBytes(snapshot: WireRunSnapshot, event: WireEvent): number {
	const bytes = (snapshot as Partial<IndexedWireRunSnapshot>)[WIRE_EVENT_BYTES];
	if (bytes === undefined) {
		return reductionError(
			event,
			"state_violation",
			"wire snapshot has no incremental byte accounting",
		);
	}
	return bytes;
}

function encodedEventBytes(event: WireEvent): number {
	return TEXT_ENCODER.encode(JSON.stringify(event)).byteLength + 1;
}

export interface WireMessageSnapshot {
	id: WireMessageId;
	role: WireMessageRecordedEvent["role"];
	stage: WireMessageRecordedEvent["stage"];
	contents: readonly JsonValue[];
	modelCallId?: WireModelCallId;
	parentToolCallId?: WireToolCallId;
}

export interface WireModelCallSnapshot {
	id: WireModelCallId;
	model: WireModelSelection;
	effort?: WireEffort;
	attempt: number;
	startedAt: string;
	status: "running" | WireModelCallCompletedEvent["status"];
	completedAt?: string;
	usage?: WireUsageSnapshot;
	usageCoverage?: WireModelCallUsageCoverage;
	errorCode?: string;
	origin?: WireModelCallCompletedEvent["origin"];
	evidence?: WireCompletionEvidence;
}

export interface WireRunUsageCoverage {
	/** Provider-origin model completions, including terminals without usable usage. */
	providerTerminalCount: number;
	scope: "wire_run_cumulative";
	totalStatus: "unknown_no_terminal" | "unknown_incomplete_terminal" | "partial" | "exact";
}

export interface WireToolCallSnapshot {
	id: WireToolCallId;
	name: string;
	messageId?: WireMessageId;
	modelCallId?: WireModelCallId;
	parentToolCallId?: WireToolCallId;
	schema: WireToolSchemaProvenance;
	argumentPreview?: string;
	argumentArtifactId?: WireArtifactId;
	admittedAt: string;
	status: "pending" | WireRecentTool["status"];
	terminalAt?: string;
	origin?: "provider" | "north";
	resultPreview?: string;
	resultArtifactId?: WireArtifactId;
	errorCode?: string;
	progress?: JsonValue;
	outputArtifactId?: WireArtifactId;
}

export interface WireArtifactSnapshot {
	id: WireArtifactId;
	resourceId?: WireResourceId;
	mediaType: string;
	bytes: number;
	digest?: string;
	label?: string;
	publishedAt: string;
}

export interface WireResourcePressureSnapshot {
	resourceId?: WireResourceId;
	scope: string;
	resource: string;
	used: number;
	reserved: number;
	limit: number;
	advisory: boolean;
	at: string;
}

export interface WireRunSnapshot {
	version: typeof WIRE_VERSION;
	runId: WireRunId;
	parentId?: WireParentId;
	parentRunId?: WireRunId;
	owner?: string;
	lifecycle: WireRunLifecycle;
	termination?: WireTerminationReason;
	lastSequence: number;
	lastEventId: WireEventId;
	startedAt: string;
	updatedAt: string;
	currentAction?: string;
	compactions: number;
	recentTools: readonly WireRecentTool[];
	outputReferences: readonly WireOutputReference[];
	model?: WireModelSelection;
	effort?: WireEffort;
	retry?: WireRetryState;
	fallback?: WireFallbackState;
	nested: readonly WireNestedProgress[];
	patch?: WirePatchState;
	branch?: WireBranchState;
	abort?: WireAbortEvidence;
	usage: WireUsageSnapshot;
	usageCoverage: WireRunUsageCoverage;
	messages: Readonly<Record<string, WireMessageSnapshot>>;
	modelCalls: Readonly<Record<string, WireModelCallSnapshot>>;
	toolCalls: Readonly<Record<string, WireToolCallSnapshot>>;
	artifacts: Readonly<Record<string, WireArtifactSnapshot>>;
	resourcePressure: readonly WireResourcePressureSnapshot[];
	eventExtensions: Readonly<Record<string, JsonObject>>;
	opaqueEvents: readonly WireEvent[];
}

const EMPTY_LIFETIME_USAGE: WireLifetimeUsage = Object.freeze({
	inputTokens: 0,
	outputTokens: 0,
	cacheReadTokens: 0,
	cacheWriteTokens: 0,
	reasoningTokens: 0,
	modelCalls: 0,
});

const EMPTY_USAGE: WireUsageSnapshot = Object.freeze({
	lifetime: EMPTY_LIFETIME_USAGE,
	context: Object.freeze({ tokens: 0 }),
});

const EMPTY_USAGE_COVERAGE: WireRunUsageCoverage = Object.freeze({
	providerTerminalCount: 0,
	scope: "wire_run_cumulative",
	totalStatus: "unknown_no_terminal",
});

function usageCoverageForModelCalls(
	modelCalls: Readonly<Record<string, WireModelCallSnapshot>>,
	previous: WireRunUsageCoverage,
): WireRunUsageCoverage {
	const calls = Object.values(modelCalls);
	let providerTerminalCount = 0;
	let hasAuthoritativeEvidence = previous.totalStatus === "partial"
		|| previous.totalStatus === "exact";
	let allExact = calls.length > 0;
	for (const call of calls) {
		if (call.status === "running" || call.usageCoverage !== "exact" || call.origin !== "provider") {
			allExact = false;
		}
		if (call.status !== "running" && call.origin === "provider") {
			providerTerminalCount += 1;
		}
		if (call.usageCoverage === "partial" || call.usageCoverage === "exact") {
			hasAuthoritativeEvidence = true;
		}
	}
	return Object.freeze({
		providerTerminalCount,
		scope: "wire_run_cumulative",
		totalStatus: allExact
			? "exact"
			: hasAuthoritativeEvidence
				? "partial"
				: providerTerminalCount > 0 ? "unknown_incomplete_terminal" : "unknown_no_terminal",
	});
}

function reductionError(
	event: WireEvent,
	code: "sequence_violation" | "state_violation",
	message: string,
): never {
	throw new WireReductionError(code, message, {
		eventId: event.id,
		runId: event.runId,
		sequence: event.sequence,
	});
}

function isOpaqueEvent(event: WireEvent): event is WireOpaqueEvent {
	return event.version !== WIRE_VERSION || !WIRE_KNOWN_EVENT_KIND_SET.has(event.kind);
}

function assertExpectedParent(event: WireKnownEvent): void {
	const expected = expectedWireParentId(event, event.runId);
	if (event.parentId === expected) return;
	const expectation = expected === undefined ? "no parent" : `parent ${expected}`;
	reductionError(
		event,
		"state_violation",
		`wire ${event.kind} must have ${expectation}; observed ${event.parentId ?? "no parent"}`,
	);
}

interface WireEntityClaimState {
	readonly index?: WireEntityIdIndex;
	readonly counts: WireEntityKindCounts;
}

interface CompleteWireEntityClaimState extends WireEntityClaimState {
	readonly index: WireEntityIdIndex;
}

function claimEntityId(
	state: WireEntityClaimState,
	event: WireEvent,
	id: string,
	kind: WireEntityKind,
): CompleteWireEntityClaimState {
	const inserted = insertEntityId(state.index, id, kind);
	if (inserted.existingKind !== undefined && inserted.existingKind !== kind) {
		return reductionError(
			event,
			"state_violation",
			`wire entity id ${id} is both ${inserted.existingKind} and ${kind}`,
		);
	}
	if (inserted.existingKind === kind) {
		return { index: inserted.index, counts: state.counts };
	}
	if (state.counts[kind] >= WIRE_MAX_ENTITIES_PER_KIND) {
		return reductionError(
			event,
			"state_violation",
			`${kind} entity ids exceed the ${WIRE_MAX_ENTITIES_PER_KIND}-entry run limit`,
		);
	}
	return {
		index: inserted.index,
		counts: Object.freeze({
			...state.counts,
			[kind]: state.counts[kind] + 1,
		}),
	};
}

function claimEventEntityIds(
	index: WireEntityIdIndex | undefined,
	counts: WireEntityKindCounts,
	event: WireEvent,
): CompleteWireEntityClaimState {
	let next = claimEntityId({ index, counts }, event, event.runId, "run");
	if (isOpaqueEvent(event)) return next;
	const claim = (id: string | undefined, kind: WireEntityKind): void => {
		if (id !== undefined) next = claimEntityId(next, event, id, kind);
	};
	switch (event.kind) {
		case "run.started":
			claim(event.parentRunId, "run");
			break;
		case "run.progress":
			for (const child of event.progress.nested ?? []) claim(child.runId, "run");
			for (const reference of event.progress.outputReferences ?? []) {
				claim(reference.kind === "artifact" ? reference.artifactId : reference.resourceId, reference.kind);
			}
			claim(event.progress.patch?.artifactId, "artifact");
			break;
		case "message.recorded":
			claim(event.messageId, "message");
			claim(event.modelCallId, "model-call");
			claim(event.parentToolCallId, "tool-call");
			break;
		case "model-call.started":
		case "model-call.completed":
			claim(event.modelCallId, "model-call");
			break;
		case "tool.admitted":
			claim(event.toolCallId, "tool-call");
			claim(event.messageId, "message");
			claim(event.modelCallId, "model-call");
			claim(event.parentToolCallId, "tool-call");
			claim(event.argumentArtifactId, "artifact");
			break;
		case "tool.progress":
			claim(event.toolCallId, "tool-call");
			claim(event.outputArtifactId, "artifact");
			break;
		case "tool.terminal":
			claim(event.toolCallId, "tool-call");
			claim(event.resultArtifactId, "artifact");
			break;
		case "artifact.published":
			claim(event.artifactId, "artifact");
			claim(event.resourceId, "resource");
			break;
		case "resource.pressure":
			claim(event.resourceId, "resource");
			break;
		case "run.terminated":
			break;
	}
	return next;
}

function optional<Value, Key extends string>(key: Key, value: Value | undefined): Partial<Record<Key, Value>> {
	return value === undefined ? {} : { [key]: value } as Record<Key, Value>;
}

function emptyRecord<Value>(): Readonly<Record<string, Value>> {
	return Object.freeze(Object.create(null) as Record<string, Value>);
}

function recordValue<Value>(source: Readonly<Record<string, Value>>, key: string): Value | undefined {
	return Object.hasOwn(source, key) ? source[key] : undefined;
}

function withRecordValue<Value>(
	source: Readonly<Record<string, Value>>,
	key: string,
	value: Value,
): Readonly<Record<string, Value>> {
	const next = Object.assign(Object.create(null) as Record<string, Value>, source);
	next[key] = value;
	return Object.freeze(next);
}

function assertRecordCapacity<Value>(
	source: Readonly<Record<string, Value>>,
	event: WireEvent,
	label: string,
): void {
	if (Object.keys(source).length >= WIRE_MAX_ENTITIES_PER_KIND) {
		reductionError(
			event,
			"state_violation",
			`${label} exceeds the ${WIRE_MAX_ENTITIES_PER_KIND}-entry run limit`,
		);
	}
}

function requireArtifact(
	snapshot: WireRunSnapshot,
	event: WireEvent,
	artifactId: WireArtifactId | undefined,
	label: string,
): void {
	if (artifactId !== undefined && !recordValue(snapshot.artifacts, artifactId)) {
		reductionError(event, "state_violation", `${label} references unpublished artifact ${artifactId}`);
	}
}

function assertUsageMonotone(previous: WireUsageSnapshot, next: WireUsageSnapshot, event: WireEvent): void {
	for (const key of [
		"inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens", "reasoningTokens", "modelCalls",
	] as const) {
		if (next.lifetime[key] < previous.lifetime[key]) {
			reductionError(event, "state_violation", `lifetime usage ${key} decreased`);
		}
	}
}

function usageSnapshotsEqual(previous: WireUsageSnapshot, next: WireUsageSnapshot): boolean {
	return previous.lifetime.inputTokens === next.lifetime.inputTokens
		&& previous.lifetime.outputTokens === next.lifetime.outputTokens
		&& previous.lifetime.cacheReadTokens === next.lifetime.cacheReadTokens
		&& previous.lifetime.cacheWriteTokens === next.lifetime.cacheWriteTokens
		&& previous.lifetime.reasoningTokens === next.lifetime.reasoningTokens
		&& previous.lifetime.modelCalls === next.lifetime.modelCalls
		&& previous.context.tokens === next.context.tokens
		&& previous.context.window === next.context.window;
}

function terminalLifecycleMatches(reason: WireTerminationReason, lifecycle: WireRunLifecycle): boolean {
	if (reason.code === "completed") return lifecycle === "completed";
	if (reason.code === "cancelled" || reason.code === "aborted") return lifecycle === "cancelled";
	if (reason.code === "blocked" || reason.code === "resource_denied") return lifecycle === "blocked";
	return lifecycle === "failed";
}

function initialSnapshot(event: WireEvent): WireRunSnapshot {
	if (event.sequence !== 0) reductionError(event, "sequence_violation", "the first wire event sequence must be zero");
	if (isOpaqueEvent(event) || event.version !== WIRE_VERSION || event.kind !== "run.started") {
		return reductionError(event, "state_violation", "the first wire event must be run.started v2");
	}
	if (event.parentRunId === event.runId) {
		return reductionError(event, "state_violation", "a run cannot be its own parent");
	}
	return Object.freeze({
		version: WIRE_VERSION,
		runId: event.runId,
		...optional("parentId", event.parentId),
		...optional("parentRunId", event.parentRunId),
		...optional("owner", event.owner),
		lifecycle: "running",
		lastSequence: event.sequence,
		lastEventId: event.id,
		startedAt: event.at,
		updatedAt: event.at,
		compactions: 0,
		recentTools: Object.freeze([]),
		outputReferences: Object.freeze([]),
		nested: Object.freeze([]),
		usage: EMPTY_USAGE,
		usageCoverage: EMPTY_USAGE_COVERAGE,
		messages: emptyRecord<WireMessageSnapshot>(),
		modelCalls: emptyRecord<WireModelCallSnapshot>(),
		toolCalls: emptyRecord<WireToolCallSnapshot>(),
		artifacts: emptyRecord<WireArtifactSnapshot>(),
		resourcePressure: Object.freeze([]),
		eventExtensions: event.extensions
			? withRecordValue(emptyRecord<JsonObject>(), event.id, event.extensions)
			: emptyRecord<JsonObject>(),
		opaqueEvents: Object.freeze([]),
	});
}

function messageSnapshot(
	current: WireMessageSnapshot | undefined,
	event: WireMessageRecordedEvent,
): WireMessageSnapshot {
	if (!current) {
		if (event.stage !== "started") {
			return reductionError(event, "state_violation", `message ${event.messageId} ${event.stage} arrived before started`);
		}
		return Object.freeze({
			id: event.messageId,
			role: event.role,
			stage: event.stage,
			contents: Object.freeze(event.content === undefined ? [] : [event.content]),
			...optional("modelCallId", event.modelCallId),
			...optional("parentToolCallId", event.parentToolCallId),
		});
	}
	if (current.stage === "completed") {
		return reductionError(event, "state_violation", `message ${event.messageId} already completed`);
	}
	if (event.stage === "started") {
		return reductionError(event, "state_violation", `message ${event.messageId} started twice`);
	}
	if (event.role !== current.role || event.modelCallId !== current.modelCallId
		|| event.parentToolCallId !== current.parentToolCallId) {
		return reductionError(event, "state_violation", `message ${event.messageId} identity changed`);
	}
	if (event.content !== undefined
		&& current.contents.length >= WIRE_MAX_MESSAGE_CONTENT_PARTS) {
		return reductionError(
			event,
			"state_violation",
			`message ${event.messageId} exceeds the ${WIRE_MAX_MESSAGE_CONTENT_PARTS}-part content limit`,
		);
	}
	return Object.freeze({
		...current,
		stage: event.stage,
		contents: Object.freeze(event.content === undefined
			? [...current.contents]
			: [...current.contents, event.content]),
	});
}

function admittedTool(event: WireToolAdmittedEvent): WireToolCallSnapshot {
	return Object.freeze({
		id: event.toolCallId,
		name: event.name,
		...optional("messageId", event.messageId),
		...optional("modelCallId", event.modelCallId),
		...optional("parentToolCallId", event.parentToolCallId),
		schema: event.schema,
		...optional("argumentPreview", event.argumentPreview),
		...optional("argumentArtifactId", event.argumentArtifactId),
		admittedAt: event.at,
		status: "pending",
	});
}

function establishedModelProvider(snapshot: WireRunSnapshot): WireModelSelection["provider"] | undefined {
	for (const modelCall of Object.values(snapshot.modelCalls)) return modelCall.model.provider;
	return undefined;
}

function withProgress(snapshot: WireRunSnapshot, event: Extract<WireEvent, { kind: "run.progress" }>): WireRunSnapshot {
	const patch = event.progress;
	let usage = snapshot.usage;
	if (patch.usage !== undefined) {
		assertUsageMonotone(snapshot.usage, patch.usage, event);
		if (patch.usage.lifetime.modelCalls !== snapshot.usage.lifetime.modelCalls) {
			return reductionError(
				event,
				"state_violation",
				"run progress cannot change the structurally observed model-call count",
			);
		}
		usage = patch.usage;
	}
	const currentAction = patch.currentAction === undefined
		? snapshot.currentAction : patch.currentAction ?? undefined;
	const compactions = patch.compactions ?? snapshot.compactions;
	if (compactions < snapshot.compactions) {
		return reductionError(event, "state_violation", "run progress compactions decreased");
	}
	const outputReferences = patch.outputReferences === undefined
		? snapshot.outputReferences
		: Object.freeze(patch.outputReferences === null ? [] : [...patch.outputReferences]);
	for (const reference of outputReferences) {
		if (reference.kind === "artifact") {
			requireArtifact(snapshot, event, reference.artifactId, "run output");
		}
	}
	const model = patch.model === undefined ? snapshot.model : patch.model ?? undefined;
	const establishedProvider = establishedModelProvider(snapshot);
	if (patch.model !== undefined && establishedProvider !== undefined
		&& model?.provider !== establishedProvider) {
		return reductionError(
			event,
			"state_violation",
			"run progress cannot change the provider after the first model call",
		);
	}
	const effort = patch.effort === undefined ? snapshot.effort : patch.effort ?? undefined;
	const retry = patch.retry === undefined ? snapshot.retry : patch.retry ?? undefined;
	const fallback = patch.fallback === undefined ? snapshot.fallback : patch.fallback ?? undefined;
	const nested = patch.nested === undefined
		? snapshot.nested : Object.freeze(patch.nested === null ? [] : [...patch.nested]);
	const nestedIds = new Set<string>();
	for (const child of nested) {
		if (child.runId === snapshot.runId) {
			return reductionError(event, "state_violation", "nested progress cannot contain the current run");
		}
		if (nestedIds.has(child.runId)) {
			return reductionError(event, "state_violation", `nested progress duplicates run ${child.runId}`);
		}
		nestedIds.add(child.runId);
	}
	const nextPatch = patch.patch === undefined ? snapshot.patch : patch.patch ?? undefined;
	requireArtifact(snapshot, event, nextPatch?.artifactId, "run patch");
	const branch = patch.branch === undefined ? snapshot.branch : patch.branch ?? undefined;
	const abort = patch.abort === undefined ? snapshot.abort : patch.abort ?? undefined;
	const {
		currentAction: _currentAction,
		model: _model,
		effort: _effort,
		retry: _retry,
		fallback: _fallback,
		patch: _patch,
		branch: _branch,
		abort: _abort,
		...base
	} = snapshot;
	return Object.freeze({
		...base,
		lifecycle: event.lifecycle,
		...optional("currentAction", currentAction),
		compactions,
		recentTools: snapshot.recentTools,
		outputReferences,
		...optional("model", model),
		...optional("effort", effort),
		...optional("retry", retry),
		...optional("fallback", fallback),
		nested,
		...optional("patch", nextPatch),
		...optional("branch", branch),
		...optional("abort", abort),
		usage,
		usageCoverage: patch.usage === undefined
			? snapshot.usageCoverage : Object.freeze({
				...snapshot.usageCoverage,
				totalStatus: "partial" as const,
			}),
	});
}

function reduceKnown(snapshot: WireRunSnapshot, event: WireKnownEvent): WireRunSnapshot {
	switch (event.kind) {
		case "run.started":
			return reductionError(event, "state_violation", "run.started may occur only once");
		case "run.progress":
			return withProgress(snapshot, event);
		case "message.recorded": {
			if (event.modelCallId !== undefined) {
				const modelCall = recordValue(snapshot.modelCalls, event.modelCallId);
				if (!modelCall || modelCall.status !== "running") {
					return reductionError(
						event,
						"state_violation",
						`message ${event.messageId} references a model call that is not running`,
					);
				}
			}
			if (event.parentToolCallId !== undefined
				&& !recordValue(snapshot.toolCalls, event.parentToolCallId)) {
				return reductionError(
					event,
					"state_violation",
					`message ${event.messageId} references an unknown parent tool call`,
				);
			}
			const currentMessage = recordValue(snapshot.messages, event.messageId);
			if (!currentMessage) assertRecordCapacity(snapshot.messages, event, "messages");
			const message = messageSnapshot(currentMessage, event);
			return Object.freeze({
				...snapshot,
				messages: withRecordValue(snapshot.messages, event.messageId, message),
			});
		}
		case "model-call.started": {
			if (recordValue(snapshot.modelCalls, event.modelCallId)) {
				return reductionError(event, "state_violation", `model call ${event.modelCallId} started twice`);
			}
			const establishedProvider = establishedModelProvider(snapshot);
			if (establishedProvider !== undefined && establishedProvider !== event.model.provider) {
				return reductionError(
					event,
					"state_violation",
					"a run cannot switch providers after its first model call",
				);
			}
			assertRecordCapacity(snapshot.modelCalls, event, "model calls");
			if (snapshot.usage.lifetime.modelCalls >= Number.MAX_SAFE_INTEGER) {
				return reductionError(event, "state_violation", "model-call count exceeds the safe integer limit");
			}
			const modelCall: WireModelCallSnapshot = Object.freeze({
				id: event.modelCallId,
				model: event.model,
				...optional("effort", event.effort),
				attempt: event.attempt,
				startedAt: event.at,
				status: "running",
			});
			const usage: WireUsageSnapshot = Object.freeze({
				...snapshot.usage,
				lifetime: Object.freeze({
					...snapshot.usage.lifetime,
					modelCalls: snapshot.usage.lifetime.modelCalls + 1,
				}),
			});
			const modelCalls = withRecordValue(snapshot.modelCalls, event.modelCallId, modelCall);
			return Object.freeze({
				...snapshot,
				model: event.model,
				...optional("effort", event.effort ?? snapshot.effort),
				usage,
				usageCoverage: usageCoverageForModelCalls(modelCalls, snapshot.usageCoverage),
				modelCalls,
			});
		}
		case "model-call.completed": {
			const current = recordValue(snapshot.modelCalls, event.modelCallId);
			if (!current) {
				return reductionError(event, "state_violation", `model call ${event.modelCallId} completed before started`);
			}
			if (current.status !== "running") {
				return reductionError(event, "state_violation", `model call ${event.modelCallId} completed twice`);
			}
			assertUsageMonotone(snapshot.usage, event.usage, event);
			if (event.usageCoverage === "unavailable"
				&& !usageSnapshotsEqual(snapshot.usage, event.usage)) {
				return reductionError(
					event,
					"state_violation",
					"model completion with unavailable usage cannot change cumulative usage",
				);
			}
			if (event.usage.lifetime.modelCalls !== snapshot.usage.lifetime.modelCalls) {
				return reductionError(
					event,
					"state_violation",
					"model completion disagrees with the structurally observed model-call count",
				);
			}
			const completed: WireModelCallSnapshot = Object.freeze({
				...current,
				status: event.status,
				completedAt: event.at,
				usage: event.usage,
				usageCoverage: event.usageCoverage,
				...optional("errorCode", event.errorCode),
				origin: event.origin,
				...optional("evidence", event.evidence),
			});
			const modelCalls = withRecordValue(snapshot.modelCalls, event.modelCallId, completed);
			return Object.freeze({
				...snapshot,
				usage: event.usage,
				usageCoverage: usageCoverageForModelCalls(modelCalls, snapshot.usageCoverage),
				modelCalls,
			});
		}
		case "tool.admitted": {
			if (recordValue(snapshot.toolCalls, event.toolCallId)) {
				return reductionError(event, "state_violation", `tool call ${event.toolCallId} admitted twice`);
			}
			assertRecordCapacity(snapshot.toolCalls, event, "tool calls");
			requireArtifact(snapshot, event, event.argumentArtifactId, "tool argument");
			let referencedMessage: WireMessageSnapshot | undefined;
			if (event.messageId !== undefined) {
				referencedMessage = recordValue(snapshot.messages, event.messageId);
				if (!referencedMessage || referencedMessage.role !== "assistant") {
					return reductionError(
						event,
						"state_violation",
						`tool call ${event.toolCallId} references an unknown non-assistant message`,
					);
				}
			}
			if (event.modelCallId !== undefined) {
				const modelCall = recordValue(snapshot.modelCalls, event.modelCallId);
				if (!modelCall || modelCall.status !== "running") {
					return reductionError(
						event,
						"state_violation",
						`tool call ${event.toolCallId} references a model call that is not running`,
					);
				}
			}
			if (referencedMessage !== undefined
				&& event.modelCallId !== undefined
				&& referencedMessage.modelCallId !== event.modelCallId) {
				return reductionError(
					event,
					"state_violation",
					`tool call ${event.toolCallId} message and model-call ancestry disagree`,
				);
			}
			if (referencedMessage !== undefined
				&& event.parentToolCallId !== undefined
				&& referencedMessage.parentToolCallId !== event.parentToolCallId) {
				return reductionError(
					event,
					"state_violation",
					`tool call ${event.toolCallId} message and parent-tool ancestry disagree`,
				);
			}
			if (event.parentToolCallId === event.toolCallId) {
				return reductionError(event, "state_violation", `tool call ${event.toolCallId} cannot parent itself`);
			}
			if (event.parentToolCallId !== undefined) {
				const parentTool = recordValue(snapshot.toolCalls, event.parentToolCallId);
				if (!parentTool || parentTool.status !== "pending") {
					return reductionError(
						event,
						"state_violation",
						`tool call ${event.toolCallId} references a parent tool call that is not pending`,
					);
				}
			}
			return Object.freeze({
				...snapshot,
				toolCalls: withRecordValue(snapshot.toolCalls, event.toolCallId, admittedTool(event)),
			});
		}
		case "tool.progress": {
			const current = recordValue(snapshot.toolCalls, event.toolCallId);
			if (!current || current.status !== "pending") {
				return reductionError(event, "state_violation", `tool call ${event.toolCallId} progress requires an open admission`);
			}
			requireArtifact(snapshot, event, event.outputArtifactId, "tool progress output");
			const progressed: WireToolCallSnapshot = Object.freeze({
				...current,
				...optional("progress", event.progress),
				...optional("outputArtifactId", event.outputArtifactId),
			});
			return Object.freeze({
				...snapshot,
				toolCalls: withRecordValue(snapshot.toolCalls, event.toolCallId, progressed),
			});
		}
		case "tool.terminal": {
			const current = recordValue(snapshot.toolCalls, event.toolCallId);
			if (!current) {
				return reductionError(event, "state_violation", `tool call ${event.toolCallId} terminal arrived before admission`);
			}
			if (current.status !== "pending") {
				return reductionError(event, "state_violation", `tool call ${event.toolCallId} has more than one terminal`);
			}
			requireArtifact(snapshot, event, event.resultArtifactId, "tool result");
			const terminal: WireToolCallSnapshot = Object.freeze({
				...current,
				status: event.status,
				terminalAt: event.at,
				origin: event.origin,
				...optional("resultPreview", event.resultPreview),
				...optional("resultArtifactId", event.resultArtifactId),
				...optional("errorCode", event.errorCode),
			});
			const recent = Object.freeze([
				...snapshot.recentTools,
				Object.freeze({
					toolCallId: event.toolCallId,
					name: current.name,
					status: event.status,
					endedAt: event.at,
				}),
			].slice(-WIRE_RECENT_TOOL_LIMIT));
			return Object.freeze({
				...snapshot,
				recentTools: recent,
				toolCalls: withRecordValue(snapshot.toolCalls, event.toolCallId, terminal),
			});
		}
		case "artifact.published": {
			if (recordValue(snapshot.artifacts, event.artifactId)) {
				return reductionError(event, "state_violation", `artifact ${event.artifactId} was published twice`);
			}
			assertRecordCapacity(snapshot.artifacts, event, "artifacts");
			const artifact: WireArtifactSnapshot = artifactSnapshot(event);
			return Object.freeze({
				...snapshot,
				artifacts: withRecordValue(snapshot.artifacts, event.artifactId, artifact),
			});
		}
		case "resource.pressure": {
			if (snapshot.resourcePressure.length >= WIRE_MAX_ACCUMULATED_RECORDS) {
				return reductionError(
					event,
					"state_violation",
					`resource pressure exceeds the ${WIRE_MAX_ACCUMULATED_RECORDS}-event run limit`,
				);
			}
			const pressure: WireResourcePressureSnapshot = Object.freeze({
				...optional("resourceId", event.resourceId),
				scope: event.scope,
				resource: event.resource,
				used: event.used,
				reserved: event.reserved,
				limit: event.limit,
				advisory: event.advisory,
				at: event.at,
			});
			return Object.freeze({
				...snapshot,
				resourcePressure: Object.freeze([...snapshot.resourcePressure, pressure]),
			});
		}
		case "run.terminated": {
			const pending = Object.values(snapshot.toolCalls).filter((tool) => tool.status === "pending");
			if (pending.length) {
				return reductionError(
					event,
					"state_violation",
					`run terminated with unterminated tool calls: ${pending.map((tool) => tool.id).join(", ")}`,
				);
			}
			const runningModelCalls = Object.values(snapshot.modelCalls)
				.filter((modelCall) => modelCall.status === "running");
			if (runningModelCalls.length) {
				return reductionError(
					event,
					"state_violation",
					`run terminated with open model calls: ${runningModelCalls.map((call) => call.id).join(", ")}`,
				);
			}
			const incompleteMessages = Object.values(snapshot.messages)
				.filter((message) => message.stage !== "completed");
			if (incompleteMessages.length) {
				return reductionError(
					event,
					"state_violation",
					`run terminated with incomplete messages: ${incompleteMessages.map((message) => message.id).join(", ")}`,
				);
			}
			if (!terminalLifecycleMatches(event.reason, event.lifecycle)) {
				return reductionError(event, "state_violation", "run terminal lifecycle and reason disagree");
			}
			return Object.freeze({
				...snapshot,
				lifecycle: event.lifecycle,
				termination: event.reason,
				...optional("abort", event.abort ?? snapshot.abort),
			});
		}
	}
	const exhaustive: never = event;
	return exhaustive;
}

function artifactSnapshot(event: WireArtifactPublishedEvent): WireArtifactSnapshot {
	return Object.freeze({
		id: event.artifactId,
		...optional("resourceId", event.resourceId),
		mediaType: event.mediaType,
		bytes: event.bytes,
		...optional("digest", event.digest),
		...optional("label", event.label),
		publishedAt: event.at,
	});
}

export function reduceWireEvent(
	snapshot: WireRunSnapshot | undefined,
	event: WireEvent,
): WireRunSnapshot {
	event = decodeWireEvent(event);
	if (!isOpaqueEvent(event)) assertExpectedParent(event);
	if (!snapshot) {
		const inserted = insertEventId(undefined, event.id);
		const entities = claimEventEntityIds(undefined, EMPTY_ENTITY_KIND_COUNTS, event);
		const bytes = encodedEventBytes(event);
		if (bytes > WIRE_MAX_STREAM_BYTES) {
			return reductionError(event, "state_violation", "wire run exceeds its byte limit");
		}
		return indexedSnapshot(
			initialSnapshot(event),
			inserted.index,
			entities.index,
			entities.counts,
			bytes,
		);
	}
	if (snapshot.lifecycle === "completed" || snapshot.lifecycle === "failed"
		|| snapshot.lifecycle === "cancelled" || snapshot.lifecycle === "blocked") {
		return reductionError(event, "state_violation", "wire event arrived after run termination");
	}
	if (event.runId !== snapshot.runId) {
		return reductionError(event, "state_violation", `wire event belongs to run ${event.runId}, expected ${snapshot.runId}`);
	}
	if (event.sequence !== snapshot.lastSequence + 1) {
		return reductionError(
			event,
			"sequence_violation",
			`wire event sequence ${event.sequence} is not contiguous after ${snapshot.lastSequence}`,
		);
	}
	if (event.sequence >= WIRE_MAX_EVENTS_PER_RUN) {
		return reductionError(
			event,
			"sequence_violation",
			`wire run exceeds the ${WIRE_MAX_EVENTS_PER_RUN}-event limit`,
		);
	}
	if (Date.parse(event.at) < Date.parse(snapshot.updatedAt)) {
		return reductionError(event, "sequence_violation", "wire event time regressed");
	}
	const insertedId = insertEventId(snapshotEventIdIndex(snapshot, event), event.id);
	if (!insertedId.inserted) {
		return reductionError(event, "sequence_violation", `wire event id ${event.id} is duplicated`);
	}
	const bytes = snapshotEventBytes(snapshot, event) + encodedEventBytes(event);
	if (bytes > WIRE_MAX_STREAM_BYTES) {
		return reductionError(
			event,
			"state_violation",
			`wire run exceeds the ${WIRE_MAX_STREAM_BYTES}-byte limit`,
		);
	}
	if (isOpaqueEvent(event)
		&& snapshot.opaqueEvents.length >= WIRE_MAX_ACCUMULATED_RECORDS) {
		return reductionError(
			event,
			"state_violation",
			`opaque events exceed the ${WIRE_MAX_ACCUMULATED_RECORDS}-event run limit`,
		);
	}
	const entities = claimEventEntityIds(
		snapshotEntityIdIndex(snapshot, event),
		snapshotEntityKindCounts(snapshot, event),
		event,
	);
	const reduced = isOpaqueEvent(event)
		? Object.freeze({
			...snapshot,
			opaqueEvents: Object.freeze([...snapshot.opaqueEvents, event]),
		})
		: reduceKnown(snapshot, event);
	let eventExtensions = reduced.eventExtensions;
	if (event.extensions) {
		if (Object.keys(eventExtensions).length >= WIRE_MAX_ACCUMULATED_RECORDS) {
			return reductionError(
				event,
				"state_violation",
				`event extensions exceed the ${WIRE_MAX_ACCUMULATED_RECORDS}-event run limit`,
			);
		}
		eventExtensions = withRecordValue(eventExtensions, event.id, event.extensions);
	}
	return indexedSnapshot(Object.freeze({
		...reduced,
		lastSequence: event.sequence,
		lastEventId: event.id,
		updatedAt: event.at,
		eventExtensions,
	}), insertedId.index, entities.index, entities.counts, bytes);
}

export function reduceWireEvents(events: readonly WireEvent[]): WireRunSnapshot {
	let snapshot: WireRunSnapshot | undefined;
	for (const value of events) {
		snapshot = reduceWireEvent(snapshot, value);
	}
	if (!snapshot) {
		throw new WireReductionError("state_violation", "cannot reduce an empty wire event stream");
	}
	return snapshot;
}
