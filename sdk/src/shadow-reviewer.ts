import { createHash } from "node:crypto";
import { privacyFilteredText } from "./privacy-filter";
import type { LearningAssignment } from "./learning-regime";
import type { TokenTotalStatus } from "./usage";
import type { WireEvent, WireModelCallId, WireRunId } from "./wire";

export const SHADOW_REVIEWER_VERSION = "north-shadow-reviewer:v1" as const;
export const SHADOW_REVIEWER_ARM = "shadow-reviewer-v1" as const;
export const SHADOW_REVIEWER_INPUT_MAX_BYTES = 16 * 1024;
export const SHADOW_REVIEWER_NOTE_MAX_BYTES = 1024;
export const SHADOW_REVIEWER_DEDUPE_CAPACITY = 256;
export const SHADOW_REVIEWER_DEADLINE_MS = 60_000;
export const SHADOW_REVIEWER_REAP_GRACE_MS = 5_000;
export const SHADOW_REVIEWER_ISSUE_CODES = [
	"contradictory_progress",
	"failed_verification",
	"missing_required_outcome",
	"unsafe_action",
	"unresolved_failure",
	"unsupported_completion_claim",
] as const;
export type ShadowReviewerIssueCode = typeof SHADOW_REVIEWER_ISSUE_CODES[number];
const RETAINED_EVENT_BYTES = 12 * 1024;
const EVENT_TEXT_BYTES = 2 * 1024;
const TARGET_ID = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/u;
const ISSUE_NOTES: Readonly<Record<ShadowReviewerIssueCode, string>> = Object.freeze({
	contradictory_progress: "The latest update contains conflicting progress evidence",
	failed_verification: "The latest update treats a failed verification as successful",
	missing_required_outcome: "The latest update omits a required outcome",
	unsafe_action: "The latest update contains a concrete safety risk",
	unresolved_failure: "The latest update contains an unresolved failure",
	unsupported_completion_claim: "The latest update claims completion without visible support",
});

function joinMissingUsageStatus(
	current: TokenTotalStatus,
	next: TokenTotalStatus,
): TokenTotalStatus {
	if (current === "partial" || next === "partial") return "partial";
	if (current === "exact") return next;
	if (next === "exact" || current === next) return current;
	const terminalRank = (status: TokenTotalStatus): number | undefined => {
		if (status === "unknown_no_terminal") return 0;
		if (status === "unknown_incomplete_terminal") return 1;
		return undefined;
	};
	const currentRank = terminalRank(current);
	const nextRank = terminalRank(next);
	if (currentRank !== undefined && nextRank !== undefined) {
		return currentRank >= nextRank ? current : next;
	}
	return "partial";
}

export interface ShadowReviewerConfig {
	readonly targetId: string;
}

export interface ShadowReviewerTarget {
	readonly id: string;
	readonly provider: string;
}

export interface ShadowReviewerUpdate {
	readonly sourceRunId: WireRunId;
	readonly sourceFromSequence: number;
	readonly sourceThroughSequence: number;
	readonly privacyOmittedEvents: number;
	readonly capacityOmittedEvents: number;
	readonly projectedSequences: readonly number[];
	readonly inputSha256: string;
	readonly projection: string;
}

export interface ShadowReviewExecution {
	readonly runId: WireRunId;
	readonly status: "succeeded" | "failed" | "cancelled";
	readonly output?: unknown;
	readonly unsafeOutput?: boolean;
	readonly usageStatus: TokenTotalStatus;
	readonly tokens?: number;
	readonly durationMs: number;
}

export interface ShadowReviewerNote {
	readonly version: typeof SHADOW_REVIEWER_VERSION;
	readonly reviewerRunId: WireRunId;
	readonly sourceRunId: WireRunId;
	readonly sourceThroughSequence: number;
	readonly severity: "nit" | "blocker";
	readonly issueCode: ShadowReviewerIssueCode;
	readonly sourceSequence: number;
	readonly note: string;
	readonly noteSha256: string;
}

export type ShadowReviewRunner = (
	update: ShadowReviewerUpdate,
	signal: AbortSignal,
) => Promise<ShadowReviewExecution>;

export type ShadowReviewerNoteSink = (
	note: ShadowReviewerNote,
	signal: AbortSignal,
) => Promise<void> | void;

export interface ShadowReviewerSummary {
	readonly version: typeof SHADOW_REVIEWER_VERSION;
	readonly targetId: string;
	readonly status: "not_assigned" | "completed" | "partial" | "aborted";
	readonly eligibleUpdates: number;
	readonly reviewedUpdates: number;
	readonly droppedUpdates: number;
	readonly emittedNotes: number;
	readonly quarantinedOutputs: number;
	readonly failedReviews: number;
	readonly usageStatus: TokenTotalStatus;
	readonly tokens?: number;
	readonly durationMs: number;
}

interface ProjectedEvent {
	readonly sequence: number;
	readonly value: Readonly<Record<string, unknown>>;
	readonly bytes: number;
}

function sha256(value: string): string {
	return createHash("sha256").update(value).digest("hex");
}

function safeCount(value: number, label: string): number {
	if (!Number.isSafeInteger(value) || value < 0) throw new TypeError(`${label} must be a nonnegative safe integer`);
	return value;
}

export function shadowReviewerConfig(
	env: NodeJS.ProcessEnv = process.env,
): ShadowReviewerConfig | undefined {
	const targetId = env.NORTH_SHADOW_REVIEWER?.trim();
	if (!targetId) return undefined;
	if (!TARGET_ID.test(targetId)) {
		throw new Error("NORTH_SHADOW_REVIEWER must be a portable target identifier");
	}
	return Object.freeze({ targetId });
}

export function shadowReviewerAssigned(
	config: ShadowReviewerConfig | undefined,
	assignment: LearningAssignment,
): config is ShadowReviewerConfig {
	return config !== undefined
		&& assignment.arm === "explore"
		&& assignment.axis === "authoring"
		&& assignment.armId === SHADOW_REVIEWER_ARM;
}

export function shadowReviewerTaskSignature(
	config: ShadowReviewerConfig | undefined,
): Readonly<{ version: typeof SHADOW_REVIEWER_VERSION; targetId: string | null }> {
	return Object.freeze({
		version: SHADOW_REVIEWER_VERSION,
		targetId: config?.targetId ?? null,
	});
}

export function assignedShadowReviewerTarget<T extends ShadowReviewerTarget>(
	config: ShadowReviewerConfig | undefined,
	assignment: LearningAssignment,
	targets: Readonly<Record<string, T>>,
): T | undefined {
	if (!shadowReviewerAssigned(config, assignment)) return undefined;
	const target = targets[config.targetId];
	if (!target) {
		throw new Error(`shadow reviewer target ${config.targetId} is unavailable`);
	}
	if (target.id !== config.targetId || target.provider !== "anthropic") {
		throw new Error("shadow reviewer requires an exact Anthropic target");
	}
	return target;
}

export function inactiveShadowReviewerSummary(
	config: ShadowReviewerConfig,
): ShadowReviewerSummary {
	return Object.freeze({
		version: SHADOW_REVIEWER_VERSION,
		targetId: config.targetId,
		status: "not_assigned",
		eligibleUpdates: 0,
		reviewedUpdates: 0,
		droppedUpdates: 0,
		emittedNotes: 0,
		quarantinedOutputs: 0,
		failedReviews: 0,
		usageStatus: "exact",
		tokens: 0,
		durationMs: 0,
	});
}

export function shadowReviewerAgentId(sourceAgentId: string): string {
	const digest = sha256(sourceAgentId).slice(0, 12);
	const prefix = sourceAgentId.replace(/[^A-Za-z0-9_.:-]+/gu, "-").slice(0, 96) || "lane";
	return `${prefix}.shadow-reviewer.${digest}`;
}

/** Host-owned correlation only; the reviewer receives no provider control handle. */
export class ShadowReviewerInterruptGate {
	#activeModelCallId: WireModelCallId | undefined;
	#interruptedModelCallId: WireModelCallId | undefined;
	#reviewerCancellationTerminalId: WireModelCallId | undefined;

	observe(event: WireEvent): void {
		if (!event.essential) return;
		if (event.kind === "model-call.started") {
			this.#activeModelCallId = event.modelCallId;
			return;
		}
		if (event.kind !== "model-call.completed") return;
		if (this.#activeModelCallId === event.modelCallId) this.#activeModelCallId = undefined;
		if (this.#interruptedModelCallId !== event.modelCallId) return;
		this.#interruptedModelCallId = undefined;
		if (event.status === "cancelled") {
			this.#reviewerCancellationTerminalId = event.modelCallId;
		}
	}

	async interruptIfArmed(
		armed: boolean,
		signal: AbortSignal,
		interrupt: (() => Promise<void>) | undefined,
	): Promise<boolean> {
		const modelCallId = this.#activeModelCallId;
		if (!armed || signal.aborted || modelCallId === undefined || interrupt === undefined) {
			return false;
		}
		this.#interruptedModelCallId = modelCallId;
		try {
			await interrupt();
			return true;
		} catch (error) {
			if (this.#interruptedModelCallId === modelCallId) {
				this.#interruptedModelCallId = undefined;
			}
			throw error;
		}
	}

	consumeReviewerCancellation(event: WireEvent): boolean {
		if (!event.essential || event.kind !== "model-call.completed"
			|| this.#reviewerCancellationTerminalId !== event.modelCallId) return false;
		this.#reviewerCancellationTerminalId = undefined;
		return true;
	}

	disarm(): void {
		this.#activeModelCallId = undefined;
		this.#interruptedModelCallId = undefined;
		this.#reviewerCancellationTerminalId = undefined;
	}
}

function text(value: unknown, home: string | undefined, maxBytes = EVENT_TEXT_BYTES): string | undefined {
	return typeof value === "string"
		? privacyFilteredText(value, { home, maxBytes })
		: undefined;
}

function projectEvent(event: WireEvent, home: string | undefined): Readonly<Record<string, unknown>> | undefined {
	if (!event.essential) return undefined;
	const base = { sequence: event.sequence, kind: event.kind };
	switch (event.kind) {
		case "run.started":
			return { ...base, lifecycle: event.lifecycle };
		case "run.progress":
			return {
				...base,
				lifecycle: event.lifecycle,
				...(text(event.progress.currentAction, home) === undefined
					? {} : { currentAction: text(event.progress.currentAction, home) }),
				...(event.progress.compactions === undefined
					? {} : { compactions: event.progress.compactions }),
				...(event.progress.patch === undefined || event.progress.patch === null
					? {} : { filesChanged: event.progress.patch.filesChanged }),
			};
		case "message.recorded": {
			const content = (event.role === "assistant" || event.role === "user")
				? text(event.content, home) : undefined;
			return {
				...base,
				role: event.role,
				stage: event.stage,
				...(content === undefined ? {} : { content }),
			};
		}
		case "model-call.started":
			return {
				...base,
				model: event.model,
				...(event.effort === undefined ? {} : { effort: event.effort }),
				attempt: event.attempt,
			};
		case "model-call.completed":
			return {
				...base,
				status: event.status,
				origin: event.origin,
				usageCoverage: event.usageCoverage,
				...(event.errorCode === undefined ? {} : { errorCode: event.errorCode }),
			};
		case "tool.admitted":
			return { ...base, name: text(event.name, home, 256) };
		case "tool.progress":
			return base;
		case "tool.terminal":
			return {
				...base,
				status: event.status,
				...(event.errorCode === undefined ? {} : { errorCode: event.errorCode }),
			};
		case "artifact.published":
			return {
				...base,
				mediaType: text(event.mediaType, home, 256),
				bytes: event.bytes,
				...(text(event.label, home, 256) === undefined
					? {} : { label: text(event.label, home, 256) }),
			};
		case "resource.pressure":
			return {
				...base,
				scope: text(event.scope, home, 256),
				resource: text(event.resource, home, 256),
				used: event.used,
				reserved: event.reserved,
				limit: event.limit,
				advisory: event.advisory,
			};
		case "run.terminated":
			return { ...base, lifecycle: event.lifecycle, reason: event.reason.code };
	}
}

class ShadowUpdateWindow {
	#events: ProjectedEvent[] = [];
	#bytes = 0;
	#capacityOmitted = 0;
	#privacyOmitted = 0;
	#fromSequence = 0;

	constructor(
		readonly sourceRunId: WireRunId,
		private readonly home: string | undefined,
	) {}

	add(event: WireEvent): void {
		const value = projectEvent(event, this.home);
		if (value === undefined) {
			this.#privacyOmitted += 1;
			return;
		}
		const bytes = Buffer.byteLength(JSON.stringify(value), "utf8");
		this.#events.push({ sequence: event.sequence, value, bytes });
		this.#bytes += bytes;
		while (this.#bytes > RETAINED_EVENT_BYTES && this.#events.length > 0) {
			const removed = this.#events.shift()!;
			this.#bytes -= removed.bytes;
			this.#capacityOmitted += 1;
		}
	}

	snapshot(throughSequence: number): ShadowReviewerUpdate {
		const document = {
			version: SHADOW_REVIEWER_VERSION,
			source: {
				runId: this.sourceRunId,
				fromSequence: this.#fromSequence,
				throughSequence,
			},
			coverage: {
				privacyOmittedEvents: this.#privacyOmitted,
				capacityOmittedEvents: this.#capacityOmitted,
			},
			events: this.#events.map(({ value }) => value),
		};
		let projection = JSON.stringify(document);
		while (Buffer.byteLength(projection, "utf8") > SHADOW_REVIEWER_INPUT_MAX_BYTES
			&& this.#events.length > 0) {
			const removed = this.#events.shift()!;
			this.#bytes -= removed.bytes;
			this.#capacityOmitted += 1;
			document.coverage.capacityOmittedEvents = this.#capacityOmitted;
			document.events = this.#events.map(({ value }) => value);
			projection = JSON.stringify(document);
		}
		if (Buffer.byteLength(projection, "utf8") > SHADOW_REVIEWER_INPUT_MAX_BYTES) {
			throw new RangeError("shadow reviewer projection exceeds its byte bound");
		}
		return Object.freeze({
			sourceRunId: this.sourceRunId,
			sourceFromSequence: this.#fromSequence,
			sourceThroughSequence: throughSequence,
			privacyOmittedEvents: this.#privacyOmitted,
			capacityOmittedEvents: this.#capacityOmitted,
			projectedSequences: Object.freeze(this.#events.map(({ sequence }) => sequence)),
			inputSha256: sha256(projection),
			projection,
		});
	}

	advance(nextSequence: number): void {
		this.#events = [];
		this.#bytes = 0;
		this.#capacityOmitted = 0;
		this.#privacyOmitted = 0;
		this.#fromSequence = nextSequence;
	}
}

type ParsedOutput =
	| { readonly kind: "none" }
	| {
		readonly kind: "note";
		readonly severity: "nit" | "blocker";
		readonly issueCode: ShadowReviewerIssueCode;
		readonly sourceSequence: number;
		readonly note: string;
	}
	| { readonly kind: "quarantined" };

function normalizeNote(value: string): string {
	return value.normalize("NFKC").toLowerCase()
		.replace(/[^\p{L}\p{N}]+/gu, " ").trim();
}

function parsedOutput(value: unknown, update: ShadowReviewerUpdate): ParsedOutput {
	let source = value;
	if (typeof source === "string") {
		try { source = JSON.parse(source); }
		catch { return { kind: "quarantined" }; }
	}
	if (!source || typeof source !== "object" || Array.isArray(source)) {
		return { kind: "quarantined" };
	}
	const record = source as Record<string, unknown>;
	const keys = Object.keys(record).sort();
	if (record.kind === "none" && keys.length === 1 && keys[0] === "kind") return { kind: "none" };
	if (record.kind !== "note"
		|| JSON.stringify(keys) !== JSON.stringify(["issueCode", "kind", "severity", "sourceSequence"])
		|| (record.severity !== "nit" && record.severity !== "blocker")
		|| !SHADOW_REVIEWER_ISSUE_CODES.includes(record.issueCode as ShadowReviewerIssueCode)
		|| !Number.isSafeInteger(record.sourceSequence)
		|| !update.projectedSequences.includes(record.sourceSequence as number)) {
		return { kind: "quarantined" };
	}
	const issueCode = record.issueCode as ShadowReviewerIssueCode;
	const sourceSequence = record.sourceSequence as number;
	const note = `${ISSUE_NOTES[issueCode]} (source event ${sourceSequence}).`;
	return { kind: "note", severity: record.severity, issueCode, sourceSequence, note };
}

export class ShadowReviewer {
	readonly #window: ShadowUpdateWindow;
	readonly #controller = new AbortController();
	readonly #seenNotes = new Map<string, true>();
	readonly #upstreamSignal: AbortSignal;
	#pending: ShadowReviewerUpdate | undefined;
	#drain: Promise<void> | undefined;
	#lastSequence = -1;
	#accepting = true;
	#eligibleUpdates = 0;
	#reviewedUpdates = 0;
	#droppedUpdates = 0;
	#emittedNotes = 0;
	#quarantinedOutputs = 0;
	#failedReviews = 0;
	#durationMs = 0;
	#tokens = 0;
	#usageStatus: TokenTotalStatus = "exact";
	#hasExactUsage = false;
	#hasMissingUsage = false;
	#upstreamAbort: (() => void) | undefined;
	readonly #reviewDeadlineMs: number;
	readonly #reapGraceMs: number;

	constructor(
		readonly config: ShadowReviewerConfig,
		readonly sourceRunId: WireRunId,
		private readonly runner: ShadowReviewRunner,
		private readonly sink: ShadowReviewerNoteSink,
		options: {
			readonly signal: AbortSignal;
			readonly home?: string;
			/** Hermetic timing seam; production always uses the fixed code-owned deadline. */
			readonly reviewDeadlineMs?: number;
			readonly reapGraceMs?: number;
		},
	) {
		this.#upstreamSignal = options.signal;
		this.#window = new ShadowUpdateWindow(sourceRunId, options.home);
		this.#reviewDeadlineMs = safeCount(
			options.reviewDeadlineMs ?? SHADOW_REVIEWER_DEADLINE_MS,
			"shadow reviewer deadline",
		);
		this.#reapGraceMs = safeCount(
			options.reapGraceMs ?? SHADOW_REVIEWER_REAP_GRACE_MS,
			"shadow reviewer reap grace",
		);
		this.#upstreamAbort = () => this.#abort(options.signal.reason);
		if (options.signal.aborted) this.#upstreamAbort();
		else options.signal.addEventListener("abort", this.#upstreamAbort, { once: true });
	}

	observe(event: WireEvent): void {
		if (!this.#accepting || this.#controller.signal.aborted) return;
		if (event.runId !== this.sourceRunId) throw new Error("shadow reviewer received another run");
		if (event.sequence <= this.#lastSequence) return;
		if (event.sequence !== this.#lastSequence + 1) {
			throw new Error("shadow reviewer canonical event sequence is discontinuous");
		}
		this.#lastSequence = event.sequence;
		this.#window.add(event);
		if (!event.essential || event.kind !== "model-call.completed"
			|| event.origin !== "provider" || event.status !== "succeeded") return;
		this.#eligibleUpdates += 1;
		const update = this.#window.snapshot(event.sequence);
		this.#window.advance(event.sequence + 1);
		if (this.#pending) this.#droppedUpdates += 1;
		this.#pending = update;
		this.#ensureDrain();
	}

	#ensureDrain(): void {
		if (this.#drain || this.#controller.signal.aborted) return;
		this.#drain = this.#drainUpdates().finally(() => {
			this.#drain = undefined;
			if (this.#pending && !this.#controller.signal.aborted) this.#ensureDrain();
		});
	}

	async #drainUpdates(): Promise<void> {
		while (!this.#controller.signal.aborted) {
			const update = this.#pending;
			if (!update) return;
			this.#pending = undefined;
			const reviewStartedAt = Date.now();
			const reviewController = new AbortController();
			const abortReview = () => reviewController.abort(this.#controller.signal.reason);
			if (this.#controller.signal.aborted) abortReview();
			else this.#controller.signal.addEventListener("abort", abortReview, { once: true });
			const executionPromise = Promise.resolve().then(
				() => this.runner(update, reviewController.signal),
			).then(
				(execution) => ({ kind: "execution" as const, execution }),
				(error: unknown) => ({ kind: "error" as const, error }),
			);
			const deadline = Promise.withResolvers<{ readonly kind: "deadline" }>();
			const deadlineTimer = setTimeout(
				() => deadline.resolve({ kind: "deadline" }),
				this.#reviewDeadlineMs,
			);
			const aborted = Promise.withResolvers<{ readonly kind: "aborted" }>();
			const resolveAbort = () => aborted.resolve({ kind: "aborted" });
			if (this.#controller.signal.aborted) resolveAbort();
			else this.#controller.signal.addEventListener("abort", resolveAbort, { once: true });
			let outcome = await Promise.race([executionPromise, deadline.promise, aborted.promise]);
			clearTimeout(deadlineTimer);
			this.#controller.signal.removeEventListener("abort", resolveAbort);
			const deadlineExceeded = outcome.kind === "deadline";
			if (outcome.kind === "deadline" || outcome.kind === "aborted") {
				reviewController.abort(new Error(
					outcome.kind === "deadline" ? "shadow_reviewer_deadline" : "shadow_reviewer_aborted",
				));
				const preReapOutcome = outcome;
				const reapDeadline = Promise.withResolvers<{ readonly kind: "reap-expired" }>();
				const reapTimer = setTimeout(
					() => reapDeadline.resolve({ kind: "reap-expired" }),
					this.#reapGraceMs,
				);
				const reaped = await Promise.race([
					executionPromise,
					reapDeadline.promise,
				]);
				clearTimeout(reapTimer);
				outcome = reaped.kind === "reap-expired" ? preReapOutcome : reaped;
				if (deadlineExceeded) {
					this.#failedReviews += 1;
					if (outcome.kind === "execution") this.#accountExecution(outcome.execution);
					else {
						this.#markMissingUsage("unknown_no_terminal");
						this.#durationMs = safeCount(
							this.#durationMs + this.#reviewDeadlineMs,
							"shadow reviewer duration",
						);
					}
					this.#controller.signal.removeEventListener("abort", abortReview);
					continue;
				}
			}
			this.#controller.signal.removeEventListener("abort", abortReview);
			if (outcome.kind !== "execution") {
				if (outcome.kind === "error") {
					if (!this.#controller.signal.aborted) this.#failedReviews += 1;
					this.#markMissingUsage("unknown_no_terminal");
				} else if (outcome.kind === "aborted") {
					this.#markMissingUsage("unknown_no_terminal");
				}
				continue;
			}
			const execution = outcome.execution;
			this.#accountExecution(execution);
			if (execution.status !== "succeeded") {
				if (execution.status === "failed") this.#failedReviews += 1;
				continue;
			}
			const output = execution.unsafeOutput
				? { kind: "quarantined" } as const
				: parsedOutput(execution.output, update);
			if (output.kind === "quarantined") {
				this.#quarantinedOutputs += 1;
				continue;
			}
			if (output.kind === "none") continue;
			const normalized = normalizeNote(output.note);
			if (!normalized || this.#seenNotes.has(normalized)) continue;
			this.#seenNotes.set(normalized, true);
			if (this.#seenNotes.size > SHADOW_REVIEWER_DEDUPE_CAPACITY) {
				const oldest = this.#seenNotes.keys().next().value;
				if (oldest !== undefined) this.#seenNotes.delete(oldest);
			}
			if (this.#controller.signal.aborted) return;
			const note: ShadowReviewerNote = Object.freeze({
				version: SHADOW_REVIEWER_VERSION,
				reviewerRunId: execution.runId,
				sourceRunId: update.sourceRunId,
				sourceThroughSequence: update.sourceThroughSequence,
				severity: output.severity,
				issueCode: output.issueCode,
				sourceSequence: output.sourceSequence,
				note: output.note,
				noteSha256: sha256(output.note),
			});
			const sinkController = new AbortController();
			const abortSink = () => sinkController.abort(this.#controller.signal.reason);
			if (this.#controller.signal.aborted) abortSink();
			else this.#controller.signal.addEventListener("abort", abortSink, { once: true });
			const publish = Promise.resolve().then(
				() => this.sink(note, sinkController.signal),
			).then(
				() => ({ kind: "published" as const }),
				() => ({ kind: "error" as const }),
			);
			const wallDeadline = Promise.withResolvers<{ readonly kind: "deadline" }>();
			const remainingMs = Math.max(
				0,
				this.#reviewDeadlineMs - (Date.now() - reviewStartedAt),
			);
			const wallTimer = setTimeout(
				() => wallDeadline.resolve({ kind: "deadline" }),
				remainingMs,
			);
			const sinkAborted = Promise.withResolvers<{ readonly kind: "aborted" }>();
			const resolveSinkAbort = () => sinkAborted.resolve({ kind: "aborted" });
			if (this.#controller.signal.aborted) resolveSinkAbort();
			else this.#controller.signal.addEventListener("abort", resolveSinkAbort, { once: true });
			const publishOutcome = await Promise.race([
				publish,
				wallDeadline.promise,
				sinkAborted.promise,
			]);
			clearTimeout(wallTimer);
			this.#controller.signal.removeEventListener("abort", resolveSinkAbort);
			if (publishOutcome.kind === "deadline" || publishOutcome.kind === "aborted") {
				if (!sinkController.signal.aborted) {
					sinkController.abort(new Error(
						publishOutcome.kind === "deadline"
							? "shadow_reviewer_deadline" : "shadow_reviewer_aborted",
					));
				}
				const reapDeadline = Promise.withResolvers<{ readonly kind: "reap-expired" }>();
				const reapTimer = setTimeout(
					() => reapDeadline.resolve({ kind: "reap-expired" }),
					this.#reapGraceMs,
				);
				await Promise.race([publish, reapDeadline.promise]);
				clearTimeout(reapTimer);
				if (publishOutcome.kind === "deadline") this.#failedReviews += 1;
			} else if (publishOutcome.kind === "published" && !this.#controller.signal.aborted) {
				this.#emittedNotes += 1;
			} else if (publishOutcome.kind === "error" && !this.#controller.signal.aborted) {
				this.#failedReviews += 1;
			}
			this.#controller.signal.removeEventListener("abort", abortSink);
		}
	}

	#abort(reason: unknown): void {
		if (this.#controller.signal.aborted) return;
		if (this.#pending) {
			this.#markMissingUsage("unknown_no_terminal");
		}
		this.#accepting = false;
		this.#pending = undefined;
		this.#controller.abort(reason);
	}

	#markMissingUsage(status: TokenTotalStatus): void {
		this.#hasMissingUsage = true;
		this.#usageStatus = this.#hasExactUsage
			? "partial" : joinMissingUsageStatus(this.#usageStatus, status);
	}

	#accountExecution(execution: ShadowReviewExecution): void {
		this.#reviewedUpdates += 1;
		this.#durationMs = safeCount(
			this.#durationMs + execution.durationMs,
			"shadow reviewer duration",
		);
		if (execution.usageStatus === "exact" && execution.tokens !== undefined) {
			this.#hasExactUsage = true;
			this.#tokens = safeCount(this.#tokens + execution.tokens, "shadow reviewer tokens");
			this.#usageStatus = this.#hasMissingUsage ? "partial" : "exact";
			return;
		}
		this.#markMissingUsage(execution.usageStatus === "exact"
			? "unknown_no_terminal" : execution.usageStatus);
	}

	forceClose(): void {
		this.#abort(new Error("shadow_reviewer_force_closed"));
	}

	async settleEligibleUpdates(): Promise<void> {
		while (this.#drain) await this.#drain;
	}

	async close(): Promise<ShadowReviewerSummary> {
		this.#accepting = false;
		await this.settleEligibleUpdates();
		if (this.#upstreamAbort) {
			this.#upstreamSignal.removeEventListener("abort", this.#upstreamAbort);
		}
		this.#upstreamAbort = undefined;
		const aborted = this.#controller.signal.aborted;
		const partial = this.#failedReviews > 0 || this.#droppedUpdates > 0
			|| this.#quarantinedOutputs > 0 || this.#usageStatus !== "exact";
		return Object.freeze({
			version: SHADOW_REVIEWER_VERSION,
			targetId: this.config.targetId,
			status: aborted ? "aborted" : partial ? "partial" : "completed",
			eligibleUpdates: this.#eligibleUpdates,
			reviewedUpdates: this.#reviewedUpdates,
			droppedUpdates: this.#droppedUpdates,
			emittedNotes: this.#emittedNotes,
			quarantinedOutputs: this.#quarantinedOutputs,
			failedReviews: this.#failedReviews,
			usageStatus: this.#usageStatus,
			...(this.#usageStatus === "exact" ? { tokens: this.#tokens } : {}),
			durationMs: this.#durationMs,
		});
	}
}
