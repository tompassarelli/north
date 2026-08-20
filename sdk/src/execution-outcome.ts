import type { DeliveryAssessment, DeliveryProof } from "./delivery-verification";
import type {
	WireAbortEvidence,
	WireOuterAbortActivityKind,
	WireProviderAbortActivityKind,
	WireTerminalLifecycle,
	WireTerminationReason,
} from "./wire/events";
import type { WireModelCallSnapshot, WireRunSnapshot } from "./wire/reducer";
import type { WatchdogAbortEvidence } from "./watchdog";

export type DeliveryOutcome = "unverified" | "reported" | "blocked";

export interface ExecutionTerminal {
	processOutcome: string;
	deliveryOutcome: DeliveryOutcome;
	deliveryReason: string;
	deliveryProof?: DeliveryProof;
}

export const EMPTY_RESULT_OUTCOME = "ran_empty";
export const PROVIDER_PROCESS_DEATH_OUTCOME = "died";
export const RUN_TOKEN_BUDGET_LIMITED_OUTCOME = "token_budget_limited";
export const PROVIDER_ERROR_DETAIL_MAX_LEN = 1200;
export const NO_PROVIDER_TERMINAL_DETAIL =
	"provider stream closed without a model-call terminal";
export const EMPTY_PROVIDER_ERROR_DETAIL =
	"model-call terminal carried no diagnostic evidence";
export const DEADLINE_EXCEEDED_DETAIL_MAX_LEN = 4_096;

export interface WireTerminalDecision {
	lifecycle: WireTerminalLifecycle;
	reason: WireTerminationReason;
	abort?: WireAbortEvidence;
}

function outerAbortActivityKind(kind: string): WireOuterAbortActivityKind {
	if (kind.startsWith("wire.message.")) return "message";
	if (kind.startsWith("wire.model-call.")) return "model";
	if (kind.startsWith("wire.tool.")) return "tool";
	if (kind === "wire.artifact.published") return "artifact";
	if (kind === "wire.run.compacted") return "compaction";
	return "activity";
}

function providerAbortActivityKind(kind: string): WireProviderAbortActivityKind {
	if (kind.includes(".mcp.") || kind.includes(".command.")) return "tool";
	if (kind.endsWith(".progress") || kind.endsWith(".diff") || kind.endsWith(".plan")
		|| kind.endsWith(".patch")) return "progress";
	if (kind.includes(".turn.")) return "turn";
	if (kind.includes(".item.")) return "item";
	if (kind.endsWith(".event.accepted")) return "event";
	return "activity";
}

/** Map North's process outcome onto one bounded, provider-neutral outer wire terminal. */
export function wireTerminalDecision(
	outcome: string,
	_detail: string | undefined,
	watchdogAbort: WatchdogAbortEvidence | undefined,
): WireTerminalDecision {
	const reason = (code: WireTerminationReason["code"]): WireTerminationReason => ({
		code,
		...(code === "completed" ? {} : { detail: code }),
	});
	if (outcome === "ran") return { lifecycle: "completed", reason: reason("completed") };
	if (outcome === "watchdog_aborted") {
		if (watchdogAbort === undefined) {
			throw new Error("watchdog_aborted requires authenticated inactivity evidence");
		}
		return {
			lifecycle: "cancelled",
			reason: reason("aborted"),
			abort: {
				requestedAt: new Date().toISOString(),
				source: "watchdog",
				reason: watchdogAbort.reason,
				watchdog: {
					silenceMs: watchdogAbort.silenceMs,
					...(watchdogAbort.lastOuter === undefined ? {} : {
						lastOuter: {
							origin: "outer" as const,
							kind: outerAbortActivityKind(watchdogAbort.lastOuter.kind),
							observedAt: watchdogAbort.lastOuter.observedAt,
						},
					}),
					...(watchdogAbort.lastProvider === undefined ? {} : {
						lastProvider: {
							origin: "provider" as const,
							kind: providerAbortActivityKind(watchdogAbort.lastProvider.kind),
							observedAt: watchdogAbort.lastProvider.observedAt,
						},
					}),
				},
			},
		};
	}
	if (outcome === "deadline_exceeded" || outcome === "session_hard_cap") {
		return { lifecycle: "failed", reason: reason("timed_out") };
	}
	if (outcome === RUN_TOKEN_BUDGET_LIMITED_OUTCOME) {
		return { lifecycle: "blocked", reason: reason("blocked") };
	}
	if (outcome === "provider_error" || outcome === "max_turns" || outcome === "capped") {
		return { lifecycle: "failed", reason: reason("provider_error") };
	}
	if (outcome === "died" || outcome === "stalled") {
		return { lifecycle: "failed", reason: reason("provider_process_died") };
	}
	if (outcome === "resource_envelope_exceeded") {
		return { lifecycle: "blocked", reason: reason("resource_denied") };
	}
	if (outcome.startsWith("blocked_") || outcome.startsWith("orchestrator_")
			|| outcome === "child_reconciliation_unavailable"
			|| outcome === "background_tasks_incomplete") {
		return { lifecycle: "blocked", reason: reason("blocked") };
	}
	return { lifecycle: "failed", reason: reason("synthetic_failure") };
}

function latestModelCallTerminal(snapshot: WireRunSnapshot): WireModelCallSnapshot | undefined {
	let latest: WireModelCallSnapshot | undefined;
	for (const modelCall of Object.values(snapshot.modelCalls)) {
		if (modelCall.status !== "running") latest = modelCall;
	}
	return latest;
}

function assistantOutputFor(
	snapshot: WireRunSnapshot,
	modelCall: WireModelCallSnapshot,
): string | undefined {
	let output: string | undefined;
	for (const message of Object.values(snapshot.messages)) {
		if (message.role !== "assistant" || message.stage !== "completed"
			|| message.modelCallId !== modelCall.id) continue;
		output = message.contents
			.filter((content): content is string => typeof content === "string")
			.join("");
	}
	return output;
}

/** True when the latest successful model call committed no assistant text. */
export function isEmptyResultTerminal(snapshot: WireRunSnapshot): boolean {
	const terminal = latestModelCallTerminal(snapshot);
	return terminal?.status === "succeeded"
		&& (assistantOutputFor(snapshot, terminal) ?? "").trim() === "";
}

function oneLine(value: string, maxLen: number): string {
	return value.replace(/\s+/g, " ").trim().slice(0, maxLen);
}

/** Render typed North-owned interrupt evidence from the latest model terminal. */
export function describeDeadlineExceededTerminal(
	snapshot: WireRunSnapshot,
): string | undefined {
	const evidence = latestModelCallTerminal(snapshot)?.evidence?.interrupt;
	if (evidence === undefined) return undefined;
	const rendered = JSON.stringify({
		reason: evidence.reason,
		deadlineMs: evidence.deadlineMs,
		inactivityThresholdMs: evidence.inactivityThresholdMs,
		lastActivityAgeMs: evidence.lastActivityAgeMs,
		...(evidence.openItemCount === undefined ? {} : { openItemCount: evidence.openItemCount }),
		...(evidence.openItem === undefined ? {} : { openItem: evidence.openItem }),
		eventCount: evidence.eventCount,
	});
	return rendered.length <= DEADLINE_EXCEEDED_DETAIL_MAX_LEN ? rendered : undefined;
}

/**
 * Render bounded provider-neutral failure evidence. Assistant output is never
 * diagnostic input and therefore cannot alter the machine reason.
 */
export function describeProviderErrorTerminal(
	snapshot: WireRunSnapshot,
	maxLen = PROVIDER_ERROR_DETAIL_MAX_LEN,
): string {
	const terminal = latestModelCallTerminal(snapshot);
	if (terminal === undefined) return NO_PROVIDER_TERMINAL_DETAIL;
	const evidence = terminal.evidence;
	const parts: string[] = [];
	if (terminal.status !== "succeeded") parts.push(`status=${terminal.status}`);
	if (terminal.errorCode !== undefined) parts.push(`code=${oneLine(terminal.errorCode, 128)}`);
	parts.push(`origin=${terminal.origin ?? "unknown"}`);
	if (evidence?.failure !== undefined) {
		parts.push(`failure=${oneLine(evidence.failure.detail, 800)}`);
		const landed = evidence.failure.landed;
		if (landed !== undefined) {
			const counts = [
				landed.completedTurns === undefined ? undefined
					: `${landed.completedTurns} completed turn(s)`,
				landed.toolItems === undefined ? undefined : `${landed.toolItems} tool item(s)`,
				landed.mcpCalls === undefined ? undefined : `${landed.mcpCalls} MCP call(s)`,
				landed.nativeCommands === undefined ? undefined
					: `${landed.nativeCommands} native command(s)`,
			].filter((value): value is string => value !== undefined);
			if (counts.length > 0) parts.push(`landed=[${counts.join(", ")}]`);
		}
	}
	if (evidence?.turns?.unit === "assistant-turn") {
		parts.push(`turns=${evidence.turns.count}`);
	} else if (evidence?.turns?.unit === "provider-turn") {
		parts.push(`turn_units=${evidence.turns.count}`);
	}
	if (evidence?.providerJoin?.sessionKey !== undefined) {
		parts.push(`provider_session=${evidence.providerJoin.sessionKey}`);
	}
	if (parts.length === 1 && terminal.status === "succeeded") {
		return EMPTY_PROVIDER_ERROR_DETAIL;
	}
	return `model-call terminal: ${parts.join(" ")}`.slice(0, maxLen);
}

const BLOCKED_REASON: Readonly<Record<string, string>> = {
	blocked_preflight: "execution_preflight_blocked",
	blocked_spend_guard: "spend_guard_budget_incomplete",
	ran_empty: "provider_terminal_empty_result",
	provider_error: "provider_terminal_error",
	deadline_exceeded: "north_turn_deadline_exceeded_after_inactivity",
	died: "provider_process_died",
	stalled: "provider_process_stalled",
	watchdog_aborted: "north_watchdog_execution_inactivity",
	session_hard_cap: "north_managed_session_hard_cap",
	token_budget_limited: "north_managed_run_token_budget_limited",
	max_turns: "provider_turn_cap",
	capped: "provider_cap",
	resource_envelope_exceeded: "resource_envelope_exceeded",
	provider_escalation_unsupported: "provider_escalation_unsupported",
	max_tier: "escalation_ladder_exhausted",
	orchestrator_children_incomplete: "orchestrator_children_live_at_terminal",
	orchestrator_child_obligation_unmet: "orchestrator_minimum_children_not_dispatched",
	child_reconciliation_unavailable: "orchestrator_child_reconciliation_unavailable",
	orchestrator_reduction_incomplete: "orchestrator_child_results_unreconciled",
	orchestrator_child_set_inconsistent: "orchestrator_child_relation_regressed",
};

export function classifyExecutionTerminal(
	processOutcome: string,
	delivery?: DeliveryAssessment,
): ExecutionTerminal {
	if (processOutcome === "ran") {
		if (delivery?.deliveryOutcome === "reported") {
			return {
				processOutcome,
				deliveryOutcome: delivery.deliveryOutcome,
				deliveryReason: delivery.deliveryReason,
				deliveryProof: delivery.proof,
			};
		}
		return {
			processOutcome,
			deliveryOutcome: delivery?.deliveryOutcome ?? "unverified",
			deliveryReason: delivery?.deliveryReason
				?? "provider_terminal_success_without_external_verification",
		};
	}
	return {
		processOutcome,
		deliveryOutcome: "blocked",
		deliveryReason: BLOCKED_REASON[processOutcome] ?? "execution_did_not_reach_success_terminal",
	};
}
