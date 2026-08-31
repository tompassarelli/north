import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { canonicalRoleId } from "./role-id.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const STAFFING_CATALOG_PATH = resolve(ROOT, "staffing/catalog.json");
export const STAFFING_CATALOG_SCHEMA_ID = "urn:agent-machinery:schema:staffing-catalog:v3";

export const CAPABILITY_IMPLICATIONS = Object.freeze({
  "filesystem.search": Object.freeze(["filesystem.read"]),
  "shell.readonly": Object.freeze(["filesystem.read", "filesystem.search"]),
  shell: Object.freeze(["filesystem.read", "filesystem.search", "filesystem.write"]),
});

export function effectiveCapabilities(capabilities) {
  const effective = new Set(capabilities);
  let changed = true;
  while (changed) {
    changed = false;
    for (const capability of [...effective]) {
      for (const implied of CAPABILITY_IMPLICATIONS[capability] ?? []) {
        if (effective.has(implied)) continue;
        effective.add(implied);
        changed = true;
      }
    }
  }
  return [...effective];
}

export function effectiveFilesystemAuthority(capabilities) {
  const effective = new Set(effectiveCapabilities(capabilities));
  return Object.freeze({
    read: effective.has("filesystem.read"),
    search: effective.has("filesystem.search"),
    write: effective.has("filesystem.write"),
  });
}

export function validateCapabilityClosure(capabilities, label = "capabilities") {
  const declared = new Set(capabilities);
  const missing = effectiveCapabilities(capabilities).filter((capability) => !declared.has(capability));
  if (missing.length)
    throw new Error(`${label}: capability list is not closed; missing implied ${missing.join(", ")}`);
  return capabilities;
}

export function validateTopologyCapabilities(topology, capabilities, label = "capabilities") {
  const has = (capability) => capabilities.includes(capability);
  validateCapabilityClosure(capabilities, label);
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

export function validatePostureCapabilities(posture, capabilities, label = "capabilities") {
  const has = (capability) => capabilities.includes(capability);
  if (posture === "preserve" && (has("filesystem.write") || has("shell")))
    throw new Error(`${label}: preserve posture requires a non-authoring capability boundary`);
  if (posture === "prune" && (!has("filesystem.write") || !has("shell")))
    throw new Error(`${label}: prune posture requires filesystem.write and shell capabilities`);
}

function keysOnly(value, allowed, label) {
  const unknown = Object.keys(value ?? {}).filter((key) => !allowed.includes(key));
  if (unknown.length) throw new Error(`agent-run template catalog: ${label} has unknown field(s): ${unknown.join(", ")}`);
}

export function validateStaffingCatalog(catalog) {
  if (catalog?.version !== 3) throw new Error("agent-run template catalog: version must be 3");
  keysOnly(catalog, ["$schema", "version", "vocabulary", "defaults", "presets"], "top level");
  if (catalog.$schema !== STAFFING_CATALOG_SCHEMA_ID)
    throw new Error(`agent-run template catalog: $schema must be ${STAFFING_CATALOG_SCHEMA_ID}`);
  const vocabulary = catalog?.vocabulary;
  const axes = ["taskGrades", "capabilityFloors", "serviceClasses", "deliberations", "topologies", "postures", "capabilities"];
  keysOnly(vocabulary, axes, "vocabulary");
  for (const axis of axes) {
    const values = vocabulary?.[axis];
    if (!Array.isArray(values) || !values.length || values.some((value) => typeof value !== "string" || !value))
      throw new Error(`agent-run template catalog: vocabulary.${axis} must contain non-empty strings`);
    if (new Set(values).size !== values.length) throw new Error(`agent-run template catalog: duplicate vocabulary.${axis}`);
  }
  keysOnly(catalog.defaults, ["taskGrade", "capabilityFloor", "serviceClass", "deliberation", "topology", "posture"], "defaults");
  for (const [field, axis] of [["taskGrade", "taskGrades"], ["capabilityFloor", "capabilityFloors"], ["serviceClass", "serviceClasses"], ["deliberation", "deliberations"], ["topology", "topologies"], ["posture", "postures"]])
    if (!vocabulary[axis].includes(catalog.defaults?.[field])) throw new Error(`agent-run template catalog: invalid defaults.${field}`);
  if (!Array.isArray(catalog.presets) || !catalog.presets.length) throw new Error("agent-run template catalog: presets must be non-empty");
  const names = new Set();
  for (const preset of catalog.presets) {
    keysOnly(preset, ["name", "taskGrade", "capabilityFloor", "serviceClass", "deliberation", "topology", "posture", "capabilities", "tagline", "description"], `preset ${preset?.name ?? "<unknown>"}`);
    canonicalRoleId(preset?.name, "agent-run template catalog preset name");
    if (names.has(preset.name)) throw new Error(`agent-run template catalog: duplicate preset name ${preset.name}`);
    names.add(preset.name);
    for (const [field, axis] of [
      ["taskGrade", "taskGrades"], ["capabilityFloor", "capabilityFloors"], ["serviceClass", "serviceClasses"], ["deliberation", "deliberations"],
      ["topology", "topologies"], ["posture", "postures"],
    ]) {
      if (!vocabulary[axis].includes(preset[field])) throw new Error(`${preset.name}: invalid ${field} ${JSON.stringify(preset[field])}`);
    }
    if (!Array.isArray(preset.capabilities) || !preset.capabilities.length ||
        preset.capabilities.some((capability) => !vocabulary.capabilities.includes(capability)) ||
        new Set(preset.capabilities).size !== preset.capabilities.length)
      throw new Error(`${preset.name}: capabilities must contain unique canonical capability labels`);
    validateTopologyCapabilities(preset.topology, preset.capabilities, `${preset.name}.capabilities`);
    validatePostureCapabilities(preset.posture, preset.capabilities, `${preset.name}.capabilities`);
    for (const field of ["tagline", "description"])
      if (typeof preset[field] !== "string" || !preset[field].trim()) throw new Error(`${preset.name}: missing ${field}`);
  }
  return catalog;
}

export function loadStaffingCatalog(path = STAFFING_CATALOG_PATH) {
  return validateStaffingCatalog(JSON.parse(readFileSync(path, "utf8")));
}
