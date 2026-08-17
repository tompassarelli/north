import { afterEach, describe, expect, test } from "bun:test";
import * as fs from "node:fs/promises";
import * as os from "node:os";
import * as path from "node:path";

import { FramRpcClient } from "../src/framrpc-client";
import { FramTriple } from "../src/framrpc-codec";
import {
	AGENT_RUN_LEDGER_CONTRACT,
	AGENT_RUN_LEDGER_VERSION,
	WireLedgerError,
	publishWireEvents,
	recordWireEventProjections,
	wireEventFacts,
	wireLedgerSummary,
	type WireEventProjection,
} from "../src/run-ledger";
import {
	recordWireRunTelemetryProjection,
	wireRunTelemetryFacts,
} from "../src/telemetry";
import { SHADOW_REVIEWER_VERSION } from "../src/shadow-reviewer";
import {
	WireEventWriter,
	decodeWireEvent,
	encodeWireJsonlLine,
	reduceWireEvents,
	wireEventId,
	wireModelCallId,
	wireRunId,
	type WireEvent,
} from "../src/wire";

const roots: string[] = [];
const identity = Object.freeze({
	thread: "@019f89ac-a86a-7399-b915-358d44a1be15",
	agent: "lane-ledger",
	parentThread: "@019f89ac-parent",
	coordinator: "north-root",
});

afterEach(async () => {
	await Promise.all(roots.splice(0).map((root) => fs.rm(root, { recursive: true, force: true })));
});

function sha256(value: string): string {
	return new Bun.CryptoHasher("sha256").update(value).digest("hex");
}

function canonicalJson(value: unknown): string {
	if (value === null || typeof value !== "object") {
		const encoded = JSON.stringify(value);
		if (encoded === undefined) throw new TypeError("test JSON value is not encodable");
		return encoded;
	}
	if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
	const entries = Object.entries(value as Record<string, unknown>)
		.sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0);
	return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonicalJson(item)}`).join(",")}}`;
}

function uncheckedProjection(event: Readonly<Record<string, unknown>>): WireEventProjection {
	const raw = canonicalJson(event);
	const runId = String(event.runId);
	const sequence = Number(event.sequence);
	return Object.freeze({
		subject: `@run:wire-event-${sha256(`north-wire-event-subject:v2\0${runId}\0${sequence}`)}`,
		facts: Object.freeze([
			["kind", "wire_event"],
			["wire_ledger_version", AGENT_RUN_LEDGER_VERSION],
			["wire_version", String(event.version)],
			["wire_run_id", runId],
			["thread", identity.thread],
			["agent", identity.agent],
			["wire_event_id", String(event.id)],
			["wire_event_sequence", String(sequence)],
			["wire_event_at", String(event.at)],
			["wire_event_kind", String(event.kind)],
			["wire_event_essential", String(event.essential)],
			["wire_event_json", raw],
			["wire_event_sha256", sha256(raw)],
			["parent_thread", identity.parentThread],
			["run_coordinator", identity.coordinator],
		] as const),
	});
}

function runEvents(
	runId = wireRunId("run:lane-ledger-001"),
	terminalDurationMs = 2_000,
): readonly WireEvent[] {
	let tick = 0;
	const writer = new WireEventWriter({
		runId,
		eventId: (sequence) => wireEventId(`@event:ledger:${sequence}`),
		now: () => {
			const offsetMs = tick === 2 ? terminalDurationMs : tick * 1_000;
			tick += 1;
			return new Date(Date.UTC(2026, 7, 10, 0, 0, 0, offsetMs)).toISOString();
		},
	});
	writer.append({
		kind: "run.started",
		lifecycle: "running",
		owner: "lane-ledger",
		extensions: {
			large: 1e21,
			small: 1e-7,
			fractional: 1.25,
			bulk: "x".repeat(150_000),
		},
	});
	writer.append({
		kind: "run.progress",
		lifecycle: "running",
		progress: {
			currentAction: "persist exact events",
			compactions: 1,
			usage: {
				lifetime: {
					inputTokens: 12,
					outputTokens: 4,
					cacheReadTokens: 3,
					cacheWriteTokens: 2,
					reasoningTokens: 1,
					modelCalls: 0,
				},
				context: { tokens: 9, window: 100 },
			},
		},
	});
	writer.terminate({ lifecycle: "completed", reason: { code: "completed" } });
	return writer.events();
}

function ledgerDigest(events: readonly WireEvent[]): string {
	const digests = events.map((event) => sha256(encodeWireJsonlLine(event).slice(0, -1)));
	return sha256(`[${digests.map((digest) => JSON.stringify(digest)).join(",")}]`);
}

describe("event-native wire ledger", () => {
	test("projects the exact canonical event bytes and a stable domain-separated subject", () => {
		const longRun = wireRunId(`r${"@x".repeat(127)}x`);
		expect(longRun.length).toBe(256);
		const event = runEvents(longRun)[0]!;
		const projection = wireEventFacts(identity, event);
		const facts = Object.fromEntries(projection.facts);
		const canonical = encodeWireJsonlLine(event).slice(0, -1);

		expect(AGENT_RUN_LEDGER_VERSION).toBe("north-agent-run-ledger:v2");
		expect(AGENT_RUN_LEDGER_CONTRACT.wireVersion).toBe("north:wire:v2");
		expect(projection.subject).toMatch(/^@run:wire-event-[a-f0-9]{64}$/);
		expect(facts.wire_run_id).toBe(longRun);
		expect(facts.wire_event_essential).toBe("true");
		expect(facts.wire_event_json).toBe(canonical);
		expect(new TextEncoder().encode(canonical).byteLength).toBeGreaterThan(128 * 1024);
		expect(facts.wire_event_sha256).toBe(sha256(canonical));
		expect(canonical).toContain("1e+21");
		expect(canonical).toContain("1e-7");
	});

	test("summarizes only a complete reducible terminal sequence", () => {
		const events = runEvents();
		const summary = wireLedgerSummary(events);
		expect(summary).toEqual({
			version: AGENT_RUN_LEDGER_VERSION,
			wireVersion: "north:wire:v2",
			runId: events[0]!.runId,
			eventCount: events.length,
			firstSequence: 0,
			lastSequence: events.length - 1,
			terminalEventId: events.at(-1)!.id,
			digest: ledgerDigest(events),
		});
		expect(() => wireLedgerSummary(events.slice(0, -1))).toThrow(WireLedgerError);
	});

	test("publishes contiguous canonical batches without retrospective synthesis", async () => {
		const events = runEvents();
		let observed: readonly WireEventProjection[] = [];
		const status = await publishWireEvents(identity, events, 4321, async (projections, timeoutMs) => {
			observed = projections;
			expect(timeoutMs).toBe(4321);
			return "recorded";
		});
		expect(status).toBe("recorded");
		expect(observed).toHaveLength(events.length);
		expect(observed.map(({ facts }) => Object.fromEntries(facts).wire_event_kind))
			.toEqual(["run.started", "run.progress", "run.terminated"]);
		expect(JSON.stringify(observed)).not.toContain("provider_routed");
		expect(JSON.stringify(observed)).not.toContain("usage_observed");
	});

	test("rejects mixed, partial, or impossible streams before invoking the writer", async () => {
		const events = runEvents();
		const other = runEvents(wireRunId("run:other"));
		const duplicateStart = decodeWireEvent({
			...events[0]!,
			id: "@event:ledger:duplicate-start",
			sequence: 1,
			at: "2026-08-10T00:00:01.000Z",
		});
		let writerCalls = 0;
		for (const invalid of [
			[events[0]!, events[2]!],
			[events[0]!, other[1]!, events[2]!],
			[events[0]!, duplicateStart, events[2]!],
			events.slice(0, -1),
		]) {
			try {
				await publishWireEvents(identity, invalid, 100, async () => {
					writerCalls += 1;
					return "recorded";
				});
			} catch (error) {
				expect(error).toBeInstanceOf(WireLedgerError);
				if (!(error instanceof WireLedgerError)) throw error;
				expect(error.code).toBe("invalid_batch");
				continue;
			}
			throw new Error("expected an invalid wire ledger batch");
		}
		expect(writerCalls).toBe(0);
	});
});

function framFixture(): { readonly home: string; readonly bin: string; readonly out: string; readonly server: string } {
	const home = process.env.FRAM_TEST_CHECKOUT
		?? process.env.FRAM_HOME
		?? "/home/tom/code/beagle/main/branch-core";
	return {
		home,
		bin: path.resolve(home, "bin"),
		out: path.resolve(home, "out"),
		server: path.resolve(home, "bin/fram-server"),
	};
}

function unusedPort(): number {
	const server = Bun.serve({ port: 0, fetch: () => new Response("unused") });
	const port = server.port;
	server.stop(true);
	return port;
}

async function waitForFram(port: number, spaceId: string): Promise<FramRpcClient> {
	for (let attempt = 0; attempt < 400; attempt += 1) {
		try {
			return await FramRpcClient.connect({
				port,
				spaceId,
				connectTimeoutMs: 100,
				readTimeoutMs: 500,
				maxAttempts: 1,
				retryDelayMs: 0,
				jitterMs: 0,
			});
		} catch {}
		await Bun.sleep(25);
	}
	throw new Error("isolated Fram server did not become ready");
}

test("Clojure accepts exact essential events with ECMAScript numbers and retries them idempotently", async () => {
	const fram = framFixture();
	if (!(await Bun.file(fram.server).exists())) {
		throw new Error("frozen Fram server is unavailable for the wire-ledger fixture");
	}
	const root = await fs.mkdtemp(path.join(os.tmpdir(), "north-wire-ledger-"));
	roots.push(root);
	const port = unusedPort();
	const spaceId = "north-wire-ledger-v2";
	const environment = {
		...process.env,
		NORTH_PORT: String(port),
		FRAM_LOG: path.join(root, "history.framlog"),
		FRAM_SPACE_ID: spaceId,
		FRAM_HOME: fram.home,
		FRAM_BIN: fram.bin,
		FRAM_OUT: fram.out,
		NORTH_TELEMETRY_PARTITION: "0",
	};
	const server = Bun.spawn([
		fram.server,
		"serve",
		String(port),
		environment.FRAM_LOG,
		spaceId,
	], {
		cwd: fram.home,
		env: {
			...environment,
			FRAM_SERVER_RUNTIME: "jvm-dev",
			FRAM_SERVER_QUIET: "1",
			FRAM_SERVER_XMX: "1g",
			FRAM_SNAPSHOT_BOOT: "0",
		},
		stdout: "ignore",
		stderr: "ignore",
	});
	let client: FramRpcClient | undefined;
	try {
		client = await waitForFram(port, spaceId);
		const invalidRunId = "run:self-consistent-invalid";
		const invalid = [
			uncheckedProjection({
				at: "2026-08-10T00:00:00.000Z",
				essential: true,
				id: "event:invalid:0",
				kind: "run.started",
				lifecycle: "waiting",
				requiredSemantics: [],
				runId: invalidRunId,
				sequence: 0,
				version: "north:wire:v2",
			}),
			uncheckedProjection({
				at: "2026-08-10T00:00:01.000Z",
				essential: true,
				id: "event:invalid:1",
				kind: "run.terminated",
				lifecycle: "completed",
				requiredSemantics: [],
				runId: invalidRunId,
				sequence: 1,
				version: "north:wire:v2",
			}),
		] as const;
		const beforeInvalid = await client.version();
		const invalidWriter = Bun.spawn([
			"bb",
			"-cp",
			fram.out,
			path.resolve(import.meta.dir, "../../cli/run-event-internal.clj"),
			String(port),
		], {
			env: environment,
			stdin: "pipe",
			stdout: "ignore",
			stderr: "ignore",
		});
		invalidWriter.stdin.write(JSON.stringify(invalid));
		invalidWriter.stdin.end();
		expect(await invalidWriter.exited).not.toBe(0);
		expect((await client.version()).servedVersion).toBe(beforeInvalid.servedVersion);
		for (const projection of invalid) {
			expect((await client.scanAll(projection.subject, null, null)).rows).toHaveLength(0);
		}

		const maximumRunId = wireRunId(`run:${"x".repeat(252)}`);
		expect(maximumRunId.length).toBe(256);
		const events = runEvents(maximumRunId, 10_000_015);
		const projections = events.map((event) => wireEventFacts(identity, event));
		const before = await client.version();
		expect(await recordWireEventProjections(projections, 4_000, environment)).toBe("recorded");
		const once = await client.version();
		expect(once.servedVersion - before.servedVersion).toBe(events.length);
		expect(await recordWireEventProjections(projections, 4_000, environment)).toBe("recorded");
		const twice = await client.version();
		expect(twice.servedVersion).toBe(once.servedVersion);

		for (const projection of projections) {
			const rows = await client.scanAll(projection.subject, null, null);
			const actual = rows.rows.map((row) => {
				expect(row).toBeInstanceOf(FramTriple);
				const fact = row as FramTriple;
				return JSON.stringify([fact.t2, fact.t3]);
			}).sort();
			const expected = projection.facts.map((fact) => JSON.stringify(fact)).sort();
			expect(actual).toEqual(expected);
		}

		const conflicting = projections.map((projection, index) => index === 1
			? Object.freeze({
				subject: projection.subject,
				facts: Object.freeze(projection.facts.map(([predicate, value]) =>
					[predicate, predicate === "thread" ? "@conflicting-thread" : value] as const)),
			})
			: projection);
		expect(await recordWireEventProjections(conflicting, 4_000, environment)).toBe("unavailable");
		expect((await client.version()).servedVersion).toBe(twice.servedVersion);

		const summary = wireLedgerSummary(events);
		const reduced = reduceWireEvents(events);
		const tools = Array.from({ length: 512 }, (_, index) => {
			const tool = `inspect_${String(index).padStart(3, "0")}_${"x".repeat(108)}`;
			return { server: "fixture", tool, count: 1 } as const;
		});
		const operationReceipts = tools.map(({ server, tool }) => ({
			tool: `${server}/${tool}`,
			operation: "reasoning.inspect",
			durationMs: 1,
			resultSize: 1,
			outcome: "ok",
		}));
		const telemetry = wireRunTelemetryFacts(
			identity,
			reduced,
			{ status: "recorded", summary },
			{
				shadowReviewerSummary: {
					version: SHADOW_REVIEWER_VERSION,
					targetId: "reviewer-standard",
					status: "partial",
					eligibleUpdates: 3,
					reviewedUpdates: 2,
					droppedUpdates: 1,
					emittedNotes: 1,
					quarantinedOutputs: 0,
					failedReviews: 0,
					usageStatus: "exact",
					tokens: 77,
					durationMs: 19,
				},
				runEstimate: {
					hours: "2.7777777777777777",
					durationMs: 10_000_000,
				},
				judgmentGrade: { grade: "m", status: "valid", source: "thread" },
				struggleObservation: {
					policyVersion: "north:struggle-observer:v2",
					topology: "worker",
					errorStreakThreshold: 3,
					loopRepeatThreshold: 3,
					loopWindow: 20,
					noProgressTurnThreshold: 6,
					errorCount: 0,
					triggers: [],
				},
				mcpActivity: {
					source: "fixture",
					coverage: "exact",
					totalCalls: 512,
					tools,
					operationReceipts,
					operationAggregates: [{
						operation: "reasoning.inspect",
						count: 512,
						totalDurationMs: 512,
						meanDurationMs: 1,
						failureCount: 0,
					}],
				},
			},
		);
		expect(new TextEncoder().encode(JSON.stringify(telemetry.facts)).byteLength)
			.toBeGreaterThan(128 * 1024);
		expect(telemetry.facts).toContainEqual(["estimate_ratio", "1.000002"]);
		expect(telemetry.facts).toContainEqual(["judgment_grade", "m"]);
		expect(telemetry.facts).toContainEqual(["shadow_reviewer_usage_status", "exact"]);
		expect(telemetry.facts).toContainEqual(["shadow_reviewer_tokens", "77"]);
		const runFactWriter = async (
			facts: readonly (readonly [string, string])[],
			subject = telemetry.subject,
		) => {
			const child = Bun.spawn([
				"bb",
				"-cp",
				fram.out,
				path.resolve(import.meta.dir, "../../cli/run-fact-internal.clj"),
				String(port),
				subject,
			], {
				env: environment,
				stdin: "pipe",
				stdout: "ignore",
				stderr: "pipe",
			});
			child.stdin.write(JSON.stringify(facts));
			child.stdin.end();
			const [exitCode, stderr] = await Promise.all([
				child.exited,
				new Response(child.stderr).text(),
			]);
			return { exitCode, stderr };
		};
		for (const [predicate, value] of [
			["wire_run_lifecycle", "failed"],
			["outcome", "provider_error"],
			["lifetime_input_tokens", "999"],
			["tool_admitted_count", "1"],
			["compaction_count", "9"],
			["model_tier", "frontier"],
			["effort", "max"],
			["estimate_delta_ms", "0"],
			["estimate_ratio", "1.000001"],
			["struggle_loop_repeat_threshold", "21"],
			["judgment_grade_status", "unavailable"],
			["usage_total_status", "exact"],
			["tokens", "19"],
			["turn_provenance", "provider-terminal"],
			["provider_turn_units", "1"],
			["provider_join_key_version", "north-provider-join:v1"],
			["watchdog_reason", "north_watchdog_execution_inactivity"],
			["shadow_reviewer_version", "north-shadow-reviewer:v2"],
			["shadow_reviewer_reviewed_updates", "4"],
			["shadow_reviewer_usage_status", "partial"],
		] as const) {
			const forged = telemetry.facts.some(([candidate]) => candidate === predicate)
				? telemetry.facts.map((fact) => fact[0] === predicate
					? [predicate, value] as const : fact)
				: [...telemetry.facts, [predicate, value] as const];
			const beforeForgery = await client.version();
			expect((await runFactWriter(forged)).exitCode).not.toBe(0);
			expect((await client.version()).servedVersion).toBe(beforeForgery.servedVersion);
			expect((await client.scanAll(telemetry.subject, null, null)).rows).toHaveLength(0);
		}
		const identityForgeries = [
			telemetry.facts.map((fact) => fact[0] === "parent_thread"
				? ["parent_thread", "@forged-parent"] as const : fact),
			telemetry.facts.filter(([predicate]) => predicate !== "parent_thread"),
			telemetry.facts.map((fact) => fact[0] === "run_coordinator"
				? ["run_coordinator", "forged-coordinator"] as const : fact),
			telemetry.facts.filter(([predicate]) => predicate !== "run_coordinator"),
		] as const;
		for (const forged of identityForgeries) {
			const beforeForgery = await client.version();
			expect((await runFactWriter(forged)).exitCode).not.toBe(0);
			expect((await client.version()).servedVersion).toBe(beforeForgery.servedVersion);
			expect((await client.scanAll(telemetry.subject, null, null)).rows).toHaveLength(0);
		}
		const directValid = await runFactWriter(telemetry.facts);
		expect(directValid.exitCode, directValid.stderr).toBe(0);
		expect(await recordWireRunTelemetryProjection(telemetry, 4_000, environment)).toBe("recorded");
		const runRows = await client.scanAll(telemetry.subject, null, null);
		const runFacts = new Map(runRows.rows.map((row) => {
			const fact = row as FramTriple;
			return [String(fact.t2), String(fact.t3)] as const;
		}));
		expect(runFacts.get("wire_ledger_status")).toBe("complete");
		expect(runFacts.get("wire_ledger_sha256")).toBe(ledgerDigest(events));
		expect(runFacts.get("wire_event_count")).toBe(String(events.length));
		expect(runFacts.get("usage_terminal_count")).toBe("0");
		expect(runFacts.get("usage_scope")).toBe("wire_run_cumulative");
		expect(runFacts.get("usage_total_status")).toBe("partial");
		expect(runFacts.has("tokens")).toBe(false);
		expect(runFacts.get("shadow_reviewer_status")).toBe("partial");
		expect(runFacts.get("shadow_reviewer_tokens")).toBe("77");
		expect(runRows.rows.filter((row) =>
			row instanceof FramTriple && row.t2 === "mcp_operation_receipt")).toHaveLength(512);

		const shadowSourceRunId = wireRunId("run:ledger-shadow-source");
		const shadowWriter = new WireEventWriter({
			runId: wireRunId("run:ledger-shadow-reviewer"),
		});
		const shadowCallId = wireModelCallId("model-call:ledger-shadow-reviewer");
		shadowWriter.append({
			kind: "run.started",
			lifecycle: "running",
			parentRunId: shadowSourceRunId,
		});
		shadowWriter.append({
			kind: "model-call.started",
			modelCallId: shadowCallId,
			model: { provider: "anthropic", tier: "standard" },
			attempt: 1,
		});
		shadowWriter.append({
			kind: "model-call.completed",
			modelCallId: shadowCallId,
			status: "succeeded",
			origin: "provider",
			usageCoverage: "exact",
			usage: {
				lifetime: {
					inputTokens: 7,
					outputTokens: 2,
					cacheReadTokens: 1,
					cacheWriteTokens: 0,
					reasoningTokens: 0,
					modelCalls: 1,
				},
				context: { tokens: 8 },
			},
			evidence: {
				turns: { unit: "provider-turn", count: 1, toolItems: 0, comparable: false },
			},
		});
		shadowWriter.terminate({ lifecycle: "completed", reason: { code: "completed" } });
		const shadowEvents = shadowWriter.events();
		expect(await recordWireEventProjections(
			shadowEvents.map((event) => wireEventFacts(identity, event)),
			4_000,
			environment,
		)).toBe("recorded");
		const shadowTelemetry = wireRunTelemetryFacts(
			identity,
			reduceWireEvents(shadowEvents),
			{ status: "recorded", summary: wireLedgerSummary(shadowEvents) },
			{
				role: "shadow-reviewer",
				provider: "anthropic",
				providerTarget: "reviewer-standard",
				shadowReviewerExecution: {
					version: SHADOW_REVIEWER_VERSION,
					targetId: "reviewer-standard",
					sourceRunId: shadowSourceRunId,
					sourceFromSequence: 2,
					sourceThroughSequence: 8,
					privacyOmittedEvents: 1,
					capacityOmittedEvents: 4,
					inputSha256: "a".repeat(64),
				},
			},
		);
		expect(shadowTelemetry.facts).toContainEqual([
			"shadow_reviewer_source_run",
			shadowSourceRunId,
		]);
		expect(shadowTelemetry.facts).toContainEqual(["shadow_reviewer_input_sha256", "a".repeat(64)]);
		expect(shadowTelemetry.facts).toContainEqual(["parent_run", "@run:ledger-shadow-source"]);
		for (const [predicate, value] of [
			["role", "worker"],
			["shadow_reviewer_source_run", "run:another-source"],
			["shadow_reviewer_input_sha256", "not-a-digest"],
		] as const) {
			const forged = shadowTelemetry.facts.map((fact) => fact[0] === predicate
				? [predicate, value] as const : fact);
			expect((await runFactWriter(forged, shadowTelemetry.subject)).exitCode).not.toBe(0);
			expect((await client.scanAll(shadowTelemetry.subject, null, null)).rows).toHaveLength(0);
		}
		const shadowValid = await runFactWriter(shadowTelemetry.facts, shadowTelemetry.subject);
		expect(shadowValid.exitCode, shadowValid.stderr).toBe(0);
		const shadowRows = await client.scanAll(shadowTelemetry.subject, null, null);
		const shadowFacts = new Map(shadowRows.rows.map((row) => {
			const fact = row as FramTriple;
			return [String(fact.t2), String(fact.t3)] as const;
		}));
		expect(shadowFacts.get("role")).toBe("shadow-reviewer");
		expect(shadowFacts.get("shadow_reviewer_source_run")).toBe(shadowSourceRunId);
		expect(shadowFacts.get("shadow_reviewer_capacity_omitted_events")).toBe("4");

		const providerTurnWriter = new WireEventWriter({
			runId: wireRunId("run:ledger-provider-turn-units"),
		});
		const providerTurnCallId = wireModelCallId("model-call:ledger-provider-turn-units");
		providerTurnWriter.append({ kind: "run.started", lifecycle: "running" });
		providerTurnWriter.append({
			kind: "model-call.started",
			modelCallId: providerTurnCallId,
			model: { provider: "openai", tier: "standard" },
			attempt: 1,
		});
		providerTurnWriter.append({
			kind: "model-call.completed",
			modelCallId: providerTurnCallId,
			status: "succeeded",
			origin: "provider",
			usageCoverage: "exact",
			usage: {
				lifetime: {
					inputTokens: 12,
					outputTokens: 4,
					cacheReadTokens: 3,
					cacheWriteTokens: 0,
					reasoningTokens: 1,
					modelCalls: 1,
				},
				context: { tokens: 9 },
			},
			evidence: {
				turns: { unit: "provider-turn", count: 3, toolItems: 4, comparable: false },
			},
		});
		providerTurnWriter.terminate({ lifecycle: "completed", reason: { code: "completed" } });
		const providerTurnEvents = providerTurnWriter.events();
		expect(await recordWireEventProjections(
			providerTurnEvents.map((event) => wireEventFacts(identity, event)),
			4_000,
			environment,
		)).toBe("recorded");
		const providerTurnTelemetry = wireRunTelemetryFacts(
			identity,
			reduceWireEvents(providerTurnEvents),
			{ status: "recorded", summary: wireLedgerSummary(providerTurnEvents) },
			{},
		);
		expect(providerTurnTelemetry.facts.filter(
			([predicate]) => predicate === "provider_turn_units",
		)).toEqual([["provider_turn_units", "3"]]);
		expect(providerTurnTelemetry.facts.filter(
			([predicate]) => predicate === "provider_tool_items",
		)).toEqual([["provider_tool_items", "4"]]);
		expect(providerTurnTelemetry.facts.filter(
			([predicate]) => predicate === "provider_turn_metric_comparable",
		)).toEqual([["provider_turn_metric_comparable", "false"]]);
		const providerTurnValid = await runFactWriter(
			providerTurnTelemetry.facts,
			providerTurnTelemetry.subject,
		);
		expect(providerTurnValid.exitCode, providerTurnValid.stderr).toBe(0);
		const providerTurnRows = await client.scanAll(providerTurnTelemetry.subject, null, null);
		for (const [predicate, value] of [
			["provider_turn_units", "3"],
			["provider_tool_items", "4"],
			["provider_turn_metric_comparable", "false"],
		] as const) {
			expect(providerTurnRows.rows.filter((row) => row instanceof FramTriple
				&& row.t2 === predicate && row.t3 === value)).toHaveLength(1);
		}
	} finally {
		client?.close();
		server.kill("SIGTERM");
		await server.exited;
	}
}, 60_000);
