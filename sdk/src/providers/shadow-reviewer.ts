import type { Options } from "@anthropic-ai/claude-agent-sdk";
import * as fs from "node:fs";

import {
	SHADOW_REVIEWER_NOTE_MAX_BYTES,
	SHADOW_REVIEWER_ISSUE_CODES,
	SHADOW_REVIEWER_VERSION,
	shadowReviewerAgentId,
	type ShadowReviewExecution,
	type ShadowReviewerUpdate,
} from "../shadow-reviewer";
import shadowReviewerPrompt from "../shadow-reviewer-prompt.md" with { type: "text" };
import shadowReviewerRoutingText from "../shadow-reviewer-routing.md" with { type: "text" };
import { harnessOptions } from "../harness";
import { admitRoutingRequest } from "../routing-admission";
import type { RoutingDraft, RoutingRequest } from "../routing-metadata";
import {
	publishWireEvents,
	wireLedgerSummary,
	wireRunLedgerIdentity,
} from "../run-ledger";
import { StreamWriter } from "../stream-writer";
import { newRunId, recordWireRunTelemetry } from "../telemetry";
import { normalizeUsage } from "../usage";
import {
	WireEventWriter,
	WIRE_PROVIDER_JOIN_VERSION,
	encodeWireJsonlLine,
	wireQueryRoute,
	wireRunId,
	type WireCompletionEvidence,
	type WireEvent,
	type WireKnownEvent,
	type WireQuery,
	type WireRunId,
} from "../wire";
import { createAnthropicQuery } from "./anthropic";
import type { AgentProviderQuery, RoutingTarget } from "./types";

const OUTPUT_MAX_BYTES = SHADOW_REVIEWER_NOTE_MAX_BYTES + 512;
let admittedRouting: RoutingRequest | undefined;

export function shadowReviewerRouting(): RoutingRequest {
	admittedRouting ??= admitRoutingRequest(
		JSON.parse(shadowReviewerRoutingText) as unknown as RoutingDraft,
		"shadow reviewer routing",
	);
	return admittedRouting;
}

const OUTPUT_SCHEMA = Object.freeze({
	oneOf: [
		{
			type: "object",
			additionalProperties: false,
			required: ["kind"],
			properties: { kind: { const: "none" } },
		},
		{
			type: "object",
			additionalProperties: false,
			required: ["kind", "severity", "issueCode", "sourceSequence"],
			properties: {
				kind: { const: "note" },
				severity: { enum: ["nit", "blocker"] },
				issueCode: { enum: SHADOW_REVIEWER_ISSUE_CODES },
				sourceSequence: { type: "integer", minimum: 0 },
			},
		},
	],
});

interface ShadowReviewerTranscript {
	writeWireEvent(event: WireEvent): Promise<WireEvent>;
	close(): Promise<void>;
}

type ShadowReviewerQueryFactory = (
	args: AgentProviderQuery,
	admitted: boolean,
) => WireQuery;

export interface AnthropicShadowReviewerRuntime {
	readonly createQuery?: ShadowReviewerQueryFactory;
	readonly openTranscript?: (agentId: string) => Promise<ShadowReviewerTranscript>;
	readonly publishEvents?: typeof publishWireEvents;
	readonly recordTelemetry?: typeof recordWireRunTelemetry;
	readonly createRunId?: (agentId: string) => WireRunId;
	readonly nowMs?: () => number;
}

export interface AnthropicShadowReviewInput {
	readonly update: ShadowReviewerUpdate;
	readonly target: RoutingTarget;
	readonly sourceAgentId: string;
	readonly thread: string;
	readonly parentThread?: string;
	readonly coordinator?: string;
	readonly signal: AbortSignal;
}

function byteLength(value: string): number {
	return new TextEncoder().encode(value).byteLength;
}

function knownWireEvent(event: WireEvent): event is WireKnownEvent {
	return event.version === "north:wire:v2" && event.essential;
}

function boundedOutputPart(
	current: string,
	part: string,
): { readonly output: string; readonly overflow: boolean } {
	if (byteLength(current) + byteLength(part) <= OUTPUT_MAX_BYTES) {
		return { output: current + part, overflow: false };
	}
	return { output: current, overflow: true };
}

function ephemeralEvidence(
	evidence: WireCompletionEvidence | undefined,
): WireCompletionEvidence {
	return Object.freeze({
		...(evidence ?? {}),
		providerJoin: Object.freeze({
			...(evidence?.providerJoin ?? {
				version: WIRE_PROVIDER_JOIN_VERSION,
				turnKeys: [],
				coverage: "unknown" as const,
		}),
			sessionPersistence: "ephemeral" as const,
		}),
	});
}

function reviewerOptions(
	signal: AbortSignal,
	reviewerAgentId: string,
	target: RoutingTarget,
	cwd: string,
): {
	readonly options: Options;
	readonly detach: () => void;
} {
	const routing = shadowReviewerRouting();
	const abortController = new AbortController();
	const forwardAbort = () => abortController.abort(signal.reason);
	if (signal.aborted) forwardAbort();
	else signal.addEventListener("abort", forwardAbort, { once: true });
	return { options: harnessOptions({
		self: reviewerAgentId,
		provider: "anthropic",
		routingMetadata: routing,
		abortController,
		cwd,
		maxTurns: 1,
		outputFormat: { type: "json_schema", schema: OUTPUT_SCHEMA },
		persistSession: false,
		systemPrompt: shadowReviewerPrompt,
		presenceRegistrar: false,
		presenceRenewer: false,
		activatedResources: [],
		availableSkills: [],
		dataOnly: true,
		modelAvailability: {
			exactModelPinned: false,
			targetId: target.id,
		},
	}), detach: () => signal.removeEventListener("abort", forwardAbort) };
}

function terminalStatus(
	status: ShadowReviewExecution["status"],
): { readonly lifecycle: "completed" | "failed" | "cancelled"; readonly code: "completed" | "provider_error" | "aborted" } {
	if (status === "succeeded") return { lifecycle: "completed", code: "completed" };
	if (status === "cancelled") return { lifecycle: "cancelled", code: "aborted" };
	return { lifecycle: "failed", code: "provider_error" };
}

/**
 * Run one passive Anthropic review. Provider-normalized assistant text is held
 * only in the in-memory writer; the durable child run is rebuilt from its
 * text-free model lifecycle and usage evidence.
 */
export async function runAnthropicShadowReview(
	input: AnthropicShadowReviewInput,
	runtime: AnthropicShadowReviewerRuntime = {},
): Promise<ShadowReviewExecution> {
	if (input.target.provider !== "anthropic") {
		throw new Error("shadow reviewer target must use the Anthropic provider");
	}
	const reviewerAgentId = shadowReviewerAgentId(input.sourceAgentId);
	const identity = wireRunLedgerIdentity({
		thread: input.thread,
		agent: reviewerAgentId,
		...(input.parentThread === undefined ? {} : { parentThread: input.parentThread }),
		...(input.coordinator === undefined ? {} : { coordinator: input.coordinator }),
	});
	const runId = (runtime.createRunId ?? newRunId)(reviewerAgentId);
	const nowMs = runtime.nowMs ?? Date.now;
	const startedAt = nowMs();
	const transcript = await (runtime.openTranscript
		?? ((agentId: string) => StreamWriter.open(agentId)))(reviewerAgentId);
	const durableWriter = new WireEventWriter({ runId: wireRunId(runId) });
	const rawWriter = new WireEventWriter({ runId: wireRunId(runId) });
	let nextDurableSequence = 0;
	const commitThrough = async (event: WireEvent): Promise<void> => {
		const canonical = durableWriter.events()[event.sequence];
		if (canonical !== event) throw new Error("shadow reviewer durable event is not canonical");
		while (nextDurableSequence <= event.sequence) {
			await transcript.writeWireEvent(durableWriter.events()[nextDurableSequence]!);
			nextDurableSequence += 1;
		}
	};
	const started = durableWriter.append({
		kind: "run.started",
		lifecycle: "running",
		owner: reviewerAgentId,
		parentRunId: input.update.sourceRunId,
	});
	await commitThrough(started);
	rawWriter.append({
		kind: "run.started",
		lifecycle: "running",
		owner: reviewerAgentId,
		parentRunId: input.update.sourceRunId,
	});

	let output = "";
	let unsafeOutput = false;
	let status: ShadowReviewExecution["status"] = "failed";
	let query: WireQuery | undefined;
	let detachAbort: (() => void) | undefined;
	let emptyCwd: string | undefined;
	let toolContractViolation = false;
	let nextRawSequence = 1;
	try {
		if (input.signal.aborted) {
			status = "cancelled";
		} else {
			emptyCwd = await fs.promises.mkdtemp("/tmp/north-shadow-reviewer-");
			const sealed = reviewerOptions(input.signal, reviewerAgentId, input.target, emptyCwd);
			detachAbort = sealed.detach;
			query = (runtime.createQuery ?? createAnthropicQuery)({
				input: input.update.projection,
				options: sealed.options,
				target: input.target,
				context: {
					writer: rawWriter,
					route: wireQueryRoute({
						model: {
							provider: "anthropic",
							tier: "economy",
							capabilityClass: "unknown",
						},
						effort: "low",
						attempt: 1,
					}),
				},
			}, false);
			for await (const event of query) {
				const canonical = rawWriter.events()[nextRawSequence];
				let canonicalMatch = canonical === event;
				if (!canonicalMatch && canonical !== undefined) {
					try { canonicalMatch = encodeWireJsonlLine(canonical) === encodeWireJsonlLine(event); }
					catch { /* Invalid provider output cannot cross the reconstruction boundary. */ }
				}
				if (!canonical || !canonicalMatch) {
					throw new Error("shadow reviewer provider event is not its writer-owned canonical event");
				}
				nextRawSequence += 1;
				if (!knownWireEvent(event)) {
					throw new Error("shadow reviewer provider emitted an opaque event");
				}
				if (event.kind === "message.recorded" && event.role === "assistant"
						&& event.stage === "delta" && event.content !== undefined) {
					if (typeof event.content !== "string") {
						unsafeOutput = true;
					} else {
						const next = boundedOutputPart(output, event.content);
						output = next.output;
						unsafeOutput ||= next.overflow;
					}
					continue;
				}
				if (event.kind === "tool.admitted") {
					unsafeOutput = true;
					toolContractViolation = true;
					const safeAdmission = durableWriter.append({
						kind: event.kind,
						toolCallId: event.toolCallId,
						name: "shadow-reviewer-data-only-violation",
						...(event.modelCallId === undefined ? {} : { modelCallId: event.modelCallId }),
						schema: {
							status: "unavailable",
							reason: "provider violated the sealed data-only tool contract",
						},
					});
					await commitThrough(safeAdmission);
					const safeTerminal = durableWriter.append({
						kind: "tool.terminal",
						toolCallId: event.toolCallId,
						status: "synthetic_failure",
						origin: "north",
						errorCode: "data_only_contract_violation",
					});
					await commitThrough(safeTerminal);
					continue;
				}
				if (event.kind === "model-call.started") {
					const safe = durableWriter.append({
						kind: event.kind,
						modelCallId: event.modelCallId,
						model: event.model,
						...(event.effort === undefined ? {} : { effort: event.effort }),
						attempt: event.attempt,
					});
					await commitThrough(safe);
					continue;
				}
				if (event.kind === "model-call.completed") {
					const safe = durableWriter.append({
						kind: event.kind,
						modelCallId: event.modelCallId,
						status: event.status,
						origin: event.origin,
						usage: event.usage,
						usageCoverage: event.usageCoverage,
						...(event.errorCode === undefined ? {} : { errorCode: event.errorCode }),
						evidence: ephemeralEvidence(event.evidence),
					});
					await commitThrough(safe);
					status = event.status === "succeeded" ? "succeeded"
						: event.status === "cancelled" ? "cancelled" : "failed";
				}
			}
			if (input.signal.aborted) status = "cancelled";
			else if (toolContractViolation) status = "failed";
		}
	} catch {
		status = input.signal.aborted ? "cancelled" : "failed";
	} finally {
		try {
			await query?.close?.();
		} catch {
			status = input.signal.aborted ? "cancelled" : "failed";
		}
		detachAbort?.();
		if (emptyCwd !== undefined) {
			try { await fs.promises.rm(emptyCwd, { recursive: true, force: true }); }
			catch { status = input.signal.aborted ? "cancelled" : "failed"; }
		}
	}

	const terminal = terminalStatus(status);
	for (const event of durableWriter.terminate({
		lifecycle: terminal.lifecycle,
		reason: { code: terminal.code },
	})) {
		await commitThrough(event);
	}
	await transcript.close();
	const events = durableWriter.events();
	const snapshot = durableWriter.snapshot();
	if (snapshot === undefined) throw new Error("shadow reviewer durable run has no snapshot");
	const eventPublisher = runtime.publishEvents ?? publishWireEvents;
	const ledgerStatus = await eventPublisher(identity, events).catch(() => "unavailable" as const);
	const ledger = ledgerStatus === "recorded" ? wireLedgerSummary(events) : undefined;
	const telemetryStatus = ledger === undefined ? "unavailable" as const
		: await (runtime.recordTelemetry ?? recordWireRunTelemetry)(
			identity,
			snapshot,
			{ status: "recorded", summary: ledger },
			{
				role: "shadow-reviewer",
				provider: "anthropic",
				providerTarget: input.target.id,
				executionSource: "north-managed",
				executionTransport: query?.executionTransport ?? "sdk-stream",
				shadowReviewerExecution: {
					version: SHADOW_REVIEWER_VERSION,
					targetId: input.target.id,
					sourceRunId: input.update.sourceRunId,
					sourceFromSequence: input.update.sourceFromSequence,
					sourceThroughSequence: input.update.sourceThroughSequence,
					privacyOmittedEvents: input.update.privacyOmittedEvents,
					capacityOmittedEvents: input.update.capacityOmittedEvents,
					inputSha256: input.update.inputSha256,
				},
			},
		).catch(() => "unavailable" as const);
	const usage = normalizeUsage(snapshot);
	const publicationExact = ledgerStatus === "recorded" && telemetryStatus === "recorded";
	const usageStatus = usage.totalStatus === "exact" && !publicationExact
		? "partial" as const : usage.totalStatus;
	const durationMs = Math.max(0, Math.round(nowMs() - startedAt));
	if (!Number.isSafeInteger(durationMs)) throw new RangeError("shadow reviewer duration is out of range");
	return Object.freeze({
		runId,
		status,
		...(output === "" || unsafeOutput ? {} : { output }),
		...(unsafeOutput ? { unsafeOutput: true } : {}),
		usageStatus,
		...(usageStatus === "exact" && usage.total !== undefined ? { tokens: usage.total } : {}),
		durationMs,
	});
}
