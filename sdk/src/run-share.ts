import { readBridgeWireJournal } from "./bridge/journal";
import {
	bridgeJournalRoot,
	parseBridgeLaunchExecutionId,
} from "./bridge/protocol";
import { RUN_ARTIFACT_MAX_COUNT } from "./run-artifacts";
import {
	RUN_SHARE_AES_GCM_AAD,
	RUN_SHARE_AUTH_TAG_BYTES,
	RUN_SHARE_BUNDLE_PROTOCOL,
	RUN_SHARE_BUNDLE_VERSION,
	RUN_SHARE_KEY_BYTES,
	RUN_SHARE_MAX_EVENTS,
	RUN_SHARE_MAX_PLAINTEXT_BYTES,
	RUN_SHARE_MAX_SEALED_BYTES,
	RUN_SHARE_NONCE_BYTES,
	RUN_SHARE_REDACTION_POLICY,
	RUN_SHARE_REPLAY_MODE,
	RUN_SHARE_SEALED_HEADER,
} from "./run-share-contract";
import {
	WIRE_EVENT_KINDS,
	WIRE_MAX_EVENTS_PER_RUN,
	WIRE_MAX_STREAM_BYTES,
	WIRE_REQUIRED_SEMANTICS,
	WIRE_VERSION,
	decodeWireEvents,
	expectedWireParentId,
	reduceWireEvents,
	wireArtifactId,
	wireEventId,
	wireMessageId,
	wireModelCallId,
	wireResourceId,
	wireRunId,
	wireToolCallId,
	type WireAbortEvidence,
	type WireArtifactId,
	type WireCompletionEvidence,
	type WireEvent,
	type WireKnownEvent,
	type WireProgressPatch,
	type WireRunId,
} from "./wire";

const TEXT_ENCODER = new TextEncoder();
const FATAL_DECODER = new TextDecoder("utf-8", { fatal: true });
const REDACTED_TEXT = "[redacted]";
const REDACTED_MESSAGE = "[content redacted]";
const REDACTED_ERROR_CODE = "redacted_error";
const REDACTED_ARTIFACT_CONTENT = '{"redacted":true}';
const REDACTED_ARTIFACT_MEDIA_TYPE = "application/vnd.north.redacted+json";
const REDACTED_ARTIFACT_BYTES = TEXT_ENCODER.encode(REDACTED_ARTIFACT_CONTENT).byteLength;
const REDACTED_ARTIFACT_DIGEST = new Bun.CryptoHasher("sha256")
	.update(REDACTED_ARTIFACT_CONTENT)
	.digest("hex");
const EVENT_KIND_SET = new Set<string>(WIRE_EVENT_KINDS);
const KEY_FRAGMENT_PATTERN = /^[A-Za-z0-9_-]{43}$/;

export type RunShareErrorCode =
	| "invalid_execution_id"
	| "source_run_mismatch"
	| "source_limit_exceeded"
	| "output_limit_exceeded"
	| "invalid_bundle"
	| "missing_key"
	| "invalid_key"
	| "authentication_failed";

export class RunShareError extends Error {
	readonly code: RunShareErrorCode;
	readonly limit?: number;
	readonly observed?: number;

	constructor(
		code: RunShareErrorCode,
		message: string,
		context: { limit?: number; observed?: number } = {},
		options?: ErrorOptions,
	) {
		super(message, options);
		this.name = "RunShareError";
		this.code = code;
		this.limit = context.limit;
		this.observed = context.observed;
	}
}

export interface RunShareArtifact {
	readonly artifactId: WireArtifactId;
	readonly mediaType: typeof REDACTED_ARTIFACT_MEDIA_TYPE;
	readonly bytes: typeof REDACTED_ARTIFACT_BYTES;
	readonly digest: typeof REDACTED_ARTIFACT_DIGEST;
	readonly content: typeof REDACTED_ARTIFACT_CONTENT;
}

export interface RunShareBundle {
	readonly protocol: typeof RUN_SHARE_BUNDLE_PROTOCOL;
	readonly version: typeof RUN_SHARE_BUNDLE_VERSION;
	readonly redactionPolicy: typeof RUN_SHARE_REDACTION_POLICY;
	readonly replay: typeof RUN_SHARE_REPLAY_MODE;
	readonly events: readonly WireEvent[];
	readonly artifacts: readonly RunShareArtifact[];
}

export interface RunShareSeal {
	readonly sealed: Uint8Array<ArrayBuffer>;
	readonly fragment: string;
}

export interface BridgeRunShareOptions {
	readonly journalRoot?: string;
	readonly maxSourceBytes?: number;
	readonly maxEvents?: number;
}

type AliasKind = "run" | "message" | "modelCall" | "toolCall" | "artifact" | "resource";

const ALIAS_PREFIX: Readonly<Record<AliasKind, string>> = Object.freeze({
	run: "run:share:",
	message: "message:share:",
	modelCall: "model-call:share:",
	toolCall: "tool-call:share:",
	artifact: "artifact:share:",
	resource: "resource:share:",
});

class ShareAliases {
	#values: Record<AliasKind, Map<string, string>> = {
		run: new Map(),
		message: new Map(),
		modelCall: new Map(),
		toolCall: new Map(),
		artifact: new Map(),
		resource: new Map(),
	};

	#alias(kind: AliasKind, value: string): string {
		const values = this.#values[kind];
		const existing = values.get(value);
		if (existing !== undefined) return existing;
		const alias = `${ALIAS_PREFIX[kind]}${values.size + 1}`;
		values.set(value, alias);
		return alias;
	}

	run(value: string): WireRunId { return wireRunId(this.#alias("run", value)); }
	message(value: string) { return wireMessageId(this.#alias("message", value)); }
	modelCall(value: string) { return wireModelCallId(this.#alias("modelCall", value)); }
	toolCall(value: string) { return wireToolCallId(this.#alias("toolCall", value)); }
	artifact(value: string) { return wireArtifactId(this.#alias("artifact", value)); }
	resource(value: string) { return wireResourceId(this.#alias("resource", value)); }
}

function positiveBound(value: number | undefined, fallback: number, ceiling: number, label: string): number {
	const resolved = value ?? fallback;
	if (!Number.isSafeInteger(resolved) || resolved < 1 || resolved > ceiling) {
		throw new RunShareError(
			"source_limit_exceeded",
			`${label} must be a positive integer no greater than ${ceiling}`,
			{ limit: ceiling, observed: resolved },
		);
	}
	return resolved;
}

function isKnownEvent(event: WireEvent): event is WireKnownEvent {
	return event.version === WIRE_VERSION && event.essential === true && EVENT_KIND_SET.has(event.kind);
}

function normalizedTime(sequence: number): string {
	return new Date(Date.UTC(2000, 0, 1) + sequence).toISOString();
}

function claimAliases(events: readonly WireEvent[]): ShareAliases {
	const aliases = new ShareAliases();
	for (const event of events) {
		aliases.run(event.runId);
		if (!isKnownEvent(event)) continue;
		switch (event.kind) {
			case "run.started":
				if (event.parentRunId !== undefined) aliases.run(event.parentRunId);
				break;
			case "run.progress":
				for (const nested of event.progress.nested ?? []) aliases.run(nested.runId);
				for (const reference of event.progress.outputReferences ?? []) {
					if (reference.kind === "artifact") aliases.artifact(reference.artifactId);
					else aliases.resource(reference.resourceId);
				}
				if (event.progress.patch !== undefined && event.progress.patch !== null)
					aliases.artifact(event.progress.patch.artifactId);
				break;
			case "message.recorded":
				aliases.message(event.messageId);
				if (event.modelCallId !== undefined) aliases.modelCall(event.modelCallId);
				if (event.parentToolCallId !== undefined) aliases.toolCall(event.parentToolCallId);
				break;
			case "model-call.started":
			case "model-call.completed":
				aliases.modelCall(event.modelCallId);
				break;
			case "tool.admitted":
				aliases.toolCall(event.toolCallId);
				if (event.messageId !== undefined) aliases.message(event.messageId);
				if (event.modelCallId !== undefined) aliases.modelCall(event.modelCallId);
				if (event.parentToolCallId !== undefined) aliases.toolCall(event.parentToolCallId);
				if (event.argumentArtifactId !== undefined) aliases.artifact(event.argumentArtifactId);
				break;
			case "tool.progress":
				aliases.toolCall(event.toolCallId);
				if (event.outputArtifactId !== undefined) aliases.artifact(event.outputArtifactId);
				break;
			case "tool.terminal":
				aliases.toolCall(event.toolCallId);
				if (event.resultArtifactId !== undefined) aliases.artifact(event.resultArtifactId);
				break;
			case "artifact.published":
				aliases.artifact(event.artifactId);
				if (event.resourceId !== undefined) aliases.resource(event.resourceId);
				break;
			case "resource.pressure":
				if (event.resourceId !== undefined) aliases.resource(event.resourceId);
				break;
			case "run.terminated":
				break;
		}
	}
	return aliases;
}

function redactAbort(evidence: WireAbortEvidence, sequence: number): WireAbortEvidence {
	return {
		requestedAt: normalizedTime(sequence),
		source: evidence.source,
		reason: REDACTED_TEXT,
		...(evidence.watchdog === undefined ? {} : {
			watchdog: {
				silenceMs: evidence.watchdog.silenceMs,
				...(evidence.watchdog.lastOuter === undefined ? {} : {
					lastOuter: {
						origin: "outer" as const,
						kind: evidence.watchdog.lastOuter.kind,
						observedAt: normalizedTime(sequence),
					},
				}),
				...(evidence.watchdog.lastProvider === undefined ? {} : {
					lastProvider: {
						origin: "provider" as const,
						kind: evidence.watchdog.lastProvider.kind,
						observedAt: normalizedTime(sequence),
					},
				}),
			},
		}),
	};
}

function redactProgress(
	progress: WireProgressPatch,
	aliases: ShareAliases,
	sequence: number,
): WireProgressPatch {
	return {
		...(progress.currentAction === undefined ? {} : {
			currentAction: progress.currentAction === null ? null : REDACTED_TEXT,
		}),
		...(progress.compactions === undefined ? {} : { compactions: progress.compactions }),
		...(progress.outputReferences === undefined ? {} : {
			outputReferences: progress.outputReferences === null ? null : progress.outputReferences.map((reference) =>
				reference.kind === "artifact"
					? { kind: "artifact" as const, artifactId: aliases.artifact(reference.artifactId) }
					: { kind: "resource" as const, resourceId: aliases.resource(reference.resourceId) }
			),
		}),
		...(progress.model === undefined ? {} : { model: progress.model }),
		...(progress.effort === undefined ? {} : { effort: progress.effort }),
		...(progress.retry === undefined ? {} : {
			retry: progress.retry === null ? null : { ...progress.retry, reason: REDACTED_TEXT },
		}),
		...(progress.fallback === undefined ? {} : {
			fallback: progress.fallback === null ? null : { ...progress.fallback, reason: REDACTED_TEXT },
		}),
		...(progress.nested === undefined ? {} : {
			nested: progress.nested === null ? null : progress.nested.map((nested) => ({
				runId: aliases.run(nested.runId),
				lifecycle: nested.lifecycle,
				...(nested.currentAction === undefined ? {} : { currentAction: REDACTED_TEXT }),
			})),
		}),
		...(progress.patch === undefined ? {} : {
			patch: progress.patch === null ? null : {
				artifactId: aliases.artifact(progress.patch.artifactId),
				filesChanged: progress.patch.filesChanged,
			},
		}),
		...(progress.branch === undefined ? {} : {
			branch: progress.branch === null ? null : {
				name: REDACTED_TEXT,
				...(progress.branch.base === undefined ? {} : { base: REDACTED_TEXT }),
			},
		}),
		...(progress.abort === undefined ? {} : {
			abort: progress.abort === null ? null : redactAbort(progress.abort, sequence),
		}),
		...(progress.usage === undefined ? {} : { usage: progress.usage }),
	};
}

function redactCompletionEvidence(evidence: WireCompletionEvidence): WireCompletionEvidence {
	return {
		...(evidence.providerJoin === undefined ? {} : {
			providerJoin: {
				version: evidence.providerJoin.version,
				turnKeys: [],
				sessionPersistence: "unknown" as const,
				coverage: "unknown" as const,
			},
		}),
		...(evidence.turns === undefined ? {} : { turns: evidence.turns }),
		...(evidence.providerDurationMs === undefined ? {} : {
			providerDurationMs: evidence.providerDurationMs,
		}),
		...(evidence.failure === undefined ? {} : {
			failure: {
				detail: REDACTED_ERROR_CODE,
				...(evidence.failure.landed === undefined ? {} : { landed: evidence.failure.landed }),
			},
		}),
		...(evidence.interrupt === undefined ? {} : {
			interrupt: {
				...evidence.interrupt,
				...(evidence.interrupt.openItem === undefined ? {} : {
					openItem: { ...evidence.interrupt.openItem, kind: "activity" },
				}),
			},
		}),
	};
}

function knownEvent(
	source: WireKnownEvent,
	aliases: ShareAliases,
	payload: Readonly<Record<string, unknown>>,
): WireKnownEvent {
	const runId = aliases.run(source.runId);
	const base = {
		version: WIRE_VERSION,
		id: wireEventId(`event:share:${source.sequence}`),
		runId,
		sequence: source.sequence,
		at: normalizedTime(source.sequence),
		kind: source.kind,
		essential: true,
		requiredSemantics: WIRE_REQUIRED_SEMANTICS,
		...payload,
	};
	const parentId = expectedWireParentId(base as unknown as WireKnownEvent, runId);
	return decodeWireEvents([{
		...base,
		...(parentId === undefined ? {} : { parentId }),
	}])[0] as WireKnownEvent;
}

function redactKnownEvent(
	event: WireKnownEvent,
	aliases: ShareAliases,
	toolNames: Map<string, string>,
): WireKnownEvent {
	switch (event.kind) {
		case "run.started":
			return knownEvent(event, aliases, {
				lifecycle: "running",
				...(event.parentRunId === undefined ? {} : { parentRunId: aliases.run(event.parentRunId) }),
			});
		case "run.progress":
			return knownEvent(event, aliases, {
				lifecycle: event.lifecycle,
				progress: redactProgress(event.progress, aliases, event.sequence),
			});
		case "message.recorded":
			return knownEvent(event, aliases, {
				messageId: aliases.message(event.messageId),
				stage: event.stage,
				role: event.role,
				...(event.content === undefined ? {} : {
					content: typeof event.content === "string"
						? event.content.trim() ? REDACTED_MESSAGE : ""
						: null,
				}),
				...(event.modelCallId === undefined ? {} : { modelCallId: aliases.modelCall(event.modelCallId) }),
				...(event.parentToolCallId === undefined ? {} : {
					parentToolCallId: aliases.toolCall(event.parentToolCallId),
				}),
			});
		case "model-call.started":
			return knownEvent(event, aliases, {
				modelCallId: aliases.modelCall(event.modelCallId),
				model: event.model,
				...(event.effort === undefined ? {} : { effort: event.effort }),
				attempt: event.attempt,
			});
		case "model-call.completed":
			return knownEvent(event, aliases, {
				modelCallId: aliases.modelCall(event.modelCallId),
				status: event.status,
				origin: event.origin,
				usage: event.usage,
				usageCoverage: event.usageCoverage,
				...(event.errorCode === undefined ? {} : { errorCode: REDACTED_ERROR_CODE }),
				...(event.evidence === undefined ? {} : {
					evidence: redactCompletionEvidence(event.evidence),
				}),
			});
		case "tool.admitted": {
			let name = toolNames.get(event.name);
			if (name === undefined) {
				name = `shared-tool-${toolNames.size + 1}`;
				toolNames.set(event.name, name);
			}
			return knownEvent(event, aliases, {
				toolCallId: aliases.toolCall(event.toolCallId),
				name,
				...(event.messageId === undefined ? {} : { messageId: aliases.message(event.messageId) }),
				...(event.modelCallId === undefined ? {} : { modelCallId: aliases.modelCall(event.modelCallId) }),
				...(event.parentToolCallId === undefined ? {} : {
					parentToolCallId: aliases.toolCall(event.parentToolCallId),
				}),
				schema: { status: "unavailable", reason: REDACTED_TEXT },
				...(event.argumentArtifactId === undefined ? {} : {
					argumentArtifactId: aliases.artifact(event.argumentArtifactId),
				}),
			});
		}
		case "tool.progress":
			return knownEvent(event, aliases, {
				toolCallId: aliases.toolCall(event.toolCallId),
				...(event.outputArtifactId === undefined ? {} : {
					outputArtifactId: aliases.artifact(event.outputArtifactId),
				}),
			});
		case "tool.terminal":
			return knownEvent(event, aliases, {
				toolCallId: aliases.toolCall(event.toolCallId),
				status: event.status,
				origin: event.origin,
				...(event.resultArtifactId === undefined ? {} : {
					resultArtifactId: aliases.artifact(event.resultArtifactId),
					resultArtifactDigest: REDACTED_ARTIFACT_DIGEST,
				}),
				...(event.errorCode === undefined ? {} : { errorCode: REDACTED_ERROR_CODE }),
			});
		case "artifact.published":
			return knownEvent(event, aliases, {
				artifactId: aliases.artifact(event.artifactId),
				...(event.resourceId === undefined ? {} : { resourceId: aliases.resource(event.resourceId) }),
				mediaType: REDACTED_ARTIFACT_MEDIA_TYPE,
				bytes: REDACTED_ARTIFACT_BYTES,
				digest: REDACTED_ARTIFACT_DIGEST,
			});
		case "resource.pressure":
			return knownEvent(event, aliases, {
				...(event.resourceId === undefined ? {} : { resourceId: aliases.resource(event.resourceId) }),
				scope: "redacted-scope",
				resource: "redacted-resource",
				used: event.used,
				reserved: event.reserved,
				limit: event.limit,
				advisory: event.advisory,
			});
		case "run.terminated":
			return knownEvent(event, aliases, {
				lifecycle: event.lifecycle,
				reason: { code: event.reason.code },
				...(event.abort === undefined ? {} : { abort: redactAbort(event.abort, event.sequence) }),
			});
	}
	const exhaustive: never = event;
	return exhaustive;
}

function redactOpaqueEvent(event: WireEvent, aliases: ShareAliases): WireEvent {
	return decodeWireEvents([{
		version: "north:run-share-redacted-opaque:v1",
		id: wireEventId(`event:share:${event.sequence}`),
		runId: aliases.run(event.runId),
		sequence: event.sequence,
		at: normalizedTime(event.sequence),
		kind: "share.redacted-opaque",
		essential: false,
		requiredSemantics: [],
	}])[0]!;
}

function structuralInvariant(source: readonly WireEvent[], redacted: readonly WireEvent[]): void {
	if (source.length !== redacted.length) {
		throw new RunShareError("invalid_bundle", "run share redaction changed the event count");
	}
	for (let index = 0; index < source.length; index += 1) {
		const before = source[index]!;
		const after = redacted[index]!;
		const expectedKind = isKnownEvent(before) ? before.kind : "share.redacted-opaque";
		if (after.sequence !== before.sequence || after.kind !== expectedKind) {
			throw new RunShareError("invalid_bundle", "run share redaction changed event order or kind");
		}
		if (isKnownEvent(before) && isKnownEvent(after)) {
			if (before.kind === "model-call.completed" && after.kind === before.kind
				&& (before.status !== after.status || before.origin !== after.origin)) {
				throw new RunShareError("invalid_bundle", "run share redaction changed model terminal state");
			}
			if (before.kind === "tool.terminal" && after.kind === before.kind
				&& (before.status !== after.status || before.origin !== after.origin)) {
				throw new RunShareError("invalid_bundle", "run share redaction changed tool terminal state");
			}
		}
	}
	const before = reduceWireEvents(source);
	const after = reduceWireEvents(redacted);
	if (before.lifecycle !== after.lifecycle
		|| Object.keys(before.messages).length !== Object.keys(after.messages).length
		|| Object.keys(before.modelCalls).length !== Object.keys(after.modelCalls).length
		|| Object.keys(before.toolCalls).length !== Object.keys(after.toolCalls).length
		|| Object.keys(before.artifacts).length !== Object.keys(after.artifacts).length) {
		throw new RunShareError("invalid_bundle", "run share redaction changed replay structure");
	}
}

export function redactWireRun(
	events: readonly WireEvent[],
	maxEvents = RUN_SHARE_MAX_EVENTS,
): RunShareBundle {
	if (!Number.isSafeInteger(maxEvents) || maxEvents < 1 || maxEvents > RUN_SHARE_MAX_EVENTS) {
		throw new RunShareError("source_limit_exceeded", "run share event limit is invalid");
	}
	if (events.length === 0 || events.length > maxEvents) {
		throw new RunShareError(
			"source_limit_exceeded",
			`run share source has ${events.length} events; limit is ${maxEvents}`,
			{ limit: maxEvents, observed: events.length },
		);
	}
	const source = decodeWireEvents(events);
	reduceWireEvents(source);
	const aliases = claimAliases(source);
	const toolNames = new Map<string, string>();
	const redacted = Object.freeze(source.map((event) => isKnownEvent(event)
		? redactKnownEvent(event, aliases, toolNames)
		: redactOpaqueEvent(event, aliases)));
	structuralInvariant(source, redacted);
	const artifacts = Object.freeze(redacted.flatMap((event): RunShareArtifact[] => {
		if (!isKnownEvent(event) || event.kind !== "artifact.published") return [];
		return [Object.freeze({
			artifactId: event.artifactId,
			mediaType: REDACTED_ARTIFACT_MEDIA_TYPE,
			bytes: REDACTED_ARTIFACT_BYTES,
			digest: REDACTED_ARTIFACT_DIGEST,
			content: REDACTED_ARTIFACT_CONTENT,
		})];
	}));
	return Object.freeze({
		protocol: RUN_SHARE_BUNDLE_PROTOCOL,
		version: RUN_SHARE_BUNDLE_VERSION,
		redactionPolicy: RUN_SHARE_REDACTION_POLICY,
		replay: RUN_SHARE_REPLAY_MODE,
		events: redacted,
		artifacts,
	});
}

export async function buildBridgeRunShareBundle(
	executionId: string,
	options: BridgeRunShareOptions = {},
): Promise<RunShareBundle> {
	let validatedExecutionId: string;
	try { validatedExecutionId = parseBridgeLaunchExecutionId(executionId); }
	catch (cause) {
		throw new RunShareError("invalid_execution_id", "run share requires a Bridge UUIDv4", {}, { cause });
	}
	const maxEvents = positiveBound(
		options.maxEvents,
		RUN_SHARE_MAX_EVENTS,
		Math.min(RUN_SHARE_MAX_EVENTS, WIRE_MAX_EVENTS_PER_RUN),
		"maxEvents",
	);
	const maxSourceBytes = positiveBound(
		options.maxSourceBytes,
		WIRE_MAX_STREAM_BYTES,
		WIRE_MAX_STREAM_BYTES,
		"maxSourceBytes",
	);
	const replay = await readBridgeWireJournal(
		options.journalRoot ?? bridgeJournalRoot(),
		validatedExecutionId,
		{ maxEvents, maxStreamBytes: maxSourceBytes },
	);
	const expectedRunId = wireRunId(`bridge:${validatedExecutionId}`);
	if (replay.snapshot?.runId !== expectedRunId) {
		throw new RunShareError("source_run_mismatch", "Bridge journal does not belong to the requested run");
	}
	return redactWireRun(replay.events, maxEvents);
}

function exactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
	const keys = Object.keys(value);
	return keys.length === expected.length && keys.every((key) => expected.includes(key));
}

export function validateRunShareBundle(value: unknown): RunShareBundle {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		throw new RunShareError("invalid_bundle", "run share bundle must be an object");
	}
	const source = value as Record<string, unknown>;
	if (!exactKeys(source, ["protocol", "version", "redactionPolicy", "replay", "events", "artifacts"])
		|| source.protocol !== RUN_SHARE_BUNDLE_PROTOCOL || source.version !== RUN_SHARE_BUNDLE_VERSION
		|| source.redactionPolicy !== RUN_SHARE_REDACTION_POLICY || source.replay !== RUN_SHARE_REPLAY_MODE
		|| !Array.isArray(source.events) || source.events.length === 0
		|| source.events.length > RUN_SHARE_MAX_EVENTS || !Array.isArray(source.artifacts)
		|| source.artifacts.length > RUN_ARTIFACT_MAX_COUNT) {
		throw new RunShareError("invalid_bundle", "run share bundle has an invalid v1 shape");
	}
	let events: readonly WireEvent[];
	try {
		events = decodeWireEvents(source.events);
		reduceWireEvents(events);
	} catch (cause) {
		throw new RunShareError("invalid_bundle", "run share Wire replay is invalid", {}, { cause });
	}
	const artifacts: RunShareArtifact[] = [];
	for (const value of source.artifacts) {
		if (!value || typeof value !== "object" || Array.isArray(value)) {
			throw new RunShareError("invalid_bundle", "run share artifact must be an object");
		}
		const artifact = value as Record<string, unknown>;
		if (!exactKeys(artifact, ["artifactId", "mediaType", "bytes", "digest", "content"])
			|| typeof artifact.artifactId !== "string"
			|| artifact.mediaType !== REDACTED_ARTIFACT_MEDIA_TYPE
			|| artifact.bytes !== REDACTED_ARTIFACT_BYTES
			|| artifact.digest !== REDACTED_ARTIFACT_DIGEST
			|| artifact.content !== REDACTED_ARTIFACT_CONTENT) {
			throw new RunShareError("invalid_bundle", "run share artifact is not structurally redacted");
		}
		artifacts.push(Object.freeze({
			artifactId: wireArtifactId(artifact.artifactId),
			mediaType: REDACTED_ARTIFACT_MEDIA_TYPE,
			bytes: REDACTED_ARTIFACT_BYTES,
			digest: REDACTED_ARTIFACT_DIGEST,
			content: REDACTED_ARTIFACT_CONTENT,
		}));
	}
	const publishedIds = events.flatMap((event) => isKnownEvent(event) && event.kind === "artifact.published"
		? [event.artifactId] : []);
	if (publishedIds.length !== artifacts.length
		|| publishedIds.some((artifactId, index) => artifactId !== artifacts[index]?.artifactId)) {
		throw new RunShareError("invalid_bundle", "run share artifact manifest does not match Wire replay");
	}
	const validated = Object.freeze({
		protocol: RUN_SHARE_BUNDLE_PROTOCOL,
		version: RUN_SHARE_BUNDLE_VERSION,
		redactionPolicy: RUN_SHARE_REDACTION_POLICY,
		replay: RUN_SHARE_REPLAY_MODE,
		events: Object.freeze([...events]),
		artifacts: Object.freeze(artifacts),
	});
	let canonical: RunShareBundle;
	try { canonical = redactWireRun(validated.events); }
	catch (cause) {
		if (cause instanceof RunShareError) throw cause;
		throw new RunShareError("invalid_bundle", "run share redaction policy is invalid", {}, { cause });
	}
	if (JSON.stringify(canonical.events) !== JSON.stringify(validated.events)
		|| JSON.stringify(canonical.artifacts) !== JSON.stringify(validated.artifacts)) {
		throw new RunShareError("invalid_bundle", "run share bundle is not canonically redacted");
	}
	return validated;
}

export function encodeRunShareBundle(bundle: RunShareBundle): Uint8Array<ArrayBuffer> {
	const validated = validateRunShareBundle(bundle);
	const bytes = TEXT_ENCODER.encode(JSON.stringify(validated));
	if (bytes.byteLength > RUN_SHARE_MAX_PLAINTEXT_BYTES) {
		throw new RunShareError("output_limit_exceeded", "run share plaintext exceeds the byte limit", {
			limit: RUN_SHARE_MAX_PLAINTEXT_BYTES,
			observed: bytes.byteLength,
		});
	}
	return bytes;
}

export function encodeRunShareKey(key: Uint8Array): string {
	if (key.byteLength !== RUN_SHARE_KEY_BYTES) {
		throw new RunShareError("invalid_key", `run share key must be ${RUN_SHARE_KEY_BYTES} bytes`);
	}
	return Buffer.from(key).toString("base64url");
}

export function decodeRunShareFragment(fragment: string): Uint8Array<ArrayBuffer> {
	if (!fragment || fragment === "#") {
		throw new RunShareError("missing_key", "run share link is missing its fragment key");
	}
	const encoded = fragment.startsWith("#") ? fragment.slice(1) : fragment;
	if (!KEY_FRAGMENT_PATTERN.test(encoded)) {
		throw new RunShareError("invalid_key", "run share fragment key is invalid");
	}
	const key = new Uint8Array(Buffer.from(encoded, "base64url"));
	if (key.byteLength !== RUN_SHARE_KEY_BYTES || encodeRunShareKey(key) !== encoded) {
		throw new RunShareError("invalid_key", "run share fragment key has the wrong length");
	}
	return key;
}

function sealedHeaderBytes(): Uint8Array<ArrayBuffer> {
	return TEXT_ENCODER.encode(RUN_SHARE_SEALED_HEADER);
}

function hasSealedHeader(sealed: Uint8Array): boolean {
	const header = sealedHeaderBytes();
	if (sealed.byteLength < header.byteLength) return false;
	return header.every((byte, index) => sealed[index] === byte);
}

export async function sealRunShareBundle(bundle: RunShareBundle): Promise<RunShareSeal> {
	const plaintext = encodeRunShareBundle(bundle);
	const compressed = Bun.gzipSync(plaintext);
	const key = new Uint8Array(RUN_SHARE_KEY_BYTES);
	const nonce = new Uint8Array(RUN_SHARE_NONCE_BYTES);
	crypto.getRandomValues(key);
	crypto.getRandomValues(nonce);
	const cryptoKey = await crypto.subtle.importKey("raw", key, "AES-GCM", false, ["encrypt"]);
	const ciphertext = new Uint8Array(await crypto.subtle.encrypt(
		{ name: "AES-GCM", iv: nonce, additionalData: TEXT_ENCODER.encode(RUN_SHARE_AES_GCM_AAD) },
		cryptoKey,
		compressed,
	));
	const header = sealedHeaderBytes();
	const sealed = new Uint8Array(header.byteLength + nonce.byteLength + ciphertext.byteLength);
	sealed.set(header, 0);
	sealed.set(nonce, header.byteLength);
	sealed.set(ciphertext, header.byteLength + nonce.byteLength);
	if (sealed.byteLength > RUN_SHARE_MAX_SEALED_BYTES) {
		key.fill(0);
		throw new RunShareError("output_limit_exceeded", "sealed run share exceeds the byte limit", {
			limit: RUN_SHARE_MAX_SEALED_BYTES,
			observed: sealed.byteLength,
		});
	}
	const fragment = encodeRunShareKey(key);
	key.fill(0);
	return Object.freeze({ sealed, fragment });
}

async function gunzipBounded(compressed: Uint8Array<ArrayBuffer>): Promise<Uint8Array<ArrayBuffer>> {
	const stream = new Blob([compressed]).stream().pipeThrough(new DecompressionStream("gzip"));
	const reader = stream.getReader();
	const chunks: Uint8Array<ArrayBuffer>[] = [];
	let total = 0;
	while (true) {
		const result = await reader.read();
		if (result.done) break;
		const chunk = new Uint8Array(result.value);
		total += chunk.byteLength;
		if (total > RUN_SHARE_MAX_PLAINTEXT_BYTES) {
			await reader.cancel();
			throw new RunShareError("output_limit_exceeded", "run share plaintext exceeds the byte limit", {
				limit: RUN_SHARE_MAX_PLAINTEXT_BYTES,
				observed: total,
			});
		}
		chunks.push(chunk);
	}
	const plaintext = new Uint8Array(total);
	let offset = 0;
	for (const chunk of chunks) {
		plaintext.set(chunk, offset);
		offset += chunk.byteLength;
	}
	return plaintext;
}

export async function openRunShareBundle(
	sealed: Uint8Array,
	key: Uint8Array,
): Promise<RunShareBundle> {
	if (key.byteLength !== RUN_SHARE_KEY_BYTES) {
		throw new RunShareError("invalid_key", `run share key must be ${RUN_SHARE_KEY_BYTES} bytes`);
	}
	const header = sealedHeaderBytes();
	if (!hasSealedHeader(sealed)
		|| sealed.byteLength < header.byteLength + RUN_SHARE_NONCE_BYTES + RUN_SHARE_AUTH_TAG_BYTES
		|| sealed.byteLength > RUN_SHARE_MAX_SEALED_BYTES) {
		throw new RunShareError("invalid_bundle", "sealed run share has an invalid envelope");
	}
	const nonce = sealed.slice(header.byteLength, header.byteLength + RUN_SHARE_NONCE_BYTES);
	const ciphertext = sealed.slice(header.byteLength + RUN_SHARE_NONCE_BYTES);
	let compressed: Uint8Array<ArrayBuffer>;
	try {
		const cryptoKey = await crypto.subtle.importKey(
			"raw",
			new Uint8Array(key),
			"AES-GCM",
			false,
			["decrypt"],
		);
		compressed = new Uint8Array(await crypto.subtle.decrypt(
			{ name: "AES-GCM", iv: nonce, additionalData: TEXT_ENCODER.encode(RUN_SHARE_AES_GCM_AAD) },
			cryptoKey,
			ciphertext,
		));
	} catch (cause) {
		throw new RunShareError(
			"authentication_failed",
			"run share authentication failed",
			{},
			{ cause },
		);
	}
	let plaintext: Uint8Array<ArrayBuffer>;
	try { plaintext = await gunzipBounded(compressed); }
	catch (cause) {
		if (cause instanceof RunShareError) throw cause;
		throw new RunShareError("invalid_bundle", "run share compression stream is invalid", {}, { cause });
	}
	let parsed: unknown;
	try { parsed = JSON.parse(FATAL_DECODER.decode(plaintext)); }
	catch (cause) {
		throw new RunShareError("invalid_bundle", "run share plaintext is invalid", {}, { cause });
	}
	return validateRunShareBundle(parsed);
}

export async function openRunShareFragment(
	sealed: Uint8Array,
	fragment: string,
): Promise<RunShareBundle> {
	return openRunShareBundle(sealed, decodeRunShareFragment(fragment));
}
