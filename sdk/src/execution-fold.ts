import { makeBgTracker, type BgEvent } from "./bgtasks";
import {
	describeDeadlineExceededTerminal,
	describeProviderErrorTerminal,
} from "./execution-outcome";
import { outerExecutionActivityKind } from "./providers/outer-activity";
import { foldProviderJoinEvidence } from "./providers/provider-join";
import {
	makeStruggleObserver,
	type StruggleObservation,
	type StrugglePolicy,
	type StruggleTrigger,
} from "./struggle";
import { normalizeUsage, type NormalizedTokenUsage } from "./usage";
import type {
	WireCompletionEvidence,
	WireEvent,
	WireProviderJoinEvidence,
	WireTurnEvidence,
} from "./wire/events";
import { WireReductionError } from "./wire/errors";
import type { WireEventId, WireToolCallId } from "./wire/ids";
import {
	reduceWireEvent,
	type WireModelCallSnapshot,
	type WireRunSnapshot,
} from "./wire/reducer";

export interface WireExecutionActivity {
	kind: string;
	eventId: WireEventId;
	at: string;
}

export interface WireToolActivity {
	admitted: number;
	progressed: number;
	terminal: number;
	succeeded: number;
	failed: number;
	cancelled: number;
	syntheticFailures: number;
	pending: number;
}

export interface ExecutionFoldSnapshot {
	run: WireRunSnapshot;
	lastCompletedAssistantOutput?: string;
	latestModelCallTerminal?: WireModelCallSnapshot;
	pendingBackgroundTasks: readonly WireToolCallId[];
	compactions: number;
	usage: NormalizedTokenUsage;
	completionEvidence: readonly WireCompletionEvidence[];
	providerJoin?: WireProviderJoinEvidence;
	turnEvidence: readonly WireTurnEvidence[];
	toolActivity: WireToolActivity;
	struggle: StruggleObservation;
	latestActivity?: WireExecutionActivity;
	activityCount: number;
	providerErrorDetail?: string;
	deadlineExceededDetail?: string;
}

export interface ExecutionFoldObservation {
	event: WireEvent;
	state: ExecutionFoldSnapshot;
	activityKind?: string;
	backgroundTask: BgEvent;
	struggleTrigger: StruggleTrigger | null;
	turnTerminal?: WireModelCallSnapshot;
}

export interface ExecutionFold {
	observe(event: WireEvent): ExecutionFoldObservation;
	snapshot(): ExecutionFoldSnapshot | undefined;
}

interface MutableToolActivity {
	admitted: number;
	progressed: number;
	terminal: number;
	succeeded: number;
	failed: number;
	cancelled: number;
	syntheticFailures: number;
}

function assistantOutput(snapshot: WireRunSnapshot, messageId: string): string {
	const message = snapshot.messages[messageId];
	if (message === undefined) return "";
	return message.contents
		.filter((content): content is string => typeof content === "string")
		.join("");
}

function frozenToolActivity(
	activity: MutableToolActivity,
	snapshot: WireRunSnapshot,
): WireToolActivity {
	return Object.freeze({
		...activity,
		pending: Object.values(snapshot.toolCalls)
			.filter((toolCall) => toolCall.status === "pending").length,
	});
}

export function makeExecutionFold(policy: StrugglePolicy): ExecutionFold {
	let run: WireRunSnapshot | undefined;
	let lastCompletedAssistantOutput: string | undefined;
	let latestModelCallTerminal: WireModelCallSnapshot | undefined;
	let latestActivity: WireExecutionActivity | undefined;
	let activityCount = 0;
	const eventIds = new Set<WireEventId>();
	const backgroundTasks = makeBgTracker();
	const struggle = makeStruggleObserver(policy);
	const completionEvidence: WireCompletionEvidence[] = [];
	const providerJoins: WireProviderJoinEvidence[] = [];
	const turnEvidence: WireTurnEvidence[] = [];
	const toolActivity: MutableToolActivity = {
		admitted: 0,
		progressed: 0,
		terminal: 0,
		succeeded: 0,
		failed: 0,
		cancelled: 0,
		syntheticFailures: 0,
	};

	function currentSnapshot(): ExecutionFoldSnapshot | undefined {
		if (run === undefined) return undefined;
		const foldedProviderJoin = foldProviderJoinEvidence(providerJoins);
		const completedModelCallCount = Object.values(run.modelCalls)
			.filter((modelCall) => modelCall.status !== "running").length;
		const providerJoin = foldedProviderJoin !== undefined
			&& providerJoins.length < completedModelCallCount
			&& foldedProviderJoin.coverage === "exact"
			? Object.freeze({ ...foldedProviderJoin, coverage: "partial" as const })
			: foldedProviderJoin;
		const providerErrorDetail = latestModelCallTerminal !== undefined
			&& latestModelCallTerminal.status !== "succeeded"
			? describeProviderErrorTerminal(run) : undefined;
		const deadlineExceededDetail = describeDeadlineExceededTerminal(run);
		return Object.freeze({
			run,
			...(lastCompletedAssistantOutput === undefined
				? {} : { lastCompletedAssistantOutput }),
			...(latestModelCallTerminal === undefined ? {} : { latestModelCallTerminal }),
			pendingBackgroundTasks: backgroundTasks.live(),
			compactions: run.compactions,
			usage: normalizeUsage(run),
			completionEvidence: Object.freeze([...completionEvidence]),
			...(providerJoin === undefined ? {} : { providerJoin }),
			turnEvidence: Object.freeze([...turnEvidence]),
			toolActivity: frozenToolActivity(toolActivity, run),
			struggle: struggle.snapshot(),
			...(latestActivity === undefined ? {} : { latestActivity }),
			activityCount,
			...(providerErrorDetail === undefined ? {} : { providerErrorDetail }),
			...(deadlineExceededDetail === undefined ? {} : { deadlineExceededDetail }),
		});
	}

	return {
		observe(event: WireEvent): ExecutionFoldObservation {
			if (eventIds.has(event.id)) {
				throw new WireReductionError(
					"sequence_violation",
					`wire event id ${event.id} is duplicated`,
					{ eventId: event.id, runId: event.runId, sequence: event.sequence },
				);
			}
			const next = reduceWireEvent(run, event);
			eventIds.add(event.id);
			run = next;

			const backgroundTask = backgroundTasks.observe(event);
			const struggleTrigger = struggle.observe(event);
			const activityKind = outerExecutionActivityKind(event);
			if (activityKind !== undefined) {
				activityCount += 1;
				latestActivity = Object.freeze({ kind: activityKind, eventId: event.id, at: event.at });
			}

			let turnTerminal: WireModelCallSnapshot | undefined;
			if (event.essential) {
				if (event.kind === "message.recorded" && event.role === "assistant"
					&& event.stage === "completed") {
					lastCompletedAssistantOutput = assistantOutput(next, event.messageId);
				} else if (event.kind === "model-call.completed") {
					turnTerminal = next.modelCalls[event.modelCallId];
					latestModelCallTerminal = turnTerminal;
					if (event.evidence !== undefined) {
						completionEvidence.push(event.evidence);
						if (event.evidence.providerJoin !== undefined) {
							providerJoins.push(event.evidence.providerJoin);
						}
						if (event.evidence.turns !== undefined) turnEvidence.push(event.evidence.turns);
					}
				} else if (event.kind === "tool.admitted") {
					toolActivity.admitted += 1;
				} else if (event.kind === "tool.progress") {
					toolActivity.progressed += 1;
				} else if (event.kind === "tool.terminal") {
					toolActivity.terminal += 1;
					if (event.status === "succeeded") toolActivity.succeeded += 1;
					else if (event.status === "failed") toolActivity.failed += 1;
					else if (event.status === "cancelled") toolActivity.cancelled += 1;
					else toolActivity.syntheticFailures += 1;
				}
			}

			const state = currentSnapshot();
			if (state === undefined) {
				throw new WireReductionError(
					"state_violation",
					"execution fold did not produce a run snapshot",
					{ eventId: event.id, runId: event.runId, sequence: event.sequence },
				);
			}
			return Object.freeze({
				event,
				state,
				...(activityKind === undefined ? {} : { activityKind }),
				backgroundTask,
				struggleTrigger,
				...(turnTerminal === undefined ? {} : { turnTerminal }),
			});
		},
		snapshot(): ExecutionFoldSnapshot | undefined {
			return currentSnapshot();
		},
	};
}

export function foldExecutionEvents(
	events: readonly WireEvent[],
	policy: StrugglePolicy,
): ExecutionFoldSnapshot | undefined {
	const fold = makeExecutionFold(policy);
	for (const event of events) fold.observe(event);
	return fold.snapshot();
}
