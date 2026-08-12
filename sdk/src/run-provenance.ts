import type { EnvironmentReceipt, PromptReceipt, RunEnvelopeReceipt } from "./composition-receipt";
import type { DeliveryProof } from "./delivery-verification";
import type { HarnessCompositionEvidence } from "./harness";
import { parseJudgmentGrade, type JudgmentGradeSnapshot } from "./judgment-grade";
import { learningAssignmentFacts, type LearningAssignment } from "./learning-regime";
import {
	NATIVE_COMMAND_SHAPES,
	type NativeCommandActivityObservation,
} from "./native-command-activity";
import { ORCHESTRATION_CAPABILITIES } from "./orchestration-capabilities";
import type { ProviderModelAdmissionReceipt } from "./provider-model-observation-store";
import type { ProviderAuthoritySurface } from "./providers";
import type { AllocationEvidence, RoutingFallbackReason } from "./providers/types";
import { compareRunEstimate, type RunEstimateSnapshot } from "./run-estimate";
import type {
	RoutingAdmissionReceipt,
	RoutingAssessment,
	RoutingPinEvidence,
} from "./routing-economics";
import type { RoutingRequest } from "./routing-metadata";
import type { McpActivityObservation } from "./tool-activity";
import type { WireExecutionTransport } from "./wire/query";
import {
	managedRunTokenBudgetHandoff,
	type ManagedRunTokenBudgetStatus,
} from "./query-lifecycle";
import {
	STRUGGLE_DETECTOR_POLICY_VERSION,
	STRUGGLE_THRESHOLD_MAX,
	type StruggleObservation,
} from "./struggle";

export type RunProvenanceFact = readonly [string, string];

export interface WireModelAvailabilityReceipt {
	readonly provider: "anthropic" | "openai";
	readonly targetId: string;
	readonly observedAt: string;
	readonly source: string;
	readonly observationDigest: string;
}

export function wireModelAvailabilityReceipt(
	receipt: ProviderModelAdmissionReceipt,
): WireModelAvailabilityReceipt {
	return Object.freeze({
		provider: receipt.provider,
		targetId: receipt.targetId,
		observedAt: receipt.observedAt,
		source: receipt.source,
		observationDigest: receipt.observationDigest,
	});
}

export interface WireRunProvenance {
	readonly posture?: string;
	readonly role?: string;
	readonly provider?: "anthropic" | "openai";
	readonly providerTarget?: string;
	readonly providerReason?: string;
	readonly modelAvailability?: WireModelAvailabilityReceipt;
	readonly requestedProvider?: string;
	readonly requestedTarget?: string;
	readonly requestedTier?: string;
	readonly requestedEffort?: string;
	readonly routingMetadata?: RoutingRequest;
	readonly routingAssessment?: RoutingAssessment;
	readonly routingAdmissionReceipt?: RoutingAdmissionReceipt;
	readonly routingPinEvidence?: RoutingPinEvidence;
	readonly promptComposition?: HarnessCompositionEvidence;
	readonly learningAssignment?: LearningAssignment;
	readonly promptReceipt?: PromptReceipt;
	readonly environmentReceipt?: EnvironmentReceipt;
	readonly runEnvelopeReceipt?: RunEnvelopeReceipt;
	readonly mcpActivity?: McpActivityObservation;
	readonly nativeCommandActivity?: NativeCommandActivityObservation;
	readonly effectiveAuthority?: ProviderAuthoritySurface;
	readonly allocationMode?: string;
	readonly entitlementPressure?: string;
	readonly allocationEvidence?: Readonly<Record<string, AllocationEvidence>>;
	readonly fallbackCount?: number;
	readonly fallbackPath?: readonly string[];
	readonly fallbackTargetPath?: readonly string[];
	readonly fallbackReasons?: readonly RoutingFallbackReason[];
	readonly envelopeScopes?: readonly string[];
	readonly envelopeRetries?: number;
	readonly envelopeAdvisories?: readonly string[];
	readonly processOutcome?: string;
	readonly deliveryOutcome?: string;
	readonly deliveryReason?: string;
	readonly deliveryProof?: DeliveryProof;
	readonly retryOfRun?: string;
	readonly retryAttempt?: number;
	readonly executionSource?: "north-managed" | "provider-native";
	readonly executionTransport?: WireExecutionTransport;
	readonly runEstimate?: RunEstimateSnapshot;
	readonly judgmentGrade?: JudgmentGradeSnapshot;
	readonly struggleObservation?: StruggleObservation;
	readonly tokenBudget?: ManagedRunTokenBudgetStatus;
}

const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$/;
const COMPONENT = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
const DIGEST = /^[a-f0-9]{64}$/;
const COVERAGE = new Set(["exact", "partial", "unknown"]);
const STRUGGLE_TRIGGERS = new Set(["consecutive_errors", "tool_loop", "no_progress"]);
const ROUTING_AXIS_PREDICATES = {
	taskGrade: "task_grade",
	topology: "topology",
	tier: "tier",
	reasoning: "reasoning",
	posture: "posture",
} as const;
const ROUTING_SIGNAL_PREDICATES = {
	decisionOwnership: "decision_ownership",
	seamScope: "seam_scope",
	errorExposure: "error_exposure",
	oracleStrength: "oracle_strength",
	foundationalImpact: "foundational_impact",
	dependencyShape: "dependency_shape",
	reasoningShape: "reasoning_shape",
} as const;

function count(value: number, label: string): string {
	if (!Number.isSafeInteger(value) || value < 0) throw new TypeError(`invalid ${label}`);
	return String(value);
}

function estimateFacts(
	estimate: RunEstimateSnapshot,
	actualDurationMs: number,
): RunProvenanceFact[] {
	const comparison = compareRunEstimate(estimate, actualDurationMs);
	return [
		["estimate_hours", estimate.hours],
		["estimate_delta_ms", String(comparison.deltaMs)],
		["estimate_ratio", comparison.ratio],
		["estimate_classification", comparison.classification],
	];
}

function struggleFacts(observation: StruggleObservation): RunProvenanceFact[] {
	if (observation.policyVersion !== STRUGGLE_DETECTOR_POLICY_VERSION) {
		throw new TypeError("unsupported struggle detector policy version");
	}
	if (observation.topology !== "worker" && observation.topology !== "orchestrator") {
		throw new TypeError("invalid struggle topology");
	}
	for (const [name, value] of [
		["error-streak", observation.errorStreakThreshold],
		["loop-repeat", observation.loopRepeatThreshold],
		["loop-window", observation.loopWindow],
		["no-progress", observation.noProgressTurnThreshold],
	] as const) {
		if (!Number.isSafeInteger(value) || value < 1 || value > STRUGGLE_THRESHOLD_MAX) {
			throw new TypeError(`invalid struggle ${name} threshold`);
		}
	}
	if (observation.loopRepeatThreshold > observation.loopWindow) {
		throw new TypeError("struggle loop-repeat threshold exceeds loop window");
	}
	if (!Number.isSafeInteger(observation.errorCount) || observation.errorCount < 0) {
		throw new TypeError("invalid struggle error count");
	}
	if (new Set(observation.triggers).size !== observation.triggers.length
		|| observation.triggers.some((trigger) => !STRUGGLE_TRIGGERS.has(trigger))) {
		throw new TypeError("invalid struggle trigger observation");
	}
	return [
		["error_count", String(observation.errorCount)],
		["struggle_detector_policy_version", observation.policyVersion],
		["struggle_topology", observation.topology],
		["struggle_error_streak_threshold", String(observation.errorStreakThreshold)],
		["struggle_loop_repeat_threshold", String(observation.loopRepeatThreshold)],
		["struggle_loop_window", String(observation.loopWindow)],
		["struggle_no_progress_turn_threshold", String(observation.noProgressTurnThreshold)],
		...observation.triggers.map((trigger) => ["struggle", trigger] as const),
	];
}

function judgmentGradeFacts(snapshot: JudgmentGradeSnapshot): RunProvenanceFact[] {
	const grade = parseJudgmentGrade(snapshot.grade);
	const valid = snapshot.status === "valid"
		&& snapshot.source === "thread"
		&& grade === snapshot.grade;
	const unavailable = snapshot.status === "unavailable"
		&& snapshot.grade === undefined
		&& (snapshot.source === "thread" || snapshot.source === "ad-hoc");
	const invalid = snapshot.status === "invalid"
		&& snapshot.grade === undefined
		&& snapshot.source === "thread";
	if (!valid && !unavailable && !invalid) {
		throw new TypeError("invalid run-local judgment_grade snapshot");
	}
	return [
		...(grade === undefined ? [] : [["judgment_grade", grade] as const]),
		["judgment_grade_status", snapshot.status],
		["judgment_grade_source", snapshot.source],
	];
}

function mcpFacts(activity: McpActivityObservation): RunProvenanceFact[] {
	if (!IDENTIFIER.test(activity.source) || !COVERAGE.has(activity.coverage)
		|| activity.tools.length > 512 || activity.operationReceipts.length > 512
		|| activity.operationAggregates.length > 512) {
		throw new TypeError("invalid MCP activity observation");
	}
	let identifiedCalls = 0;
	const toolIdentities = new Set<string>();
	for (const tool of activity.tools) {
		const identity = `${tool.server}\0${tool.tool}`;
		if (!COMPONENT.test(tool.server) || !COMPONENT.test(tool.tool)
			|| !Number.isSafeInteger(tool.count) || tool.count < 1
			|| toolIdentities.has(identity)) {
			throw new TypeError("invalid MCP tool activity");
		}
		toolIdentities.add(identity);
		identifiedCalls += tool.count;
		if (!Number.isSafeInteger(identifiedCalls)) {
			throw new TypeError("MCP tool activity count exceeds the safe integer range");
		}
	}
	if (activity.totalCalls !== undefined
		&& (!Number.isSafeInteger(activity.totalCalls) || activity.totalCalls < 0
			|| identifiedCalls > activity.totalCalls)) {
		throw new TypeError("MCP tool activity does not reconcile with total calls");
	}
	if (activity.coverage === "exact"
		&& (activity.totalCalls === undefined || identifiedCalls !== activity.totalCalls)) {
		throw new TypeError("exact MCP tool activity does not reconcile with total calls");
	}
	if (activity.coverage === "unknown"
		&& (activity.totalCalls !== undefined || activity.tools.length > 0
			|| activity.operationReceipts.length > 0 || activity.operationAggregates.length > 0)) {
		throw new TypeError("unknown MCP activity carries terminal evidence");
	}
	const derived = new Map<string, { count: number; totalDurationMs: number; failureCount: number }>();
	for (const receipt of activity.operationReceipts) {
		if (!IDENTIFIER.test(receipt.tool)
			|| !IDENTIFIER.test(receipt.operation)
			|| !IDENTIFIER.test(receipt.outcome)
			|| !Number.isSafeInteger(receipt.durationMs) || receipt.durationMs < 0
			|| !Number.isSafeInteger(receipt.resultSize) || receipt.resultSize < 0
			|| (receipt.batchSize !== undefined
				&& (!Number.isSafeInteger(receipt.batchSize) || receipt.batchSize < 0))) {
			throw new TypeError("invalid MCP operation receipt");
		}
		const aggregate = derived.get(receipt.operation)
			?? { count: 0, totalDurationMs: 0, failureCount: 0 };
		aggregate.count += 1;
		aggregate.totalDurationMs += receipt.durationMs;
		if (receipt.outcome !== "ok") aggregate.failureCount += 1;
		derived.set(receipt.operation, aggregate);
	}
	if (activity.operationAggregates.length !== derived.size
		|| activity.operationAggregates.some((aggregate) => {
			const expected = derived.get(aggregate.operation);
			return expected === undefined
				|| aggregate.count !== expected.count
				|| aggregate.totalDurationMs !== expected.totalDurationMs
				|| aggregate.failureCount !== expected.failureCount
				|| aggregate.meanDurationMs !== expected.totalDurationMs / expected.count;
		})) {
		throw new TypeError("MCP operation aggregates do not reconcile");
	}
	const facts: RunProvenanceFact[] = [
		["mcp_activity_source", activity.source],
		["mcp_activity_coverage", activity.coverage],
	];
	if (activity.totalCalls !== undefined) {
		facts.push(["mcp_actual_calls", count(activity.totalCalls, "MCP call count")]);
	}
	for (const tool of activity.tools) facts.push(["mcp_actual_tool", JSON.stringify(tool)]);
	for (const receipt of activity.operationReceipts) {
		facts.push(["mcp_operation_receipt", JSON.stringify(receipt)]);
	}
	for (const aggregate of activity.operationAggregates) {
		facts.push(["mcp_operation_aggregate", JSON.stringify(aggregate)]);
	}
	return facts;
}

function nativeCommandFacts(activity: NativeCommandActivityObservation): RunProvenanceFact[] {
	if (!IDENTIFIER.test(activity.source) || !COVERAGE.has(activity.coverage)
		|| !["passed", "failed", "not_observed"].includes(activity.northBinaryProbe)
		|| activity.completions.length > 32) {
		throw new TypeError("invalid native command activity observation");
	}
	const counts = [
		activity.totalCommands,
		activity.successfulCommands,
		activity.failedCommands,
		activity.declinedCommands,
		activity.openCommands,
		activity.truncatedCommands,
		activity.readCommands,
		activity.editCommands,
	];
	if (counts.some((value) => value !== undefined
		&& (!Number.isSafeInteger(value) || value < 0))) {
		throw new TypeError("invalid native command activity count");
	}
	if (activity.coverage === "unknown"
		&& (activity.totalCommands !== undefined || activity.completions.length > 0
			|| activity.northBinaryProbe !== "not_observed")) {
		throw new TypeError("unknown native command activity carries terminal evidence");
	}
	if (activity.coverage === "exact" && activity.totalCommands === undefined) {
		throw new TypeError("exact native command activity requires a total command count");
	}
	if (activity.totalCommands !== undefined
		&& (activity.successfulCommands ?? 0) + (activity.failedCommands ?? 0)
			+ (activity.declinedCommands ?? 0) + (activity.openCommands ?? 0)
			!== activity.totalCommands) {
		throw new TypeError("native command activity counts do not reconcile");
	}
	const facts: RunProvenanceFact[] = [
		["native_command_activity_source", activity.source],
		["native_command_activity_coverage", activity.coverage],
		["native_north_binary_probe", activity.northBinaryProbe],
	];
	for (const [predicate, value] of [
		["native_command_total", activity.totalCommands],
		["native_command_successful", activity.successfulCommands],
		["native_command_failed", activity.failedCommands],
		["native_command_declined", activity.declinedCommands],
		["native_command_open", activity.openCommands],
		["native_command_truncated", activity.truncatedCommands],
		["native_command_read", activity.readCommands],
		["native_command_edit", activity.editCommands],
	] as const) {
		if (value !== undefined) facts.push([predicate, count(value, predicate)]);
	}
	for (const completion of activity.completions) {
		if (!DIGEST.test(completion.commandSha256) || !DIGEST.test(completion.outputSha256)
			|| !["completed", "failed", "declined"].includes(completion.status)
			|| !NATIVE_COMMAND_SHAPES.includes(completion.shape)
			|| !Number.isSafeInteger(completion.durationMs) || completion.durationMs < 0
			|| !Number.isSafeInteger(completion.exitCode)
			|| completion.exitCode < -2_147_483_648 || completion.exitCode > 2_147_483_647) {
			throw new TypeError("invalid native command completion evidence");
		}
		facts.push(["native_command_completion", JSON.stringify(completion)]);
	}
	return facts;
}

function receiptFacts(context: WireRunProvenance): RunProvenanceFact[] {
	const facts: RunProvenanceFact[] = [];
	if (context.learningAssignment) facts.push(...learningAssignmentFacts(context.learningAssignment));
	if (context.promptReceipt) {
		facts.push(["prompt_receipt_version", context.promptReceipt.version]);
		facts.push(["prompt_receipt_sha256", context.promptReceipt.manifestSha256]);
		facts.push(["prompt_wire_sha256", context.promptReceipt.wireBytesSha256]);
		facts.push(["prompt_receipt_coverage", context.promptReceipt.coverage]);
	}
	if (context.environmentReceipt) {
		facts.push(["environment_receipt_version", context.environmentReceipt.version]);
		facts.push(["environment_receipt_sha256", context.environmentReceipt.manifestSha256]);
		facts.push(["environment_receipt_coverage", context.environmentReceipt.coverage]);
		facts.push([
			"available_skill_catalog_sha256",
			context.environmentReceipt.availableSkillCatalogSha256,
		]);
		facts.push([
			"activated_resource_closure_sha256",
			context.environmentReceipt.activatedResourceClosureSha256,
		]);
	}
	if (context.runEnvelopeReceipt) {
		facts.push(["run_envelope_version", context.runEnvelopeReceipt.version]);
		facts.push(["run_envelope_sha256", context.runEnvelopeReceipt.manifestSha256]);
	}
	return facts;
}

function authorityFacts(authority: ProviderAuthoritySurface): RunProvenanceFact[] {
	const facts: RunProvenanceFact[] = [
		["effective_authority_provider", authority.provider],
		["effective_native_multi_agent", authority.nativeMultiAgent],
		["effective_live_input", authority.liveInput],
		["effective_authoring_hooks", authority.authoringHooks],
	];
	for (const capability of authority.capabilities) {
		facts.push(["effective_authority_capability", capability]);
	}
	for (const tool of authority.northEnabledTools) {
		facts.push(["effective_north_enabled_tool", tool]);
	}
	if (authority.provider === "openai") {
		facts.push(["effective_sandbox", authority.sandbox]);
		facts.push(["effective_web", authority.web]);
	} else {
		facts.push(["effective_web", authority.web]);
		for (const tool of authority.builtins) facts.push(["effective_builtin", tool]);
		for (const tool of authority.managedTools) facts.push(["effective_mcp_tool", tool]);
	}
	return facts;
}

function routingFacts(context: WireRunProvenance): RunProvenanceFact[] {
	const facts: RunProvenanceFact[] = [];
	const request = context.routingMetadata;
	if (request) {
		facts.push(["requested_role", request.role]);
		facts.push(["routing_tier", request.tier]);
		facts.push(["requested_reasoning", request.reasoning]);
		facts.push(["routing_posture", request.posture]);
		facts.push(["task_grade", request.taskGrade]);
		facts.push(["topology", request.topology]);
		for (const domain of request.domainRequirements) facts.push(["domain_requirement", domain]);
		facts.push(["composition_kind", request.composition.kind]);
		facts.push(["composition_id", request.composition.id]);
		if (request.composition.kind === "preset") {
			for (const field of request.composition.overrides) {
				facts.push(["composition_override", field]);
			}
		} else {
			if (request.composition.nearestPreset) {
				facts.push(["nearest_preset", request.composition.nearestPreset]);
			}
			facts.push(["promotion_candidate", String(request.composition.promotionCandidate)]);
		}
	}

	const assessment = context.routingAssessment;
	const receipt = context.routingAdmissionReceipt;
	if (receipt) {
		facts.push(["routing_admission_receipt_version", String(receipt.version)]);
		facts.push(["routing_request_sha256", receipt.routingRequestSha256]);
		if (receipt.staffingCatalogSha256) {
			facts.push(["staffing_catalog_sha256", receipt.staffingCatalogSha256]);
		}
		if (receipt.providerCatalogsSha256) {
			facts.push(["provider_catalogs_sha256", receipt.providerCatalogsSha256]);
		}
		facts.push(["routing_policy_sha256", receipt.routingPolicySha256]);
		if (receipt.orchestrationPolicyPinSha256) {
			facts.push(["orchestration_policy_pin_sha256", receipt.orchestrationPolicyPinSha256]);
		}
		if (receipt.orchestrationCatalogDigestSha256) {
			facts.push(["orchestration_catalog_digest_sha256", receipt.orchestrationCatalogDigestSha256]);
		}
		if (receipt.orchestrationCatalogVersion !== undefined) {
			facts.push(["orchestration_catalog_version", String(receipt.orchestrationCatalogVersion)]);
		}
		if (receipt.orchestrationCatalogTxVersion !== undefined) {
			facts.push(["orchestration_catalog_tx_version", String(receipt.orchestrationCatalogTxVersion)]);
		}
		facts.push(["routing_assessment_status", assessment ? "recorded" : "unavailable"]);
		if (receipt.routingAssessmentSha256) {
			facts.push(["routing_assessment_sha256", receipt.routingAssessmentSha256]);
		}
		facts.push(["routing_pin_evidence_status", receipt.pinEvidenceStatus]);
		if (receipt.pinEvidenceSha256) {
			facts.push(["routing_pin_evidence_sha256", receipt.pinEvidenceSha256]);
		}
		facts.push(["routing_override_evidence_status", receipt.overrideEvidence.status]);
		if (receipt.overrideEvidence.exceptionCode) {
			facts.push(["routing_override_exception_code", receipt.overrideEvidence.exceptionCode]);
		}
		for (const field of receipt.overrideEvidence.changedAxes) {
			facts.push(["routing_receipt_override", field]);
		}
		for (const [axis, value] of Object.entries(receipt.appliedAxes)) {
			facts.push([`routing_applied_${ROUTING_AXIS_PREDICATES[
				axis as keyof typeof ROUTING_AXIS_PREDICATES
			]}`, value]);
		}
		for (const [axis, value] of Object.entries(receipt.stockAxes ?? {})) {
			facts.push([`routing_stock_${ROUTING_AXIS_PREDICATES[
				axis as keyof typeof ROUTING_AXIS_PREDICATES
			]}`, value]);
		}
	}
	if (assessment) {
		facts.push(["routing_assessment_policy", assessment.version]);
		for (const [signal, value] of Object.entries(assessment.signals)) {
			facts.push([`routing_signal_${ROUTING_SIGNAL_PREDICATES[
				signal as keyof typeof ROUTING_SIGNAL_PREDICATES
			]}`, value]);
		}
		facts.push(["routing_derived_tier", assessment.derived.minimumTier]);
		facts.push(["routing_derived_reasoning", assessment.derived.minimumReasoning]);
		for (const code of assessment.derived.ruleCodes) facts.push(["routing_rule_code", code]);
		facts.push(["routing_selected_tier", assessment.selected.tier]);
		facts.push(["routing_selected_reasoning", assessment.selected.reasoning]);
		if (assessment.exception) {
			facts.push(["routing_exception_code", assessment.exception.code]);
		}
	}
	if (context.routingPinEvidence) {
		const pin = context.routingPinEvidence;
		facts.push(["routing_pin_policy", pin.policyVersion]);
		facts.push(["routing_pin_issued_at", pin.issuedAt]);
		facts.push(["routing_pin_expires_at", pin.expiresAt]);
		facts.push(["routing_pin_reason_code", pin.reasonCode]);
		for (const item of pin.pins) {
			// Account and model pins are authenticated by pinEvidenceSha256, never
			// copied as provider-private exact identifiers.
			if (item.kind === "provider") facts.push(["routing_pin", JSON.stringify(item)]);
		}
	}
	return facts;
}

function promptCompositionFacts(applied: HarnessCompositionEvidence): RunProvenanceFact[] {
	const facts: RunProvenanceFact[] = [["prompt_composition_applied", "true"]];
	if (applied.roleKind && applied.roleId) {
		facts.push(["applied_role_contract", `${applied.roleKind}:${applied.roleId}`]);
	}
	if (applied.bespokeContractHash) {
		facts.push(["applied_bespoke_contract_sha256", applied.bespokeContractHash]);
	}
	if (applied.bespokeContractFingerprintVersion) {
		facts.push([
			"applied_bespoke_contract_fingerprint_version",
			applied.bespokeContractFingerprintVersion,
		]);
	}
	if (applied.bespokeContractFingerprintDomain) {
		facts.push([
			"applied_bespoke_contract_fingerprint_domain",
			applied.bespokeContractFingerprintDomain,
		]);
	}
	for (const field of applied.presetOverrides ?? []) facts.push(["applied_preset_override", field]);
	if (applied.presetOverrideReasonHash) {
		facts.push(["applied_preset_override_reason_sha256", applied.presetOverrideReasonHash]);
	}
	const order = new Map(ORCHESTRATION_CAPABILITIES.map((capability, index) => [capability, index]));
	for (const capability of [...(applied.capabilities ?? [])]
		.sort((left, right) => order.get(left)! - order.get(right)!)) {
		facts.push(["applied_capability", capability]);
	}
	if (applied.commsContractHash) {
		facts.push(["applied_comms_contract_sha256", applied.commsContractHash]);
	}
	if (applied.taskGrade) facts.push(["applied_task_grade", applied.taskGrade]);
	if (applied.topology) facts.push(["applied_topology", applied.topology]);
	if (applied.tier) facts.push(["applied_routing_tier", applied.tier]);
	if (applied.reasoning) facts.push(["applied_reasoning", applied.reasoning]);
	if (applied.posture) facts.push(["applied_posture", applied.posture]);
	for (const domain of applied.domainRequirements ?? []) {
		facts.push(["applied_domain_requirement", domain]);
	}
	facts.push(["applied_domain_requirement_count", String(applied.domainRequirements?.length ?? 0)]);
	if (applied.modelDelta) {
		if (applied.modelDelta.provider) {
			facts.push(["model_delta_provider", applied.modelDelta.provider]);
		}
		facts.push(["model_delta_kind", applied.modelDelta.kind]);
	}
	const economics = applied.promptEconomics;
	if (!economics) return facts;
	if (economics.stablePrefixBytes + economics.uniqueTailBytes !== economics.totalBytes) {
		throw new TypeError("prompt byte measurements do not sum to total");
	}
	facts.push(["prompt_composition_version", economics.compositionVersion]);
	facts.push(["prompt_composition_sha256", economics.compositionDigest]);
	facts.push(["prompt_capability_class", economics.capabilityClass]);
	facts.push(["prompt_byte_measurement_source", economics.byteMeasurementSource]);
	facts.push(["prompt_token_measurement_status", economics.tokenMeasurementStatus]);
	facts.push(["prompt_token_measurement_source", economics.tokenMeasurementSource]);
	facts.push(["context_window_status", economics.contextWindowStatus]);
	facts.push(["context_window_source", economics.contextWindowSource]);
	facts.push(["context_budget_status", economics.contextBudgetStatus]);
	facts.push(["context_budget_source", economics.contextBudgetSource]);
	facts.push(["compaction_policy", economics.compactionPolicy]);
	facts.push(["compaction_policy_version", economics.compactionPolicyVersion]);
	if (economics.contextWindowEffectiveFrom) {
		facts.push(["context_window_effective_from", economics.contextWindowEffectiveFrom]);
	}
	for (const [predicate, value] of [
		["prompt_stable_prefix_bytes", economics.stablePrefixBytes],
		["prompt_unique_tail_bytes", economics.uniqueTailBytes],
		["prompt_total_bytes", economics.totalBytes],
		["prompt_capability_count", economics.capabilityCount],
		["prompt_stable_prefix_tokens", economics.stablePrefixTokens],
		["prompt_unique_tail_tokens", economics.uniqueTailTokens],
		["prompt_total_composition_tokens", economics.totalCompositionTokens],
		["provider_context_window_tokens", economics.providerContextWindowTokens],
		["effective_context_budget_tokens", economics.effectiveContextBudgetTokens],
	] as const) {
		if (value !== undefined) facts.push([predicate, count(value, predicate)]);
	}
	return facts;
}

export function wireRunProvenanceFacts(
	context: WireRunProvenance,
	actualDurationMs: number,
): readonly RunProvenanceFact[] {
	const facts: RunProvenanceFact[] = receiptFacts(context);
	if (context.posture) facts.push(["posture", context.posture]);
	if (context.role) facts.push(["role", context.role]);
	if (context.provider) facts.push(["provider", context.provider]);
	if (context.providerTarget) facts.push(["provider_target", context.providerTarget]);
	if (context.providerReason) facts.push(["provider_reason", context.providerReason]);
	if (context.modelAvailability) {
		const receipt = context.modelAvailability;
		if (context.provider !== receipt.provider || context.providerTarget !== receipt.targetId
			|| !DIGEST.test(receipt.observationDigest)) {
			throw new TypeError("model availability receipt does not match the final semantic route");
		}
		facts.push(["model_availability_target", receipt.targetId]);
		facts.push(["model_availability_source", receipt.source]);
		facts.push(["model_availability_observed_at", receipt.observedAt]);
		facts.push(["model_availability_digest", receipt.observationDigest]);
	}
	if (context.requestedProvider) facts.push(["requested_provider", context.requestedProvider]);
	if (context.requestedTarget) facts.push(["requested_target", context.requestedTarget]);
	if (context.requestedTier) facts.push(["requested_tier", context.requestedTier]);
	if (context.requestedEffort) facts.push(["requested_effort", context.requestedEffort]);
	if (context.allocationMode) facts.push(["allocation_mode", context.allocationMode]);
	if (context.entitlementPressure) {
		facts.push(["entitlement_pressure", context.entitlementPressure]);
	}
	for (const [target, evidence] of Object.entries(context.allocationEvidence ?? {})) {
		facts.push(["allocation_evidence", JSON.stringify({ target, ...evidence })]);
	}
	if (context.fallbackCount !== undefined) {
		facts.push(["fallback_count", count(context.fallbackCount, "fallback count")]);
	}
	if (context.fallbackPath?.length) facts.push(["fallback_path", context.fallbackPath.join(" -> ")]);
	if (context.fallbackTargetPath?.length) {
		facts.push(["fallback_target_path", context.fallbackTargetPath.join(" -> ")]);
	}
	for (const reason of context.fallbackReasons ?? []) {
		facts.push(["fallback_reason", JSON.stringify(reason)]);
	}
	for (const scope of context.envelopeScopes ?? []) facts.push(["envelope_scope", scope]);
	if (context.envelopeRetries !== undefined) {
		facts.push(["envelope_retries", count(context.envelopeRetries, "envelope retry count")]);
	}
	for (const advisory of context.envelopeAdvisories ?? []) {
		facts.push(["envelope_advisory", advisory]);
	}
	if (context.processOutcome) facts.push(["process_outcome", context.processOutcome]);
	if (context.tokenBudget) {
		const budget = context.tokenBudget;
		if (!Number.isSafeInteger(budget.targetTokens) || budget.targetTokens <= 0) {
			throw new TypeError("invalid managed run token target");
		}
		const exactWithinTarget = budget.state === "within_target"
			&& budget.coverage === "exact"
			&& budget.observedTokens !== undefined
			&& Number.isSafeInteger(budget.observedTokens)
			&& budget.observedTokens >= 0
			&& budget.observedTokens < budget.targetTokens
			&& budget.overshootTokens === undefined;
		const exactLimited = budget.state === "budget_limited"
			&& budget.coverage === "exact"
			&& budget.observedTokens !== undefined
			&& Number.isSafeInteger(budget.observedTokens)
			&& budget.observedTokens >= budget.targetTokens
			&& budget.overshootTokens === budget.observedTokens - budget.targetTokens;
		const unenforceable = budget.state === "unenforceable"
			&& budget.coverage !== "exact"
			&& budget.observedTokens === undefined
			&& budget.overshootTokens === undefined;
		if (!exactWithinTarget && !exactLimited && !unenforceable) {
			throw new TypeError("invalid managed run token budget evidence");
		}
		facts.push(["run_token_target", String(budget.targetTokens)]);
		facts.push(["run_token_budget_status", budget.state]);
		facts.push(["run_token_budget_coverage", budget.coverage]);
		if (budget.observedTokens !== undefined) {
			facts.push(["run_token_observed", String(budget.observedTokens)]);
		}
		if (budget.overshootTokens !== undefined) {
			facts.push(["run_token_overshoot", String(budget.overshootTokens)]);
		}
		if (exactLimited) {
			facts.push([
				"run_token_budget_handoff",
				JSON.stringify(managedRunTokenBudgetHandoff(budget)),
			]);
		}
	}
	if (context.deliveryOutcome) facts.push(["delivery_outcome", context.deliveryOutcome]);
	if (context.deliveryReason) facts.push(["delivery_reason", context.deliveryReason]);
	if (context.deliveryProof?.deliveryEvidence) {
		facts.push(["delivery_evidence", context.deliveryProof.deliveryEvidence]);
	}
	if (context.deliveryProof?.deliveryEvidenceSha256) {
		facts.push(["delivery_evidence_sha256", context.deliveryProof.deliveryEvidenceSha256]);
	}
	if (context.deliveryProof?.deliveryAttestation) {
		facts.push(["delivery_attestation", context.deliveryProof.deliveryAttestation]);
	}
	if (context.deliveryProof?.deliveryAttestationSha256) {
		facts.push(["delivery_attestation_sha256", context.deliveryProof.deliveryAttestationSha256]);
	}
	if (context.retryOfRun) {
		facts.push(["retry_of_run", context.retryOfRun.startsWith("@")
			? context.retryOfRun : `@${context.retryOfRun}`]);
	}
	if (context.retryAttempt !== undefined) {
		if (!Number.isSafeInteger(context.retryAttempt) || context.retryAttempt < 1) {
			throw new TypeError("invalid retry attempt");
		}
		facts.push(["retry_attempt", String(context.retryAttempt)]);
	}
	if (context.executionSource) facts.push(["execution_source", context.executionSource]);
	if (context.executionTransport) {
		facts.push(["execution_transport", context.executionTransport]);
	}
	if (context.effectiveAuthority) {
		if (context.provider && context.effectiveAuthority.provider !== context.provider) {
			throw new TypeError("effective authority does not match the final provider");
		}
		facts.push(...authorityFacts(context.effectiveAuthority));
	}
	if (context.mcpActivity) facts.push(...mcpFacts(context.mcpActivity));
	if (context.nativeCommandActivity) {
		facts.push(...nativeCommandFacts(context.nativeCommandActivity));
	}
	if (context.runEstimate) facts.push(...estimateFacts(context.runEstimate, actualDurationMs));
	if (context.judgmentGrade) facts.push(...judgmentGradeFacts(context.judgmentGrade));
	if (context.struggleObservation) facts.push(...struggleFacts(context.struggleObservation));
	facts.push(...routingFacts(context));
	if (context.promptComposition) facts.push(...promptCompositionFacts(context.promptComposition));
	return Object.freeze(facts);
}
