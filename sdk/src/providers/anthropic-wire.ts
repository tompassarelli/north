import {
	WireDecodeError,
	WireReductionError,
	wireMessageId,
	wireModelCallId,
	wireToolCallId,
	type WireCompletionEvidence,
	type WireArtifactSink,
	type WireEffort,
	type WireEventDraft,
	type WireEventWriter,
	type WireKnownEvent,
	type WireMessageId,
	type WireModelCallId,
	type WireModelSelection,
	type WireProviderJoinEvidence,
	type WireQueryRoute,
	type WireToolCallId,
	type WireUsageSnapshot,
	wireToolArgumentDigest,
} from "../wire";
import {
	isArtifactReadToolName,
	persistRetainedProviderMaterial,
	RetainedArtifactPersistenceError,
	retainedProviderMaterial,
	type RetainedProviderMaterial,
} from "./retained-artifact";

const MAX_PROVIDER_ID_BYTES = 1_024;
const MAX_CONTENT_BLOCKS = 256;
const MAX_MESSAGE_BYTES = 4_096;
const MAX_PREVIEW_BYTES = 2_048;
const MAX_TOOL_NAME_BYTES = 128;
const MAX_PREVIEW_DEPTH = 8;
const MAX_PREVIEW_NODES = 256;
const MAX_PREVIEW_ITEMS = 32;

const RESULT_SUBTYPES = new Set([
	"success",
	"error_during_execution",
	"error_max_turns",
	"error_max_budget_usd",
	"error_max_structured_output_retries",
]);

const CANCELLED_TERMINAL_REASONS = new Set([
	"aborted_streaming",
	"aborted_tools",
	"hook_stopped",
]);

const FAILED_TERMINAL_REASONS = new Set([
	"blocking_limit",
	"rapid_refill_breaker",
	"prompt_too_long",
	"image_error",
	"model_error",
	"api_error",
	"malformed_tool_use_exhausted",
	"stop_hook_prevented",
	"max_turns",
	"budget_exhausted",
	"structured_output_retry_exhausted",
	"tool_deferred_unavailable",
	"turn_setup_failed",
]);

const IGNORED_MESSAGE_TYPES = new Set([
	"active_goal",
	"auth_status",
	"commands_changed",
	"control_request",
	"control_request_progress",
	"conversation_reset",
	"elicitation_complete",
	"files_persisted",
	"hook_progress",
	"hook_response",
	"hook_started",
	"informational",
	"local_command_output",
	"memory_recall",
	"model_refusal_fallback",
	"model_refusal_no_fallback",
	"notification",
	"permission_denial",
	"plugin_install",
	"prompt_suggestion",
	"rate_limit_event",
	"session_state_changed",
	"stream_event",
	"tool_use_summary",
	"worker_shutting_down",
]);

const IGNORED_SYSTEM_SUBTYPES = new Set([
	"init",
	"mirror_error",
	"status",
]);

type RecordValue = Record<string, unknown>;

interface ActiveModelCall {
	id: WireModelCallId;
	issue?: "provider_error" | "provider_cancelled";
}

interface AdmittedTool {
	id: WireToolCallId;
	rawName: string;
	status: "pending" | "terminal";
}

interface BackgroundTask {
	id: WireToolCallId;
	status: "pending" | "terminal";
}

interface ParsedToolAdmission {
	rawId: string;
	rawName: string;
	wireId: WireToolCallId;
}

interface ParsedToolTerminal {
	rawId: string;
	wireId: WireToolCallId;
	status: "succeeded" | "failed" | "cancelled";
	preview?: string;
	result?: unknown;
	kind?: string;
	label?: string;
	errorCode?: string;
	retainResult?: boolean;
}

export type AnthropicWireTurnStatus = "succeeded" | "failed" | "cancelled";

export interface AnthropicWireTurnOutcome {
	readonly status: AnthropicWireTurnStatus;
	readonly modelCallId: WireModelCallId;
	readonly usage: WireUsageSnapshot;
	readonly errorCode?: string;
}

export interface AnthropicWireAcceptResult {
	readonly events: readonly WireKnownEvent[];
	readonly turnOutcome?: AnthropicWireTurnOutcome;
}

export interface AnthropicWireEventEvidence {
	readonly providerJoin?: WireProviderJoinEvidence;
}

interface PreviewBudget {
	nodes: number;
}

function asRecord(value: unknown): RecordValue | undefined {
	if (typeof value !== "object" || value === null || Array.isArray(value)) return undefined;
	return value as RecordValue;
}

function utf8Length(value: string): number {
	return new TextEncoder().encode(value).byteLength;
}

function boundedText(value: string, maxBytes: number): string {
	let output = "";
	let bytes = 0;
	for (let index = 0; index < value.length;) {
		const code = value.codePointAt(index);
		if (code === undefined) break;
		const width = code > 0xffff ? 2 : 1;
		let next: string;
		if (code === 0x0d) {
			next = "\n";
			if (value.charCodeAt(index + width) === 0x0a) index += 1;
		} else if (code === 0x09) {
			next = "  ";
		} else if ((code >= 0 && code <= 0x08) || (code >= 0x0b && code <= 0x1f)
			|| (code >= 0x7f && code <= 0x9f)) {
			next = "�";
		} else {
			next = String.fromCodePoint(code);
		}
		const nextBytes = utf8Length(next);
		if (bytes + nextBytes > maxBytes - 3) return `${output}…`;
		output += next;
		bytes += nextBytes;
		index += width;
	}
	return output;
}

function previewValue(value: unknown, budget: PreviewBudget, depth: number): unknown {
	budget.nodes += 1;
	if (budget.nodes > MAX_PREVIEW_NODES || depth > MAX_PREVIEW_DEPTH) return "[truncated]";
	if (value === null || typeof value === "boolean") return value;
	if (typeof value === "string") return boundedText(value, MAX_PREVIEW_BYTES);
	if (typeof value === "number") return Number.isFinite(value) ? value : "[non-finite number]";
	if (typeof value === "bigint") return boundedText(value.toString(), MAX_PREVIEW_BYTES);
	if (typeof value !== "object") return `[${typeof value}]`;
	if (Array.isArray(value)) {
		const result: unknown[] = [];
		for (let index = 0; index < value.length && index < MAX_PREVIEW_ITEMS; index += 1) {
			result.push(previewValue(value[index], budget, depth + 1));
		}
		if (value.length > MAX_PREVIEW_ITEMS) result.push("[truncated]");
		return result;
	}
	const source = value as RecordValue;
	const result: RecordValue = {};
	let count = 0;
	for (const key in source) {
		if (!Object.hasOwn(source, key)) continue;
		if (count >= MAX_PREVIEW_ITEMS) {
			result["[truncated]"] = true;
			break;
		}
		result[boundedText(key, 128)] = previewValue(source[key], budget, depth + 1);
		count += 1;
	}
	return result;
}

function boundedPreview(value: unknown): string | undefined {
	if (value === undefined) return undefined;
	try {
		const encoded = JSON.stringify(previewValue(value, { nodes: 0 }, 0));
		return encoded === undefined ? "[unavailable]" : boundedText(encoded, MAX_PREVIEW_BYTES);
	} catch {
		return "[unavailable]";
	}
}

function stableErrorCode(subtype: string, terminalReason: string | undefined): string {
	if (CANCELLED_TERMINAL_REASONS.has(terminalReason ?? "")) return "provider_cancelled";
	if (terminalReason === "prompt_too_long") return "provider_context_limit";
	if (terminalReason === "budget_exhausted" || subtype === "error_max_budget_usd") {
		return "provider_budget_exhausted";
	}
	if (terminalReason === "max_turns" || subtype === "error_max_turns") return "provider_max_turns";
	if (terminalReason === "structured_output_retry_exhausted"
		|| subtype === "error_max_structured_output_retries") {
		return "provider_structured_output_retries_exhausted";
	}
	return "provider_error";
}

export class AnthropicWireNormalizer {
	#writer: WireEventWriter;
	#route: WireQueryRoute;
	#artifacts?: WireArtifactSink;
	#model: WireModelSelection;
	#effort?: WireEffort;
	#activeCall?: ActiveModelCall;
	#tools = new Map<string, AdmittedTool>();
	#backgroundTasks = new Map<string, BackgroundTask>();
	#turnOpen = true;
	#semanticEvents = 0;

	constructor(writer: WireEventWriter, route: WireQueryRoute, artifacts?: WireArtifactSink) {
		const snapshot = writer.snapshot();
		if (!snapshot) {
			throw new WireReductionError(
				"state_violation",
				"anthropic wire normalization requires a started run",
				{ runId: writer.runId },
			);
		}
		if (snapshot.lifecycle !== "running" && snapshot.lifecycle !== "waiting") {
			throw new WireReductionError(
				"state_violation",
				"anthropic wire normalization requires an open run",
				{ runId: writer.runId },
			);
		}
		if (route.model.provider !== "anthropic") {
			throw new TypeError("anthropic wire route must select the anthropic provider");
		}
		if (!Number.isSafeInteger(route.attempt) || route.attempt <= 0) {
			throw new TypeError("anthropic wire route attempt must be a positive safe integer");
		}
		if (route.contextWindow !== undefined
			&& (!Number.isSafeInteger(route.contextWindow) || route.contextWindow <= 0)) {
			throw new TypeError("anthropic wire route contextWindow must be a positive safe integer");
		}
		this.#writer = writer;
		this.#route = Object.freeze({ ...route, model: Object.freeze({ ...route.model }) });
		this.#artifacts = artifacts;
		this.#model = this.#route.model;
		this.#effort = this.#route.effort;
	}

	setModel(selection: WireModelSelection): void {
		if (selection.provider !== "anthropic") {
			throw new TypeError("anthropic wire normalizer cannot select another provider");
		}
		this.#model = Object.freeze({ ...selection });
	}

	setEffort(effort: WireEffort | undefined): void {
		this.#effort = effort;
	}

	beginNextTurn(): void {
		const snapshot = this.#writer.snapshot();
		if (!snapshot || (snapshot.lifecycle !== "running" && snapshot.lifecycle !== "waiting")) {
			return this.#stateViolation("anthropic continuation requires an open run");
		}
		if (this.#turnOpen || this.#activeCall !== undefined) {
			return this.#stateViolation("anthropic continuation requires a completed prior turn");
		}
		this.#turnOpen = true;
	}

	accept(message: unknown, evidence: AnthropicWireEventEvidence = {}): AnthropicWireAcceptResult {
		const source = asRecord(message);
		if (!source) return this.#malformed("anthropic provider event must be an object");
		if (typeof source.type !== "string" || source.type.length === 0) {
			return this.#malformed("anthropic provider event type is missing");
		}
		switch (source.type) {
			case "assistant":
				return this.#acceptAssistant(source);
			case "user":
				return this.#acceptUser(source);
			case "tool_progress":
				return this.#acceptToolProgress(source);
			case "result":
				return this.#acceptResult(source, evidence);
			case "system":
				return this.#acceptSystem(source);
			default:
				if (IGNORED_MESSAGE_TYPES.has(source.type)) return Object.freeze({ events: Object.freeze([]) });
				throw new WireDecodeError(
					"unsupported_event_kind",
					"anthropic provider event type is unsupported",
					{ runId: this.#writer.runId },
				);
		}
	}

	settleAbrupt(status: "failed" | "cancelled"): AnthropicWireAcceptResult {
		const errorCode = status === "cancelled" ? "provider_cancelled" : "provider_error";
		const drafts: WireEventDraft[] = [];
		for (const tool of this.#tools.values()) {
			if (tool.status !== "pending") continue;
			drafts.push({
				kind: "tool.terminal",
				toolCallId: tool.id,
				status: status === "cancelled" ? "cancelled" : "synthetic_failure",
				origin: "north",
				errorCode,
			});
		}
		for (const task of this.#backgroundTasks.values()) {
			if (task.status !== "pending") continue;
			drafts.push({
				kind: "tool.terminal",
				toolCallId: task.id,
				status: status === "cancelled" ? "cancelled" : "synthetic_failure",
				origin: "north",
				errorCode,
			});
		}
		const activeCall = this.#activeCall;
		if (activeCall) {
			const snapshot = this.#writer.snapshot();
			if (!snapshot) return this.#stateViolation("anthropic abrupt settlement preceded run start");
			drafts.push({
				kind: "model-call.completed",
				modelCallId: activeCall.id,
				status,
				origin: "north",
				usage: snapshot.usage,
				usageCoverage: "unavailable",
				errorCode,
				evidence: { failure: { detail: errorCode } },
			});
		}
		const events = this.#writer.appendAll(drafts);
		for (const tool of this.#tools.values()) {
			if (tool.status === "pending") tool.status = "terminal";
		}
		for (const task of this.#backgroundTasks.values()) {
			if (task.status === "pending") task.status = "terminal";
		}
		this.#activeCall = undefined;
		this.#turnOpen = false;
		this.#semanticEvents += 1;
		if (!activeCall) return Object.freeze({ events });
		const outcome: AnthropicWireTurnOutcome = Object.freeze({
			status,
			modelCallId: activeCall.id,
			usage: this.#writer.snapshot()!.usage,
			errorCode,
		});
		return Object.freeze({ events, turnOutcome: outcome });
	}

	#acceptAssistant(source: RecordValue): AnthropicWireAcceptResult {
		const rawUuid = this.#providerId(source.uuid, "anthropic assistant uuid is malformed");
		const message = asRecord(source.message);
		if (!message || message.role !== "assistant" || !Array.isArray(message.content)
			|| message.content.length > MAX_CONTENT_BLOCKS) {
			return this.#malformed("anthropic assistant message is malformed");
		}
		if (source.error !== undefined && typeof source.error !== "string") {
			return this.#malformed("anthropic assistant error marker is malformed");
		}
		if (source.aborted !== undefined && source.aborted !== true) {
			return this.#malformed("anthropic assistant abort marker is malformed");
		}
		const parentTool = this.#parentTool(source.parent_tool_use_id, true);
		const startingCall = this.#activeCall === undefined;
		const modelCallId = this.#activeCall?.id
			?? this.#modelCallId(`assistant:${rawUuid}`);
		const messageId = this.#messageId(`assistant:${rawUuid}`);
		const drafts: WireEventDraft[] = [];
		if (startingCall) drafts.push(this.#modelCallStarted(modelCallId));
		drafts.push({
			kind: "message.recorded",
			messageId,
			stage: "started",
			role: "assistant",
			modelCallId,
			...(parentTool === undefined ? {} : { parentToolCallId: parentTool.id }),
		});

		const admissions: ParsedToolAdmission[] = [];
		const terminals: ParsedToolTerminal[] = [];
		const localTools = new Map<string, ParsedToolAdmission>();
		const locallyTerminal = new Set<string>();
		for (const rawBlock of message.content) {
			const block = asRecord(rawBlock);
			if (!block || typeof block.type !== "string" || block.type.length === 0) {
				return this.#malformed("anthropic assistant content block is malformed");
			}
			if (block.type === "text") {
				if (typeof block.text !== "string") {
					return this.#malformed("anthropic assistant text block is malformed");
				}
				drafts.push({
					kind: "message.recorded",
					messageId,
					stage: "delta",
					role: "assistant",
					content: boundedText(block.text, MAX_MESSAGE_BYTES),
					modelCallId,
					...(parentTool === undefined ? {} : { parentToolCallId: parentTool.id }),
				});
				continue;
			}
			if (block.type === "thinking" || block.type === "redacted_thinking") continue;
			if (block.type === "tool_use" || block.type === "server_tool_use"
				|| block.type.endsWith("_tool_use")) {
				const rawId = this.#providerId(block.id, "anthropic tool admission id is malformed");
				const rawName = this.#providerId(block.name, "anthropic tool admission name is malformed");
				if (this.#tools.has(rawId) || localTools.has(rawId)) {
					return this.#stateViolation("anthropic tool admission is duplicated");
				}
				const wireId = this.#toolCallId(rawId);
				const argumentDigest = block.input === undefined
					? undefined : wireToolArgumentDigest(block.input);
				const admission = { rawId, rawName, wireId };
				admissions.push(admission);
				localTools.set(rawId, admission);
				drafts.push({
					kind: "tool.admitted",
					toolCallId: wireId,
					name: boundedText(rawName, MAX_TOOL_NAME_BYTES),
					messageId,
					modelCallId,
					...(parentTool === undefined ? {} : { parentToolCallId: parentTool.id }),
					schema: { status: "unavailable", reason: "provider event omitted schema provenance" },
					...(argumentDigest === undefined ? {} : { argumentDigest }),
					...(block.input === undefined ? {} : { argumentPreview: boundedPreview(block.input) }),
				});
				continue;
			}
			if (block.type.endsWith("_tool_result")) {
				const rawId = this.#providerId(block.tool_use_id, "anthropic tool terminal id is malformed");
				const known = this.#tools.get(rawId);
				const local = localTools.get(rawId);
				if ((!known && !local) || known?.status === "terminal" || locallyTerminal.has(rawId)) {
					return this.#stateViolation("anthropic tool terminal has no open admission");
				}
				if (block.is_error !== undefined && typeof block.is_error !== "boolean") {
					return this.#malformed("anthropic tool terminal status is malformed");
				}
				const terminal: ParsedToolTerminal = {
					rawId,
					wireId: known?.id ?? local!.wireId,
					status: block.is_error === true ? "failed" : "succeeded",
					preview: boundedPreview(block.content),
					...(block.content === undefined ? {} : { result: block.content }),
					retainResult: !isArtifactReadToolName(
						"anthropic",
						known?.rawName ?? local!.rawName,
					),
				};
				terminals.push(terminal);
				locallyTerminal.add(rawId);
				drafts.push(...this.#toolTerminalDrafts(terminal));
			}
		}

		drafts.push({
			kind: "message.recorded",
			messageId,
			stage: "completed",
			role: "assistant",
			modelCallId,
			...(parentTool === undefined ? {} : { parentToolCallId: parentTool.id }),
		});
		const events = this.#writer.appendAll(drafts);
		if (startingCall) this.#activeCall = { id: modelCallId };
		if (source.error !== undefined) this.#activeCall!.issue = "provider_error";
		if (source.aborted === true) this.#activeCall!.issue = "provider_cancelled";
		for (const admission of admissions) {
			this.#tools.set(admission.rawId, {
				id: admission.wireId,
				rawName: admission.rawName,
				status: locallyTerminal.has(admission.rawId) ? "terminal" : "pending",
			});
		}
		for (const terminal of terminals) {
			const tool = this.#tools.get(terminal.rawId);
			if (tool) tool.status = "terminal";
		}
		this.#markTurnActivity();
		return Object.freeze({ events });
	}

	#acceptUser(source: RecordValue): AnthropicWireAcceptResult {
		const message = asRecord(source.message);
		if (!message || message.role !== "user") {
			return this.#malformed("anthropic user message is malformed");
		}
		const rawUuid = source.uuid === undefined
			? `event:${this.#semanticEvents}`
			: this.#providerId(source.uuid, "anthropic user uuid is malformed");
		const blocks = typeof message.content === "string" ? [
			{ type: "text", text: message.content },
		] : message.content;
		if (!Array.isArray(blocks) || blocks.length > MAX_CONTENT_BLOCKS) {
			return this.#malformed("anthropic user content is malformed");
		}
		const parentTool = this.#parentTool(source.parent_tool_use_id, false);
		const drafts: WireEventDraft[] = [];
		const terminals: ParsedToolTerminal[] = [];
		const terminalIds = new Set<string>();
		const textParts: string[] = [];
		let blockIndex = 0;
		for (const rawBlock of blocks) {
			const block = asRecord(rawBlock);
			if (!block || typeof block.type !== "string" || block.type.length === 0) {
				return this.#malformed("anthropic user content block is malformed");
			}
			if (block.type === "text") {
				if (typeof block.text !== "string") {
					return this.#malformed("anthropic user text block is malformed");
				}
				textParts.push(block.text);
				continue;
			}
			if (block.type !== "tool_result") continue;
			const rawId = this.#providerId(block.tool_use_id, "anthropic tool result id is malformed");
			const tool = this.#tools.get(rawId);
			if (!tool || tool.status !== "pending" || terminalIds.has(rawId)) {
				return this.#stateViolation("anthropic tool result has no open admission");
			}
			if (block.is_error !== undefined && typeof block.is_error !== "boolean") {
				return this.#malformed("anthropic tool result status is malformed");
			}
			const terminal: ParsedToolTerminal = {
				rawId,
				wireId: tool.id,
				status: block.is_error === true ? "failed" : "succeeded",
				preview: boundedPreview(block.content),
				...(block.content === undefined ? {} : { result: block.content }),
				retainResult: !isArtifactReadToolName("anthropic", tool.rawName),
			};
			terminals.push(terminal);
			terminalIds.add(rawId);
			const messageId = this.#messageId(`user:${rawUuid}:tool:${blockIndex}`);
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "started",
				role: "tool",
				...(terminal.preview === undefined ? {} : { content: terminal.preview }),
				parentToolCallId: tool.id,
			});
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "completed",
				role: "tool",
				parentToolCallId: tool.id,
			});
			drafts.push(...this.#toolTerminalDrafts(terminal));
			blockIndex += 1;
		}
		if (textParts.length > 0) {
			const messageId = this.#messageId(`user:${rawUuid}:text`);
			const text = boundedText(textParts.join("\n"), MAX_MESSAGE_BYTES);
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "started",
				role: "user",
				content: text,
				...(parentTool === undefined ? {} : { parentToolCallId: parentTool.id }),
			});
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "completed",
				role: "user",
				...(parentTool === undefined ? {} : { parentToolCallId: parentTool.id }),
			});
		}
		const events = this.#writer.appendAll(drafts);
		for (const terminal of terminals) this.#tools.get(terminal.rawId)!.status = "terminal";
		this.#markTurnActivity();
		return Object.freeze({ events });
	}

	#acceptToolProgress(source: RecordValue): AnthropicWireAcceptResult {
		const rawId = this.#providerId(source.tool_use_id, "anthropic tool progress id is malformed");
		const rawName = this.#providerId(source.tool_name, "anthropic tool progress name is malformed");
		const tool = this.#tools.get(rawId);
		if (!tool || tool.status !== "pending") {
			return this.#stateViolation("anthropic tool progress has no open admission");
		}
		if (tool.rawName !== rawName) {
			return this.#stateViolation("anthropic tool progress changed tool identity");
		}
		if (typeof source.elapsed_time_seconds !== "number" || !Number.isFinite(source.elapsed_time_seconds)
			|| source.elapsed_time_seconds < 0) {
			return this.#malformed("anthropic tool progress elapsed time is malformed");
		}
		const elapsedMs = Math.round(source.elapsed_time_seconds * 1_000);
		if (!Number.isSafeInteger(elapsedMs)) {
			return this.#malformed("anthropic tool progress elapsed time is out of range");
		}
		const events = this.#writer.appendAll([{
			kind: "tool.progress",
			toolCallId: tool.id,
			progress: { elapsedMs },
		}]);
		this.#markTurnActivity();
		return Object.freeze({ events });
	}

	#acceptSystem(source: RecordValue): AnthropicWireAcceptResult {
		if (typeof source.subtype !== "string" || source.subtype.length === 0) {
			return this.#malformed("anthropic system event subtype is missing");
		}
		switch (source.subtype) {
			case "compact_boundary": {
				const snapshot = this.#writer.snapshot();
				if (!snapshot) return this.#stateViolation("anthropic compaction preceded run start");
				const events = this.#writer.appendAll([{
					kind: "run.progress",
					lifecycle: "running",
					progress: { compactions: snapshot.compactions + 1 },
				}]);
				return Object.freeze({ events });
			}
			case "task_started":
				return this.#acceptTaskStarted(source);
			case "task_progress":
				return this.#acceptTaskProgress(source);
			case "task_updated":
				return this.#acceptTaskUpdated(source);
			case "task_notification":
				return this.#acceptTaskNotification(source);
			default:
				if (IGNORED_SYSTEM_SUBTYPES.has(source.subtype)) {
					return Object.freeze({ events: Object.freeze([]) });
				}
				throw new WireDecodeError(
					"unsupported_event_kind",
					"anthropic system event subtype is unsupported",
					{ runId: this.#writer.runId },
				);
		}
	}

	#acceptTaskStarted(source: RecordValue): AnthropicWireAcceptResult {
		const rawTaskId = this.#providerId(source.task_id, "anthropic background task id is malformed");
		if (source.description !== undefined && typeof source.description !== "string") {
			return this.#malformed("anthropic background task description is malformed");
		}
		const rawParentId = source.tool_use_id === undefined
			? undefined
			: this.#providerId(source.tool_use_id, "anthropic background task parent is malformed");
		if (this.#backgroundTasks.has(rawTaskId)) return Object.freeze({ events: Object.freeze([]) });
		const taskId = this.#toolCallId(`background-task:${rawTaskId}`);
		const parent = rawParentId === undefined
			? undefined
			: this.#tools.get(rawParentId);
		if (rawParentId !== undefined && parent?.status !== "pending") {
			return this.#stateViolation(
				"anthropic background task has no known pending parent tool admission",
			);
		}
		const pendingParent = parent?.status === "pending" ? parent : undefined;
		const events = this.#writer.appendAll([{
			kind: "tool.admitted",
			toolCallId: taskId,
			name: "background-task",
			...(pendingParent === undefined ? {} : { parentToolCallId: pendingParent.id }),
			schema: { status: "unavailable", reason: "provider background task has no callable schema" },
			...(typeof source.description === "string"
				? { argumentPreview: boundedText(source.description, MAX_PREVIEW_BYTES) } : {}),
		}]);
		this.#backgroundTasks.set(rawTaskId, { id: taskId, status: "pending" });
		return Object.freeze({ events });
	}

	#acceptTaskProgress(source: RecordValue): AnthropicWireAcceptResult {
		const rawTaskId = this.#providerId(source.task_id, "anthropic background task id is malformed");
		const task = this.#backgroundTasks.get(rawTaskId);
		if (!task || task.status !== "pending") return Object.freeze({ events: Object.freeze([]) });
		if (source.description !== undefined && typeof source.description !== "string") {
			return this.#malformed("anthropic background task progress is malformed");
		}
		const progress: Record<string, string | number> = { state: "running" };
		if (typeof source.description === "string") {
			progress.description = boundedText(source.description, MAX_MESSAGE_BYTES);
		}
		const usage = source.usage === undefined ? undefined : asRecord(source.usage);
		if (source.usage !== undefined && !usage) {
			return this.#malformed("anthropic background task usage is malformed");
		}
		if (usage) {
			progress.totalTokens = this.#count(
				usage.total_tokens,
				"anthropic background task token usage is malformed",
			);
			progress.toolUses = this.#count(
				usage.tool_uses,
				"anthropic background task tool usage is malformed",
			);
			progress.durationMs = this.#count(
				usage.duration_ms,
				"anthropic background task duration is malformed",
			);
		}
		const events = this.#writer.appendAll([{
			kind: "tool.progress",
			toolCallId: task.id,
			progress,
		}]);
		return Object.freeze({ events });
	}

	#acceptTaskUpdated(source: RecordValue): AnthropicWireAcceptResult {
		const rawTaskId = this.#providerId(source.task_id, "anthropic background task id is malformed");
		const task = this.#backgroundTasks.get(rawTaskId);
		if (!task || task.status !== "pending") return Object.freeze({ events: Object.freeze([]) });
		const patch = asRecord(source.patch);
		if (!patch) return this.#malformed("anthropic background task update is malformed");
		const status = patch.status;
		if (status !== undefined && typeof status !== "string") {
			return this.#malformed("anthropic background task status is malformed");
		}
		if (status === "completed" || status === "failed" || status === "killed") {
			return this.#completeBackgroundTask(
				rawTaskId,
				task,
				status === "completed" ? "succeeded" : status === "failed" ? "failed" : "cancelled",
				patch.error,
			);
		}
		if (status !== undefined && status !== "pending" && status !== "running" && status !== "paused") {
			return this.#malformed("anthropic background task status is unsupported");
		}
		if (patch.description !== undefined && typeof patch.description !== "string") {
			return this.#malformed("anthropic background task update description is malformed");
		}
		if (patch.is_backgrounded !== undefined && typeof patch.is_backgrounded !== "boolean") {
			return this.#malformed("anthropic background task mode is malformed");
		}
		const progress: Record<string, string | boolean> = {
			state: status ?? "running",
			...(typeof patch.description === "string"
				? { description: boundedText(patch.description, MAX_MESSAGE_BYTES) } : {}),
			...(typeof patch.is_backgrounded === "boolean"
				? { backgrounded: patch.is_backgrounded } : {}),
		};
		const events = this.#writer.appendAll([{
			kind: "tool.progress",
			toolCallId: task.id,
			progress,
		}]);
		return Object.freeze({ events });
	}

	#acceptTaskNotification(source: RecordValue): AnthropicWireAcceptResult {
		const rawTaskId = this.#providerId(source.task_id, "anthropic background task id is malformed");
		const task = this.#backgroundTasks.get(rawTaskId);
		if (!task || task.status !== "pending") return Object.freeze({ events: Object.freeze([]) });
		if (source.status !== "completed" && source.status !== "failed" && source.status !== "stopped") {
			return this.#malformed("anthropic background task terminal status is malformed");
		}
		return this.#completeBackgroundTask(
			rawTaskId,
			task,
			source.status === "completed" ? "succeeded" : source.status === "failed" ? "failed" : "cancelled",
			source.summary,
		);
	}

	#completeBackgroundTask(
		rawTaskId: string,
		task: BackgroundTask,
		status: "succeeded" | "failed" | "cancelled",
		result: unknown,
	): AnthropicWireAcceptResult {
		if (result !== undefined && typeof result !== "string") {
			return this.#malformed("anthropic background task terminal detail is malformed");
		}
		const events = this.#writer.appendAll(this.#toolTerminalDrafts({
			rawId: rawTaskId,
			wireId: task.id,
			status,
			...(typeof result === "string" ? {
				preview: boundedText(result, MAX_PREVIEW_BYTES),
				result,
			} : {}),
			kind: "background-task-result",
			label: "background task result",
			...(status === "failed" ? { errorCode: "background_task_failed" } : {}),
			...(status === "cancelled" ? { errorCode: "background_task_cancelled" } : {}),
		}));
		task.status = "terminal";
		this.#backgroundTasks.set(rawTaskId, task);
		return Object.freeze({ events });
	}

	#acceptResult(
		source: RecordValue,
		eventEvidence: AnthropicWireEventEvidence,
	): AnthropicWireAcceptResult {
		if (!this.#turnOpen) return this.#stateViolation("anthropic result terminal is duplicated");
		const rawUuid = this.#providerId(source.uuid, "anthropic result uuid is malformed");
		if (typeof source.subtype !== "string" || !RESULT_SUBTYPES.has(source.subtype)) {
			throw new WireDecodeError(
				"unsupported_event_kind",
				"anthropic result subtype is unsupported",
				{ runId: this.#writer.runId },
			);
		}
		if (source.terminal_reason !== undefined && typeof source.terminal_reason !== "string") {
			return this.#malformed("anthropic result terminal reason is malformed");
		}
		if (source.is_error !== undefined && typeof source.is_error !== "boolean") {
			return this.#malformed("anthropic result error marker is malformed");
		}
		const terminalReason = source.terminal_reason as string | undefined;
		const startingCall = this.#activeCall === undefined;
		const modelCallId = this.#activeCall?.id ?? this.#modelCallId(`result:${rawUuid}`);
		let status: AnthropicWireTurnStatus;
		if (CANCELLED_TERMINAL_REASONS.has(terminalReason ?? "")
			|| this.#activeCall?.issue === "provider_cancelled") {
			status = "cancelled";
		} else if (source.subtype !== "success" || source.is_error === true
			|| FAILED_TERMINAL_REASONS.has(terminalReason ?? "")
			|| this.#activeCall?.issue === "provider_error") {
			status = "failed";
		} else {
			status = "succeeded";
		}
		const errorCode = status === "succeeded"
			? undefined
			: stableErrorCode(source.subtype, terminalReason);
		const pending = [...this.#tools.entries()].filter(([, tool]) => tool.status === "pending");
		if (status === "succeeded" && pending.length > 0) {
			return this.#stateViolation("anthropic result succeeded with open tool admissions");
		}
		const usage = this.#usage(source.usage, startingCall);
		const assistantTurns = this.#count(
			source.num_turns,
			"anthropic result turn count is malformed",
		);
		const providerDurationMs = this.#count(
			source.duration_ms,
			"anthropic result duration is malformed",
		);
		const evidence: WireCompletionEvidence = Object.freeze({
			...(eventEvidence.providerJoin === undefined
				? {} : { providerJoin: eventEvidence.providerJoin }),
			turns: Object.freeze({
				unit: "assistant-turn",
				count: assistantTurns,
				comparable: true,
			}),
			providerDurationMs,
			...(errorCode === undefined ? {} : {
				failure: Object.freeze({ detail: errorCode }),
			}),
		});
		const drafts: WireEventDraft[] = [];
		if (startingCall) drafts.push(this.#modelCallStarted(modelCallId));
		if (startingCall && typeof source.result === "string" && source.result.length > 0) {
			const messageId = this.#messageId(`result:${rawUuid}`);
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "started",
				role: "assistant",
				modelCallId,
			});
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "delta",
				role: "assistant",
				content: boundedText(source.result, MAX_MESSAGE_BYTES),
				modelCallId,
			});
			drafts.push({
				kind: "message.recorded",
				messageId,
				stage: "completed",
				role: "assistant",
				modelCallId,
			});
		}
		for (const [, tool] of pending) {
			drafts.push({
				kind: "tool.terminal",
				toolCallId: tool.id,
				status: status === "cancelled" ? "cancelled" : "synthetic_failure",
				origin: "north",
				...(errorCode === undefined ? {} : { errorCode }),
			});
		}
		drafts.push({
			kind: "model-call.completed",
			modelCallId,
			status,
			origin: "provider",
			usage,
			usageCoverage: "exact",
			...(errorCode === undefined ? {} : { errorCode }),
			evidence,
		});
		const events = this.#writer.appendAll(drafts);
		for (const [, tool] of pending) tool.status = "terminal";
		const outcome: AnthropicWireTurnOutcome = Object.freeze({
			status,
			modelCallId,
			usage,
			...(errorCode === undefined ? {} : { errorCode }),
		});
		this.#activeCall = undefined;
		this.#turnOpen = false;
		this.#semanticEvents += 1;
		return Object.freeze({ events, turnOutcome: outcome });
	}

	#usage(value: unknown, startingCall: boolean): WireUsageSnapshot {
		const source = asRecord(value);
		if (!source) return this.#malformed("anthropic result usage is malformed");
		const inputTokens = this.#count(source.input_tokens, "anthropic input token usage is malformed");
		const outputTokens = this.#count(source.output_tokens, "anthropic output token usage is malformed");
		const cacheWriteTokens = this.#count(
			source.cache_creation_input_tokens,
			"anthropic cache-write token usage is malformed",
		);
		const cacheReadTokens = this.#count(
			source.cache_read_input_tokens,
			"anthropic cache-read token usage is malformed",
		);
		const snapshot = this.#writer.snapshot()!;
		const contextTokens = this.#safeSum(
			[inputTokens, cacheWriteTokens, cacheReadTokens],
			"anthropic context token usage is out of range",
		);
		if (this.#route.contextWindow !== undefined && contextTokens > this.#route.contextWindow) {
			return this.#malformed("anthropic context token usage exceeds the semantic route window");
		}
		const lifetime = snapshot.usage.lifetime;
		const modelCalls = this.#safeSum(
			[lifetime.modelCalls, startingCall ? 1 : 0],
			"anthropic model-call usage is out of range",
		);
		return Object.freeze({
			lifetime: Object.freeze({
				inputTokens: this.#safeSum(
					[lifetime.inputTokens, inputTokens],
					"anthropic input token usage is out of range",
				),
				outputTokens: this.#safeSum(
					[lifetime.outputTokens, outputTokens],
					"anthropic output token usage is out of range",
				),
				cacheReadTokens: this.#safeSum(
					[lifetime.cacheReadTokens, cacheReadTokens],
					"anthropic cache-read token usage is out of range",
				),
				cacheWriteTokens: this.#safeSum(
					[lifetime.cacheWriteTokens, cacheWriteTokens],
					"anthropic cache-write token usage is out of range",
				),
				reasoningTokens: lifetime.reasoningTokens,
				modelCalls,
			}),
			context: Object.freeze({
				tokens: contextTokens,
				...(this.#route.contextWindow === undefined ? {} : { window: this.#route.contextWindow }),
			}),
		});
	}

	#modelCallStarted(modelCallId: WireModelCallId): WireEventDraft {
		return {
			kind: "model-call.started",
			modelCallId,
			model: this.#model,
			...(this.#effort === undefined ? {} : { effort: this.#effort }),
			attempt: this.#route.attempt,
		};
	}

	#toolTerminalDrafts(terminal: ParsedToolTerminal): WireEventDraft[] {
		let material: RetainedProviderMaterial | undefined;
		if (terminal.result !== undefined && terminal.retainResult !== false && this.#artifacts) {
			try {
				material = retainedProviderMaterial({
					runId: this.#writer.runId,
					provider: "anthropic",
					kind: terminal.kind ?? "tool-result",
					identity: terminal.rawId,
					value: terminal.result,
					label: terminal.label ?? "tool result",
				});
				persistRetainedProviderMaterial(this.#artifacts, material);
			} catch (cause) {
				if (cause instanceof RetainedArtifactPersistenceError) {
					throw new WireReductionError(
						"state_violation",
						"anthropic tool result artifact could not be persisted",
						{ runId: this.#writer.runId },
						{ cause },
					);
				}
				throw new WireDecodeError(
					"malformed_event",
					"anthropic tool result is not retainable",
					{ runId: this.#writer.runId },
					{ cause },
				);
			}
		}
		const drafts: WireEventDraft[] = [];
		if (material) {
			drafts.push({
				kind: "artifact.published",
				artifactId: material.artifactId,
				mediaType: material.mediaType,
				bytes: material.bytes,
				digest: material.digest,
				label: material.label,
			});
		}
		drafts.push({
			kind: "tool.terminal",
			toolCallId: terminal.wireId,
			status: terminal.status,
			origin: "provider",
			...(terminal.preview === undefined ? {} : { resultPreview: terminal.preview }),
			...(material === undefined ? {} : {
				resultArtifactId: material.artifactId,
				resultArtifactDigest: material.digest,
			}),
			...(terminal.errorCode !== undefined
				? { errorCode: terminal.errorCode }
				: terminal.status === "failed" ? { errorCode: "tool_failed" } : {}),
		});
		return drafts;
	}

	#parentTool(value: unknown, requirePending: boolean): AdmittedTool | undefined {
		if (value === undefined || value === null) return undefined;
		const rawId = this.#providerId(value, "anthropic parent tool id is malformed");
		const parent = this.#tools.get(rawId);
		if (!parent || (requirePending && parent.status !== "pending")) {
			return this.#stateViolation("anthropic message has no known parent tool admission");
		}
		return parent;
	}

	#providerId(value: unknown, message: string): string {
		if (typeof value !== "string" || value.length === 0 || utf8Length(value) > MAX_PROVIDER_ID_BYTES) {
			return this.#malformed(message);
		}
		return value;
	}

	#digest(domain: string, raw: string): string {
		const hasher = new Bun.CryptoHasher("sha256");
		hasher.update(`${this.#writer.runId}\0${domain}\0${raw}`);
		return hasher.digest("hex");
	}

	#messageId(raw: string): WireMessageId {
		return wireMessageId(`message:anthropic:${this.#digest("message", raw)}`);
	}

	#modelCallId(raw: string): WireModelCallId {
		return wireModelCallId(`model-call:anthropic:${this.#digest("model-call", raw)}`);
	}

	#toolCallId(raw: string): WireToolCallId {
		return wireToolCallId(`tool-call:anthropic:${this.#digest("tool-call", raw)}`);
	}

	#count(value: unknown, message: string): number {
		if (!Number.isSafeInteger(value) || (value as number) < 0) return this.#malformed(message);
		return value as number;
	}

	#safeSum(values: readonly number[], message: string): number {
		let total = 0;
		for (const value of values) {
			total += value;
			if (!Number.isSafeInteger(total)) return this.#malformed(message);
		}
		return total;
	}

	#markTurnActivity(): void {
		this.#turnOpen = true;
		this.#semanticEvents += 1;
	}

	#malformed(message: string): never {
		throw new WireDecodeError("malformed_event", message, { runId: this.#writer.runId });
	}

	#stateViolation(message: string): never {
		throw new WireReductionError("state_violation", message, { runId: this.#writer.runId });
	}
}
