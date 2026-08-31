import {
  loadStaffingCatalog, validatePostureCapabilities, validateTopologyCapabilities,
} from "./staffing-catalog.mjs";
import { canonicalRoleId } from "./role-id.mjs";
import { resolveProjectExposureProfile } from "./project-exposure-profile.mjs";

export const ROUTING_REQUEST_SCHEMA_ID = "urn:agent-machinery:schema:routing-request:v3";
export const ROUTING_FIELDS = [
  "role", "taskGrade", "domainRequirements", "topology", "capabilityFloor", "serviceClass", "reasoning", "posture", "composition",
];
export const OVERRIDE_FIELDS = [
  "taskGrade", "domainRequirements", "capabilityFloor", "serviceClass", "reasoning", "posture",
];
export const CONTRACT_FIELDS = [
  "responsibility", "deliverable", "capabilities", "mayDecide", "mustEscalate", "doneWhen", "report",
];

function object(value, label) {
  if (value == null || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value;
}

function keysExactly(value, allowed, label) {
  const unknown = Object.keys(value).filter((key) => !allowed.includes(key));
  const missing = allowed.filter((key) => !Object.hasOwn(value, key));
  if (unknown.length) throw new Error(`${label} has unknown field(s): ${unknown.join(", ")}`);
  if (missing.length) throw new Error(`${label} is missing field(s): ${missing.join(", ")}`);
}

function nonEmptyString(value, label) {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${label} must be a non-empty string`);
  return value.trim();
}

function stringList(value, label, { nonEmpty = false } = {}) {
  if (!Array.isArray(value) || (nonEmpty && value.length === 0) ||
      value.some((item) => typeof item !== "string" || !item.trim()))
    throw new Error(`${label} must be ${nonEmpty ? "a non-empty" : "an"} array of non-empty strings`);
  if (new Set(value.map((item) => item.trim())).size !== value.length)
    throw new Error(`${label} must not contain duplicates`);
  return value.map((item) => item.trim());
}

const equal = (left, right) => JSON.stringify(left) === JSON.stringify(right);

export function effectivePreset(preset, catalog) {
  return {
    taskGrade: preset.taskGrade,
    domainRequirements: [],
    topology: preset.topology,
    capabilityFloor: preset.capabilityFloor,
    serviceClass: preset.serviceClass,
    reasoning: preset.deliberation,
    posture: preset.posture,
  };
}

export function templateOverrides(request, preset, catalog) {
  const base = effectivePreset(preset, catalog);
  return OVERRIDE_FIELDS.filter((field) => !equal(request[field], base[field]));
}

function validateContract(value, catalog) {
  const contract = object(value, "composition.contract");
  keysExactly(contract, CONTRACT_FIELDS, "composition.contract");
  for (const field of ["responsibility", "deliverable", "report"])
    nonEmptyString(contract[field], `composition.contract.${field}`);
  for (const field of ["capabilities", "mayDecide", "mustEscalate", "doneWhen"])
    stringList(contract[field], `composition.contract.${field}`, { nonEmpty: true });
  for (const capability of contract.capabilities)
    if (!catalog.vocabulary.capabilities.includes(capability))
      throw new Error(`composition.contract.capabilities contains unknown canonical capability: ${capability}`);
}

export function validateRoutingRequest(value, catalog = loadStaffingCatalog()) {
  const request = object(value, "routing request");
  keysExactly(request, ROUTING_FIELDS, "routing request");
  const role = canonicalRoleId(request.role, "role");
  for (const [field, axis] of [
    ["taskGrade", "taskGrades"], ["topology", "topologies"], ["capabilityFloor", "capabilityFloors"],
    ["serviceClass", "serviceClasses"],
    ["reasoning", "deliberations"], ["posture", "postures"],
  ]) {
    if (!catalog.vocabulary[axis].includes(request[field]))
      throw new Error(`${field} must be one of: ${catalog.vocabulary[axis].join(", ")}`);
  }
  stringList(request.domainRequirements, "domainRequirements");

  const composition = object(request.composition, "composition");

  if (composition.kind === "template") {
    keysExactly(composition, composition.overrideReason === undefined
      ? ["kind", "id", "overrides"] : ["kind", "id", "overrides", "overrideReason"], "composition");
    const compositionId = canonicalRoleId(composition.id, "composition.id");
    const preset = catalog.presets.find(({ name }) => name === compositionId);
    if (!preset) throw new Error(`unknown stock template ${compositionId}`);
    if (request.topology !== preset.topology)
      throw new Error(`stock-template topology is fixed at '${preset.topology}'; use a bespoke composition for '${request.topology}'`);
    const declared = stringList(composition.overrides, "composition.overrides");
    if (declared.some((field) => !OVERRIDE_FIELDS.includes(field)))
      throw new Error(`composition.overrides may contain only: ${OVERRIDE_FIELDS.join(", ")}`);
    const actual = templateOverrides(request, preset, catalog);
    if (!equal([...declared].sort(), [...actual].sort()))
      throw new Error(`composition.overrides must exactly record changed stock-template axes: ${actual.join(", ") || "none"}`);
    if (actual.length) nonEmptyString(composition.overrideReason, "composition.overrideReason");
    else if (composition.overrideReason !== undefined)
      throw new Error("unchanged stock template must omit composition.overrideReason");
    validateTopologyCapabilities(request.topology, preset.capabilities, `routing stock template ${role}`);
    validatePostureCapabilities(request.posture, preset.capabilities, `routing stock template ${role}`);
  } else if (composition.kind === "bespoke") {
    const allowed = ["kind", "id", "nearestTemplate", "bespokeReason", "promotionCandidate", "contract"];
    const unknown = Object.keys(composition).filter((key) => !allowed.includes(key));
    const missing = ["kind", "id", "bespokeReason", "promotionCandidate", "contract"]
      .filter((key) => !Object.hasOwn(composition, key));
    if (unknown.length) throw new Error(`composition has unknown field(s): ${unknown.join(", ")}`);
    if (missing.length) throw new Error(`composition is missing field(s): ${missing.join(", ")}`);
    canonicalRoleId(composition.id, "composition.id");
    if (composition.nearestTemplate !== undefined) {
      const nearest = canonicalRoleId(composition.nearestTemplate, "composition.nearestTemplate");
      if (!catalog.presets.some(({ name }) => name === nearest))
        throw new Error(`composition.nearestTemplate must name a canonical stock template`);
    }
    nonEmptyString(composition.bespokeReason, "composition.bespokeReason");
    if (typeof composition.promotionCandidate !== "boolean")
      throw new Error("composition.promotionCandidate must be boolean");
    validateContract(composition.contract, catalog);
    validateTopologyCapabilities(request.topology, composition.contract.capabilities, `routing bespoke ${role}`);
    validatePostureCapabilities(request.posture, composition.contract.capabilities, `routing bespoke ${role}`);
  } else {
    throw new Error("composition.kind must be template or bespoke");
  }
  return request;
}

export function validateRoutingAdmission(projectProfile, routingRequest, catalog = loadStaffingCatalog()) {
  resolveProjectExposureProfile(projectProfile);
  return validateRoutingRequest(routingRequest, catalog);
}
