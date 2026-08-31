import { str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aset as $$bh$aset, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const path_module = require("node:path");

const resolve = path_module.resolve;

const crypto_module = require("node:crypto");

const createHash = crypto_module.createHash;

const randomUUID = crypto_module.randomUUID;

const dateParse = Date.parse;

const bunSpawn = Bun.spawn;

const beagle_store_module = require("./beagle-store");

const beagleStoreBabashkaArguments = beagle_store_module.beagleStoreBabashkaArguments;

const beagleStoreEnvironment = beagle_store_module.beagleStoreEnvironment;

const settleBeagleStoreCoordinatorChild = beagle_store_module.settleBeagleStoreCoordinatorChild;

const run_ledger_module = require("./run-ledger");

const AGENT_RUN_LEDGER_CONTRACT = run_ledger_module.AGENT_RUN_LEDGER_CONTRACT;

const AGENT_RUN_LEDGER_VERSION = run_ledger_module.AGENT_RUN_LEDGER_VERSION;

const isWireRunLedgerSummary = run_ledger_module.isWireRunLedgerSummary;

const wireRunLedgerIdentity = run_ledger_module.wireRunLedgerIdentity;

const wire_provenance_module = require("./run-provenance");

const wireRunProvenanceFacts = wire_provenance_module.wireRunProvenanceFacts;

const usage_module = require("./usage");

const tokenTotalLiteral = usage_module.tokenTotalLiteral;

const execution_observation_module = require("./execution-observation");

const executionObservationJson = execution_observation_module.executionObservationJson;

const normalizeExecutionObservation = execution_observation_module.normalizeExecutionObservation;

const unknownExecutionObservation = execution_observation_module.unknownExecutionObservation;

const provider_join_module = require("./providers/provider-join");

const foldProviderJoinEvidence = provider_join_module.foldProviderJoinEvidence;

const wire_events_module = require("./wire/events");

const WIRE_PROVIDER_JOIN_VERSION = wire_events_module.WIRE_PROVIDER_JOIN_VERSION;

const WIRE_VERSION = wire_events_module.WIRE_VERSION;

const wire_ids_module = require("./wire/ids");

const wireRunId = wire_ids_module.wireRunId;

const REPO = resolve(import.meta.dir, "../..");

const INTERNAL_WRITER = resolve(REPO, "cli/run-fact-internal.clj");

const TERMINAL_COORDINATOR_READ_TIMEOUT_MS = 70000;

const RUN_WRITE_TIMEOUT_MS = (() => { const raw = process.env.NORTH_RUN_WRITE_TIMEOUT_MS; const value = Number(raw); return ((Number.isFinite(value) && (value > 0)) ? value : 120000); })();

const RECORDED = "recorded";

const UNAVAILABLE = "unavailable";

function fact(predicate, value) {
  return [predicate, value];
}

function push_fact_bang(facts, predicate, value) {
  facts.push(fact(predicate, value));
  return null;
}

function apply_terminal_coordinator_read_timeout_bang(env) {
  if ((env.NORTH_COORD_READ_TIMEOUT_MS === undefined)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(env, "NORTH_COORD_READ_TIMEOUT_MS", String(TERMINAL_COORDINATOR_READ_TIMEOUT_MS));
  }
  return null;
}

function duration_ms(snapshot) {
  const duration = (dateParse(snapshot.updatedAt) - dateParse(snapshot.startedAt));
  return ((Number.isSafeInteger(duration) && (duration >= 0)) ? duration : (() => { throw new TypeError("wire run snapshot has an invalid duration"); })());
}

function outcome(lifecycle, termination_code) {
  return (((lifecycle === "completed")) ? "ran" : ((lifecycle === "cancelled")) ? ((_logical) => (_logical !== false && _logical != null ? _logical : "cancelled"))(termination_code) : ((lifecycle === "blocked")) ? ((_logical) => (_logical !== false && _logical != null ? _logical : "blocked"))(termination_code) : ((lifecycle === "failed")) ? ((_logical) => (_logical !== false && _logical != null ? _logical : "failed"))(termination_code) : (() => { throw new TypeError("wire run telemetry requires a terminal snapshot"); })());
}

function count_tools(snapshot, status) {
  const tools = Object.values(snapshot.toolCalls);
  return ((status === undefined) ? tools.length : tools.filter((tool) => (tool.status === status)).length);
}

function completed_model_calls(snapshot) {
  return Object.values(snapshot.modelCalls).filter((model_call) => (!(model_call.status === "running")));
}

function safe_evidence_sum(values) {
  return (() => { let index = 0; let total = 0; while (true) {
    if ((index >= values.length)) { return total; } else { const value = values[index]; const next = (total + value); if (Number.isSafeInteger(next)) { const _recur_0 = (index + 1); const _recur_1 = next; index = _recur_0; total = _recur_1; continue; } else { return null; } }
  } })();
}

function wire_completion_evidence_facts_bang(snapshot) {
  const model_calls = Object.values(snapshot.modelCalls);
  const completed = completed_model_calls(snapshot);
  const provider_completed = [];
  const join_evidence = [];
  completed.forEach((model_call) => {
  if ((model_call.origin === "provider")) {
    provider_completed.push(model_call);
  }
  const evidence = model_call.evidence;
  const join = (((_truthy) => _truthy !== false && _truthy != null)(evidence) ? evidence.providerJoin : null);
  if (((_truthy) => _truthy !== false && _truthy != null)(join)) {
    join_evidence.push(join);
  }
});
  const folded_join = foldProviderJoinEvidence(join_evidence);
  const exact_folded = ((!(folded_join == null)) && ((join_evidence.length < completed.length) && (folded_join.coverage === "exact")));
  const join = (exact_folded ? Object.freeze($$bh$js_obj("version", folded_join.version, "sessionKey", folded_join.sessionKey, "turnKeys", folded_join.turnKeys, "sessionPersistence", folded_join.sessionPersistence, "coverage", "partial")) : folded_join);
  const providers = new Set();
  const facts = [fact("provider_session_persistence", (((_truthy) => _truthy !== false && _truthy != null)(join) ? join.sessionPersistence : "unknown"))];
  provider_completed.forEach((model_call) => {
  providers.add(model_call.model.provider);
});
  if ((providers.size > 1)) {
    (() => { throw new TypeError("provider terminal evidence conflicts within one Wire run"); })();
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(join)) {
    push_fact_bang(facts, "provider_join_key_version", WIRE_PROVIDER_JOIN_VERSION);
    push_fact_bang(facts, "provider_join_coverage", join.coverage);
    if ((!(join.sessionKey === undefined))) {
      push_fact_bang(facts, "provider_session_key", join.sessionKey);
    }
    join.turnKeys.forEach((key) => {
  push_fact_bang(facts, "provider_turn_key", key);
});
  }
  const all_provider_terminal = ((model_calls.length > 0) && model_calls.every((model_call) => ((!(model_call.status === "running")) && (model_call.origin === "provider"))));
  if (((_truthy) => _truthy !== false && _truthy != null)((all_provider_terminal && model_calls.every((model_call) => { const evidence = model_call.evidence;
return (!(evidence.providerDurationMs === undefined)); })))) {
    const durations = [];
    model_calls.forEach((model_call) => {
  durations.push(model_call.evidence.providerDurationMs);
});
    const provider_duration_ms = safe_evidence_sum(durations);
    if (((_truthy) => _truthy !== false && _truthy != null)(provider_duration_ms)) {
      push_fact_bang(facts, "provider_duration_ms", String(provider_duration_ms));
    }
  }
  const turn_evidence = [];
  provider_completed.forEach((model_call) => {
  const evidence = model_call.evidence;
  const turns = (((_truthy) => _truthy !== false && _truthy != null)(evidence) ? evidence.turns : null);
  if (((_truthy) => _truthy !== false && _truthy != null)(turns)) {
    turn_evidence.push(turns);
  }
});
  const turn_units = new Set();
  turn_evidence.forEach((turns) => {
  turn_units.add(turns.unit);
});
  if ((turn_units.size > 1)) {
    (() => { throw new TypeError("provider turn evidence uses incompatible units within one Wire run"); })();
  }
  const witnessed_turns = ((_logical) => (_logical !== false && _logical != null ? (turn_evidence.length === model_calls.length) : _logical))(all_provider_terminal);
  const turn_count = (witnessed_turns ? (() => { const counts = []; (() => { turn_evidence.forEach((turns) => {
  counts.push(turns.count);
}); })();
return safe_evidence_sum(counts); })() : null);
  const exact_turns = (witnessed_turns && (!(turn_count == null)));
  if ((!exact_turns)) {
    const pre_provider = ((snapshot.lifecycle === "blocked") && (model_calls.length === 0));
    push_fact_bang(facts, "turn_provenance", (pre_provider ? "pre-provider" : "unknown"));
    if (pre_provider) {
      push_fact_bang(facts, "num_turns", "0");
    }
  } else {
    push_fact_bang(facts, "turn_provenance", "provider-terminal");
    const first_turn = (() => { const _x = turn_evidence, _i = 0; return _x[_i] != null ? _x[_i] : null; })();
    if ((first_turn.unit === "assistant-turn")) {
      push_fact_bang(facts, "num_turns", String(turn_count));
    } else if ((first_turn.unit === "provider-turn")) {
      push_fact_bang(facts, "provider_turn_units", String(turn_count));
      if (((_truthy) => _truthy !== false && _truthy != null)(turn_evidence.every((turns) => ((turns.unit === "provider-turn") && (!(turns.toolItems === undefined)))))) {
        const tool_items = [];
        turn_evidence.forEach((turns) => {
  tool_items.push(turns.toolItems);
});
        const total = safe_evidence_sum(tool_items);
        if (((_truthy) => _truthy !== false && _truthy != null)(total)) {
          push_fact_bang(facts, "provider_tool_items", String(total));
        }
      }
      push_fact_bang(facts, "provider_turn_metric_comparable", "false");
    } else {
      null;
    }
  }
  return facts;
}

function wire_watchdog_facts(snapshot) {
  const abort = snapshot.abort;
  const watchdog = (((_truthy) => _truthy !== false && _truthy != null)(abort) ? abort.watchdog : null);
  if ((!((_truthy) => _truthy !== false && _truthy != null)(watchdog))) {
    return [];
  } else {
    const activity = (value) => ((value === undefined) ? "none" : JSON.stringify(value));
    return [fact("watchdog_reason", abort.reason), fact("watchdog_silence_ms", String(watchdog.silenceMs)), fact("watchdog_last_outer_activity", activity(watchdog.lastOuter)), fact("watchdog_last_provider_activity", activity(watchdog.lastProvider))];
  }
}

function unavailable_execution_observation_source(snapshot, provenance) {
  if (((provenance.executionSource === "provider-native") && (provenance.provider === "openai"))) {
    return "codex_rollout_initial_mode_or_join_unavailable";
  } else {
    const model = snapshot.model;
    const provider = (((_truthy) => _truthy !== false && _truthy != null)(model) ? model.provider : provenance.provider);
    return (((provider === "openai")) ? "codex_app_server_mode_unavailable" : ((provider === "anthropic")) ? "anthropic_execution_mode_unsupported" : "execution_mode_telemetry_unavailable");
  }
}

function sha256(value) {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

function wire_run_subject(run_id) {
  return (((_truthy) => _truthy !== false && _truthy != null)(new RegExp("^run:[A-Za-z0-9][A-Za-z0-9._:-]*$", "u").test(run_id)) ? $$bc$str("@", run_id) : $$bc$str("@run:wire-summary-", sha256($$bc$str("north-wire-run-summary-subject:v2\x00", run_id))));
}

function assert_ledger_matches_snapshot_bang(snapshot, ledger) {
  const summary = ledger.summary;
  const last_sequence = snapshot.lastSequence;
  if (((!isWireRunLedgerSummary(summary)) || ((!(summary.runId === snapshot.runId)) || ((!(summary.lastSequence === snapshot.lastSequence)) || ((!(summary.terminalEventId === snapshot.lastEventId)) || (!(summary.eventCount === (last_sequence + 1)))))))) {
    (() => { throw new TypeError("recorded wire ledger summary does not match the run snapshot"); })();
  }
  if (((!((_truthy) => _truthy !== false && _truthy != null)(["completed", "failed", "cancelled", "blocked"].includes(snapshot.lifecycle))) || (snapshot.termination === undefined))) {
    (() => { throw new TypeError("wire run telemetry requires a terminated snapshot"); })();
  }
  return null;
}

function wire_run_telemetry_facts_bang(identity, snapshot, ledger, provenance) {
  const context = wireRunLedgerIdentity(identity);
  assert_ledger_matches_snapshot_bang(snapshot, ledger);
  const snapshot_model = snapshot.model;
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(provenance.provider === undefined)) && ((_logical) => (_logical !== false && _logical != null ? (!(provenance.provider === snapshot_model.provider)) : _logical))(snapshot_model)))) {
    (() => { throw new TypeError("run provenance provider differs from the reduced wire snapshot"); })();
  }
  const actual_duration_ms = duration_ms(snapshot);
  const usage = snapshot.usage;
  const exact_token_total = tokenTotalLiteral(snapshot);
  const summary = ledger.summary;
  const facts = [fact("kind", "run"), fact("wire_run_id", snapshot.runId), fact("thread", context.thread), fact("thread_provenance", ((context.thread === "(ad-hoc)") ? "ad-hoc" : "exact")), fact("agent", context.agent), fact("wire_ledger_version", AGENT_RUN_LEDGER_VERSION), fact("wire_version", WIRE_VERSION), fact("wire_ledger_status", "complete"), fact("wire_event_count", String(summary.eventCount)), fact("wire_event_first_sequence", String(summary.firstSequence)), fact("wire_event_last_sequence", String(summary.lastSequence)), fact("wire_terminal_event_id", summary.terminalEventId), fact("wire_ledger_sha256", summary.digest), fact("wire_run_lifecycle", snapshot.lifecycle), fact("wire_termination_code", snapshot.termination.code), fact("outcome", outcome(snapshot.lifecycle, snapshot.termination.code)), fact("at", snapshot.updatedAt), fact("started_at", snapshot.startedAt), fact("duration_ms", String(actual_duration_ms)), fact("lifetime_input_tokens", String(usage.lifetime.inputTokens)), fact("lifetime_output_tokens", String(usage.lifetime.outputTokens)), fact("lifetime_cache_read_tokens", String(usage.lifetime.cacheReadTokens)), fact("lifetime_cache_write_tokens", String(usage.lifetime.cacheWriteTokens)), fact("lifetime_reasoning_tokens", String(usage.lifetime.reasoningTokens)), fact("model_call_count", String(usage.lifetime.modelCalls)), fact("usage_terminal_count", String(snapshot.usageCoverage.providerTerminalCount)), fact("usage_scope", snapshot.usageCoverage.scope), fact("usage_total_status", snapshot.usageCoverage.totalStatus), fact("context_tokens", String(usage.context.tokens)), fact("compaction_count", String(snapshot.compactions)), fact("tool_admitted_count", String(count_tools(snapshot, null))), fact("tool_succeeded_count", String(count_tools(snapshot, "succeeded"))), fact("tool_failed_count", String(count_tools(snapshot, "failed"))), fact("tool_cancelled_count", String(count_tools(snapshot, "cancelled"))), fact("tool_synthetic_failure_count", String(count_tools(snapshot, "synthetic_failure"))), fact("execution_observation", executionObservationJson(((provenance.executionObservation === undefined) ? unknownExecutionObservation(unavailable_execution_observation_source(snapshot, provenance)) : normalizeExecutionObservation(provenance.executionObservation))))];
  wire_completion_evidence_facts_bang(snapshot).forEach((fact) => {
  facts.push(fact);
});
  wire_watchdog_facts(snapshot).forEach((fact) => {
  facts.push(fact);
});
  if (((_truthy) => _truthy !== false && _truthy != null)(exact_token_total)) {
    push_fact_bang(facts, "tokens", exact_token_total);
  }
  const usage_context = usage.context;
  if ((!(usage_context.window === undefined))) {
    push_fact_bang(facts, "context_window_tokens", String(usage_context.window));
  }
  if ((!(snapshot.parentRunId === undefined))) {
    push_fact_bang(facts, "parent_run", wire_run_subject(snapshot.parentRunId));
  }
  if ((!(context.parentThread === undefined))) {
    push_fact_bang(facts, "parent_thread", context.parentThread);
  }
  if ((!(context.coordinator === undefined))) {
    push_fact_bang(facts, "run_coordinator", context.coordinator);
  }
  if ((!(snapshot.owner === undefined))) {
    push_fact_bang(facts, "run_owner", snapshot.owner);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(snapshot_model.tier === undefined)) : _logical))(snapshot_model))) {
    push_fact_bang(facts, "model_tier", snapshot_model.tier);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(snapshot_model.capabilityClass === undefined)) : _logical))(snapshot_model))) {
    push_fact_bang(facts, "capability_class", snapshot_model.capabilityClass);
  }
  if ((!(snapshot.effort === undefined))) {
    push_fact_bang(facts, "effort", snapshot.effort);
  }
  wireRunProvenanceFacts(provenance, actual_duration_ms).forEach((fact) => {
  const predicate = fact[0];
  const value = fact[1];
  if ((!((_truthy) => _truthy !== false && _truthy != null)(((predicate === "provider") && snapshot_model)))) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((predicate === "prompt_capability_class") && ((_logical) => (_logical !== false && _logical != null ? (!(value === snapshot_model.capabilityClass)) : _logical))(snapshot_model)))) {
      (() => { throw new TypeError("prompt capability class differs from the reduced wire snapshot"); })();
    }
    facts.push(fact);
  }
});
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(snapshot_model.provider === undefined)) : _logical))(snapshot_model))) {
    push_fact_bang(facts, "provider", snapshot_model.provider);
  }
  return Object.freeze($$bh$js_obj("subject", wire_run_subject(snapshot.runId), "facts", Object.freeze(facts)));
}

async function run_telemetry_writer_bang(projection, timeout_ms, environment) {
  const env = beagleStoreEnvironment(environment);
  apply_terminal_coordinator_read_timeout_bang(env);
  const payload = JSON.stringify(projection.facts);
  if ((new TextEncoder().encode(payload).byteLength > AGENT_RUN_LEDGER_CONTRACT.bounds.maxTelemetryProjectionBytes)) {
    (() => { throw new RangeError("wire run telemetry projection exceeds its byte bound"); })();
  }
  const command = ["bb"].concat(beagleStoreBabashkaArguments([INTERNAL_WRITER, ((_logical) => (_logical !== false && _logical != null ? _logical : "7977"))(environment.NORTH_PORT), projection.subject], environment));
  const spawn_options = $$bh$js_obj("env", env, "stdin", "pipe", "stdout", "ignore", "stderr", "ignore");
  const child = bunSpawn(command, spawn_options);
  child.stdin.write(payload);
  child.stdin.end();
  const result = await settleBeagleStoreCoordinatorChild(child, timeout_ms);
  return Promise.resolve((((!((_truthy) => _truthy !== false && _truthy != null)(result.timedOut)) && (result.exitCode === 0)) ? RECORDED : UNAVAILABLE));
}

function apply_terminal_coordinator_read_timeout_public_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return apply_terminal_coordinator_read_timeout_bang(env);
  }
  if (arguments.length === 0) {
    return apply_terminal_coordinator_read_timeout_bang(process.env);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const applyTerminalCoordinatorReadTimeout = apply_terminal_coordinator_read_timeout_public_bang;

function wire_run_telemetry_facts_public_bang(identity, snapshot, ledger, provenance) {
  return wire_run_telemetry_facts_bang(identity, snapshot, ledger, provenance);
}

const wireRunTelemetryFacts = wire_run_telemetry_facts_public_bang;

async function record_wire_run_telemetry_projection_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const projection = $beagle$args[0];
    return record_wire_run_telemetry_projection_bang(projection, RUN_WRITE_TIMEOUT_MS, process.env);
  }
  if (arguments.length === 2) {
    const projection = $beagle$args[0];
    const timeout_ms = $beagle$args[1];
    return record_wire_run_telemetry_projection_bang(projection, timeout_ms, process.env);
  }
  if (arguments.length === 3) {
    const projection = $beagle$args[0];
    const timeout_ms = $beagle$args[1];
    const env = $beagle$args[2];
    return (() => { try {
    return run_telemetry_writer_bang(projection, timeout_ms, env);
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return Promise.resolve(UNAVAILABLE);
        break;
      }
    }
  } })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const recordWireRunTelemetryProjection = record_wire_run_telemetry_projection_bang;

async function default_wire_run_telemetry_writer_bang(projection, timeout_ms) {
  return record_wire_run_telemetry_projection_bang(projection, timeout_ms, process.env);
}

async function record_wire_run_telemetry_bang(...$beagle$args) {
  if (arguments.length === 4) {
    const identity = $beagle$args[0];
    const snapshot = $beagle$args[1];
    const ledger = $beagle$args[2];
    const provenance = $beagle$args[3];
    const projection = wire_run_telemetry_facts_bang(identity, snapshot, ledger, provenance);
    return default_wire_run_telemetry_writer_bang(projection, RUN_WRITE_TIMEOUT_MS);
  }
  if (arguments.length === 5) {
    const identity = $beagle$args[0];
    const snapshot = $beagle$args[1];
    const ledger = $beagle$args[2];
    const provenance = $beagle$args[3];
    const timeout_ms = $beagle$args[4];
    const projection = wire_run_telemetry_facts_bang(identity, snapshot, ledger, provenance);
    return default_wire_run_telemetry_writer_bang(projection, timeout_ms);
  }
  if (arguments.length === 6) {
    const identity = $beagle$args[0];
    const snapshot = $beagle$args[1];
    const ledger = $beagle$args[2];
    const provenance = $beagle$args[3];
    const timeout_ms = $beagle$args[4];
    const writer = $beagle$args[5];
    const projection = wire_run_telemetry_facts_bang(identity, snapshot, ledger, provenance);
    return writer(projection, timeout_ms);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const recordWireRunTelemetry = record_wire_run_telemetry_bang;

function newRunId(agent) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(new RegExp("^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$", "u").test(agent)))) {
    (() => { throw new TypeError("invalid run agent identity"); })();
  }
  return wireRunId($$bc$str("run:", agent, "-", randomUUID()));
}

export { applyTerminalCoordinatorReadTimeout as "applyTerminalCoordinatorReadTimeout" };
export { newRunId as "newRunId" };
export { recordWireRunTelemetry as "recordWireRunTelemetry" };
export { recordWireRunTelemetryProjection as "recordWireRunTelemetryProjection" };
export { wireRunTelemetryFacts as "wireRunTelemetryFacts" };
