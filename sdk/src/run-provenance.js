import { keyword as $$bc$keyword, property_key as $$bc$property_key, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, host_array as $$bh$host_array, host_object as $$bh$host_object } from './bridge/generated/beagle/host.js';

const judgment_grade_module = require("./judgment-grade");

const parseJudgmentGrade = judgment_grade_module.parseJudgmentGrade;

const learning_module = require("./learning-regime");

const learningAssignmentFacts = learning_module.learningAssignmentFacts;

const native_command_module = require("./native-command-activity");

const NATIVE__COMMAND__SHAPES = native_command_module.NATIVE_COMMAND_SHAPES;

const orchestration_capabilities_module = require("./orchestration-capabilities");

const ORCHESTRATION__CAPABILITIES = orchestration_capabilities_module.ORCHESTRATION_CAPABILITIES;

const run_estimate_module = require("./run-estimate");

const compareRunEstimate = run_estimate_module.compareRunEstimate;

const shadow_reviewer_module = require("./shadow-reviewer");

const SHADOW__REVIEWER__VERSION = shadow_reviewer_module.SHADOW_REVIEWER_VERSION;

const query_lifecycle_module = require("./query-lifecycle");

const managedRunTokenBudgetHandoff = query_lifecycle_module.managedRunTokenBudgetHandoff;

const struggle_module = require("./struggle");

const STRUGGLE__DETECTOR__POLICY__VERSION = struggle_module.STRUGGLE_DETECTOR_POLICY_VERSION;

const STRUGGLE__THRESHOLD__MAX = struggle_module.STRUGGLE_THRESHOLD_MAX;

const IDENTIFIER = new RegExp("^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$");

const COMPONENT = new RegExp("^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$");

const DIGEST = new RegExp("^[a-f0-9]{64}$");

const WIRE_ID = new RegExp("^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$");

const COVERAGE = new Set(["exact", "partial", "unknown"]);

const SHADOW_REVIEWER_STATUSES = new Set(["not_assigned", "completed", "partial", "aborted"]);

const TOKEN_TOTAL_STATUSES = new Set(["exact", "partial", "unknown_incomplete_terminal", "unknown_no_terminal", "unknown_provider", "unknown_overflow"]);

const STRUGGLE_TRIGGERS = new Set(["consecutive_errors", "tool_loop", "no_progress"]);

const ROUTING_AXIS_PREDICATES = $$bh$host_object($$bc$keyword("taskGrade"), "task_grade", $$bc$keyword("topology"), "topology", $$bc$keyword("capabilityFloor"), "capability_floor", $$bc$keyword("serviceClass"), "service_class", $$bc$keyword("reasoning"), "reasoning", $$bc$keyword("posture"), "posture");

const ROUTING_SIGNAL_PREDICATES = $$bh$host_object($$bc$keyword("decisionOwnership"), "decision_ownership", $$bc$keyword("seamScope"), "seam_scope", $$bc$keyword("errorExposure"), "error_exposure", $$bc$keyword("oracleStrength"), "oracle_strength", $$bc$keyword("foundationalImpact"), "foundational_impact", $$bc$keyword("dependencyShape"), "dependency_shape", $$bc$keyword("reasoningShape"), "reasoning_shape");

function fact(predicate, value) {
  const result = [predicate, value];
  return result;
}

function push_fact_bang(facts, predicate, value) {
  facts.push(fact(predicate, value));
  return null;
}

function push_facts_bang(facts, additions) {
  additions.forEach((addition) => {
  facts.push(addition);
});
  return null;
}

function count_value(value, label) {
  if (((!Number.isSafeInteger(value)) || (value < 0))) {
    (() => { throw new TypeError($$bc$str("invalid ", label)); })();
  }
  return String(value);
}

function push_optional_count_bang(facts, predicate, value) {
  if ((!(value === undefined))) {
    push_fact_bang(facts, predicate, count_value(((_logical) => (_logical !== false && _logical != null ? _logical : 0))(value), predicate));
  }
  return null;
}

function shadow_reviewer_header(version, target_id) {
  if (((!(version === SHADOW__REVIEWER__VERSION)) || (!((_truthy) => _truthy !== false && _truthy != null)(COMPONENT.test(target_id))))) {
    (() => { throw new TypeError("invalid shadow reviewer identity"); })();
  }
  return [fact("shadow_reviewer_version", version), fact("shadow_reviewer_target", target_id)];
}

function shadow_reviewer_summary_facts_bang(summary) {
  const facts = shadow_reviewer_header(summary.version, summary.targetId);
  if (((!((_truthy) => _truthy !== false && _truthy != null)(SHADOW_REVIEWER_STATUSES.has(summary.status))) || (!((_truthy) => _truthy !== false && _truthy != null)(TOKEN_TOTAL_STATUSES.has(summary.usageStatus))))) {
    (() => { throw new TypeError("invalid shadow reviewer summary status"); })();
  }
  const eligible = count_value(summary.eligibleUpdates, "shadow reviewer eligible update count");
  const reviewed = count_value(summary.reviewedUpdates, "shadow reviewer reviewed update count");
  const dropped = count_value(summary.droppedUpdates, "shadow reviewer dropped update count");
  const emitted = count_value(summary.emittedNotes, "shadow reviewer emitted note count");
  const quarantined = count_value(summary.quarantinedOutputs, "shadow reviewer quarantined output count");
  const failed = count_value(summary.failedReviews, "shadow reviewer failed review count");
  const duration = count_value(summary.durationMs, "shadow reviewer duration");
  const reviewed_updates = summary.reviewedUpdates;
  const dropped_updates = summary.droppedUpdates;
  const emitted_notes = summary.emittedNotes;
  const quarantined_outputs = summary.quarantinedOutputs;
  const handled = (reviewed_updates + dropped_updates);
  const surfaced = (emitted_notes + quarantined_outputs);
  const exact_usage = (summary.usageStatus === "exact");
  const tokens_known = (!(summary.tokens === undefined));
  if (((!Number.isSafeInteger(handled)) || ((!Number.isSafeInteger(surfaced)) || ((handled > summary.eligibleUpdates) || ((surfaced > summary.reviewedUpdates) || (summary.failedReviews > summary.eligibleUpdates)))))) {
    (() => { throw new TypeError("shadow reviewer summary counts do not reconcile"); })();
  }
  if ((!(exact_usage === tokens_known))) {
    (() => { throw new TypeError("shadow reviewer exact usage and token total do not reconcile"); })();
  }
  const all_zero = ((summary.eligibleUpdates === 0) && ((summary.reviewedUpdates === 0) && ((summary.droppedUpdates === 0) && ((summary.emittedNotes === 0) && ((summary.quarantinedOutputs === 0) && ((summary.failedReviews === 0) && ((summary.durationMs === 0) && (summary.tokens === 0))))))));
  if (((summary.status === "not_assigned") && ((!all_zero) || (!exact_usage)))) {
    (() => { throw new TypeError("inactive shadow reviewer summary carries activity"); })();
  }
  if (((summary.status === "completed") && ((!exact_usage) || ((!(summary.reviewedUpdates === summary.eligibleUpdates)) || ((!(summary.droppedUpdates === 0)) || ((!(summary.quarantinedOutputs === 0)) || (!(summary.failedReviews === 0)))))))) {
    (() => { throw new TypeError("completed shadow reviewer summary carries incomplete work"); })();
  }
  if (((summary.status === "partial") && ((summary.droppedUpdates === 0) && ((summary.quarantinedOutputs === 0) && ((summary.failedReviews === 0) && exact_usage))))) {
    (() => { throw new TypeError("partial shadow reviewer summary lacks partial evidence"); })();
  }
  [["shadow_reviewer_status", summary.status], ["shadow_reviewer_eligible_updates", eligible], ["shadow_reviewer_reviewed_updates", reviewed], ["shadow_reviewer_dropped_updates", dropped], ["shadow_reviewer_emitted_notes", emitted], ["shadow_reviewer_quarantined_outputs", quarantined], ["shadow_reviewer_failed_reviews", failed], ["shadow_reviewer_usage_status", summary.usageStatus]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
  if (tokens_known) {
    push_fact_bang(facts, "shadow_reviewer_tokens", count_value(summary.tokens, "shadow reviewer token count"));
  }
  push_fact_bang(facts, "shadow_reviewer_duration_ms", duration);
  return facts;
}

function shadow_reviewer_execution_facts_bang(execution) {
  const facts = shadow_reviewer_header(execution.version, execution.targetId);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(WIRE_ID.test(execution.sourceRunId)))) {
    (() => { throw new TypeError("invalid shadow reviewer source run"); })();
  }
  if ((execution.sourceFromSequence > execution.sourceThroughSequence)) {
    (() => { throw new TypeError("shadow reviewer source sequence interval is inverted"); })();
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(DIGEST.test(execution.inputSha256)))) {
    (() => { throw new TypeError("invalid shadow reviewer input digest"); })();
  }
  [["shadow_reviewer_source_run", execution.sourceRunId], ["shadow_reviewer_source_from_sequence", count_value(execution.sourceFromSequence, "shadow reviewer source from-sequence")], ["shadow_reviewer_source_through_sequence", count_value(execution.sourceThroughSequence, "shadow reviewer source through-sequence")], ["shadow_reviewer_privacy_omitted_events", count_value(execution.privacyOmittedEvents, "shadow reviewer privacy omission count")], ["shadow_reviewer_capacity_omitted_events", count_value(execution.capacityOmittedEvents, "shadow reviewer capacity omission count")], ["shadow_reviewer_input_sha256", execution.inputSha256]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
  return facts;
}

function estimate_facts(estimate, actual_duration_ms) {
  const comparison = compareRunEstimate(estimate, actual_duration_ms);
  return [fact("estimate_hours", estimate.hours), fact("estimate_delta_ms", String(comparison.deltaMs)), fact("estimate_ratio", comparison.ratio), fact("estimate_classification", comparison.classification)];
}

function validate_struggle_threshold(label, value) {
  if (((!Number.isSafeInteger(value)) || ((value < 1) || (value > STRUGGLE__THRESHOLD__MAX)))) {
    (() => { throw new TypeError($$bc$str("invalid struggle ", label, " threshold")); })();
  }
  return null;
}

function struggle_facts_bang(observation) {
  if ((!(observation.policyVersion === STRUGGLE__DETECTOR__POLICY__VERSION))) {
    (() => { throw new TypeError("unsupported struggle detector policy version"); })();
  }
  if (((!(observation.topology === "worker")) && (!(observation.topology === "orchestrator")))) {
    (() => { throw new TypeError("invalid struggle topology"); })();
  }
  validate_struggle_threshold("error-streak", observation.errorStreakThreshold);
  validate_struggle_threshold("loop-repeat", observation.loopRepeatThreshold);
  validate_struggle_threshold("loop-window", observation.loopWindow);
  validate_struggle_threshold("no-progress", observation.noProgressTurnThreshold);
  if ((observation.loopRepeatThreshold > observation.loopWindow)) {
    (() => { throw new TypeError("struggle loop-repeat threshold exceeds loop window"); })();
  }
  if (((!Number.isSafeInteger(observation.errorCount)) || (observation.errorCount < 0))) {
    (() => { throw new TypeError("invalid struggle error count"); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(new Set(observation.triggers).size === observation.triggers.length)) || observation.triggers.some((trigger) => (!((_truthy) => _truthy !== false && _truthy != null)(STRUGGLE_TRIGGERS.has(trigger))))))) {
    (() => { throw new TypeError("invalid struggle trigger observation"); })();
  }
  const facts = [fact("error_count", String(observation.errorCount)), fact("struggle_detector_policy_version", observation.policyVersion), fact("struggle_topology", observation.topology), fact("struggle_error_streak_threshold", String(observation.errorStreakThreshold)), fact("struggle_loop_repeat_threshold", String(observation.loopRepeatThreshold)), fact("struggle_loop_window", String(observation.loopWindow)), fact("struggle_no_progress_turn_threshold", String(observation.noProgressTurnThreshold))];
  observation.triggers.forEach((trigger) => {
  push_fact_bang(facts, "struggle", trigger);
});
  return facts;
}

function judgment_grade_facts_bang(snapshot) {
  const grade = parseJudgmentGrade(snapshot.grade);
  const valid = ((snapshot.status === "valid") && ((snapshot.source === "thread") && (grade === snapshot.grade)));
  const unavailable = ((snapshot.status === "unavailable") && ((snapshot.grade === undefined) && ((snapshot.source === "thread") || (snapshot.source === "ad-hoc"))));
  const invalid = ((snapshot.status === "invalid") && ((snapshot.grade === undefined) && (snapshot.source === "thread")));
  if ((!(valid || (unavailable || invalid)))) {
    (() => { throw new TypeError("invalid run-local judgment_grade snapshot"); })();
  }
  const facts = [];
  if (((_truthy) => _truthy !== false && _truthy != null)(grade)) {
    push_fact_bang(facts, "judgment_grade", grade);
  }
  push_fact_bang(facts, "judgment_grade_status", snapshot.status);
  push_fact_bang(facts, "judgment_grade_source", snapshot.source);
  return facts;
}

function receipt_facts_bang(context) {
  const facts = [];
  if (((_truthy) => _truthy !== false && _truthy != null)(context.learningAssignment)) {
    push_facts_bang(facts, learningAssignmentFacts(context.learningAssignment));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.promptReceipt)) {
    const receipt = context.promptReceipt;
    [["prompt_receipt_version", receipt.version], ["prompt_receipt_sha256", receipt.manifestSha256], ["prompt_wire_sha256", receipt.wireBytesSha256], ["prompt_receipt_coverage", receipt.coverage]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.environmentReceipt)) {
    const receipt = context.environmentReceipt;
    [["environment_receipt_version", receipt.version], ["environment_receipt_sha256", receipt.manifestSha256], ["environment_receipt_coverage", receipt.coverage], ["available_skill_catalog_sha256", receipt.availableSkillCatalogSha256], ["activated_resource_closure_sha256", receipt.activatedResourceClosureSha256]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.runEnvelopeReceipt)) {
    push_fact_bang(facts, "run_envelope_version", context.runEnvelopeReceipt.version);
    push_fact_bang(facts, "run_envelope_sha256", context.runEnvelopeReceipt.manifestSha256);
  }
  return facts;
}

function authority_facts_bang(authority) {
  const facts = [fact("effective_authority_provider", authority.provider), fact("effective_native_multi_agent", authority.nativeMultiAgent), fact("effective_live_input", authority.liveInput), fact("effective_authoring_hooks", authority.authoringHooks)];
  authority.capabilities.forEach((capability) => {
  push_fact_bang(facts, "effective_authority_capability", capability);
});
  authority.northEnabledTools.forEach((tool) => {
  push_fact_bang(facts, "effective_north_enabled_tool", tool);
});
  push_fact_bang(facts, "effective_web", authority.web);
  if ((authority.provider === "openai")) {
    push_fact_bang(facts, "effective_sandbox", authority.sandbox);
  } else {
    authority.builtins.forEach((tool) => {
  push_fact_bang(facts, "effective_builtin", tool);
});
    authority.managedTools.forEach((tool) => {
  push_fact_bang(facts, "effective_mcp_tool", tool);
});
  }
  return facts;
}

function routing_facts_bang(context) {
  const facts = [];
  const request = context.routingMetadata;
  if (((_truthy) => _truthy !== false && _truthy != null)(request)) {
    [["requested_role", request.role], ["routing_capability_floor", request.capabilityFloor], ["routing_service_class", request.serviceClass], ["requested_reasoning", request.reasoning], ["routing_posture", request.posture], ["task_grade", request.taskGrade], ["topology", request.topology]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
    request.domainRequirements.forEach((domain) => {
  push_fact_bang(facts, "domain_requirement", domain);
});
    push_fact_bang(facts, "composition_kind", request.composition.kind);
    push_fact_bang(facts, "composition_id", request.composition.id);
    if ((request.composition.kind === "template")) {
      request.composition.overrides.forEach((field) => {
  push_fact_bang(facts, "composition_override", field);
});
    } else {
      if (((_truthy) => _truthy !== false && _truthy != null)(request.composition.nearestTemplate)) {
        push_fact_bang(facts, "nearest_template", request.composition.nearestTemplate);
      }
      push_fact_bang(facts, "promotion_candidate", String(request.composition.promotionCandidate));
    }
  }
  const assessment = context.routingAssessment;
  const receipt = context.routingAdmissionReceipt;
  if (((_truthy) => _truthy !== false && _truthy != null)(receipt)) {
    [["routing_admission_receipt_version", String(receipt.version)], ["routing_request_sha256", receipt.routingRequestSha256], ["routing_policy_sha256", receipt.routingPolicySha256], ["routing_assessment_status", (((_truthy) => _truthy !== false && _truthy != null)(assessment) ? "recorded" : "unavailable")], ["routing_pin_evidence_status", receipt.pinEvidenceStatus], ["routing_override_evidence_status", receipt.overrideEvidence.status]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
    [["staffing_catalog_sha256", receipt.staffingCatalogSha256], ["provider_catalogs_sha256", receipt.providerCatalogsSha256], ["orchestration_policy_pin_sha256", receipt.orchestrationPolicyPinSha256], ["orchestration_catalog_digest_sha256", receipt.orchestrationCatalogDigestSha256], ["routing_assessment_sha256", receipt.routingAssessmentSha256], ["routing_pin_evidence_sha256", receipt.pinEvidenceSha256], ["routing_override_exception_code", receipt.overrideEvidence.exceptionCode]].forEach((entry) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(entry[1])) {
    push_fact_bang(facts, entry[0], entry[1]);
  }
});
    if ((!(receipt.orchestrationCatalogVersion === undefined))) {
      push_fact_bang(facts, "orchestration_catalog_version", String(receipt.orchestrationCatalogVersion));
    }
    if ((!(receipt.orchestrationCatalogTxVersion === undefined))) {
      push_fact_bang(facts, "orchestration_catalog_tx_version", String(receipt.orchestrationCatalogTxVersion));
    }
    receipt.overrideEvidence.changedAxes.forEach((field) => {
  push_fact_bang(facts, "routing_receipt_override", field);
});
    Object.entries(receipt.appliedAxes).forEach((entry) => {
  push_fact_bang(facts, $$bc$str("routing_applied_", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(ROUTING_AXIS_PREDICATES, entry[0])), entry[1]);
});
    Object.entries(((_logical) => (_logical !== false && _logical != null ? _logical : {}))(receipt.stockAxes)).forEach((entry) => {
  push_fact_bang(facts, $$bc$str("routing_stock_", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(ROUTING_AXIS_PREDICATES, entry[0])), entry[1]);
});
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(assessment)) {
    push_fact_bang(facts, "routing_assessment_policy", assessment.version);
    Object.entries(assessment.signals).forEach((entry) => {
  push_fact_bang(facts, $$bc$str("routing_signal_", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(ROUTING_SIGNAL_PREDICATES, entry[0])), entry[1]);
});
    push_fact_bang(facts, "routing_derived_capability_floor", assessment.derived.minimumCapabilityFloor);
    push_fact_bang(facts, "routing_derived_reasoning", assessment.derived.minimumReasoning);
    assessment.derived.ruleCodes.forEach((code) => {
  push_fact_bang(facts, "routing_rule_code", code);
});
    push_fact_bang(facts, "routing_selected_capability_floor", assessment.selected.capabilityFloor);
    push_fact_bang(facts, "routing_selected_reasoning", assessment.selected.reasoning);
    if (((_truthy) => _truthy !== false && _truthy != null)(assessment.exception)) {
      push_fact_bang(facts, "routing_exception_code", assessment.exception.code);
    }
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.routingPinEvidence)) {
    const pin = context.routingPinEvidence;
    [["routing_pin_policy", pin.policyVersion], ["routing_pin_issued_at", pin.issuedAt], ["routing_pin_expires_at", pin.expiresAt], ["routing_pin_reason_code", pin.reasonCode]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
    pin.pins.forEach((item) => {
  if ((item.kind === "provider")) {
    push_fact_bang(facts, "routing_pin", JSON.stringify(item));
  }
});
  }
  return facts;
}

function prompt_composition_facts_bang(applied) {
  const facts = [fact("prompt_composition_applied", "true")];
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? applied.roleId : _logical))(applied.roleKind))) {
    push_fact_bang(facts, "applied_role_contract", $$bc$str(applied.roleKind, ":", applied.roleId));
  }
  [["applied_bespoke_contract_sha256", applied.bespokeContractHash], ["applied_bespoke_contract_fingerprint_version", applied.bespokeContractFingerprintVersion], ["applied_bespoke_contract_fingerprint_domain", applied.bespokeContractFingerprintDomain], ["applied_template_override_reason_sha256", applied.templateOverrideReasonHash], ["applied_comms_contract_sha256", applied.commsContractHash], ["applied_task_grade", applied.taskGrade], ["applied_topology", applied.topology], ["applied_capability_floor", applied.capabilityFloor], ["applied_service_class", applied.serviceClass], ["applied_reasoning", applied.reasoning], ["applied_posture", applied.posture]].forEach((entry) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(entry[1])) {
    push_fact_bang(facts, entry[0], entry[1]);
  }
});
  ((_logical) => (_logical !== false && _logical != null ? _logical : []))(applied.templateOverrides).forEach((field) => {
  push_fact_bang(facts, "applied_template_override", field);
});
  const order = new Map(ORCHESTRATION__CAPABILITIES.map((capability, index) => $$bh$host_array(capability, index)));
  const capabilities = ((_logical) => (_logical !== false && _logical != null ? _logical : []))(applied.capabilities).sort((left, right) => { const left_order = order.get(left);
const right_order = order.get(right);
return (left_order - right_order); });
  capabilities.forEach((capability) => {
  push_fact_bang(facts, "applied_capability", capability);
});
  ((_logical) => (_logical !== false && _logical != null ? _logical : []))(applied.domainRequirements).forEach((domain) => {
  push_fact_bang(facts, "applied_domain_requirement", domain);
});
  push_fact_bang(facts, "applied_domain_requirement_count", String(((_logical) => (_logical !== false && _logical != null ? _logical : []))(applied.domainRequirements).length));
  if (((_truthy) => _truthy !== false && _truthy != null)(applied.modelDelta)) {
    if (((_truthy) => _truthy !== false && _truthy != null)(applied.modelDelta.provider)) {
      push_fact_bang(facts, "model_delta_provider", applied.modelDelta.provider);
    }
    push_fact_bang(facts, "model_delta_kind", applied.modelDelta.kind);
  }
  const economics = applied.promptEconomics;
  if (((_truthy) => _truthy !== false && _truthy != null)(economics)) {
    const stable_prefix_bytes = economics.stablePrefixBytes;
    const unique_tail_bytes = economics.uniqueTailBytes;
    if ((!((stable_prefix_bytes + unique_tail_bytes) === economics.totalBytes))) {
      (() => { throw new TypeError("prompt byte measurements do not sum to total"); })();
    }
    [["prompt_composition_version", economics.compositionVersion], ["prompt_composition_sha256", economics.compositionDigest], ["prompt_capability_class", economics.capabilityClass], ["prompt_byte_measurement_source", economics.byteMeasurementSource], ["prompt_token_measurement_status", economics.tokenMeasurementStatus], ["prompt_token_measurement_source", economics.tokenMeasurementSource], ["context_window_status", economics.contextWindowStatus], ["context_window_source", economics.contextWindowSource], ["context_budget_status", economics.contextBudgetStatus], ["context_budget_source", economics.contextBudgetSource], ["compaction_policy", economics.compactionPolicy], ["compaction_policy_version", economics.compactionPolicyVersion]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
    if (((_truthy) => _truthy !== false && _truthy != null)(economics.contextWindowEffectiveFrom)) {
      push_fact_bang(facts, "context_window_effective_from", economics.contextWindowEffectiveFrom);
    }
    push_optional_count_bang(facts, "prompt_stable_prefix_bytes", economics.stablePrefixBytes);
    push_optional_count_bang(facts, "prompt_unique_tail_bytes", economics.uniqueTailBytes);
    push_optional_count_bang(facts, "prompt_total_bytes", economics.totalBytes);
    push_optional_count_bang(facts, "prompt_capability_count", economics.capabilityCount);
    push_optional_count_bang(facts, "prompt_stable_prefix_tokens", economics.stablePrefixTokens);
    push_optional_count_bang(facts, "prompt_unique_tail_tokens", economics.uniqueTailTokens);
    push_optional_count_bang(facts, "prompt_total_composition_tokens", economics.totalCompositionTokens);
    push_optional_count_bang(facts, "provider_context_window_tokens", economics.providerContextWindowTokens);
    push_optional_count_bang(facts, "effective_context_budget_tokens", economics.effectiveContextBudgetTokens);
  }
  return facts;
}

function mcp_facts_bang(activity) {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(activity.source))) || ((!((_truthy) => _truthy !== false && _truthy != null)(COVERAGE.has(activity.coverage))) || ((activity.tools.length > 512) || ((activity.operationReceipts.length > 512) || (activity.operationAggregates.length > 512)))))) {
    (() => { throw new TypeError("invalid MCP activity observation"); })();
  }
  const facts = [fact("mcp_activity_source", activity.source), fact("mcp_activity_coverage", activity.coverage)];
  const identities = new Set();
  const derived = new Map();
  const state = $$bh$host_object($$bc$keyword("identifiedCalls"), 0);
  activity.tools.forEach((tool) => {
  const identity = $$bc$str(tool.server, "\x00", tool.tool);
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(COMPONENT.test(tool.server))) || ((!((_truthy) => _truthy !== false && _truthy != null)(COMPONENT.test(tool.tool))) || ((!Number.isSafeInteger(tool.count)) || ((tool.count < 1) || identities.has(identity))))))) {
    (() => { throw new TypeError("invalid MCP tool activity"); })();
  }
  identities.add(identity);
  const identified_calls = state.identifiedCalls;
  const tool_count = tool.count;
  (state.identifiedCalls = (identified_calls + tool_count));
  if ((!Number.isSafeInteger(state.identifiedCalls))) {
    (() => { throw new TypeError("MCP tool activity count exceeds the safe integer range"); })();
  }
});
  if (((!(activity.totalCalls === undefined)) && ((!Number.isSafeInteger(activity.totalCalls)) || ((activity.totalCalls < 0) || (state.identifiedCalls > activity.totalCalls))))) {
    (() => { throw new TypeError("MCP tool activity does not reconcile with total calls"); })();
  }
  if (((activity.coverage === "exact") && ((activity.totalCalls === undefined) || (!(state.identifiedCalls === activity.totalCalls))))) {
    (() => { throw new TypeError("exact MCP tool activity does not reconcile with total calls"); })();
  }
  if (((activity.coverage === "unknown") && ((!(activity.totalCalls === undefined)) || ((activity.tools.length > 0) || ((activity.operationReceipts.length > 0) || (activity.operationAggregates.length > 0)))))) {
    (() => { throw new TypeError("unknown MCP activity carries terminal evidence"); })();
  }
  activity.operationReceipts.forEach((receipt) => {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(receipt.tool))) || ((!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(receipt.operation))) || ((!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(receipt.outcome))) || ((!Number.isSafeInteger(receipt.durationMs)) || ((receipt.durationMs < 0) || ((!Number.isSafeInteger(receipt.resultSize)) || ((receipt.resultSize < 0) || ((!(receipt.batchSize === undefined)) && ((!Number.isSafeInteger(receipt.batchSize)) || (receipt.batchSize < 0))))))))))) {
    (() => { throw new TypeError("invalid MCP operation receipt"); })();
  }
  const aggregate = ((_logical) => (_logical !== false && _logical != null ? _logical : $$bh$host_object($$bc$keyword("count"), 0, $$bc$keyword("totalDurationMs"), 0, $$bc$keyword("failureCount"), 0)))(derived.get(receipt.operation));
  const count = aggregate.count;
  const total_duration_ms = aggregate.totalDurationMs;
  const duration_ms = receipt.durationMs;
  (aggregate.count = (count + 1));
  (aggregate.totalDurationMs = (total_duration_ms + duration_ms));
  if ((!(receipt.outcome === "ok"))) {
    const failure_count = aggregate.failureCount;
    (aggregate.failureCount = (failure_count + 1));
  }
  derived.set(receipt.operation, aggregate);
});
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(activity.operationAggregates.length === derived.size)) || activity.operationAggregates.some((aggregate) => { const expected = derived.get(aggregate.operation);
return ((!((_truthy) => _truthy !== false && _truthy != null)(expected)) || ((!(aggregate.count === expected.count)) || ((!(aggregate.totalDurationMs === expected.totalDurationMs)) || ((!(aggregate.failureCount === expected.failureCount)) || (!(aggregate.meanDurationMs === (() => { const total_duration_ms = expected.totalDurationMs; const count = expected.count; return (total_duration_ms / count); })())))))); })))) {
    (() => { throw new TypeError("MCP operation aggregates do not reconcile"); })();
  }
  if ((!(activity.totalCalls === undefined))) {
    push_fact_bang(facts, "mcp_actual_calls", count_value(activity.totalCalls, "MCP call count"));
  }
  activity.tools.forEach((tool) => {
  push_fact_bang(facts, "mcp_actual_tool", JSON.stringify(tool));
});
  activity.operationReceipts.forEach((receipt) => {
  push_fact_bang(facts, "mcp_operation_receipt", JSON.stringify(receipt));
});
  activity.operationAggregates.forEach((aggregate) => {
  push_fact_bang(facts, "mcp_operation_aggregate", JSON.stringify(aggregate));
});
  return facts;
}

function native_command_facts_bang(activity) {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(activity.source))) || ((!((_truthy) => _truthy !== false && _truthy != null)(COVERAGE.has(activity.coverage))) || ((!((_truthy) => _truthy !== false && _truthy != null)(["passed", "failed", "not_observed"].includes(activity.northBinaryProbe))) || (activity.completions.length > 32))))) {
    (() => { throw new TypeError("invalid native command activity observation"); })();
  }
  const counts = [activity.totalCommands, activity.successfulCommands, activity.failedCommands, activity.declinedCommands, activity.openCommands, activity.truncatedCommands, activity.readCommands, activity.editCommands];
  if (((_truthy) => _truthy !== false && _truthy != null)(counts.some((value) => ((!(value === undefined)) && ((!Number.isSafeInteger(value)) || (((_logical) => (_logical !== false && _logical != null ? _logical : 0))(value) < 0)))))) {
    (() => { throw new TypeError("invalid native command activity count"); })();
  }
  if (((activity.coverage === "unknown") && ((!(activity.totalCommands === undefined)) || ((activity.completions.length > 0) || (!(activity.northBinaryProbe === "not_observed")))))) {
    (() => { throw new TypeError("unknown native command activity carries terminal evidence"); })();
  }
  if (((activity.coverage === "exact") && (activity.totalCommands === undefined))) {
    (() => { throw new TypeError("exact native command activity requires a total command count"); })();
  }
  if (((!(activity.totalCommands === undefined)) && (() => { const successful = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(activity.successfulCommands); const failed = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(activity.failedCommands); const declined = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(activity.declinedCommands); const open = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(activity.openCommands); return (!((successful + failed + declined + open) === activity.totalCommands)); })())) {
    (() => { throw new TypeError("native command activity counts do not reconcile"); })();
  }
  const facts = [fact("native_command_activity_source", activity.source), fact("native_command_activity_coverage", activity.coverage), fact("native_north_binary_probe", activity.northBinaryProbe)];
  push_optional_count_bang(facts, "native_command_total", activity.totalCommands);
  push_optional_count_bang(facts, "native_command_successful", activity.successfulCommands);
  push_optional_count_bang(facts, "native_command_failed", activity.failedCommands);
  push_optional_count_bang(facts, "native_command_declined", activity.declinedCommands);
  push_optional_count_bang(facts, "native_command_open", activity.openCommands);
  push_optional_count_bang(facts, "native_command_truncated", activity.truncatedCommands);
  push_optional_count_bang(facts, "native_command_read", activity.readCommands);
  push_optional_count_bang(facts, "native_command_edit", activity.editCommands);
  activity.completions.forEach((completion) => {
  if (((!((_truthy) => _truthy !== false && _truthy != null)(DIGEST.test(completion.commandSha256))) || ((!((_truthy) => _truthy !== false && _truthy != null)(DIGEST.test(completion.outputSha256))) || ((!((_truthy) => _truthy !== false && _truthy != null)(["completed", "failed", "declined"].includes(completion.status))) || ((!((_truthy) => _truthy !== false && _truthy != null)(NATIVE__COMMAND__SHAPES.includes(completion.shape))) || ((!Number.isSafeInteger(completion.durationMs)) || ((completion.durationMs < 0) || ((!Number.isSafeInteger(completion.exitCode)) || ((completion.exitCode < -2147483648) || (completion.exitCode > 2147483647)))))))))) {
    (() => { throw new TypeError("invalid native command completion evidence"); })();
  }
  push_fact_bang(facts, "native_command_completion", JSON.stringify(completion));
});
  return facts;
}

function wireModelAvailabilityReceipt(receipt) {
  return Object.freeze($$bh$host_object($$bc$keyword("provider"), receipt.provider, $$bc$keyword("targetId"), receipt.targetId, $$bc$keyword("observedAt"), receipt.observedAt, $$bc$keyword("source"), receipt.source, $$bc$keyword("observationDigest"), receipt.observationDigest));
}

function wire_run_provenance_facts_bang(context, actual_duration_ms) {
  const facts = receipt_facts_bang(context);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? context.shadowReviewerExecution : _logical))(context.shadowReviewerSummary))) {
    (() => { throw new TypeError("run cannot carry both shadow reviewer summary and execution provenance"); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.shadowReviewerSummary)) {
    push_facts_bang(facts, shadow_reviewer_summary_facts_bang(context.shadowReviewerSummary));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.shadowReviewerExecution)) {
    push_facts_bang(facts, shadow_reviewer_execution_facts_bang(context.shadowReviewerExecution));
  }
  [["posture", context.posture], ["role", context.role], ["provider", context.provider], ["provider_target", context.providerTarget], ["provider_reason", context.providerReason], ["requested_provider", context.requestedProvider], ["requested_target", context.requestedTarget], ["requested_capability_floor", context.requestedCapabilityFloor], ["requested_service_class", context.requestedServiceClass], ["requested_effort", context.requestedEffort], ["allocation_mode", context.allocationMode], ["entitlement_pressure", context.entitlementPressure]].forEach((entry) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(entry[1])) {
    push_fact_bang(facts, entry[0], entry[1]);
  }
});
  if (((_truthy) => _truthy !== false && _truthy != null)(context.modelAvailability)) {
    const receipt = context.modelAvailability;
    if (((!(context.provider === receipt.provider)) || ((!(context.providerTarget === receipt.targetId)) || (!((_truthy) => _truthy !== false && _truthy != null)(DIGEST.test(receipt.observationDigest)))))) {
      (() => { throw new TypeError("model availability receipt does not match the final semantic route"); })();
    }
    [["model_availability_target", receipt.targetId], ["model_availability_source", receipt.source], ["model_availability_observed_at", receipt.observedAt], ["model_availability_digest", receipt.observationDigest]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
  }
  Object.entries(((_logical) => (_logical !== false && _logical != null ? _logical : {}))(context.allocationEvidence)).forEach((entry) => {
  push_fact_bang(facts, "allocation_evidence", JSON.stringify(Object.assign({}, {[$$bc$property_key($$bc$keyword("target"))]: entry[0]}, entry[1])));
});
  if ((!(context.fallbackCount === undefined))) {
    push_fact_bang(facts, "fallback_count", count_value(context.fallbackCount, "fallback count"));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (context.fallbackPath.length > 0) : _logical))(context.fallbackPath))) {
    push_fact_bang(facts, "fallback_path", context.fallbackPath.join(" -> "));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (context.fallbackTargetPath.length > 0) : _logical))(context.fallbackTargetPath))) {
    push_fact_bang(facts, "fallback_target_path", context.fallbackTargetPath.join(" -> "));
  }
  ((_logical) => (_logical !== false && _logical != null ? _logical : []))(context.fallbackReasons).forEach((reason) => {
  push_fact_bang(facts, "fallback_reason", JSON.stringify(reason));
});
  ((_logical) => (_logical !== false && _logical != null ? _logical : []))(context.envelopeScopes).forEach((scope) => {
  push_fact_bang(facts, "envelope_scope", scope);
});
  if ((!(context.envelopeRetries === undefined))) {
    push_fact_bang(facts, "envelope_retries", count_value(context.envelopeRetries, "envelope retry count"));
  }
  ((_logical) => (_logical !== false && _logical != null ? _logical : []))(context.envelopeAdvisories).forEach((advisory) => {
  push_fact_bang(facts, "envelope_advisory", advisory);
});
  if (((_truthy) => _truthy !== false && _truthy != null)(context.processOutcome)) {
    push_fact_bang(facts, "process_outcome", context.processOutcome);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.tokenBudget)) {
    const budget = context.tokenBudget;
    const target_tokens = budget.targetTokens;
    const observed_tokens = ((_logical) => (_logical !== false && _logical != null ? _logical : 0))(budget.observedTokens);
    const exact_within = ((budget.state === "within_target") && ((budget.coverage === "exact") && ((!(budget.observedTokens === undefined)) && (Number.isSafeInteger(budget.observedTokens) && ((observed_tokens >= 0) && ((observed_tokens < target_tokens) && (budget.overshootTokens === undefined)))))));
    const exact_limited = ((budget.state === "budget_limited") && ((budget.coverage === "exact") && ((!(budget.observedTokens === undefined)) && (Number.isSafeInteger(budget.observedTokens) && ((observed_tokens >= target_tokens) && (budget.overshootTokens === (observed_tokens - target_tokens)))))));
    const unenforceable = ((budget.state === "unenforceable") && ((!(budget.coverage === "exact")) && ((budget.observedTokens === undefined) && (budget.overshootTokens === undefined))));
    if (((!Number.isSafeInteger(budget.targetTokens)) || ((budget.targetTokens <= 0) || (!(exact_within || (exact_limited || unenforceable)))))) {
      (() => { throw new TypeError("invalid managed run token budget evidence"); })();
    }
    [["run_token_target", String(budget.targetTokens)], ["run_token_budget_status", budget.state], ["run_token_budget_coverage", budget.coverage]].forEach((entry) => {
  push_fact_bang(facts, entry[0], entry[1]);
});
    if ((!(budget.observedTokens === undefined))) {
      push_fact_bang(facts, "run_token_observed", String(budget.observedTokens));
    }
    if ((!(budget.overshootTokens === undefined))) {
      push_fact_bang(facts, "run_token_overshoot", String(budget.overshootTokens));
    }
    if (exact_limited) {
      push_fact_bang(facts, "run_token_budget_handoff", JSON.stringify(managedRunTokenBudgetHandoff(budget)));
    }
  }
  [["delivery_outcome", context.deliveryOutcome], ["delivery_reason", context.deliveryReason]].forEach((entry) => {
  if (((_truthy) => _truthy !== false && _truthy != null)(entry[1])) {
    push_fact_bang(facts, entry[0], entry[1]);
  }
});
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? context.deliveryProof.deliveryEvidence : _logical))(context.deliveryProof))) {
    push_fact_bang(facts, "delivery_evidence", context.deliveryProof.deliveryEvidence);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? context.deliveryProof.deliveryEvidenceSha256 : _logical))(context.deliveryProof))) {
    push_fact_bang(facts, "delivery_evidence_sha256", context.deliveryProof.deliveryEvidenceSha256);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.retryOfRun)) {
    push_fact_bang(facts, "retry_of_run", (((_truthy) => _truthy !== false && _truthy != null)(context.retryOfRun.startsWith("@")) ? context.retryOfRun : $$bc$str("@", context.retryOfRun)));
  }
  if ((!(context.retryAttempt === undefined))) {
    if (((!Number.isSafeInteger(context.retryAttempt)) || (context.retryAttempt < 1))) {
      (() => { throw new TypeError("invalid retry attempt"); })();
    }
    push_fact_bang(facts, "retry_attempt", String(context.retryAttempt));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.executionSource)) {
    push_fact_bang(facts, "execution_source", context.executionSource);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.executionTransport)) {
    push_fact_bang(facts, "execution_transport", context.executionTransport);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.effectiveAuthority)) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(context.effectiveAuthority.provider === context.provider)) : _logical))(context.provider))) {
      (() => { throw new TypeError("effective authority does not match the final provider"); })();
    }
    push_facts_bang(facts, authority_facts_bang(context.effectiveAuthority));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.mcpActivity)) {
    push_facts_bang(facts, mcp_facts_bang(context.mcpActivity));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.nativeCommandActivity)) {
    push_facts_bang(facts, native_command_facts_bang(context.nativeCommandActivity));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.runEstimate)) {
    push_facts_bang(facts, estimate_facts(context.runEstimate, actual_duration_ms));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.judgmentGrade)) {
    push_facts_bang(facts, judgment_grade_facts_bang(context.judgmentGrade));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(context.struggleObservation)) {
    push_facts_bang(facts, struggle_facts_bang(context.struggleObservation));
  }
  push_facts_bang(facts, routing_facts_bang(context));
  if (((_truthy) => _truthy !== false && _truthy != null)(context.promptComposition)) {
    push_facts_bang(facts, prompt_composition_facts_bang(context.promptComposition));
  }
  return Object.freeze(facts);
}

const wireRunProvenanceFacts = wire_run_provenance_facts_bang;

export { wireModelAvailabilityReceipt as "wireModelAvailabilityReceipt" };
export { wireRunProvenanceFacts as "wireRunProvenanceFacts" };
