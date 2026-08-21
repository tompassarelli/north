import { afterEach, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
	providerNativeActorKey,
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
	options: { readonly identity?: boolean; readonly event?: "SessionEnd" | "SubagentStop" } = {},
): Fixture {
	const root = mkdtempSync(join(tmpdir(), "north-native-terminal-"));
	roots.push(root);
	const event = options.event ?? "SessionEnd";
	const sessionId = "session-123";
	const agentId = "agent-456";
	const actorId = event === "SessionEnd" ? sessionId : agentId;
	const actorNamespace = event === "SessionEnd" ? "session" : "agent";
	const transcriptPath = join(root, "rollout.jsonl");
	const records = [
		{
			type: "session_meta",
			timestamp: "2026-08-21T00:00:00.000Z",
			payload: { id: actorId, session_id: sessionId },
		},
		{
			type: "event_msg",
			timestamp: "2026-08-21T00:00:00.100Z",
			payload: { type: "task_started", turn_id: "turn-1" },
		},
		terminal,
	];
	writeFileSync(transcriptPath, `${records.map((entry) => JSON.stringify(entry)).join("\n")}\n`);
	const runtimeDir = join(root, "runtime");
	if (options.identity !== false) {
		const identityDir = join(runtimeDir, "north-agent-ids");
		mkdirSync(identityDir, { recursive: true });
		writeFileSync(
			join(identityDir, providerNativeActorKey(actorNamespace, actorId)),
			"native-exact-agent",
		);
	}
	const ledger: WireEventProjection[][] = [];
	const telemetry: WireRunTelemetryProjection[] = [];
	return {
		input: {
			hook_event_name: event,
			session_id: sessionId,
			...(event === "SubagentStop"
				? { agent_id: agentId, agent_transcript_path: transcriptPath }
				: { transcript_path: transcriptPath }),
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

test("direct Codex success publishes one attributable duration-bearing Wire run", async () => {
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
});

test("direct Codex provider failure preserves the existing failed/provider_error vocabulary", async () => {
	const state = fixture(terminal("turn_aborted", { reason: "provider_error" }));
	const result = await recordCodexProviderNativeTerminal(state.input, state.dependencies);
	expect(result).toMatchObject({ status: "recorded", processOutcome: "provider_error" });
	const run = facts(state.telemetry[0]!);
	expect(run.get("wire_run_lifecycle")).toBe("failed");
	expect(run.get("wire_termination_code")).toBe("provider_error");
	expect(run.get("process_outcome")).toBe("provider_error");
});

test("direct Codex abandonment preserves the existing cancelled/aborted vocabulary", async () => {
	const state = fixture(terminal("turn_aborted", { reason: "interrupted" }), {
		event: "SubagentStop",
	});
	const result = await recordCodexProviderNativeTerminal(state.input, state.dependencies);
	expect(result).toMatchObject({ status: "recorded", processOutcome: "aborted" });
	const run = facts(state.telemetry[0]!);
	expect(run.get("wire_run_lifecycle")).toBe("cancelled");
	expect(run.get("wire_termination_code")).toBe("aborted");
	expect(run.get("process_outcome")).toBe("aborted");
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
