import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
import type {
  RoutingDraft, RoutingMetadata, RoutingOverrideField, RoutingRequest,
} from "./routing-metadata";
import {
  POSTURES, REASONING_LEVELS, SEMANTIC_TIERS, TASK_GRADES, TOPOLOGIES,
  parseCompleteRoutingRequest,
} from "./routing-metadata";
import {
  ORCHESTRATION_CAPABILITIES, ORCHESTRATION_PRESET_CAPABILITIES, requireOrchestrationCapabilities,
  validatePostureCapabilities, validateTopologyCapabilities,
  type OrchestrationCapability,
} from "./orchestration-capabilities";
import { requireOrchestrationRoleId } from "./orchestration-role-id";
import { requireProviderNeutralRoute } from "./provider-neutral-route";
import {
  projectStaffingCatalog, staffingSource, warnGraphCatalogFallback,
} from "./orchestration-graph-source";

interface StaffingPreset {
  name: string; taskGrade: string; tier: string; deliberation: string;
  topology: string; posture: string; tagline: string; description: string;
  capabilities: OrchestrationCapability[];
}
interface StaffingDefaults {
  taskGrade: string;
  tier: string;
  deliberation: string;
  topology: string;
  posture: string;
}
interface StaffingCatalog {
  sourceVersion: 2;
  vocabulary: { capabilities: OrchestrationCapability[] };
  defaults: StaffingDefaults;
  presets: StaffingPreset[];
}

export const ORCHESTRATION_STOCK_ROLE_IDS = [
  "executor", "curator", "implementer", "integrator", "designer", "director", "scout",
  "analyst", "guardian", "reviewer", "verifier", "judge", "scientist",
  "team-lead", "program", "portfolio",
] as const;
const STOCK_AUTHORING_ROLES = new Set(["executor", "curator", "implementer", "integrator"]);

export const DEFAULT_ORCHESTRATION_STAFFING_PATH = resolve(
  process.env.AGENT_MACHINERY_HOME ?? resolve(process.env.HOME ?? "", "code/agent-machinery/main"),
  "staffing/catalog.json",
);

const TOP_LEVEL_FIELDS = ["$schema", "version", "vocabulary", "defaults", "presets"];
const VOCABULARY_FIELDS = [
  "taskGrades", "semanticTiers", "deliberations", "topologies", "postures", "capabilities",
];
const DEFAULT_FIELDS = ["taskGrade", "tier", "deliberation", "topology", "posture"];
const PRESET_FIELDS = [
  "name", "taskGrade", "tier", "deliberation", "topology", "posture",
  "capabilities", "tagline", "description",
];

function exactKeys(value: unknown, allowed: readonly string[], label: string): value is Record<string, any> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`staffing catalog: ${label} must be an object`);
  const keys = Object.keys(value);
  const unknown = keys.filter((key) => !allowed.includes(key));
  const missing = allowed.filter((key) => key !== "$schema" && !Object.hasOwn(value, key));
  if (unknown.length) throw new Error(`staffing catalog: ${label} has unknown field(s): ${unknown.join(", ")}`);
  if (missing.length) throw new Error(`staffing catalog: ${label} is missing field(s): ${missing.join(", ")}`);
  return true;
}

function uniqueVocabulary(value: unknown, label: string): string[] {
  if (!Array.isArray(value) || value.length === 0
      || value.some((entry) => typeof entry !== "string" || !entry))
    throw new Error(`staffing catalog: vocabulary.${label} must contain non-empty strings`);
  if (new Set(value).size !== value.length)
    throw new Error(`staffing catalog: duplicate vocabulary.${label}`);
  return value;
}

export function loadOrchestrationStaffing(
  path = process.env.ORCHESTRATION_STAFFING_CATALOG ?? DEFAULT_ORCHESTRATION_STAFFING_PATH,
): StaffingCatalog {
  // Dual-read seam (Phase 1): graph mode reconstructs the identical catalog
  // shape from @catalog:current; file mode (default) reads the Agent Machinery JSON.
  // On projector failure graph mode FALLS BACK to the packaged JSON so spawn
  // admission never blocks on the graph (the named failure is logged).
  let value: Record<string, any>;
  if (staffingSource() === "graph") {
    try {
      value = projectStaffingCatalog() as Record<string, any>;
    } catch (error) {
      warnGraphCatalogFallback("staffing catalog", error);
      value = JSON.parse(readFileSync(path, "utf8"));
    }
  } else {
    value = JSON.parse(readFileSync(path, "utf8"));
  }
  const sourceVersion = value.version;
  if (sourceVersion !== 2) throw new Error("staffing catalog: version must be 2");
  exactKeys(value, TOP_LEVEL_FIELDS, "top level");
  exactKeys(value.vocabulary, VOCABULARY_FIELDS, "vocabulary");
  const vocabularyByAxis = Object.fromEntries(
    VOCABULARY_FIELDS.map((axis) => [axis, uniqueVocabulary(value.vocabulary[axis], axis)]),
  );
  for (const [axis, expected] of Object.entries({
    taskGrades: TASK_GRADES,
    semanticTiers: SEMANTIC_TIERS,
    deliberations: REASONING_LEVELS,
    topologies: TOPOLOGIES,
    postures: POSTURES,
    capabilities: ORCHESTRATION_CAPABILITIES,
  })) {
    const actual = [...vocabularyByAxis[axis]].sort();
    if (JSON.stringify(actual) !== JSON.stringify([...expected].sort()))
      throw new Error(`Agent Machinery wire vocabulary drift at ${path}: ${axis}`);
  }
  exactKeys(value.defaults, DEFAULT_FIELDS, "defaults");
  for (const [field, axis] of [
    ["taskGrade", "taskGrades"], ["tier", "semanticTiers"], ["deliberation", "deliberations"],
    ["topology", "topologies"], ["posture", "postures"],
  ] as const) {
    if (!vocabularyByAxis[axis].includes(value.defaults[field]))
      throw new Error(`staffing catalog: invalid defaults.${field}`);
  }
  const presets = value.presets;
  if (!Array.isArray(presets) || presets.length === 0)
    throw new Error("staffing catalog: presets must be non-empty");
  const vocabulary = requireOrchestrationCapabilities(
    value.vocabulary?.capabilities, "staffing catalog vocabulary.capabilities",
  );
  if (JSON.stringify([...vocabulary].sort())
      !== JSON.stringify([...ORCHESTRATION_CAPABILITIES].sort()))
    throw new Error(`Agent Machinery capability vocabulary drift at ${path}`);
  const presetNames = new Set<string>();
  for (const preset of presets) {
    exactKeys(preset, PRESET_FIELDS, `preset ${preset?.name ?? "<unknown>"}`);
    requireOrchestrationRoleId(preset.name, "staffing catalog preset");
    if (presetNames.has(preset.name)) throw new Error(`duplicate Agent Machinery template ${preset.name}`);
    presetNames.add(preset.name);
    preset.capabilities = requireOrchestrationCapabilities(
      preset.capabilities, `staffing preset ${preset.name}.capabilities`,
    );
    if (preset.capabilities.some((capability: OrchestrationCapability) => !ORCHESTRATION_PRESET_CAPABILITIES.includes(capability as typeof ORCHESTRATION_PRESET_CAPABILITIES[number])))
      throw new Error(`staffing preset ${preset.name}.capabilities contains a bespoke-only capability`);
    for (const [field, axis] of [
      ["taskGrade", "taskGrades"], ["tier", "semanticTiers"], ["deliberation", "deliberations"],
      ["topology", "topologies"], ["posture", "postures"],
    ] as const) {
      if (!vocabularyByAxis[axis].includes(preset[field]))
        throw new Error(`${preset.name}: invalid ${field} ${JSON.stringify(preset[field])}`);
    }
    if (typeof preset.tagline !== "string" || !preset.tagline.trim()
        || typeof preset.description !== "string" || !preset.description.trim())
      throw new Error(`${preset.name}: missing tagline or description`);
    if (preset.topology !== "worker" && preset.topology !== "orchestrator")
      throw new Error(`invalid Agent Machinery topology for ${preset.name}`);
    validateTopologyCapabilities(preset.topology, preset.capabilities, `${preset.name}.capabilities`);
    validatePostureCapabilities(preset.posture, preset.capabilities, `${preset.name}.capabilities`);
  }
  const exactNames = [...ORCHESTRATION_STOCK_ROLE_IDS].sort();
  const actualNames = [...presetNames].sort();
  if (JSON.stringify(actualNames) !== JSON.stringify(exactNames))
    throw new Error(`Agent Machinery stock template set drift at ${path}`);
  const orchestrators = presets.filter(({ topology }) => topology === "orchestrator")
    .map(({ name }) => name);
  const stockOrchestrators = ["director", "team-lead", "program", "portfolio"];
  if (JSON.stringify([...orchestrators].sort()) !== JSON.stringify([...stockOrchestrators].sort()))
    throw new Error(
      `Agent Machinery stock topology drift at ${path}: orchestrator topology is the director plus the scope ladder`,
    );
  for (const preset of presets) {
    const capabilities = new Set(preset.capabilities);
    if (!capabilities.has("filesystem.read") || !capabilities.has("filesystem.search"))
      throw new Error(`Agent Machinery stock template ${preset.name} must retain read and search authority`);
    if (STOCK_AUTHORING_ROLES.has(preset.name)) {
      if (!capabilities.has("filesystem.write") || !capabilities.has("shell"))
        throw new Error(`Agent Machinery stock authoring template ${preset.name} must retain write and shell authority`);
    } else if (capabilities.has("filesystem.write") || capabilities.has("shell")
               || !capabilities.has("shell.readonly")) {
      throw new Error(`Agent Machinery stock nonauthoring template ${preset.name} must remain read-only`);
    }
    const ORCHESTRATOR_LADDER = new Set(["director", "team-lead", "program", "portfolio"]);
    if (ORCHESTRATOR_LADDER.has(preset.name) !== capabilities.has("coordination"))
      throw new Error("Agent Machinery stock coordination authority belongs to the orchestrator ladder");
  }
  return { sourceVersion, vocabulary: value.vocabulary, defaults: value.defaults, presets };
}

export function canonicalStaffingRole(role: string | undefined, catalog = loadOrchestrationStaffing()): string | undefined {
  if (role === undefined) return undefined;
  requireOrchestrationRoleId(role);
  return role;
}

export function orchestrationCapabilities(
  metadata: RoutingRequest,
  catalog = loadOrchestrationStaffing(),
): OrchestrationCapability[] {
  const role = canonicalStaffingRole(metadata.role, catalog);
  if (!role || !metadata.composition)
    throw new Error("managed Agent Machinery capabilities require a selected role and composition");
  if (metadata.composition.kind === "bespoke")
    return requireOrchestrationCapabilities(
      metadata.composition.contract.capabilities, "composition.contract.capabilities",
    );
  const templateId = metadata.composition.id;
  const preset = catalog.presets.find(({ name }) => name === templateId);
  if (!preset) throw new Error(`Agent Machinery stock template ${templateId} is absent from the catalog`);
  return [...preset.capabilities];
}

/** Compose a request: overrideable axes may vary, but stock topology is fixed. */
export function applyOrchestrationStaffing(
  metadata: RoutingDraft,
  catalog = loadOrchestrationStaffing(),
): RoutingRequest {
  const role = canonicalStaffingRole(metadata.role, catalog);
  if (!role) throw new Error("Agent Machinery run request composer requires an explicit role");
  const composition = metadata.composition;
  if (composition?.kind === "bespoke") {
    const nearest = composition?.kind === "bespoke" && composition.nearestTemplate
      ? catalog.presets.find(({ name }) => name === composition.nearestTemplate)
      : undefined;
    const nearestKnown = composition?.kind === "bespoke"
      && (composition.nearestTemplate === undefined || nearest !== undefined);
    if (!nearestKnown)
      throw new Error("composition.nearestTemplate must name a canonical stock template");
    const missing = ["taskGrade", "topology", "tier", "reasoning", "posture"]
      .filter((field) => metadata[field as keyof RoutingMetadata] === undefined);
    if (!composition.bespokeReason || typeof composition.promotionCandidate !== "boolean"
        || !composition.contract || missing.length) {
      const detail = missing.length ? `; missing executable axes: ${missing.join(", ")}` : "";
      throw new Error(
        `bespoke Agent Machinery composition ${composition.id} for role ${role} requires `
        + "an optional-but-valid nearestTemplate, composition.bespokeReason, explicit promotionCandidate, "
        + `structured contract, and all unseeded routing axes${detail}`,
      );
    }
    const request = {
      ...metadata,
      role,
      taskGrade: metadata.taskGrade,
      domainRequirements: metadata.domainRequirements ?? [],
      topology: metadata.topology,
      tier: metadata.tier,
      reasoning: metadata.reasoning,
      posture: metadata.posture,
    } as RoutingRequest;
    validateTopologyCapabilities(
      request.topology, composition.contract.capabilities, `${composition.id}.capabilities`,
    );
    validatePostureCapabilities(
      request.posture, composition.contract.capabilities, `${composition.id}.capabilities`,
    );
    requireProviderNeutralRoute(request.tier, request.reasoning);
    return parseCompleteRoutingRequest(request, "Agent Machinery run request composer");
  }
  const templateId = composition?.kind === "template" ? composition.id : role;
  requireOrchestrationRoleId(templateId, "composition.id");
  const preset = catalog.presets.find(({ name }) => name === templateId);
  if (!preset) throw new Error(`unknown stock template ${templateId}`);
  const base = {
    taskGrade: preset.taskGrade,
    domainRequirements: [],
    topology: preset.topology,
    tier: preset.tier,
    reasoning: preset.deliberation,
    posture: preset.posture ?? catalog.defaults.posture,
  };
  if (metadata.topology !== undefined && metadata.topology !== preset.topology) {
    throw new Error(
      `stock-template topology is fixed at '${preset.topology}'; `
      + "use a bespoke composition with explicit capabilities to change topology",
    );
  }
  const overrideFields: RoutingOverrideField[] = [
    "taskGrade", "domainRequirements", "tier", "reasoning", "posture",
  ];
  const same = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);
  const actualOverrides = overrideFields.filter((field) =>
    metadata[field] !== undefined && !same(metadata[field], base[field]));
  if (actualOverrides.length && !composition) {
    throw new Error(
      `Agent Machinery stock template ${templateId} overrides ${actualOverrides.join(", ")}; supply template composition.overrides and composition.overrideReason`,
    );
  }
  if (composition?.kind === "template") {
    const declared = [...composition.overrides].sort();
    const actual = [...actualOverrides].sort();
    if (!same(declared, actual))
      throw new Error(`composition.overrides must exactly record changed template axes: ${actual.join(", ") || "none"}`);
    if (actualOverrides.length && !composition.overrideReason)
      throw new Error("template axis overrides require composition.overrideReason");
    if (!actualOverrides.length && composition.overrideReason !== undefined)
      throw new Error("unchanged template must omit composition.overrideReason");
  }
  validateTopologyCapabilities(
    preset.topology as "worker" | "orchestrator",
    preset.capabilities,
    `${templateId}.capabilities`,
  );
  const request = {
    role,
    taskGrade: metadata.taskGrade ?? base.taskGrade as RoutingMetadata["taskGrade"],
    domainRequirements: metadata.domainRequirements ?? [],
    topology: base.topology as RoutingMetadata["topology"],
    tier: metadata.tier ?? base.tier as RoutingMetadata["tier"],
    reasoning: metadata.reasoning ?? base.reasoning as RoutingMetadata["reasoning"],
    posture: metadata.posture ?? base.posture as RoutingMetadata["posture"],
    composition: composition ?? { kind: "template", id: templateId, overrides: [] },
  } as RoutingRequest;
  validatePostureCapabilities(request.posture, preset.capabilities, `${templateId}.capabilities`);
  requireProviderNeutralRoute(request.tier, request.reasoning);
  return parseCompleteRoutingRequest(request, "Agent Machinery run request composer");
}

/**
 * Managed North lanes must have an attributable Agent Machinery run design.
 * Template identity is provenance inside composition and is independent of
 * role; bespoke compositions carry their complete authority contract. Native
 * provider sessions are outside this boundary and remain honestly unselected.
 */
export function requireManagedOrchestrationSelection(
  metadata: RoutingDraft,
  surface = "managed North agent",
): RoutingRequest {
  const required = [
    "role", "taskGrade", "domainRequirements", "topology",
    "tier", "reasoning", "posture", "composition",
  ] as const;
  const missing = required.filter((field) => metadata[field] === undefined);
  if (missing.length) {
    throw new Error(
      `${surface} requires the complete eight-field Agent Machinery run request; missing: ${missing.join(", ")}`,
    );
  }
  return metadata as RoutingRequest;
}
