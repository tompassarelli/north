import {
	WIRE_EVENT_KINDS,
	WIRE_CAPABILITY_CLASSES,
	WIRE_EFFORTS,
	WIRE_PROVIDER_JOIN_VERSION,
	WIRE_REQUIRED_SEMANTICS,
	WIRE_SEMANTIC_TIERS,
	WIRE_VERSION,
	type WireAbortEvidence,
	type WireAbortActivityEvidence,
	type WireCompletionEvidence,
	type WireCompletionFailureEvidence,
	type WireCompletionInterruptEvidence,
	type WireCompletionLandedCounts,
	type WireEvent,
	type WireEventEnvelope,
	type WireEventKind,
	type WireKnownEvent,
	type WireOpaqueEvent,
	type WireProgressPatch,
	type WireProviderJoinEvidence,
	type WireTerminationReason,
	type WireToolSchemaProvenance,
	type WireTurnEvidence,
	type WireUsageSnapshot,
} from "./events";
import { WireDecodeError } from "./errors";
import {
	wireArtifactId,
	wireEventId,
	wireMessageId,
	wireModelCallId,
	wireParentId,
	wireResourceId,
	wireRunId,
	wireToolCallId,
} from "./ids";
import { jsonObject, type JsonObject, type JsonValue } from "./json";
import { isProviderNeutralWireErrorCode } from "./semantics";

const EVENT_KIND_SET = new Set<string>(WIRE_EVENT_KINDS);
const REQUIRED_SEMANTIC_SET = new Set<string>(WIRE_REQUIRED_SEMANTICS);
const COMMON_KEYS = [
	"version", "id", "runId", "parentId", "sequence", "at", "kind", "essential",
	"requiredSemantics", "extensions",
] as const;
const PAYLOAD_KEYS: Readonly<Record<WireEventKind, readonly string[]>> = {
	"run.started": ["lifecycle", "parentRunId", "owner"],
	"run.progress": ["lifecycle", "progress"],
	"message.recorded": [
		"messageId", "stage", "role", "content", "modelCallId", "parentToolCallId",
	],
	"model-call.started": ["modelCallId", "model", "effort", "attempt"],
	"model-call.completed": [
		"modelCallId", "status", "origin", "usage", "usageCoverage", "errorCode", "evidence",
	],
	"tool.admitted": [
		"toolCallId", "name", "messageId", "modelCallId", "parentToolCallId", "schema",
		"argumentDigest", "argumentPreview", "argumentArtifactId",
	],
	"tool.progress": ["toolCallId", "progress", "outputArtifactId"],
	"tool.terminal": [
		"toolCallId", "status", "origin", "resultPreview", "resultArtifactId",
		"resultArtifactDigest", "errorCode",
	],
	"artifact.published": ["artifactId", "resourceId", "mediaType", "bytes", "digest", "label"],
	"resource.pressure": ["resourceId", "scope", "resource", "used", "reserved", "limit", "advisory"],
	"run.terminated": ["lifecycle", "reason", "abort"],
};

function malformed(message: string): never {
	throw new WireDecodeError("malformed_event", message);
}

function record(value: JsonValue | undefined, label: string): JsonObject {
	if (value === null || typeof value !== "object" || Array.isArray(value)) {
		return malformed(`${label} must be an object`);
	}
	return value as JsonObject;
}

function text(value: JsonValue | undefined, label: string, maxLength = 65_536): string {
	if (typeof value !== "string" || value.length === 0 || value.length > maxLength) {
		return malformed(`${label} must be a non-empty string of at most ${maxLength} characters`);
	}
	return value;
}

function optionalText(value: JsonValue | undefined, label: string, maxLength = 65_536): string | undefined {
	return value === undefined ? undefined : text(value, label, maxLength);
}

function sha256Digest(value: JsonValue | undefined, label: string): string {
	const parsed = text(value, label, 64);
	if (!/^[a-f0-9]{64}$/.test(parsed)) {
		malformed(`${label} must be a 64-character lowercase SHA-256 digest`);
	}
	return parsed;
}

function publicErrorCode(value: JsonValue | undefined, label: string): string {
	const parsed = text(value, label, 256);
	if (!isProviderNeutralWireErrorCode(parsed)) {
		malformed(`${label} must be a provider-neutral lowercase snake-case code`);
	}
	return parsed;
}

function optionalPublicErrorCode(value: JsonValue | undefined, label: string): string | undefined {
	return value === undefined ? undefined : publicErrorCode(value, label);
}

function oneOf<const Values extends readonly string[]>(
	value: JsonValue | undefined,
	values: Values,
	label: string,
): Values[number] {
	const parsed = text(value, label, 128);
	if (!(values as readonly string[]).includes(parsed)) return malformed(`${label} is unsupported: ${parsed}`);
	return parsed as Values[number];
}

function count(value: JsonValue | undefined, label: string): number {
	if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
		return malformed(`${label} must be a non-negative safe integer`);
	}
	return value;
}

function positiveCount(value: JsonValue | undefined, label: string): number {
	const parsed = count(value, label);
	if (parsed === 0) return malformed(`${label} must be positive`);
	return parsed;
}

function finiteNumber(value: JsonValue | undefined, label: string): number {
	if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
		return malformed(`${label} must be a non-negative finite number`);
	}
	return value;
}

function flag(value: JsonValue | undefined, label: string): boolean {
	if (typeof value !== "boolean") return malformed(`${label} must be boolean`);
	return value;
}

function instant(value: JsonValue | undefined, label: string): string {
	const parsed = text(value, label, 64);
	const timestamp = Date.parse(parsed);
	if (!Number.isFinite(timestamp) || new Date(timestamp).toISOString() !== parsed) {
		return malformed(`${label} must be a canonical UTC ISO timestamp`);
	}
	return parsed;
}

function optionalId<T>(
	value: JsonValue | undefined,
	label: string,
	parse: (source: string) => T,
): T | undefined {
	if (value === undefined) return undefined;
	try {
		return parse(text(value, label, 256));
	} catch (error) {
		return malformed(error instanceof Error ? error.message : `${label} is invalid`);
	}
}

function requiredId<T>(
	value: JsonValue | undefined,
	label: string,
	parse: (source: string) => T,
): T {
	return optionalId(value, label, parse) ?? malformed(`${label} is required`);
}

function exactKeys(source: JsonObject, keys: readonly string[], label: string): void {
	const unknown = Object.keys(source).filter((key) => !keys.includes(key));
	if (unknown.length) malformed(`${label} has unknown fields: ${unknown.join(", ")}`);
}

function semanticList(value: JsonValue | undefined): readonly string[] {
	if (!Array.isArray(value) || value.length > 32) {
		return malformed("wire event requiredSemantics must be an array of at most 32 strings");
	}
	const parsed = value.map((item, index) => text(item, `requiredSemantics[${index}]`, 128));
	if (new Set(parsed).size !== parsed.length) malformed("wire event requiredSemantics must be unique");
	return Object.freeze(parsed);
}

function assertSupportedSemantics(required: readonly string[]): void {
	const unsupported = required.filter((semantic) => !REQUIRED_SEMANTIC_SET.has(semantic));
	if (unsupported.length) {
		throw new WireDecodeError(
			"unsupported_required_semantics",
			`wire event requires unsupported semantics: ${unsupported.join(", ")}`,
		);
	}
}

function usageSnapshot(value: JsonValue | undefined, label: string): WireUsageSnapshot {
	const source = record(value, label);
	exactKeys(source, ["lifetime", "context"], label);
	const lifetime = record(source.lifetime, `${label}.lifetime`);
	exactKeys(lifetime, [
		"inputTokens", "outputTokens", "cacheReadTokens", "cacheWriteTokens", "reasoningTokens", "modelCalls",
	], `${label}.lifetime`);
	const context = record(source.context, `${label}.context`);
	exactKeys(context, ["tokens", "window"], `${label}.context`);
	const tokens = count(context.tokens, `${label}.context.tokens`);
	const window = context.window === undefined ? undefined : positiveCount(context.window, `${label}.context.window`);
	if (window !== undefined && tokens > window) malformed(`${label}.context.tokens exceeds its window`);
	return Object.freeze({
		lifetime: Object.freeze({
			inputTokens: count(lifetime.inputTokens, `${label}.lifetime.inputTokens`),
			outputTokens: count(lifetime.outputTokens, `${label}.lifetime.outputTokens`),
			cacheReadTokens: count(lifetime.cacheReadTokens, `${label}.lifetime.cacheReadTokens`),
			cacheWriteTokens: count(lifetime.cacheWriteTokens, `${label}.lifetime.cacheWriteTokens`),
			reasoningTokens: count(lifetime.reasoningTokens, `${label}.lifetime.reasoningTokens`),
			modelCalls: count(lifetime.modelCalls, `${label}.lifetime.modelCalls`),
		}),
		context: Object.freeze({ tokens, ...(window === undefined ? {} : { window }) }),
	});
}

function modelSelection(value: JsonValue | undefined, label: string) {
	const source = record(value, label);
	exactKeys(source, ["provider", "tier", "capabilityClass"], label);
	return Object.freeze({
		provider: oneOf(source.provider, ["anthropic", "openai"] as const, `${label}.provider`),
		...(source.tier === undefined ? {} : { tier: oneOf(source.tier, WIRE_SEMANTIC_TIERS, `${label}.tier`) }),
		...(source.capabilityClass === undefined ? {} : {
			capabilityClass: oneOf(
				source.capabilityClass,
				WIRE_CAPABILITY_CLASSES,
				`${label}.capabilityClass`,
			),
		}),
	});
}

function normalizedLine(value: JsonValue | undefined, label: string, maxLength: number): string {
	const parsed = text(value, label, maxLength);
	if (parsed !== parsed.replace(/\s+/gu, " ").trim()) {
		malformed(`${label} must be a normalized single line`);
	}
	if (/[\u0000-\u001f\u007f-\u009f]/u.test(parsed)) {
		malformed(`${label} must not contain control characters`);
	}
	return parsed;
}

function providerJoinEvidence(value: JsonValue | undefined): WireProviderJoinEvidence {
	const source = record(value, "model-call completion providerJoin");
	exactKeys(source, [
		"version", "sessionKey", "turnKeys", "sessionPersistence", "coverage",
	], "model-call completion providerJoin");
	if (source.version !== WIRE_PROVIDER_JOIN_VERSION) {
		malformed(`model-call completion providerJoin version must be ${WIRE_PROVIDER_JOIN_VERSION}`);
	}
	const sessionKey = optionalText(source.sessionKey, "model-call completion providerJoin sessionKey", 64);
	if (sessionKey !== undefined && !/^[a-f0-9]{64}$/.test(sessionKey)) {
		malformed("model-call completion providerJoin sessionKey must be a SHA-256 digest");
	}
	if (!Array.isArray(source.turnKeys) || source.turnKeys.length > 4_096) {
		malformed("model-call completion providerJoin turnKeys must contain at most 4096 entries");
	}
	const turnKeys = source.turnKeys.map((item, index) => {
		const key = text(item, `model-call completion providerJoin turnKeys[${index}]`, 64);
		if (!/^[a-f0-9]{64}$/.test(key)) {
			malformed(`model-call completion providerJoin turnKeys[${index}] must be a SHA-256 digest`);
		}
		return key;
	});
	if (new Set(turnKeys).size !== turnKeys.length) {
		malformed("model-call completion providerJoin turnKeys must be unique");
	}
	for (let index = 1; index < turnKeys.length; index += 1) {
		if (turnKeys[index - 1]! > turnKeys[index]!) {
			malformed("model-call completion providerJoin turnKeys must be sorted");
		}
	}
	const coverage = oneOf(
		source.coverage,
		["exact", "partial", "unknown"] as const,
		"model-call completion providerJoin coverage",
	);
	if (coverage === "exact" && (sessionKey === undefined || turnKeys.length === 0)) {
		malformed("exact providerJoin coverage requires a session key and at least one turn key");
	}
	if (coverage === "partial" && sessionKey === undefined && turnKeys.length === 0) {
		malformed("partial providerJoin coverage requires a session key or at least one turn key");
	}
	return Object.freeze({
		version: WIRE_PROVIDER_JOIN_VERSION,
		...(sessionKey === undefined ? {} : { sessionKey }),
		turnKeys: Object.freeze(turnKeys),
		sessionPersistence: oneOf(
			source.sessionPersistence,
			["persisted", "ephemeral", "unknown"] as const,
			"model-call completion providerJoin sessionPersistence",
		),
		coverage,
	});
}

function turnEvidence(value: JsonValue | undefined): WireTurnEvidence {
	const source = record(value, "model-call completion turns");
	const unit = oneOf(
		source.unit,
		["assistant-turn", "provider-turn"] as const,
		"model-call completion turns unit",
	);
	if (unit === "assistant-turn") {
		exactKeys(source, ["unit", "count", "comparable"], "assistant-turn evidence");
		if (source.comparable !== true) malformed("assistant-turn evidence comparable must be true");
		return Object.freeze({
			unit,
			count: count(source.count, "assistant-turn evidence count"),
			comparable: true,
		});
	}
	exactKeys(source, ["unit", "count", "toolItems", "comparable"], "provider-turn evidence");
	if (source.comparable !== false) malformed("provider-turn evidence comparable must be false");
	return Object.freeze({
		unit,
		count: count(source.count, "provider-turn evidence count"),
		...(source.toolItems === undefined ? {} : {
			toolItems: count(source.toolItems, "provider-turn evidence toolItems"),
		}),
		comparable: false,
	});
}

function landedCounts(value: JsonValue | undefined): WireCompletionLandedCounts {
	const source = record(value, "model-call completion failure landed counts");
	const keys = ["completedTurns", "toolItems", "mcpCalls", "nativeCommands"] as const;
	exactKeys(source, keys, "model-call completion failure landed counts");
	if (!keys.some((key) => source[key] !== undefined)) {
		malformed("model-call completion failure landed counts must contain an observed count");
	}
	return Object.freeze({
		...(source.completedTurns === undefined ? {} : {
			completedTurns: count(source.completedTurns, "failure landed completedTurns"),
		}),
		...(source.toolItems === undefined ? {} : {
			toolItems: count(source.toolItems, "failure landed toolItems"),
		}),
		...(source.mcpCalls === undefined ? {} : {
			mcpCalls: count(source.mcpCalls, "failure landed mcpCalls"),
		}),
		...(source.nativeCommands === undefined ? {} : {
			nativeCommands: count(source.nativeCommands, "failure landed nativeCommands"),
		}),
	});
}

function completionFailureEvidence(value: JsonValue | undefined): WireCompletionFailureEvidence {
	const source = record(value, "model-call completion failure");
	exactKeys(source, ["detail", "landed"], "model-call completion failure");
	return Object.freeze({
		detail: normalizedLine(source.detail, "model-call completion failure detail", 1_200),
		...(source.landed === undefined ? {} : { landed: landedCounts(source.landed) }),
	});
}

function completionInterruptEvidence(value: JsonValue | undefined): WireCompletionInterruptEvidence {
	const source = record(value, "model-call completion interrupt");
	exactKeys(source, [
		"reason", "deadlineMs", "inactivityThresholdMs", "lastActivityAgeMs",
		"openItemCount", "openItem", "eventCount",
	], "model-call completion interrupt");
	const eventCount = count(source.eventCount, "model-call completion interrupt eventCount");
	let openItem: WireCompletionInterruptEvidence["openItem"];
	if (source.openItem !== undefined) {
		const item = record(source.openItem, "model-call completion interrupt openItem");
		exactKeys(item, ["kind", "ageMs"], "model-call completion interrupt openItem");
		const kind = text(item.kind, "model-call completion interrupt openItem kind", 128);
		if (!/^[a-z][A-Za-z0-9._/-]{0,127}$/.test(kind)) {
			malformed("model-call completion interrupt openItem kind is invalid");
		}
		openItem = Object.freeze({
			kind,
			ageMs: count(item.ageMs, "model-call completion interrupt openItem ageMs"),
		});
	}
	const openItemCount = source.openItemCount === undefined ? undefined
		: count(source.openItemCount, "model-call completion interrupt openItemCount");
	if (openItem !== undefined && (openItemCount === undefined || openItemCount === 0)) {
		malformed("model-call completion interrupt openItem requires a positive openItemCount");
	}
	return Object.freeze({
		reason: oneOf(
			source.reason,
			[
				"north_turn_deadline",
				"north_post_tool_silence",
				"north_in_flight_item_ceiling",
			] as const,
			"model-call completion interrupt reason",
		),
		deadlineMs: positiveCount(source.deadlineMs, "model-call completion interrupt deadlineMs"),
		inactivityThresholdMs: positiveCount(
			source.inactivityThresholdMs,
			"model-call completion interrupt inactivityThresholdMs",
		),
		lastActivityAgeMs: count(
			source.lastActivityAgeMs,
			"model-call completion interrupt lastActivityAgeMs",
		),
		...(openItemCount === undefined ? {} : { openItemCount }),
		...(openItem === undefined ? {} : { openItem }),
		eventCount,
	});
}

function completionEvidence(value: JsonValue | undefined): WireCompletionEvidence {
	const source = record(value, "model-call completion evidence");
	const keys = ["providerJoin", "turns", "providerDurationMs", "failure", "interrupt"] as const;
	exactKeys(source, keys, "model-call completion evidence");
	if (!keys.some((key) => source[key] !== undefined)) {
		malformed("model-call completion evidence must contain observed evidence");
	}
	return Object.freeze({
		...(source.providerJoin === undefined ? {} : {
			providerJoin: providerJoinEvidence(source.providerJoin),
		}),
		...(source.turns === undefined ? {} : { turns: turnEvidence(source.turns) }),
		...(source.providerDurationMs === undefined ? {} : {
			providerDurationMs: count(source.providerDurationMs, "model-call completion providerDurationMs"),
		}),
		...(source.failure === undefined ? {} : {
			failure: completionFailureEvidence(source.failure),
		}),
		...(source.interrupt === undefined ? {} : {
			interrupt: completionInterruptEvidence(source.interrupt),
		}),
	});
}

function abortActivityEvidence(
	value: JsonValue | undefined,
	label: string,
	origin: "outer",
): WireAbortActivityEvidence<"outer">;
function abortActivityEvidence(
	value: JsonValue | undefined,
	label: string,
	origin: "provider",
): WireAbortActivityEvidence<"provider">;
function abortActivityEvidence(
	value: JsonValue | undefined,
	label: string,
	origin: "outer" | "provider",
): WireAbortActivityEvidence<"outer"> | WireAbortActivityEvidence<"provider"> {
	const source = record(value, label);
	exactKeys(source, ["origin", "kind", "observedAt"], label);
	if (origin === "outer") {
		return Object.freeze({
			origin: oneOf(source.origin, ["outer"] as const, `${label}.origin`),
			kind: oneOf(
				source.kind,
				["message", "model", "tool", "artifact", "compaction", "activity"] as const,
				`${label}.kind`,
			),
			observedAt: instant(source.observedAt, `${label}.observedAt`),
		});
	}
	return Object.freeze({
		origin: oneOf(source.origin, ["provider"] as const, `${label}.origin`),
		kind: oneOf(
			source.kind,
			["turn", "item", "tool", "progress", "frame", "activity"] as const,
			`${label}.kind`,
		),
		observedAt: instant(source.observedAt, `${label}.observedAt`),
	});
}

function watchdogAbortEvidence(value: JsonValue | undefined, label: string) {
	const source = record(value, label);
	exactKeys(source, ["silenceMs", "lastOuter", "lastProvider"], label);
	return Object.freeze({
		silenceMs: positiveCount(source.silenceMs, `${label}.silenceMs`),
		...(source.lastOuter === undefined ? {} : {
			lastOuter: abortActivityEvidence(source.lastOuter, `${label}.lastOuter`, "outer"),
		}),
		...(source.lastProvider === undefined ? {} : {
			lastProvider: abortActivityEvidence(source.lastProvider, `${label}.lastProvider`, "provider"),
		}),
	});
}

function abortEvidence(value: JsonValue | undefined, label: string): WireAbortEvidence {
	const source = record(value, label);
	exactKeys(source, ["requestedAt", "source", "reason", "watchdog"], label);
	const abortSource = oneOf(
		source.source,
		["operator", "parent", "watchdog", "provider", "runtime"] as const,
		`${label}.source`,
	);
	const reason = text(source.reason, `${label}.reason`, 1_024);
	const canonicalWatchdog = abortSource === "watchdog"
		&& reason === "north_watchdog_execution_inactivity";
	if (abortSource === "watchdog" && !canonicalWatchdog) {
		malformed(`${label} watchdog source requires the canonical inactivity reason`);
	}
	if (source.watchdog !== undefined && !canonicalWatchdog) {
		malformed(`${label}.watchdog requires the canonical watchdog inactivity reason`);
	}
	if (canonicalWatchdog && source.watchdog === undefined) {
		malformed(`${label}.watchdog is required for canonical watchdog inactivity`);
	}
	return Object.freeze({
		requestedAt: instant(source.requestedAt, `${label}.requestedAt`),
		source: abortSource,
		reason,
		...(source.watchdog === undefined ? {} : {
			watchdog: watchdogAbortEvidence(source.watchdog, `${label}.watchdog`),
		}),
	});
}

function toolSchema(value: JsonValue | undefined): WireToolSchemaProvenance {
	const source = record(value, "tool schema provenance");
	const status = oneOf(source.status, ["valid", "invalid", "unavailable"] as const, "tool schema status");
	if (status === "valid") {
		exactKeys(source, ["status", "source", "digest"], "valid tool schema provenance");
		const digest = text(source.digest, "tool schema digest", 128);
		if (!/^[a-f0-9]{64}$/.test(digest)) malformed("tool schema digest must be 64 lowercase hexadecimal characters");
		return Object.freeze({ status, source: text(source.source, "tool schema source", 128), digest });
	}
	if (status === "invalid") {
		exactKeys(source, ["status", "source", "reason"], "invalid tool schema provenance");
		return Object.freeze({
			status,
			source: text(source.source, "tool schema source", 128),
			reason: text(source.reason, "tool schema reason", 1_024),
		});
	}
	exactKeys(source, ["status", "reason"], "unavailable tool schema provenance");
	return Object.freeze({ status, reason: text(source.reason, "tool schema reason", 1_024) });
}

function outputReferences(value: JsonValue | undefined) {
	if (!Array.isArray(value) || value.length > 256) malformed("outputReferences must contain at most 256 entries");
	return Object.freeze(value.map((item, index) => {
		const source = record(item, `outputReferences[${index}]`);
		const kind = oneOf(source.kind, ["artifact", "resource"] as const, `outputReferences[${index}].kind`);
		if (kind === "artifact") {
			exactKeys(source, ["kind", "artifactId"], `outputReferences[${index}]`);
			return Object.freeze({
				kind,
				artifactId: requiredId(source.artifactId, `outputReferences[${index}].artifactId`, wireArtifactId),
			});
		}
		exactKeys(source, ["kind", "resourceId"], `outputReferences[${index}]`);
		return Object.freeze({
			kind,
			resourceId: requiredId(source.resourceId, `outputReferences[${index}].resourceId`, wireResourceId),
		});
	}));
}

function nestedProgress(value: JsonValue | undefined) {
	if (!Array.isArray(value) || value.length > 128) malformed("nested progress must contain at most 128 entries");
	return Object.freeze(value.map((item, index) => {
		const source = record(item, `nested[${index}]`);
		exactKeys(source, ["runId", "lifecycle", "currentAction"], `nested[${index}]`);
		return Object.freeze({
			runId: requiredId(source.runId, `nested[${index}].runId`, wireRunId),
			lifecycle: oneOf(
				source.lifecycle,
				["running", "waiting", "completed", "failed", "cancelled", "blocked"] as const,
				`nested[${index}].lifecycle`,
			),
			...(source.currentAction === undefined ? {} : {
				currentAction: text(source.currentAction, `nested[${index}].currentAction`, 4_096),
			}),
		});
	}));
}

function progressPatch(value: JsonValue | undefined): WireProgressPatch {
	const source = record(value, "run progress");
	exactKeys(source, [
		"currentAction", "compactions", "outputReferences", "model", "effort", "retry", "fallback",
		"nested", "patch", "branch", "abort", "usage",
	], "run progress");
	const result: Record<string, unknown> = {};
	if (source.currentAction !== undefined) result.currentAction = source.currentAction === null
		? null : text(source.currentAction, "run progress currentAction", 4_096);
	if (source.compactions !== undefined) {
		result.compactions = count(source.compactions, "run progress compactions");
	}
	if (source.outputReferences !== undefined) result.outputReferences = source.outputReferences === null
		? null : outputReferences(source.outputReferences);
	if (source.model !== undefined) result.model = source.model === null
		? null : modelSelection(source.model, "run progress model");
	if (source.effort !== undefined) result.effort = source.effort === null
		? null : oneOf(source.effort, WIRE_EFFORTS, "run progress effort");
	if (source.retry !== undefined) {
		if (source.retry === null) result.retry = null;
		else {
			const retry = record(source.retry, "run progress retry");
			exactKeys(retry, ["attempt", "maxAttempts", "delayMs", "reason"], "run progress retry");
			const attempt = positiveCount(retry.attempt, "run progress retry attempt");
			const maxAttempts = positiveCount(retry.maxAttempts, "run progress retry maxAttempts");
			if (attempt > maxAttempts) malformed("run progress retry attempt exceeds maxAttempts");
			result.retry = Object.freeze({
				attempt, maxAttempts,
				delayMs: count(retry.delayMs, "run progress retry delayMs"),
				reason: text(retry.reason, "run progress retry reason", 1_024),
			});
		}
	}
	if (source.fallback !== undefined) {
		if (source.fallback === null) result.fallback = null;
		else {
			const fallback = record(source.fallback, "run progress fallback");
			exactKeys(fallback, ["fromProvider", "toProvider", "reason", "phase"], "run progress fallback");
			const fromProvider = oneOf(fallback.fromProvider, ["anthropic", "openai"] as const, "fallback fromProvider");
			const toProvider = oneOf(fallback.toProvider, ["anthropic", "openai"] as const, "fallback toProvider");
			if (fromProvider === toProvider) malformed("fallback providers must differ");
			result.fallback = Object.freeze({
				fromProvider,
				toProvider,
				reason: text(fallback.reason, "fallback reason", 1_024),
				phase: oneOf(fallback.phase, ["preaccept"] as const, "fallback phase"),
			});
		}
	}
	if (source.nested !== undefined) result.nested = source.nested === null ? null : nestedProgress(source.nested);
	if (source.patch !== undefined) {
		if (source.patch === null) result.patch = null;
		else {
			const patch = record(source.patch, "run progress patch");
			exactKeys(patch, ["artifactId", "filesChanged"], "run progress patch");
			result.patch = Object.freeze({
				artifactId: requiredId(patch.artifactId, "run progress patch artifactId", wireArtifactId),
				filesChanged: count(patch.filesChanged, "run progress patch filesChanged"),
			});
		}
	}
	if (source.branch !== undefined) {
		if (source.branch === null) result.branch = null;
		else {
			const branch = record(source.branch, "run progress branch");
			exactKeys(branch, ["name", "base"], "run progress branch");
			result.branch = Object.freeze({
				name: text(branch.name, "run progress branch name", 256),
				...(branch.base === undefined ? {} : { base: text(branch.base, "run progress branch base", 256) }),
			});
		}
	}
	if (source.abort !== undefined) result.abort = source.abort === null ? null : abortEvidence(source.abort, "run progress abort");
	if (source.usage !== undefined) result.usage = usageSnapshot(source.usage, "run progress usage");
	return Object.freeze(result) as unknown as WireProgressPatch;
}

function terminationReason(value: JsonValue | undefined): WireTerminationReason {
	const source = record(value, "run termination reason");
	exactKeys(source, ["code", "detail"], "run termination reason");
	const code = oneOf(source.code, [
		"completed", "failed", "cancelled", "aborted", "timed_out", "provider_error",
		"provider_process_died", "resource_denied", "blocked", "synthetic_failure",
	] as const, "run termination code");
	const detail = optionalText(source.detail, "run termination detail", 4_096);
	if (detail !== undefined && detail !== code) {
		malformed("run termination detail must equal its public code");
	}
	return Object.freeze({
		code,
		...(detail === undefined ? {} : { detail }),
	});
}

function validatePayload(kind: WireEventKind, source: JsonObject): void {
	switch (kind) {
		case "run.started":
			oneOf(source.lifecycle, ["running"] as const, "run.started lifecycle");
			optionalId(source.parentRunId, "run.started parentRunId", wireRunId);
			optionalText(source.owner, "run.started owner", 256);
			return;
		case "run.progress":
			oneOf(source.lifecycle, ["running", "waiting"] as const, "run.progress lifecycle");
			progressPatch(source.progress);
			return;
		case "message.recorded":
			requiredId(source.messageId, "message.recorded messageId", wireMessageId);
			oneOf(source.stage, ["started", "delta", "completed"] as const, "message.recorded stage");
			oneOf(source.role, ["user", "assistant", "tool", "system"] as const, "message.recorded role");
			optionalId(source.modelCallId, "message.recorded modelCallId", wireModelCallId);
			optionalId(source.parentToolCallId, "message.recorded parentToolCallId", wireToolCallId);
			return;
		case "model-call.started":
			requiredId(source.modelCallId, "model-call.started modelCallId", wireModelCallId);
			modelSelection(source.model, "model-call.started model");
			if (source.effort !== undefined) oneOf(source.effort, WIRE_EFFORTS, "model-call.started effort");
			positiveCount(source.attempt, "model-call.started attempt");
			return;
		case "model-call.completed": {
			requiredId(source.modelCallId, "model-call.completed modelCallId", wireModelCallId);
			const status = oneOf(
				source.status,
				["succeeded", "failed", "cancelled"] as const,
				"model-call.completed status",
			);
			const origin = oneOf(
				source.origin,
				["provider", "north"] as const,
				"model-call.completed origin",
			);
			usageSnapshot(source.usage, "model-call.completed usage");
			const usageCoverage = oneOf(
				source.usageCoverage,
				["exact", "partial", "unavailable"] as const,
				"model-call.completed usageCoverage",
			);
			if (usageCoverage === "exact" && origin !== "provider") {
				malformed("model-call.completed exact usage requires provider origin");
			}
			const errorCode = optionalPublicErrorCode(
				source.errorCode,
				"model-call.completed errorCode",
			);
			if (source.evidence !== undefined) {
				const evidence = completionEvidence(source.evidence);
				if (status === "succeeded" && (evidence.failure !== undefined || evidence.interrupt !== undefined)) {
					malformed("successful model-call completion cannot carry failure or interrupt evidence");
				}
				if (evidence.failure !== undefined && evidence.failure.detail !== errorCode) {
					malformed("model-call completion failure detail must equal its public errorCode");
				}
			}
			return;
		}
		case "tool.admitted":
			requiredId(source.toolCallId, "tool.admitted toolCallId", wireToolCallId);
			text(source.name, "tool.admitted name", 256);
			optionalId(source.messageId, "tool.admitted messageId", wireMessageId);
			optionalId(source.modelCallId, "tool.admitted modelCallId", wireModelCallId);
			optionalId(source.parentToolCallId, "tool.admitted parentToolCallId", wireToolCallId);
			toolSchema(source.schema);
			if (source.argumentDigest !== undefined
				&& !/^[a-f0-9]{64}$/.test(text(source.argumentDigest, "tool.admitted argumentDigest", 64))) {
				malformed("tool.admitted argumentDigest must be 64 lowercase hexadecimal characters");
			}
			optionalText(source.argumentPreview, "tool.admitted argumentPreview", 8_192);
			optionalId(source.argumentArtifactId, "tool.admitted argumentArtifactId", wireArtifactId);
			return;
		case "tool.progress":
			requiredId(source.toolCallId, "tool.progress toolCallId", wireToolCallId);
			optionalId(source.outputArtifactId, "tool.progress outputArtifactId", wireArtifactId);
			return;
		case "tool.terminal": {
			requiredId(source.toolCallId, "tool.terminal toolCallId", wireToolCallId);
			const status = oneOf(
				source.status,
				["succeeded", "failed", "cancelled", "synthetic_failure"] as const,
				"tool.terminal status",
			);
			const origin = oneOf(source.origin, ["provider", "north"] as const, "tool.terminal origin");
			if (status === "synthetic_failure" && origin !== "north") {
				malformed("synthetic tool failures must originate from North");
			}
			optionalText(source.resultPreview, "tool.terminal resultPreview", 8_192);
			const resultArtifactId = optionalId(
				source.resultArtifactId,
				"tool.terminal resultArtifactId",
				wireArtifactId,
			);
			const resultArtifactDigest = source.resultArtifactDigest === undefined
				? undefined
				: sha256Digest(source.resultArtifactDigest, "tool.terminal resultArtifactDigest");
			if ((resultArtifactId === undefined) !== (resultArtifactDigest === undefined)) {
				malformed("tool.terminal resultArtifactId and resultArtifactDigest must appear together");
			}
			optionalPublicErrorCode(source.errorCode, "tool.terminal errorCode");
			return;
		}
		case "artifact.published": {
			requiredId(source.artifactId, "artifact.published artifactId", wireArtifactId);
			optionalId(source.resourceId, "artifact.published resourceId", wireResourceId);
			const mediaType = text(source.mediaType, "artifact.published mediaType", 256);
			if (!/^[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]*\/[A-Za-z0-9][A-Za-z0-9!#$&^_.+-]*$/.test(mediaType)) {
				malformed("artifact.published mediaType is invalid");
			}
			count(source.bytes, "artifact.published bytes");
			if (source.digest !== undefined) sha256Digest(source.digest, "artifact.published digest");
			optionalText(source.label, "artifact.published label", 512);
			return;
		}
		case "resource.pressure":
			optionalId(source.resourceId, "resource.pressure resourceId", wireResourceId);
			text(source.scope, "resource.pressure scope", 256);
			text(source.resource, "resource.pressure resource", 256);
			finiteNumber(source.used, "resource.pressure used");
			finiteNumber(source.reserved, "resource.pressure reserved");
			finiteNumber(source.limit, "resource.pressure limit");
			flag(source.advisory, "resource.pressure advisory");
			return;
		case "run.terminated":
			oneOf(source.lifecycle, ["completed", "failed", "cancelled", "blocked"] as const, "run.terminated lifecycle");
			terminationReason(source.reason);
			if (source.abort !== undefined) abortEvidence(source.abort, "run.terminated abort");
	}
}

function extensions(
	source: JsonObject,
	payloadKeys: readonly string[],
): JsonObject | undefined {
	const explicit = source.extensions === undefined ? {} : record(source.extensions, "wire event extensions");
	const known = new Set<string>([...COMMON_KEYS, ...payloadKeys]);
	const extra = Object.fromEntries(Object.entries(source).filter(([key]) => !known.has(key)));
	const conflicts = Object.keys(extra).filter((key) => Object.hasOwn(explicit, key));
	if (conflicts.length) malformed(`wire event extension fields conflict: ${conflicts.join(", ")}`);
	const merged = { ...explicit, ...extra };
	return Object.keys(merged).length ? jsonObject(merged, "wire event extensions") : undefined;
}

function envelope(source: JsonObject): WireEventEnvelope {
	return {
		version: text(source.version, "wire event version", 128),
		id: requiredId(source.id, "wire event id", wireEventId),
		runId: requiredId(source.runId, "wire event runId", wireRunId),
		...(source.parentId === undefined ? {} : {
			parentId: requiredId(source.parentId, "wire event parentId", wireParentId),
		}),
		sequence: count(source.sequence, "wire event sequence"),
		at: instant(source.at, "wire event at"),
		kind: text(source.kind, "wire event kind", 128),
		essential: flag(source.essential, "wire event essential"),
		requiredSemantics: semanticList(source.requiredSemantics),
		...(source.extensions === undefined ? {} : {
			extensions: record(source.extensions, "wire event extensions"),
		}),
	};
}

function knownEvent(
	base: WireEventEnvelope,
	kind: WireEventKind,
	source: JsonObject,
	preserved: JsonObject | undefined,
): WireKnownEvent {
	const common = {
		version: WIRE_VERSION,
		id: base.id,
		runId: base.runId,
		...(base.parentId === undefined ? {} : { parentId: base.parentId }),
		sequence: base.sequence,
		at: base.at,
		essential: true as const,
		requiredSemantics: WIRE_REQUIRED_SEMANTICS,
		...(preserved === undefined ? {} : { extensions: preserved }),
	};
	switch (kind) {
		case "run.started":
			return Object.freeze({
				...common,
				kind,
				lifecycle: oneOf(source.lifecycle, ["running"] as const, "run.started lifecycle"),
				...(source.parentRunId === undefined ? {} : {
					parentRunId: requiredId(source.parentRunId, "run.started parentRunId", wireRunId),
				}),
				...(source.owner === undefined ? {} : { owner: text(source.owner, "run.started owner", 256) }),
			} satisfies WireKnownEvent);
		case "run.progress":
			return Object.freeze({
				...common,
				kind,
				lifecycle: oneOf(source.lifecycle, ["running", "waiting"] as const, "run.progress lifecycle"),
				progress: progressPatch(source.progress),
			} satisfies WireKnownEvent);
		case "message.recorded":
			return Object.freeze({
				...common,
				kind,
				messageId: requiredId(source.messageId, "message.recorded messageId", wireMessageId),
				stage: oneOf(source.stage, ["started", "delta", "completed"] as const, "message.recorded stage"),
				role: oneOf(source.role, ["user", "assistant", "tool", "system"] as const, "message.recorded role"),
				...(source.content === undefined ? {} : { content: source.content }),
				...(source.modelCallId === undefined ? {} : {
					modelCallId: requiredId(source.modelCallId, "message.recorded modelCallId", wireModelCallId),
				}),
				...(source.parentToolCallId === undefined ? {} : {
					parentToolCallId: requiredId(
						source.parentToolCallId,
						"message.recorded parentToolCallId",
						wireToolCallId,
					),
				}),
			} satisfies WireKnownEvent);
		case "model-call.started":
			return Object.freeze({
				...common,
				kind,
				modelCallId: requiredId(source.modelCallId, "model-call.started modelCallId", wireModelCallId),
				model: modelSelection(source.model, "model-call.started model"),
				...(source.effort === undefined ? {} : {
					effort: oneOf(source.effort, WIRE_EFFORTS, "model-call.started effort"),
				}),
				attempt: positiveCount(source.attempt, "model-call.started attempt"),
			} satisfies WireKnownEvent);
		case "model-call.completed":
			{
				const origin = oneOf(
					source.origin,
					["provider", "north"] as const,
					"model-call.completed origin",
				);
				const usageCoverage = oneOf(
					source.usageCoverage,
					["exact", "partial", "unavailable"] as const,
					"model-call.completed usageCoverage",
				);
				if (usageCoverage === "exact" && origin !== "provider") {
					malformed("model-call.completed exact usage requires provider origin");
				}
				const errorCode = optionalPublicErrorCode(
					source.errorCode,
					"model-call.completed errorCode",
				);
				return Object.freeze({
				...common,
				kind,
				modelCallId: requiredId(source.modelCallId, "model-call.completed modelCallId", wireModelCallId),
				status: oneOf(
					source.status,
					["succeeded", "failed", "cancelled"] as const,
					"model-call.completed status",
				),
				origin,
				usage: usageSnapshot(source.usage, "model-call.completed usage"),
				usageCoverage,
				...(errorCode === undefined ? {} : { errorCode }),
				...(source.evidence === undefined ? {} : {
					evidence: completionEvidence(source.evidence),
				}),
				} satisfies WireKnownEvent);
			}
		case "tool.admitted":
			return Object.freeze({
				...common,
				kind,
				toolCallId: requiredId(source.toolCallId, "tool.admitted toolCallId", wireToolCallId),
				name: text(source.name, "tool.admitted name", 256),
				...(source.messageId === undefined ? {} : {
					messageId: requiredId(source.messageId, "tool.admitted messageId", wireMessageId),
				}),
				...(source.modelCallId === undefined ? {} : {
					modelCallId: requiredId(source.modelCallId, "tool.admitted modelCallId", wireModelCallId),
				}),
				...(source.parentToolCallId === undefined ? {} : {
					parentToolCallId: requiredId(
						source.parentToolCallId,
						"tool.admitted parentToolCallId",
						wireToolCallId,
					),
				}),
				schema: toolSchema(source.schema),
				...(source.argumentDigest === undefined ? {} : {
					argumentDigest: text(source.argumentDigest, "tool.admitted argumentDigest", 64),
				}),
				...(source.argumentPreview === undefined ? {} : {
					argumentPreview: text(source.argumentPreview, "tool.admitted argumentPreview", 8_192),
				}),
				...(source.argumentArtifactId === undefined ? {} : {
					argumentArtifactId: requiredId(
						source.argumentArtifactId,
						"tool.admitted argumentArtifactId",
						wireArtifactId,
					),
				}),
			} satisfies WireKnownEvent);
		case "tool.progress":
			return Object.freeze({
				...common,
				kind,
				toolCallId: requiredId(source.toolCallId, "tool.progress toolCallId", wireToolCallId),
				...(source.progress === undefined ? {} : { progress: source.progress }),
				...(source.outputArtifactId === undefined ? {} : {
					outputArtifactId: requiredId(
						source.outputArtifactId,
						"tool.progress outputArtifactId",
						wireArtifactId,
					),
				}),
			} satisfies WireKnownEvent);
		case "tool.terminal":
			{
				const errorCode = optionalPublicErrorCode(
					source.errorCode,
					"tool.terminal errorCode",
				);
				return Object.freeze({
				...common,
				kind,
				toolCallId: requiredId(source.toolCallId, "tool.terminal toolCallId", wireToolCallId),
				status: oneOf(
					source.status,
					["succeeded", "failed", "cancelled", "synthetic_failure"] as const,
					"tool.terminal status",
				),
				origin: oneOf(source.origin, ["provider", "north"] as const, "tool.terminal origin"),
				...(source.resultPreview === undefined ? {} : {
					resultPreview: text(source.resultPreview, "tool.terminal resultPreview", 8_192),
				}),
				...(source.resultArtifactId === undefined ? {} : {
					resultArtifactId: requiredId(
						source.resultArtifactId,
						"tool.terminal resultArtifactId",
						wireArtifactId,
					),
				}),
				...(source.resultArtifactDigest === undefined ? {} : {
					resultArtifactDigest: sha256Digest(
						source.resultArtifactDigest,
						"tool.terminal resultArtifactDigest",
					),
				}),
				...(errorCode === undefined ? {} : { errorCode }),
				} satisfies WireKnownEvent);
			}
		case "artifact.published":
			return Object.freeze({
				...common,
				kind,
				artifactId: requiredId(source.artifactId, "artifact.published artifactId", wireArtifactId),
				...(source.resourceId === undefined ? {} : {
					resourceId: requiredId(source.resourceId, "artifact.published resourceId", wireResourceId),
				}),
				mediaType: text(source.mediaType, "artifact.published mediaType", 256),
				bytes: count(source.bytes, "artifact.published bytes"),
				...(source.digest === undefined ? {} : {
					digest: sha256Digest(source.digest, "artifact.published digest"),
				}),
				...(source.label === undefined ? {} : {
					label: text(source.label, "artifact.published label", 512),
				}),
			} satisfies WireKnownEvent);
		case "resource.pressure":
			return Object.freeze({
				...common,
				kind,
				...(source.resourceId === undefined ? {} : {
					resourceId: requiredId(source.resourceId, "resource.pressure resourceId", wireResourceId),
				}),
				scope: text(source.scope, "resource.pressure scope", 256),
				resource: text(source.resource, "resource.pressure resource", 256),
				used: finiteNumber(source.used, "resource.pressure used"),
				reserved: finiteNumber(source.reserved, "resource.pressure reserved"),
				limit: finiteNumber(source.limit, "resource.pressure limit"),
				advisory: flag(source.advisory, "resource.pressure advisory"),
			} satisfies WireKnownEvent);
		case "run.terminated":
			return Object.freeze({
				...common,
				kind,
				lifecycle: oneOf(
					source.lifecycle,
					["completed", "failed", "cancelled", "blocked"] as const,
					"run.terminated lifecycle",
				),
				reason: terminationReason(source.reason),
				...(source.abort === undefined ? {} : {
					abort: abortEvidence(source.abort, "run.terminated abort"),
				}),
			} satisfies WireKnownEvent);
	}
}

export function decodeWireEvent(value: unknown): WireEvent {
	let source: JsonObject;
	try {
		source = jsonObject(value, "wire event");
	} catch (error) {
		if (error instanceof WireDecodeError) throw error;
		throw new WireDecodeError(
			"malformed_event",
			error instanceof Error ? error.message : "wire event normalization failed",
			{},
			{ cause: error },
		);
	}
	const base = envelope(source);
	assertSupportedSemantics(base.requiredSemantics);
	if (base.version !== WIRE_VERSION) {
		if (base.essential) {
			throw new WireDecodeError(
				"unsupported_version",
				`essential wire event version is unsupported: ${base.version}`,
				{ eventId: base.id, runId: base.runId, sequence: base.sequence },
			);
		}
		return source as unknown as WireOpaqueEvent;
	}
	if (!EVENT_KIND_SET.has(base.kind)) {
		if (base.essential) {
			throw new WireDecodeError(
				"unsupported_event_kind",
				`essential wire event kind is unsupported: ${base.kind}`,
				{ eventId: base.id, runId: base.runId, sequence: base.sequence },
			);
		}
		return source as unknown as WireOpaqueEvent;
	}
	if (!base.essential) malformed("known wire-v2 events must be essential");
	const missingSemantics = WIRE_REQUIRED_SEMANTICS
		.filter((semantic) => !base.requiredSemantics.includes(semantic));
	if (missingSemantics.length) {
		malformed(`known wire-v2 event is missing required semantics: ${missingSemantics.join(", ")}`);
	}
	const kind = base.kind as WireEventKind;
	validatePayload(kind, source);
	const payloadKeys = PAYLOAD_KEYS[kind];
	const preserved = extensions(source, payloadKeys);
	return knownEvent(base, kind, source, preserved);
}

export function decodeWireEvents(values: readonly unknown[]): readonly WireEvent[] {
	return Object.freeze(values.map(decodeWireEvent));
}
