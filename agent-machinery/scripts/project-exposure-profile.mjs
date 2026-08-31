export const PROJECT_EXPOSURE_PROFILE_VERSION = "project-exposure-v1";
export const PROJECT_EXPOSURE_PROFILE_SCHEMA_ID = "urn:agent-machinery:schema:project-exposure-profile:v1";

export const PROFILE_FACT_VALUES = Object.freeze({
  consumer: ["unknown", "owner-only", "named-break-tolerant", "named-break-intolerant"],
  state: ["none", "volatile-local", "live-durable-owner", "production-or-public"],
  effect: ["reversible", "irreversible-external"],
  correctness: ["exploratory-thesis", "exact-bounded-claim", "external-contract"],
  boundaries: ["producer-substitution", "concurrent-writer", "security", "audit", "financial", "availability"],
  stage: ["exploratory", "personally-operational", "externally-depended-upon"],
});

export const ENGINEERING_CONTEXTS = Object.freeze([
  "volatile-owner-controlled-research",
  "personally-operational",
  "externally-depended-upon",
]);

export const LIFECYCLE_EVIDENCE = Object.freeze({
  compatibility: ["named-intolerant-consumer", "explicit-operator-instruction"],
  rollback: ["live-durable-owner-state", "production-public-state", "irreversible-external-effect", "explicit-operator-instruction"],
  "generalized-assurance": ["named-intolerant-consumer", "production-public-state", "security-obligation", "audit-obligation", "financial-obligation", "availability-obligation", "explicit-operator-instruction"],
  "provenance-immutability": ["producer-substitution", "concurrent-writer", "security-obligation", "audit-obligation", "explicit-operator-instruction"],
  "release-ceremony": ["named-intolerant-consumer", "production-public-state", "irreversible-external-effect", "security-obligation", "audit-obligation", "financial-obligation", "availability-obligation", "explicit-operator-instruction"],
  "operational-hardening": ["named-intolerant-consumer", "live-durable-owner-state", "production-public-state", "irreversible-external-effect", "security-obligation", "audit-obligation", "financial-obligation", "availability-obligation", "explicit-operator-instruction"],
});
const LIFECYCLE_MECHANISMS = Object.freeze(Object.keys(LIFECYCLE_EVIDENCE));

export function defaultProjectExposureProfile(scope = "unclassified work") {
  return validateProjectExposureProfile({
    version: PROJECT_EXPOSURE_PROFILE_VERSION,
    scope,
    facts: {
      consumer: "unknown",
      state: "none",
      effect: "reversible",
      correctness: "exact-bounded-claim",
      boundaries: [],
      stage: "exploratory",
      explicitLifecycleActions: [],
    },
    engineeringContext: "volatile-owner-controlled-research",
    lifecycleBudget: [],
  });
}

export function resolveProjectExposureProfile(value, scope = "unclassified work") {
  return value === undefined
    ? defaultProjectExposureProfile(scope)
    : validateProjectExposureProfile(value);
}

function object(value, label) {
  if (value == null || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value;
}

function keysExactly(value, required, optional, label) {
  const allowed = [...required, ...optional];
  const unknown = Object.keys(value).filter((key) => !allowed.includes(key));
  const missing = required.filter((key) => !Object.hasOwn(value, key));
  if (unknown.length) throw new Error(`${label} has unknown field(s): ${unknown.join(", ")}`);
  if (missing.length) throw new Error(`${label} is missing field(s): ${missing.join(", ")}`);
}

function enumValue(value, allowed, label) {
  if (!allowed.includes(value)) throw new Error(`${label} must be one of: ${allowed.join(", ")}`);
  return value;
}

function nonEmptyString(value, label) {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${label} must be a non-empty string`);
  return value.trim();
}

function validateFacts(value) {
  const facts = object(value, "project exposure facts");
  const fields = ["consumer", "state", "effect", "correctness", "boundaries", "stage", "explicitLifecycleActions"];
  keysExactly(facts, fields, [], "project exposure facts");
  for (const field of ["consumer", "state", "effect", "correctness"])
    enumValue(facts[field], PROFILE_FACT_VALUES[field], `project exposure facts.${field}`);
  if (!Array.isArray(facts.boundaries) ||
      facts.boundaries.some((boundary) => !PROFILE_FACT_VALUES.boundaries.includes(boundary)) ||
      new Set(facts.boundaries).size !== facts.boundaries.length)
    throw new Error(`project exposure facts.boundaries must contain unique values from: ${PROFILE_FACT_VALUES.boundaries.join(", ")}`);
  enumValue(facts.stage, PROFILE_FACT_VALUES.stage, "project exposure facts.stage");
  if (!Array.isArray(facts.explicitLifecycleActions) ||
      facts.explicitLifecycleActions.some((mechanism) => !LIFECYCLE_MECHANISMS.includes(mechanism)) ||
      new Set(facts.explicitLifecycleActions).size !== facts.explicitLifecycleActions.length)
    throw new Error(`project exposure facts.explicitLifecycleActions must contain unique values from: ${LIFECYCLE_MECHANISMS.join(", ")}`);
  return facts;
}

export function deriveEngineeringContext(factsValue) {
  const facts = validateFacts(factsValue);
  const externallyDependedUpon = facts.stage === "externally-depended-upon" ||
    facts.consumer === "named-break-tolerant" || facts.consumer === "named-break-intolerant" ||
    facts.state === "production-or-public" ||
    facts.effect === "irreversible-external" ||
    facts.boundaries.some((boundary) => ["security", "audit", "financial", "availability"].includes(boundary));
  if (externallyDependedUpon) return "externally-depended-upon";
  if (facts.stage === "personally-operational" || facts.state === "live-durable-owner") return "personally-operational";
  return "volatile-owner-controlled-research";
}

function presentEvidence(facts) {
  const evidence = new Set();
  if (facts.consumer === "named-break-intolerant") evidence.add("named-intolerant-consumer");
  if (facts.state === "live-durable-owner") evidence.add("live-durable-owner-state");
  if (facts.state === "production-or-public") evidence.add("production-public-state");
  if (facts.effect === "irreversible-external") evidence.add("irreversible-external-effect");
  for (const boundary of facts.boundaries) {
    evidence.add(["producer-substitution", "concurrent-writer"].includes(boundary)
      ? boundary : `${boundary}-obligation`);
  }
  return evidence;
}

export function validateProjectExposureProfile(value) {
  const profile = object(value, "project exposure profile");
  keysExactly(profile, ["version", "scope", "facts", "engineeringContext", "lifecycleBudget"], ["$schema"], "project exposure profile");
  if (profile.$schema !== undefined && profile.$schema !== PROJECT_EXPOSURE_PROFILE_SCHEMA_ID)
    throw new Error(`project exposure profile.$schema must be ${PROJECT_EXPOSURE_PROFILE_SCHEMA_ID}`);
  if (profile.version !== PROJECT_EXPOSURE_PROFILE_VERSION)
    throw new Error(`project exposure profile.version must be ${PROJECT_EXPOSURE_PROFILE_VERSION}`);
  nonEmptyString(profile.scope, "project exposure profile.scope");
  const facts = validateFacts(profile.facts);
  enumValue(profile.engineeringContext, ENGINEERING_CONTEXTS, "project exposure profile.engineeringContext");
  const derived = deriveEngineeringContext(facts);
  if (profile.engineeringContext !== derived)
    throw new Error(`project exposure profile.engineeringContext must equal mechanically derived context ${derived}`);
  if (!Array.isArray(profile.lifecycleBudget))
    throw new Error("project exposure profile.lifecycleBudget must be an array");
  const present = presentEvidence(facts);
  const seen = new Set();
  for (const entryValue of profile.lifecycleBudget) {
    const entry = object(entryValue, "project exposure lifecycle entry");
    keysExactly(entry, ["mechanism", "evidence"], [], "project exposure lifecycle entry");
    const allowed = LIFECYCLE_EVIDENCE[entry.mechanism];
    if (!allowed) throw new Error(`unknown project exposure lifecycle mechanism: ${entry.mechanism}`);
    if (seen.has(entry.mechanism)) throw new Error(`duplicate project exposure lifecycle mechanism: ${entry.mechanism}`);
    seen.add(entry.mechanism);
    enumValue(entry.evidence, allowed, `project exposure lifecycle ${entry.mechanism} evidence`);
    if (entry.evidence === "explicit-operator-instruction" &&
        !facts.explicitLifecycleActions.includes(entry.mechanism))
      throw new Error(`project exposure lifecycle ${entry.mechanism} cites absent explicit action ${entry.mechanism}`);
    if (entry.evidence !== "explicit-operator-instruction" && !present.has(entry.evidence))
      throw new Error(`project exposure lifecycle ${entry.mechanism} cites absent fact ${entry.evidence}`);
  }
  return profile;
}
