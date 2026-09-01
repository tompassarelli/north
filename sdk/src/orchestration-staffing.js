import * as routing from "./routing-metadata.js";
import { count as $$bc$count, distinct_equivV as $$bc$distinct_equiv, eager_seq as $$bc$eager_seq, get as $$bc$get, into_value as $$bc$into_value, range as $$bc$range, record_value as $$bc$record_value, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_array as $$bh$admit_host_array, admit_host_object as $$bh$admit_host_object, aget as $$bh$aget, alength as $$bh$alength, aset as $$bh$aset, into_array as $$bh$into_array, js_keys as $$bh$js_keys, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

function BespokeContractV3(responsibility, deliverable, capabilities, mayDecide, mustEscalate, doneWhen, report) {
  return $$bc$record_value("north.sdk.orchestration-staffing/BespokeContractV3", {_tag: "BespokeContractV3", responsibility, deliverable, capabilities, mayDecide, mustEscalate, doneWhen, report});
}

function bespokecontractv3_responsibility(r) { return r.responsibility; }

function bespokecontractv3_deliverable(r) { return r.deliverable; }

function bespokecontractv3_capabilities(r) { return r.capabilities; }

function bespokecontractv3_mayDecide(r) { return r.mayDecide; }

function bespokecontractv3_mustEscalate(r) { return r.mustEscalate; }

function bespokecontractv3_doneWhen(r) { return r.doneWhen; }

function bespokecontractv3_report(r) { return r.report; }

function TemplateCompositionV3(kind, id, overrides, overrideReason) {
  return $$bc$record_value("north.sdk.orchestration-staffing/TemplateCompositionV3", {_tag: "TemplateCompositionV3", kind, id, overrides, overrideReason});
}

function templatecompositionv3_kind(r) { return r.kind; }

function templatecompositionv3_id(r) { return r.id; }

function templatecompositionv3_overrides(r) { return r.overrides; }

function templatecompositionv3_overrideReason(r) { return r.overrideReason; }

function BespokeCompositionV3(kind, id, nearestTemplate, bespokeReason, promotionCandidate, contract) {
  return $$bc$record_value("north.sdk.orchestration-staffing/BespokeCompositionV3", {_tag: "BespokeCompositionV3", kind, id, nearestTemplate, bespokeReason, promotionCandidate, contract});
}

function bespokecompositionv3_kind(r) { return r.kind; }

function bespokecompositionv3_id(r) { return r.id; }

function bespokecompositionv3_nearestTemplate(r) { return r.nearestTemplate; }

function bespokecompositionv3_bespokeReason(r) { return r.bespokeReason; }

function bespokecompositionv3_promotionCandidate(r) { return r.promotionCandidate; }

function bespokecompositionv3_contract(r) { return r.contract; }

// AgentCompositionV3 = TemplateCompositionV3 | BespokeCompositionV3

function RoutingDraftV3(role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition) {
  return $$bc$record_value("north.sdk.orchestration-staffing/RoutingDraftV3", {_tag: "RoutingDraftV3", role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition});
}

function routingdraftv3_role(r) { return r.role; }

function routingdraftv3_taskGrade(r) { return r.taskGrade; }

function routingdraftv3_domainRequirements(r) { return r.domainRequirements; }

function routingdraftv3_topology(r) { return r.topology; }

function routingdraftv3_capabilityFloor(r) { return r.capabilityFloor; }

function routingdraftv3_serviceClass(r) { return r.serviceClass; }

function routingdraftv3_reasoning(r) { return r.reasoning; }

function routingdraftv3_posture(r) { return r.posture; }

function routingdraftv3_composition(r) { return r.composition; }

function RoutingRequestV3(role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition) {
  return $$bc$record_value("north.sdk.orchestration-staffing/RoutingRequestV3", {_tag: "RoutingRequestV3", role, taskGrade, domainRequirements, topology, capabilityFloor, serviceClass, reasoning, posture, composition});
}

function routingrequestv3_role(r) { return r.role; }

function routingrequestv3_taskGrade(r) { return r.taskGrade; }

function routingrequestv3_domainRequirements(r) { return r.domainRequirements; }

function routingrequestv3_topology(r) { return r.topology; }

function routingrequestv3_capabilityFloor(r) { return r.capabilityFloor; }

function routingrequestv3_serviceClass(r) { return r.serviceClass; }

function routingrequestv3_reasoning(r) { return r.reasoning; }

function routingrequestv3_posture(r) { return r.posture; }

function routingrequestv3_composition(r) { return r.composition; }

function FsModuleV1(readFileSync) {
  return $$bc$record_value("north.sdk.orchestration-staffing/FsModuleV1", {_tag: "FsModuleV1", readFileSync});
}

function fsmodulev1_readFileSync(r) { return r.readFileSync; }

function PathModuleV1(resolve2, resolve4) {
  return $$bc$record_value("north.sdk.orchestration-staffing/PathModuleV1", {_tag: "PathModuleV1", resolve2, resolve4});
}

function pathmodulev1_resolve2(r) { return r.resolve2; }

function pathmodulev1_resolve4(r) { return r.resolve4; }

function CapabilityModuleV3(orchestrationCapabilities, requireOrchestrationCapabilities, validatePostureCapabilities, validateTopologyCapabilities) {
  return $$bc$record_value("north.sdk.orchestration-staffing/CapabilityModuleV3", {_tag: "CapabilityModuleV3", orchestrationCapabilities, requireOrchestrationCapabilities, validatePostureCapabilities, validateTopologyCapabilities});
}

function capabilitymodulev3_orchestrationCapabilities(r) { return r.orchestrationCapabilities; }

function capabilitymodulev3_requireOrchestrationCapabilities(r) { return r.requireOrchestrationCapabilities; }

function capabilitymodulev3_validatePostureCapabilities(r) { return r.validatePostureCapabilities; }

function capabilitymodulev3_validateTopologyCapabilities(r) { return r.validateTopologyCapabilities; }

function RoleModuleV3(requireOrchestrationRoleId1, requireOrchestrationRoleId2) {
  return $$bc$record_value("north.sdk.orchestration-staffing/RoleModuleV3", {_tag: "RoleModuleV3", requireOrchestrationRoleId1, requireOrchestrationRoleId2});
}

function rolemodulev3_requireOrchestrationRoleId1(r) { return r.requireOrchestrationRoleId1; }

function rolemodulev3_requireOrchestrationRoleId2(r) { return r.requireOrchestrationRoleId2; }

function GraphModuleV3(projectStaffingCatalog, staffingSource, warnGraphCatalogFallback) {
  return $$bc$record_value("north.sdk.orchestration-staffing/GraphModuleV3", {_tag: "GraphModuleV3", projectStaffingCatalog, staffingSource, warnGraphCatalogFallback});
}

function graphmodulev3_projectStaffingCatalog(r) { return r.projectStaffingCatalog; }

function graphmodulev3_staffingSource(r) { return r.staffingSource; }

function graphmodulev3_warnGraphCatalogFallback(r) { return r.warnGraphCatalogFallback; }

function JsonV1(parse, stringify) {
  return $$bc$record_value("north.sdk.orchestration-staffing/JsonV1", {_tag: "JsonV1", parse, stringify});
}

function jsonv1_parse(r) { return r.parse; }

function jsonv1_stringify(r) { return r.stringify; }

function ProcessV1(env) {
  return $$bc$record_value("north.sdk.orchestration-staffing/ProcessV1", {_tag: "ProcessV1", env});
}

function processv1_env(r) { return r.env; }

const FS = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", require("node:fs")), "value");

const PATH = (() => { const module = require("node:path"); return PathModuleV1((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "resolve"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "resolve")); })();

const CAPABILITIES = (() => { const module = require("./orchestration-capabilities"); return CapabilityModuleV3((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "ORCHESTRATION_CAPABILITIES"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "requireOrchestrationCapabilities"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "validatePostureCapabilities"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "validateTopologyCapabilities")); })();

const ROLE = (() => { const module = require("./orchestration-role-id"); return RoleModuleV3((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "requireOrchestrationRoleId"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "requireOrchestrationRoleId")); })();

const GRAPH = (() => { const module = require("./orchestration-graph-source"); return GraphModuleV3((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "projectStaffingCatalog"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "staffingSource"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(module, "warnGraphCatalogFallback")); })();

const CAPABILITY__FLOORS = routing["CAPABILITY_FLOORS"];

const POSTURES = routing["POSTURES"];

const REASONING__LEVELS = routing["REASONING_LEVELS"];

const ROUTING__OVERRIDE__FIELDS = routing["ROUTING_OVERRIDE_FIELDS"];

const SERVICE__CLASSES = routing["SERVICE_CLASSES"];

const TASK__GRADES = routing["TASK_GRADES"];

const TOPOLOGIES = routing["TOPOLOGIES"];

const ORCHESTRATION__CAPABILITIES = CAPABILITIES.orchestrationCapabilities;

function readFileSync(path, encoding) {
  return (FS.readFileSync)(path, encoding);
}

function resolve2(left, right) {
  return (PATH.resolve2)(left, right);
}

function resolve4(a, b, c, d) {
  return (PATH.resolve4)(a, b, c, d);
}

function validateRoutingMetadata(value) {
  return routing["validateRoutingMetadata"](value);
}

function parseCompleteRoutingRequest1(value) {
  return routing["parseCompleteRoutingRequest"](value);
}

function parseCompleteRoutingRequest2(value, surface) {
  return routing["parseCompleteRoutingRequest"](value, surface);
}

function requireOrchestrationCapabilities_bang(value, label) {
  return (CAPABILITIES.requireOrchestrationCapabilities)($$bh$into_array(value), label);
}

function validatePostureCapabilities_bang(posture, capabilities, label) {
  return (CAPABILITIES.validatePostureCapabilities)(posture, capabilities, label);
}

function validateTopologyCapabilities_bang(topology, capabilities, label) {
  return (CAPABILITIES.validateTopologyCapabilities)(topology, capabilities, label);
}

function requireOrchestrationRoleId_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const value = $beagle$args[0];
    return (ROLE.requireOrchestrationRoleId1)(value);
  }
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const label = $beagle$args[1];
    return (ROLE.requireOrchestrationRoleId2)(value, label);
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function projectStaffingCatalog() {
  return (GRAPH.projectStaffingCatalog)();
}

function staffingSource() {
  return (GRAPH.staffingSource)();
}

function warnGraphCatalogFallback_bang(label, failure) {
  return (GRAPH.warnGraphCatalogFallback)(label, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))($$bh$js_obj("value", failure), "value"));
}

function StaffingVocabulary(taskGrades, capabilityFloors, serviceClasses, deliberations, topologies, postures, capabilities) {
  return $$bc$record_value("north.sdk.orchestration-staffing/StaffingVocabulary", {_tag: "StaffingVocabulary", taskGrades, capabilityFloors, serviceClasses, deliberations, topologies, postures, capabilities});
}

function staffingvocabulary_taskGrades(r) { return r.taskGrades; }

function staffingvocabulary_capabilityFloors(r) { return r.capabilityFloors; }

function staffingvocabulary_serviceClasses(r) { return r.serviceClasses; }

function staffingvocabulary_deliberations(r) { return r.deliberations; }

function staffingvocabulary_topologies(r) { return r.topologies; }

function staffingvocabulary_postures(r) { return r.postures; }

function staffingvocabulary_capabilities(r) { return r.capabilities; }

function StaffingDefaults(taskGrade, capabilityFloor, serviceClass, deliberation, topology, posture) {
  return $$bc$record_value("north.sdk.orchestration-staffing/StaffingDefaults", {_tag: "StaffingDefaults", taskGrade, capabilityFloor, serviceClass, deliberation, topology, posture});
}

function staffingdefaults_taskGrade(r) { return r.taskGrade; }

function staffingdefaults_capabilityFloor(r) { return r.capabilityFloor; }

function staffingdefaults_serviceClass(r) { return r.serviceClass; }

function staffingdefaults_deliberation(r) { return r.deliberation; }

function staffingdefaults_topology(r) { return r.topology; }

function staffingdefaults_posture(r) { return r.posture; }

function StaffingPreset(name, taskGrade, capabilityFloor, serviceClass, deliberation, topology, posture, capabilities, tagline, description) {
  return $$bc$record_value("north.sdk.orchestration-staffing/StaffingPreset", {_tag: "StaffingPreset", name, taskGrade, capabilityFloor, serviceClass, deliberation, topology, posture, capabilities, tagline, description});
}

function staffingpreset_name(r) { return r.name; }

function staffingpreset_taskGrade(r) { return r.taskGrade; }

function staffingpreset_capabilityFloor(r) { return r.capabilityFloor; }

function staffingpreset_serviceClass(r) { return r.serviceClass; }

function staffingpreset_deliberation(r) { return r.deliberation; }

function staffingpreset_topology(r) { return r.topology; }

function staffingpreset_posture(r) { return r.posture; }

function staffingpreset_capabilities(r) { return r.capabilities; }

function staffingpreset_tagline(r) { return r.tagline; }

function staffingpreset_description(r) { return r.description; }

function StaffingCatalog(sourceVersion, vocabulary, defaults, presets) {
  return $$bc$record_value("north.sdk.orchestration-staffing/StaffingCatalog", {_tag: "StaffingCatalog", sourceVersion, vocabulary, defaults, presets});
}

function staffingcatalog_sourceVersion(r) { return r.sourceVersion; }

function staffingcatalog_vocabulary(r) { return r.vocabulary; }

function staffingcatalog_defaults(r) { return r.defaults; }

function staffingcatalog_presets(r) { return r.presets; }

function CatalogAxis(field, vocabularyField, expected) {
  return $$bc$record_value("north.sdk.orchestration-staffing/CatalogAxis", {_tag: "CatalogAxis", field, vocabularyField, expected});
}

function catalogaxis_field(r) { return r.field; }

function catalogaxis_vocabularyField(r) { return r.vocabularyField; }

function catalogaxis_expected(r) { return r.expected; }

const ORCHESTRATION__STOCK__ROLE__IDS = ["executor", "curator", "implementer", "integrator", "designer", "director", "scout", "analyst", "guardian", "reviewer", "verifier", "judge", "scientist", "team-lead", "program", "portfolio"];

const STOCK__AUTHORING__ROLES = ["executor", "curator", "implementer", "integrator"];

const STOCK__ORCHESTRATOR__ROLES = ["director", "team-lead", "program", "portfolio"];

const CATALOG__AXES = [CatalogAxis("taskGrade", "taskGrades", TASK__GRADES), CatalogAxis("capabilityFloor", "capabilityFloors", CAPABILITY__FLOORS), CatalogAxis("serviceClass", "serviceClasses", SERVICE__CLASSES), CatalogAxis("deliberation", "deliberations", REASONING__LEVELS), CatalogAxis("topology", "topologies", TOPOLOGIES), CatalogAxis("posture", "postures", POSTURES)];

const REPO = resolve2(import.meta.dir, "../..");

const DEFAULT__ORCHESTRATION__STAFFING__PATH = resolve4(REPO, "agent-machinery", "staffing", "catalog.json");

const TOP__LEVEL__FIELDS = ["$schema", "version", "vocabulary", "defaults", "presets"];

const VOCABULARY__FIELDS = ["taskGrades", "capabilityFloors", "serviceClasses", "deliberations", "topologies", "postures", "capabilities"];

const DEFAULT__FIELDS = ["taskGrade", "capabilityFloor", "serviceClass", "deliberation", "topology", "posture"];

const PRESET__FIELDS = ["name", "taskGrade", "capabilityFloor", "serviceClass", "deliberation", "topology", "posture", "capabilities", "tagline", "description"];

function error_bang(message) {
  return (() => { throw new Error(message); })();
}

function object_keys(value) {
  (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "__beagle_object_admission__");
  const raw = $$bh$js_keys(value);
  return $$bc$eager_seq($$bc$range($$bh$alength(raw)).map((index) => (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw, index)));
}

function exact_object_bang(value, allowed, required, label) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((value == null) || ((!(typeof value === "object")) || Array.isArray(value))))) {
    error_bang($$bc$str("staffing catalog: ", label, " must be an object"));
  }
  const actual = object_keys(value);
  const unknown = actual.filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(allowed.includes(key))));
  const missing = required.filter((key) => (!((_truthy) => _truthy !== false && _truthy != null)(actual.includes(key))));
  if ((!($$bc$count(unknown) === 0))) {
    error_bang($$bc$str("staffing catalog: ", label, " has unknown field(s): ", unknown.join(", ")));
  }
  if ((!($$bc$count(missing) === 0))) {
    error_bang($$bc$str("staffing catalog: ", label, " is missing field(s): ", missing.join(", ")));
  }
  return value;
}

function non_empty_string_bang(value, field) {
  if (((!(typeof value === "string")) || ($$bc$str(value).trim() === ""))) {
    error_bang($$bc$str(field, " must be a non-empty string"));
  }
  return $$bc$str(value).trim();
}

function raw_strings_bang(value, label) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(Array.isArray(value)))) {
    error_bang($$bc$str("staffing catalog: ", label, " must contain non-empty strings"));
  }
  const raw = $$bh$into_array(value);
  if (($$bh$alength(raw) === 0)) {
    error_bang($$bc$str("staffing catalog: ", label, " must contain non-empty strings"));
  }
  const values = $$bc$eager_seq($$bc$range($$bh$alength(raw)).map((index) => non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw, index), $$bc$str("staffing catalog: ", label))));
  if ((!($$bc$distinct_equiv(values).length === values.length))) {
    error_bang($$bc$str("staffing catalog: duplicate ", label));
  }
  return values;
}

function sorted_equal_p(left, right) {
  return ($$bc$into_value([], left).sort().join("\u0000") === $$bc$into_value([], right).sort().join("\u0000"));
}

function axis_value_bang(raw, axis, label) {
  const value = non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, axis.field), $$bc$str("staffing catalog: ", label, ".", axis.field));
  if ((!((_truthy) => _truthy !== false && _truthy != null)(axis.expected.includes(value)))) {
    error_bang($$bc$str("staffing catalog: invalid ", label, ".", axis.field));
  }
  return value;
}

function decode_vocabulary_bang(raw, path) {
  const value = exact_object_bang(raw, VOCABULARY__FIELDS, VOCABULARY__FIELDS, "vocabulary");
  CATALOG__AXES.forEach((axis) => {
  const actual = raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, axis.vocabularyField), $$bc$str("vocabulary.", axis.vocabularyField));
  if ((!sorted_equal_p(actual, axis.expected))) {
    error_bang($$bc$str("Agent Machinery wire vocabulary drift at ", path, ": ", axis.vocabularyField));
  }
});
  const capabilities = raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilities"), "vocabulary.capabilities");
  const canonical = requireOrchestrationCapabilities_bang(capabilities, "staffing catalog vocabulary.capabilities");
  if ((!sorted_equal_p(canonical, ORCHESTRATION__CAPABILITIES))) {
    error_bang($$bc$str("Agent Machinery capability vocabulary drift at ", path));
  }
  return StaffingVocabulary(raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "taskGrades"), "vocabulary.taskGrades"), raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilityFloors"), "vocabulary.capabilityFloors"), raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "serviceClasses"), "vocabulary.serviceClasses"), raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "deliberations"), "vocabulary.deliberations"), raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "topologies"), "vocabulary.topologies"), raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "postures"), "vocabulary.postures"), canonical);
}

function decode_defaults_bang(raw) {
  const value = exact_object_bang(raw, DEFAULT__FIELDS, DEFAULT__FIELDS, "defaults");
  return StaffingDefaults(axis_value_bang(value, CATALOG__AXES[0], "defaults"), axis_value_bang(value, CATALOG__AXES[1], "defaults"), axis_value_bang(value, CATALOG__AXES[2], "defaults"), axis_value_bang(value, CATALOG__AXES[3], "defaults"), axis_value_bang(value, CATALOG__AXES[4], "defaults"), axis_value_bang(value, CATALOG__AXES[5], "defaults"));
}

function decode_preset_bang(raw) {
  const name = requireOrchestrationRoleId_bang(non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(raw, "name"), "staffing catalog preset"), "staffing catalog preset");
  const value = exact_object_bang(raw, PRESET__FIELDS, PRESET__FIELDS, $$bc$str("preset ", name));
  const capabilities = requireOrchestrationCapabilities_bang(raw_strings_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilities"), $$bc$str("preset ", name, ".capabilities")), $$bc$str("staffing preset ", name, ".capabilities"));
  const topology = axis_value_bang(value, CATALOG__AXES[4], name);
  const posture = axis_value_bang(value, CATALOG__AXES[5], name);
  validateTopologyCapabilities_bang(topology, capabilities, $$bc$str(name, ".capabilities"));
  validatePostureCapabilities_bang(posture, capabilities, $$bc$str(name, ".capabilities"));
  return StaffingPreset(name, axis_value_bang(value, CATALOG__AXES[0], name), axis_value_bang(value, CATALOG__AXES[1], name), axis_value_bang(value, CATALOG__AXES[2], name), axis_value_bang(value, CATALOG__AXES[3], name), topology, posture, capabilities, non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "tagline"), $$bc$str(name, ": tagline")), non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "description"), $$bc$str(name, ": description")));
}

function decode_presets_bang(value) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(Array.isArray(value)))) {
    error_bang("staffing catalog: presets must be non-empty");
  }
  const raw = $$bh$into_array(value);
  if (($$bh$alength(raw) === 0)) {
    error_bang("staffing catalog: presets must be non-empty");
  }
  const presets = $$bc$eager_seq($$bc$range($$bh$alength(raw)).map((index) => decode_preset_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_array($beagle$host$arg$0), $beagle$host$arg$1))(raw, index))));
  const names = presets.map((preset) => preset.name);
  if ((!($$bc$distinct_equiv(names).length === $$bc$count(names)))) {
    error_bang("duplicate Agent Machinery template");
  }
  return presets;
}

function validate_stock_shape_bang(presets) {
  const names = presets.map((preset) => preset.name);
  const orchestrators = presets.filter((preset) => (preset.topology === "orchestrator")).map((preset) => preset.name);
  if ((!sorted_equal_p(names, ORCHESTRATION__STOCK__ROLE__IDS))) {
    error_bang("Agent Machinery stock template set drift");
  }
  if ((!sorted_equal_p(orchestrators, STOCK__ORCHESTRATOR__ROLES))) {
    error_bang("Agent Machinery stock topology drift: orchestrator topology is the director plus the scope ladder");
  }
  presets.forEach((preset) => {
  const name = preset.name;
  const capabilities = preset.capabilities;
  const read = capabilities.includes("filesystem.read");
  const search = capabilities.includes("filesystem.search");
  const write = capabilities.includes("filesystem.write");
  const shell = capabilities.includes("shell");
  const readonly_shell = capabilities.includes("shell.readonly");
  const coordination = capabilities.includes("coordination");
  if (((!read) || (!search))) {
    error_bang($$bc$str("Agent Machinery stock template ", name, " must retain read and search authority"));
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(STOCK__AUTHORING__ROLES.includes(name))) {
    if (((!write) || (!shell))) {
      error_bang($$bc$str("Agent Machinery stock authoring template ", name, " must retain write and shell authority"));
    }
  } else {
    if ((write || (shell || (!readonly_shell)))) {
      error_bang($$bc$str("Agent Machinery stock nonauthoring template ", name, " must remain read-only"));
    }
  }
  if ((!(STOCK__ORCHESTRATOR__ROLES.includes(name) === coordination))) {
    error_bang("Agent Machinery stock coordination authority belongs to the orchestrator ladder");
  }
});
  return null;
}

function staffing_vocabulary_object(value) {
  return $$bh$js_obj("taskGrades", value.taskGrades, "capabilityFloors", value.capabilityFloors, "serviceClasses", value.serviceClasses, "deliberations", value.deliberations, "topologies", value.topologies, "postures", value.postures, "capabilities", value.capabilities);
}

function staffing_defaults_object(value) {
  return $$bh$js_obj("taskGrade", value.taskGrade, "capabilityFloor", value.capabilityFloor, "serviceClass", value.serviceClass, "deliberation", value.deliberation, "topology", value.topology, "posture", value.posture);
}

function staffing_preset_object(value) {
  return $$bh$js_obj("name", value.name, "taskGrade", value.taskGrade, "capabilityFloor", value.capabilityFloor, "serviceClass", value.serviceClass, "deliberation", value.deliberation, "topology", value.topology, "posture", value.posture, "capabilities", value.capabilities, "tagline", value.tagline, "description", value.description);
}

function staffing_catalog_object(value) {
  return $$bh$js_obj("sourceVersion", value.sourceVersion, "vocabulary", staffing_vocabulary_object(value.vocabulary), "defaults", staffing_defaults_object(value.defaults), "presets", value.presets.map((preset) => staffing_preset_object(preset)));
}

const CATALOG__WIRE__FIELDS = ["sourceVersion", "vocabulary", "defaults", "presets"];

function staffing_catalog_record_bang(raw) {
  const value = exact_object_bang(raw, CATALOG__WIRE__FIELDS, CATALOG__WIRE__FIELDS, "catalog value");
  const version = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "sourceVersion");
  if ((!(version === 3))) {
    error_bang("staffing catalog value: sourceVersion must be 3");
  }
  const vocabulary = decode_vocabulary_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "vocabulary"), "provided staffing catalog");
  const defaults = decode_defaults_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "defaults"));
  const presets = decode_presets_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "presets"));
  validate_stock_shape_bang(presets);
  return StaffingCatalog(3, vocabulary, defaults, presets);
}

function read_file_catalog_object_bang(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function decode_catalog_bang(raw, path) {
  const value = exact_object_bang(raw, TOP__LEVEL__FIELDS, TOP__LEVEL__FIELDS, "top level");
  const schema = non_empty_string_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "$schema"), "staffing catalog: $schema");
  const version = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "version");
  if ((!(schema === "urn:agent-machinery:schema:staffing-catalog:v3"))) {
    error_bang("staffing catalog: $schema must be urn:agent-machinery:schema:staffing-catalog:v3");
  }
  if ((!(version === 3))) {
    error_bang("staffing catalog: version must be 3");
  }
  const vocabulary = decode_vocabulary_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "vocabulary"), path);
  const defaults = decode_defaults_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "defaults"));
  const presets = decode_presets_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "presets"));
  validate_stock_shape_bang(presets);
  return StaffingCatalog(3, vocabulary, defaults, presets);
}

function load_orchestration_staffing_record_bang(...$beagle$args) {
  if (arguments.length === 0) {
    const configured = $$bc$get(process.env, "ORCHESTRATION_STAFFING_CATALOG");
    return load_orchestration_staffing_record_bang((((_truthy) => _truthy !== false && _truthy != null)(configured) ? configured : DEFAULT__ORCHESTRATION__STAFFING__PATH));
  }
  if (arguments.length === 1) {
    const path = $beagle$args[0];
    return ((staffingSource() === "graph") ? (() => { try {
    return decode_catalog_bang(projectStaffingCatalog(), path);
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const failure = _catch_0;
        warnGraphCatalogFallback_bang("staffing catalog", failure);
        return decode_catalog_bang(read_file_catalog_object_bang(path), path);
        break;
      }
    }
  } })() : decode_catalog_bang(read_file_catalog_object_bang(path), path));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function loadOrchestrationStaffing_bang(...$beagle$args) {
  if (arguments.length === 0) {
    return staffing_catalog_object(load_orchestration_staffing_record_bang());
  }
  if (arguments.length === 1) {
    const path = $beagle$args[0];
    return staffing_catalog_object(load_orchestration_staffing_record_bang(path));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const loadOrchestrationStaffing = loadOrchestrationStaffing_bang;

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

function routing_contract_record_bang(value) {
  return BespokeContractV3((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "responsibility"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "deliverable"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilities"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "mayDecide"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "mustEscalate"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "doneWhen"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "report"));
}

function routing_composition_record_bang(value) {
  const kind = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "kind");
  return ((kind === "template") ? TemplateCompositionV3(kind, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "id"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "overrides"), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "overrideReason") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "overrideReason"))) : BespokeCompositionV3(kind, (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "id"), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "nearestTemplate") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "nearestTemplate")), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "bespokeReason"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "promotionCandidate"), routing_contract_record_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "contract"))));
}

function routing_draft_record_bang(value) {
  return RoutingDraftV3((((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "role") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "role")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "taskGrade") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "taskGrade")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "domainRequirements") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "domainRequirements")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "topology") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "topology")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilityFloor") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilityFloor")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "serviceClass") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "serviceClass")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "reasoning") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "reasoning")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "posture") == null) ? null : (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "posture")), (((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "composition") == null) ? null : routing_composition_record_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "composition"))));
}

function routing_request_record_bang(value) {
  return RoutingRequestV3((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "role"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "taskGrade"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "domainRequirements"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "topology"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "capabilityFloor"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "serviceClass"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "reasoning"), (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "posture"), routing_composition_record_bang((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(value, "composition")));
}

function routing_draft_bang(value) {
  return routing_draft_record_bang(validateRoutingMetadata(value));
}

function routing_request_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const value = $beagle$args[0];
    return routing_request_record_bang(parseCompleteRoutingRequest1(value));
  }
  if (arguments.length === 2) {
    const value = $beagle$args[0];
    const surface = $beagle$args[1];
    return routing_request_record_bang(parseCompleteRoutingRequest2(value, surface));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function find_preset(catalog, name) {
  return (() => { const _x = catalog.presets.filter((preset) => (preset.name === name)), _i = 0; return _x[_i] != null ? _x[_i] : null; })();
}

function canonical_staffing_role_record_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const role = $beagle$args[0];
    return canonical_staffing_role_record_bang(role, load_orchestration_staffing_record_bang());
  }
  if (arguments.length === 2) {
    const role = $beagle$args[0];
    const catalog = $beagle$args[1];
    return ((role == null) ? null : requireOrchestrationRoleId_bang(role));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function canonicalStaffingRole_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const role = $beagle$args[0];
    return canonical_staffing_role_record_bang(role);
  }
  if (arguments.length === 2) {
    const role = $beagle$args[0];
    const catalog = $beagle$args[1];
    return canonical_staffing_role_record_bang(role, staffing_catalog_record_bang(catalog));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const canonicalStaffingRole = canonicalStaffingRole_bang;

function routing_contract_object(value) {
  return $$bh$js_obj("responsibility", value.responsibility, "deliverable", value.deliverable, "capabilities", value.capabilities, "mayDecide", value.mayDecide, "mustEscalate", value.mustEscalate, "doneWhen", value.doneWhen, "report", value.report);
}

function composition_object(composition) {
  return (() => { const _match_0 = composition; if (_match_0._tag === "TemplateCompositionV3") { const kind = _match_0.kind; const id = _match_0.id; const overrides = _match_0.overrides; const override_reason = _match_0.overrideReason; return (() => { const object = $$bh$js_obj("kind", kind, "id", id, "overrides", overrides); if (((_truthy) => _truthy !== false && _truthy != null)(override_reason)) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "overrideReason", override_reason);
}
return object; })(); } else if (_match_0._tag === "BespokeCompositionV3") { const kind = _match_0.kind; const id = _match_0.id; const nearest = _match_0.nearestTemplate; const bespoke_reason = _match_0.bespokeReason; const promotion = _match_0.promotionCandidate; const contract = _match_0.contract; return (() => { const object = $$bh$js_obj("kind", kind, "id", id, "bespokeReason", bespoke_reason, "promotionCandidate", promotion, "contract", routing_contract_object(contract)); if (((_truthy) => _truthy !== false && _truthy != null)(nearest)) {
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "nearestTemplate", nearest);
}
return object; })(); } else { return null; } })();
}

function routing_draft_object(metadata) {
  const object = $$bh$js_obj();
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.role)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "role", metadata.role);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.taskGrade)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "taskGrade", metadata.taskGrade);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.domainRequirements)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "domainRequirements", metadata.domainRequirements);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.topology)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "topology", metadata.topology);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.capabilityFloor)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "capabilityFloor", metadata.capabilityFloor);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.serviceClass)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "serviceClass", metadata.serviceClass);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.reasoning)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "reasoning", metadata.reasoning);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(metadata.posture)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "posture", metadata.posture);
  }
  const composition = metadata.composition;
  if (((_truthy) => _truthy !== false && _truthy != null)(composition)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(object, "composition", composition_object(composition));
  }
  return object;
}

function routing_request_object(metadata) {
  return $$bh$js_obj("role", metadata.role, "taskGrade", metadata.taskGrade, "domainRequirements", metadata.domainRequirements, "topology", metadata.topology, "capabilityFloor", metadata.capabilityFloor, "serviceClass", metadata.serviceClass, "reasoning", metadata.reasoning, "posture", metadata.posture, "composition", composition_object(metadata.composition));
}

function orchestration_capabilities_record_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const metadata = $beagle$args[0];
    return orchestration_capabilities_record_bang(metadata, load_orchestration_staffing_record_bang());
  }
  if (arguments.length === 2) {
    const metadata = $beagle$args[0];
    const catalog = $beagle$args[1];
    const request = routing_request_bang(metadata);
    const role = required_string_bang(canonical_staffing_role_record_bang(request.role, catalog), "role");
    const composition = request.composition;
    return (() => { const _match_1 = composition; if (_match_1._tag === "BespokeCompositionV3") { const __kind = _match_1.kind; const __id = _match_1.id; const __nearest = _match_1.nearestTemplate; const __reason = _match_1.bespokeReason; const __promotion = _match_1.promotionCandidate; const contract = _match_1.contract; return requireOrchestrationCapabilities_bang(contract.capabilities, "composition.contract.capabilities"); } else if (_match_1._tag === "TemplateCompositionV3") { const __kind = _match_1.kind; const template_id = _match_1.id; const __overrides = _match_1.overrides; const __reason = _match_1.overrideReason; return (() => { const preset = find_preset(catalog, template_id); if (((_truthy) => _truthy !== false && _truthy != null)(preset)) {
  return $$bc$into_value([], preset.capabilities);
} else {
  error_bang($$bc$str("Agent Machinery stock template ", template_id, " is absent from the catalog for role ", role));
  return [];
} })(); } else { return null; } })();
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function orchestrationCapabilities_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const metadata = $beagle$args[0];
    return orchestration_capabilities_record_bang(metadata);
  }
  if (arguments.length === 2) {
    const metadata = $beagle$args[0];
    const catalog = $beagle$args[1];
    return orchestration_capabilities_record_bang(metadata, staffing_catalog_record_bang(catalog));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const orchestrationCapabilities = orchestrationCapabilities_bang;

function same_route_value_p(left, right) {
  return (JSON.stringify($$bh$js_obj("value", left)) === JSON.stringify($$bh$js_obj("value", right)));
}

function route_field(object, field) {
  return (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(object, field);
}

function template_request_bang(metadata, catalog, role, composition) {
  const composition_object_value = composition;
  const template_id = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(composition_object_value, "kind") === "template") : _logical))(composition_object_value)) ? (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(composition_object_value, "id") : role);
  const preset = find_preset(catalog, template_id);
  if ((preset == null)) {
    error_bang($$bc$str("unknown stock template ", template_id));
  }
  const selected = (((_truthy) => _truthy !== false && _truthy != null)(preset) ? preset : (() => { error_bang("stock template is required");
return catalog.presets[0]; })());
  const base = $$bh$js_obj("taskGrade", selected.taskGrade, "domainRequirements", [], "topology", selected.topology, "capabilityFloor", selected.capabilityFloor, "serviceClass", selected.serviceClass, "reasoning", selected.deliberation, "posture", selected.posture);
  const metadata_object = routing_draft_object(metadata);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(metadata.topology === selected.topology)) : _logical))(metadata.topology))) {
    error_bang($$bc$str("stock-template topology is fixed at '", selected.topology, "'; use a bespoke composition with explicit capabilities to change topology"));
  }
  const actual_overrides = ROUTING__OVERRIDE__FIELDS.filter((field) => { const candidate = route_field(metadata_object, field);
return ((!(candidate == null)) && (!same_route_value_p(candidate, route_field(base, field)))); });
  const chosen_composition = (((_truthy) => _truthy !== false && _truthy != null)(composition) ? (() => { const declared = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(composition, "overrides"); const reason = (($beagle$host$arg$0, $beagle$host$arg$1) => $$bh$aget($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1))(composition, "overrideReason"); if ((!sorted_equal_p(declared, actual_overrides))) {
  error_bang($$bc$str("composition.overrides must exactly record changed template axes: ", (($$bc$count(actual_overrides) === 0) ? "none" : actual_overrides.join(", "))));
}
if (((!($$bc$count(actual_overrides) === 0)) && (reason == null))) {
  error_bang("template axis overrides require composition.overrideReason");
}
if (((_truthy) => _truthy !== false && _truthy != null)((($$bc$count(actual_overrides) === 0) && reason))) {
  error_bang("unchanged template must omit composition.overrideReason");
}
return composition; })() : (() => { if ((!($$bc$count(actual_overrides) === 0))) {
  error_bang($$bc$str("Agent Machinery stock template ", template_id, " overrides ", actual_overrides.join(", "), "; supply template composition.overrides and composition.overrideReason"));
}
return $$bh$js_obj("kind", "template", "id", template_id, "overrides", []); })());
  const request = $$bh$js_obj("role", role, "taskGrade", (((_truthy) => _truthy !== false && _truthy != null)(metadata.taskGrade) ? metadata.taskGrade : selected.taskGrade), "domainRequirements", (((_truthy) => _truthy !== false && _truthy != null)(metadata.domainRequirements) ? metadata.domainRequirements : []), "topology", selected.topology, "capabilityFloor", (((_truthy) => _truthy !== false && _truthy != null)(metadata.capabilityFloor) ? metadata.capabilityFloor : selected.capabilityFloor), "serviceClass", (((_truthy) => _truthy !== false && _truthy != null)(metadata.serviceClass) ? metadata.serviceClass : selected.serviceClass), "reasoning", (((_truthy) => _truthy !== false && _truthy != null)(metadata.reasoning) ? metadata.reasoning : selected.deliberation), "posture", (((_truthy) => _truthy !== false && _truthy != null)(metadata.posture) ? metadata.posture : selected.posture), "composition", chosen_composition);
  validateTopologyCapabilities_bang(selected.topology, selected.capabilities, $$bc$str(template_id, ".capabilities"));
  validatePostureCapabilities_bang(required_string_bang((((_truthy) => _truthy !== false && _truthy != null)(metadata.posture) ? metadata.posture : selected.posture), "posture"), selected.capabilities, $$bc$str(template_id, ".capabilities"));
  return routing_request_bang(request, "Agent Machinery run request composer");
}

function bespoke_request_bang(metadata, catalog, role, composition, nearest, composition_id, capabilities) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (find_preset(catalog, nearest) == null) : _logical))(nearest))) {
    error_bang("composition.nearestTemplate must name a canonical stock template");
  }
  const request = $$bh$js_obj("role", role, "taskGrade", required_string_bang(metadata.taskGrade, "taskGrade"), "domainRequirements", (((_truthy) => _truthy !== false && _truthy != null)(metadata.domainRequirements) ? metadata.domainRequirements : []), "topology", required_string_bang(metadata.topology, "topology"), "capabilityFloor", required_string_bang(metadata.capabilityFloor, "capabilityFloor"), "serviceClass", required_string_bang(metadata.serviceClass, "serviceClass"), "reasoning", required_string_bang(metadata.reasoning, "reasoning"), "posture", required_string_bang(metadata.posture, "posture"), "composition", composition);
  validateTopologyCapabilities_bang(required_string_bang(metadata.topology, "topology"), capabilities, $$bc$str(composition_id, ".capabilities"));
  validatePostureCapabilities_bang(required_string_bang(metadata.posture, "posture"), capabilities, $$bc$str(composition_id, ".capabilities"));
  return routing_request_bang(request, "Agent Machinery run request composer");
}

function apply_orchestration_staffing_record_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const metadata = $beagle$args[0];
    return apply_orchestration_staffing_record_bang(metadata, load_orchestration_staffing_record_bang());
  }
  if (arguments.length === 2) {
    const metadata = $beagle$args[0];
    const catalog = $beagle$args[1];
    const normalized = routing_draft_bang(metadata);
    const role = required_string_bang(canonical_staffing_role_record_bang(normalized.role, catalog), "Agent Machinery run request composer role");
    const composition = normalized.composition;
    return (((_truthy) => _truthy !== false && _truthy != null)(composition) ? (() => { const _match_2 = composition; if (_match_2._tag === "BespokeCompositionV3") { const kind = _match_2.kind; const composition_id = _match_2.id; const nearest = _match_2.nearestTemplate; const bespoke_reason = _match_2.bespokeReason; const promotion_candidate = _match_2.promotionCandidate; const contract = _match_2.contract; return bespoke_request_bang(normalized, catalog, role, composition_object(composition), nearest, composition_id, contract.capabilities); } else if (_match_2._tag === "TemplateCompositionV3") { const kind = _match_2.kind; const template_id = _match_2.id; const overrides = _match_2.overrides; const override_reason = _match_2.overrideReason; return template_request_bang(normalized, catalog, role, composition_object(composition)); } else { return null; } })() : template_request_bang(normalized, catalog, role, null));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function applyOrchestrationStaffing_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const metadata = $beagle$args[0];
    return routing_request_object(apply_orchestration_staffing_record_bang(metadata));
  }
  if (arguments.length === 2) {
    const metadata = $beagle$args[0];
    const catalog = $beagle$args[1];
    return routing_request_object(apply_orchestration_staffing_record_bang(metadata, staffing_catalog_record_bang(catalog)));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const applyOrchestrationStaffing = applyOrchestrationStaffing_bang;

function requireManagedOrchestrationSelection_bang(...$beagle$args) {
  if (arguments.length === 1) {
    const metadata = $beagle$args[0];
    return requireManagedOrchestrationSelection_bang(metadata, "managed North agent");
  }
  if (arguments.length === 2) {
    const metadata = $beagle$args[0];
    const surface = $beagle$args[1];
    return routing_request_object(routing_request_bang(metadata, surface));
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

const requireManagedOrchestrationSelection = requireManagedOrchestrationSelection_bang;

export { DEFAULT__ORCHESTRATION__STAFFING__PATH as "DEFAULT_ORCHESTRATION_STAFFING_PATH" };
export { ORCHESTRATION__STOCK__ROLE__IDS as "ORCHESTRATION_STOCK_ROLE_IDS" };
export { applyOrchestrationStaffing as "applyOrchestrationStaffing" };
export { canonicalStaffingRole as "canonicalStaffingRole" };
export { loadOrchestrationStaffing as "loadOrchestrationStaffing" };
export { orchestrationCapabilities as "orchestrationCapabilities" };
export { requireManagedOrchestrationSelection as "requireManagedOrchestrationSelection" };
