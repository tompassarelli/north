import { eager_seq as $$bc$eager_seq, record_instance_p as $$bc$record_instance_p, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_array as $$bh$admit_host_array, admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, array as $$bh$array, aset as $$bh$aset, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const node_module = process.getBuiltinModule("node:module");

const create_require = node_module.createRequire;

const require_module = create_require(import.meta.url);

const telemetry_module = require_module("./telemetry");

const run_ledger_module = require_module("./run-ledger");

const providers_module = require_module("./providers");

const execution_fold_module = require_module("./execution-fold");

const run_provenance_module = require_module("./run-provenance");

const path_module = process.getBuiltinModule("node:path");

const fs_module = process.getBuiltinModule("node:fs");

const crypto_module = process.getBuiltinModule("node:crypto");

const resolve = path_module.resolve;

const join = path_module.join;

const mkdirSync = fs_module.mkdirSync;

const renameSync = fs_module.renameSync;

const writeFileSync = fs_module.writeFileSync;

const writeSync = fs_module.writeSync;

const randomUUID = crypto_module.randomUUID;

const routing_admission = require_module("./routing-admission");

const routing_economics_module = require_module("./routing-economics");

const orchestration_staffing = require_module("./orchestration-staffing");

const orchestration_capabilities = require_module("./orchestration-capabilities");

const execution_outcome = require_module("./execution-outcome");

const stream_writer_module = require_module("./stream-writer");

const run_artifacts_module = require_module("./run-artifacts");

const harness_module = require_module("./harness");

const execution_activity_module = require_module("./execution-activity");

const worktree_module = require_module("./worktree");

const death_module = require_module("./death");

const coordination_module = require_module("./coordination");

const identity_module = require_module("./identity");

const bespoke_contract_module = require_module("./bespoke-contract");

const struggle_module = require_module("./struggle");

const watchdog_module = require_module("./watchdog");

const bgtasks_module = require_module("./bgtasks");

const children_module = require_module("./children");

const account_usage_module = require_module("./account-usage");

const resource_envelopes_module = require_module("./resource-envelopes");

const topology_authority_module = require_module("./topology-authority");

const execution_admission_module = require_module("./execution-admission");

const delivery_liveness_module = require_module("./delivery-liveness");

const empty_result_repair_module = require_module("./empty-result-repair");

const wire_module = require_module("./wire");

const live_input_route_module = require_module("./live-input-route");

const terminal_notification_module = require_module("./terminal-notification");

const delivery_verification_module = require_module("./delivery-verification");

const north_client_module = require_module("./north-client");

const delivery_evidence_module = require_module("./delivery-evidence");

const test_runtime_module = require_module("./internal/test-runtime");

const judgment_grade_module = require_module("./judgment-grade");

const query_lifecycle_module = require_module("./query-lifecycle");

const managed_learning_module = require_module("./managed-learning");

const learning_assignment_writer_module = require_module("./learning-assignment-writer");

const shadow_reviewer_note_module = require_module("./shadow-reviewer-note");

const shadow_reviewer_module = require_module("./shadow-reviewer");

const shadow_review_runner_module = require_module("./providers/shadow-reviewer");

const composition_receipt_module = require_module("./composition-receipt");

const tool_activity_module = require_module("./tool-activity");

const native_command_activity_module = require_module("./native-command-activity");

const bridge_protocol_module = require_module("./bridge/generated/north/bridge/protocol.js");

const bridge_journal_module = require_module("./bridge/journal");

function foreign_call_bang(module, name, arguments$) {
  return Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, name), null, arguments$);
}

function foreign_new_bang(module, name, arguments$) {
  return Reflect.construct((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, name), arguments$);
}

function foreign_call_async_bang(module, name, arguments$) {
  return Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, name), null, arguments$);
}

const admitResolvedRoutingRequest = routing_admission.admitResolvedRoutingRequest;

const projectProfileFromEnv = routing_admission.projectProfileFromEnv;

const routingRequestFromEnv = routing_admission.routingRequestFromEnv;

const admitRoutingEconomics = routing_economics_module.admitRoutingEconomics;

const orchestrationCapabilities = orchestration_staffing.orchestrationCapabilities;

const hasAuthoringCapability = orchestration_capabilities.hasAuthoringCapability;

const NO__PROVIDER__TERMINAL__DETAIL = execution_outcome.NO_PROVIDER_TERMINAL_DETAIL;

const PROVIDER__PROCESS__DEATH__OUTCOME = execution_outcome.PROVIDER_PROCESS_DEATH_OUTCOME;

const ResourceEnvelopeExceededError = resource_envelopes_module.ResourceEnvelopeExceededError;

const ProviderRetrySafeError = providers_module.ProviderRetrySafeError;

const LiveFeedReapTimeoutError = coordination_module.LiveFeedReapTimeoutError;

const REPO_ROOT = resolve(import.meta.dir, "..", "..");

const spawn_terminal_line_written = ({value: false, watches: {}});

function terminal_cause(value) {
  const detail = ((value instanceof Error) ? $$bc$str(value.name, ": ", value.message) : $$bc$str(value));
  return ((_logical) => (_logical !== false && _logical != null ? _logical : "unknown"))(detail.trim().split(/\s+/).join(" "));
}

function latest_turn_evidence(state) {
  const evidence = state.turnEvidence;
  return (() => { const _x = evidence, _i = (evidence.length - 1); return _x[_i] != null ? _x[_i] : null; })();
}

function append_spawn_terminal_line_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const kind = $beagle$args[0];
    return append_spawn_terminal_line_bang(kind, null);
  }
  if (arguments.length === 2) {
    const kind = $beagle$args[0];
    const cause = $beagle$args[1];
    if ((!spawn_terminal_line_written.value)) {
      (() => { const _a = spawn_terminal_line_written, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
      const detail = ((cause == null) ? kind : $$bc$str(kind, ": ", terminal_cause(cause)));
      (() => { try {
    return writeSync(2, $$bc$str("[spawn] terminal ", detail, "\n"));
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return null;
        break;
      }
    }
  } })();
      return null;
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const appendSpawnTerminalLine = append_spawn_terminal_line_bang;

function install_spawn_terminal_handlers_bang() {
  const signals = [$$bh$js_obj("signal", "SIGHUP", "exitCode", 129), $$bh$js_obj("signal", "SIGINT", "exitCode", 130), $$bh$js_obj("signal", "SIGTERM", "exitCode", 143)];
  $$bc$eager_seq(signals.map((entry) => process.once(entry.signal, () => { append_spawn_terminal_line_bang($$bc$str("signal=", entry.signal));
return process.exit(entry.exitCode); })));
  process.once("uncaughtException", (error) => { append_spawn_terminal_line_bang("uncaughtException", error);
return process.exit(1); });
  return process.once("unhandledRejection", (reason) => { append_spawn_terminal_line_bang("unhandledRejection", reason);
return process.exit(1); });
}

const installSpawnTerminalHandlers = install_spawn_terminal_handlers_bang;

function write_lane_meta_bang(agent_id, meta) {
  return (() => { try {
    const dir = ((_logical) => (_logical !== false && _logical != null ? _logical : join(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(process.env.HOME), ".local/state/north/agents")))(process.env.NORTH_AGENT_LOGS_DIR);
  const file = (((_truthy) => _truthy !== false && _truthy != null)(agent_id.startsWith("lane-")) ? agent_id : $$bc$str("lane-", agent_id));
  const target = join(dir, $$bc$str(file, ".meta.json"));
  const temporary = $$bc$str(target, ".", randomUUID(), ".tmp");
  mkdirSync(dir, $$bh$js_obj("recursive", true));
  writeFileSync(temporary, $$bc$str(JSON.stringify(meta), "\n"), "utf8");
  return renameSync(temporary, target);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        return null;
        break;
      }
    }
  } })();
}

const SPAWN_OPTION_FIELDS = new Set(["prompt", "agentId", "model", "tools", "systemPrompt", "maxTurns", "thread", "coordinator", "provider", "target", "routingMetadata", "projectProfile", "project", "sessionId", "worktree", "setupCmd", "routingAssessment", "pinEvidence", "tokenTarget"]);

function allowlisted_spawn_options(value) {
  if (((!(typeof value === "object")) || ((value == null) || Array.isArray(value)))) {
    (() => { throw new Error("managed North spawn request must be an object"); })();
  }
  const admitted = Object();
  const descriptors = Object.getOwnPropertyDescriptors(value);
  const fields = Object.keys(descriptors);
  $$bc$eager_seq(fields.map((field) => (() => { const descriptor = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(descriptors, field); if ((!((_truthy) => _truthy !== false && _truthy != null)(SPAWN_OPTION_FIELDS.has(field)))) {
  (() => { throw new Error($$bc$str("managed North spawn request has unknown field ", field)); })();
}
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : descriptor.set))(descriptor.get))) {
  (() => { throw new Error($$bc$str("managed North spawn request field ", field, " must be a data property")); })();
}
return (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(admitted, field, descriptor.value); })()));
  return admitted;
}

function createSpawnAgentId(...$beagle$args) {
  if (arguments.length === 0) {
    return createSpawnAgentId(Date.now(), randomUUID());
  }
  if (arguments.length === 1) {
    const now = $beagle$args[0];
    return createSpawnAgentId(now, randomUUID());
  }
  if (arguments.length === 2) {
    const now = $beagle$args[0];
    const uuid = $beagle$args[1];
    return $$bc$str("lane-", now.toString(36), "-", uuid);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const PROVIDER_PROCESS_DEATH_MAX_RETRIES = 1;

function apply_codex_turn_deadline_from_reasoning_bang(...$beagle$args) {
  if (arguments.length === 0) {
    return applyCodexTurnDeadlineFromReasoning(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    if ((env.NORTH_CODEX_TURN_DEADLINE_MS == null)) {
      const reasoning = env.AGENT_REASONING;
      const deadline_ms = (((reasoning === "low")) ? 600000 : ((reasoning === "medium")) ? 900000 : ((reasoning === "high")) ? 1500000 : (((reasoning === "xhigh") || (reasoning === "max"))) ? 2400000 : null);
      if ((!(deadline_ms == null))) {
        (env.NORTH_CODEX_TURN_DEADLINE_MS = $$bc$str(deadline_ms));
      }
    }
    return null;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const applyCodexTurnDeadlineFromReasoning = apply_codex_turn_deadline_from_reasoning_bang;

function eligibleForLaneStartProviderRetry(outcome, provider_error_detail, num_turns, sibling_target) {
  return (((!(outcome === "provider_error")) || (sibling_target == null)) ? false : ((provider_error_detail === NO__PROVIDER__TERMINAL__DETAIL) || ((num_turns === 0) && /\b529\b|overloaded/.test(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(provider_error_detail)))));
}

function eligibleForProviderProcessDeathRetry(outcome, topology, capabilities) {
  return ((outcome === PROVIDER__PROCESS__DEATH__OUTCOME) && ((topology === "worker") && (!hasAuthoringCapability(capabilities))));
}

function compose_spawn_options(opts) {
  const admission = admitResolvedRoutingRequest(((_logical) => (_logical !== false && _logical != null ? _logical : Object()))(opts.routingMetadata), "managed North spawn", $$bh$js_obj("projectProfile", opts.projectProfile));
  const routing_metadata = admission.routingRequest;
  const routing_economics = admitRoutingEconomics($$bh$js_obj("request", routing_metadata, "routingAssessment", opts.routingAssessment, "pinEvidence", opts.pinEvidence, "provider", opts.provider, "target", opts.target, "model", opts.model, "surface", "managed North spawn routing economics"));
  const worktree = ((opts.worktree == null) ? ((process.env.AGENT_WORKTREE === "1") || hasAuthoringCapability(orchestrationCapabilities(routing_metadata))) : opts.worktree);
  return Object.assign(Object(), opts, $$bh$js_obj("routingMetadata", routing_metadata, "projectProfile", admission.projectProfile, "routingAssessment", routing_economics.assessment, "pinEvidence", routing_economics.pinEvidence, "routingEconomics", routing_economics, "worktree", worktree));
}

async function publish_learning_assignment_for_run_bang(assignment_writer, assignment_run_id, learning_assignment) {
  const status = await Reflect.apply(assignment_writer, null, $$bh$array(assignment_run_id, learning_assignment));
  if ((!(status === "recorded"))) {
    (() => { throw new Error("managed North spawn requires a durable pre-provider learning assignment"); })();
  }
  return null;
}

function active_route(state) {
  const routing = state.routing;
  const opts = state.opts;
  return $$bh$js_obj("provider", routing.provider, "providerTarget", routing.target, "liveInput", (providers_module.providerLiveInput)(routing.provider), "model", ((_logical) => (_logical !== false && _logical != null ? _logical : opts.model))(routing.resolvedModel), "effort", ((_logical) => (_logical !== false && _logical != null ? _logical : opts.routingMetadata.reasoning))(routing.resolvedEffort));
}

function refresh_identity_route_bang(state, required) {
  state.liveInputRoute.refresh(active_route(state), required);
  return null;
}

function end_run_bang(state, outcome) {
  (state.outcome = outcome);
  (() => { try {
    return state.channel.end();
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __ = _catch_2;
        return null;
        break;
      }
    }
  } })();
  return null;
}

async function observe_wire_event_bang(state, event) {
  const writer = state.wireWriter;
  const committer = state.wireCommitter;
  if (((writer == null) || (committer == null))) {
    (() => { throw new Error("wire event observed before run admission"); })();
  }
  const events = writer.events();
  const canonical = events[state.nextObservedSequence];
  const comparison = $$bh$js_obj("matches", (canonical === event));
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(comparison.matches)) && canonical))) {
    (() => { try {
    return (comparison.matches = ((wire_module.encodeWireJsonlLine)(canonical) === (wire_module.encodeWireJsonlLine)(event)));
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __ = _catch_3;
        return null;
        break;
      }
    }
  } })();
  }
  if (((canonical == null) || (!((_truthy) => _truthy !== false && _truthy != null)(comparison.matches)))) {
    (() => { throw new Error("provider yielded an event that differs from its shared writer canonical event"); })();
  }
  await committer.commitThrough(canonical);
  const observation = state.executionFold.observe(canonical);
  state.shadowReviewerInterruptGate.observe(canonical);
  if (((_truthy) => _truthy !== false && _truthy != null)(state.shadowReviewer)) {
    state.shadowReviewer.observe(canonical);
  }
  (state.nextObservedSequence = (state.nextObservedSequence + 1));
  return observation;
}

async function observe_committed_wire_events_bang(state) {
  const writer = state.wireWriter;
  const committer = state.wireCommitter;
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? committer : _logical))(writer))) {
    await (async () => {  while (true) {
    const events = writer.events(); if ((state.nextObservedSequence < events.length)) { await observe_wire_event_bang(state, events[state.nextObservedSequence]);  continue; } else { return null; }
  } })();
  }
  return null;
}

async function start_wire_run_bang(state) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.wireWriter)) {
    return state.wireWriter;
  } else {
    const opened = await (stream_writer_module.StreamWriter.open)(state.agentId);
    const writer = foreign_new_bang(wire_module, "WireEventWriter", $$bh$array($$bh$js_obj("runId", (wire_module.wireRunId)(state.runId))));
    const committer = foreign_new_bang(stream_writer_module, "SerializedWireEventCommitter", $$bh$array(writer, (run_ledger_module.createWireEventStorePublisher)(state.wireIdentity), opened));
    (state.stream = opened);
    (state.wireWriter = writer);
    (state.wireCommitter = committer);
    const started = writer.append(((state.parentRunId == null) ? $$bh$js_obj("kind", "run.started", "lifecycle", "running", "owner", state.agentId) : $$bh$js_obj("kind", "run.started", "lifecycle", "running", "owner", state.agentId, "parentRunId", state.parentRunId)));
    await observe_wire_event_bang(state, started);
    return writer;
  }
}

async function before_provider_fallback_bang(state, transition) {
  await state.liveInputRoute.beforeFallback(transition, () => (resource_envelopes_module.reserveResourceEnvelopeRetry)(state.envelopeAdmission));
  return null;
}

async function admit_provider_route_bang(state, decision, evidence, authority) {
  const admitted = $$bh$js_obj("provider", decision.provider);
  if (((_truthy) => _truthy !== false && _truthy != null)(evidence)) {
    (admitted.evidence = evidence);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(authority)) {
    (admitted.authority = authority);
  }
  (state.admittedRoute = admitted);
  if (((_truthy) => _truthy !== false && _truthy != null)(authority)) {
    await state.liveInputRoute.activate(Object.assign(Object(), active_route(state), $$bh$js_obj("liveInput", authority.liveInput)), (authority.liveInput === "turn-messages"));
    console.log($$bc$str("[spawn] effective authority: ", (providers_module.formatProviderAuthoritySurface)(authority)));
  }
  return null;
}

function record_provider_route_bang(state, decision) {
  if (((_truthy) => _truthy !== false && _truthy != null)(state.worktreeLease)) {
    (worktree_module.recordWorktreeAuthorityProfile)(state.worktreeLease.allocation, (worktree_module.resolvedWorktreeAuthorityProfile)(decision));
  }
  return null;
}

async function publish_shadow_note_bang(state, note, signal) {
  await Reflect.apply(state.shadowNotePublisher, null, $$bh$array(state.agentId, note, state.shadowNoteCapability, signal));
  if ((note.severity === "blocker")) {
    await state.shadowReviewerInterruptGate.interruptIfArmed(state.liveInputRoute.isArmed(), signal, (((_truthy) => _truthy !== false && _truthy != null)(state.activeQuery) ? state.activeQuery.interruptTurn : null));
  }
  return null;
}

function orchestrator_terminal_decision_bang(state) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(state.orchestrator))) {
    return $$bh$js_obj("handled", false, "continue", false);
  } else {
    const decision = (children_module.decideChildTurnEnd)(state.childContinuation, Reflect.apply(state.readChildSettlement, null, $$bh$array(state.agentId)), (bgtasks_module.maxBgContinuations)());
    (state.childContinuation = decision.state);
    return (((decision.action === "continue")) ? (() => { const continuation = (((decision.reason === "children_live")) ? (() => { console.error($$bc$str("[harness] @agent:", state.agentId, " refusing orchestrator turn-end — ", decision.live.length, " live child lane(s): ", decision.live.join(", "), " (no-progress ", decision.attempt, "/", decision.cap, ")"));
return (children_module.childContinuationMessage)(decision.live); })() : ((decision.reason === "child_dispatch_required")) ? (() => { console.error($$bc$str("[harness] @agent:", state.agentId, " requiring direct-child dispatch — ", decision.children.length, "/", decision.required, " child lane(s) observed (no-progress ", decision.attempt, "/", decision.cap, ")"));
return (children_module.childDispatchMessage)(decision.children, decision.required); })() : (() => { console.error($$bc$str("[harness] @agent:", state.agentId, " requiring post-settlement reduction — ", decision.children.length, " settled child lane(s): ", decision.children.join(", ")));
return (children_module.childReductionMessage)(decision.children); })()); (state.pendingContinuation = decision.reason);
if (((_truthy) => _truthy !== false && _truthy != null)(state.resumeContinuations)) {
  if ((state.activeQuery.continueTurn == null)) {
    end_run_bang(state, "provider_error");
    (state.providerErrorDetail = "active provider cannot retain a private continuation turn");
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT BLOCKED", "detail", state.providerErrorDetail));
    return $$bh$js_obj("handled", true, "continue", false);
  } else {
    console.error($$bc$str("[harness] @agent:", state.agentId, " opening a provider-neutral resumed continuation turn"));
    (state.privateContinuation = continuation);
    return $$bh$js_obj("handled", true, "continue", true);
  }
} else {
  state.channel.push(continuation);
  return $$bh$js_obj("handled", true, "continue", true);
} })() : ((decision.action === "block")) ? (() => { const blocked_outcome = (((decision.reason === "child_reconciliation_unavailable")) ? "child_reconciliation_unavailable" : ((decision.reason === "child_set_regressed")) ? "orchestrator_child_set_inconsistent" : ((decision.reason === "child_dispatch_continuation_cap")) ? "orchestrator_child_obligation_unmet" : "orchestrator_children_incomplete"); const detail = ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (decision.missing.length > 0) : _logical))(decision.missing))) ? $$bc$str(" (missing previously observed: ", decision.missing.join(", "), ")") : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (decision.live.length > 0) : _logical))(decision.live))) ? $$bc$str(" (", decision.live.join(", "), ")") : ((!(decision.required == null))) ? $$bc$str(" (", (((_truthy) => _truthy !== false && _truthy != null)(decision.children) ? decision.children.length : 0), "/", decision.required, " direct children)") : ""); console.error($$bc$str("[harness] @agent:", state.agentId, " orchestrator completion blocked: ", decision.reason, detail));
end_run_bang(state, blocked_outcome);
return $$bh$js_obj("handled", true, "continue", false); })() : $$bh$js_obj("handled", false, "continue", false));
  }
}

async function handle_successful_terminal_bang(state, observation) {
  await (live_input_route_module.prepareManagedTerminalFollowUp)(state.liveInputRoute, state.termination);
  if (((_truthy) => _truthy !== false && _truthy != null)(((state.channel.pending() === 0) && state.shadowReviewer))) {
    await state.shadowReviewer.settleEligibleUpdates();
    await (live_input_route_module.prepareManagedTerminalFollowUp)(state.liveInputRoute, state.termination);
  }
  if ((state.channel.pending() > 0)) {
    return true;
  } else {
    const result = state.result;
    const run_state = observation.state;
    const background = run_state.pendingBackgroundTasks;
    return ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (result.trim() === "") : _logical))(state.pendingContinuation) : _logical))(state.orchestrator))) ? (() => { const raced = (children_module.continuationRaceOutcome)(state.pendingContinuation); console.error($$bc$str("[harness] @agent:", state.agentId, " orchestrator ", state.pendingContinuation, " continuation answered by an empty provider terminal — ", "closing-stream race, recording ", raced, " (never ran_empty)"));
end_run_bang(state, raced);
return false; })() : (((background.length > 0) && (state.bgContinuations < (bgtasks_module.maxBgContinuations)()))) ? (() => { const next_count = (state.bgContinuations + 1); const continuation = (bgtasks_module.bgContinuationMessage)(background); (state.bgContinuations = next_count);
console.error($$bc$str("[harness] @agent:", state.agentId, " refusing turn-end exit — ", background.length, " live background task(s): ", background.join(", "), " (continuation ", next_count, "/", (bgtasks_module.maxBgContinuations)(), ")"));
if (((_truthy) => _truthy !== false && _truthy != null)(state.resumeContinuations)) {
  (state.privateContinuation = continuation);
} else {
  state.channel.push(continuation);
}
return true; })() : ((background.length > 0)) ? (() => { console.error($$bc$str("[harness] @agent:", state.agentId, " continuation cap (", (bgtasks_module.maxBgContinuations)(), ") reached with ", background.length, " task(s) still live — blocking terminal"));
end_run_bang(state, "background_tasks_incomplete");
return false; })() : (() => { const child_flow = orchestrator_terminal_decision_bang(state); if (((_truthy) => _truthy !== false && _truthy != null)(child_flow.handled)) {
  return child_flow.continue;
} else {
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(state.emptyResultRepairAttempted)) && ((_logical) => (_logical !== false && _logical != null ? state.termination.emptyResultRepairAllowed() : _logical))((execution_outcome.isEmptyResultTerminal)(run_state.run))))) {
    const repair_mode = (empty_result_repair_module.emptyResultRepairMode)(state.activeQuery);
    if (((_truthy) => _truthy !== false && _truthy != null)(repair_mode)) {
      (state.emptyResultRepairAttempted = true);
      const repair_input = (empty_result_repair_module.successfulEmptyResultRepairInput)();
      console.error($$bc$str("[empty-result] @agent:", state.agentId, " opening one same-session corrective turn"));
      if ((repair_mode === "streaming")) {
        (state.emptyResultRepairContinuation = true);
        (state.privateContinuation = repair_input);
      } else {
        state.termination.throwIfTerminated();
        state.channel.push(repair_input);
      }
      return true;
    } else {
      end_run_bang(state, "ran");
      return false;
    }
  } else {
    end_run_bang(state, "ran");
    return false;
  }
} })());
  }
}

async function handle_provider_event_bang(state, agent_options, event) {
  const observation = await observe_wire_event_bang(state, event);
  const run_state = observation.state;
  state.liveInputRoute.observeCommittedEvent(event);
  if (((_truthy) => _truthy !== false && _truthy != null)(observation.activityKind)) {
    state.executionActivity.record("outer", observation.activityKind);
    (harness_module.renewHarnessPresence)(agent_options);
  }
  refresh_identity_route_bang(state, false);
  if ((run_state.compactions > state.announcedCompactions)) {
    (state.announcedCompactions = run_state.compactions);
    console.error($$bc$str("[harness] @agent:", state.agentId, " context compaction #", state.announcedCompactions));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (observation.backgroundTask.kind === "settled") : _logical))(observation.backgroundTask))) {
    (state.bgContinuations = 0);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(observation.struggleTrigger)) {
    console.error($$bc$str("[struggle] @agent:", state.agentId, " sensor fired: ", observation.struggleTrigger, " (model calls ", run_state.run.usage.lifetime.modelCalls, ", ", run_state.struggle.errorCount, " tool error(s)) — recorded as execution-axis evidence, ", "no in-flight change"));
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (event.kind === "model-call.completed") : _logical))(event.essential)))) {
    return true;
  } else {
    const reviewer_cancelled = state.shadowReviewerInterruptGate.consumeReviewerCancellation(event);
    const turn_result = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(run_state.lastCompletedAssistantOutput);
    const turn_evidence = event.evidence.turns;
    if (((_truthy) => _truthy !== false && _truthy != null)(state.lifecycleJournal)) {
      state.lifecycleJournal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.turnBoundary, $$bh$js_obj("status", event.status, "errorCode", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(event.errorCode), "turnUnit", (((_truthy) => _truthy !== false && _truthy != null)(turn_evidence) ? turn_evidence.unit : null), "turnCount", (((_truthy) => _truthy !== false && _truthy != null)(turn_evidence) ? turn_evidence.count : null), "resultBytes", Buffer.byteLength(turn_result)));
    }
    if (((_truthy) => _truthy !== false && _truthy != null)((wire_module.isIntermediateProviderSessionReplacement)(event))) {
      return true;
    } else {
      (state.result = turn_result);
      const token_budget = state.termination.observeCompletedCallUsage(run_state.run);
      return ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (token_budget.state === "budget_limited") : _logical))(token_budget))) ? (() => { end_run_bang(state, execution_outcome.RUN_TOKEN_BUDGET_LIMITED_OUTCOME);
(state.terminalSignal = $$bh$js_obj("subject", "TOKEN TARGET", "detail", JSON.stringify((query_lifecycle_module.managedRunTokenBudgetHandoff)(token_budget))));
return false; })() : (((event.errorCode === "provider_max_turns") || ((event.errorCode === "provider_budget_exhausted") || (event.errorCode === "provider_structured_output_retries_exhausted")))) ? (() => { const cap = event.errorCode; const partial = ((!(turn_result.trim() === "")) ? $$bc$str("partial: ", turn_result.trim().slice(0, 200)) : "no partial result"); const detail = $$bc$str(cap, " — ", partial); end_run_bang(state, ((cap === "provider_max_turns") ? "max_turns" : "capped"));
(state.terminalSignal = $$bh$js_obj("subject", "TURN CAP", "detail", detail));
state.terminalAuxiliaryWrites.push((timeout_ms) => (watchdog_module.notifyTurnCap)(state.agentId, detail, Object(), timeout_ms));
return false; })() : (((_truthy) => _truthy !== false && _truthy != null)(run_state.deadlineExceededDetail)) ? (() => { const detail = run_state.deadlineExceededDetail; (state.deadlineExceededDetail = detail);
end_run_bang(state, "deadline_exceeded");
console.error($$bc$str("[deadline_exceeded] @agent:", state.agentId, " process=deadline_exceeded detail=", detail));
(state.terminalSignal = $$bh$js_obj("subject", "DEADLINE EXCEEDED", "detail", detail));
return false; })() : ((!(event.status === "succeeded"))) ? await (async () => { if ((reviewer_cancelled && (!((_truthy) => _truthy !== false && _truthy != null)(state.termination.signal.aborted)))) {
  await (live_input_route_module.prepareManagedTerminalFollowUp)(state.liveInputRoute, state.termination);
}
if ((reviewer_cancelled && (state.channel.pending() > 0))) {
  return true;
} else {
  end_run_bang(state, "provider_error");
  (state.providerErrorDetail = ((_logical) => (_logical !== false && _logical != null ? _logical : "model-call terminal failed without diagnostic evidence"))(run_state.providerErrorDetail));
  console.error($$bc$str("[provider_error] @agent:", state.agentId, " ", state.providerErrorDetail));
  (state.terminalSignal = $$bh$js_obj("subject", "AGENT BLOCKED", "detail", state.providerErrorDetail));
  return false;
} })() : await handle_successful_terminal_bang(state, observation));
    }
  }
}

async function run_provider_loop_bang(state, agent_options) {
  await (async () => {  while (true) {
    (state.privateContinuation = null); await (async () => { const active_query = state.activeQuery; const source_iterator = Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(active_query, Symbol.asyncIterator), active_query, $$bh$array()); const watched = (watchdog_module.withStallWatchdog)(source_iterator, $$bh$js_obj("stallMs", state.watchdogWindow, "onStall", (minutes) => (watchdog_module.notifyStall)(state.agentId, minutes, $$bh$js_obj("coordinator", state.coordinator)), "onAbort", (evidence) => { (state.watchdogAbort = evidence);
return null; }, "activitySources", [state.executionActivity.source])); const iterator = Reflect.apply((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(watched, Symbol.asyncIterator), watched, $$bh$array()); return (async () => {  while (true) {
    const step = await iterator.next(); if ((!((_truthy) => _truthy !== false && _truthy != null)(step.done))) { const keep_reading = await handle_provider_event_bang(state, agent_options, step.value); if (keep_reading) {  continue; } else { return null; } } else { return null; }
  } })(); })(); const continuation = state.privateContinuation; if (((_truthy) => _truthy !== false && _truthy != null)(continuation)) { if (((_truthy) => _truthy !== false && _truthy != null)(state.emptyResultRepairContinuation)) { const resume_state = $$bh$js_obj("resume", true); state.channel.end(); await (async () => { try {
    return await state.liveInputRoute.freezeAndUnbind();
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const error = _catch_4;
        if ((state.liveInputFreezeError == null)) {
          (state.liveInputFreezeError = error);
        }
        (state.emptyResultRepairContinuation = false);
        return (resume_state.resume = false);
        break;
      }
    }
  } })(); (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((state.activeQuery.continueTurn == null) || (!((_truthy) => _truthy !== false && _truthy != null)(state.termination.emptyResultRepairAllowed()))) : _logical))(resume_state.resume)) ? (() => { (state.emptyResultRepairContinuation = false);
return (resume_state.resume = false); })() : null); (((_truthy) => _truthy !== false && _truthy != null)(resume_state.resume) ? await (async () => { state.termination.throwIfTerminated();
return (async () => { try {
    return await (state.activeQuery.continueTurn)(continuation);
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const __ = _catch_5;
        state.termination.throwIfTerminated();
        (state.emptyResultRepairContinuation = false);
        return (resume_state.resume = false);
        break;
      }
    }
  } })(); })() : null); (state.emptyResultRepairContinuation = false); if (((_truthy) => _truthy !== false && _truthy != null)(resume_state.resume)) {  continue; } else { return null; } } else { if ((state.activeQuery.continueTurn == null)) { return (() => { end_run_bang(state, "provider_error");
(state.providerErrorDetail = "active provider cannot retain a private continuation turn");
return (state.terminalSignal = $$bh$js_obj("subject", "AGENT BLOCKED", "detail", state.providerErrorDetail)); })(); } else { await (state.activeQuery.continueTurn)(continuation);  continue; } } } else { return null; }
  } })();
  return null;
}

async function finalize_run_bang(state) {
  const outcome_before_cleanup = state.outcome;
  const reached_provider_success = (outcome_before_cleanup === "ran");
  const termination = state.termination;
  const agent_id = state.agentId;
  const opts = state.opts;
  const routing = state.routing;
  const routing_metadata = state.routingMetadata;
  const lifecycle_journal = state.lifecycleJournal;
  const worktree_lease = state.worktreeLease;
  const session_hard_cap = termination.hardCapStatus();
  const host_signal = termination.hostSignal();
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (state.watchdogAbort == null) : _logical))(session_hard_cap))) {
    (state.outcome = "session_hard_cap");
    (state.providerErrorDetail = null);
    (state.worktreeTerminalFailure = $$bh$js_obj("code", "session_hard_cap", "phase", "provider_execution"));
    (state.terminalSignal = $$bh$js_obj("subject", "SESSION CAP", "detail", $$bc$str("managed session reached ", session_hard_cap.hardCapMs, "ms hard cap; handoff=", ((_logical) => (_logical !== false && _logical != null ? _logical : "unavailable"))(session_hard_cap.handoffPath), "; handoff_index=", (((_truthy) => _truthy !== false && _truthy != null)(session_hard_cap.indexed) ? "thread" : "outbox"))));
  } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (state.watchdogAbort == null) : _logical))(host_signal))) {
    const error = new Error($$bc$str("host terminated by ", host_signal));
    (state.outcome = "died");
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT DEATH", "detail", (death_module.deathReason)(error)));
  } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (state.watchdogAbort == null) : _logical))(state.queryCloseError))) {
    const close_error = state.queryCloseError;
    const error = ((close_error instanceof Error) ? close_error : new Error("provider query cleanup failed"));
    (state.outcome = "died");
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT DEATH", "detail", (death_module.deathReason)(error)));
  } else {
    null;
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((state.watchdogAbort == null) && (session_hard_cap == null)) : _logical))(state.liveInputFreezeError))) {
    const retry_state = $$bh$js_obj("succeeded", false);
    const original = state.liveInputFreezeError;
    await (async () => { try {
    await state.liveInputRoute.freezeAndUnbind();
  return (retry_state.succeeded = true);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const __ = _catch_6;
        return null;
        break;
      }
    }
  } })();
    const error = ((original instanceof Error) ? original : new Error("managed live-input route could not be frozen"));
    const settlement_failed = $$bc$record_instance_p("north.spawn/LiveFeedReapTimeoutError", error);
    if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!settlement_failed) : _logical))(retry_state.succeeded))) {
      (state.liveInputFreezeError = null);
    } else {
      if ((reached_provider_success && (!settlement_failed))) {
        console.error($$bc$str("[live-input] @agent:", agent_id, " terminal live-feed drain failed after a completed provider turn — ", "process/delivery preserved (", error.message, ")"));
      } else {
        (state.outcome = "died");
        (state.terminalSignal = $$bh$js_obj("subject", "AGENT DEATH", "detail", (death_module.deathReason)(error)));
        state.terminalAuxiliaryWrites.push((timeout_ms) => (death_module.notifyDeath)(agent_id, error, $$bh$js_obj("thread", null), timeout_ms));
      }
    }
  }
  const final_children = Reflect.apply(state.readChildSettlement, null, $$bh$array(agent_id));
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (state.outcome === "ran") : _logical))(state.orchestrator))) {
    const finalization = (children_module.assessChildFinalization)(state.childContinuation, final_children);
    if ((!((_truthy) => _truthy !== false && _truthy != null)(finalization.ok))) {
      (state.outcome = finalization.outcome);
      if ((finalization.outcome === "orchestrator_child_set_inconsistent")) {
        console.error($$bc$str("[harness] @agent:", agent_id, " CHILD SET REGRESSED: missing previously observed ", "coordinator relation(s) ", (((_truthy) => _truthy !== false && _truthy != null)(finalization.missing) ? finalization.missing.join(", ") : "(unknown)"), "; terminal cannot be process=ran"));
      }
    }
  }
  if ((final_children.kind === "live")) {
    const live = final_children.live;
    const child_detail = $$bc$str(live.length, " live child(ren): ", live.join(","));
    state.terminalAuxiliaryWrites.push((timeout_ms) => (children_module.notifyEarlyExitChildren)(agent_id, live, Object(), timeout_ms));
    (state.terminalSignal = (((_truthy) => _truthy !== false && _truthy != null)(state.terminalSignal.subject) ? Object.assign(Object(), state.terminalSignal, $$bh$js_obj("detail", [state.terminalSignal.detail, child_detail].filter((value) => (!(value == null))).join("; "))) : $$bh$js_obj("subject", "EARLY EXIT WITH LIVE CHILDREN", "detail", child_detail)));
  } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (final_children.kind === "unavailable") : _logical))(state.orchestrator))) {
    console.error($$bc$str("[harness] @agent:", agent_id, " CHILD SETTLEMENT UNAVAILABLE: ", final_children.reason, "; terminal cannot be process=ran"));
  } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((state.outcome === "orchestrator_reduction_incomplete") && (final_children.kind === "settled")) : _logical))(state.orchestrator))) {
    console.error($$bc$str("[harness] @agent:", agent_id, " CHILD RESULTS UNREDUCED: settled set changed or lacked a ", "completed reduction turn (", final_children.children.join(", "), "); terminal cannot be process=ran"));
  } else if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((state.outcome === "orchestrator_child_obligation_unmet") && (final_children.kind === "settled")) : _logical))(state.orchestrator))) {
    console.error($$bc$str("[harness] @agent:", agent_id, " DIRECT-CHILD OBLIGATION UNMET: ", final_children.children.length, "/", state.childContinuation.requiredChildren, " direct child lane(s) observed; terminal cannot be process=ran"));
  } else {
    null;
  }
  const execution_before_cleanup = state.executionFold.snapshot();
  if (((_truthy) => _truthy !== false && _truthy != null)(((state.outcome === "ran") && ((_logical) => (_logical !== false && _logical != null ? (execution_before_cleanup.pendingBackgroundTasks.length > 0) : _logical))(execution_before_cleanup)))) {
    (state.outcome = "background_tasks_incomplete");
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT BLOCKED", "detail", $$bc$str(execution_before_cleanup.pendingBackgroundTasks.length, " background task(s) remained open")));
  }
  const worktree_harvest = (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? (worktree_module.worktreeFinalize)(agent_id, state.outcome, worktree_lease, state.worktreeTerminalFailure) : null);
  (state.worktreeHarvest = worktree_harvest);
  if (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease)) {
    (worktree_lease.finalized = true);
  }
  refresh_identity_route_bang(state, false);
  const delivery_state = $$bh$js_obj("assessment", null);
  if (((_truthy) => _truthy !== false && _truthy != null)(((state.outcome === "ran") && opts.thread))) {
    if (((!((_truthy) => _truthy !== false && _truthy != null)(state.deliveryReservationReady)) || ((state.deliveryReservation == null) || (state.deliveryRuntime == null)))) {
      (delivery_state.assessment = $$bh$js_obj("deliveryOutcome", "unverified", "deliveryReason", "delivery_reservation_unavailable_at_finalize"));
    } else {
      const runtime = state.deliveryRuntime;
      const resolution = (delivery_evidence_module.resolveDeliveryRunState)(state.runId, (id) => Reflect.apply(runtime.load, null, $$bh$array(id)), runtime.loadOptions);
      const run_state = (((_truthy) => _truthy !== false && _truthy != null)(resolution.transientFailure) ? null : resolution.state);
      if (((run_state == null) || (!((_truthy) => _truthy !== false && _truthy != null)(run_state.reservationValid)))) {
        (state.deliveryReservationReady = false);
        console.error($$bc$str("[delivery] @", state.runId, " ", (((_truthy) => _truthy !== false && _truthy != null)(resolution.transientFailure) ? $$bc$str("reservation unreadable at finalize after ", resolution.attempts, " attempt(s) (", resolution.transientFailure, ")") : "reservation invalid at finalize"), "; retaining the wire run identity and leaving delivery unverified"));
        (delivery_state.assessment = $$bh$js_obj("deliveryOutcome", "unverified", "deliveryReason", (((_truthy) => _truthy !== false && _truthy != null)(resolution.transientFailure) ? "delivery_reservation_load_failed_at_finalize" : "delivery_reservation_unavailable_at_finalize")));
      } else {
        const load_thread_facts = ((_logical) => (_logical !== false && _logical != null ? _logical : north_client_module.getThreadFacts))(state.injected.loadThreadFacts);
        const thread_resolution = (delivery_evidence_module.resolveThreadFacts)(opts.thread, (id) => Reflect.apply(load_thread_facts, null, $$bh$array(id)), state.injected.threadFactsLoadOptions);
        if (((_truthy) => _truthy !== false && _truthy != null)(thread_resolution.transientFailure)) {
          console.error($$bc$str("[delivery] @", opts.thread, " thread unreadable at finalize after ", thread_resolution.attempts, " attempt(s) (", thread_resolution.transientFailure, "); leaving delivery unverified"));
          (delivery_state.assessment = $$bh$js_obj("deliveryOutcome", "unverified", "deliveryReason", "delivery_thread_load_failed_at_finalize"));
        } else {
          (delivery_state.assessment = (delivery_verification_module.assessThreadDelivery)(opts.thread, agent_id, ((_logical) => (_logical !== false && _logical != null ? _logical : []))(thread_resolution.facts), state.deliveryReservation.baselineDoneWhen.map((value) => $$bh$js_obj("predicate", "done_when", "value", value)), state.runId, run_state.evidence));
        }
      }
    }
  }
  const delivery = delivery_state.assessment;
  const terminal = (execution_outcome.classifyExecutionTerminal)(state.outcome, delivery);
  const terminal_detail = ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : state.outcome))(state.preflightDetail)))(state.providerErrorDetail)))(state.deadlineExceededDetail)))(state.terminalSignal.detail);
  const final_writer = await start_wire_run_bang(state);
  const wire_terminal = (execution_outcome.wireTerminalDecision)(state.outcome, terminal_detail, state.watchdogAbort);
  const wire_terminal_events = final_writer.terminate(wire_terminal);
  (state.terminal = terminal);
  for (const event of wire_terminal_events) {
  await observe_wire_event_bang(state, event);
};
  if (((_truthy) => _truthy !== false && _truthy != null)(state.wireCommitter)) {
    await state.wireCommitter.commitAll();
  }
  const final_execution = state.executionFold.snapshot();
  if (((final_execution == null) || ((final_execution.run.lifecycle === "running") || (final_execution.run.lifecycle === "waiting")))) {
    (() => { throw new Error("wire run did not reach its outer terminal"); })();
  }
  const final_turn = latest_turn_evidence(final_execution);
  const num_turns = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (final_turn.unit === "assistant-turn") : _logical))(final_turn)) ? final_turn.count : (((terminal.processOutcome === "blocked_preflight") || (terminal.processOutcome === "blocked_spend_guard")) ? 0 : null));
  const journal_state = $$bh$js_obj("error", null);
  (state.numTurns = num_turns);
  (() => { try {
    if (((_truthy) => _truthy !== false && _truthy != null)(lifecycle_journal)) {
    lifecycle_journal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.terminal, $$bh$js_obj("outcome", state.outcome, "processOutcome", terminal.processOutcome, "deliveryOutcome", terminal.deliveryOutcome, "deliveryReason", terminal.deliveryReason, "deliveryProof", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(terminal.deliveryProof), "numTurns", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(num_turns), "resultBytes", Buffer.byteLength(state.result)));
    const harvest = state.worktreeHarvest;
    return lifecycle_journal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.harvest, (((_truthy) => _truthy !== false && _truthy != null)(harvest) ? $$bh$js_obj("status", harvest.status, "branch", worktree_lease.branch, "sha", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(harvest.headOid), "ref", harvest.ref, "commits", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(harvest.commits), "reason", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(harvest.reason)) : $$bh$js_obj("status", "not-applicable", "branch", null, "sha", null)));
  }
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const error = _catch_7;
        return (journal_state.error = error);
        break;
      }
    }
  } })();
  const publication_budget = foreign_new_bang(terminal_notification_module, "TerminalPublicationBudget", $$bh$array());
  const terminal_publication = (identity_module.writeAgentTerminal)(agent_id, terminal, publication_budget.publicationTimeout(1));
  const auxiliary_writes = state.terminalAuxiliaryWrites;
  auxiliary_writes.forEach((write_auxiliary, index) => Reflect.apply(write_auxiliary, null, $$bh$array(publication_budget.publicationTimeout(((auxiliary_writes.length - index) + 2)))));
  const wire_events = final_writer.events();
  const final_route = active_route(state);
  const admitted_route = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (state.admittedRoute.provider === routing.provider) : _logical))(state.admittedRoute)) ? state.admittedRoute : null);
  const prompt_composition = ((_logical) => (_logical !== false && _logical != null ? _logical : ((routing.fallbackCount === 0) ? state.initialComposition : null)))((((_truthy) => _truthy !== false && _truthy != null)(admitted_route) ? admitted_route.evidence : null));
  const prompt_receipt = (((_truthy) => _truthy !== false && _truthy != null)(prompt_composition) ? prompt_composition.promptReceipt : null);
  const environment_receipt = (((_truthy) => _truthy !== false && _truthy != null)(prompt_composition) ? prompt_composition.environmentReceipt : null);
  const final_effort = ((_logical) => (_logical !== false && _logical != null ? _logical : routing_metadata.reasoning))(final_route.effort);
  const run_envelope_receipt = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? final_effort : _logical))(environment_receipt) : _logical))(prompt_receipt)) ? (composition_receipt_module.buildRunEnvelope)($$bh$js_obj("promptReceipt", prompt_receipt, "environmentReceipt", environment_receipt, "assignmentSha256", state.learningAssignment.manifestSha256, "capabilityFloor", routing_metadata.capabilityFloor, "effort", final_effort, "providerAdapterVersion", "north-managed-adapter:v1", "providerRuntimeVersion", $$bc$str("bun-", Bun.version))) : null);
  const active_query = state.activeQuery;
  const mcp_activity = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? active_query.mcpActivity : _logical))(active_query)) ? active_query.mcpActivity() : (tool_activity_module.unknownMcpActivity)("provider-activity-unavailable"));
  const native_command_activity = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? active_query.nativeCommandActivity : _logical))(active_query)) ? active_query.nativeCommandActivity() : (native_command_activity_module.unknownNativeCommandActivity)("provider-activity-unavailable"));
  const observed_token_budget = termination.tokenBudgetStatus();
  const token_budget = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (observed_token_budget.state === "budget_limited") : _logical))(observed_token_budget)) ? observed_token_budget : termination.observeCompletedCallUsage(final_execution.run));
  const requested = state.requested;
  const routing_economics = opts.routingEconomics;
  const provenance = $$bh$js_obj("posture", "spawn", "role", routing_metadata.role, "provider", routing.provider, "providerTarget", routing.target, "providerReason", routing.selectionReason, "requestedProvider", routing.requestedProvider, "requestedCapabilityFloor", routing_metadata.capabilityFloor, "requestedServiceClass", routing_metadata.serviceClass, "requestedEffort", routing_metadata.reasoning, "routingMetadata", routing_metadata, "routingAdmissionReceipt", routing_economics.receipt, "learningAssignment", state.learningAssignment, "mcpActivity", mcp_activity, "nativeCommandActivity", native_command_activity, "executionSource", "north-managed", "allocationMode", routing.allocationMode, "entitlementPressure", routing.entitlementPressure, "fallbackCount", routing.fallbackCount, "fallbackPath", routing.fallbackPath, "fallbackTargetPath", routing.fallbackTargetPath, "fallbackReasons", routing.fallbackReasons, "processOutcome", terminal.processOutcome, "deliveryOutcome", terminal.deliveryOutcome, "judgmentGrade", state.judgmentGrade, "struggleObservation", final_execution.struggle);
  const receipts = routing.modelAvailabilityReceipts;
  const receipt = (((_truthy) => _truthy !== false && _truthy != null)(receipts) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(receipts, routing.target) : null);
  if (((_truthy) => _truthy !== false && _truthy != null)(receipt)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "modelAvailability", (run_provenance_module.wireModelAvailabilityReceipt)(receipt));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(requested.target)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "requestedTarget", requested.target);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(routing_economics.assessment)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "routingAssessment", routing_economics.assessment);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(routing_economics.pinEvidence)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "routingPinEvidence", routing_economics.pinEvidence);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(prompt_composition)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "promptComposition", prompt_composition);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.shadowReviewerSummary)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "shadowReviewerSummary", state.shadowReviewerSummary);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(prompt_receipt)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "promptReceipt", prompt_receipt);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(environment_receipt)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "environmentReceipt", environment_receipt);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(run_envelope_receipt)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "runEnvelopeReceipt", run_envelope_receipt);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? active_query.executionTransport : _logical))(active_query))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "executionTransport", active_query.executionTransport);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? admitted_route.authority : _logical))(admitted_route))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "effectiveAuthority", admitted_route.authority);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(routing.allocationEvidenceByTarget)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "allocationEvidence", routing.allocationEvidenceByTarget);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.envelopeAdmission)) {
    const admission = state.envelopeAdmission;
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "envelopeScopes", admission.scopes.map((scope) => scope.id));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "envelopeRetries", admission.retries);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "envelopeAdvisories", admission.advisories);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(token_budget)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "tokenBudget", token_budget);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(terminal.deliveryReason)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "deliveryReason", terminal.deliveryReason);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(terminal.deliveryProof)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "deliveryProof", terminal.deliveryProof);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.retryContext)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "retryOfRun", state.retryContext.retryOfRun);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(provenance, "retryAttempt", state.retryContext.retryAttempt);
  }
  const run_ledger = (run_ledger_module.wireLedgerSummary)(wire_events);
  const run_publication = await (telemetry_module.recordWireRunTelemetry)(state.wireIdentity, final_execution.run, $$bh$js_obj("status", "recorded", "summary", run_ledger), provenance, publication_budget.publicationTimeout(1));
  (terminal_notification_module.notifyTerminalSettlement)(agent_id, state.coordinator, Object.assign(Object(), $$bh$js_obj("outcome", state.outcome, "terminal", terminal, "terminalPublication", terminal_publication, "runPublication", run_publication), state.terminalSignal), publication_budget.notificationTimeout());
  if (((_truthy) => _truthy !== false && _truthy != null)(journal_state.error)) {
    (() => { throw journal_state.error; })();
  }
  const struggle_snapshot = final_execution.struggle;
  const turns_label = ((!(num_turns == null)) ? $$bc$str(num_turns) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (final_turn.unit === "provider-turn") : _logical))(final_turn)) ? $$bc$str(final_turn.count, " provider turn(s)", ((!(final_turn.toolItems == null)) ? $$bc$str("/", final_turn.toolItems, " items") : "")) : "?"));
  console.log($$bc$str("[spawn] @agent:", agent_id, " complete (process=", state.outcome, ", delivery=", terminal.deliveryOutcome, ", turns=", turns_label, ", result=", state.result.length, "b", ((struggle_snapshot.triggers.length > 0) ? $$bc$str(", struggle: ", struggle_snapshot.triggers.join(",")) : ""), ")"));
  const sibling_target = routing.fallbackTargets.find((target) => ($$bh$aget(routing.routingTargets, target).provider === routing.provider));
  return $$bh$js_obj("result", state.result, "outcome", state.outcome, "runId", state.runId, "providerErrorDetail", state.providerErrorDetail, "numTurns", num_turns, "provider", routing.provider, "siblingTarget", sibling_target);
}

async function run_spawn_bang(opts, judgment_grade, struggle_policy, envelope_admission, injected, termination, worktree_lease, retry_context, retry_target, parent_run_id, learning_assignment, shadow_config, lifecycle_journal) {
  const routing_metadata = opts.routingMetadata;
  const capabilities = (orchestration_staffing.orchestrationCapabilities)(routing_metadata);
  const requested = $$bh$js_obj("provider", opts.provider, "target", opts.target, "capabilityFloor", routing_metadata.capabilityFloor, "serviceClass", routing_metadata.serviceClass, "model", opts.model, "reasoning", routing_metadata.reasoning);
  const agent_id = ((_logical) => (_logical !== false && _logical != null ? _logical : createSpawnAgentId()))(opts.agentId);
  const repo_root = (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.repoRoot : process.cwd());
  const run_id = (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.allocation.runId : (telemetry_module.newRunId)(agent_id));
  const assignment_writer = ((_logical) => (_logical !== false && _logical != null ? _logical : (((_truthy) => _truthy !== false && _truthy != null)(injected.queryFn) ? (__run_id, __assignment) => Promise.resolve("recorded") : learning_assignment_writer_module.publishLearningAssignment)))(injected.publishLearningAssignment);
  await publish_learning_assignment_for_run_bang(assignment_writer, run_id, learning_assignment);
  const bound_thread_id = ((_logical) => (_logical !== false && _logical != null ? _logical : process.env.AGENT_THREAD))(opts.thread);
  const run_context = (((_truthy) => _truthy !== false && _truthy != null)(bound_thread_id) ? (delivery_evidence_module.newDeliveryRunContext)(run_id, bound_thread_id, agent_id) : null);
  const delivery_runtime = ((_logical) => (_logical !== false && _logical != null ? _logical : (((_truthy) => _truthy !== false && _truthy != null)(injected.queryFn) ? null : $$bh$js_obj("reserve", delivery_evidence_module.reserveDeliveryRun, "load", delivery_evidence_module.loadDeliveryRunState))))(injected.deliveryRuntime);
  const provider_preference = ((_logical) => (_logical !== false && _logical != null ? _logical : "auto"))(opts.provider);
  const target_preference = opts.target;
  const routing_request = $$bh$js_obj("provider", provider_preference, "target", ((_logical) => (_logical !== false && _logical != null ? _logical : target_preference))(retry_target));
  const __pinned = (((_truthy) => _truthy !== false && _truthy != null)(injected.queryFn) ? Object() : (execution_admission_module.admitPinnedProvider)(opts.provider, capabilities));
  const routing_context = $$bh$js_obj("capabilityFloor", routing_metadata.capabilityFloor, "serviceClass", routing_metadata.serviceClass, "reasoning", routing_metadata.reasoning, "model", opts.model, "stableKey", agent_id, "capabilities", capabilities, "signal", termination.signal, "pinEvidence", opts.routingEconomics.pinEvidence);
  const routing_policy = (providers_module.resourcePolicyFromEnv)();
  const routing = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(injected.executionSelection)) : _logical))(injected.queryFn)) ? (providers_module.selectProvider)(routing_request, routing_policy, routing_context) : await (async () => { try {
    return await (providers_module.selectProviderForExecution)(routing_request, routing_policy, routing_context, (((_truthy) => _truthy !== false && _truthy != null)(injected.refreshAccountUsages) ? $$bh$js_obj("refreshAccountUsages", injected.refreshAccountUsages) : Object()));
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const error = _catch_8;
        termination.throwIfTerminated();
        return (() => { throw error; })();
        break;
      }
    }
  } })());
  termination.throwIfTerminated();
  const shadow_target = (shadow_reviewer_module.assignedShadowReviewerTarget)(shadow_config, learning_assignment, routing.routingTargets);
  const shadow_note_capability = (((_truthy) => _truthy !== false && _truthy != null)(shadow_target) ? (shadow_reviewer_note_module.mintShadowReviewerNoteCapability)() : null);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? injected.queryFn : _logical))(worktree_lease))) {
    (worktree_module.recordWorktreeAuthorityProfile)(worktree_lease.allocation, (worktree_module.resolvedWorktreeAuthorityProfile)(routing));
  }
  const resolved_model = ((_logical) => (_logical !== false && _logical != null ? _logical : opts.model))(routing.resolvedModel);
  const resolved_effort = ((_logical) => (_logical !== false && _logical != null ? _logical : routing_metadata.reasoning))(routing.resolvedEffort);
  const identity_role = routing_metadata.role;
  (opts.model = resolved_model);
  write_lane_meta_bang(agent_id, $$bh$js_obj("thread", bound_thread_id, "role", identity_role, "capabilityFloor", routing_metadata.capabilityFloor, "serviceClass", routing_metadata.serviceClass, "reasoning", resolved_effort, "model", resolved_model, "provider", routing.provider, "startedAt", new Date().toISOString()));
  const composition = routing_metadata.composition;
  const bespoke_p = (composition.kind === "bespoke");
  const template_p = (composition.kind === "template");
  const identity_base = $$bh$js_obj("kind", "lane", "role", identity_role, "compositionKind", composition.kind, "compositionId", composition.id, "compositionOverrides", (template_p ? composition.overrides : null), "compositionOverrideReason", (template_p ? composition.overrideReason : null), "compositionNearestTemplate", (bespoke_p ? composition.nearestTemplate : null), "compositionBespokeReason", (bespoke_p ? composition.bespokeReason : null), "compositionPromotionCandidate", (bespoke_p ? composition.promotionCandidate : null), "compositionContractFingerprint", (bespoke_p ? (identity_module.bespokeContractFingerprint)(composition.contract) : null), "compositionContractFingerprintVersion", (bespoke_p ? bespoke_contract_module.BESPOKE_FINGERPRINT_VERSION : null), "compositionContractFingerprintDomain", (bespoke_p ? bespoke_contract_module.BESPOKE_FINGERPRINT_DOMAIN : null), "repo", (identity_module.userAnchoredPath)(process.cwd()), "goal", (identity_module.goalFromPrompt)(opts.prompt), "coordinator", opts.coordinator, "worktree", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.path : null), "branch", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.branch : null), "retryOfAgent", (((_truthy) => _truthy !== false && _truthy != null)(retry_context) ? retry_context.retryOfAgent : null), "shadowReviewerNoteCapabilitySha256", (((_truthy) => _truthy !== false && _truthy != null)(shadow_note_capability) ? shadow_note_capability.sha256 : null));
  const initial_live_input = (providers_module.providerLiveInput)(routing.provider);
  const channel = (coordination_module.inputChannel)(opts.prompt);
  termination.attachInput(() => (() => { try {
    return channel.end();
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const __ = _catch_9;
        return null;
        break;
      }
    }
  } })());
  const live_input_route = foreign_new_bang(live_input_route_module, "ManagedLiveInputRoute", $$bh$array(agent_id, identity_base, $$bh$js_obj("provider", routing.provider, "providerTarget", routing.target, "liveInput", initial_live_input, "model", resolved_model, "effort", resolved_effort), (message) => channel.push(message), ((_logical) => (_logical !== false && _logical != null ? _logical : coordination_module.subscribeFeed))(injected.feedSubscriber)));
  await (identity_module.writeAgentFacts)(agent_id, Object.assign(Object(), identity_base, $$bh$js_obj("model", resolved_model, "provider", routing.provider, "providerTarget", routing.target, "liveInput", initial_live_input), live_input_route.initialProjection(), $$bh$js_obj("effort", resolved_effort)));
  if (((_truthy) => _truthy !== false && _truthy != null)(lifecycle_journal)) {
    lifecycle_journal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.identityAdmitted, $$bh$js_obj("thread", bound_thread_id, "role", identity_role, "provider", routing.provider, "target", routing.target, "capabilityFloor", routing_metadata.capabilityFloor, "serviceClass", routing_metadata.serviceClass, "reasoning", resolved_effort, "model", resolved_model, "worktree", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.path : null), "branch", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.branch : null)));
  }
  write_lane_meta_bang(agent_id, $$bh$js_obj("thread", bound_thread_id, "role", identity_role, "capabilityFloor", routing_metadata.capabilityFloor, "serviceClass", routing_metadata.serviceClass, "reasoning", resolved_effort, "model", resolved_model, "provider", routing.provider, "startedAt", new Date().toISOString()));
  const wire_identity = Object.assign(Object(), $$bh$js_obj("thread", ((_logical) => (_logical !== false && _logical != null ? _logical : "(ad-hoc)"))(bound_thread_id), "agent", agent_id), (((_truthy) => _truthy !== false && _truthy != null)(process.env.NORTH_THREAD_ID) ? $$bh$js_obj("parentThread", process.env.NORTH_THREAD_ID) : Object()), (((_truthy) => _truthy !== false && _truthy != null)(opts.coordinator) ? $$bh$js_obj("coordinator", opts.coordinator) : Object()));
  const state = $$bh$js_obj("opts", opts, "judgmentGrade", judgment_grade, "strugglePolicy", struggle_policy, "envelopeAdmission", envelope_admission, "injected", injected, "termination", termination, "worktreeLease", worktree_lease, "retryContext", retry_context, "parentRunId", parent_run_id, "learningAssignment", learning_assignment, "shadowConfig", shadow_config, "lifecycleJournal", lifecycle_journal, "routingMetadata", routing_metadata, "capabilities", capabilities, "requested", requested, "agentId", agent_id, "repoRoot", repo_root, "runId", run_id, "assignmentWriter", assignment_writer, "boundThreadId", bound_thread_id, "runContext", run_context, "deliveryRuntime", delivery_runtime, "deliveryReservation", null, "deliveryReservationReady", false, "deliveryLeaseClaim", null, "routing", routing, "shadowTarget", shadow_target, "shadowNoteCapability", shadow_note_capability, "resolvedModel", resolved_model, "resolvedEffort", resolved_effort, "identityRole", identity_role, "identityBase", identity_base, "channel", channel, "liveInputRoute", live_input_route, "executionFold", (execution_fold_module.makeExecutionFold)(struggle_policy), "shadowReviewer", null, "shadowReviewerSummary", (((_truthy) => _truthy !== false && _truthy != null)(shadow_config) ? (shadow_reviewer_module.inactiveShadowReviewerSummary)(shadow_config) : null), "shadowReviewerInterruptGate", foreign_new_bang(shadow_reviewer_module, "ShadowReviewerInterruptGate", $$bh$array()), "wireWriter", null, "stream", null, "wireCommitter", null, "nextObservedSequence", 0, "announcedCompactions", 0, "result", "", "outcome", "ran", "preflightDetail", null, "providerErrorDetail", null, "deadlineExceededDetail", null, "worktreeTerminalFailure", null, "initialComposition", null, "admittedRoute", null, "providerQueryConstructionStarted", false, "queryCloseError", null, "coordinator", opts.coordinator, "wireIdentity", wire_identity, "watchdogWindow", (watchdog_module.stallMs)(), "executionActivity", (execution_activity_module.createExecutionActivityEmitter)(), "watchdogAbort", null, "stopProviderActivity", () => null, "terminalSignal", Object(), "terminalAuxiliaryWrites", $$bh$array(), "liveInputFreezeError", null, "bgContinuations", 0, "orchestrator", (routing_metadata.topology === "orchestrator"), "readChildSettlement", ((_logical) => (_logical !== false && _logical != null ? _logical : children_module.settleChildren))(injected.childSettlementReader), "childContinuation", (children_module.initialChildContinuationState)((children_module.requiredDirectChildCount)(routing_metadata)), "pendingContinuation", null, "emptyResultRepairAttempted", false, "emptyResultRepairContinuation", false, "resumeContinuations", ((routing_metadata.topology === "orchestrator") && ((providers_module.providerLiveInput)(routing.provider) === "streaming")), "activeQuery", null);
  const query_function = ((_logical) => (_logical !== false && _logical != null ? _logical : (arguments$) => (providers_module.routedQuery)(routing, arguments$, routing_metadata.capabilityFloor, (transition) => before_provider_fallback_bang(state, transition), (decision, evidence, authority) => admit_provider_route_bang(state, decision, evidence, authority), (decision) => record_provider_route_bang(state, decision))))(injected.queryFn);
  (state.queryFunction = query_function);
  await (async () => { try {
    termination.throwIfTerminated();
  console.log($$bc$str("[spawn] @agent:", agent_id, " starting provider=", routing.provider, " target=", routing.target, " capabilityFloor=", routing_metadata.capabilityFloor, " serviceClass=", routing_metadata.serviceClass, " (", routing.selectionReason, ")"));
  if (((_truthy) => _truthy !== false && _truthy != null)(run_context)) {
    await (async () => { try {
    if (((_truthy) => _truthy !== false && _truthy != null)(delivery_runtime)) {
    const receipt = routing.executionAccountReceipt;
    let attempt_route = ((_logical) => (_logical !== false && _logical != null ? _logical : Object()))(delivery_runtime.attemptRoute);
    if ((Object.keys(attempt_route).length === 0)) {
      if (((!(routing.provider === "openai")) || (receipt == null))) {
        (() => { throw new Error("managed North spawn requires a Store-authorized execution account route"); })();
      }
      const acquire_leases = ((_logical) => (_logical !== false && _logical != null ? _logical : delivery_evidence_module.acquireDeliveryAttemptLeases))(delivery_runtime.acquireLeases);
      const lease = await Reflect.apply(acquire_leases, null, $$bh$array(run_context, routing.target));
      (state.deliveryLeaseClaim = lease);
      (attempt_route = $$bh$js_obj("provider", routing.provider, "accountId", routing.target, "model", resolved_model, "accountAuthorityReceiptSha256", receipt.accountAuthority.digest, "routeObservationReceiptSha256", receipt.usage.receipt.digest, "threadLease", lease.threadLease, "accountLease", lease.accountLease));
    }
    const reservation = (delivery_evidence_module.reserveDeliveryRunWithRecovery)(run_context, attempt_route, delivery_runtime.reserve, Object.assign(Object(), ((_logical) => (_logical !== false && _logical != null ? _logical : Object()))(delivery_runtime.reserveOptions), $$bh$js_obj("onRetry", (error, next_attempt, max_attempts, backoff_ms) => console.error($$bc$str("[delivery] @", state.runId, " reservation writer failed before provider; ", "relaunching the same reservation identity after ", backoff_ms, "ms (attempt ", next_attempt, "/", max_attempts, "): ", error.message)))));
    if ((reservation == null)) {
      (() => { throw new Error("reservation acknowledgement unavailable"); })();
    }
    (state.deliveryReservation = reservation);
    return (state.deliveryReservationReady = true);
  }
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const error = _catch_10;
        if ((!((_truthy) => _truthy !== false && _truthy != null)(state.deliveryReservationReady))) {
          if (((_truthy) => _truthy !== false && _truthy != null)(state.deliveryLeaseClaim)) {
            await state.deliveryLeaseClaim.release();
          }
          const attempted_run_id = state.runId;
          const next_run_id = (telemetry_module.newRunId)(agent_id);
          (state.runId = next_run_id);
          if (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease)) {
            (worktree_module.recordWorktreeRunRotation)(worktree_lease.allocation, next_run_id);
          }
          console.error($$bc$str("[delivery] @", attempted_run_id, " reservation unavailable; rotating blocked telemetry ", "to fresh non-reservation @", next_run_id, ": ", ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(error)))(error.message)));
          await publish_learning_assignment_for_run_bang(assignment_writer, next_run_id, learning_assignment);
          return (() => { throw error; })();
        }
        break;
      }
    }
  } })();
  }
  const artifacts = foreign_new_bang(run_artifacts_module, "RunArtifactStore", $$bh$array(state.runId));
  (state.artifacts = artifacts);
  if (((_truthy) => _truthy !== false && _truthy != null)(shadow_target)) {
    const review_runner = ((_logical) => (_logical !== false && _logical != null ? _logical : (update, signal) => (shadow_review_runner_module.runAnthropicShadowReview)(Object.assign(Object(), $$bh$js_obj("update", update, "target", shadow_target, "sourceAgentId", agent_id, "thread", ((_logical) => (_logical !== false && _logical != null ? _logical : "(ad-hoc)"))(bound_thread_id), "signal", signal), (((_truthy) => _truthy !== false && _truthy != null)(process.env.NORTH_THREAD_ID) ? $$bh$js_obj("parentThread", process.env.NORTH_THREAD_ID) : Object()), (((_truthy) => _truthy !== false && _truthy != null)(opts.coordinator) ? $$bh$js_obj("coordinator", opts.coordinator) : Object())))))(injected.shadowReviewRunner);
    const note_publisher = ((_logical) => (_logical !== false && _logical != null ? _logical : shadow_reviewer_note_module.publishShadowReviewerNote))(injected.publishShadowReviewerNote);
    (state.shadowNotePublisher = note_publisher);
    (state.shadowReviewer = foreign_new_bang(shadow_reviewer_module, "ShadowReviewer", $$bh$array(shadow_config, (wire_module.wireRunId)(state.runId), review_runner, (note, signal) => publish_shadow_note_bang(state, note, signal), $$bh$js_obj("signal", termination.signal, "home", process.env.HOME))));
  }
  const default_tools = ["Read", "Edit", "Write", "Bash", "Grep", "Glob"];
  const system_prompt = (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? $$bc$str(((_logical) => (_logical !== false && _logical != null ? _logical : harness_module.DEFAULT_SYSTEM_PROMPT))(opts.systemPrompt), (worktree_module.worktreePayload)($$bh$js_obj("path", worktree_lease.path, "branch", worktree_lease.branch, "mainReportsDir", $$bc$str(repo_root, "/docs/private")))) : opts.systemPrompt);
  const receipts = ((_logical) => (_logical !== false && _logical != null ? _logical : Object()))(routing.modelAvailabilityReceipts);
  const agent_options = (harness_module.harnessOptions)($$bh$js_obj("self", agent_id, "extraTools", ((_logical) => (_logical !== false && _logical != null ? _logical : default_tools))(opts.tools), "model", opts.model, "provider", routing.provider, "modelAvailability", $$bh$js_obj("exactModelPinned", (!(requested.model == null)), "targetId", routing.target, "receipt", (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(receipts, routing.target)), "routingMetadata", routing_metadata, "projectProfile", opts.projectProfile, "systemPrompt", system_prompt, "maxTurns", opts.maxTurns, "abortController", termination.abortController, "cwd", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.path : process.cwd()), "deliveryRun", (((_truthy) => _truthy !== false && _truthy != null)(state.deliveryReservationReady) ? run_context : null), "artifactDirectory", artifacts.directory));
  (state.agentOptions = agent_options);
  (state.initialComposition = (harness_module.harnessCompositionEvidence)(agent_options));
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? injected.feedSubscriber : _logical))(injected.queryFn))) {
    await live_input_route.activate(active_route(state));
  }
  termination.throwIfTerminated();
  (state.providerQueryConstructionStarted = true);
  const writer = await start_wire_run_bang(state);
  const active_query = Reflect.apply(state.queryFunction, null, $$bh$array($$bh$js_obj("input", channel.stream(), "options", agent_options, "writer", writer, "eventCommitter", state.wireCommitter, "artifacts", artifacts)));
  (state.activeQuery = active_query);
  termination.attachQuery(active_query);
  if ((active_query.executionTransport === "managed-app-server")) {
    await live_input_route.activate(active_route(state), true);
  }
  (state.stopProviderActivity)();
  (state.stopProviderActivity = (execution_activity_module.forwardExecutionActivity)(active_query.executionActivity, state.executionActivity));
  await run_provider_loop_bang(state, agent_options);
  await observe_committed_wire_events_bang(state);
  const provider_state = state.executionFold.snapshot();
  (state.providerState = provider_state);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((!(provider_state.latestModelCallTerminal.status === "succeeded")) && ((state.outcome === "ran") && (state.watchdogAbort == null))) : _logical))(provider_state))) {
    (state.outcome = "provider_error");
    (state.providerErrorDetail = ((_logical) => (_logical !== false && _logical != null ? _logical : NO__PROVIDER__TERMINAL__DETAIL))(provider_state.providerErrorDetail));
    console.error($$bc$str("[provider_error] @agent:", agent_id, " ", state.providerErrorDetail));
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT BLOCKED", "detail", state.providerErrorDetail));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((state.watchdogAbort == null) && ((state.outcome === "ran") && ((_logical) => (_logical !== false && _logical != null ? (execution_outcome.isEmptyResultTerminal)(provider_state.run) : _logical))(provider_state))))) {
    (state.outcome = execution_outcome.EMPTY_RESULT_OUTCOME);
    const turn_evidence = latest_turn_evidence(provider_state);
    const turns = (((_truthy) => _truthy !== false && _truthy != null)(turn_evidence) ? turn_evidence.count : "unknown turn count");
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT EMPTY RESULT", "detail", $$bc$str("provider success terminal with empty result (0b) after ", turns, " — no deliverable text committed (likely output-token ceiling hit mid extended-thinking/tool_use)")));
    console.error($$bc$str("[empty-result] @agent:", agent_id, " provider success terminal carried 0b result — ", "recording process=ran_empty (loud, non-clean)"));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(state.watchdogAbort)) {
    (state.outcome = "watchdog_aborted");
    (state.providerErrorDetail = null);
    const detail = (watchdog_module.describeWatchdogAbortEvidence)(state.watchdogAbort);
    const error = new Error(detail);
    console.error($$bc$str("[watchdog-abort] @agent:", agent_id, " ", detail));
    (state.terminalSignal = $$bh$js_obj("subject", "AGENT DEATH", "detail", (death_module.deathReason)(error)));
    state.terminalAuxiliaryWrites.push((timeout_ms) => (death_module.notifyDeath)(agent_id, error, $$bh$js_obj("thread", null), timeout_ms));
    return (async () => { try {
    return await termination.closeQuery(state.activeQuery);
  } catch (_catch_11) {
    switch ($$bd$catch_dispatch(_catch_11, [Error])) {
      case 0: {
        const error = _catch_11;
        return (state.queryCloseError = error);
        break;
      }
    }
  } })();
  }
  } catch (_catch_12) {
    switch ($$bd$catch_dispatch(_catch_12, [Error])) {
      case 0: {
        const error = _catch_12;
        return ((((_truthy) => _truthy !== false && _truthy != null)(termination.hardCapStatus())) ? (() => { (state.outcome = "session_hard_cap");
return (state.worktreeTerminalFailure = $$bh$js_obj("code", "session_hard_cap", "phase", "provider_execution")); })() : ($$bc$record_instance_p("north.spawn/ResourceEnvelopeExceededError", error)) ? (() => { (state.outcome = "resource_envelope_exceeded");
(state.worktreeTerminalFailure = $$bh$js_obj("code", "resource_envelope_retry_refused", "phase", "provider_preflight"));
return console.error($$bc$str("[envelope] @agent:", agent_id, " ", error.message)); })() : ($$bc$record_instance_p("north.spawn/ProviderRetrySafeError", error)) ? (() => { const carried = error.processOutcome; (state.outcome = ((typeof carried === "string") ? carried : "blocked_preflight"));
(state.worktreeTerminalFailure = $$bh$js_obj("code", "provider_preflight_refused", "phase", "provider_admission"));
(state.preflightDetail = (providers_module.providerRetrySafeTerminalDetail)(error));
return console.error($$bc$str("[", state.outcome, "] @agent:", agent_id, " ", (death_module.causeChain)(error))); })() : ((!((_truthy) => _truthy !== false && _truthy != null)(state.providerQueryConstructionStarted))) ? (() => { (state.outcome = "blocked_preflight");
(state.worktreeTerminalFailure = $$bh$js_obj("code", "spawn_pre_provider_setup_failed", "phase", "provider_preflight"));
(state.preflightDetail = "spawn failed during North pre-provider setup");
return console.error($$bc$str("[blocked_preflight] @agent:", agent_id, " spawn_pre_provider_setup_failed: ", (death_module.causeChain)(error))); })() : (() => { (state.outcome = "died");
(state.terminalSignal = $$bh$js_obj("subject", "AGENT DEATH", "detail", (death_module.deathReason)(error)));
return state.terminalAuxiliaryWrites.push((timeout_ms) => (death_module.notifyDeath)(agent_id, error, $$bh$js_obj("thread", null), timeout_ms)); })());
        break;
      }
    }
  } finally {
    (state.stopProviderActivity)();
    await (async () => { try {
    return await observe_committed_wire_events_bang(state);
  } catch (_catch_13) {
    switch ($$bd$catch_dispatch(_catch_13, [Error])) {
      case 0: {
        const error = _catch_13;
        if ((state.queryCloseError == null)) {
          return (state.queryCloseError = error);
        }
        break;
      }
    }
  } })();
    state.shadowReviewerInterruptGate.disarm();
    if (((_truthy) => _truthy !== false && _truthy != null)(state.shadowReviewer)) {
      (state.shadowReviewerSummary = await state.shadowReviewer.close());
    }
    await (async () => { try {
    return await live_input_route.freezeAndUnbind();
  } catch (_catch_14) {
    switch ($$bd$catch_dispatch(_catch_14, [Error])) {
      case 0: {
        const error = _catch_14;
        return (state.liveInputFreezeError = error);
        break;
      }
    }
  } })();
    end_run_bang(state, state.outcome);
    await (async () => { try {
    return await termination.closeQuery(state.activeQuery);
  } catch (_catch_15) {
    switch ($$bd$catch_dispatch(_catch_15, [Error])) {
      case 0: {
        const error = _catch_15;
        return (state.queryCloseError = error);
        break;
      }
    }
  } })();
    await (async () => { try {
    return await observe_committed_wire_events_bang(state);
  } catch (_catch_16) {
    switch ($$bd$catch_dispatch(_catch_16, [Error])) {
      case 0: {
        const error = _catch_16;
        if ((state.queryCloseError == null)) {
          return (state.queryCloseError = error);
        }
        break;
      }
    }
  } })();
  } })();
  return (async () => { try {
    return await finalize_run_bang(state);
  } finally {
    if (((_truthy) => _truthy !== false && _truthy != null)(state.stream)) {
      await state.stream.close();
    }
  } })();
}

const bootstrap_authority_granted = ({value: false, watches: {}});

function recursive_child_binding_error_bang(message) {
  const error = new Error(message);
  Object.setPrototypeOf(error, recursive_child_binding_error_bang.prototype);
  (error.name = "RecursiveChildBindingError");
  (error.code = "NORTH_RECURSIVE_CHILD_BINDING_REQUIRED");
  (error.preSideEffect = true);
  return error;
}

Object.setPrototypeOf(recursive_child_binding_error_bang.prototype, Error.prototype);

const RecursiveChildBindingError = recursive_child_binding_error_bang;

function normalized_north_entity_id(value) {
  return (north_client_module.normalizeNorthEntityId)(value);
}

function assert_recursive_child_binding_bang(composed, caller_topology, load_thread_facts) {
  if ((!(caller_topology === "orchestrator"))) {
    return null;
  } else {
    const env = process.env;
    const parent_thread = env.NORTH_THREAD_ID;
    const parent_run = env.NORTH_RUN_ID;
    const parent_capability = env.NORTH_RUN_CAPABILITY;
    const child_thread = composed.thread;
    if (((parent_thread == null) || ((parent_run == null) || ((parent_capability == null) || (child_thread == null))))) {
      (() => { throw new RecursiveChildBindingError("recursive SDK spawn requires an exact managed parent run and a fresh child thread"); })();
    }
    const parent_thread_value = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(parent_thread);
    const parent_run_value = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(parent_run);
    const child_thread_value = ((_logical) => (_logical !== false && _logical != null ? _logical : ""))(child_thread);
    const parent_run_id = (() => { try {
    return (wire_module.wireRunId)(parent_run_value);
  } catch (_catch_17) {
    switch ($$bd$catch_dispatch(_catch_17, [Error])) {
      case 0: {
        const __ = _catch_17;
        return (() => { throw new RecursiveChildBindingError("recursive SDK spawn received an invalid parent run id"); })();
        break;
      }
    }
  } })();
    const child = (() => { try {
    return normalized_north_entity_id(child_thread_value);
  } catch (_catch_18) {
    switch ($$bd$catch_dispatch(_catch_18, [Error])) {
      case 0: {
        const __ = _catch_18;
        return (() => { throw new RecursiveChildBindingError("recursive SDK spawn received an invalid parent or child thread id"); })();
        break;
      }
    }
  } })();
    const parent = (() => { try {
    return normalized_north_entity_id(parent_thread_value);
  } catch (_catch_19) {
    switch ($$bd$catch_dispatch(_catch_19, [Error])) {
      case 0: {
        const __ = _catch_19;
        return (() => { throw new RecursiveChildBindingError("recursive SDK spawn received an invalid parent or child thread id"); })();
        break;
      }
    }
  } })();
    if ((child === parent)) {
      (() => { throw new RecursiveChildBindingError("recursive SDK spawn cannot reuse the parent thread as the child thread"); })();
    }
    const parents = (() => { try {
    const facts = Reflect.apply(load_thread_facts, null, $$bh$array(child));
  return facts.filter((fact) => (fact.predicate === "part_of")).map((fact) => normalized_north_entity_id(fact.value));
  } catch (_catch_20) {
    switch ($$bd$catch_dispatch(_catch_20, [Error])) {
      case 0: {
        const __ = _catch_20;
        return (() => { throw new RecursiveChildBindingError("recursive SDK spawn could not verify the child thread parent link"); })();
        break;
      }
    }
  } })();
    if (((!(parents.length === 1)) || (!((() => { const _x = parents, _i = 0; return _x[_i] != null ? _x[_i] : null; })() === parent)))) {
      (() => { throw new RecursiveChildBindingError("recursive SDK spawn requires exactly one child part_of link to its immediate parent thread"); })();
    }
    return parent_run_id;
  }
}

function open_lifecycle_journal_bang(root, agent_id, retry_of_agent, composed, worktree_lease) {
  if ((root == null)) {
    return null;
  } else {
    const journal = foreign_new_bang(bridge_journal_module, "ExecutionJournal", $$bh$array(root, agent_id));
    journal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.spawnStart, $$bh$js_obj("prompt", composed.prompt, "cwd", process.cwd(), "thread", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(composed.thread), "role", composed.routingMetadata.role, "topology", composed.routingMetadata.topology, "worktree", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.path : null), "branch", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.branch : null), "retryOfAgent", ((_logical) => (_logical !== false && _logical != null ? _logical : null))(retry_of_agent)));
    return journal;
  }
}

async function spawn_bang(opts) {
  const injected = ((_logical) => (_logical !== false && _logical != null ? _logical : Object()))((test_runtime_module.takeSpawnTestRuntime)(opts));
  const admitted = allowlisted_spawn_options(opts);
  const caller_topology = process.env.AGENT_TOPOLOGY;
  if ((!bootstrap_authority_granted.value)) {
    (topology_authority_module.assertCoordinationAuthority)("spawn", caller_topology);
  }
  const composed = compose_spawn_options(admitted);
  const dispatch_authority = ((_logical) => (_logical !== false && _logical != null ? _logical : execution_admission_module.admitManagedDispatchAuthority))(injected.admitDispatchAuthority);
  Reflect.apply(dispatch_authority, null, $$bh$array(process.env, (delivery_liveness_module.deliveryDispatchClassForRouting)(composed.routingEconomics.pinEvidence, $$bh$js_obj("provider", composed.provider, "target", composed.target, "model", composed.model))));
  const load_shadow_config = ((_logical) => (_logical !== false && _logical != null ? _logical : shadow_reviewer_module.shadowReviewerConfig))(injected.loadShadowReviewerConfig);
  const shadow_config = Reflect.apply(load_shadow_config, null, $$bh$array());
  (query_lifecycle_module.managedRunTokenTarget)(admitted.tokenTarget);
  const parent_run_id = (bootstrap_authority_granted.value ? null : assert_recursive_child_binding_bang(composed, caller_topology, ((_logical) => (_logical !== false && _logical != null ? _logical : north_client_module.getThreadFacts))(injected.loadThreadFacts)));
  const requested_capabilities = (orchestration_staffing.orchestrationCapabilities)(composed.routingMetadata);
  const requests_mutation = hasAuthoringCapability(requested_capabilities);
  if ((requests_mutation && (composed.worktree === false))) {
    (() => { throw new Error("managed mutation cannot opt out of a registered worktree allocation: remove worktree:false to use the default managed worktree lane, or drop mutation capabilities for a read-only lane; canonical checkout mutation denied"); })();
  }
  const struggle_policy = (struggle_module.resolveStrugglePolicy)(composed.routingMetadata.topology);
  (struggle_module.assertExpectedStrugglePolicy)(struggle_policy);
  const judgment_state = $$bh$js_obj("value", (judgment_grade_module.adHocJudgmentGrade)());
  if (((_truthy) => _truthy !== false && _truthy != null)(composed.thread)) {
    (() => { try {
    return (judgment_state.value = (judgment_grade_module.judgmentGradeFromThreadFacts)(Reflect.apply(((_logical) => (_logical !== false && _logical != null ? _logical : north_client_module.getThreadFacts))(injected.loadThreadFacts), null, $$bh$array(composed.thread))));
  } catch (_catch_21) {
    switch ($$bd$catch_dispatch(_catch_21, [Error])) {
      case 0: {
        const __ = _catch_21;
        return (judgment_state.value = (judgment_grade_module.judgmentGradeFromThreadFacts)([]));
        break;
      }
    }
  } })();
  }
  const context = (resource_envelopes_module.envelopeContextFromEnv)();
  const agent_id = ((_logical) => (_logical !== false && _logical != null ? _logical : createSpawnAgentId()))(composed.agentId);
  (composed.agentId = agent_id);
  if ((composed.sessionId == null)) {
    (composed.sessionId = context.sessionId);
  }
  const metadata = composed.routingMetadata;
  const domains = metadata.domainRequirements.slice().sort();
  const task_signature = $$bh$js_obj("surface", "spawn", "promptSha256", (composition_receipt_module.sha256Bytes)(composed.prompt), "role", metadata.role, "taskGrade", metadata.taskGrade, "domainRequirements", domains, "topology", metadata.topology, "capabilityFloor", metadata.capabilityFloor, "serviceClass", metadata.serviceClass, "reasoning", metadata.reasoning, "posture", metadata.posture, "composition", metadata.composition, "shadowReviewer", (shadow_reviewer_module.shadowReviewerTaskSignature)(shadow_config));
  const learning_input = $$bh$js_obj("episodeId", agent_id, "taskSignature", task_signature, "taskSignatureCoverage", "exact", "routingMetadata", metadata, "routingAssessment", composed.routingEconomics.assessment, "pinEvidence", composed.routingEconomics.pinEvidence);
  if (((_truthy) => _truthy !== false && _truthy != null)(shadow_config)) {
    (learning_input.authoringArms = [shadow_reviewer_module.SHADOW_REVIEWER_ARM]);
  }
  const learning = (managed_learning_module.decideManagedLearning)(learning_input);
  (composed.routingMetadata = learning.routingMetadata);
  (composed.routingAssessment = learning.routingAssessment);
  (composed.routingEconomics = admitRoutingEconomics($$bh$js_obj("request", learning.routingMetadata, "routingAssessment", learning.routingAssessment, "pinEvidence", composed.pinEvidence, "provider", composed.provider, "target", composed.target, "model", composed.model, "surface", "managed North spawn learning admission")));
  const worktree_state = $$bh$js_obj("lease", null);
  if (((_truthy) => _truthy !== false && _truthy != null)(composed.worktree)) {
    const repo_root = process.cwd();
    const allocation_run_id = (telemetry_module.newRunId)(agent_id);
    (() => { try {
    const provisioned = (worktree_module.provisionWorktree)(agent_id, $$bh$js_obj("repoRoot", repo_root, "setupCmd", ((_logical) => (_logical !== false && _logical != null ? _logical : process.env.AGENT_SETUP_CMD))(composed.setupCmd), "runId", allocation_run_id, "thread", composed.thread, "provider", composed.provider, "target", composed.target, "writer", injected.worktreeAllocationWriter));
  const lease = Object.assign(Object(), provisioned, $$bh$js_obj("finalized", false));
  (worktree_state.lease = lease);
  return console.log($$bc$str("[spawn] @agent:", agent_id, " worktree ", provisioned.path, " on ", provisioned.branch));
  } catch (_catch_22) {
    switch ($$bd$catch_dispatch(_catch_22, [Error])) {
      case 0: {
        const error = _catch_22;
        const wrapped = new Error($$bc$str("[spawn] @agent:", agent_id, " explicit worktree provisioning failed; ", "spawn aborted before provider execution: ", ((_logical) => (_logical !== false && _logical != null ? _logical : $$bc$str(error)))(error.message)));
        (wrapped.cause = error);
        return (() => { throw wrapped; })();
        break;
      }
    }
  } })();
  }
  const worktree_lease = worktree_state.lease;
  const termination = foreign_new_bang(query_lifecycle_module, "ManagedQueryTermination", $$bh$array(injected.registerTermination, Object.assign(Object(), ((_logical) => (_logical !== false && _logical != null ? _logical : Object()))(injected.sessionHardCapRuntime), $$bh$js_obj("agentId", agent_id, "threadId", composed.thread, "goal", (identity_module.goalFromPrompt)(composed.prompt), "repo", process.cwd(), "worktree", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.path : null), "branch", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.branch : null), "tokenTarget", composed.tokenTarget))));
  const lifecycle_root = ((_logical) => (_logical !== false && _logical != null ? _logical : (((_truthy) => _truthy !== false && _truthy != null)(injected.queryFn) ? null : (bridge_protocol_module["bridge-journal-root"])())))(injected.journalRoot);
  const outer = $$bh$js_obj("journal", open_lifecycle_journal_bang(lifecycle_root, agent_id, null, composed, worktree_lease), "admission", null, "result", null, "failed", false, "primaryError", null);
  const cleanup_errors = $$bh$array();
  await (async () => { try {
    termination.throwIfTerminated();
  const admit_envelope = ((_logical) => (_logical !== false && _logical != null ? _logical : resource_envelopes_module.admitResourceEnvelope))(injected.admitResourceEnvelope);
  (outer.admission = await Reflect.apply(admit_envelope, null, $$bh$array($$bh$js_obj("agentId", agent_id, "capabilityFloor", composed.routingMetadata.capabilityFloor, "project", ((_logical) => (_logical !== false && _logical != null ? _logical : context.project))(composed.project), "sessionId", ((_logical) => (_logical !== false && _logical != null ? _logical : context.sessionId))(composed.sessionId)))));
  termination.throwIfTerminated();
  ((_logical) => (_logical !== false && _logical != null ? _logical : []))((((_truthy) => _truthy !== false && _truthy != null)(outer.admission) ? outer.admission.advisories : null)).forEach((advisory) => {
  console.warn($$bc$str("[envelope] advisory: ", advisory));
});
  const attempt_state = $$bh$js_obj("value", await run_spawn_bang(Object.assign(Object(), composed), judgment_state.value, struggle_policy, outer.admission, injected, termination, worktree_lease, null, null, parent_run_id, learning.assignment, shadow_config, outer.journal), "retries", 0, "deadAgentId", agent_id);
  await (async () => {  while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((attempt_state.retries < PROVIDER_PROCESS_DEATH_MAX_RETRIES) && termination.continuationAllowed()))) { const attempt = attempt_state.value; const process_death_retry = eligibleForProviderProcessDeathRetry(attempt.outcome, composed.routingMetadata.topology, requested_capabilities); const lane_start_retry = eligibleForLaneStartProviderRetry(attempt.outcome, attempt.providerErrorDetail, attempt.numTurns, attempt.siblingTarget); if ((process_death_retry || lane_start_retry)) { (attempt_state.retries = (attempt_state.retries + 1)); const dead_run_id = attempt.runId; const retry_agent_id = createSpawnAgentId(); const retry_target = (lane_start_retry ? attempt.siblingTarget : null); console.error($$bc$str("[spawn] @agent:", attempt_state.deadAgentId, " ", (lane_start_retry ? "start-of-stream provider failure" : "provider-process death"), " (run @", dead_run_id, ") is retry-safe — retrying once as a fresh run", (((_truthy) => _truthy !== false && _truthy != null)(retry_target) ? $$bc$str(" on sibling target=", retry_target) : ""), " on a fresh @agent:", retry_agent_id, " (attempt ", attempt_state.retries, ")")); termination.throwIfTerminated(); (((_truthy) => _truthy !== false && _truthy != null)(outer.journal) ? (() => { return outer.journal.close(); })() : null); (outer.journal = open_lifecycle_journal_bang(lifecycle_root, retry_agent_id, attempt_state.deadAgentId, composed, worktree_lease)); (attempt_state.value = await run_spawn_bang(Object.assign(Object(), composed, $$bh$js_obj("agentId", retry_agent_id)), judgment_state.value, struggle_policy, outer.admission, injected, termination, worktree_lease, $$bh$js_obj("retryOfRun", dead_run_id, "retryAttempt", attempt_state.retries, "retryOfAgent", attempt_state.deadAgentId), retry_target, parent_run_id, learning.assignment, shadow_config, outer.journal)); (attempt_state.deadAgentId = retry_agent_id);  continue; } else { return null; } } else { return null; }
  } })();
  return (outer.result = attempt_state.value.result);
  } catch (_catch_23) {
    switch ($$bd$catch_dispatch(_catch_23, [Error])) {
      case 0: {
        const error = _catch_23;
        (outer.failed = true);
        return (outer.primaryError = error);
        break;
      }
    }
  } })();
  await (async () => { try {
    return await termination.close();
  } catch (_catch_24) {
    switch ($$bd$catch_dispatch(_catch_24, [Error])) {
      case 0: {
        const error = _catch_24;
        if ((!((_truthy) => _truthy !== false && _truthy != null)(outer.failed))) {
          (outer.failed = true);
          return (outer.primaryError = error);
        } else {
          return (outer.primaryError = new AggregateError($$bh$array(outer.primaryError, error), "spawn execution and managed resource cleanup failed"));
        }
        break;
      }
    }
  } })();
  termination.publicationSettled();
  await (async () => { try {
    return await Reflect.apply(((_logical) => (_logical !== false && _logical != null ? _logical : resource_envelopes_module.completeResourceEnvelope))(injected.completeResourceEnvelope), null, $$bh$array(outer.admission));
  } catch (_catch_25) {
    switch ($$bd$catch_dispatch(_catch_25, [Error])) {
      case 0: {
        const error = _catch_25;
        return cleanup_errors.push(error);
        break;
      }
    }
  } finally {
    termination.cleanupSettled();
    termination.release();
  } })();
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(worktree_lease.finalized)) : _logical))(worktree_lease))) {
    (() => { try {
    return (worktree_module.rollbackProvisionedWorktree)(agent_id, worktree_lease);
  } catch (_catch_26) {
    switch ($$bd$catch_dispatch(_catch_26, [Error])) {
      case 0: {
        const error = _catch_26;
        return cleanup_errors.push(error);
        break;
      }
    }
  } finally {
    (worktree_lease.finalized = true);
  } })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? outer.journal : _logical))(outer.failed))) {
    (() => { try {
    const records = outer.journal.scan().records;
  if ((!((_truthy) => _truthy !== false && _truthy != null)(records.some((record) => (record.kind === bridge_journal_module.LANE_LIFECYCLE_KINDS.terminal))))) {
    outer.journal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.terminal, $$bh$js_obj("outcome", "rejected", "processOutcome", "blocked_preflight", "deliveryOutcome", "blocked", "deliveryReason", "spawn_rejected_before_terminal_publication", "detail", terminal_cause(outer.primaryError)));
    return outer.journal.append(bridge_journal_module.LANE_LIFECYCLE_KINDS.harvest, $$bh$js_obj("status", "unavailable", "branch", (((_truthy) => _truthy !== false && _truthy != null)(worktree_lease) ? worktree_lease.branch : null), "sha", null, "reason", "spawn rejected before terminal harvest"));
  }
  } catch (_catch_27) {
    switch ($$bd$catch_dispatch(_catch_27, [Error])) {
      case 0: {
        const __ = _catch_27;
        return null;
        break;
      }
    }
  } })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(outer.journal)) {
    outer.journal.close();
  }
  const errors = (((_truthy) => _truthy !== false && _truthy != null)(outer.failed) ? $$bh$array(outer.primaryError).concat(cleanup_errors) : cleanup_errors);
  return (((errors.length === 1)) ? (() => { throw (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(errors, 0); })() : ((errors.length > 1)) ? (() => { throw new AggregateError(errors, "spawn execution and outer cleanup failed"); })() : outer.result);
}

const spawn = spawn_bang;

async function spawn_parallel_bang(tasks) {
  (topology_authority_module.assertCoordinationAuthority)("spawnParallel");
  return await Promise.all(tasks.map((task) => spawn_bang(task)));
}

const spawnParallel = spawn_parallel_bang;

function managed_child_spawn_options_bang(prompt) {
  const env = process.env;
  const raw_delegate_thread = env.NORTH_DELEGATE_THREAD_ID;
  delete env.NORTH_DELEGATE_THREAD_ID;
  const delegate_thread = ((raw_delegate_thread == null) ? null : (() => { try {
    return normalized_north_entity_id(((_logical) => (_logical !== false && _logical != null ? _logical : ""))(raw_delegate_thread));
  } catch (_catch_28) {
    switch ($$bd$catch_dispatch(_catch_28, [Error])) {
      case 0: {
        const __ = _catch_28;
        return (() => { throw new Error("managed delegate bootstrap received an invalid exact North thread id"); })();
        break;
      }
    }
  } })());
  const token_target = (query_lifecycle_module.managedRunTokenTarget)(((env.NORTH_RUN_TOKEN_TARGET === undefined) ? undefined : Number(env.NORTH_RUN_TOKEN_TARGET)));
  return $$bh$js_obj("prompt", prompt, "agentId", env.AGENT_ID, "model", env.AGENT_MODEL, "provider", env.AGENT_PROVIDER, "target", env.AGENT_TARGET, "thread", delegate_thread, "coordinator", env.AGENT_COORDINATOR, "routingMetadata", routingRequestFromEnv("managed North spawn bootstrap"), "projectProfile", projectProfileFromEnv(), "routingAssessment", (((_truthy) => _truthy !== false && _truthy != null)(env.AGENT_ROUTING_ASSESSMENT) ? JSON.parse(env.AGENT_ROUTING_ASSESSMENT) : undefined), "pinEvidence", (((_truthy) => _truthy !== false && _truthy != null)(env.NORTH_ROUTING_PIN_EVIDENCE) ? JSON.parse(env.NORTH_ROUTING_PIN_EVIDENCE) : undefined), "tokenTarget", token_target);
}

const managedChildSpawnOptions = managed_child_spawn_options_bang;

if (((_truthy) => _truthy !== false && _truthy != null)(import.meta.main)) {
  install_spawn_terminal_handlers_bang();
  (() => { const _a = bootstrap_authority_granted, _v = true; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
  const prompt = process.argv.slice(2).join(" ");
  if ((prompt === "")) {
    console.error("usage: bun run src/spawn.js <prompt>");
    process.exit(1);
  } else {
    apply_codex_turn_deadline_from_reasoning_bang();
    spawn_bang(managed_child_spawn_options_bang(prompt)).then((result) => console.log(result)).catch((error) => { append_spawn_terminal_line_bang("rejected", error);
console.error(error);
return process.exit(1); });
  }
}

export { RecursiveChildBindingError as "RecursiveChildBindingError" };
export { appendSpawnTerminalLine as "appendSpawnTerminalLine" };
export { applyCodexTurnDeadlineFromReasoning as "applyCodexTurnDeadlineFromReasoning" };
export { createSpawnAgentId as "createSpawnAgentId" };
export { eligibleForLaneStartProviderRetry as "eligibleForLaneStartProviderRetry" };
export { eligibleForProviderProcessDeathRetry as "eligibleForProviderProcessDeathRetry" };
export { installSpawnTerminalHandlers as "installSpawnTerminalHandlers" };
export { managedChildSpawnOptions as "managedChildSpawnOptions" };
export { spawn as "spawn" };
export { spawnParallel as "spawnParallel" };
