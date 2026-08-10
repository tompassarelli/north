import { afterEach, expect, test } from "bun:test";
import { bgContinuationMessage, makeBgTracker, maxBgContinuations } from "../src/bgtasks";
import {
	WireEventWriter,
	wireEventId,
	wireRunId,
	wireToolCallId,
} from "../src/wire";

const originalMax = process.env.NORTH_BG_MAX_CONTINUATIONS;

afterEach(() => {
	if (originalMax === undefined) delete process.env.NORTH_BG_MAX_CONTINUATIONS;
	else process.env.NORTH_BG_MAX_CONTINUATIONS = originalMax;
});

function writer(label: string): WireEventWriter {
	const result = new WireEventWriter({
		runId: wireRunId(`run:bg-${label}`),
		eventId: (sequence) => wireEventId(`event:bg-${label}-${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
	result.append({ kind: "run.started", lifecycle: "running" });
	return result;
}

test("background work is live from tool admission through its canonical terminal", () => {
	const source = writer("lifecycle");
	const tracker = makeBgTracker();
	const taskId = wireToolCallId("tool:bg-task");
	const admitted = source.append({
		kind: "tool.admitted",
		toolCallId: taskId,
		name: "background-task",
		schema: { status: "unavailable", reason: "provider lifecycle" },
	});
	expect(tracker.observe(admitted)).toEqual({ kind: "started", toolCallId: taskId });
	expect(tracker.live()).toEqual([taskId]);

	const progress = source.append({
		kind: "tool.progress",
		toolCallId: taskId,
		progress: { status: "completed" },
	});
	expect(tracker.observe(progress)).toBeNull();
	expect(tracker.live()).toEqual([taskId]);

	const terminal = source.append({
		kind: "tool.terminal",
		toolCallId: taskId,
		status: "succeeded",
		origin: "provider",
	});
	expect(tracker.observe(terminal)).toEqual({ kind: "settled", toolCallId: taskId });
	expect(tracker.live()).toEqual([]);
});

test("ordinary tools do not enter the background-task liveness set", () => {
	const source = writer("ordinary");
	const tracker = makeBgTracker();
	const toolCallId = wireToolCallId("tool:ordinary");
	expect(tracker.observe(source.append({
		kind: "tool.admitted",
		toolCallId,
		name: "Read",
		schema: { status: "unavailable", reason: "test" },
	}))).toBeNull();
	expect(tracker.size()).toBe(0);
});

test("continuation text names canonical task IDs and the refusal cap is bounded", () => {
	expect(bgContinuationMessage([
		wireToolCallId("tool:bg-a"),
		wireToolCallId("tool:bg-b"),
	])).toContain("tool:bg-a, tool:bg-b");
	expect(bgContinuationMessage([])).toContain("unknown");

	delete process.env.NORTH_BG_MAX_CONTINUATIONS;
	expect(maxBgContinuations()).toBe(5);
	process.env.NORTH_BG_MAX_CONTINUATIONS = "3";
	expect(maxBgContinuations()).toBe(3);
	process.env.NORTH_BG_MAX_CONTINUATIONS = "invalid";
	expect(maxBgContinuations()).toBe(5);
});
