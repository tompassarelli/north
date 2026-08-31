import { count as $$bc$count, distinct_equivV as $$bc$distinct_equiv, eager_seq as $$bc$eager_seq, into_value as $$bc$into_value, range as $$bc$range, record_value as $$bc$record_value, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_array as $$bh$admit_host_array, admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, alength as $$bh$alength, aset as $$bh$aset, into_array as $$bh$into_array, js_keys as $$bh$js_keys, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';

function RegExpV1(test) {
  return $$bc$record_value("north.sdk.routing-metadata/RegExpV1", {_tag: "RegExpV1", test});
}

function regexpv1_test(r) { return r.test; }

function JsonV1(parse) {
  return $$bc$record_value("north.sdk.routing-metadata/JsonV1", {_tag: "JsonV1", parse});
}

function jsonv1_parse(r) { return r.parse; }

function ProcessV1(env) {
  return $$bc$record_value("north.sdk.routing-metadata/ProcessV1", {_tag: "ProcessV1", env});
}

function processv1_env(r) { return r.env; }

const TASK__GRADES = ["novice", "junior", "mid", "senior", "staff", "principal", "distinguished"];

const TOPOLOGIES = ["worker", "orchestrator"];

const COMPOSITION__KINDS = ["template", "bespoke"];

const CAPABILITY__FLOORS = ["baseline", "standard", "advanced", "frontier"];

const SERVICE__CLASSES = ["economy", "fast", "balanced", "premium"];

const REASONING__LEVELS = ["low", "medium", "high", "xhigh", "max"];

const POSTURES = ["explore", "evaluate", "deliver", "preserve", "prune"];

const ROUTING__OVERRIDE__FIELDS = ["taskGrade", "domainRequirements", "capabilityFloor", "serviceClass", "reasoning", "posture"];

const ORCHESTRATION__CAPABILITIES = ["filesystem.read", "filesystem.search", "filesystem.write", "shell", "shell.readonly", "web", "coordination"];

function BespokeContractRecord(responsibility, deliverable, capabilities, mayDecide, mustEscalate, doneWhen, report) {
  return $$bc$record_value("north.sdk.routing-metadata/BespokeContractRecord", {_tag: "BespokeContractRecord", responsibility, deliverable, capabilities, mayDecide, mustEscalate, doneWhen, report});
}

function bespokecontractrecord_responsibility(r) { return r.responsibility; }

function bespokecontractrecord_deliverable(r) { return r.deliverable; }

function bespokecontractrecord_capabilities(r) { return r.capabilities; }

function bespokecontractrecord_mayDecide(r) { return r.mayDecide; }

function bespokecontractrecord_mustEscalate(r) { return r.mustEscalate; }

function bespokecontractrecord_doneWhen(r) { return r.doneWhen; }

function bespokecontractrecord_report(r) { return r.report; }

function TemplateCompositionRecord(kind, id, overrides, overrideReason) {
  return $$bc$record_value("north.sdk.routing-metadata/TemplateCompositionRecord", {_tag: "TemplateCompositionRecord", kind, id, overrides, overrideReason});
}

function templatecompositionrecord_kind(r) { return r.kind; }

function templatecompositionrecord_id(r) { return r.id; }

function templatecompositionrecord_overrides(r) { return r.overrides; }

function templatecompositionrecord_overrideReason(r) { return r.overrideReason; }

function BespokeCompositionRecord(kind, id, nearestTemplate, bespokeReason, promotionCandidate, contract) {
  return $$bc$record_value("north.sdk.routing-metadata/BespokeCompositionRecord", {_tag: "BespokeCompositionRecord", kind, id, nearestTemplate, bespokeReason, promotionCandidate, contract});
}

function bespokecompositionrecord_kind(r) { return r.kind; }

function bespokecompositionrecord_id(r) { return r.id; }

function bespokecompositionrecord_nearestTemplate(r) { return r.nearestTemplate; }

function bespokecompositionrecord_bespokeReason(r) { return r.bespokeReason; }

function bespokecompositionrecord_promotionCandidate(r) { return r.promotionCandidate; }

function bespokecompositionrecord_contract(r) { return r.contract; }

// AgentCompositionRecord = TemplateCompositionRecord | BespokeCompositionRecord

function RoutingDraftRecord(role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition) {
  return $$bc$record_value("north.sdk.routing-metadata/RoutingDraftRecord", {_tag: "RoutingDraftRecord", role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition});
}

function routingdraftrecord_role(r) { return r.role; }

function routingdraftrecord_taskGrade(r) { return r.taskGrade; }

function routingdraftrecord_domainRequirements(r) { return r.domainRequirements; }

function routingdraftrecord_topology(r) { return r.topology; }

function routingdraftrecord_capabilityFloor(r) { return r.capabilityFloor; }

function routingdraftrecord_serviceClass(r) { return r.serviceClass; }

function routingdraftrecord_reasoning(r) { return r.reasoning; }

function routingdraftrecord_posture(r) { return r.posture; }

function routingdraftrecord_composition(r) { return r.composition; }

function RoutingRequestRecord(role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition) {
  return $$bc$record_value("north.sdk.routing-metadata/RoutingRequestRecord", {_tag: "RoutingRequestRecord", role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition});
}

function routingrequestrecord_role(r) { return r.role; }

function routingrequestrecord_taskGrade(r) { return r.taskGrade; }

function routingrequestrecord_domainRequirements(r) { return r.domainRequirements; }

function routingrequestrecord_topology(r) { return r.topology; }

function routingrequestrecord_capabilityFloor(r) { return r.capabilityFloor; }

function routingrequestrecord_serviceClass(r) { return r.serviceClass; }

function routingrequestrecord_reasoning(r) { return r.reasoning; }

function routingrequestrecord_posture(r) { return r.posture; }

function routingrequestrecord_composition(r) { return r.composition; }

function error_bang(message) {
  return (() => { throw new Error(message); })();
}

function required_string_bang(value, label) {
  if (((_truthy) => _truthy !== false && _truthy != null)(value)) {
    return value;
  } else {
    error_bang($$bc$str(label, " is required"));
    return "";
  }
}

function required_strings_bang(value, label) {
  if (((_truthy) => _truthy !== false && _truthy != null)(value)) {
    return value;
  } else {
    error_bang($$bc$str(label, " is required"));
    return [];
  }
}

function required_composition_bang(value, label) {
  if (((_truthy) => _truthy !== false && _truthy != null)(value)) {
    return value;
  } else {
    error_bang($$bc$str(label, " is required"));
    return TemplateCompositionRecord("template", "unreachable", [], null);
  }
}

function object_keys(value) {
  (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "__beagle_object_admission__");
  const raw = $$bh$js_keys(value);
  return $$bc$eager_seq($$bc$range($$bh$alength(raw)).map((index) => (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw, index)));
}

function exact_object_bang(value, allowed, required, label) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((value == null) || ((!(typeof value === "object")) || Array.isArray(value))))) {
    error_bang($$bc$str(label, " must be an object"));
  }
  const actual = object_keys(value);
  const unknown = actual.filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed.includes(key))));
  const missing = required.filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(actual.includes(key))));
  if ((!($$bc$count(unknown) === 0))) {
    error_bang($$bc$str(label, " has unknown field(s): ", unknown.join(", ")));
  }
  if ((!($$bc$count(missing) === 0))) {
    error_bang($$bc$str(label, " is missing field(s): ", missing.join(", ")));
  }
  return value;
}

function member_bang(values, value, field) {
  return ((((value == null) || (value === ""))) ? null : ((!(typeof value === "string"))) ? error_bang($$bc$str(field, " must be one of: ", values.join(", "))) : ((!((_truthy) => _truthy !== false && _truthy != null)(values.includes($$bc$str(value))))) ? error_bang($$bc$str(field, " must be one of: ", values.join(", "))) : $$bc$str(value));
}

function non_empty_string_bang(value, field) {
  if (((!(typeof value === "string")) || ($$bc$str(value).trim() === ""))) {
    error_bang($$bc$str(field, " must be a non-empty string"));
  }
  return $$bc$str(value).trim();
}

function raw_strings_bang(value, field, require_items) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(Array.isArray(value)))) {
    error_bang($$bc$str(field, " must be ", (require_items ? "a non-empty" : "an"), " array of non-empty strings"));
  }
  const raw = $$bh$into_array(value);
  if ((require_items && ($$bh$alength(raw) === 0))) {
    error_bang($$bc$str(field, " must be a non-empty array of non-empty strings"));
  }
  const normalized = $$bc$eager_seq($$bc$range($$bh$alength(raw)).map((index) => non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw, index), field)));
  if ((!($$bc$distinct_equiv(normalized).length === normalized.length))) {
    error_bang($$bc$str(field, " must not contain duplicates"));
  }
  return normalized;
}

const EDGE__ASCII__WHITESPACE = new RegExp("^[\\u0009-\\u000d\\u0020]+|[\\u0009-\\u000d\\u0020]+$", "g");

const WINDOWS__NEWLINES = new RegExp("\\r\\n?", "g");

function canonical_text_bang(value, field) {
  const text = non_empty_string_bang(value, $$bc$str("bespoke contract ", field));
  const normalized = text.replace(WINDOWS__NEWLINES, "\n").normalize("NFC").replace(EDGE__ASCII__WHITESPACE, "");
  if ((normalized === "")) {
    error_bang($$bc$str("bespoke contract ", field, " must be a non-empty string"));
  }
  return normalized;
}

function canonical_text_set_bang(value, field) {
  const raw = raw_strings_bang(value, $$bc$str("bespoke contract ", field), true);
  const normalized = raw.map((entry) => canonical_text_bang(entry, field));
  if ((!($$bc$distinct_equiv(normalized).length === $$bc$count(normalized)))) {
    error_bang($$bc$str("bespoke contract ", field, " must not contain duplicates"));
  }
  return normalized.sort();
}

function canonical_capabilities_bang(value, field) {
  const normalized = raw_strings_bang(value, field, true);
  const unknown = normalized.filter((capability) => (!((_truthy) => _truthy !== false && _truthy != null)(ORCHESTRATION__CAPABILITIES.includes(capability))));
  if ((!($$bc$count(unknown) === 0))) {
    error_bang($$bc$str(field, " contains unknown values: ", unknown.join(", ")));
  }
  return ORCHESTRATION__CAPABILITIES.filter((capability) => normalized.includes(capability));
}

function has_capability_p(capabilities, capability) {
  return capabilities.includes(capability);
}

function validate_topology_capabilities_bang(topology, capabilities, label) {
  const shell = has_capability_p(capabilities, "shell");
  const readonly_shell = has_capability_p(capabilities, "shell.readonly");
  const read = has_capability_p(capabilities, "filesystem.read");
  const search = has_capability_p(capabilities, "filesystem.search");
  const write = has_capability_p(capabilities, "filesystem.write");
  const coordination = has_capability_p(capabilities, "coordination");
  if ((shell && readonly_shell)) {
    error_bang($$bc$str(label, ": shell and shell.readonly are mutually exclusive"));
  }
  if ((topology === "orchestrator")) {
    if ((!coordination)) {
      error_bang($$bc$str(label, ": orchestrator topology requires coordination capability"));
    }
    if (write) {
      error_bang($$bc$str(label, ": orchestrator topology forbids filesystem.write capability"));
    }
    if (shell) {
      error_bang($$bc$str(label, ": orchestrator topology forbids unrestricted shell capability"));
    }
  } else {
    if (coordination) {
      error_bang($$bc$str(label, ": worker topology forbids coordination capability"));
    }
  }
  if ((shell && ((!read) || ((!search) || (!write))))) {
    const missing = ["filesystem.read", "filesystem.search", "filesystem.write"].filter((capability) => (!has_capability_p(capabilities, capability)));
    error_bang($$bc$str(label, ": capability list is not closed; missing implied ", missing.join(", ")));
  }
  if ((readonly_shell && ((!read) || (!search)))) {
    const missing = ["filesystem.read", "filesystem.search"].filter((capability) => (!has_capability_p(capabilities, capability)));
    error_bang($$bc$str(label, ": capability list is not closed; missing implied ", missing.join(", ")));
  }
  return null;
}

function validate_posture_capabilities_bang(posture, capabilities, label) {
  const write = has_capability_p(capabilities, "filesystem.write");
  const shell = has_capability_p(capabilities, "shell");
  if (((posture === "preserve") && (write || shell))) {
    error_bang($$bc$str(label, ": preserve posture requires a non-authoring capability boundary"));
  }
  if (((posture === "prune") && ((!write) || (!shell)))) {
    error_bang($$bc$str(label, ": prune posture requires filesystem.write and shell capabilities"));
  }
  return null;
}

const RETIRED__ROLE__IDS = ["researcher", "research-scientist", "cs-researcher"];

const ROLE__ID = new RegExp("^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$", "u");

function require_role_id_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const value = $beagle$args[0];
    return require_role_id_bang(value, "role");
  }
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    if (((_truthy) => _truthy !== false && _truthy != null)(((typeof value === "string") && RETIRED__ROLE__IDS.includes($$bc$str(value))))) {
      error_bang($$bc$str("role ", $$bc$str(value), " is retired; use scout, analyst, or scientist"));
    }
    if (((!(typeof value === "string")) || (!ROLE__ID.test($$bc$str(value))))) {
      error_bang($$bc$str(label, " must be a lowercase kebab-case Orchestration role id"));
    }
    return $$bc$str(value);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const CONTRACT__FIELDS = ["responsibility", "deliverable", "capabilities", "mayDecide", "mustEscalate", "doneWhen", "report"];

function canonical_bespoke_contract_bang(value) {
  const raw = exact_object_bang(value, CONTRACT__FIELDS, CONTRACT__FIELDS, "composition.contract");
  return BespokeContractRecord(canonical_text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "responsibility"), "responsibility"), canonical_text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "deliverable"), "deliverable"), canonical_capabilities_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "capabilities"), "bespoke contract capabilities"), canonical_text_set_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "mayDecide"), "mayDecide"), canonical_text_set_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "mustEscalate"), "mustEscalate"), canonical_text_set_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "doneWhen"), "doneWhen"), canonical_text_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "report"), "report"));
}

const ROUTING__FIELDS = ["role", "taskGrade", "domainRequirements", "topology", "capabilityFloor", "serviceClass", "reasoning", "posture", "composition"];

const TEMPLATE__FIELDS = ["kind", "id", "overrides", "overrideReason"];

const BESPOKE__FIELDS = ["kind", "id", "nearestTemplate", "bespokeReason", "promotionCandidate", "contract"];

function contract_object(value) {
  return $$bh$js_obj("responsibility", value.responsibility, "deliverable", value.deliverable, "capabilities", value.capabilities, "mayDecide", value.mayDecide, "mustEscalate", value.mustEscalate, "doneWhen", value.doneWhen, "report", value.report);
}

function composition_object(value) {
  return (() => { const _match_0 = value; if (_match_0._tag === "TemplateCompositionRecord") { const kind = _match_0.kind; const id = _match_0.id; const overrides = _match_0.overrides; const override_reason = _match_0.overrideReason; return (() => { const object = $$bh$js_obj("kind", kind, "id", id, "overrides", overrides); if (((_truthy) => _truthy !== false && _truthy != null)(override_reason)) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "overrideReason", override_reason);
}
return object; })(); } else if (_match_0._tag === "BespokeCompositionRecord") { const kind = _match_0.kind; const id = _match_0.id; const nearest = _match_0.nearestTemplate; const bespoke_reason = _match_0.bespokeReason; const promotion = _match_0.promotionCandidate; const contract = _match_0.contract; return (() => { const object = $$bh$js_obj("kind", kind, "id", id, "bespokeReason", bespoke_reason, "promotionCandidate", promotion, "contract", contract_object(contract)); if (((_truthy) => _truthy !== false && _truthy != null)(nearest)) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "nearestTemplate", nearest);
}
return object; })(); } else { return null; } })();
}

function draft_object(value) {
  const object = $$bh$js_obj();
  if (((_truthy) => _truthy !== false && _truthy != null)(value.role)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "role", value.role);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.taskGrade)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "taskGrade", value.taskGrade);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.domainRequirements)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "domainRequirements", value.domainRequirements);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.topology)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "topology", value.topology);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.capabilityFloor)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "capabilityFloor", value.capabilityFloor);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.serviceClass)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "serviceClass", value.serviceClass);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.reasoning)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "reasoning", value.reasoning);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(value.posture)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "posture", value.posture);
  }
  const composition = value.composition;
  if (((_truthy) => _truthy !== false && _truthy != null)(composition)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "composition", composition_object(composition));
  }
  return object;
}

function request_object(value) {
  return $$bh$js_obj("role", value.role, "taskGrade", value.taskGrade, "domainRequirements", value.domainRequirements, "topology", value.topology, "capabilityFloor", value.capabilityFloor, "serviceClass", value.serviceClass, "reasoning", value.reasoning, "posture", value.posture, "composition", composition_object(value.composition));
}

function decode_composition_bang(value, role, task_grade, domain_requirements, topology, capability_floor, service_class, reasoning, posture) {
  const kind = member_bang(COMPOSITION__KINDS, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "kind"), "composition.kind");
  if ((kind == null)) {
    error_bang("composition.kind is required");
  }
  if ((role == null)) {
    error_bang("composition requires role");
  }
  const composition_id = require_role_id_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "id"), "composition.id");
  if ((kind === "template")) {
    const raw = exact_object_bang(value, TEMPLATE__FIELDS, ["kind", "id", "overrides"], "composition");
    const overrides = raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "overrides"), "composition.overrides", false);
    const invalid = overrides.filter((field) => (!((_truthy) => _truthy !== false && _truthy != null)(ROUTING__OVERRIDE__FIELDS.includes(field))));
    const raw_reason = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "overrideReason");
    const reason = ((overrides.length === 0) ? (() => { if ((!(raw_reason == null))) {
  error_bang("unchanged template must omit composition.overrideReason");
}
return null; })() : non_empty_string_bang(raw_reason, "composition.overrideReason"));
    if ((!($$bc$count(invalid) === 0))) {
      error_bang($$bc$str("composition.overrides may contain only: ", ROUTING__OVERRIDE__FIELDS.join(", ")));
    }
    return TemplateCompositionRecord("template", composition_id, overrides, reason);
  } else {
    const raw = exact_object_bang(value, BESPOKE__FIELDS, ["kind", "id", "bespokeReason", "promotionCandidate", "contract"], "composition");
    const nearest = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "nearestTemplate") == null) ? null : require_role_id_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "nearestTemplate"), "composition.nearestTemplate"));
    const bespoke_reason = non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "bespokeReason"), "composition.bespokeReason");
    const promotion = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "promotionCandidate");
    const contract = canonical_bespoke_contract_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "contract"));
    const missing = $$bc$into_value([], [].concat(((task_grade == null) ? ["taskGrade"] : []), ((domain_requirements == null) ? ["domainRequirements"] : []), ((topology == null) ? ["topology"] : []), ((capability_floor == null) ? ["capabilityFloor"] : []), ((service_class == null) ? ["serviceClass"] : []), ((reasoning == null) ? ["reasoning"] : []), ((posture == null) ? ["posture"] : [])));
    if ((!(typeof promotion === "boolean"))) {
      error_bang("composition.promotionCandidate must be boolean");
    }
    if ((!(missing.length === 0))) {
      error_bang($$bc$str("bespoke composition requires all routing axes; missing: ", missing.join(", ")));
    }
    validate_topology_capabilities_bang(required_string_bang(topology, "topology"), contract.capabilities, "composition.contract.capabilities");
    validate_posture_capabilities_bang(required_string_bang(posture, "posture"), contract.capabilities, "composition.contract.capabilities");
    return BespokeCompositionRecord("bespoke", composition_id, nearest, bespoke_reason, ((promotion === true) ? true : false), contract);
  }
}

function canonicalRole_bang(role) {
  return ((role == null) ? null : require_role_id_bang(role));
}

const canonicalRole = canonicalRole_bang;

function decode_routing_metadata_bang(value) {
  const raw = exact_object_bang(value, ROUTING__FIELDS, [], "routing metadata");
  const role = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "role") == null) ? null : require_role_id_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "role")));
  const task_grade = member_bang(TASK__GRADES, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "taskGrade"), "taskGrade");
  const topology = member_bang(TOPOLOGIES, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "topology"), "topology");
  const capability_floor = member_bang(CAPABILITY__FLOORS, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "capabilityFloor"), "capabilityFloor");
  const service_class = member_bang(SERVICE__CLASSES, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "serviceClass"), "serviceClass");
  const reasoning = member_bang(REASONING__LEVELS, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "reasoning"), "reasoning");
  const posture = member_bang(POSTURES, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "posture"), "posture");
  const domain_requirements = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "domainRequirements") == null) ? null : raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "domainRequirements"), "domainRequirements", false));
  const composition = (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "composition") == null) ? null : decode_composition_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "composition"), role, task_grade, domain_requirements, topology, capability_floor, service_class, reasoning, posture));
  return RoutingDraftRecord(role, task_grade, domain_requirements, topology, capability_floor, service_class, reasoning, posture, composition);
}

function validateRoutingMetadata_bang(value) {
  return draft_object(decode_routing_metadata_bang(value));
}

const validateRoutingMetadata = validateRoutingMetadata_bang;

const ROUTING__REQUEST__FIELDS = ["role", "taskGrade", "domainRequirements", "topology", "capabilityFloor", "serviceClass", "reasoning", "posture", "composition"];

function parse_complete_routing_request_record_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const value = $beagle$args[0];
    return parse_complete_routing_request_record_bang(value, "managed North agent");
  }
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const surface = $beagle$args[1];
    const normalized = decode_routing_metadata_bang(value);
    const missing = ROUTING__REQUEST__FIELDS.filter((field) => ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(draft_object(normalized), field) == null));
    if ((!($$bc$count(missing) === 0))) {
      error_bang($$bc$str(surface, " requires the complete nine-field Agent Machinery run request; missing: ", missing.join(", "), " (recover the valid payload shape: north show @contract:dispatch)"));
    }
    return RoutingRequestRecord(required_string_bang(normalized.role, "role"), required_string_bang(normalized.taskGrade, "taskGrade"), required_strings_bang(normalized.domainRequirements, "domainRequirements"), required_string_bang(normalized.topology, "topology"), required_string_bang(normalized.capabilityFloor, "capabilityFloor"), required_string_bang(normalized.serviceClass, "serviceClass"), required_string_bang(normalized.reasoning, "reasoning"), required_string_bang(normalized.posture, "posture"), required_composition_bang(normalized.composition, "composition"));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function parseCompleteRoutingRequest_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const value = $beagle$args[0];
    return request_object(parse_complete_routing_request_record_bang(value));
  }
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const surface = $beagle$args[1];
    return request_object(parse_complete_routing_request_record_bang(value, surface));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const parseCompleteRoutingRequest = parseCompleteRoutingRequest_bang;

function routingMetadataFromEnv_bang() {
  const env = process.env;
  const raw_composition = env.AGENT_COMPOSITION;
  const raw = $$bh$js_obj("role", env.AGENT_ROLE, "taskGrade", env.AGENT_TASK_GRADE, "domainRequirements", (() => { const encoded = env.AGENT_DOMAIN_REQUIREMENTS; return (((_truthy) => _truthy !== false && _truthy != null)(encoded) ? JSON.parse(encoded) : null); })(), "topology", env.AGENT_TOPOLOGY, "capabilityFloor", env.AGENT_CAPABILITY_FLOOR, "serviceClass", env.AGENT_SERVICE_CLASS, "reasoning", env.AGENT_REASONING, "posture", env.AGENT_POSTURE, "composition", (((_truthy) => _truthy !== false && _truthy != null)(raw_composition) ? JSON.parse(raw_composition) : null));
  return validateRoutingMetadata_bang(raw);
}

const routingMetadataFromEnv = routingMetadataFromEnv_bang;

export { CAPABILITY__FLOORS as "CAPABILITY_FLOORS" };
export { COMPOSITION__KINDS as "COMPOSITION_KINDS" };
export { POSTURES as "POSTURES" };
export { REASONING__LEVELS as "REASONING_LEVELS" };
export { ROUTING__OVERRIDE__FIELDS as "ROUTING_OVERRIDE_FIELDS" };
export { ROUTING__REQUEST__FIELDS as "ROUTING_REQUEST_FIELDS" };
export { SERVICE__CLASSES as "SERVICE_CLASSES" };
export { TASK__GRADES as "TASK_GRADES" };
export { TOPOLOGIES as "TOPOLOGIES" };
export { canonicalRole as "canonicalRole" };
export { parseCompleteRoutingRequest as "parseCompleteRoutingRequest" };
export { routingMetadataFromEnv as "routingMetadataFromEnv" };
export { validateRoutingMetadata as "validateRoutingMetadata" };
