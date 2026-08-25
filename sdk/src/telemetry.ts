import * as path from "node:path";

import {
	beagleStoreBabashkaArguments,
	beagleStoreEnvironment,
	settleBeagleStoreCoordinatorChild,
} from "./beagle-store";
import {
	AGENT_RUN_LEDGER_CONTRACT,
	AGENT_RUN_LEDGER_VERSION,
	isWireRunLedgerSummary,
	wireRunLedgerIdentity,
	type WireLedgerPublicationStatus,
	type WireRunLedgerIdentity,
	type WireRunLedgerSummary,
} from "./run-ledger";
import {
	wireRunProvenanceFacts,
	type WireRunProvenance,
} from "./run-provenance";
import { tokenTotalLiteral } from "./usage";
import {
	executionObservationJson,
	normalizeExecutionObservation,
	unknownExecutionObservation,
} from "./execution-observation";
import { foldProviderJoinEvidence } from "./providers/provider-join";
import {
	WIRE_PROVIDER_JOIN_VERSION,
	WIRE_VERSION,
	type WireAbortActivityEvidence,
	type WireRunLifecycle,
} from "./wire/events";
import { wireRunId, type WireRunId } from "./wire/ids";
import type { WireModelCallSnapshot, WireRunSnapshot } from "./wire/reducer";

const REPO = path.resolve(import.meta.dir, "../..");
const INTERNAL_WRITER = path.resolve(REPO, "cli/run-fact-internal.clj");
const TERMINAL_COORDINATOR_READ_TIMEOUT_MS = 70_000;
const RUN_WRITE_TIMEOUT_MS = (() => {
	const raw = Number(process.env.NORTH_RUN_WRITE_TIMEOUT_MS);
	return Number.isFinite(raw) && raw > 0 ? raw : 120_000;
})();

export type RunPublicationStatus = WireLedgerPublicationStatus;

export interface RecordedWireRunLedger {
	readonly status: "recorded";
	readonly summary: WireRunLedgerSummary;
}

export interface WireRunTelemetryProjection {
	readonly subject: string;
	readonly facts: readonly (readonly [string, string])[];
}

export type WireRunTelemetryWriter = (
	projection: WireRunTelemetryProjection,
	timeoutMs: number,
) => Promise<RunPublicationStatus>;

/** Keep an explicit caller deadline authoritative for terminal publication. */
export function applyTerminalCoordinatorReadTimeout(
	env: NodeJS.ProcessEnv = process.env,
): void {
	if (env.NORTH_COORD_READ_TIMEOUT_MS === undefined) {
		env.NORTH_COORD_READ_TIMEOUT_MS = String(TERMINAL_COORDINATOR_READ_TIMEOUT_MS);
	}
}

function durationMs(snapshot: WireRunSnapshot): number {
	const duration = Date.parse(snapshot.updatedAt) - Date.parse(snapshot.startedAt);
	if (!Number.isSafeInteger(duration) || duration < 0) {
		throw new TypeError("wire run snapshot has an invalid duration");
	}
	return duration;
}

function outcome(lifecycle: WireRunLifecycle, terminationCode: string | undefined): string {
	if (lifecycle === "completed") return "ran";
	if (lifecycle === "cancelled") return terminationCode ?? "cancelled";
	if (lifecycle === "blocked") return terminationCode ?? "blocked";
	if (lifecycle === "failed") return terminationCode ?? "failed";
	throw new TypeError("wire run telemetry requires a terminal snapshot");
}

function countTools(snapshot: WireRunSnapshot, status?: string): number {
	const tools = Object.values(snapshot.toolCalls);
	return status === undefined ? tools.length : tools.filter((tool) => tool.status === status).length;
}

function completedModelCalls(snapshot: WireRunSnapshot): readonly WireModelCallSnapshot[] {
	return Object.values(snapshot.modelCalls)
		.filter((modelCall) => modelCall.status !== "running");
}

function safeEvidenceSum(values: readonly number[]): number | undefined {
	let total = 0;
	for (const value of values) {
		total += value;
		if (!Number.isSafeInteger(total)) return undefined;
	}
	return total;
}

function wireCompletionEvidenceFacts(
	snapshot: WireRunSnapshot,
): readonly (readonly [string, string])[] {
	const modelCalls = Object.values(snapshot.modelCalls);
	const completed = completedModelCalls(snapshot);
	const providerCompleted = completed.filter((modelCall) => modelCall.origin === "provider");
	const joinEvidence = completed.flatMap((modelCall) => {
		const evidence = modelCall.evidence?.providerJoin;
		return evidence === undefined ? [] : [evidence];
	});
	const foldedJoin = foldProviderJoinEvidence(joinEvidence);
	const join = foldedJoin !== undefined
		&& joinEvidence.length < completed.length
		&& foldedJoin.coverage === "exact"
		? Object.freeze({ ...foldedJoin, coverage: "partial" as const })
		: foldedJoin;
	const providers = new Set(providerCompleted.map((modelCall) => modelCall.model.provider));
	if (providers.size > 1) {
		throw new TypeError("provider terminal evidence conflicts within one Wire run");
	}
	const facts: Array<readonly [string, string]> = [
		["provider_session_persistence", join?.sessionPersistence ?? "unknown"],
	];
	if (join !== undefined) {
		facts.push(["provider_join_key_version", WIRE_PROVIDER_JOIN_VERSION]);
		facts.push(["provider_join_coverage", join.coverage]);
		if (join.sessionKey !== undefined) facts.push(["provider_session_key", join.sessionKey]);
		for (const key of join.turnKeys) facts.push(["provider_turn_key", key]);
	}
	const allProviderTerminal = modelCalls.length > 0
		&& modelCalls.every((modelCall) => modelCall.status !== "running"
			&& modelCall.origin === "provider");
	if (allProviderTerminal
		&& modelCalls.every((modelCall) => modelCall.evidence?.providerDurationMs !== undefined)) {
		const providerDurationMs = safeEvidenceSum(
			modelCalls.map((modelCall) => modelCall.evidence!.providerDurationMs!),
		);
		if (providerDurationMs !== undefined) {
			facts.push(["provider_duration_ms", String(providerDurationMs)]);
		}
	}
	const turnEvidence = providerCompleted.flatMap((modelCall) => {
		const turns = modelCall.evidence?.turns;
		return turns === undefined ? [] : [turns];
	});
	const turnUnits = new Set(turnEvidence.map((turns) => turns.unit));
	if (turnUnits.size > 1) {
		throw new TypeError("provider turn evidence uses incompatible units within one Wire run");
	}
	const witnessedTurns = allProviderTerminal && turnEvidence.length === modelCalls.length;
	const turnCount = witnessedTurns
		? safeEvidenceSum(turnEvidence.map((turns) => turns.count)) : undefined;
	const exactTurns = witnessedTurns && turnCount !== undefined;
	if (!exactTurns) {
		const preProvider = snapshot.lifecycle === "blocked"
			&& modelCalls.length === 0;
		facts.push(["turn_provenance", preProvider ? "pre-provider" : "unknown"]);
		if (preProvider) facts.push(["num_turns", "0"]);
		return facts;
	}
	facts.push(["turn_provenance", "provider-terminal"]);
	if (turnEvidence[0]?.unit === "assistant-turn") {
		facts.push(["num_turns", String(turnCount)]);
	} else if (turnEvidence[0]?.unit === "provider-turn") {
		facts.push(["provider_turn_units", String(turnCount)]);
		if (turnEvidence.every((turns) => turns.unit === "provider-turn"
			&& turns.toolItems !== undefined)) {
			const toolItems = safeEvidenceSum(
				turnEvidence.map((turns) => turns.unit === "provider-turn" ? turns.toolItems! : 0),
			);
			if (toolItems !== undefined) facts.push(["provider_tool_items", String(toolItems)]);
		}
		facts.push(["provider_turn_metric_comparable", "false"]);
	}
	return facts;
}

function wireWatchdogFacts(
	snapshot: WireRunSnapshot,
): readonly (readonly [string, string])[] {
	const watchdog = snapshot.abort?.watchdog;
	if (watchdog === undefined) return [];
	const activity = <Origin extends "outer" | "provider">(
		value: WireAbortActivityEvidence<Origin> | undefined,
	): string => value === undefined ? "none" : JSON.stringify(value);
	return [
		["watchdog_reason", snapshot.abort!.reason],
		["watchdog_silence_ms", String(watchdog.silenceMs)],
		["watchdog_last_outer_activity", activity(watchdog.lastOuter)],
		["watchdog_last_provider_activity", activity(watchdog.lastProvider)],
	];
}

function unavailableExecutionObservationSource(
	snapshot: WireRunSnapshot,
	provenance: WireRunProvenance,
): string {
	if (provenance.executionSource === "provider-native"
		&& provenance.provider === "openai") {
		return "codex_rollout_initial_mode_or_join_unavailable";
	}
	const provider = snapshot.model?.provider ?? provenance.provider;
	if (provider === "openai") return "codex_app_server_mode_unavailable";
	if (provider === "anthropic") return "anthropic_execution_mode_unsupported";
	return "execution_mode_telemetry_unavailable";
}

function sha256(value: string): string {
	return new Bun.CryptoHasher("sha256").update(value).digest("hex");
}

function wireRunSubject(runId: WireRunId): string {
	if (/^run:[A-Za-z0-9][A-Za-z0-9._:-]*$/u.test(runId)) return `@${runId}`;
	return `@run:wire-summary-${sha256(`north-wire-run-summary-subject:v2\0${runId}`)}`;
}

function assertLedgerMatchesSnapshot(
	snapshot: WireRunSnapshot,
	ledger: RecordedWireRunLedger,
): void {
	if (!isWireRunLedgerSummary(ledger.summary)
		|| ledger.summary.runId !== snapshot.runId
		|| ledger.summary.lastSequence !== snapshot.lastSequence
		|| ledger.summary.terminalEventId !== snapshot.lastEventId
		|| ledger.summary.eventCount !== snapshot.lastSequence + 1) {
		throw new TypeError("recorded wire ledger summary does not match the run snapshot");
	}
	if (!["completed", "failed", "cancelled", "blocked"].includes(snapshot.lifecycle)
		|| snapshot.termination === undefined) {
		throw new TypeError("wire run telemetry requires a terminated snapshot");
	}
}

export function wireRunTelemetryFacts(
	identity: WireRunLedgerIdentity,
	snapshot: WireRunSnapshot,
	ledger: RecordedWireRunLedger,
	provenance: WireRunProvenance,
): WireRunTelemetryProjection {
	const context = wireRunLedgerIdentity(identity);
	assertLedgerMatchesSnapshot(snapshot, ledger);
	if (provenance.provider !== undefined && snapshot.model?.provider !== undefined
		&& provenance.provider !== snapshot.model.provider) {
		throw new TypeError("run provenance provider differs from the reduced wire snapshot");
	}
	const actualDurationMs = durationMs(snapshot);
	const usage = snapshot.usage;
	const exactTokenTotal = tokenTotalLiteral(snapshot);
	const facts: Array<readonly [string, string]> = [
		["kind", "run"],
		["wire_run_id", snapshot.runId],
		["thread", context.thread],
		["thread_provenance", context.thread === "(ad-hoc)" ? "ad-hoc" : "exact"],
		["agent", context.agent],
		["wire_ledger_version", AGENT_RUN_LEDGER_VERSION],
		["wire_version", WIRE_VERSION],
		["wire_ledger_status", "complete"],
		["wire_event_count", String(ledger.summary.eventCount)],
		["wire_event_first_sequence", String(ledger.summary.firstSequence)],
		["wire_event_last_sequence", String(ledger.summary.lastSequence)],
		["wire_terminal_event_id", ledger.summary.terminalEventId],
		["wire_ledger_sha256", ledger.summary.digest],
		["wire_run_lifecycle", snapshot.lifecycle],
		["wire_termination_code", snapshot.termination!.code],
		["outcome", outcome(snapshot.lifecycle, snapshot.termination?.code)],
		["at", snapshot.updatedAt],
		["started_at", snapshot.startedAt],
		["duration_ms", String(actualDurationMs)],
		["lifetime_input_tokens", String(usage.lifetime.inputTokens)],
		["lifetime_output_tokens", String(usage.lifetime.outputTokens)],
		["lifetime_cache_read_tokens", String(usage.lifetime.cacheReadTokens)],
		["lifetime_cache_write_tokens", String(usage.lifetime.cacheWriteTokens)],
		["lifetime_reasoning_tokens", String(usage.lifetime.reasoningTokens)],
		["model_call_count", String(usage.lifetime.modelCalls)],
		["usage_terminal_count", String(snapshot.usageCoverage.providerTerminalCount)],
		["usage_scope", snapshot.usageCoverage.scope],
		["usage_total_status", snapshot.usageCoverage.totalStatus],
		["context_tokens", String(usage.context.tokens)],
		["compaction_count", String(snapshot.compactions)],
		["tool_admitted_count", String(countTools(snapshot))],
		["tool_succeeded_count", String(countTools(snapshot, "succeeded"))],
		["tool_failed_count", String(countTools(snapshot, "failed"))],
		["tool_cancelled_count", String(countTools(snapshot, "cancelled"))],
		["tool_synthetic_failure_count", String(countTools(snapshot, "synthetic_failure"))],
		[
			"execution_observation",
			executionObservationJson(provenance.executionObservation === undefined
				? unknownExecutionObservation(unavailableExecutionObservationSource(snapshot, provenance))
				: normalizeExecutionObservation(provenance.executionObservation)),
		],
		...wireCompletionEvidenceFacts(snapshot),
		...wireWatchdogFacts(snapshot),
	];
	if (exactTokenTotal !== undefined) facts.push(["tokens", exactTokenTotal]);
	if (usage.context.window !== undefined) {
		facts.push(["context_window_tokens", String(usage.context.window)]);
	}
	if (snapshot.parentRunId !== undefined) {
		facts.push(["parent_run", wireRunSubject(snapshot.parentRunId)]);
	}
	if (context.parentThread !== undefined) facts.push(["parent_thread", context.parentThread]);
	if (context.coordinator !== undefined) facts.push(["run_coordinator", context.coordinator]);
	if (snapshot.owner !== undefined) facts.push(["run_owner", snapshot.owner]);
	if (snapshot.model?.tier !== undefined) facts.push(["model_tier", snapshot.model.tier]);
	if (snapshot.model?.capabilityClass !== undefined) {
		facts.push(["capability_class", snapshot.model.capabilityClass]);
	}
	if (snapshot.effort !== undefined) facts.push(["effort", snapshot.effort]);
	const provenanceFacts = wireRunProvenanceFacts(provenance, actualDurationMs);
	for (const [predicate, value] of provenanceFacts) {
		if (predicate === "provider" && snapshot.model?.provider !== undefined) continue;
		if (predicate === "prompt_capability_class"
			&& snapshot.model?.capabilityClass !== undefined
			&& value !== snapshot.model.capabilityClass) {
			throw new TypeError("prompt capability class differs from the reduced wire snapshot");
		}
		facts.push([predicate, value]);
	}
	if (snapshot.model?.provider !== undefined) facts.push(["provider", snapshot.model.provider]);
	return Object.freeze({
		subject: wireRunSubject(snapshot.runId),
		facts: Object.freeze(facts),
	});
}

async function runTelemetryWriter(
	projection: WireRunTelemetryProjection,
	timeoutMs: number,
	environment: NodeJS.ProcessEnv,
): Promise<RunPublicationStatus> {
	const env = beagleStoreEnvironment(environment);
	applyTerminalCoordinatorReadTimeout(env);
	const payload = JSON.stringify(projection.facts);
	if (new TextEncoder().encode(payload).byteLength
		> AGENT_RUN_LEDGER_CONTRACT.bounds.maxTelemetryProjectionBytes) {
		throw new RangeError("wire run telemetry projection exceeds its byte bound");
	}
	const child = Bun.spawn([
		"bb",
		...beagleStoreBabashkaArguments([
			INTERNAL_WRITER,
			environment.NORTH_PORT ?? "7977",
			projection.subject,
		], environment),
	], { env, stdin: "pipe", stdout: "ignore", stderr: "ignore" });
	child.stdin.write(payload);
	child.stdin.end();
	const result = await settleBeagleStoreCoordinatorChild(child, timeoutMs);
	return !result.timedOut && result.exitCode === 0 ? "recorded" : "unavailable";
}

export async function recordWireRunTelemetryProjection(
	projection: WireRunTelemetryProjection,
	timeoutMs = RUN_WRITE_TIMEOUT_MS,
	env: NodeJS.ProcessEnv = process.env,
): Promise<RunPublicationStatus> {
	try {
		return await runTelemetryWriter(projection, timeoutMs, env);
	} catch {
		return "unavailable";
	}
}

export async function recordWireRunTelemetry(
	identity: WireRunLedgerIdentity,
	snapshot: WireRunSnapshot,
	ledger: RecordedWireRunLedger,
	provenance: WireRunProvenance,
	timeoutMs = RUN_WRITE_TIMEOUT_MS,
	writer: WireRunTelemetryWriter = recordWireRunTelemetryProjection,
): Promise<RunPublicationStatus> {
	const projection = wireRunTelemetryFacts(identity, snapshot, ledger, provenance);
	return writer(projection, timeoutMs);
}

export function newRunId(agent: string): WireRunId {
	if (!/^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/u.test(agent)) {
		throw new TypeError("invalid run agent identity");
	}
	return wireRunId(`run:${agent}-${crypto.randomUUID()}`);
}
