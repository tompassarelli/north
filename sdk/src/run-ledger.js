import { keyword as $$bc$keyword, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aset as $$bh$aset, host_object as $$bh$host_object, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const node_module = process.getBuiltinModule("node:module");

const create_require = node_module.createRequire;

const require_module = create_require(import.meta.url);

const path_module = process.getBuiltinModule("node:path");

const fs_module = process.getBuiltinModule("node:fs");

const crypto_module = process.getBuiltinModule("node:crypto");

const resolve = path_module.resolve;

const read_file_sync = fs_module.readFileSync;

const create_hash = crypto_module.createHash;

const bun_spawn = Bun.spawn;

const beagle_store_module = require_module("./beagle-store");

const beagle_store_babashka_arguments = beagle_store_module.beagleStoreBabashkaArguments;

const beagle_store_environment = beagle_store_module.beagleStoreEnvironment;

const settle_beagle_store_coordinator_child = beagle_store_module.settleBeagleStoreCoordinatorChild;

const wire_events_module = require_module("./wire/events");

const WIRE_MAX_EVENTS_PER_RUN = wire_events_module.WIRE_MAX_EVENTS_PER_RUN;

const WIRE_VERSION = wire_events_module.WIRE_VERSION;

const wire_jsonl_module = require_module("./wire/jsonl");

const encode_wire_jsonl_line = wire_jsonl_module.encodeWireJsonlLine;

const wire_reducer_module = require_module("./wire/reducer");

const reduce_wire_events = wire_reducer_module.reduceWireEvents;

const RECORDED = "recorded";

const UNAVAILABLE = "unavailable";

const REPO = resolve(import.meta.dir, "../..");

const CONTRACT_PATH = resolve(REPO, "contracts/agent-run-ledger-v2.json");

const INTERNAL_WRITER = resolve(REPO, "cli/run-event-internal.clj");

const IDENTIFIER = new RegExp("^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,255}$");

const WIRE_ID = new RegExp("^[A-Za-z0-9@][A-Za-z0-9@_.:/-]{0,255}$");

const ENTITY = new RegExp("^@?[A-Za-z0-9][A-Za-z0-9_.:-]{0,255}$");

const SHA256 = new RegExp("^[a-f0-9]{64}$");

const AGENT__RUN__LEDGER__CONTRACT = Object.freeze(JSON.parse(read_file_sync(CONTRACT_PATH, "utf8")));

const AGENT__RUN__LEDGER__VERSION = AGENT__RUN__LEDGER__CONTRACT.version;

function wire_ledger_error_new_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const code = $beagle$args[0];
    const message = $beagle$args[1];
    return wire_ledger_error_new_bang(code, message, null);
  }
  if (arguments.length === 3) {
    const code = $beagle$args[0];
    const message = $beagle$args[1];
    const cause = $beagle$args[2];
    const error = new Error(message);
    Object.setPrototypeOf(error, wire_ledger_error_new_bang.prototype);
    (error.name = "WireLedgerError");
    (error.code = code);
    if (((_truthy) => _truthy !== false && _truthy != null)(cause)) {
      (error.cause = cause);
    }
    return error;
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function wire_ledger_error_constructor_bang(code, message, options) {
  return wire_ledger_error_new_bang(code, message, (((_truthy) => _truthy !== false && _truthy != null)(options) ? options.cause : null));
}

Object.setPrototypeOf(wire_ledger_error_new_bang.prototype, wire_ledger_error_constructor_bang.prototype);

Object.setPrototypeOf(wire_ledger_error_constructor_bang.prototype, Error.prototype);

const WireLedgerError = wire_ledger_error_constructor_bang;

function ledger_error_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const code = $beagle$args[0];
    const message = $beagle$args[1];
    return (() => { throw wire_ledger_error_new_bang(code, message); })();
  }
  if (arguments.length === 3) {
    const code = $beagle$args[0];
    const message = $beagle$args[1];
    const cause = $beagle$args[2];
    return (() => { throw wire_ledger_error_new_bang(code, message, cause); })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function canonical_entity_bang(value, label) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(ENTITY.test(value)))) {
    ledger_error_bang("invalid_identity", $$bc$str("invalid wire ledger ", label));
  }
  return (((_truthy) => _truthy !== false && _truthy != null)(value.startsWith("@")) ? value : $$bc$str("@", value));
}

function wire_run_ledger_identity_bang(identity) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(identity.agent)))) {
    ledger_error_bang("invalid_identity", "invalid wire ledger agent");
  }
  const thread = ((identity.thread === "(ad-hoc)") ? identity.thread : canonical_entity_bang(identity.thread, "thread"));
  const parent_thread = ((identity.parentThread === undefined) ? null : canonical_entity_bang(identity.parentThread, "parentThread"));
  const coordinator = ((identity.coordinator === undefined) ? null : identity.coordinator.replace(new RegExp("^@agent:"), ""));
  const result = $$bh$js_obj("thread", thread, "agent", identity.agent);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test(coordinator))) : _logical))(coordinator))) {
    ledger_error_bang("invalid_identity", "invalid wire ledger coordinator");
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(parent_thread)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "parentThread", parent_thread);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(coordinator)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "coordinator", coordinator);
  }
  return Object.freeze(result);
}

const wireRunLedgerIdentity = wire_run_ledger_identity_bang;

function sha256(value) {
  const hash = create_hash("sha256");
  hash.update(value);
  return hash.digest("hex");
}

function canonical_string_array(values) {
  return $$bc$str("[", values.map((value) => JSON.stringify(value)).join(","), "]");
}

function canonical_event_bang(event) {
  return (() => { try {
    const line = encode_wire_jsonl_line(event, $$bh$js_obj("maxLineBytes", AGENT__RUN__LEDGER__CONTRACT.bounds.maxCanonicalEventBytes));
  const canonical = $$bh$js_obj("event", event, "json", line.slice(0, -1));
  return canonical;
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const error = _catch_0;
        return (() => { throw wire_ledger_error_new_bang("invalid_event", "wire ledger event is invalid or oversized", error); })();
        break;
      }
    }
  } })();
}

function event_subject(run_id, sequence) {
  const digest = sha256($$bc$str("north-wire-event-subject:v2\x00", run_id, "\x00", sequence));
  return $$bc$str("@run:wire-event-", digest);
}

function fact(predicate, value) {
  const result = [predicate, value];
  return result;
}

function wire_event_facts_bang(identity, event) {
  const context = wire_run_ledger_identity_bang(identity);
  const canonical = canonical_event_bang(event);
  const digest = sha256(canonical.json);
  const facts = [fact("kind", "wire_event"), fact("wire_ledger_version", AGENT__RUN__LEDGER__VERSION), fact("wire_version", canonical.event.version), fact("wire_run_id", canonical.event.runId), fact("thread", context.thread), fact("agent", context.agent), fact("wire_event_id", canonical.event.id), fact("wire_event_sequence", String(canonical.event.sequence)), fact("wire_event_at", canonical.event.at), fact("wire_event_kind", canonical.event.kind), fact("wire_event_essential", String(canonical.event.essential)), fact("wire_event_json", canonical.json), fact("wire_event_sha256", digest)];
  if ((!(context.parentThread === undefined))) {
    facts.push(fact("parent_thread", context.parentThread));
  }
  if ((!(context.coordinator === undefined))) {
    facts.push(fact("run_coordinator", context.coordinator));
  }
  return Object.freeze($$bh$host_object($$bc$keyword("subject"), event_subject(canonical.event.runId, canonical.event.sequence), $$bc$keyword("facts"), Object.freeze(facts)));
}

const wireEventFacts = wire_event_facts_bang;

function validate_event_slice_bang(events) {
  if ((events.length === 0)) {
    ledger_error_bang("invalid_batch", "wire ledger batch must not be empty");
  }
  if ((events.length > AGENT__RUN__LEDGER__CONTRACT.bounds.maxBatchEvents)) {
    ledger_error_bang("invalid_batch", $$bc$str("wire ledger batch exceeds ", AGENT__RUN__LEDGER__CONTRACT.bounds.maxBatchEvents, " events"));
  }
  const first = events[0];
  (() => { let index = 0; while (true) {
    if ((index < events.length)) { const event = events[index]; canonical_event_bang(event); (((!(event.runId === first.runId)) || (!(event.sequence === (first.sequence + index)))) ? (() => { return ledger_error_bang("invalid_batch", "wire ledger batch must be one contiguous run slice"); })() : null); (((index < (events.length - 1)) && (event.kind === "run.terminated")) ? (() => { return ledger_error_bang("invalid_batch", "wire ledger batch cannot continue after run.terminated"); })() : null); const _recur_0 = (index + 1); index = _recur_0; continue; } else { return null; }
  } })();
  return null;
}

function terminal_event(events) {
  return events[(events.length - 1)];
}

const TERMINAL_LIFECYCLES = ["completed", "failed", "cancelled", "blocked"];

function wire_ledger_summary_bang(events) {
  if (((events.length === 0) || (events.length > WIRE_MAX_EVENTS_PER_RUN))) {
    ledger_error_bang("invalid_summary", "wire ledger summary requires a bounded event sequence");
  }
  const snapshot = (() => { try {
    return reduce_wire_events(events);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const error = _catch_1;
        return (() => { throw wire_ledger_error_new_bang("invalid_summary", "wire ledger summary requires a valid event sequence", error); })();
        break;
      }
    }
  } })();
  const terminal = terminal_event(events);
  if (((!(terminal.kind === "run.terminated")) || (!((_truthy) => _truthy !== false && _truthy != null)(TERMINAL_LIFECYCLES.includes(snapshot.lifecycle))))) {
    ledger_error_bang("invalid_summary", "wire ledger summary requires run.terminated last");
  }
  const digests = events.map((event) => sha256(canonical_event_bang(event).json));
  return Object.freeze($$bh$host_object($$bc$keyword("version"), AGENT__RUN__LEDGER__VERSION, $$bc$keyword("wireVersion"), WIRE_VERSION, $$bc$keyword("runId"), snapshot.runId, $$bc$keyword("eventCount"), events.length, $$bc$keyword("firstSequence"), 0, $$bc$keyword("lastSequence"), terminal.sequence, $$bc$keyword("terminalEventId"), terminal.id, $$bc$keyword("digest"), sha256(canonical_string_array(digests))));
}

const wireLedgerSummary = wire_ledger_summary_bang;

function validate_batch_bang(events) {
  validate_event_slice_bang(events);
  const first = events[0];
  const terminal = terminal_event(events);
  if (((!(first.sequence === 0)) || ((!(first.kind === "run.started")) || (!(terminal.kind === "run.terminated"))))) {
    ledger_error_bang("invalid_batch", "wire ledger publication requires one complete terminal run");
  }
  (() => { try {
    return wire_ledger_summary_bang(events);
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const error = _catch_2;
        return ledger_error_bang("invalid_batch", "wire ledger publication requires a reducible terminal run", error);
        break;
      }
    }
  } })();
  return null;
}

function projection_payload_bang(projections) {
  const payload = JSON.stringify(projections);
  if ((new TextEncoder().encode(payload).byteLength > AGENT__RUN__LEDGER__CONTRACT.bounds.maxProjectionBatchBytes)) {
    ledger_error_bang("invalid_batch", "wire ledger projection batch exceeds its byte bound");
  }
  return payload;
}

async function run_writer_bang(projections, timeout_ms, env) {
  const payload = projection_payload_bang(projections);
  const command = ["bb"].concat(beagle_store_babashka_arguments([INTERNAL_WRITER, ((_logical) => (_logical !== false && _logical != null ? _logical : "7977"))(env.NORTH_PORT)], env));
  const spawn_options = $$bh$js_obj("env", beagle_store_environment(env), "stdin", "pipe", "stdout", "ignore", "stderr", "ignore");
  const child = bun_spawn(command, spawn_options);
  child.stdin.write(payload);
  child.stdin.end();
  const outcome = await settle_beagle_store_coordinator_child(child, timeout_ms);
  return Promise.resolve((((!((_truthy) => _truthy !== false && _truthy != null)(outcome.timedOut)) && (outcome.exitCode === 0)) ? RECORDED : UNAVAILABLE));
}

async function record_wire_event_projections_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const projections = $beagle$args[0];
    return record_wire_event_projections_bang(projections, 10000, process.env);
  }
  if (arguments.length === 2) {
    const projections = $beagle$args[0];
    const timeout_ms = $beagle$args[1];
    return record_wire_event_projections_bang(projections, timeout_ms, process.env);
  }
  if (arguments.length === 3) {
    const projections = $beagle$args[0];
    const timeout_ms = $beagle$args[1];
    const env = $beagle$args[2];
    if ((projections.length === 0)) {
      ledger_error_bang("invalid_batch", "wire ledger projection batch must not be empty");
    }
    return (() => { try {
    return run_writer_bang(projections, timeout_ms, env);
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const __error = _catch_3;
        return Promise.resolve(UNAVAILABLE);
        break;
      }
    }
  } })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const recordWireEventProjections = record_wire_event_projections_bang;

function default_wire_ledger_batch_writer_bang(projections, timeout_ms) {
  return record_wire_event_projections_bang(projections, timeout_ms);
}

const DEFAULT_WIRE_LEDGER_BATCH_WRITER = default_wire_ledger_batch_writer_bang;

function event_projections_bang(identity, events) {
  return events.map((event) => wire_event_facts_bang(identity, event));
}

function create_wire_event_store_publisher_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const identity = $beagle$args[0];
    return create_wire_event_store_publisher_bang(identity, $$bh$js_obj());
  }
  if (arguments.length === 2) {
    const identity = $beagle$args[0];
    const options = $beagle$args[1];
    const context = wire_run_ledger_identity_bang(identity);
    const timeout_ms = ((_logical) => (_logical !== false && _logical != null ? _logical : 10000))(options.timeoutMs);
    const writer = ((_logical) => (_logical !== false && _logical != null ? _logical : DEFAULT_WIRE_LEDGER_BATCH_WRITER))(options.writer);
    const next_sequence = ({value: 0, watches: {}});
    const poisoned = ({value: null, watches: {}});
    return $$bh$js_obj("publish", (events) => { const failure = poisoned.value;
return (((_truthy) => _truthy !== false && _truthy != null)(failure) ? Promise.reject(failure) : (() => { try {
    validate_event_slice_bang(events);
  if ((!(events[0].sequence === next_sequence.value))) {
    ledger_error_bang("invalid_batch", "wire event Store suffix does not begin at the next sequence");
  }
  const projections = event_projections_bang(context, events);
  const published = writer(projections, timeout_ms);
  const settled = published.then((status) => { if ((!(status === "recorded"))) {
  (() => { throw wire_ledger_error_new_bang("invalid_batch", "wire event Store publication is unavailable"); })();
}
(() => { const _a = next_sequence, _v = (next_sequence.value + events.length); const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
return null; });
  projection_payload_bang(projections);
  return settled.catch((error) => { (() => { const _a = poisoned, _v = error; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
return (() => { throw error; })(); });
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const error = _catch_4;
        (() => { const _a = poisoned, _v = error; const _old = _a.value; _a.value = _v; for (const _k in _a.watches) _a.watches[_k](_k, _a, _old, _v); return _v; })();
        return Promise.reject(error);
        break;
      }
    }
  } })()); });
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const createWireEventStorePublisher = create_wire_event_store_publisher_bang;

async function publish_wire_events_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const identity = $beagle$args[0];
    const events = $beagle$args[1];
    return publish_wire_events_bang(identity, events, 10000, DEFAULT_WIRE_LEDGER_BATCH_WRITER);
  }
  if (arguments.length === 3) {
    const identity = $beagle$args[0];
    const events = $beagle$args[1];
    const timeout_ms = $beagle$args[2];
    return publish_wire_events_bang(identity, events, timeout_ms, DEFAULT_WIRE_LEDGER_BATCH_WRITER);
  }
  if (arguments.length === 4) {
    const identity = $beagle$args[0];
    const events = $beagle$args[1];
    const timeout_ms = $beagle$args[2];
    const writer = $beagle$args[3];
    validate_batch_bang(events);
    const projections = event_projections_bang(identity, events);
    projection_payload_bang(projections);
    return writer(projections, timeout_ms);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const publishWireEvents = publish_wire_events_bang;

function is_wire_run_ledger_summary_p(value) {
  return ((!(value == null)) && ((typeof value === "object") && ((!Array.isArray(value)) && ((value.version === AGENT__RUN__LEDGER__VERSION) && ((value.wireVersion === WIRE_VERSION) && ((typeof value.runId === "string") && ((_logical) => (_logical !== false && _logical != null ? (Number.isSafeInteger(value.eventCount) && ((value.eventCount > 0) && ((value.firstSequence === 0) && (Number.isSafeInteger(value.lastSequence) && ((value.lastSequence === (value.eventCount - 1)) && ((typeof value.terminalEventId === "string") && ((_logical) => (_logical !== false && _logical != null ? ((typeof value.digest === "string") && SHA256.test(value.digest)) : _logical))(WIRE_ID.test(value.terminalEventId)))))))) : _logical))(WIRE_ID.test(value.runId))))))));
}

const isWireRunLedgerSummary = is_wire_run_ledger_summary_p;

export { AGENT__RUN__LEDGER__CONTRACT as "AGENT_RUN_LEDGER_CONTRACT" };
export { AGENT__RUN__LEDGER__VERSION as "AGENT_RUN_LEDGER_VERSION" };
export { WireLedgerError as "WireLedgerError" };
export { createWireEventStorePublisher as "createWireEventStorePublisher" };
export { isWireRunLedgerSummary as "isWireRunLedgerSummary" };
export { publishWireEvents as "publishWireEvents" };
export { recordWireEventProjections as "recordWireEventProjections" };
export { wireEventFacts as "wireEventFacts" };
export { wireLedgerSummary as "wireLedgerSummary" };
export { wireRunLedgerIdentity as "wireRunLedgerIdentity" };
