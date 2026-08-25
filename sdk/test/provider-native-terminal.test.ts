import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
	providerNativeAgentKey,
	recordCodexProviderNativeTerminal,
	type ProviderNativeTerminalDependencies,
} from "../src/provider-native-terminal";
import type { WireEventProjection } from "../src/run-ledger";
import type { WireRunTelemetryProjection } from "../src/telemetry";

const roots: string[] = [];

afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { force: true, recursive: true });
});

interface Fixture {
	readonly input: Readonly<Record<string, unknown>>;
	readonly dependencies: ProviderNativeTerminalDependencies;
	readonly ledger: WireEventProjection[][];
	readonly telemetry: WireRunTelemetryProjection[];
}

function fixture(
	terminal: Readonly<Record<string, unknown>>,
	options: {
		readonly identity?: boolean;
		readonly metaSessionId?: string;
		readonly metaPayload?: Readonly<Record<string, unknown>>;
		readonly paddingBytes?: number;
		readonly executionRecords?: readonly Readonly<Record<string, unknown>>[];
	} = {},
): Fixture {
	const root = mkdtempSync(join(tmpdir(), "north-native-terminal-"));
	roots.push(root);
	const sessionId = "session-123";
	const agentId = "agent-456";
	const transcriptPath = join(root, "rollout.jsonl");
	const records = [
		{
			type: "session_meta",
			timestamp: "2026-08-21T00:00:00.000Z",
			payload: {
				id: agentId,
				session_id: options.metaSessionId ?? sessionId,
				...options.metaPayload,
			},
		},
		...(options.paddingBytes === undefined ? [] : [{
			type: "world_state",
			timestamp: "2026-08-21T00:00:00.001Z",
			payload: { padding: "x".repeat(options.paddingBytes) },
		}]),
		...(options.executionRecords ?? [{
			type: "event_msg",
			timestamp: "2026-08-21T00:00:00.100Z",
			payload: { type: "task_started", turn_id: "turn-1" },
		}]),
		terminal,
	];
	writeFileSync(transcriptPath, `${records.map((entry) => JSON.stringify(entry)).join("\n")}\n`);
	const runtimeDir = join(root, "runtime");
	if (options.identity !== false) {
		const identityDir = join(runtimeDir, "north-agent-ids");
		mkdirSync(identityDir, { recursive: true });
		writeFileSync(
			join(identityDir, providerNativeAgentKey(agentId)),
			"native-exact-agent",
		);
	}
	const ledger: WireEventProjection[][] = [];
	const telemetry: WireRunTelemetryProjection[] = [];
	return {
		input: {
			hook_event_name: "SubagentStop",
			session_id: sessionId,
			agent_id: agentId,
			agent_transcript_path: transcriptPath,
		},
		dependencies: {
			runtimeDir,
			env: {},
			ledgerWriter: async (projections) => {
				ledger.push([...projections]);
				return "recorded";
			},
			telemetryWriter: async (projection) => {
				telemetry.push(projection);
				return "recorded";
			},
		},
		ledger,
		telemetry,
	};
}

function terminal(type: string, extra: Readonly<Record<string, unknown>> = {}) {
	return {
		type: "event_msg",
		timestamp: "2026-08-21T00:00:02.500Z",
		payload: { type, turn_id: "turn-1", ...extra },
	};
}

function facts(projection: WireRunTelemetryProjection): Map<string, string> {
	return new Map(projection.facts);
}

function observation(state: Fixture): Readonly<Record<string, unknown>> {
	return JSON.parse(facts(state.telemetry[0]!).get("execution_observation")!);
}

function event(timestamp: string, payload: Readonly<Record<string, unknown>>) {
	return { type: "event_msg", timestamp, payload };
}

function responseItem(
	timestamp: string,
	type: string,
	callId: string,
): Readonly<Record<string, unknown>> {
	const request = type === "custom_tool_call"
		? { id: `item-${callId}`, input: "{}", name: "opaque", status: "completed" }
		: type === "function_call"
			? { id: `item-${callId}`, arguments: "{}", name: "opaque" }
			: { output: "opaque" };
	return { type: "response_item", timestamp, payload: { type, call_id: callId, ...request } };
}

test("native Codex subagent success publishes one attributable duration-bearing Wire run", async () => {
	const state = fixture(terminal("task_complete"));
	const result = await recordCodexProviderNativeTerminal(state.input, state.dependencies);
	expect(result).toMatchObject({ status: "recorded", processOutcome: "ran" });
	expect(state.ledger).toHaveLength(1);
	expect(state.ledger[0]).toHaveLength(2);
	const run = facts(state.telemetry[0]!);
	expect(run.get("agent")).toBe("native-exact-agent");
	expect(run.get("thread")).toBe("(ad-hoc)");
	expect(run.get("wire_run_lifecycle")).toBe("completed");
	expect(run.get("wire_termination_code")).toBe("completed");
	expect(run.get("process_outcome")).toBe("ran");
	expect(run.get("duration_ms")).toBe("2500");
	expect(run.get("execution_source")).toBe("provider-native");
	expect(JSON.parse(run.get("execution_observation")!)).toEqual({
		version: "agent-execution-observation/v1",
		coverage: "unknown",
		source: "codex_rollout_settlement_attempt_join_unavailable",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
});

test("real-schema copied forks with inherited parent history remain unknown", async () => {
	const state = fixture(terminal("task_complete", { turn_id: "child-turn" }), {
		metaPayload: {
			forked_from_id: "parent-session",
			parent_thread_id: "parent-session",
			thread_source: "subagent",
			source: { subagent: { thread_spawn: { parent_thread_id: "parent-session", depth: 1 } } },
		},
		executionRecords: [
			{
				type: "session_meta",
				timestamp: "2026-08-20T23:59:00.000Z",
				payload: { id: "parent-agent", session_id: "parent-session" },
			},
			event("2026-08-20T23:59:00.010Z", {
				type: "thread_settings_applied", thread_settings: { service_tier: "default" },
			}),
			event("2026-08-20T23:59:00.100Z", { type: "task_started", turn_id: "parent-turn" }),
			responseItem("2026-08-20T23:59:00.200Z", "function_call", "parent-call"),
			responseItem("2026-08-20T23:59:00.300Z", "function_call_output", "parent-call"),
			event("2026-08-20T23:59:00.400Z", { type: "task_complete", turn_id: "parent-turn" }),
			event("2026-08-21T00:00:00.700Z", {
				type: "thread_settings_applied", thread_settings: { service_tier: "priority" },
			}),
			event("2026-08-21T00:00:00.800Z", { type: "task_started", turn_id: "child-turn" }),
			responseItem("2026-08-21T00:00:00.900Z", "custom_tool_call", "child-call"),
			responseItem("2026-08-21T00:00:01.000Z", "custom_tool_call_output", "child-call"),
		],
	});
	expect((await recordCodexProviderNativeTerminal(state.input, state.dependencies)).status)
		.toBe("recorded");
	expect(observation(state)).toEqual({
		version: "agent-execution-observation/v1",
		coverage: "unknown",
		source: "codex_rollout_child_boundary_contaminated",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
});

test("fork metadata cannot stand in for an explicit child-local boundary", async () => {
	const state = fixture(terminal("task_complete"), {
		metaPayload: {
			forked_from_id: "parent-session",
			parent_thread_id: "parent-session",
			thread_source: "subagent",
		},
	});
	expect((await recordCodexProviderNativeTerminal(state.input, state.dependencies)).status)
		.toBe("recorded");
	expect(observation(state)).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_child_boundary_unavailable",
		evidence: {},
		segments: [],
	});
});

test("output-only and duplicate request records cannot establish admitted call counts", async () => {
	const executionPrefix = [
		event("2026-08-21T00:00:00.010Z", {
			type: "thread_settings_applied", thread_settings: { service_tier: "default" },
		}),
		event("2026-08-21T00:00:00.100Z", { type: "task_started", turn_id: "turn-1" }),
	] as const;
	for (const anomalousItems of [
		[responseItem("2026-08-21T00:00:00.200Z", "custom_tool_call_output", "call-1")],
		[
			responseItem("2026-08-21T00:00:00.200Z", "function_call", "call-1"),
			responseItem("2026-08-21T00:00:00.300Z", "function_call", "call-1"),
		],
	]) {
		const state = fixture(terminal("task_complete"), {
			executionRecords: [...executionPrefix, ...anomalousItems],
		});
		expect((await recordCodexProviderNativeTerminal(state.input, state.dependencies)).status)
			.toBe("recorded");
		expect(observation(state)).toMatchObject({
			coverage: "unknown",
			source: "codex_rollout_call_admission_unavailable",
			evidence: {},
			segments: [],
		});
	}
});

test("native Codex subagent provider failure preserves the failed/provider_error vocabulary", async () => {
	const state = fixture(terminal("task_failed"));
	const result = await recordCodexProviderNativeTerminal(state.input, state.dependencies);
	expect(result).toMatchObject({ status: "recorded", processOutcome: "provider_error" });
	const run = facts(state.telemetry[0]!);
	expect(run.get("wire_run_lifecycle")).toBe("failed");
	expect(run.get("wire_termination_code")).toBe("provider_error");
	expect(run.get("process_outcome")).toBe("provider_error");
	expect(observation(state)).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_settlement_attempt_join_unavailable",
		evidence: {},
		segments: [],
	});
});

test("native Codex subagent abandonment preserves the cancelled/aborted vocabulary", async () => {
	const state = fixture(terminal("turn_aborted", { reason: "interrupted" }));
	const result = await recordCodexProviderNativeTerminal(state.input, state.dependencies);
	expect(result).toMatchObject({ status: "recorded", processOutcome: "aborted" });
	const run = facts(state.telemetry[0]!);
	expect(run.get("wire_run_lifecycle")).toBe("cancelled");
	expect(run.get("wire_termination_code")).toBe("aborted");
	expect(run.get("process_outcome")).toBe("aborted");
	expect(observation(state)).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_settlement_attempt_join_unavailable",
		evidence: {},
		segments: [],
	});
});

test("a failed-turn retry cannot be folded into one exact execution observation", async () => {
	const state = fixture(terminal("task_complete", { turn_id: "turn-2" }), {
		executionRecords: [
			event("2026-08-21T00:00:00.010Z", {
				type: "thread_settings_applied", thread_settings: { service_tier: "default" },
			}),
			event("2026-08-21T00:00:00.100Z", { type: "task_started", turn_id: "turn-1" }),
			responseItem("2026-08-21T00:00:00.200Z", "function_call", "call-1"),
			event("2026-08-21T00:00:00.300Z", {
				type: "turn_aborted", turn_id: "turn-1", reason: "provider_error",
			}),
			event("2026-08-21T00:00:00.400Z", {
				type: "thread_settings_applied", thread_settings: { service_tier: "priority" },
			}),
			event("2026-08-21T00:00:00.500Z", { type: "task_started", turn_id: "turn-2" }),
			responseItem("2026-08-21T00:00:00.600Z", "custom_tool_call", "call-2"),
			responseItem("2026-08-21T00:00:00.700Z", "custom_tool_call_output", "call-2"),
		],
	});
	expect(await recordCodexProviderNativeTerminal(state.input, state.dependencies))
		.toMatchObject({ status: "recorded", processOutcome: "ran" });
	expect(observation(state)).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_turn_retry_join_unavailable",
		evidence: {},
		segments: [],
	});
});

test("duplicate terminal hooks replay byte-identical idempotent Store projections", async () => {
	const state = fixture(terminal("task_complete"));
	const durable = new Map<string, string>();
	let inserted = 0;
	const remember = (subject: string, value: unknown) => {
		const canonical = JSON.stringify(value);
		const prior = durable.get(subject);
		if (prior === undefined) {
			durable.set(subject, canonical);
			inserted += 1;
		} else {
			expect(canonical).toBe(prior);
		}
	};
	const dependencies: ProviderNativeTerminalDependencies = {
		...state.dependencies,
		ledgerWriter: async (projections) => {
			for (const projection of projections) remember(projection.subject, projection.facts);
			return "recorded";
		},
		telemetryWriter: async (projection) => {
			remember(projection.subject, projection.facts);
			return "recorded";
		},
	};
	expect((await recordCodexProviderNativeTerminal(state.input, dependencies)).status).toBe("recorded");
	expect((await recordCodexProviderNativeTerminal(state.input, dependencies)).status).toBe("recorded");
	expect(inserted).toBe(3);
	expect(durable.size).toBe(3);
});

test("managed lanes bypass native terminal publication", async () => {
	const state = fixture(terminal("task_complete"));
	const result = await recordCodexProviderNativeTerminal(state.input, {
		...state.dependencies,
		env: {
			NORTH_MANAGED_LANE: "1",
			AGENT_TOPOLOGY: "worker",
			AGENT_ID: "managed-lane",
		},
	});
	expect(result).toEqual({ status: "bypassed", reason: "managed" });
	expect(state.ledger).toHaveLength(0);
	expect(state.telemetry).toHaveLength(0);
});

test("unavailable persistence fails open without attempting an unbacked run summary", async () => {
	const state = fixture(terminal("task_complete"));
	let telemetryCalls = 0;
	const result = await recordCodexProviderNativeTerminal(state.input, {
		...state.dependencies,
		ledgerWriter: async () => "unavailable",
		telemetryWriter: async () => {
			telemetryCalls += 1;
			return "recorded";
		},
	});
	expect(result).toEqual({ status: "unavailable" });
	expect(telemetryCalls).toBe(0);
});

test("missing identity and incomplete terminals remain typed unknown and publish nothing", async () => {
	const missingIdentity = fixture(terminal("task_complete"), { identity: false });
	expect(await recordCodexProviderNativeTerminal(
		missingIdentity.input,
		missingIdentity.dependencies,
	)).toEqual({ status: "unknown", reason: "identity" });
	expect(missingIdentity.ledger).toHaveLength(0);

	const incomplete = fixture(terminal("task_started"));
	expect(await recordCodexProviderNativeTerminal(
		incomplete.input,
		incomplete.dependencies,
	)).toEqual({ status: "unknown", reason: "terminal" });
	expect(incomplete.ledger).toHaveLength(0);
});

test("an inexact attempt-session join publishes an explicit unknown observation", async () => {
	const state = fixture(terminal("task_complete"), {
		metaSessionId: "different-session",
		executionRecords: [
			{
				type: "event_msg",
				timestamp: "2026-08-21T00:00:00.010Z",
				payload: { type: "thread_settings_applied", thread_settings: { service_tier: "default" } },
			},
			{
				type: "event_msg",
				timestamp: "2026-08-21T00:00:00.100Z",
				payload: { type: "task_started", turn_id: "turn-1" },
			},
		],
	});
	expect((await recordCodexProviderNativeTerminal(state.input, state.dependencies)).status)
		.toBe("recorded");
	expect(JSON.parse(facts(state.telemetry[0]!).get("execution_observation")!)).toEqual({
		version: "agent-execution-observation/v1",
		coverage: "unknown",
		source: "codex_rollout_attempt_session_join_unavailable",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
});

test("a truncated transcript edge cannot claim exact execution coverage", async () => {
	const state = fixture(terminal("task_complete"), {
		paddingBytes: 8 * 1024 * 1024 + 1024,
		executionRecords: [
			{
				type: "event_msg", timestamp: "2026-08-21T00:00:00.010Z",
				payload: { type: "thread_settings_applied", thread_settings: { service_tier: "default" } },
			},
			{
				type: "event_msg", timestamp: "2026-08-21T00:00:00.100Z",
				payload: { type: "task_started", turn_id: "turn-1" },
			},
		],
	});
	expect((await recordCodexProviderNativeTerminal(state.input, state.dependencies)).status)
		.toBe("recorded");
	expect(JSON.parse(facts(state.telemetry[0]!).get("execution_observation")!)).toMatchObject({
		coverage: "unknown",
		source: "codex_rollout_transcript_incomplete",
		turn_unit: "unknown",
		tool_call_unit: "unknown",
		evidence: {},
		segments: [],
	});
});
