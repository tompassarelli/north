import {
	WireEventWriter,
	jsonValue,
	wireArtifactId,
	wireMessageId,
	wireModelCallId,
	wireToolCallId,
	type JsonValue,
	type WireArtifactId,
	type WireCompletionEvidence,
	type WireEventDraft,
	type WireKnownEvent,
	type WireMessageId,
	type WireModelCallId,
	type WireToolCallId,
	type WireUsageSnapshot,
	type WireArtifactSink,
	type WireQueryRoute,
} from "../wire";
import { providerJoinEvidence } from "./provider-join";
import {
	isArtifactReadToolName,
	persistRetainedProviderMaterial,
	retainedProviderMaterial,
	retainedProviderPreview,
	type RetainedProviderMaterial,
} from "./retained-artifact";

const MAX_PROVIDER_ID_BYTES = 512;
const MAX_PROVIDER_TEXT_BYTES = 1_048_576;
const MAX_TOOL_COMPONENT_BYTES = 96;
const MAX_ITEMS_PER_TURN = 10_000;
const MAX_TURNS = 4_096;
const PROVIDER_ID = /^[A-Za-z0-9._:-]+$/;
const ERROR_CODE = /^[a-z][a-z0-9_.-]{0,127}$/;
const TEXT_ENCODER = new TextEncoder();

type UnknownRecord = Record<string, unknown>;

export type OpenAIWireNotificationMethod =
	| "configWarning"
	| "deprecationNotice"
	| "remoteControl/status/changed"
	| "mcpServer/startupStatus/updated"
	| "model/safetyBuffering/updated"
	| "account/rateLimits/updated"
	| "serverRequest/resolved"
	| "thread/started"
	| "thread/status/changed"
	| "turn/started"
	| "thread/tokenUsage/updated"
	| "item/started"
	| "item/agentMessage/delta"
	| "item/plan/delta"
	| "item/reasoning/summaryTextDelta"
	| "item/reasoning/summaryPartAdded"
	| "item/reasoning/textDelta"
	| "item/mcpToolCall/progress"
	| "item/commandExecution/outputDelta"
	| "item/commandExecution/terminalInteraction"
	| "item/fileChange/outputDelta"
	| "item/fileChange/patchUpdated"
	| "item/completed"
	| "turn/diff/updated"
	| "turn/plan/updated"
	| "hook/started"
	| "hook/completed"
	| "turn/completed";

const IGNORED_NOTIFICATION_METHODS = new Set<string>([
	"configWarning",
	"deprecationNotice",
	"remoteControl/status/changed",
	"mcpServer/startupStatus/updated",
	"model/safetyBuffering/updated",
	"account/rateLimits/updated",
	"serverRequest/resolved",
	"thread/started",
	"thread/status/changed",
	"item/reasoning/summaryTextDelta",
	"item/reasoning/summaryPartAdded",
	"item/reasoning/textDelta",
	"hook/started",
	"hook/completed",
] satisfies readonly OpenAIWireNotificationMethod[]);

export type OpenAIWireNormalizationErrorCode =
	| "malformed_notification"
	| "unsupported_notification"
	| "lifecycle_violation"
	| "artifact_persistence_failed";

export class OpenAIWireNormalizationError extends Error {
	readonly code: OpenAIWireNormalizationErrorCode;

	constructor(
		code: OpenAIWireNormalizationErrorCode,
		message: string,
		options?: ErrorOptions,
	) {
		super(message, options);
		this.name = "OpenAIWireNormalizationError";
		this.code = code;
	}
}

export type OpenAIWireRouteMetadata = WireQueryRoute;

export interface OpenAIWireIdFactory {
	modelCall(sequence: number): WireModelCallId;
	message(sequence: number): WireMessageId;
	toolCall(sequence: number): WireToolCallId;
	artifact(sequence: number): WireArtifactId;
}

export interface OpenAIWireNormalizerOptions {
	writer: WireEventWriter;
	route: OpenAIWireRouteMetadata;
	/** Optional durable sink. Without one, patch activity remains semantic progress only. */
	artifacts?: WireArtifactSink;
	ids?: OpenAIWireIdFactory;
}

export interface OpenAIWireTurnOutcome {
	status: "succeeded" | "failed" | "cancelled";
	modelCallId: WireModelCallId;
	usage: WireUsageSnapshot;
	errorCode?: string;
}

export type OpenAIWireNotificationResult =
	| { type: "events"; events: readonly WireKnownEvent[] }
	| OpenAIWireTurnTerminalResult;

export interface OpenAIWireTurnTerminalResult {
	type: "turn.terminal";
	events: readonly WireKnownEvent[];
	outcome: OpenAIWireTurnOutcome;
}

export interface OpenAIWireTurnSettlementInput {
	status: "failed" | "cancelled";
	origin: "provider" | "north";
	errorCode: string;
	/** Explicit incomplete terminal observation; absence means no terminal usage. */
	usage?: WireUsageSnapshot;
	evidence?: WireCompletionEvidence;
}

interface OpenMessageItem {
	category: "message";
	kind: "agentMessage";
	messageId: WireMessageId;
	hasDelta: boolean;
}

export type OpenAIWireSemanticToolKind =
	| "commandExecution"
	| "mcpToolCall"
	| "fileChange"
	| "webSearch"
	| "todoList";

export interface OpenAIWireToolIdentity {
	readonly kind: OpenAIWireSemanticToolKind;
	readonly name: string;
}

interface OpenToolItem {
	category: "tool";
	kind: OpenAIWireSemanticToolKind;
	toolCallId: WireToolCallId;
	name: string;
	latestArtifactId?: WireArtifactId;
	latestArtifactDigest?: string;
}

interface IgnoredItem {
	category: "ignored";
	kind: "reasoning" | "plan";
}

type OpenItem = OpenMessageItem | OpenToolItem | IgnoredItem;

interface ActiveTurn {
	providerTurnId: string;
	modelCallId: WireModelCallId;
	items: Map<string, OpenItem>;
	completedItemIds: Set<string>;
	toolItems: number;
	usage?: WireUsageSnapshot;
	providerUsage?: OpenAIProviderUsage;
}

interface OpenAIProviderUsage {
	inputTokens: number;
	outputTokens: number;
	cacheReadTokens: number;
	cacheWriteTokens: number;
	reasoningTokens: number;
}

function normalizationError(
	code: OpenAIWireNormalizationErrorCode,
	message: string,
	cause?: unknown,
): never {
	throw new OpenAIWireNormalizationError(
		code,
		message,
		cause === undefined ? undefined : { cause },
	);
}

function record(value: unknown, label: string): UnknownRecord {
	if (!value || typeof value !== "object" || Array.isArray(value)) {
		return normalizationError("malformed_notification", `${label} must be an object`);
	}
	return value as UnknownRecord;
}

function bytes(value: string): number {
	return TEXT_ENCODER.encode(value).byteLength;
}

function providerId(value: unknown, label: string): string {
	if (typeof value !== "string" || !value || value !== value.trim()
		|| bytes(value) > MAX_PROVIDER_ID_BYTES || !PROVIDER_ID.test(value)) {
		return normalizationError("malformed_notification", `${label} is invalid`);
	}
	return value;
}

function boundedText(value: unknown, label: string, maxBytes = MAX_PROVIDER_TEXT_BYTES): string {
	if (typeof value !== "string" || bytes(value) > maxBytes) {
		return normalizationError("malformed_notification", `${label} is invalid`);
	}
	return value;
}

function positiveInteger(value: unknown, label: string): number {
	if (!Number.isSafeInteger(value) || (value as number) < 1) {
		return normalizationError("malformed_notification", `${label} must be a positive safe integer`);
	}
	return value as number;
}

function counter(value: unknown, label: string): number {
	if (!Number.isSafeInteger(value) || (value as number) < 0) {
		return normalizationError("malformed_notification", `${label} must be a non-negative safe integer`);
	}
	return value as number;
}

function addUsageCounter(base: number, current: number, label: string): number {
	if (!Number.isSafeInteger(base) || base < 0) {
		return normalizationError("lifecycle_violation", `${label} base is not a safe usage counter`);
	}
	const combined = BigInt(base) + BigInt(current);
	if (combined > BigInt(Number.MAX_SAFE_INTEGER)) {
		return normalizationError("malformed_notification", `${label} exceeds the safe integer limit`);
	}
	return Number(combined);
}

function errorCode(value: unknown): string {
	if (typeof value !== "string" || !ERROR_CODE.test(value)) {
		return normalizationError("malformed_notification", "turn settlement errorCode is invalid");
	}
	return value;
}

function defaultIds(): OpenAIWireIdFactory {
	return {
		modelCall: () => wireModelCallId(`model-call:${crypto.randomUUID()}`),
		message: () => wireMessageId(`message:${crypto.randomUUID()}`),
		toolCall: () => wireToolCallId(`tool:${crypto.randomUUID()}`),
		artifact: () => wireArtifactId(`artifact:${crypto.randomUUID()}`),
	};
}

function toolName(item: UnknownRecord, kind: OpenAIWireSemanticToolKind): string {
	if (kind === "mcpToolCall") {
		const server = boundedText(item.server, "Codex MCP server", MAX_TOOL_COMPONENT_BYTES);
		const tool = boundedText(item.tool, "Codex MCP tool", MAX_TOOL_COMPONENT_BYTES);
		if (!server || !tool || /[\u0000-\u001f\u007f/]/.test(server)
			|| /[\u0000-\u001f\u007f/]/.test(tool)) {
			return normalizationError("malformed_notification", "Codex MCP identity is invalid");
		}
		return `mcp:${server}/${tool}`;
	}
	if (kind === "commandExecution") return "command";
	if (kind === "fileChange") return "file-change";
	if (kind === "webSearch") return "web-search";
	return "todo-list";
}

function semanticToolKind(value: string): OpenAIWireSemanticToolKind | undefined {
	if (value === "commandExecution" || value === "mcpToolCall" || value === "fileChange"
		|| value === "webSearch" || value === "todoList") return value;
	return undefined;
}

/** One privacy-bounded semantic identity shared by Wire projection and crash harvest. */
export function openAIWireToolIdentity(
	item: Readonly<Record<string, unknown>>,
): OpenAIWireToolIdentity | undefined {
	const kind = semanticToolKind(boundedText(item.type, "Codex item type", 128));
	if (!kind) return undefined;
	return Object.freeze({ kind, name: toolName(item, kind) });
}

function terminalToolStatus(item: UnknownRecord): {
	status: "succeeded" | "failed" | "cancelled";
	errorCode?: string;
} {
	const observed = item.status;
	if (observed === undefined || observed === "completed" || observed === "succeeded") {
		if (item.type === "commandExecution" && item.exitCode !== undefined
			&& item.exitCode !== null && item.exitCode !== 0) {
			return { status: "failed", errorCode: "command_failed" };
		}
		return { status: "succeeded" };
	}
	if (observed === "cancelled" || observed === "canceled" || observed === "interrupted") {
		return { status: "cancelled", errorCode: "tool_cancelled" };
	}
	if (observed === "declined") return { status: "failed", errorCode: "tool_declined" };
	if (observed === "failed" || observed === "error") {
		return { status: "failed", errorCode: "tool_failed" };
	}
	return normalizationError("malformed_notification", "Codex tool completion status is invalid");
}

function usageCounters(value: unknown, label: string): {
	totalTokens: number;
	inputTokens: number;
	cachedInputTokens: number;
	outputTokens: number;
	reasoningOutputTokens: number;
} {
	const source = record(value, label);
	const totalTokens = counter(source.totalTokens, `${label} totalTokens`);
	const inputTokens = counter(source.inputTokens, `${label} inputTokens`);
	const cachedInputTokens = counter(source.cachedInputTokens, `${label} cachedInputTokens`);
	const outputTokens = counter(source.outputTokens, `${label} outputTokens`);
	const reasoningOutputTokens = counter(
		source.reasoningOutputTokens,
		`${label} reasoningOutputTokens`,
	);
	if (totalTokens !== inputTokens + outputTokens || cachedInputTokens > inputTokens
		|| reasoningOutputTokens > outputTokens) {
		return normalizationError("malformed_notification", `${label} is incoherent`);
	}
	return { totalTokens, inputTokens, cachedInputTokens, outputTokens, reasoningOutputTokens };
}

function frozenEvents(events: readonly WireKnownEvent[]): readonly WireKnownEvent[] {
	return Object.freeze([...events]);
}

function eventResult(events: readonly WireKnownEvent[]): OpenAIWireNotificationResult {
	return Object.freeze({ type: "events", events: frozenEvents(events) });
}

export class OpenAIWireNormalizer {
	readonly #writer: WireEventWriter;
	readonly #route: OpenAIWireRouteMetadata;
	readonly #ids: OpenAIWireIdFactory;
	readonly #artifacts?: WireArtifactSink;
	#providerThreadId?: string;
	#providerUsageBase?: OpenAIProviderUsage;
	#lastCompletedProviderUsage?: OpenAIProviderUsage;
	#activeTurn?: ActiveTurn;
	#settledTurnIds = new Set<string>();
	#turnSequence = 0;
	#modelCallSequence = 0;
	#messageSequence = 0;
	#toolCallSequence = 0;
	#artifactSequence = 0;

	constructor(options: OpenAIWireNormalizerOptions) {
		const snapshot = options.writer.snapshot();
		if (!snapshot || (snapshot.lifecycle !== "running" && snapshot.lifecycle !== "waiting")) {
			normalizationError("lifecycle_violation", "wire writer run must already be started and live");
		}
		if (Object.values(snapshot.modelCalls).some((call) => call.status === "running")
			|| Object.values(snapshot.toolCalls).some((call) => call.status === "pending")
			|| Object.values(snapshot.messages).some((message) => message.stage !== "completed")) {
			normalizationError("lifecycle_violation", "wire writer has lifecycle work owned by another adapter");
		}
		positiveInteger(options.route.attempt, "semantic route attempt");
		if (options.route.contextWindow !== undefined) {
			positiveInteger(options.route.contextWindow, "semantic route context window");
		}
		this.#writer = options.writer;
		this.#route = Object.freeze({ ...options.route });
		this.#ids = options.ids ?? defaultIds();
		this.#artifacts = options.artifacts;
	}

	normalize(method: string, params: unknown): OpenAIWireNotificationResult {
		this.#assertLive();
		if (IGNORED_NOTIFICATION_METHODS.has(method)) {
			record(params, "Codex ignored notification params");
			return eventResult([]);
		}
		switch (method as OpenAIWireNotificationMethod) {
			case "turn/started": return this.#turnStarted(params);
			case "thread/tokenUsage/updated": return this.#usageUpdated(params);
			case "item/started": return this.#itemStarted(params);
			case "item/agentMessage/delta": return this.#messageDelta(params);
			case "item/plan/delta": return this.#planDelta(params);
			case "item/mcpToolCall/progress": return this.#toolProgress(params, "mcpToolCall", "running");
			case "item/commandExecution/outputDelta":
				return this.#toolProgress(params, "commandExecution", "streaming-output");
			case "item/commandExecution/terminalInteraction":
				return this.#terminalInteraction(params);
			case "item/fileChange/outputDelta":
				return this.#toolProgress(params, "fileChange", "streaming-output");
			case "item/fileChange/patchUpdated": return this.#patchUpdated(params);
			case "item/completed": return this.#itemCompleted(params);
			case "turn/diff/updated": return this.#turnDiffUpdated(params);
			case "turn/plan/updated": return this.#turnPlanUpdated(params);
			case "turn/completed": return this.#turnCompleted(params);
			default:
				return normalizationError(
					"unsupported_notification",
					"Codex notification is outside the wire-v2 normalization subset",
				);
		}
	}

	settleTurn(input: OpenAIWireTurnSettlementInput): OpenAIWireTurnTerminalResult {
		this.#assertLive();
		const turn = this.#requireTurn();
		if (input.status !== "failed" && input.status !== "cancelled") {
			return normalizationError("malformed_notification", "turn settlement status is invalid");
		}
		const code = errorCode(input.errorCode);
		const snapshot = this.#writer.snapshot()!;
		const usage = input.usage ?? snapshot.usage;
		const usageCoverage = input.usage === undefined
			? "unavailable" as const : "partial" as const;
		const drafts: WireEventDraft[] = [];
		for (const item of turn.items.values()) {
			if (item.category === "tool") {
				drafts.push({
					kind: "tool.terminal",
					toolCallId: item.toolCallId,
					status: input.status === "cancelled" ? "cancelled" : "synthetic_failure",
					origin: "north",
					errorCode: code,
				});
			} else if (item.category === "message") {
				drafts.push({
					kind: "message.recorded",
					messageId: item.messageId,
					modelCallId: turn.modelCallId,
					stage: "completed",
					role: "assistant",
				});
			}
		}
		drafts.push({
			kind: "model-call.completed",
			modelCallId: turn.modelCallId,
			status: input.status,
			origin: input.origin,
			usage,
			usageCoverage,
			errorCode: code,
			...(input.evidence === undefined ? {} : { evidence: input.evidence }),
		});
		const events = this.#writer.appendAll(drafts);
		this.#settledTurnIds.add(`${this.#providerThreadId}\0${turn.providerTurnId}`);
		this.#activeTurn = undefined;
		const outcome: OpenAIWireTurnOutcome = Object.freeze({
			status: input.status,
			modelCallId: turn.modelCallId,
			usage,
			errorCode: code,
		});
		return Object.freeze({ type: "turn.terminal", events: frozenEvents(events), outcome });
	}

	/** Close a dead managed provider attempt before its replacement starts. */
	settleProviderRespawn(): OpenAIWireTurnTerminalResult {
		const turn = this.#requireTurn();
		const threadId = this.#providerThreadId
			?? normalizationError("lifecycle_violation", "Codex respawn has no provider thread");
		return this.settleTurn({
			status: "failed",
			origin: "north",
			errorCode: "provider_session_replaced",
			evidence: {
				providerJoin: providerJoinEvidence("openai", {
					sessionId: threadId,
					turnIds: [turn.providerTurnId],
					sessionPersistence: "ephemeral",
				}),
				turns: {
					unit: "provider-turn",
					count: 1,
					toolItems: turn.toolItems,
					comparable: false,
				},
				failure: {
					detail: "provider_session_replaced",
					landed: { completedTurns: 0, toolItems: turn.toolItems },
				},
			},
		});
	}

	hasActiveTurn(): boolean {
		return this.#activeTurn !== undefined;
	}

	lastCompletedProviderUsage(): Readonly<OpenAIProviderUsage> | undefined {
		return this.#lastCompletedProviderUsage;
	}

	#assertLive(): void {
		const lifecycle = this.#writer.snapshot()?.lifecycle;
		if (lifecycle !== "running" && lifecycle !== "waiting") {
			normalizationError("lifecycle_violation", "Codex wire run is no longer live");
		}
	}

	#requireTurn(): ActiveTurn {
		return this.#activeTurn
			?? normalizationError("lifecycle_violation", "Codex notification has no active turn");
	}

	#runtime(params: unknown): { source: UnknownRecord; turn: ActiveTurn } {
		const source = record(params, "Codex notification params");
		const turn = this.#requireTurn();
		const threadId = providerId(source.threadId, "Codex thread id");
		const turnId = providerId(source.turnId, "Codex turn id");
		if (threadId !== this.#providerThreadId || turnId !== turn.providerTurnId) {
			return normalizationError("lifecycle_violation", "Codex notification belongs to another turn");
		}
		return { source, turn };
	}

	#turnStarted(params: unknown): OpenAIWireNotificationResult {
		if (this.#activeTurn) {
			return normalizationError("lifecycle_violation", "Codex turn started while another turn is active");
		}
		if (this.#turnSequence >= MAX_TURNS) {
			return normalizationError("lifecycle_violation", "Codex turn count exceeds the wire bound");
		}
		const source = record(params, "Codex turn-start params");
		const threadId = providerId(source.threadId, "Codex thread id");
		const providerTurn = record(source.turn, "Codex started turn");
		const turnId = providerId(providerTurn.id, "Codex turn id");
		if (providerTurn.status !== undefined && providerTurn.status !== "inProgress") {
			return normalizationError("malformed_notification", "Codex turn start status is invalid");
		}
		const turnKey = `${threadId}\0${turnId}`;
		if (this.#settledTurnIds.has(turnKey)) {
			return normalizationError("lifecycle_violation", "Codex turn identity was reused");
		}
		const modelCallId = this.#ids.modelCall(this.#modelCallSequence);
		const currentUsage = this.#writer.snapshot()!.usage.lifetime;
		const providerUsageBase = threadId === this.#providerThreadId
			? this.#providerUsageBase
			: Object.freeze({
				inputTokens: currentUsage.inputTokens,
				outputTokens: currentUsage.outputTokens,
				cacheReadTokens: currentUsage.cacheReadTokens,
				cacheWriteTokens: currentUsage.cacheWriteTokens,
				reasoningTokens: currentUsage.reasoningTokens,
			});
		if (!providerUsageBase) {
			return normalizationError(
				"lifecycle_violation",
				"Codex continuation thread has no provider-session usage base",
			);
		}
		const events = this.#writer.appendAll([{
			kind: "model-call.started",
			modelCallId,
			model: {
				provider: "openai",
				...(this.#route.model.tier === undefined ? {} : { tier: this.#route.model.tier }),
				...(this.#route.model.capabilityClass === undefined
					? {} : { capabilityClass: this.#route.model.capabilityClass }),
			},
			...(this.#route.effort === undefined ? {} : { effort: this.#route.effort }),
			attempt: this.#route.attempt,
		}]);
		this.#providerThreadId = threadId;
		this.#providerUsageBase = providerUsageBase;
		this.#lastCompletedProviderUsage = undefined;
		this.#activeTurn = {
			providerTurnId: turnId,
			modelCallId,
			items: new Map(),
			completedItemIds: new Set(),
			toolItems: 0,
		};
		this.#turnSequence += 1;
		this.#modelCallSequence += 1;
		return eventResult(events);
	}

	#usageUpdated(params: unknown): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const tokenUsage = record(source.tokenUsage, "Codex token usage");
		const total = usageCounters(tokenUsage.total, "Codex lifetime token usage");
		const last = tokenUsage.last === undefined
			? undefined : usageCounters(tokenUsage.last, "Codex current-context token usage");
		const rawWindow = tokenUsage.modelContextWindow;
		const contextWindow = rawWindow === undefined || rawWindow === null
			? this.#route.contextWindow
			: positiveInteger(rawWindow, "Codex model context window");
		const contextTokens = last?.totalTokens ?? 0;
		if (contextWindow !== undefined && contextTokens > contextWindow) {
			return normalizationError(
				"malformed_notification",
				"Codex current-context usage exceeds its semantic context window",
			);
		}
		const base = this.#providerUsageBase
			?? normalizationError("lifecycle_violation", "Codex provider session has no usage base");
		const providerUsage: OpenAIProviderUsage = Object.freeze({
			inputTokens: total.inputTokens,
			outputTokens: total.outputTokens,
			cacheReadTokens: total.cachedInputTokens,
			cacheWriteTokens: 0,
			reasoningTokens: total.reasoningOutputTokens,
		});
		const modelCalls = this.#writer.snapshot()!.usage.lifetime.modelCalls;
		const usage: WireUsageSnapshot = Object.freeze({
			lifetime: Object.freeze({
				inputTokens: addUsageCounter(base.inputTokens, providerUsage.inputTokens, "Codex input usage"),
				outputTokens: addUsageCounter(base.outputTokens, providerUsage.outputTokens, "Codex output usage"),
				cacheReadTokens: addUsageCounter(
					base.cacheReadTokens,
					providerUsage.cacheReadTokens,
					"Codex cache-read usage",
				),
				cacheWriteTokens: addUsageCounter(
					base.cacheWriteTokens,
					providerUsage.cacheWriteTokens,
					"Codex cache-write usage",
				),
				reasoningTokens: addUsageCounter(
					base.reasoningTokens,
					providerUsage.reasoningTokens,
					"Codex reasoning usage",
				),
				modelCalls,
			}),
			context: Object.freeze({
				tokens: contextTokens,
				...(contextWindow === undefined ? {} : { window: contextWindow }),
			}),
		});
		const events = this.#writer.appendAll([{
			kind: "run.progress",
			lifecycle: "running",
			progress: { usage },
		}]);
		turn.usage = usage;
		turn.providerUsage = providerUsage;
		return eventResult(events);
	}

	#itemStarted(params: unknown): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const item = record(source.item, "Codex started item");
		const itemId = providerId(item.id, "Codex item id");
		const kind = boundedText(item.type, "Codex item type", 128);
		if (turn.items.size + turn.completedItemIds.size >= MAX_ITEMS_PER_TURN) {
			return normalizationError("lifecycle_violation", "Codex item count exceeds the wire bound");
		}
		if (turn.items.has(itemId) || turn.completedItemIds.has(itemId)) {
			return normalizationError("lifecycle_violation", "Codex item started more than once");
		}
		if (kind === "agentMessage") {
			const messageId = this.#ids.message(this.#messageSequence);
			const events = this.#writer.appendAll([{
				kind: "message.recorded",
				messageId,
				modelCallId: turn.modelCallId,
				stage: "started",
				role: "assistant",
			}]);
			turn.items.set(itemId, { category: "message", kind, messageId, hasDelta: false });
			this.#messageSequence += 1;
			return eventResult(events);
		}
		if (kind === "reasoning" || kind === "plan") {
			turn.items.set(itemId, { category: "ignored", kind });
			return eventResult([]);
		}
		const tool = openAIWireToolIdentity(item);
		if (!tool) {
			return normalizationError("unsupported_notification", "Codex item type has no wire-v2 semantic mapping");
		}
		const toolCallId = this.#ids.toolCall(this.#toolCallSequence);
		const events = this.#writer.appendAll([{
			kind: "tool.admitted",
			toolCallId,
			modelCallId: turn.modelCallId,
			name: tool.name,
			schema: {
				status: "unavailable",
				reason: "tool schema unavailable at normalization boundary",
			},
		}]);
		turn.items.set(itemId, {
			category: "tool", kind: tool.kind, toolCallId, name: tool.name,
		});
		this.#toolCallSequence += 1;
		return eventResult(events);
	}

	#openItem(turn: ActiveTurn, itemId: string): OpenItem {
		const item = turn.items.get(itemId);
		if (item) return item;
		if (turn.completedItemIds.has(itemId)) {
			return normalizationError("lifecycle_violation", "Codex item emitted an event after completion");
		}
		return normalizationError("lifecycle_violation", "Codex item event arrived before admission");
	}

	#messageDelta(params: unknown): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const itemId = providerId(source.itemId, "Codex item id");
		const delta = boundedText(source.delta, "Codex assistant message delta");
		let item = turn.items.get(itemId);
		if (!item) {
			if (turn.completedItemIds.has(itemId)) {
				return normalizationError("lifecycle_violation", "Codex item emitted an event after completion");
			}
			if (turn.items.size + turn.completedItemIds.size >= MAX_ITEMS_PER_TURN) {
				return normalizationError("lifecycle_violation", "Codex item count exceeds the wire bound");
			}
			const messageId = this.#ids.message(this.#messageSequence);
			const drafts: WireEventDraft[] = [{
				kind: "message.recorded",
				messageId,
				modelCallId: turn.modelCallId,
				stage: "started",
				role: "assistant",
			}];
			if (delta) drafts.push({
				kind: "message.recorded",
				messageId,
				modelCallId: turn.modelCallId,
				stage: "delta",
				role: "assistant",
				content: delta,
			});
			const events = this.#writer.appendAll(drafts);
			item = { category: "message", kind: "agentMessage", messageId, hasDelta: Boolean(delta) };
			turn.items.set(itemId, item);
			this.#messageSequence += 1;
			return eventResult(events);
		}
		if (item.category !== "message") {
			return normalizationError("lifecycle_violation", "Codex message delta belongs to another item kind");
		}
		if (!delta) return eventResult([]);
		const events = this.#writer.appendAll([{
			kind: "message.recorded",
			messageId: item.messageId,
			modelCallId: turn.modelCallId,
			stage: "delta",
			role: "assistant",
			content: delta,
		}]);
		item.hasDelta = true;
		return eventResult(events);
	}

	#planDelta(params: unknown): OpenAIWireNotificationResult {
		const { source } = this.#runtime(params);
		providerId(source.itemId, "Codex plan item id");
		const delta = boundedText(source.delta, "Codex plan delta");
		if (!delta) return eventResult([]);
		return eventResult(this.#writer.appendAll([{
			kind: "run.progress",
			lifecycle: "running",
			progress: { currentAction: "Updating execution plan" },
		}]));
	}

	#toolProgress(
		params: unknown,
		expectedKind: Extract<
			OpenAIWireSemanticToolKind,
			"mcpToolCall" | "commandExecution" | "fileChange"
		>,
		phase: "running" | "streaming-output",
	): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const itemId = providerId(source.itemId, "Codex item id");
		const item = this.#openItem(turn, itemId);
		if (item.category !== "tool" || item.kind !== expectedKind) {
			return normalizationError("lifecycle_violation", "Codex tool progress belongs to another item kind");
		}
		const observed = expectedKind === "mcpToolCall"
			? boundedText(source.message, "Codex MCP progress")
			: boundedText(source.delta, "Codex tool output delta");
		const progress: JsonValue = Object.freeze({ phase, observedBytes: bytes(observed) });
		return eventResult(this.#writer.appendAll([{
			kind: "tool.progress",
			toolCallId: item.toolCallId,
			progress,
		}]));
	}

	#terminalInteraction(params: unknown): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const itemId = providerId(source.itemId, "Codex item id");
		const item = this.#openItem(turn, itemId);
		if (item.category !== "tool" || item.kind !== "commandExecution") {
			return normalizationError(
				"lifecycle_violation",
				"Codex terminal interaction belongs to another item kind",
			);
		}
		providerId(source.processId, "Codex command process id");
		const stdin = boundedText(source.stdin, "Codex terminal interaction");
		return eventResult(this.#writer.appendAll([{
			kind: "tool.progress",
			toolCallId: item.toolCallId,
			progress: { phase: "terminal-interaction", observedBytes: bytes(stdin) },
		}]));
	}

	#patchUpdated(params: unknown): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const itemId = providerId(source.itemId, "Codex item id");
		const item = this.#openItem(turn, itemId);
		if (item.category !== "tool" || item.kind !== "fileChange") {
			return normalizationError("lifecycle_violation", "Codex patch update belongs to another item kind");
		}
		if (!Array.isArray(source.changes)) {
			return normalizationError("malformed_notification", "Codex patch changes must be an array");
		}
		let normalized: JsonValue;
		try {
			normalized = jsonValue(source.changes, "Codex patch changes");
		} catch (cause) {
			return normalizationError("malformed_notification", "Codex patch changes exceed wire bounds", cause);
		}
		const serialized = JSON.stringify(normalized);
		const filesChanged = source.changes.length;
		if (!this.#artifacts) {
			return eventResult(this.#writer.appendAll([
				{
					kind: "tool.progress",
					toolCallId: item.toolCallId,
					progress: { phase: "patch-updated", filesChanged },
				},
				{
					kind: "run.progress",
					lifecycle: "running",
					progress: { currentAction: "Applying workspace changes" },
				},
			]));
		}
		const artifactId = this.#ids.artifact(this.#artifactSequence);
		const digest = new Bun.CryptoHasher("sha256").update(serialized).digest("hex");
		const artifact = Object.freeze({
			artifactId,
			mediaType: "application/vnd.north.patch+json",
			content: serialized,
			digest,
			label: "workspace patch",
		});
		try {
			const receipt = this.#artifacts.persist(artifact);
			if (!receipt || receipt.artifactId !== artifactId || receipt.digest !== digest) {
				return normalizationError(
					"artifact_persistence_failed",
					"Codex patch artifact persistence receipt does not match",
				);
			}
		} catch (cause) {
			if (cause instanceof OpenAIWireNormalizationError) throw cause;
			return normalizationError(
				"artifact_persistence_failed",
				"Codex patch artifact could not be persisted",
				cause,
			);
		}
		const events = this.#writer.appendAll([
			{
				kind: "artifact.published",
				artifactId,
				mediaType: artifact.mediaType,
				bytes: bytes(serialized),
				digest,
				label: artifact.label,
			},
			{
				kind: "tool.progress",
				toolCallId: item.toolCallId,
				progress: { phase: "patch-updated", filesChanged },
				outputArtifactId: artifactId,
			},
			{
				kind: "run.progress",
				lifecycle: "running",
				progress: {
					currentAction: "Applying workspace changes",
					patch: { artifactId, filesChanged },
				},
			},
		]);
		item.latestArtifactId = artifactId;
		item.latestArtifactDigest = digest;
		this.#artifactSequence += 1;
		return eventResult(events);
	}

	#retainedTerminalMaterial(
		turn: ActiveTurn,
		itemId: string,
		item: OpenToolItem,
		providerItem: UnknownRecord,
	): { material?: RetainedProviderMaterial; preview?: string } {
		let value: unknown;
		let kind: string;
		let label: string;
		if (item.kind === "commandExecution") {
			value = providerItem.aggregatedOutput;
			if (value === undefined || value === null) return {};
			if (typeof value !== "string") {
				return normalizationError(
					"malformed_notification",
					"Codex command aggregate output is invalid",
				);
			}
			kind = "command-output";
			label = "command output";
		} else if (item.kind === "mcpToolCall") {
			if (isArtifactReadToolName("openai", item.name)) return {};
			value = providerItem.result;
			if (value === undefined) return {};
			kind = "mcp-tool-result";
			label = "MCP tool result";
		} else return {};
		let material: RetainedProviderMaterial;
		try {
			material = retainedProviderMaterial({
				runId: this.#writer.runId,
				provider: "openai",
				kind,
				identity: `${this.#providerThreadId ?? ""}\0${turn.providerTurnId}\0${itemId}`,
				value,
				label,
			});
		} catch (cause) {
			return normalizationError(
				"malformed_notification",
				"Codex tool result is not retainable",
				cause,
			);
		}
		if (this.#artifacts) {
			try {
				persistRetainedProviderMaterial(this.#artifacts, material);
			} catch (cause) {
				return normalizationError(
					"artifact_persistence_failed",
					"Codex tool result artifact could not be persisted",
					cause,
				);
			}
		}
		return {
			...(this.#artifacts === undefined ? {} : { material }),
			preview: retainedProviderPreview(value),
		};
	}

	#itemCompleted(params: unknown): OpenAIWireNotificationResult {
		const { source, turn } = this.#runtime(params);
		const providerItem = record(source.item, "Codex completed item");
		const itemId = providerId(providerItem.id, "Codex item id");
		const observedKind = boundedText(providerItem.type, "Codex item type", 128);
		const item = this.#openItem(turn, itemId);
		if (observedKind !== item.kind) {
			return normalizationError("lifecycle_violation", "Codex item type changed before completion");
		}
		let events: readonly WireKnownEvent[];
		if (item.category === "message") {
			const content = boundedText(providerItem.text, "Codex completed assistant message");
			events = this.#writer.appendAll([{
				kind: "message.recorded",
				messageId: item.messageId,
				modelCallId: turn.modelCallId,
				stage: "completed",
				role: "assistant",
				...(item.hasDelta ? {} : { content }),
			}]);
		} else if (item.category === "tool") {
			const terminal = terminalToolStatus(providerItem);
			const retained = this.#retainedTerminalMaterial(turn, itemId, item, providerItem);
			const resultArtifactId = retained.material?.artifactId ?? item.latestArtifactId;
			const resultArtifactDigest = retained.material?.digest ?? item.latestArtifactDigest;
			const drafts: WireEventDraft[] = [];
			if (retained.material) {
				drafts.push({
					kind: "artifact.published",
					artifactId: retained.material.artifactId,
					mediaType: retained.material.mediaType,
					bytes: retained.material.bytes,
					digest: retained.material.digest,
					label: retained.material.label,
				});
			}
			drafts.push({
				kind: "tool.terminal",
				toolCallId: item.toolCallId,
				status: terminal.status,
				origin: "provider",
				...(retained.preview === undefined ? {} : { resultPreview: retained.preview }),
				...(resultArtifactId === undefined ? {} : { resultArtifactId }),
				...(resultArtifactDigest === undefined ? {} : { resultArtifactDigest }),
				...(terminal.errorCode === undefined ? {} : { errorCode: terminal.errorCode }),
			});
			events = this.#writer.appendAll(drafts);
		} else events = Object.freeze([]);
		turn.items.delete(itemId);
		turn.completedItemIds.add(itemId);
		if (observedKind !== "agentMessage" && observedKind !== "reasoning") {
			turn.toolItems += 1;
		}
		return eventResult(events);
	}

	#turnDiffUpdated(params: unknown): OpenAIWireNotificationResult {
		const { source } = this.#runtime(params);
		const diff = boundedText(source.diff, "Codex turn diff");
		if (!diff) return eventResult([]);
		return eventResult(this.#writer.appendAll([{
			kind: "run.progress",
			lifecycle: "running",
			progress: { currentAction: "Reviewing workspace changes" },
		}]));
	}

	#turnPlanUpdated(params: unknown): OpenAIWireNotificationResult {
		const { source } = this.#runtime(params);
		const explanation = source.explanation === null
			? null : boundedText(source.explanation, "Codex turn-plan explanation");
		if (!Array.isArray(source.plan)) {
			return normalizationError("malformed_notification", "Codex turn plan must be an array");
		}
		try {
			jsonValue(source.plan, "Codex turn plan");
		} catch (cause) {
			return normalizationError("malformed_notification", "Codex turn plan exceeds wire bounds", cause);
		}
		if (!explanation && source.plan.length === 0) return eventResult([]);
		return eventResult(this.#writer.appendAll([{
			kind: "run.progress",
			lifecycle: "running",
			progress: { currentAction: "Updating execution plan" },
		}]));
	}

	#turnCompleted(params: unknown): OpenAIWireNotificationResult {
		const source = record(params, "Codex turn-completion params");
		const turn = this.#requireTurn();
		const threadId = providerId(source.threadId, "Codex thread id");
		const providerTurn = record(source.turn, "Codex completed turn");
		const turnId = providerId(providerTurn.id, "Codex turn id");
		if (threadId !== this.#providerThreadId || turnId !== turn.providerTurnId) {
			return normalizationError("lifecycle_violation", "Codex turn terminal belongs to another turn");
		}
		if (providerTurn.status !== undefined && providerTurn.status !== "completed") {
			return normalizationError("malformed_notification", "Codex turn terminal status is invalid");
		}
		if (turn.items.size) {
			return normalizationError("lifecycle_violation", "Codex turn completed with open item lifecycles");
		}
		if (!turn.usage) {
			return normalizationError("lifecycle_violation", "Codex turn completed without exact usage");
		}
		const events = this.#writer.appendAll([{
			kind: "model-call.completed",
			modelCallId: turn.modelCallId,
			status: "succeeded",
			origin: "provider",
			usage: turn.usage,
			usageCoverage: "exact",
			evidence: {
				providerJoin: providerJoinEvidence("openai", {
					sessionId: threadId,
					turnIds: [turnId],
					sessionPersistence: "ephemeral",
				}),
				turns: {
					unit: "provider-turn",
					count: 1,
					toolItems: turn.toolItems,
					comparable: false,
				},
				...(providerTurn.durationMs === undefined ? {} : {
					providerDurationMs: counter(
						providerTurn.durationMs,
						"Codex completed turn durationMs",
					),
				}),
			},
		}]);
		const outcome: OpenAIWireTurnOutcome = Object.freeze({
			status: "succeeded",
			modelCallId: turn.modelCallId,
			usage: turn.usage,
		});
		this.#lastCompletedProviderUsage = turn.providerUsage;
		this.#settledTurnIds.add(`${threadId}\0${turn.providerTurnId}`);
		this.#activeTurn = undefined;
		return Object.freeze({ type: "turn.terminal", events: frozenEvents(events), outcome });
	}
}
