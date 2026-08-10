import { expect, test } from "bun:test";
import {
	checkStruggle,
	makeStruggleObserver,
	resolveStrugglePolicy,
	type StruggleObserver,
} from "../src/struggle";
import {
	WireEventWriter,
	wireEventId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
} from "../src/wire";

function writer(label: string): WireEventWriter {
	const result = new WireEventWriter({
		runId: wireRunId(`run:struggle-${label}`),
		eventId: (sequence) => wireEventId(`event:struggle-${label}-${sequence}`),
		now: () => "2026-08-10T00:00:00.000Z",
	});
	result.append({ kind: "run.started", lifecycle: "running" });
	return result;
}

function toolResult(
	source: WireEventWriter,
	observer: StruggleObserver,
	name: string,
	index: number,
	status: "succeeded" | "failed" = "succeeded",
	argumentPreview = `{"index":${index}}`,
): void {
	const toolCallId = wireToolCallId(`tool:struggle-${index}`);
	observer.observe(source.append({
		kind: "tool.admitted",
		toolCallId,
		name,
		argumentPreview,
		schema: { status: "unavailable", reason: "test" },
	}));
	observer.observe(source.append({
		kind: "tool.terminal",
		toolCallId,
		status,
		origin: "provider",
	}));
}

test("struggle policy validates bounded topology-specific thresholds", () => {
	expect(resolveStrugglePolicy("worker", {})).toMatchObject({
		topology: "worker",
		errorStreak: 3,
		loopRepeat: 3,
		loopWindow: 20,
		noProgressTurns: 6,
	});
	expect(resolveStrugglePolicy("orchestrator", {})).toMatchObject({
		topology: "orchestrator",
		noProgressTurns: 12,
	});
	expect(() => resolveStrugglePolicy("worker", {
		STRUGGLE_LOOP_REPEAT: "21",
		STRUGGLE_LOOP_WINDOW: "20",
	})).toThrow("less than or equal");
	expect(() => resolveStrugglePolicy("worker", {
		STRUGGLE_STALL_TURNS: "20",
		STRUGGLE_STALL_TURNS_ORCHESTRATOR: "10",
	})).toThrow("greater than or equal");
});

test("canonical failed tool terminals drive the consecutive-error trigger", () => {
	const source = writer("errors");
	const observer = makeStruggleObserver(resolveStrugglePolicy("worker", {}));
	for (let index = 0; index < 3; index += 1) {
		toolResult(source, observer, "Read", index, "failed");
	}
	expect(checkStruggle(observer.state)).toBe("consecutive_errors");
	expect(observer.snapshot()).toMatchObject({
		errorCount: 3,
		triggers: ["consecutive_errors"],
	});
});

test("identical semantic tool admissions preserve loop detection after success", () => {
	const source = writer("loop");
	const observer = makeStruggleObserver(resolveStrugglePolicy("worker", {}));
	for (let index = 0; index < 3; index += 1) {
		toolResult(source, observer, "Read", index, "succeeded", "{\"path\":\"same\"}");
	}
	expect(checkStruggle(observer.state)).toBe("tool_loop");
	expect(observer.state.lastProgressTurn).toBe(3);
});

test("non-progress work units fire at the topology bound while successful work resets it", () => {
	for (const [topology, threshold] of [["worker", 6], ["orchestrator", 12]] as const) {
		const source = writer(`stall-${topology}`);
		const observer = makeStruggleObserver(resolveStrugglePolicy(topology, {}));
		for (let index = 0; index < threshold; index += 1) {
			toolResult(source, observer, "UnknownTool", index);
		}
		expect(checkStruggle(observer.state)).toBe("no_progress");
		expect(observer.state.workTurns).toBe(threshold);

		toolResult(source, observer, "mcp:north/show", threshold);
		expect(observer.state.lastProgressTurn).toBe(threshold + 1);
	}
});

test("provider-neutral model terminals count turns without spending the work-turn budget", () => {
	const source = writer("narration");
	const observer = makeStruggleObserver(resolveStrugglePolicy("worker", {}));
	for (let index = 0; index < 12; index += 1) {
		const modelCallId = wireModelCallId(`model-call:narration-${index}`);
		observer.observe(source.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "anthropic", tier: "standard" },
			attempt: 1,
		}));
		observer.observe(source.append({
			kind: "model-call.completed",
			modelCallId,
			status: "succeeded",
			origin: "provider",
			usage: source.snapshot()!.usage,
			usageCoverage: "exact",
		}));
	}
	expect(observer.state.turn).toBe(12);
	expect(observer.state.workTurns).toBe(0);
	expect(checkStruggle(observer.state)).toBeNull();
});

test("tool progress and background-task lifecycle do not settle or distort struggle state", () => {
	const source = writer("progress");
	const observer = makeStruggleObserver(resolveStrugglePolicy("worker", {}));
	const toolCallId = wireToolCallId("tool:background-progress");
	observer.observe(source.append({
		kind: "tool.admitted",
		toolCallId,
		name: "background-task",
		schema: { status: "unavailable", reason: "provider lifecycle" },
	}));
	observer.observe(source.append({
		kind: "tool.progress",
		toolCallId,
		progress: { status: "failed" },
	}));
	expect(observer.state.pending.size).toBe(0);
	expect(observer.state.totalErrors).toBe(0);
	expect(observer.state.workTurns).toBe(0);
});
