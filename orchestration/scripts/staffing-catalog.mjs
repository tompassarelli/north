import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalRoleId } from "./role-id.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const STAFFING_CATALOG_PATH = resolve(ROOT, "staffing/catalog.json");

export function validateTopologyCapabilities(topology, capabilities, label = "capabilities") {
  const has = (capability) => capabilities.includes(capability);
  if (has("shell") && has("shell.readonly"))
    throw new Error(`${label}: shell and shell.readonly are mutually exclusive`);
  if (topology === "orchestrator") {
    if (!has("coordination"))
      throw new Error(`${label}: orchestrator topology requires coordination capability`);
    if (has("filesystem.write"))
      throw new Error(`${label}: orchestrator topology forbids filesystem.write capability`);
    if (has("shell"))
      throw new Error(`${label}: orchestrator topology forbids unrestricted shell capability`);
  } else if (topology === "worker" && has("coordination")) {
    throw new Error(`${label}: worker topology forbids coordination capability`);
  }
}

function keysOnly(value, allowed, label) {
  const unknown = Object.keys(value ?? {}).filter((key) => !allowed.includes(key));
  if (unknown.length) throw new Error(`staffing catalog: ${label} has unknown field(s): ${unknown.join(", ")}`);
}

export function validateStaffingCatalog(catalog) {
  if (catalog?.version !== 2) throw new Error("staffing catalog: version must be 2");
  keysOnly(catalog, ["$schema", "version", "vocabulary", "defaults", "presets", "aliases"], "top level");
  const vocabulary = catalog?.vocabulary;
  const axes = ["taskGrades", "semanticTiers", "deliberations", "topologies", "postures", "capabilities"];
  keysOnly(vocabulary, axes, "vocabulary");
  for (const axis of axes) {
    const values = vocabulary?.[axis];
    if (!Array.isArray(values) || !values.length || values.some((value) => typeof value !== "string" || !value))
      throw new Error(`staffing catalog: vocabulary.${axis} must contain non-empty strings`);
    if (new Set(values).size !== values.length) throw new Error(`staffing catalog: duplicate vocabulary.${axis}`);
  }
  keysOnly(catalog.defaults, ["taskGrade", "tier", "deliberation", "topology", "posture"], "defaults");
  for (const [field, axis] of [["taskGrade", "taskGrades"], ["tier", "semanticTiers"], ["deliberation", "deliberations"], ["topology", "topologies"], ["posture", "postures"]])
    if (!vocabulary[axis].includes(catalog.defaults?.[field])) throw new Error(`staffing catalog: invalid defaults.${field}`);
  if (!Array.isArray(catalog.presets) || !catalog.presets.length) throw new Error("staffing catalog: presets must be non-empty");
  const names = new Set();
  for (const preset of catalog.presets) {
    keysOnly(preset, ["name", "taskGrade", "tier", "deliberation", "topology", "posture", "capabilities", "tagline", "description"], `preset ${preset?.name ?? "<unknown>"}`);
    canonicalRoleId(preset?.name, "staffing catalog preset name");
    if (names.has(preset.name)) throw new Error(`staffing catalog: duplicate preset name ${preset.name}`);
    names.add(preset.name);
    for (const [field, axis] of [
      ["taskGrade", "taskGrades"], ["tier", "semanticTiers"], ["deliberation", "deliberations"],
      ["topology", "topologies"], ["posture", "postures"],
    ]) {
      if (!vocabulary[axis].includes(preset[field])) throw new Error(`${preset.name}: invalid ${field} ${JSON.stringify(preset[field])}`);
    }
    if (!Array.isArray(preset.capabilities) || !preset.capabilities.length ||
        preset.capabilities.some((capability) => !vocabulary.capabilities.includes(capability)) ||
        new Set(preset.capabilities).size !== preset.capabilities.length)
      throw new Error(`${preset.name}: capabilities must contain unique canonical capability labels`);
    validateTopologyCapabilities(preset.topology, preset.capabilities, `${preset.name}.capabilities`);
    for (const field of ["tagline", "description"])
      if (typeof preset[field] !== "string" || !preset[field].trim()) throw new Error(`${preset.name}: missing ${field}`);
  }
  if (!Array.isArray(catalog.aliases)) throw new Error("staffing catalog: aliases must be an array");
  const aliases = new Set();
  for (const alias of catalog.aliases) {
    keysOnly(alias, ["name", "target"], `alias ${alias?.name ?? "<unknown>"}`);
    canonicalRoleId(alias?.name, "staffing catalog alias name");
    canonicalRoleId(alias?.target, `staffing catalog alias ${alias.name} target`);
    if (aliases.has(alias.name) || names.has(alias.name)) throw new Error(`staffing catalog: duplicate or colliding alias ${alias.name}`);
    aliases.add(alias.name);
    if (!names.has(alias.target)) throw new Error(`staffing catalog: alias target missing: ${alias.target}`);
  }
  return catalog;
}

export function loadStaffingCatalog(path = STAFFING_CATALOG_PATH) {
  return validateStaffingCatalog(JSON.parse(readFileSync(path, "utf8")));
}
