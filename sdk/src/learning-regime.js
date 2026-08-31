import { count as $$bc$count, distinct_equivV as $$bc$distinct_equiv, eager_seq as $$bc$eager_seq, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, aset as $$bh$aset, into_array as $$bh$into_array, js_keys as $$bh$js_keys, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const fs_module = require("node:fs");

const os_module = require("node:os");

const path_module = require("node:path");

const routing_metadata_module = require("./routing-metadata");

const composition_receipt_module = require("./composition-receipt");

const read_file_text = fs_module.readFileSync;

const home_directory = os_module.homedir;

const resolve_path = path_module.resolve;

const canonical_receipt_json = composition_receipt_module.canonicalReceiptJson;

const sha256_bytes = composition_receipt_module.sha256Bytes;

const sha256_manifest = composition_receipt_module.sha256Manifest;

const LEARNING__POLICY__VERSION = "north-learning-policy:v1";

const LEARNING__ASSIGNMENT__VERSION = "north-learning-assignment:v1";

const LEARNING__AXES = ["capabilityFloor", "serviceClass", "reasoning", "prompt", "authoring", "history"];

const CAPABILITY_FLOORS = routing_metadata_module.CAPABILITY_FLOORS;

const SERVICE_CLASSES = routing_metadata_module.SERVICE_CLASSES;

const REASONING_LEVELS = routing_metadata_module.REASONING_LEVELS;

const RISK_ORDER = ["p0", "p1", "p2", "p3"];

const ROUTE_AXES = ["capabilityFloor", "serviceClass", "reasoning"];

const SHA256 = new RegExp("^[a-f0-9]{64}$");

const IDENTIFIER = new RegExp("^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$");

function error_bang(message) {
  return (() => { throw new Error(message); })();
}

function as_object(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function as_number(value) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", value), "value");
}

function require_object_bang(value, label) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((value == null) || ((!(typeof value === "object")) || Array.isArray(value))))) {
    error_bang($$bc$str(label, " must be an object"));
  }
  return as_object(value);
}

function exact_object_bang(value, allowed, required, label) {
  const raw = require_object_bang(value, label);
  const __admitted = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "__beagle_object_admission__");
  const actual = $$bc$eager_seq($$bh$js_keys(raw).map((key) => key));
  const unknown = actual.filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed.includes(key))));
  const missing = required.filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(actual.includes(key))));
  if (($$bc$count(unknown) > 0)) {
    error_bang($$bc$str(label, " has unknown field(s): ", unknown.join(", ")));
  }
  if (($$bc$count(missing) > 0)) {
    error_bang($$bc$str(label, " is missing field(s): ", missing.join(", ")));
  }
  return raw;
}

function require_identifier_bang(value, label) {
  if (((!(typeof value === "string")) || (!((_truthy) => _truthy !== false && _truthy != null)(IDENTIFIER.test($$bc$str(value)))))) {
    error_bang($$bc$str(label, " must be a portable identifier"));
  }
  return $$bc$str(value);
}

function require_member_bang(value, values, label) {
  if (((!(typeof value === "string")) || (!((_truthy) => _truthy !== false && _truthy != null)(values.includes($$bc$str(value)))))) {
    error_bang($$bc$str(label, " must be one of: ", values.join(", ")));
  }
  return $$bc$str(value);
}

function raw_string_vector_bang(value, label) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(Array.isArray(value)))) {
    error_bang($$bc$str(label, " must be an array"));
  }
  return $$bc$eager_seq($$bh$into_array(value).map((entry) => require_identifier_bang(entry, label)));
}

const POLICY_FIELDS = ["version", "mode", "intensity", "axes", "maxAxisDelta", "riskCeiling", "seed", "epoch", "evidenceMode"];

function validate_learning_policy_bang(value) {
  const raw = exact_object_bang(value, POLICY_FIELDS, POLICY_FIELDS, "learning policy");
  const axes = raw_string_vector_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "axes"), "learning axes");
  const unknown_axes = axes.filter((axis) => (!((_truthy) => _truthy !== false && _truthy != null)(LEARNING__AXES.includes(axis))));
  const intensity = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "intensity");
  const max_axis_delta = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "maxAxisDelta");
  if ((!((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "version") === 1))) {
    error_bang("learning policy version must be 1");
  }
  if (($$bc$count(unknown_axes) > 0)) {
    error_bang($$bc$str("learning axes must contain only: ", LEARNING__AXES.join(", ")));
  }
  if ((!($$bc$distinct_equiv(axes).length === axes.length))) {
    error_bang("learning axes must not contain duplicates");
  }
  if (((!(typeof intensity === "number")) || ((!Number.isFinite(intensity)) || ((as_number(intensity) < 0) || (as_number(intensity) > 1))))) {
    error_bang("learning intensity must be between 0 and 1");
  }
  if (((!(typeof max_axis_delta === "number")) || ((!Number.isSafeInteger(max_axis_delta)) || ((as_number(max_axis_delta) < 0) || (as_number(max_axis_delta) > ($$bc$count(REASONING_LEVELS) - 1)))))) {
    error_bang($$bc$str("learning maxAxisDelta must be 0..", ($$bc$count(REASONING_LEVELS) - 1)));
  }
  return Object.freeze($$bh$js_obj("version", 1, "mode", require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "mode"), ["frozen", "learning"], "learning mode"), "intensity", intensity, "axes", Object.freeze(axes), "maxAxisDelta", max_axis_delta, "riskCeiling", require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "riskCeiling"), RISK_ORDER, "learning risk ceiling"), "seed", require_identifier_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "seed"), "learning seed"), "epoch", require_identifier_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "epoch"), "learning epoch"), "evidenceMode", require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "evidenceMode"), ["discovery", "evaluation"], "learning evidence mode")));
}

const validateLearningPolicy = validate_learning_policy_bang;

const DEFAULT__LEARNING__POLICY = Object.freeze($$bh$js_obj("version", 1, "mode", "frozen", "intensity", 0.1, "axes", Object.freeze(["capabilityFloor", "serviceClass", "reasoning", "prompt", "authoring", "history"]), "maxAxisDelta", 1, "riskCeiling", "p1", "seed", "north-default", "epoch", "1", "evidenceMode", "discovery"));

function learning_policy_path(...$beagle$args) {
  if (arguments.length === 0) {
    return learning_policy_path(process.env);
  }
  if (arguments.length === 1) {
    const env = $beagle$args[0];
    return ((_logical) => (_logical !== false && _logical != null ? _logical : resolve_path(((_logical) => (_logical !== false && _logical != null ? _logical : home_directory()))((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(env, "HOME")), ".config/north/learning-policy.json")))((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(env, "NORTH_LEARNING_POLICY"));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const learningPolicyPath = learning_policy_path;

function load_learning_policy_bang(...$beagle$args) {
  if (arguments.length === 0) {
    return load_learning_policy_bang(learning_policy_path(), read_file_text);
  }
  if (arguments.length === 1) {
    const path = $beagle$args[0];
    return load_learning_policy_bang(path, read_file_text);
  }
  if (arguments.length === 2) {
    const path = $beagle$args[0];
    const read = $beagle$args[1];
    return (() => { try {
    return validate_learning_policy_bang(JSON.parse(read(path, "utf8")));
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const error = _catch_0;
        if ((error.code === "ENOENT")) {
          return DEFAULT__LEARNING__POLICY;
        } else {
          error_bang($$bc$str("invalid learning policy ", path, ": ", error.message));
          return DEFAULT__LEARNING__POLICY;
        }
        break;
      }
    }
  } })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const loadLearningPolicy = load_learning_policy_bang;

function learning_policy_sha256(policy) {
  return sha256_manifest(policy);
}

const learningPolicySha256 = learning_policy_sha256;

function unit_interval(key) {
  return (parseInt(sha256_bytes(key).slice(0, 13), 16) / 4503599627370496);
}

function select_arm(values, draw) {
  return values[Math.min((values.length - 1), Math.floor((draw * values.length)))];
}

function route_values(axis) {
  return (((axis === "capabilityFloor")) ? CAPABILITY_FLOORS : ((axis === "serviceClass")) ? SERVICE_CLASSES : ((axis === "reasoning")) ? REASONING_LEVELS : null);
}

function baseline_arm_bang(baseline, axis) {
  return require_identifier_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(baseline, axis), $$bc$str("baseline ", axis));
}

function floor_arm_bang(input, axis, baseline) {
  const hard_floor = input.hardFloor;
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((axis === "capabilityFloor") || (axis === "reasoning")) : _logical))(hard_floor)) ? require_identifier_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(hard_floor, axis), $$bc$str("hard floor ", axis)) : baseline);
}

function within_route_bounds_p_bang(axis, arm, input, policy) {
  const values = route_values(axis);
  if ((values == null)) {
    return true;
  } else {
    const baseline = baseline_arm_bang(input.baseline, axis);
    const floor = floor_arm_bang(input, axis, baseline);
    const candidate_index = values.indexOf(arm);
    const baseline_index = values.indexOf(baseline);
    const floor_index = ((axis === "serviceClass") ? 0 : values.indexOf(floor));
    return ((candidate_index >= 0) && ((candidate_index >= floor_index) && (Math.abs((candidate_index - baseline_index)) <= policy.maxAxisDelta)));
  }
}

function eligible_options_bang(policy, input) {
  const pinned_values = (((_truthy) => _truthy !== false && _truthy != null)(input.pinnedAxes) ? raw_string_vector_bang(input.pinnedAxes, "pinned learning axes") : []);
  const pinned = new Set(pinned_values);
  const arms = input.eligibleArms;
  const baseline = input.baseline;
  const policy_axes = policy.axes;
  const result = $$bh$js_obj();
  policy_axes.forEach((axis) => {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(pinned.has(axis)))) {
    const raw = (((_truthy) => _truthy !== false && _truthy != null)(arms) ? (() => { return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(arms, axis); })() : null);
    const values = ((raw == null) ? [] : raw_string_vector_bang(raw, $$bc$str("eligible ", axis, " arms")));
    const candidates = (Array.from($$bc$distinct_equiv(values))).filter((arm) => ((!(arm === baseline_arm_bang(baseline, axis))) && within_route_bounds_p_bang(axis, arm, input, policy))).sort();
    if (($$bc$count(candidates) > 0)) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(result, axis, Object.freeze(candidates));
    }
  }
});
  return Object.freeze(result);
}

function control_reason(policy, input, eligible_axes) {
  return (((policy.mode === "frozen")) ? "mode:frozen" : ((input.risk == null)) ? "risk:unknown" : ((RISK_ORDER.indexOf(input.risk) > RISK_ORDER.indexOf(policy.riskCeiling))) ? "risk:above-ceiling" : ((policy.intensity === 0)) ? "intensity:zero" : ((eligible_axes.length === 0)) ? "arms:none-eligible" : "assignment:control");
}

function validate_baseline_bang(baseline) {
  const raw = exact_object_bang(baseline, LEARNING__AXES, LEARNING__AXES, "learning baseline");
  require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "capabilityFloor"), CAPABILITY_FLOORS, "baseline capability floor");
  require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "serviceClass"), SERVICE_CLASSES, "baseline service class");
  require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "reasoning"), REASONING_LEVELS, "baseline reasoning");
  LEARNING__AXES.forEach((axis) => {
  require_identifier_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, axis), $$bc$str("baseline ", axis));
});
  return raw;
}

function validate_hard_floor_bang(hard_floor) {
  if ((hard_floor == null)) {
    return null;
  } else {
    const raw = exact_object_bang(hard_floor, ["capabilityFloor", "reasoning"], ["capabilityFloor", "reasoning"], "learning hard floor");
    require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "capabilityFloor"), CAPABILITY_FLOORS, "hard floor capability floor");
    require_member_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "reasoning"), REASONING_LEVELS, "hard floor reasoning");
    return raw;
  }
}

function assign_learning_episode_bang(policy_value, input) {
  const policy = validate_learning_policy_bang(policy_value);
  const raw_input = input;
  const baseline = validate_baseline_bang(raw_input.baseline);
  const __hard_floor = validate_hard_floor_bang(raw_input.hardFloor);
  require_identifier_bang(raw_input.episodeId, "learning episode id");
  if ((!((_truthy) => _truthy !== false && _truthy != null)(SHA256.test(raw_input.taskSignatureSha256)))) {
    error_bang("task signature must be a SHA-256 digest");
  }
  const policy_sha = sha256_manifest(policy);
  const options = eligible_options_bang(policy, raw_input);
  const eligible_axes = $$bc$eager_seq($$bh$js_keys(options).map((key) => key));
  const risk = raw_input.risk;
  const risk_eligible = (((_truthy) => _truthy !== false && _truthy != null)(risk) ? (RISK_ORDER.indexOf(risk) <= RISK_ORDER.indexOf(policy.riskCeiling)) : false);
  const key = $$bc$str(policy_sha, ":", policy.seed, ":", policy.epoch, ":", raw_input.episodeId);
  const explore = ((policy.mode === "learning") && (risk_eligible && ((eligible_axes.length > 0) && (unit_interval($$bc$str(key, ":explore")) < policy.intensity))));
  const axis = (explore ? select_arm(eligible_axes, unit_interval($$bc$str(key, ":axis"))) : "control");
  const arms = ((axis === "control") ? [] : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(options, axis));
  const arm_id = ((axis === "control") ? "control" : select_arm(arms, unit_interval($$bc$str(key, ":arm:", axis))));
  const explore_propensity = (((policy.mode === "learning") && (risk_eligible && (eligible_axes.length > 0))) ? policy.intensity : 0);
  const axis_propensity = ((axis === "control") ? 1 : (1 / eligible_axes.length));
  const arm_propensity = ((axis === "control") ? 1 : (1 / arms.length));
  const assigned_propensity = ((axis === "control") ? (1 - explore_propensity) : (explore_propensity * axis_propensity * arm_propensity));
  const base = $$bh$js_obj("version", LEARNING__ASSIGNMENT__VERSION, "policyVersion", LEARNING__POLICY__VERSION, "policySha256", policy_sha, "mode", policy.mode, "evidenceMode", policy.evidenceMode, "experimentId", (((_truthy) => _truthy !== false && _truthy != null)(raw_input.experimentId) ? require_identifier_bang(raw_input.experimentId, "learning experiment id") : $$bc$str("exp-", policy_sha.slice(0, 16))), "episodeId", raw_input.episodeId, "taskSignatureSha256", raw_input.taskSignatureSha256, "taskSignatureCoverage", raw_input.taskSignatureCoverage, "risk", ((_logical) => (_logical !== false && _logical != null ? _logical : "unknown"))(risk), "arm", ((axis === "control") ? "control" : "explore"), "axis", axis, "armId", arm_id, "baseline", Object.freeze(Object.assign($$bh$js_obj(), baseline)), "options", options, "propensity", Object.freeze($$bh$js_obj("assigned", assigned_propensity, "explore", explore_propensity, "axis", axis_propensity, "arm", arm_propensity)), "narrowingReason", ((axis === "control") ? control_reason(policy, raw_input, eligible_axes) : $$bc$str("explore:", axis, ":", arm_id)));
  return Object.freeze(Object.assign($$bh$js_obj(), base, $$bh$js_obj("manifestSha256", sha256_manifest(base))));
}

const assignLearningEpisode = assign_learning_episode_bang;

function learning_fact(predicate, value) {
  const result = [predicate, value];
  return result;
}

function learning_assignment_facts(assignment) {
  const propensity = assignment.propensity;
  return [learning_fact("learning_assignment_version", assignment.version), learning_fact("learning_policy_version", assignment.policyVersion), learning_fact("learning_policy_sha256", assignment.policySha256), learning_fact("learning_mode", assignment.mode), learning_fact("learning_evidence_mode", assignment.evidenceMode), learning_fact("learning_experiment_id", assignment.experimentId), learning_fact("learning_episode_id", assignment.episodeId), learning_fact("learning_task_signature_sha256", assignment.taskSignatureSha256), learning_fact("learning_task_signature_coverage", assignment.taskSignatureCoverage), learning_fact("learning_risk", assignment.risk), learning_fact("learning_arm", assignment.arm), learning_fact("learning_axis", assignment.axis), learning_fact("learning_arm_id", assignment.armId), learning_fact("learning_propensity", propensity.assigned.toFixed(12)), learning_fact("learning_explore_propensity", propensity.explore.toFixed(12)), learning_fact("learning_narrowing_reason", assignment.narrowingReason), learning_fact("learning_baseline_sha256", sha256_bytes(canonical_receipt_json(assignment.baseline))), learning_fact("learning_options_sha256", sha256_bytes(canonical_receipt_json(assignment.options))), learning_fact("learning_assignment_sha256", assignment.manifestSha256)];
}

const learningAssignmentFacts = learning_assignment_facts;

export { DEFAULT__LEARNING__POLICY as "DEFAULT_LEARNING_POLICY" };
export { LEARNING__ASSIGNMENT__VERSION as "LEARNING_ASSIGNMENT_VERSION" };
export { LEARNING__AXES as "LEARNING_AXES" };
export { LEARNING__POLICY__VERSION as "LEARNING_POLICY_VERSION" };
export { assignLearningEpisode as "assignLearningEpisode" };
export { learningAssignmentFacts as "learningAssignmentFacts" };
export { learningPolicyPath as "learningPolicyPath" };
export { learningPolicySha256 as "learningPolicySha256" };
export { loadLearningPolicy as "loadLearningPolicy" };
export { validateLearningPolicy as "validateLearningPolicy" };
