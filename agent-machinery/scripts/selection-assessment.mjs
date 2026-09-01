export const SELECTION_ASSESSMENT_VERSION = "minimum-sufficient-v2";
export const SELECTION_ASSESSMENT_SCHEMA_ID = "urn:agent-machinery:schema:selection-assessment:v2";

export const SIGNAL_VALUES = Object.freeze({
  decisionOwnership: ["none", "bounded", "cross-boundary", "system-shaping", "open-solution-class"],
  seamScope: ["none", "established", "consequential", "system-wide"],
  errorExposure: ["contained-reversible", "material-recoverable", "high-or-hard-to-reverse"],
  oracleStrength: ["not-applicable", "objective-local", "objective-end-to-end", "partial", "judgment-only"],
  foundationalImpact: ["none", "implementation-only", "invariant-decision-owned"],
  dependencyShape: ["atomic-cohesive", "deterministic-workflow", "parallel-breadth", "dynamic-decomposition", "tightly-coupled-sequential"],
  reasoningShape: ["deterministic", "bounded-branching", "multi-hypothesis", "system-synthesis", "exceptional"],
});

export const EXCEPTION_CODES = Object.freeze([
  "explicit-human-floor",
  "recent-lower-capability-failure",
  "calibration-experiment",
  "unmodeled-risk",
]);

export const CAPABILITY_FLOORS = Object.freeze(["baseline", "standard", "advanced", "frontier"]);
export const REASONING_LEVELS = Object.freeze(["low", "medium", "high", "xhigh", "max"]);

const DECISION_ROUTE = Object.freeze({
  bounded: ["standard", "medium"],
  "cross-boundary": ["advanced", "high"],
  "system-shaping": ["frontier", "xhigh"],
  "open-solution-class": ["frontier", "xhigh"],
});
const SEAM_ROUTE = Object.freeze({
  established: ["standard", "medium"],
  consequential: ["advanced", "high"],
  "system-wide": ["frontier", "xhigh"],
});
const ERROR_ROUTE = Object.freeze({
  "material-recoverable": ["standard", "medium"],
  "high-or-hard-to-reverse": ["advanced", "high"],
});
const ORACLE_ROUTE = Object.freeze({
  partial: ["advanced", "high"],
  "judgment-only": ["advanced", "high"],
});
const FOUNDATIONAL_ROUTE = Object.freeze({
  "invariant-decision-owned": ["advanced", "high"],
});
const DEPENDENCY_ROUTE = Object.freeze({
  "parallel-breadth": ["standard", "medium"],
  "dynamic-decomposition": ["frontier", "xhigh"],
  "tightly-coupled-sequential": ["advanced", "high"],
});
const REASONING_ROUTE = Object.freeze({
  "bounded-branching": ["standard", "medium"],
  "multi-hypothesis": ["advanced", "high"],
  "system-synthesis": ["frontier", "xhigh"],
  exceptional: ["frontier", "max"],
});

const rank = (values, value) => values.indexOf(value);
const maxByRank = (values, left, right) => rank(values, left) >= rank(values, right) ? left : right;
const same = (left, right) => JSON.stringify(left) === JSON.stringify(right);

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

function nonEmptyString(value, label) {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${label} must be a non-empty string`);
  return value.trim();
}

function enumValue(value, allowed, label) {
  if (!allowed.includes(value)) throw new Error(`${label} must be one of: ${allowed.join(", ")}`);
  return value;
}

function applyRoute(state, ruleCode, route) {
  if (!route) return;
  state.ruleCodes.push(ruleCode);
  state.minimumCapabilityFloor = maxByRank(CAPABILITY_FLOORS, state.minimumCapabilityFloor, route[0]);
  state.minimumReasoning = maxByRank(REASONING_LEVELS, state.minimumReasoning, route[1]);
}

export function validateSelectionSignals(value) {
  const signals = object(value, "selection assessment signals");
  const fields = Object.keys(SIGNAL_VALUES);
  keysExactly(signals, fields, [], "selection assessment signals");
  for (const field of fields) enumValue(signals[field], SIGNAL_VALUES[field], `selection assessment signals.${field}`);
  return signals;
}

export function deriveSelectionAssessment(signalsValue) {
  const signals = validateSelectionSignals(signalsValue);
  const state = { minimumCapabilityFloor: "baseline", minimumReasoning: "low", ruleCodes: [] };

  applyRoute(state, `decision-ownership:${signals.decisionOwnership}`, DECISION_ROUTE[signals.decisionOwnership]);
  applyRoute(state, `seam-scope:${signals.seamScope}`, SEAM_ROUTE[signals.seamScope]);
  applyRoute(state, `error-exposure:${signals.errorExposure}`, ERROR_ROUTE[signals.errorExposure]);
  applyRoute(state, `oracle-strength:${signals.oracleStrength}`, ORACLE_ROUTE[signals.oracleStrength]);
  applyRoute(state, `foundational-impact:${signals.foundationalImpact}`, FOUNDATIONAL_ROUTE[signals.foundationalImpact]);
  applyRoute(state, `dependency-shape:${signals.dependencyShape}`, DEPENDENCY_ROUTE[signals.dependencyShape]);
  if (signals.reasoningShape === "deterministic") {
    const tightStrongOracle = signals.decisionOwnership === "none" && signals.seamScope === "none" &&
      signals.errorExposure === "contained-reversible" && signals.oracleStrength === "objective-end-to-end" &&
      signals.dependencyShape === "atomic-cohesive";
    applyRoute(state,
      tightStrongOracle ? "reasoning-shape:deterministic-tight-strong-oracle" : "reasoning-shape:deterministic-without-tight-strong-oracle",
      tightStrongOracle ? ["baseline", "low"] : ["standard", "medium"],
    );
  } else {
    applyRoute(state, `reasoning-shape:${signals.reasoningShape}`, REASONING_ROUTE[signals.reasoningShape]);
  }

  return state;
}

export function validateSelectionAssessment(value) {
  const assessment = object(value, "selection assessment");
  keysExactly(
    assessment,
    ["version", "signals", "derived", "selected"],
    ["$schema", "exception", "exceptionalDeliberation"],
    "selection assessment",
  );
  if (assessment.$schema !== undefined && assessment.$schema !== SELECTION_ASSESSMENT_SCHEMA_ID)
    throw new Error(`selection assessment.$schema must be ${SELECTION_ASSESSMENT_SCHEMA_ID}`);
  if (assessment.version !== SELECTION_ASSESSMENT_VERSION)
    throw new Error(`selection assessment.version must be ${SELECTION_ASSESSMENT_VERSION}`);
  const signals = validateSelectionSignals(assessment.signals);
  const derived = object(assessment.derived, "selection assessment.derived");
  keysExactly(derived, ["minimumCapabilityFloor", "minimumReasoning", "ruleCodes"], [], "selection assessment.derived");
  enumValue(derived.minimumCapabilityFloor, CAPABILITY_FLOORS, "selection assessment.derived.minimumCapabilityFloor");
  enumValue(derived.minimumReasoning, REASONING_LEVELS, "selection assessment.derived.minimumReasoning");
  if (!Array.isArray(derived.ruleCodes) || derived.ruleCodes.some((code) => typeof code !== "string" || !code) ||
      new Set(derived.ruleCodes).size !== derived.ruleCodes.length)
    throw new Error("selection assessment.derived.ruleCodes must be an array of unique non-empty strings");
  const recomputed = deriveSelectionAssessment(signals);
  if (!same(derived, recomputed))
    throw new Error(`selection assessment.derived must equal mechanically recomputed values: ${JSON.stringify(recomputed)}`);

  const selected = object(assessment.selected, "selection assessment.selected");
  keysExactly(selected, ["capabilityFloor", "reasoning"], [], "selection assessment.selected");
  enumValue(selected.capabilityFloor, CAPABILITY_FLOORS, "selection assessment.selected.capabilityFloor");
  enumValue(selected.reasoning, REASONING_LEVELS, "selection assessment.selected.reasoning");

  const capabilityComparison = rank(CAPABILITY_FLOORS, selected.capabilityFloor) - rank(CAPABILITY_FLOORS, derived.minimumCapabilityFloor);
  const reasoningComparison = rank(REASONING_LEVELS, selected.reasoning) - rank(REASONING_LEVELS, derived.minimumReasoning);
  if (capabilityComparison < 0 || reasoningComparison < 0)
    throw new Error(`selected route ${selected.capabilityFloor}/${selected.reasoning} is below derived minimum ${derived.minimumCapabilityFloor}/${derived.minimumReasoning}`);
  const aboveMinimum = capabilityComparison > 0 || reasoningComparison > 0;
  if (aboveMinimum) {
    const exception = object(assessment.exception, "selection assessment.exception");
    keysExactly(exception, ["code", "detail"], [], "selection assessment.exception");
    enumValue(exception.code, EXCEPTION_CODES, "selection assessment.exception.code");
    nonEmptyString(exception.detail, "selection assessment.exception.detail");
  } else if (assessment.exception !== undefined) {
    throw new Error("selection assessment at its derived minimum must omit exception");
  }

  const maxRequired = derived.minimumReasoning === "max" || selected.reasoning === "max";
  if (maxRequired) {
    if (signals.reasoningShape !== "exceptional")
      throw new Error("max reasoning requires reasoningShape exceptional");
    nonEmptyString(assessment.exceptionalDeliberation, "selection assessment.exceptionalDeliberation");
  } else if (assessment.exceptionalDeliberation !== undefined) {
    throw new Error("selection assessment below max must omit exceptionalDeliberation");
  }
  return assessment;
}

export function assertAssessmentSelection(assessment, capabilityFloor, reasoning) {
  validateSelectionAssessment(assessment);
  if (assessment.selected.capabilityFloor !== capabilityFloor || assessment.selected.reasoning !== reasoning)
    throw new Error(`selection assessment selected ${assessment.selected.capabilityFloor}/${assessment.selected.reasoning} but routing request selected ${capabilityFloor}/${reasoning}`);
}
