import { count as $$bc$count, eager_seq as $$bc$eager_seq, into_value as $$bc$into_value, keyword as $$bc$keyword, range as $$bc$range, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_array as $$bh$admit_host_array, admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, alength as $$bh$alength, aset as $$bh$aset, host_object as $$bh$host_object, into_array as $$bh$into_array, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const node_module = process.getBuiltinModule("node:module");

const create_require = node_module.createRequire;

const require_module = create_require(import.meta.url);

const path_module = process.getBuiltinModule("node:path");

const fs_module = process.getBuiltinModule("node:fs");

const crypto_module = process.getBuiltinModule("node:crypto");

const child_process_module = process.getBuiltinModule("node:child_process");

const resolve_path = path_module.resolve;

const read_file_text = fs_module.readFileSync;

const read_file_bytes = fs_module.readFileSync;

const create_hash = crypto_module.createHash;

const spawn_sync = child_process_module.spawnSync;

const orchestration_staffing_module = require_module("./orchestration-staffing");

const DEFAULT_ORCHESTRATION_STAFFING_PATH = orchestration_staffing_module.DEFAULT_ORCHESTRATION_STAFFING_PATH;

const resource_policy_module = require_module("./resource-policy");

const DEFAULT_ROUTING_POLICY_PATH = resource_policy_module.DEFAULT_ROUTING_POLICY_PATH;

const orchestration_graph_source_module = require_module("./orchestration-graph-source");

const staffing_source = orchestration_graph_source_module.staffingSource;

const catalog_graph_pin_for_admission = orchestration_graph_source_module.catalogGraphPinForAdmission;

const orchestration_policy_pin_module = require_module("./orchestration-policy-pin");

const verify_policy_digest_pin = orchestration_policy_pin_module.verifyPolicyDigestPin;

const ROUTING__ASSESSMENT__POLICY__VERSION = "minimum-sufficient-v2";

const ROUTING__PIN__POLICY__VERSION = "north-routing-pin-v1";

const MAX__PIN__LIFETIME__MS = 86400000;

const AGENT_MACHINERY = resolve_path(import.meta.dir, "../..", "agent-machinery");

const SIGNAL_KEYS = ["decisionOwnership", "seamScope", "errorExposure", "oracleStrength", "foundationalImpact", "dependencyShape", "reasoningShape"];

const SIGNAL_VALUES = $$bh$host_object($$bc$keyword("decisionOwnership"), ["none", "bounded", "cross-boundary", "system-shaping", "open-solution-class"], $$bc$keyword("seamScope"), ["none", "established", "consequential", "system-wide"], $$bc$keyword("errorExposure"), ["contained-reversible", "material-recoverable", "high-or-hard-to-reverse"], $$bc$keyword("oracleStrength"), ["not-applicable", "objective-local", "objective-end-to-end", "partial", "judgment-only"], $$bc$keyword("foundationalImpact"), ["none", "implementation-only", "invariant-decision-owned"], $$bc$keyword("dependencyShape"), ["atomic-cohesive", "deterministic-workflow", "parallel-breadth", "dynamic-decomposition", "tightly-coupled-sequential"], $$bc$keyword("reasoningShape"), ["deterministic", "bounded-branching", "multi-hypothesis", "system-synthesis", "exceptional"]);

const ASSESSMENT_FIELDS = ["$schema", "version", "signals", "derived", "selected", "exception", "exceptionalDeliberation"];

const DERIVED_FIELDS = ["minimumCapabilityFloor", "minimumReasoning", "ruleCodes"];

const SELECTED_FIELDS = ["capabilityFloor", "reasoning"];

const EXCEPTION_FIELDS = ["code", "detail"];

const PIN_FIELDS = ["policyVersion", "issuedAt", "expiresAt", "reasonCode", "detail", "pins"];

const PIN_ITEM_FIELDS = ["kind", "value"];

const CAPABILITY_FLOORS = ["baseline", "standard", "advanced", "frontier"];

const SERVICE_CLASSES = ["economy", "fast", "balanced", "premium"];

const REASONING_LEVELS = ["low", "medium", "high", "xhigh", "max"];

const EXCEPTION_CODES = ["explicit-human-floor", "recent-lower-capability-failure", "calibration-experiment", "unmodeled-risk"];

const PIN_REASON_CODES = ["explicit-human-request", "provider-recovery", "capability-requirement", "calibration-experiment"];

const PIN_KINDS = ["provider", "account", "model"];

function error_bang(message) {
  return (() => { throw new Error(message); })();
}

function as_object(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function as_array(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function as_string(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function foreign_object_bang(value, label) {
  if (((value == null) || ((!(typeof value === "object")) || Array.isArray(value)))) {
    error_bang($$bc$str(label, " must be an object"));
  }
  return as_object(value);
}

function exact_fields_bang(value, expected, label) {
  const unknown = Object.keys(value).filter((field) => (!((_truthy) => _truthy !== false && _truthy != null)(expected.includes(field))));
  if ((!($$bc$count(unknown) === 0))) {
    error_bang($$bc$str(label, " has unknown field(s): ", unknown.join(", ")));
  }
  return null;
}

function required_string_bang(value, label) {
  if (((!(typeof value === "string")) || (as_string(value).trim() === ""))) {
    error_bang($$bc$str(label, " must be a non-empty string"));
  }
  return as_string(value).trim();
}

function enum_value_bang(value, allowed, label) {
  if (((!(typeof value === "string")) || (!((_truthy) => _truthy !== false && _truthy != null)(allowed.includes(as_string(value)))))) {
    error_bang($$bc$str(label, " has an unsupported value"));
  }
  return as_string(value);
}

function unique_strings_bang(value, label) {
  if (((!Array.isArray(value)) || (as_array(value).length === 0))) {
    error_bang($$bc$str(label, " must be a non-empty array"));
  }
  const raw = as_array(value);
  const normalized = $$bc$eager_seq($$bc$range($$bh$alength(raw)).map((index) => required_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw, index), $$bc$str(label, "[", index, "]"))));
  if ((!(new Set(normalized).size === normalized.length))) {
    error_bang($$bc$str(label, " must not contain duplicates"));
  }
  return normalized;
}

function iso_instant_bang(value, label) {
  const source = required_string_bang(value, label);
  const time = Date.parse(source);
  if (((!Number.isFinite(time)) || (!((_truthy) => _truthy !== false && _truthy != null)(source.includes("T"))))) {
    error_bang($$bc$str(label, " must be an ISO instant"));
  }
  return $$bh$host_object($$bc$keyword("source"), new Date(time).toISOString(), $$bc$keyword("time"), time);
}

function deep_freeze(value) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof value === "object") && (!Object.isFrozen(value))) : _logical))(value))) {
    Object.values(value).forEach((child) => {
  deep_freeze(child);
});
    Object.freeze(value);
  }
  return value;
}

function canonical(value) {
  return ((Array.isArray(value)) ? (value).map((child) => canonical(child)) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (typeof value === "object") : _logical))(value))) ? (() => { const entries = Object.entries(value).sort((left, right) => ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(left, 0)).localeCompare((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(right, 0))); return Object.fromEntries(entries.map((entry) => $$bh$into_array([(($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0), canonical((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1))]))); })() : value);
}

function canonical_json_value(value) {
  return JSON.stringify(canonical(value));
}

const canonicalJson = (value) => canonical_json_value(value);

function digest(value) {
  return create_hash("sha256").update(canonical_json_value(value)).digest("hex");
}

function file_digest(path) {
  return (() => { try {
    return create_hash("sha256").update(read_file_bytes(path)).digest("hex");
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __error = _catch_0;
        return "unavailable";
        break;
      }
    }
  } })();
}

function admitted_signals_bang(raw, surface) {
  exact_fields_bang(raw, SIGNAL_KEYS, $$bc$str(surface, ".signals"));
  const signals = $$bh$js_obj();
  SIGNAL_KEYS.forEach((key) => {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(signals, key, enum_value_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, key), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(SIGNAL_VALUES, key), $$bc$str(surface, ".signals.", key)));
});
  return signals;
}

function admit_routing_assessment_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const request = $beagle$args[1];
    return admit_routing_assessment_bang(value, request, "managed North routing assessment");
  }
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const request = $beagle$args[1];
    const surface = $beagle$args[2];
    if ((value === undefined)) {
      return null;
    } else {
      const raw = foreign_object_bang(value, surface);
      exact_fields_bang(raw, ASSESSMENT_FIELDS, surface);
      if ((!((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "version") === ROUTING__ASSESSMENT__POLICY__VERSION))) {
        error_bang($$bc$str(surface, ".version must be ", ROUTING__ASSESSMENT__POLICY__VERSION));
      }
      const schema = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "$schema") === undefined) ? null : required_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "$schema"), $$bc$str(surface, ".$schema")));
      const signals = admitted_signals_bang(foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "signals"), $$bc$str(surface, ".signals")), surface);
      const raw_derived = foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "derived"), $$bc$str(surface, ".derived"));
      const __derived_fields = exact_fields_bang(raw_derived, DERIVED_FIELDS, $$bc$str(surface, ".derived"));
      const derived = $$bh$host_object($$bc$keyword("minimumCapabilityFloor"), enum_value_bang($$bh$aget(raw_derived, "minimumCapabilityFloor"), CAPABILITY_FLOORS, $$bc$str(surface, ".derived.minimumCapabilityFloor")), $$bc$keyword("minimumReasoning"), enum_value_bang($$bh$aget(raw_derived, "minimumReasoning"), REASONING_LEVELS, $$bc$str(surface, ".derived.minimumReasoning")), $$bc$keyword("ruleCodes"), unique_strings_bang($$bh$aget(raw_derived, "ruleCodes"), $$bc$str(surface, ".derived.ruleCodes")));
      const raw_selected = foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "selected"), $$bc$str(surface, ".selected"));
      const __selected_fields = exact_fields_bang(raw_selected, SELECTED_FIELDS, $$bc$str(surface, ".selected"));
      const selected = $$bh$host_object($$bc$keyword("capabilityFloor"), enum_value_bang($$bh$aget(raw_selected, "capabilityFloor"), CAPABILITY_FLOORS, $$bc$str(surface, ".selected.capabilityFloor")), $$bc$keyword("reasoning"), enum_value_bang($$bh$aget(raw_selected, "reasoning"), REASONING_LEVELS, $$bc$str(surface, ".selected.reasoning")));
      if (((!(selected.capabilityFloor === request.capabilityFloor)) || (!(selected.reasoning === request.reasoning)))) {
        error_bang($$bc$str(surface, ".selected must equal the admitted RoutingRequest capabilityFloor/reasoning"));
      }
      const changed = ((!(selected.capabilityFloor === derived.minimumCapabilityFloor)) || (!(selected.reasoning === derived.minimumReasoning)));
      const exception = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "exception") === undefined) ? null : (() => { const candidate = foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "exception"), $$bc$str(surface, ".exception")); exact_fields_bang(candidate, EXCEPTION_FIELDS, $$bc$str(surface, ".exception"));
return $$bh$host_object($$bc$keyword("code"), enum_value_bang($$bh$aget(candidate, "code"), EXCEPTION_CODES, $$bc$str(surface, ".exception.code")), $$bc$keyword("detail"), required_string_bang($$bh$aget(candidate, "detail"), $$bc$str(surface, ".exception.detail"))); })());
      if ((!(changed === (!(exception == null))))) {
        error_bang($$bc$str(surface, ".exception is required exactly when selected differs from derived"));
      }
      const maximum = ((derived.minimumReasoning === "max") || (selected.reasoning === "max"));
      const exceptional_deliberation = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "exceptionalDeliberation") === undefined) ? null : required_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "exceptionalDeliberation"), $$bc$str(surface, ".exceptionalDeliberation")));
      if ((!(maximum === (!(exceptional_deliberation == null))))) {
        error_bang($$bc$str(surface, ".exceptionalDeliberation is required exactly when derived or selected reasoning is max"));
      }
      const admitted = $$bh$host_object($$bc$keyword("version"), ROUTING__ASSESSMENT__POLICY__VERSION, $$bc$keyword("signals"), signals, $$bc$keyword("derived"), derived, $$bc$keyword("selected"), selected);
      if (((_truthy) => _truthy !== false && _truthy != null)(schema)) {
        (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(admitted, "$schema", schema);
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(exception)) {
        (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(admitted, "exception", exception);
      }
      if (((_truthy) => _truthy !== false && _truthy != null)(exceptional_deliberation)) {
        (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(admitted, "exceptionalDeliberation", exceptional_deliberation);
      }
      const env = process.env;
      const validator = ((_logical) => (_logical !== false && _logical != null ? _logical : resolve_path(AGENT_MACHINERY, "scripts/selection-assessment.mjs")))(env.ORCHESTRATION_SELECTION_ASSESSMENT_MODULE);
      const validation = spawn_sync(process.execPath, ["--eval", "import {pathToFileURL} from 'node:url';const m=await import(pathToFileURL(process.argv[1]).href);let s='';for await(const c of process.stdin)s+=c;process.stdout.write(JSON.stringify(m.validateSelectionAssessment(JSON.parse(s))));", validator], $$bh$host_object($$bc$keyword("input"), JSON.stringify(admitted), $$bc$keyword("encoding"), "utf8", $$bc$keyword("timeout"), 5000));
      if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : (!(validation.status === 0))))(validation.error))) {
        const stderr = ((typeof validation.stderr === "string") ? validation.stderr.trim() : "");
        const error_message = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? validation.error.message : _logical))(validation.error)) ? validation.error.message : "");
        error_bang($$bc$str(surface, " failed canonical Orchestration validation: ", ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : "canonical validator failed"))(error_message)))(stderr)));
      }
      const canonical_assessment = (() => { try {
    return JSON.parse(validation.stdout);
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __error = _catch_1;
        error_bang($$bc$str(surface, " canonical Orchestration validator returned invalid JSON"));
        return $$bh$js_obj();
        break;
      }
    }
  } })();
      if ((!(canonical_json_value(canonical_assessment) === canonical_json_value(admitted)))) {
        error_bang($$bc$str(surface, " canonical Orchestration validator changed the admitted assessment"));
      }
      return deep_freeze(admitted);
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const admitRoutingAssessment = admit_routing_assessment_bang;

function admit_routing_pin_evidence_bang(...$beagle$args) {
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const pins = $beagle$args[1];
    return admit_routing_pin_evidence_bang(value, pins, new Date(), "managed North routing pin evidence");
  }
  if (arguments.length === 3) {
    const value = $beagle$args[0];
    const pins = $beagle$args[1];
    const now = $beagle$args[2];
    return admit_routing_pin_evidence_bang(value, pins, now, "managed North routing pin evidence");
  }
  if (arguments.length === 4) {
    const value = $beagle$args[0];
    const pins = $beagle$args[1];
    const now = $beagle$args[2];
    const surface = $beagle$args[3];
    if ((value === undefined)) {
      return null;
    } else {
      const raw = foreign_object_bang(value, surface);
      exact_fields_bang(raw, PIN_FIELDS, surface);
      if ((!((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "policyVersion") === ROUTING__PIN__POLICY__VERSION))) {
        error_bang($$bc$str(surface, ".policyVersion must be ", ROUTING__PIN__POLICY__VERSION));
      }
      const issued = iso_instant_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "issuedAt"), $$bc$str(surface, ".issuedAt"));
      const expires = iso_instant_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "expiresAt"), $$bc$str(surface, ".expiresAt"));
      const issued_time = issued.time;
      const expires_time = expires.time;
      const now_time = now.getTime();
      if ((issued_time > (now_time + 60000))) {
        error_bang($$bc$str(surface, ".issuedAt is in the future"));
      }
      if ((expires_time <= now_time)) {
        error_bang($$bc$str(surface, " is expired"));
      }
      if (((expires_time <= issued_time) || ((expires_time - issued_time) > MAX__PIN__LIFETIME__MS))) {
        error_bang($$bc$str(surface, " lifetime must be positive and no more than 24 hours"));
      }
      const raw_pins_value = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "pins");
      if (((!Array.isArray(raw_pins_value)) || (as_array(raw_pins_value).length === 0))) {
        error_bang($$bc$str(surface, ".pins must be non-empty"));
      }
      const raw_pins = as_array(raw_pins_value);
      const admitted_pins = $$bc$eager_seq($$bc$range($$bh$alength(raw_pins)).map((index) => (() => { const label = $$bc$str(surface, ".pins[", index, "]"); const pin = foreign_object_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw_pins, index), label); exact_fields_bang(pin, PIN_ITEM_FIELDS, label);
return $$bh$host_object($$bc$keyword("kind"), enum_value_bang($$bh$aget(pin, "kind"), PIN_KINDS, $$bc$str(label, ".kind")), $$bc$keyword("value"), required_string_bang($$bh$aget(pin, "value"), $$bc$str(label, ".value"))); })()));
      const unique_kinds = new Set(admitted_pins.map((pin) => pin.kind));
      if ((!(unique_kinds.size === admitted_pins.length))) {
        error_bang($$bc$str(surface, ".pins may contain each kind at most once"));
      }
      const expected = Object.entries(pins).filter((entry) => (!((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1) == null)));
      if (((_truthy) => _truthy !== false && _truthy != null)(((!(expected.length === admitted_pins.length)) || expected.some((entry) => { const kind = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 0);
const expected_value = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(entry, 1);
return (!((_truthy) => _truthy !== false && _truthy != null)(admitted_pins.some((pin) => ((pin.kind === kind) && (pin.value === expected_value))))); })))) {
        error_bang($$bc$str(surface, ".pins must exactly match explicit provider/account/model selectors"));
      }
      return deep_freeze($$bh$host_object($$bc$keyword("policyVersion"), ROUTING__PIN__POLICY__VERSION, $$bc$keyword("issuedAt"), issued.source, $$bc$keyword("expiresAt"), expires.source, $$bc$keyword("reasonCode"), enum_value_bang($$bh$aget(raw, "reasonCode"), PIN_REASON_CODES, $$bc$str(surface, ".reasonCode")), $$bc$keyword("detail"), required_string_bang($$bh$aget(raw, "detail"), $$bc$str(surface, ".detail")), $$bc$keyword("pins"), admitted_pins));
    }
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const admitRoutingPinEvidence = admit_routing_pin_evidence_bang;

const policy_pin_cache = $$bh$host_object($$bc$keyword("digest"), undefined);

function graph_policy_pin_bang(surface) {
  if ((policy_pin_cache.digest == null)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(policy_pin_cache, "digest", verify_policy_digest_pin(undefined, surface));
  }
  const digest_value = policy_pin_cache.digest;
  if (((_truthy) => _truthy !== false && _truthy != null)(digest_value)) {
    return digest_value;
  } else {
    error_bang("routing policy pin was not established");
    return "";
  }
}

const catalog_pin_cache = $$bh$host_object($$bc$keyword("pin"), undefined);

function graph_catalog_pin_bang() {
  if ((catalog_pin_cache.pin == null)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(catalog_pin_cache, "pin", catalog_graph_pin_for_admission());
  }
  const pin_value = catalog_pin_cache.pin;
  if (((_truthy) => _truthy !== false && _truthy != null)(pin_value)) {
    return pin_value;
  } else {
    error_bang("routing catalog pin was not established");
    return $$bh$js_obj();
  }
}

function stock_axes_bang(request) {
  if ((!(request.composition.kind === "template"))) {
    return null;
  } else {
    const env = process.env;
    const catalog = JSON.parse(read_file_text(((_logical) => (_logical !== false && _logical != null ? _logical : DEFAULT_ORCHESTRATION_STAFFING_PATH))(env.ORCHESTRATION_STAFFING_CATALOG), "utf8"));
    const template_id = request.composition.id;
    const presets = ((_logical) => (_logical !== false && _logical != null ? _logical : []))(catalog.presets);
    const preset = presets.find((candidate) => (candidate.name === template_id));
    if ((preset == null)) {
      error_bang($$bc$str("Orchestration stock preset ", template_id, " is absent while issuing admission receipt"));
    }
    return $$bh$host_object($$bc$keyword("taskGrade"), String(preset.taskGrade), $$bc$keyword("topology"), String(preset.topology), $$bc$keyword("capabilityFloor"), enum_value_bang(preset.capabilityFloor, CAPABILITY_FLOORS, $$bc$str("Orchestration stock preset ", template_id, ".capabilityFloor")), $$bc$keyword("serviceClass"), enum_value_bang(preset.serviceClass, SERVICE_CLASSES, $$bc$str("Orchestration stock preset ", template_id, ".serviceClass")), $$bc$keyword("reasoning"), String(preset.deliberation), $$bc$keyword("posture"), String(preset.posture));
  }
}

function admit_routing_economics_bang(args) {
  const surface = ((_logical) => (_logical !== false && _logical != null ? _logical : "managed North routing economics"))(args.surface);
  const request = args.request;
  const assessment = admit_routing_assessment_bang(args.routingAssessment, request, surface);
  const explicit_pins = $$bh$js_obj();
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(args.provider === "auto")) : _logical))(args.provider))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(explicit_pins, "provider", args.provider);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(args.target)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(explicit_pins, "account", args.target);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(args.model)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(explicit_pins, "model", args.model);
  }
  const pin_evidence = admit_routing_pin_evidence_bang(args.pinEvidence, explicit_pins, ((_logical) => (_logical !== false && _logical != null ? _logical : new Date()))(args.now), $$bc$str(surface, " pin evidence"));
  if (((request.reasoning === "max") && (assessment == null))) {
    error_bang($$bc$str(surface, " reasoning=max requires a canonical routingAssessment with exceptional deliberation"));
  }
  if (((Object.keys(explicit_pins).length > 0) && (pin_evidence == null))) {
    error_bang($$bc$str(surface, " explicit provider/account/model selectors require current typed pinEvidence"));
  }
  const graph_mode = (staffing_source() === "graph");
  const policy_pin = (graph_mode ? graph_policy_pin_bang(surface) : null);
  const catalog_pin = (graph_mode ? graph_catalog_pin_bang() : null);
  const env = process.env;
  const orchestration_root = resolve_path(((_logical) => (_logical !== false && _logical != null ? _logical : resolve_path(import.meta.dir, "..", "..", "agent-runtime/orchestration")))(env.NORTH_AGENT_RUNTIME_HOME));
  const provider_digests = (graph_mode ? null : $$bh$host_object($$bc$keyword("anthropic"), file_digest(resolve_path(orchestration_root, "providers/anthropic.json")), $$bc$keyword("openai"), file_digest(resolve_path(orchestration_root, "providers/openai.json"))));
  const stock = stock_axes_bang(request);
  const overrides = ((request.composition.kind === "template") ? $$bc$into_value([], request.composition.overrides) : []);
  const receipt = $$bh$host_object($$bc$keyword("version"), 1, $$bc$keyword("routingRequestSha256"), digest(request), $$bc$keyword("routingPolicySha256"), file_digest(((_logical) => (_logical !== false && _logical != null ? _logical : DEFAULT_ROUTING_POLICY_PATH))(env.NORTH_ROUTING_POLICY)), $$bc$keyword("appliedAxes"), $$bh$host_object($$bc$keyword("taskGrade"), request.taskGrade, $$bc$keyword("topology"), request.topology, $$bc$keyword("capabilityFloor"), request.capabilityFloor, $$bc$keyword("serviceClass"), request.serviceClass, $$bc$keyword("reasoning"), request.reasoning, $$bc$keyword("posture"), request.posture), $$bc$keyword("overrideEvidence"), $$bh$host_object($$bc$keyword("changedAxes"), overrides, $$bc$keyword("status"), (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? assessment.exception : _logical))(assessment)) ? "assessment-exception" : (((_truthy) => _truthy !== false && _truthy != null)((!((_truthy) => _truthy !== false && _truthy != null)(($$bc$count(overrides) === 0)))) ? "composition-only" : "none"))), $$bc$keyword("pinEvidenceStatus"), (((_truthy) => _truthy !== false && _truthy != null)((Object.keys(explicit_pins).length === 0)) ? "none" : "current"));
  if (((_truthy) => _truthy !== false && _truthy != null)(assessment)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "routingAssessmentSha256", digest(assessment));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(pin_evidence)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "pinEvidenceSha256", digest(pin_evidence));
  }
  if ((!graph_mode)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "staffingCatalogSha256", file_digest(((_logical) => (_logical !== false && _logical != null ? _logical : DEFAULT_ORCHESTRATION_STAFFING_PATH))(env.ORCHESTRATION_STAFFING_CATALOG)));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "providerCatalogsSha256", digest(provider_digests));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(stock)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "stockAxes", stock);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? assessment.exception : _logical))(assessment))) {
    $$bh$aset(receipt.overrideEvidence, "exceptionCode", assessment.exception.code);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(policy_pin)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "orchestrationPolicyPinSha256", policy_pin);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(catalog_pin)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "orchestrationCatalogDigestSha256", catalog_pin.catalogDigestSha256);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "orchestrationCatalogVersion", catalog_pin.catalogVersion);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(receipt, "orchestrationCatalogTxVersion", catalog_pin.coordinatorVersion);
  }
  const result = $$bh$host_object($$bc$keyword("receipt"), receipt);
  if (((_truthy) => _truthy !== false && _truthy != null)(assessment)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "assessment", assessment);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(pin_evidence)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, "pinEvidence", pin_evidence);
  }
  return deep_freeze(result);
}

const admitRoutingEconomics = admit_routing_economics_bang;

function json_env_bang(name) {
  const source = $$bh$aget(process.env, name);
  return ((source == null) ? undefined : (() => { try {
    return JSON.parse(source);
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const __error = _catch_2;
        error_bang($$bc$str(name, " must contain valid JSON"));
        return undefined;
        break;
      }
    }
  } })());
}

function routing_economics_from_env_bang(request) {
  const env = process.env;
  return admit_routing_economics_bang($$bh$host_object($$bc$keyword("request"), request, $$bc$keyword("routingAssessment"), json_env_bang("AGENT_ROUTING_ASSESSMENT"), $$bc$keyword("pinEvidence"), json_env_bang("NORTH_ROUTING_PIN_EVIDENCE"), $$bc$keyword("provider"), env.AGENT_PROVIDER, $$bc$keyword("target"), env.AGENT_TARGET, $$bc$keyword("model"), env.AGENT_MODEL, $$bc$keyword("surface"), "managed North environment routing economics"));
}

const routingEconomicsFromEnv = routing_economics_from_env_bang;

export { MAX__PIN__LIFETIME__MS as "MAX_PIN_LIFETIME_MS" };
export { ROUTING__ASSESSMENT__POLICY__VERSION as "ROUTING_ASSESSMENT_POLICY_VERSION" };
export { ROUTING__PIN__POLICY__VERSION as "ROUTING_PIN_POLICY_VERSION" };
export { admitRoutingAssessment as "admitRoutingAssessment" };
export { admitRoutingEconomics as "admitRoutingEconomics" };
export { admitRoutingPinEvidence as "admitRoutingPinEvidence" };
export { canonicalJson as "canonicalJson" };
export { routingEconomicsFromEnv as "routingEconomicsFromEnv" };
