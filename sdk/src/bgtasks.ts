import type { WireEvent } from "./wire/events";
import type { WireToolCallId } from "./wire/ids";

export type BgEvent =
	| { kind: "started"; toolCallId: WireToolCallId }
	| { kind: "settled"; toolCallId: WireToolCallId }
	| null;

export interface BgTracker {
	/** Fold one canonical event into the live background-task set. */
	observe(event: WireEvent): BgEvent;
	live(): readonly WireToolCallId[];
	size(): number;
}

/**
 * Track semantic background work. A tool is live from admission until its one
 * canonical terminal; progress frames cannot settle it.
 */
export function makeBgTracker(): BgTracker {
	const tasks = new Set<WireToolCallId>();

	return {
		observe(event: WireEvent): BgEvent {
			if (!event.essential) return null;
			if (event.kind === "tool.admitted" && event.name === "background-task") {
				if (tasks.has(event.toolCallId)) return null;
				tasks.add(event.toolCallId);
				return Object.freeze({ kind: "started", toolCallId: event.toolCallId });
			}
			if (event.kind === "tool.terminal" && tasks.delete(event.toolCallId)) {
				return Object.freeze({ kind: "settled", toolCallId: event.toolCallId });
			}
			return null;
		},
		live(): readonly WireToolCallId[] {
			return Object.freeze([...tasks]);
		},
		size(): number {
			return tasks.size;
		},
	};
}

export function bgContinuationMessage(ids: readonly WireToolCallId[]): string {
	const list = ids.length > 0 ? ids.join(", ") : "unknown";
	return `[harness] background task(s) still live (${list}) — a lane cannot exit with `
		+ "tracked work running; sleep-poll until done, consume the result, or kill it.";
}

export function maxBgContinuations(): number {
	const raw = Number(process.env.NORTH_BG_MAX_CONTINUATIONS);
	return Number.isFinite(raw) && raw > 0 ? Math.floor(raw) : 5;
}
