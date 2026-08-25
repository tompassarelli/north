import { expect, test } from "bun:test";

import {
	EXECUTION_OBSERVATION_VERSION,
	executionObservationJson,
	executionObservationMode,
	executionObservationTotals,
	normalizeExecutionObservation,
	reduceCodexExecutionObservation,
} from "../src/execution-observation";

const attempt = "a".repeat(64);
const session = "b".repeat(64);
const turns = ["c", "d", "e", "f", "1", "2", "3", "4"].map((value) => value.repeat(64));
const calls = ["5", "6", "7", "8", "9", "0"].map((value) => value.repeat(64));

const exact = {
	version: EXECUTION_OBSERVATION_VERSION,
	coverage: "exact",
	source: "codex-rollout",
	turn_unit: "assistant-turn",
	tool_call_unit: "admitted-tool-call",
	evidence: { provider: "openai", attempt_sha256: attempt, session_sha256: session },
} as const;

test("normalization preserves ordered contiguous mode segments and derives totals", () => {
	const observation = normalizeExecutionObservation({
		...exact,
		segments: [
			{ mode: "standard", turn_count: 2, tool_call_count: 3, turn_sha256: turns.slice(0, 2) },
			{ mode: "fast", turn_count: 1, tool_call_count: 4, turn_sha256: turns.slice(2, 3) },
			{ mode: "standard", turn_count: 5, tool_call_count: 6, turn_sha256: turns.slice(3, 8) },
		],
	});
	expect(observation.segments.map((segment) => segment.mode))
		.toEqual(["standard", "fast", "standard"]);
	expect(executionObservationTotals(observation)).toEqual({ turn_count: 8, tool_call_count: 13 });
	expect(executionObservationMode(observation)).toBe("mixed");
});

test("durable Codex events reduce to standard, fast, and switched exact observations", () => {
	const joined = { attempt_sha256: attempt, session_sha256: session };
	const switched = reduceCodexExecutionObservation([
		{ kind: "thread_settings_applied", ...joined, service_tier: "default" },
		{ kind: "task_started", ...joined, turn_sha256: turns[0]! },
		{ kind: "tool_call_admitted", ...joined, turn_sha256: turns[0]!, tool_call_sha256: calls[0]! },
		{ kind: "thread_settings_applied", ...joined, service_tier: "priority" },
		{ kind: "task_started", ...joined, turn_sha256: turns[1]! },
		{ kind: "tool_call_admitted", ...joined, turn_sha256: turns[1]!, tool_call_sha256: calls[1]! },
		{ kind: "tool_call_admitted", ...joined, turn_sha256: turns[1]!, tool_call_sha256: calls[2]! },
		{ kind: "thread_settings_applied", ...joined, service_tier: "default" },
		{ kind: "task_started", ...joined, turn_sha256: turns[2]! },
	]);
	expect(switched.coverage).toBe("exact");
	expect(switched.segments).toEqual([
		{ mode: "standard", turn_count: 1, tool_call_count: 1, turn_sha256: [turns[0]] },
		{ mode: "fast", turn_count: 1, tool_call_count: 2, turn_sha256: [turns[1]] },
		{ mode: "standard", turn_count: 1, tool_call_count: 0, turn_sha256: [turns[2]] },
	]);
	expect(executionObservationMode(switched)).toBe("mixed");

	for (const [service_tier, mode] of [["default", "standard"], ["priority", "fast"]] as const) {
		const pure = reduceCodexExecutionObservation([
			{ kind: "thread_settings_applied", ...joined, service_tier },
			{ kind: "task_started", ...joined, turn_sha256: turns[3]! },
		]);
		expect(executionObservationMode(pure)).toBe(mode);
	}
});

test("missing initial settings or exact joins remain unknown", () => {
	const noSettings = reduceCodexExecutionObservation([
		{ kind: "task_started", attempt_sha256: attempt, session_sha256: session, turn_sha256: turns[0]! },
	]);
	expect(noSettings).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_initial_settings_unavailable",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
	const noJoin = reduceCodexExecutionObservation([
		{ kind: "thread_settings_applied", attempt_sha256: attempt, session_sha256: session,
			service_tier: "default" },
		{ kind: "task_started", attempt_sha256: "f".repeat(64), session_sha256: session,
			turn_sha256: turns[0]! },
	]);
	expect(noJoin).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_attempt_session_join_unavailable",
		segments: [],
	});
});

test("one admitted call identity cannot be counted again under a later turn", () => {
	const joined = { attempt_sha256: attempt, session_sha256: session };
	const observation = reduceCodexExecutionObservation([
		{ kind: "thread_settings_applied", ...joined, service_tier: "default" },
		{ kind: "task_started", ...joined, turn_sha256: turns[0]! },
		{ kind: "tool_call_admitted", ...joined, turn_sha256: turns[0]!,
			tool_call_sha256: calls[0]! },
		{ kind: "task_started", ...joined, turn_sha256: turns[1]! },
		{ kind: "tool_call_admitted", ...joined, turn_sha256: turns[1]!,
			tool_call_sha256: calls[0]! },
	]);
	expect(observation).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_tool_evidence_invalid",
		evidence: {},
		segments: [],
	});
});

test("provider-turn and provider-item units cannot masquerade as comparable observations", () => {
	expect(() => normalizeExecutionObservation({
		...exact,
		turn_unit: "provider-turn",
		tool_call_unit: "provider-tool-item",
		segments: [{ mode: "fast", turn_count: 1, tool_call_count: 1, turn_sha256: [turns[0]] }],
	})).toThrow("units are not comparable");
});

test("unknown telemetry never manufactures a standard segment or zero counts", () => {
	const observation = normalizeExecutionObservation({
		version: EXECUTION_OBSERVATION_VERSION,
		coverage: "unknown",
		source: "codex-rollout-initial-mode-unavailable",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
	expect(executionObservationMode(observation)).toBe("unknown");
	expect(executionObservationTotals(observation)).toBeUndefined();
	expect(executionObservationJson(observation)).not.toContain("standard");
});

test("malformed and non-contiguous observations fail closed", () => {
	expect(() => normalizeExecutionObservation({ ...exact, segments: [] }))
		.toThrow("requires a segment");
	expect(() => normalizeExecutionObservation({
		...exact,
		segments: [
			{ mode: "fast", turn_count: 1, tool_call_count: 0, turn_sha256: [turns[0]] },
			{ mode: "fast", turn_count: 1, tool_call_count: 0, turn_sha256: [turns[1]] },
		],
	})).toThrow("adjacent equal modes");
	expect(() => normalizeExecutionObservation({
		...exact,
		segments: [{ mode: "standard", turn_count: 0, tool_call_count: 0, turn_sha256: [] }],
	})).toThrow("safe positive integer");
	expect(() => normalizeExecutionObservation({
		...exact,
		segments: [
			{ mode: "standard", turn_count: 1, tool_call_count: Number.MAX_SAFE_INTEGER,
				turn_sha256: [turns[0]] },
			{ mode: "fast", turn_count: 1, tool_call_count: 1, turn_sha256: [turns[1]] },
		],
	})).toThrow("total tool-call count");
});
