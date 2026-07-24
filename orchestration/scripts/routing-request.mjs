import { loadStaffingCatalog, validateTopologyCapabilities } from "./staffing-catalog.mjs";
import { resolvableDeliberations } from "./provider-catalog.mjs";
import { canonicalRoleId } from "./role-id.mjs";

export const ROUTING_FIELDS = [
  "role", "taskGrade", "domainRequirements", "topology", "tier", "reasoning", "posture", "composition",
];
export const OVERRIDE_FIELDS = [
  "taskGrade", "domainRequirements", "tier", "reasoning", "posture",
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
    tier: preset.tier,
    reasoning: preset.deliberation,
    posture: preset.posture,
  };
}

export function presetOverrides(request, preset, catalog) {
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
    ["taskGrade", "taskGrades"], ["topology", "topologies"], ["tier", "semanticTiers"],
    ["reasoning", "deliberations"], ["posture", "postures"],
  ]) {
    if (!catalog.vocabulary[axis].includes(request[field]))
      throw new Error(`${field} must be one of: ${catalog.vocabulary[axis].join(", ")}`);
  }
  stringList(request.domainRequirements, "domainRequirements");
  if (!resolvableDeliberations(request.tier).has(request.reasoning))
    throw new Error(`unsupported route: tier '${request.tier}' with deliberation '${request.reasoning}' resolves through no provider catalog`);

  const composition = object(request.composition, "composition");
  const alias = catalog.aliases.find(({ name }) => name === role);
  const canonicalRole = alias?.target ?? role;
  if (canonicalRole !== role) throw new Error(`role must use canonical stock-template name ${canonicalRole}`);
  const preset = catalog.presets.find(({ name }) => name === role);

  if (composition.kind === "preset") {
    keysExactly(composition, composition.overrideReason === undefined
      ? ["kind", "id", "overrides"] : ["kind", "id", "overrides", "overrideReason"], "composition");
    if (!preset) throw new Error(`unknown role ${role} requires a bespoke composition`);
    const compositionId = canonicalRoleId(composition.id, "composition.id");
    if (compositionId !== role) throw new Error(`composition.id must match canonical role ${role}`);
    if (request.topology !== preset.topology)
      throw new Error(`stock-template topology is fixed at '${preset.topology}'; use a bespoke composition for '${request.topology}'`);
    const declared = stringList(composition.overrides, "composition.overrides");
    if (declared.some((field) => !OVERRIDE_FIELDS.includes(field)))
      throw new Error(`composition.overrides may contain only: ${OVERRIDE_FIELDS.join(", ")}`);
    const actual = presetOverrides(request, preset, catalog);
    if (!equal([...declared].sort(), [...actual].sort()))
      throw new Error(`composition.overrides must exactly record changed stock-template axes: ${actual.join(", ") || "none"}`);
    if (actual.length) nonEmptyString(composition.overrideReason, "composition.overrideReason");
    else if (composition.overrideReason !== undefined)
      throw new Error("unchanged stock template must omit composition.overrideReason");
    validateTopologyCapabilities(request.topology, preset.capabilities, `routing stock template ${role}`);
  } else if (composition.kind === "bespoke") {
    const allowed = ["kind", "id", "nearestPreset", "bespokeReason", "promotionCandidate", "contract"];
    const unknown = Object.keys(composition).filter((key) => !allowed.includes(key));
    const missing = ["kind", "id", "bespokeReason", "promotionCandidate", "contract"]
      .filter((key) => !Object.hasOwn(composition, key));
    if (unknown.length) throw new Error(`composition has unknown field(s): ${unknown.join(", ")}`);
    if (missing.length) throw new Error(`composition is missing field(s): ${missing.join(", ")}`);
    if (preset) throw new Error(`known stock-template role ${role} requires composition.kind "preset"`);
    const compositionId = canonicalRoleId(composition.id, "composition.id");
    if (compositionId !== role) throw new Error(`composition.id must match canonical role ${role}`);
    if (composition.nearestPreset !== undefined) {
      const nearest = canonicalRoleId(composition.nearestPreset, "composition.nearestPreset");
      if (!catalog.presets.some(({ name }) => name === nearest))
        throw new Error(`composition.nearestPreset must name a canonical stock template`);
    }
    nonEmptyString(composition.bespokeReason, "composition.bespokeReason");
    if (typeof composition.promotionCandidate !== "boolean")
      throw new Error("composition.promotionCandidate must be boolean");
    validateContract(composition.contract, catalog);
    validateTopologyCapabilities(request.topology, composition.contract.capabilities, `routing bespoke ${role}`);
  } else {
    throw new Error("composition.kind must be preset or bespoke");
  }
  return request;
}
