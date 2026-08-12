import {
	wireParentId,
	type WireArtifactId,
	type WireEventId,
	type WireMessageId,
	type WireModelCallId,
	type WireParentId,
	type WireResourceId,
	type WireRunId,
	type WireToolCallId,
} from "./ids";
import type { JsonObject, JsonValue } from "./json";

export const WIRE_VERSION = "north:wire:v2" as const;
export const WIRE_REQUIRED_SEMANTICS = [
	"north.event-order.v1",
	"north.tool-terminal.v1",
	"north.usage-split.v1",
] as const;

export const WIRE_EVENT_KINDS = [
	"run.started",
	"run.progress",
	"message.recorded",
	"model-call.started",
	"model-call.completed",
	"tool.admitted",
	"tool.progress",
	"tool.terminal",
	"artifact.published",
	"resource.pressure",
	"run.terminated",
] as const;

/** Conservative run-local bounds until hierarchical resource admission lands. */
export const WIRE_MAX_EVENTS_PER_RUN = 16_384;
export const WIRE_MAX_STREAM_BYTES = 64 * 1_024 * 1_024;
export const WIRE_MAX_ENTITIES_PER_KIND = 1_024;
export const WIRE_MAX_MESSAGE_CONTENT_PARTS = 8_192;
export const WIRE_MAX_ACCUMULATED_RECORDS = 4_096;

export const WIRE_SEMANTIC_TIERS = ["economy", "standard", "senior", "frontier"] as const;
export const WIRE_EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;
export const WIRE_CAPABILITY_CLASSES = [
	"unknown", "readonly", "readonly-web", "authoring", "orchestrator",
] as const;

export type WireEventKind = typeof WIRE_EVENT_KINDS[number];
export type WireSemanticTier = typeof WIRE_SEMANTIC_TIERS[number];
export type WireEffort = typeof WIRE_EFFORTS[number];
export type WireCapabilityClass = typeof WIRE_CAPABILITY_CLASSES[number];
export type WireRunLifecycle = "running" | "waiting" | "completed" | "failed" | "cancelled" | "blocked";
export type WireTerminalLifecycle = Extract<WireRunLifecycle, "completed" | "failed" | "cancelled" | "blocked">;

export type WireTerminationCode =
	| "completed"
	| "failed"
	| "cancelled"
	| "aborted"
	| "timed_out"
	| "provider_error"
	| "provider_process_died"
	| "resource_denied"
	| "blocked"
	| "synthetic_failure";

export interface WireTerminationReason {
	code: WireTerminationCode;
	detail?: string;
}

export interface WireLifetimeUsage {
	inputTokens: number;
	outputTokens: number;
	cacheReadTokens: number;
	cacheWriteTokens: number;
	reasoningTokens: number;
	modelCalls: number;
}

export interface WireContextUsage {
	tokens: number;
	window?: number;
}

export interface WireUsageSnapshot {
	lifetime: WireLifetimeUsage;
	context: WireContextUsage;
}

export interface WireModelSelection {
	provider: "anthropic" | "openai";
	tier?: WireSemanticTier;
	capabilityClass?: WireCapabilityClass;
}

export const WIRE_PROVIDER_JOIN_VERSION = "north-provider-join:v1" as const;

/** Privacy-bounded provider identity. Raw provider session and turn IDs never cross the adapter. */
export interface WireProviderJoinEvidence {
	version: typeof WIRE_PROVIDER_JOIN_VERSION;
	sessionKey?: string;
	turnKeys: readonly string[];
	sessionPersistence: "persisted" | "ephemeral" | "unknown";
	coverage: "exact" | "partial" | "unknown";
}

export type WireTurnEvidence =
	| { unit: "assistant-turn"; count: number; comparable: true }
	| { unit: "provider-turn"; count: number; toolItems?: number; comparable: false };

export interface WireCompletionLandedCounts {
	completedTurns?: number;
	toolItems?: number;
	mcpCalls?: number;
	nativeCommands?: number;
}

export interface WireCompletionFailureEvidence {
	/** Public provider-neutral code; exactly equals the completion errorCode. */
	detail: string;
	landed?: WireCompletionLandedCounts;
}

export interface WireCompletionInterruptOpenItem {
	kind: string;
	ageMs: number;
}

export interface WireCompletionInterruptEvidence {
	reason: "north_turn_deadline" | "north_post_tool_silence" | "north_in_flight_item_ceiling";
	deadlineMs: number;
	inactivityThresholdMs: number;
	lastActivityAgeMs: number;
	openItemCount?: number;
	openItem?: WireCompletionInterruptOpenItem;
	eventCount: number;
}

export interface WireCompletionEvidence {
	providerJoin?: WireProviderJoinEvidence;
	turns?: WireTurnEvidence;
	/** Provider-reported duration only; North-observed wall time is run evidence. */
	providerDurationMs?: number;
	failure?: WireCompletionFailureEvidence;
	interrupt?: WireCompletionInterruptEvidence;
}

export type WireToolSchemaProvenance =
	| { status: "valid"; source: string; digest: string }
	| { status: "invalid"; source: string; reason: string }
	| { status: "unavailable"; reason: string };

export interface WireRetryState {
	attempt: number;
	maxAttempts: number;
	delayMs: number;
	reason: string;
}

export interface WireFallbackState {
	fromProvider: "anthropic" | "openai";
	toProvider: "anthropic" | "openai";
	reason: string;
	phase: "preaccept";
}

export interface WireNestedProgress {
	runId: WireRunId;
	lifecycle: WireRunLifecycle;
	currentAction?: string;
}

export interface WirePatchState {
	artifactId: WireArtifactId;
	filesChanged: number;
}

export interface WireBranchState {
	name: string;
	base?: string;
}

export type WireOuterAbortActivityKind =
	| "message"
	| "model"
	| "tool"
	| "artifact"
	| "compaction"
	| "activity";

export type WireProviderAbortActivityKind =
	| "turn"
	| "item"
	| "tool"
	| "progress"
	| "frame"
	| "activity";

export interface WireAbortActivityEvidence<Origin extends "outer" | "provider"> {
	origin: Origin;
	kind: Origin extends "outer" ? WireOuterAbortActivityKind : WireProviderAbortActivityKind;
	observedAt: string;
}

export interface WireWatchdogAbortEvidence {
	silenceMs: number;
	lastOuter?: WireAbortActivityEvidence<"outer">;
	lastProvider?: WireAbortActivityEvidence<"provider">;
}

export interface WireAbortEvidence {
	requestedAt: string;
	source: "operator" | "parent" | "watchdog" | "provider" | "runtime";
	reason: string;
	watchdog?: WireWatchdogAbortEvidence;
}

export interface WireRecentTool {
	toolCallId: WireToolCallId;
	name: string;
	status: "succeeded" | "failed" | "cancelled" | "synthetic_failure";
	endedAt: string;
}

export type WireOutputReference =
	| { kind: "artifact"; artifactId: WireArtifactId }
	| { kind: "resource"; resourceId: WireResourceId };

export interface WireProgressPatch {
	currentAction?: string | null;
	/** Cumulative provider-confirmed context compactions for this run. */
	compactions?: number;
	outputReferences?: readonly WireOutputReference[] | null;
	model?: WireModelSelection | null;
	effort?: WireEffort | null;
	retry?: WireRetryState | null;
	fallback?: WireFallbackState | null;
	nested?: readonly WireNestedProgress[] | null;
	patch?: WirePatchState | null;
	branch?: WireBranchState | null;
	abort?: WireAbortEvidence | null;
	usage?: WireUsageSnapshot;
}

export interface WireEventEnvelope {
	version: string;
	id: WireEventId;
	runId: WireRunId;
	parentId?: WireParentId;
	sequence: number;
	at: string;
	kind: string;
	essential: boolean;
	requiredSemantics: readonly string[];
	extensions?: JsonObject;
}

interface WireKnownEventEnvelope<K extends WireEventKind> extends WireEventEnvelope {
	version: typeof WIRE_VERSION;
	kind: K;
	essential: true;
	requiredSemantics: typeof WIRE_REQUIRED_SEMANTICS;
}

export interface WireRunStartedEvent extends WireKnownEventEnvelope<"run.started"> {
	lifecycle: "running";
	parentRunId?: WireRunId;
	owner?: string;
}

export interface WireRunProgressEvent extends WireKnownEventEnvelope<"run.progress"> {
	lifecycle: "running" | "waiting";
	progress: WireProgressPatch;
}

export interface WireMessageRecordedEvent extends WireKnownEventEnvelope<"message.recorded"> {
	messageId: WireMessageId;
	stage: "started" | "delta" | "completed";
	role: "user" | "assistant" | "tool" | "system";
	content?: JsonValue;
	modelCallId?: WireModelCallId;
	parentToolCallId?: WireToolCallId;
}

export interface WireModelCallStartedEvent extends WireKnownEventEnvelope<"model-call.started"> {
	modelCallId: WireModelCallId;
	model: WireModelSelection;
	effort?: WireEffort;
	attempt: number;
}

/** Authority for this completed call's contribution to cumulative run usage. */
export type WireModelCallUsageCoverage = "exact" | "partial" | "unavailable";

export interface WireModelCallCompletedEvent extends WireKnownEventEnvelope<"model-call.completed"> {
	modelCallId: WireModelCallId;
	status: "succeeded" | "failed" | "cancelled";
	origin: "provider" | "north";
	usage: WireUsageSnapshot;
	usageCoverage: WireModelCallUsageCoverage;
	errorCode?: string;
	evidence?: WireCompletionEvidence;
}

export interface WireToolAdmittedEvent extends WireKnownEventEnvelope<"tool.admitted"> {
	toolCallId: WireToolCallId;
	name: string;
	messageId?: WireMessageId;
	modelCallId?: WireModelCallId;
	parentToolCallId?: WireToolCallId;
	schema: WireToolSchemaProvenance;
	argumentPreview?: string;
	argumentArtifactId?: WireArtifactId;
}

export interface WireToolProgressEvent extends WireKnownEventEnvelope<"tool.progress"> {
	toolCallId: WireToolCallId;
	progress?: JsonValue;
	outputArtifactId?: WireArtifactId;
}

export interface WireToolTerminalEvent extends WireKnownEventEnvelope<"tool.terminal"> {
	toolCallId: WireToolCallId;
	status: "succeeded" | "failed" | "cancelled" | "synthetic_failure";
	origin: "provider" | "north";
	resultPreview?: string;
	resultArtifactId?: WireArtifactId;
	resultArtifactDigest?: string;
	errorCode?: string;
}

export interface WireArtifactPublishedEvent extends WireKnownEventEnvelope<"artifact.published"> {
	artifactId: WireArtifactId;
	resourceId?: WireResourceId;
	mediaType: string;
	bytes: number;
	digest?: string;
	label?: string;
}

export interface WireResourcePressureEvent extends WireKnownEventEnvelope<"resource.pressure"> {
	resourceId?: WireResourceId;
	scope: string;
	resource: string;
	used: number;
	reserved: number;
	limit: number;
	advisory: boolean;
}

export interface WireRunTerminatedEvent extends WireKnownEventEnvelope<"run.terminated"> {
	lifecycle: WireTerminalLifecycle;
	reason: WireTerminationReason;
	abort?: WireAbortEvidence;
}

export type WireKnownEvent =
	| WireRunStartedEvent
	| WireRunProgressEvent
	| WireMessageRecordedEvent
	| WireModelCallStartedEvent
	| WireModelCallCompletedEvent
	| WireToolAdmittedEvent
	| WireToolProgressEvent
	| WireToolTerminalEvent
	| WireArtifactPublishedEvent
	| WireResourcePressureEvent
	| WireRunTerminatedEvent;

export interface WireOpaqueEvent extends WireEventEnvelope {
	essential: false;
}

export type WireEvent = WireKnownEvent | WireOpaqueEvent;

type GeneratedEnvelopeField =
	| "version" | "id" | "runId" | "sequence" | "at"
	| "essential" | "requiredSemantics";
type OptionalEnvelopeField = "extensions";
type WireEventDraftFor<Event extends WireKnownEvent> =
	Omit<Event, GeneratedEnvelopeField | OptionalEnvelopeField | "parentId">
	& Partial<Pick<Event, OptionalEnvelopeField>>;

export type WireEventDraft = WireKnownEvent extends infer Event
	? Event extends WireKnownEvent
		? WireEventDraftFor<Event>
		: never
	: never;

/** The sole parentage rule used by producers and live/replay reduction. */
export function expectedWireParentId(
	event: WireKnownEvent | WireEventDraft,
	runId: WireRunId,
): WireParentId | undefined {
	switch (event.kind) {
		case "run.started":
			return event.parentRunId === undefined ? undefined : wireParentId(event.parentRunId);
		case "run.progress":
		case "model-call.started":
		case "run.terminated":
			return wireParentId(runId);
		case "artifact.published":
		case "resource.pressure":
			return wireParentId(event.resourceId ?? runId);
		case "model-call.completed":
			return wireParentId(event.modelCallId);
		case "message.recorded":
			if (event.stage !== "started") return wireParentId(event.messageId);
			return wireParentId(event.parentToolCallId ?? event.modelCallId ?? runId);
		case "tool.admitted":
			return wireParentId(
				event.messageId ?? event.parentToolCallId ?? event.modelCallId ?? runId,
			);
		case "tool.progress":
		case "tool.terminal":
			return wireParentId(event.toolCallId);
	}
	const exhaustive: never = event;
	return exhaustive;
}
