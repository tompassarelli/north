import { expect, test } from "bun:test";

import { providerJoinEvidence } from "../src/providers/provider-join";
import { wireLedgerSummary } from "../src/run-ledger";
import {
	applyTerminalCoordinatorReadTimeout,
	newRunId,
	recordWireRunTelemetry,
	wireRunTelemetryFacts,
} from "../src/telemetry";
import {
	WireEventWriter,
	reduceWireEvents,
	wireEventId,
	wireModelCallId,
	wireRunId,
} from "../src/wire";

const identity = Object.freeze({
	thread: "@thread-telemetry",
	agent: "telemetry-lane",
	parentThread: "@thread-parent",
	coordinator: "root",
});

function completedRun(runId = wireRunId("run:telemetry-lane-001")) {
	let tick = 0;
	const writer = new WireEventWriter({
		runId,
		eventId: (sequence) => wireEventId(`event:telemetry:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 1, 0, tick++)).toISOString(),
	});
	writer.append({
		kind: "run.started",
		lifecycle: "running",
		owner: "telemetry-lane",
		parentRunId: wireRunId("run:telemetry-parent"),
	});
	writer.append({
		kind: "run.progress",
		lifecycle: "running",
		progress: {
			compactions: 2,
			model: { provider: "openai", tier: "senior", capabilityClass: "authoring" },
			effort: "high",
			usage: {
				lifetime: {
					inputTokens: 100,
					outputTokens: 20,
					cacheReadTokens: 40,
					cacheWriteTokens: 10,
					reasoningTokens: 5,
					modelCalls: 0,
				},
				context: { tokens: 80, window: 400_000 },
			},
		},
	});
	writer.terminate({ lifecycle: "completed", reason: { code: "completed" } });
	return writer.events();
}

test("terminal coordinator timeout preserves an explicit caller value", () => {
	const derived: NodeJS.ProcessEnv = {};
	applyTerminalCoordinatorReadTimeout(derived);
	expect(derived.NORTH_COORD_READ_TIMEOUT_MS).toBe("70000");
	const explicit: NodeJS.ProcessEnv = { NORTH_COORD_READ_TIMEOUT_MS: "90000" };
	applyTerminalCoordinatorReadTimeout(explicit);
	expect(explicit.NORTH_COORD_READ_TIMEOUT_MS).toBe("90000");
});

test("run telemetry is derived only from a recorded terminal wire snapshot", () => {
	const events = completedRun();
	const snapshot = reduceWireEvents(events);
	const summary = wireLedgerSummary(events);
	const projection = wireRunTelemetryFacts(identity, snapshot, {
		status: "recorded",
		summary,
	}, {});
	const facts = new Map(projection.facts);

	expect(projection.subject).toBe("@run:telemetry-lane-001");
	expect(facts.get("wire_run_id")).toBe("run:telemetry-lane-001");
	expect(facts.get("wire_ledger_status")).toBe("complete");
	expect(facts.get("wire_event_count")).toBe(String(events.length));
	expect(facts.get("wire_ledger_sha256")).toBe(summary.digest);
	expect(facts.get("wire_run_lifecycle")).toBe("completed");
	expect(facts.get("outcome")).toBe("ran");
	expect(facts.get("duration_ms")).toBe("2000");
	expect(facts.get("lifetime_input_tokens")).toBe("100");
	expect(facts.get("lifetime_cache_write_tokens")).toBe("10");
	expect(facts.get("usage_terminal_count")).toBe("0");
	expect(facts.get("usage_scope")).toBe("wire_run_cumulative");
	expect(facts.get("usage_total_status")).toBe("partial");
	expect(facts.has("tokens")).toBe(false);
	expect(facts.get("context_tokens")).toBe("80");
	expect(facts.get("context_window_tokens")).toBe("400000");
	expect(facts.get("model_tier")).toBe("senior");
	expect(facts.get("effort")).toBe("high");
	expect(facts.get("parent_run")).toBe("@run:telemetry-parent");
	expect(facts.get("provider_session_persistence")).toBe("unknown");
	expect(facts.get("turn_provenance")).toBe("unknown");
	expect(JSON.parse(facts.get("execution_observation")!)).toEqual({
		version: "agent-execution-observation/v1",
		coverage: "unknown",
		source: "codex_app_server_mode_unavailable",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});

	const encoded = JSON.stringify(projection);
	expect(encoded).not.toContain("provider_target");
	expect(encoded).not.toContain("transport");
	expect(encoded).not.toContain("_north");
	expect(encoded).not.toContain("gpt-");
});

test("terminal telemetry preserves dispatch estimate calibration and struggle evidence", () => {
	const events = completedRun();
	const projection = wireRunTelemetryFacts(
		identity,
		reduceWireEvents(events),
		{ status: "recorded", summary: wireLedgerSummary(events) },
		{
			runEstimate: { hours: "0.001", durationMs: 3_600 },
			judgmentGrade: { grade: "m", status: "valid", source: "thread" },
			struggleObservation: {
				policyVersion: "north:struggle-observer:v2",
				topology: "worker",
				errorStreakThreshold: 3,
				loopRepeatThreshold: 3,
				loopWindow: 20,
				noProgressTurnThreshold: 6,
				errorCount: 3,
				triggers: ["consecutive_errors"],
			},
		},
	);

	expect(projection.facts).toContainEqual(["estimate_hours", "0.001"]);
	expect(projection.facts).toContainEqual(["estimate_delta_ms", "-1600"]);
	expect(projection.facts).toContainEqual(["estimate_ratio", "0.555556"]);
	expect(projection.facts).toContainEqual(["estimate_classification", "under"]);
	expect(projection.facts).toContainEqual(["judgment_grade", "m"]);
	expect(projection.facts).toContainEqual(["judgment_grade_status", "valid"]);
	expect(projection.facts).toContainEqual(["judgment_grade_source", "thread"]);
	expect(projection.facts).toContainEqual(["error_count", "3"]);
	expect(projection.facts).toContainEqual(["struggle", "consecutive_errors"]);
	expect(projection.facts).toContainEqual([
		"struggle_detector_policy_version",
		"north:struggle-observer:v2",
	]);
	expect(projection.facts).toContainEqual(["struggle_topology", "worker"]);
	expect(projection.facts).toContainEqual(["struggle_error_streak_threshold", "3"]);
	expect(projection.facts).toContainEqual(["struggle_loop_repeat_threshold", "3"]);
	expect(projection.facts).toContainEqual(["struggle_loop_window", "20"]);
	expect(projection.facts).toContainEqual(["struggle_no_progress_turn_threshold", "6"]);
});

test("run telemetry aggregates complete provider evidence across a managed session replacement", () => {
	let tick = 0;
	const writer = new WireEventWriter({
		runId: wireRunId("run:telemetry-provider-evidence"),
		eventId: (sequence) => wireEventId(`event:telemetry-provider-evidence:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 2, 0, tick++)).toISOString(),
	});
	writer.append({ kind: "run.started", lifecycle: "running" });
	for (const [index, inputTokens, outputTokens, toolItems, durationMs] of [
		[1, 100, 20, 2, 100],
		[2, 150, 30, 3, 150],
	] as const) {
		const modelCallId = wireModelCallId(`model-call:telemetry-provider-evidence:${index}`);
		writer.append({
			kind: "model-call.started",
			modelCallId,
			model: { provider: "openai", tier: "senior" },
			attempt: index,
		});
		writer.append({
			kind: "model-call.completed",
			modelCallId,
			status: "succeeded",
			origin: "provider",
			usageCoverage: "exact",
			usage: {
				lifetime: {
					inputTokens,
					outputTokens,
					cacheReadTokens: 40,
					cacheWriteTokens: 0,
					reasoningTokens: 5,
					modelCalls: index,
				},
				context: { tokens: inputTokens },
			},
			evidence: {
				providerJoin: providerJoinEvidence("openai", {
					sessionId: `private-session-${index}`,
					turnIds: [`private-turn-${index}`],
					sessionPersistence: "ephemeral",
				}),
				turns: { unit: "provider-turn", count: 1, toolItems, comparable: false },
				providerDurationMs: durationMs,
			},
		});
	}
	writer.terminate({ lifecycle: "completed", reason: { code: "completed" } });
	const events = writer.events();
	const projection = wireRunTelemetryFacts(
		identity,
		reduceWireEvents(events),
		{ status: "recorded", summary: wireLedgerSummary(events) },
		{},
	);
	const facts = new Map(projection.facts);
	expect(facts.get("usage_terminal_count")).toBe("2");
	expect(facts.get("usage_scope")).toBe("wire_run_cumulative");
	expect(facts.get("usage_total_status")).toBe("exact");
	expect(facts.get("tokens")).toBe("180");
	expect(facts.get("turn_provenance")).toBe("provider-terminal");
	expect(projection.facts.filter(([predicate]) => predicate === "provider_turn_units"))
		.toEqual([["provider_turn_units", "2"]]);
	expect(projection.facts.filter(([predicate]) => predicate === "provider_tool_items"))
		.toEqual([["provider_tool_items", "5"]]);
	expect(projection.facts.filter(
		([predicate]) => predicate === "provider_turn_metric_comparable",
	)).toEqual([["provider_turn_metric_comparable", "false"]]);
	expect(projection.facts.some(([predicate]) => predicate.startsWith("codex_"))).toBe(false);
	expect(facts.get("provider_duration_ms")).toBe("250");
	expect(facts.get("provider_join_coverage")).toBe("partial");
	expect(facts.has("provider_session_key")).toBe(false);
	expect(projection.facts.filter(([predicate]) => predicate === "provider_turn_key"))
		.toHaveLength(2);
	expect(JSON.stringify(projection)).not.toContain("private-session");
	expect(JSON.stringify(projection)).not.toContain("private-turn");
});

test("managed respawn telemetry preserves joins without overstating reset-session totals", () => {
	let tick = 0;
	const writer = new WireEventWriter({
		runId: wireRunId("run:telemetry-provider-respawn"),
		eventId: (sequence) => wireEventId(`event:telemetry-provider-respawn:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 2, 30, tick++)).toISOString(),
	});
	writer.append({ kind: "run.started", lifecycle: "running" });
	const firstCallId = wireModelCallId("model-call:telemetry-provider-respawn:1");
	writer.append({
		kind: "model-call.started",
		modelCallId: firstCallId,
		model: { provider: "openai", tier: "senior" },
		attempt: 1,
	});
	writer.append({
		kind: "run.progress",
		lifecycle: "running",
		progress: {
			usage: {
				lifetime: {
					inputTokens: 60,
					outputTokens: 10,
					cacheReadTokens: 0,
					cacheWriteTokens: 0,
					reasoningTokens: 0,
					modelCalls: 1,
				},
				context: { tokens: 70 },
			},
		},
	});
	writer.append({
		kind: "model-call.completed",
		modelCallId: firstCallId,
		status: "failed",
		origin: "north",
		usageCoverage: "unavailable",
		usage: writer.snapshot()!.usage,
		errorCode: "provider_session_replaced",
		evidence: {
			providerJoin: providerJoinEvidence("openai", {
				sessionId: "private-session-before-respawn",
				turnIds: ["private-turn-before-respawn"],
				sessionPersistence: "persisted",
			}),
			turns: { unit: "provider-turn", count: 1, toolItems: 0, comparable: false },
		},
	});
	const replacementCallId = wireModelCallId("model-call:telemetry-provider-respawn:2");
	writer.append({
		kind: "model-call.started",
		modelCallId: replacementCallId,
		model: { provider: "openai", tier: "senior" },
		attempt: 2,
	});
	writer.append({
		kind: "model-call.completed",
		modelCallId: replacementCallId,
		status: "succeeded",
		origin: "provider",
		usageCoverage: "exact",
		usage: {
			lifetime: {
				inputTokens: 160,
				outputTokens: 30,
				cacheReadTokens: 0,
				cacheWriteTokens: 0,
				reasoningTokens: 0,
				modelCalls: 2,
			},
			context: { tokens: 130 },
		},
		evidence: {
			providerJoin: providerJoinEvidence("openai", {
				sessionId: "private-session-after-respawn",
				turnIds: ["private-turn-after-respawn"],
				sessionPersistence: "persisted",
			}),
			turns: { unit: "provider-turn", count: 1, toolItems: 2, comparable: false },
			providerDurationMs: 200,
		},
	});
	writer.terminate({ lifecycle: "completed", reason: { code: "completed" } });
	const events = writer.events();
	const projection = wireRunTelemetryFacts(
		identity,
		reduceWireEvents(events),
		{ status: "recorded", summary: wireLedgerSummary(events) },
		{},
	);
	const facts = new Map(projection.facts);
	expect(facts.get("usage_total_status")).toBe("partial");
	expect(facts.get("usage_terminal_count")).toBe("1");
	expect(facts.has("tokens")).toBe(false);
	expect(facts.get("provider_join_coverage")).toBe("partial");
	expect(facts.has("provider_session_key")).toBe(false);
	expect(projection.facts.filter(([predicate]) => predicate === "provider_turn_key"))
		.toHaveLength(2);
	expect(facts.get("turn_provenance")).toBe("unknown");
	expect(facts.has("provider_turn_units")).toBe(false);
	expect(facts.has("provider_tool_items")).toBe(false);
	expect(facts.has("provider_duration_ms")).toBe(false);
	const encoded = JSON.stringify(projection);
	expect(encoded).not.toContain("private-session");
	expect(encoded).not.toContain("private-turn");
});

test("a North-synthetic latest call keeps joins but makes run turn and token totals inexact", () => {
	let tick = 0;
	const writer = new WireEventWriter({
		runId: wireRunId("run:telemetry-synthetic-latest"),
		eventId: (sequence) => wireEventId(`event:telemetry-synthetic-latest:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 3, 0, tick++)).toISOString(),
	});
	writer.append({ kind: "run.started", lifecycle: "running" });
	const firstCallId = wireModelCallId("model-call:telemetry-synthetic-latest:1");
	writer.append({
		kind: "model-call.started",
		modelCallId: firstCallId,
		model: { provider: "openai", tier: "standard" },
		attempt: 1,
	});
	writer.append({
		kind: "model-call.completed",
		modelCallId: firstCallId,
		status: "succeeded",
		origin: "provider",
		usageCoverage: "exact",
		usage: {
			lifetime: {
				inputTokens: 100,
				outputTokens: 20,
				cacheReadTokens: 40,
				cacheWriteTokens: 0,
				reasoningTokens: 5,
				modelCalls: 1,
			},
			context: { tokens: 80 },
		},
		evidence: {
			providerJoin: providerJoinEvidence("openai", {
				sessionId: "private-session",
				turnIds: ["private-turn-1"],
				sessionPersistence: "ephemeral",
			}),
			turns: { unit: "provider-turn", count: 1, toolItems: 2, comparable: false },
			providerDurationMs: 100,
		},
	});
	const secondCallId = wireModelCallId("model-call:telemetry-synthetic-latest:2");
	writer.append({
		kind: "model-call.started",
		modelCallId: secondCallId,
		model: { provider: "openai", tier: "standard" },
		attempt: 2,
	});
	writer.append({
		kind: "model-call.completed",
		modelCallId: secondCallId,
		status: "cancelled",
		origin: "north",
		usageCoverage: "unavailable",
		usage: writer.snapshot()!.usage,
		errorCode: "north_abort",
		evidence: {
			turns: { unit: "provider-turn", count: 1, toolItems: 99, comparable: false },
			providerDurationMs: 999,
		},
	});
	writer.terminate({ lifecycle: "cancelled", reason: { code: "aborted" } });
	const events = writer.events();
	const projection = wireRunTelemetryFacts(
		identity,
		reduceWireEvents(events),
		{ status: "recorded", summary: wireLedgerSummary(events) },
		{},
	);
	const facts = new Map(projection.facts);
	expect(facts.get("provider_session_persistence")).toBe("ephemeral");
	expect(facts.get("provider_join_coverage")).toBe("partial");
	expect(projection.facts.filter(([predicate]) => predicate === "provider_turn_key"))
		.toHaveLength(1);
	expect(facts.get("turn_provenance")).toBe("unknown");
	expect(facts.has("provider_turn_units")).toBe(false);
	expect(facts.has("provider_tool_items")).toBe(false);
	expect(facts.has("provider_duration_ms")).toBe(false);
	expect(facts.get("usage_terminal_count")).toBe("1");
	expect(facts.get("usage_total_status")).toBe("partial");
	expect(facts.has("tokens")).toBe(false);
});

test("watchdog facts come only from replayed provider-neutral abort evidence", () => {
	let tick = 0;
	const writer = new WireEventWriter({
		runId: wireRunId("run:telemetry-watchdog"),
		eventId: (sequence) => wireEventId(`event:telemetry-watchdog:${sequence}`),
		now: () => new Date(Date.UTC(2026, 7, 10, 4, 0, tick++)).toISOString(),
	});
	writer.append({ kind: "run.started", lifecycle: "running" });
	writer.terminate({
		lifecycle: "cancelled",
		reason: { code: "aborted" },
		abort: {
			requestedAt: "2026-08-10T04:00:01.000Z",
			source: "watchdog",
			reason: "north_watchdog_execution_inactivity",
			watchdog: {
				silenceMs: 20_000,
				lastOuter: {
					origin: "outer",
					kind: "message",
					observedAt: "2026-08-10T03:59:40.000Z",
				},
				lastProvider: {
					origin: "provider",
					kind: "tool",
					observedAt: "2026-08-10T03:59:39.000Z",
				},
			},
		},
	});
	const events = writer.events();
	const projection = wireRunTelemetryFacts(
		identity,
		reduceWireEvents(events),
		{ status: "recorded", summary: wireLedgerSummary(events) },
		{},
	);
	expect(projection.facts).toContainEqual([
		"watchdog_reason",
		"north_watchdog_execution_inactivity",
	]);
	expect(projection.facts).toContainEqual(["watchdog_silence_ms", "20000"]);
	expect(projection.facts).toContainEqual([
		"watchdog_last_outer_activity",
		JSON.stringify({
			origin: "outer",
			kind: "message",
			observedAt: "2026-08-10T03:59:40.000Z",
		}),
	]);
	expect(projection.facts).toContainEqual([
		"watchdog_last_provider_activity",
		JSON.stringify({
			origin: "provider",
			kind: "tool",
			observedAt: "2026-08-10T03:59:39.000Z",
		}),
	]);
});

test("a mismatched ledger can never produce a completeness projection", () => {
	const events = completedRun();
	const snapshot = reduceWireEvents(events);
	const summary = wireLedgerSummary(events);
	expect(() => wireRunTelemetryFacts(identity, snapshot, {
		status: "recorded",
		summary: { ...summary, digest: "0".repeat(64), eventCount: summary.eventCount + 1 },
	}, {})).toThrow("does not match the run snapshot");
});

test("run summary subjects cannot collide across exact wire run IDs", () => {
	const plainEvents = completedRun();
	const atPrefixedEvents = completedRun(wireRunId("@run:telemetry-lane-001"));
	const plain = wireRunTelemetryFacts(identity, reduceWireEvents(plainEvents), {
		status: "recorded",
		summary: wireLedgerSummary(plainEvents),
	}, {});
	const atPrefixed = wireRunTelemetryFacts(identity, reduceWireEvents(atPrefixedEvents), {
		status: "recorded",
		summary: wireLedgerSummary(atPrefixedEvents),
	}, {});

	expect(plain.subject).toBe("@run:telemetry-lane-001");
	expect(atPrefixed.subject).toMatch(/^@run:wire-summary-[a-f0-9]{64}$/);
	expect(atPrefixed.subject).not.toBe(plain.subject);
	expect(new Map(atPrefixed.facts).get("wire_run_id")).toBe("@run:telemetry-lane-001");
});

test("recording forwards one immutable projection and preserves sink unavailability", async () => {
	const events = completedRun();
	const snapshot = reduceWireEvents(events);
	const summary = wireLedgerSummary(events);
	let calls = 0;
	const status = await recordWireRunTelemetry(
		identity,
		snapshot,
		{ status: "recorded", summary },
		{},
		1234,
		async (projection, timeoutMs) => {
			calls += 1;
			expect(Object.isFrozen(projection)).toBe(true);
			expect(timeoutMs).toBe(1234);
			return "unavailable";
		},
	);
	expect(status).toBe("unavailable");
	expect(calls).toBe(1);
});

test("new run IDs are branded, unique North run identities", () => {
	const first = newRunId("same-agent");
	const second = newRunId("same-agent");
	expect(first).toMatch(/^run:same-agent-[0-9a-f-]{36}$/);
	expect(second).not.toBe(first);
	expect(() => newRunId("bad agent")).toThrow("invalid run agent identity");
});
