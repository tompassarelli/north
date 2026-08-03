import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
import {
  REASONING_LEVELS, SEMANTIC_TIERS,
  type ReasoningLevel, type RoutingTier,
} from "./routing-metadata";
import { canonicalReceiptJson, sha256Bytes, sha256Manifest, type ReceiptCoverage } from "./composition-receipt";

export const LEARNING_POLICY_VERSION = "north-learning-policy:v1" as const;
export const LEARNING_ASSIGNMENT_VERSION = "north-learning-assignment:v1" as const;
export const LEARNING_AXES = ["model-tier", "effort", "prompt", "authoring", "history"] as const;
export type LearningAxis = typeof LEARNING_AXES[number];
export type LearningMode = "frozen" | "learning";
export type LearningEvidenceMode = "discovery" | "evaluation";
export type LearningRisk = "p0" | "p1" | "p2" | "p3";
export type GraphTextExperimentMode = "off" | "armed";
export type GraphTextExperimentArm = "graph" | "text";

export interface GraphTextExperimentAssignment {
  version: "north-graph-text-assignment:v1";
  status: "off" | "ineligible" | "pinned" | "assigned";
  arm: GraphTextExperimentArm | "none";
  applied: boolean;
  reason: string;
  manifestSha256: string;
}

export interface LearningPolicy {
  version: 1;
  mode: LearningMode;
  intensity: number;
  axes: LearningAxis[];
  maxTierDelta: number;
  riskCeiling: LearningRisk;
  seed: string;
  epoch: string;
  evidenceMode: LearningEvidenceMode;
  graphTextExperiment: GraphTextExperimentMode;
}

export interface LearningBaseline {
  modelTier: RoutingTier;
  effort: ReasoningLevel;
  prompt: string;
  authoring: string;
  history: string;
}

export interface LearningAssignmentInput {
  episodeId: string;
  experimentId?: string;
  taskSignatureSha256: string;
  taskSignatureCoverage: ReceiptCoverage;
  risk?: LearningRisk;
  baseline: LearningBaseline;
  eligibleArms?: Partial<Record<LearningAxis, readonly string[]>>;
  hardFloor?: { modelTier: RoutingTier; effort: ReasoningLevel };
  pinnedAxes?: readonly LearningAxis[];
}

export interface LearningAssignment {
  version: typeof LEARNING_ASSIGNMENT_VERSION;
  policyVersion: typeof LEARNING_POLICY_VERSION;
  policySha256: string;
  mode: LearningMode;
  evidenceMode: LearningEvidenceMode;
  experimentId: string;
  episodeId: string;
  taskSignatureSha256: string;
  taskSignatureCoverage: ReceiptCoverage;
  risk: LearningRisk | "unknown";
  arm: "control" | "explore";
  axis: "control" | LearningAxis;
  armId: string;
  baseline: LearningBaseline;
  options: Readonly<Partial<Record<LearningAxis, readonly string[]>>>;
  propensity: {
    assigned: number;
    explore: number;
    axis: number;
    arm: number;
  };
  narrowingReason: string;
  manifestSha256: string;
  graphTextExperiment: GraphTextExperimentAssignment;
}

export const DEFAULT_LEARNING_POLICY: LearningPolicy = Object.freeze({
  version: 1,
  mode: "frozen",
  intensity: 0.1,
  axes: [...LEARNING_AXES],
  maxTierDelta: 1,
  riskCeiling: "p1",
  seed: "north-default",
  epoch: "1",
  evidenceMode: "discovery",
  graphTextExperiment: "off",
});

const SHA256 = /^[a-f0-9]{64}$/;
const IDENTIFIER = /^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$/;
const RISK_ORDER: readonly LearningRisk[] = ["p0", "p1", "p2", "p3"];
const AXIS_SET = new Set<string>(LEARNING_AXES);

function requireIdentifier(value: unknown, label: string): string {
  if (typeof value !== "string" || !IDENTIFIER.test(value))
    throw new Error(`${label} must be a portable identifier`);
  return value;
}

function requireEnum<T extends string>(value: unknown, allowed: readonly T[], label: string): T {
  if (typeof value !== "string" || !allowed.includes(value as T))
    throw new Error(`${label} must be one of: ${allowed.join(", ")}`);
  return value as T;
}

export function validateLearningPolicy(value: unknown): LearningPolicy {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("learning policy must be an object");
  const raw = value as Record<string, unknown>;
  const allowed = new Set([
    "version", "mode", "intensity", "axes", "maxTierDelta", "riskCeiling",
    "seed", "epoch", "evidenceMode", "graphTextExperiment",
  ]);
  const unknown = Object.keys(raw).filter((key) => !allowed.has(key));
  if (unknown.length) throw new Error(`learning policy has unknown field(s): ${unknown.join(", ")}`);
  if (raw.version !== 1) throw new Error("learning policy version must be 1");
  if (typeof raw.intensity !== "number" || !Number.isFinite(raw.intensity)
      || raw.intensity < 0 || raw.intensity > 1)
    throw new Error("learning intensity must be between 0 and 1");
  if (!Array.isArray(raw.axes) || raw.axes.some((axis) => !AXIS_SET.has(String(axis))))
    throw new Error(`learning axes must contain only: ${LEARNING_AXES.join(", ")}`);
  const axes = raw.axes as LearningAxis[];
  if (new Set(axes).size !== axes.length) throw new Error("learning axes must not contain duplicates");
  if (!Number.isSafeInteger(raw.maxTierDelta) || (raw.maxTierDelta as number) < 0
      || (raw.maxTierDelta as number) > SEMANTIC_TIERS.length - 1)
    throw new Error(`learning maxTierDelta must be 0..${SEMANTIC_TIERS.length - 1}`);
  return Object.freeze({
    version: 1,
    mode: requireEnum(raw.mode, ["frozen", "learning"] as const, "learning mode"),
    intensity: raw.intensity,
    axes: Object.freeze([...axes]) as LearningAxis[],
    maxTierDelta: raw.maxTierDelta as number,
    riskCeiling: requireEnum(raw.riskCeiling, RISK_ORDER, "learning risk ceiling"),
    seed: requireIdentifier(raw.seed, "learning seed"),
    epoch: requireIdentifier(raw.epoch, "learning epoch"),
    evidenceMode: requireEnum(raw.evidenceMode, ["discovery", "evaluation"] as const, "learning evidence mode"),
    graphTextExperiment: requireEnum(
      raw.graphTextExperiment ?? "off", ["off", "armed"] as const, "graph-text experiment",
    ),
  });
}

export function learningPolicyPath(env: NodeJS.ProcessEnv = process.env): string {
  return env.NORTH_LEARNING_POLICY
    ?? resolve(env.HOME ?? homedir(), ".config/north/learning-policy.json");
}

export function loadLearningPolicy(
  path = learningPolicyPath(),
  read: (path: string, encoding: "utf8") => string = readFileSync,
): LearningPolicy {
  try { return validateLearningPolicy(JSON.parse(read(path, "utf8"))); }
  catch (error) {
    if ((error as NodeJS.ErrnoException)?.code === "ENOENT") return DEFAULT_LEARNING_POLICY;
    throw new Error(`invalid learning policy ${path}: ${(error as Error).message}`);
  }
}

export function learningPolicySha256(policy: LearningPolicy): string {
  return sha256Manifest(policy);
}

function unitInterval(key: string): number {
  const sample = BigInt(`0x${sha256Bytes(key).slice(0, 13)}`);
  return Number(sample) / Number(0x10000000000000n);
}

export function graphTextExperimentAssignment(
  policy: LearningPolicy,
  episodeId: string,
  eligibility: "eligible" | "ineligible" | "risk-ineligible" | "pinned-graph" | "pinned-text" = "ineligible",
): GraphTextExperimentAssignment {
  const base = (() => {
    if (policy.graphTextExperiment === "off")
      return { status: "off" as const, arm: "none" as const, applied: false, reason: "config:off" };
    if (eligibility === "pinned-graph" || eligibility === "pinned-text") {
      return {
        status: "pinned" as const,
        arm: eligibility === "pinned-graph" ? "graph" as const : "text" as const,
        applied: false,
        reason: "operator-pinned-authoring-surface",
      };
    }
    if (eligibility === "risk-ineligible")
      return { status: "ineligible" as const, arm: "none" as const, applied: false, reason: "risk:outside-bounds" };
    if (eligibility !== "eligible")
      return { status: "ineligible" as const, arm: "none" as const, applied: false, reason: "spawn-not-eligible" };
    const key = `${learningPolicySha256(policy)}:${policy.seed}:${policy.epoch}:${episodeId}:graph-text`;
    return {
      status: "assigned" as const,
      arm: unitInterval(key) < 0.5 ? "graph" as const : "text" as const,
      applied: true,
      reason: "deterministic-balanced-assignment",
    };
  })();
  const record = { version: "north-graph-text-assignment:v1" as const, ...base };
  return Object.freeze({ ...record, manifestSha256: sha256Manifest(record) });
}

function select<T>(values: readonly T[], draw: number): T {
  return values[Math.min(values.length - 1, Math.floor(draw * values.length))]!;
}

function withinRouteBounds(
  axis: LearningAxis,
  arm: string,
  input: LearningAssignmentInput,
  policy: LearningPolicy,
): boolean {
  if (axis === "model-tier") {
    const candidate = SEMANTIC_TIERS.indexOf(arm as RoutingTier);
    const baseline = SEMANTIC_TIERS.indexOf(input.baseline.modelTier);
    const floor = SEMANTIC_TIERS.indexOf(input.hardFloor?.modelTier ?? input.baseline.modelTier);
    return candidate >= floor && Math.abs(candidate - baseline) <= policy.maxTierDelta;
  }
  if (axis === "effort") {
    const candidate = REASONING_LEVELS.indexOf(arm as ReasoningLevel);
    const baseline = REASONING_LEVELS.indexOf(input.baseline.effort);
    const floor = REASONING_LEVELS.indexOf(input.hardFloor?.effort ?? input.baseline.effort);
    return candidate >= floor && Math.abs(candidate - baseline) <= policy.maxTierDelta;
  }
  return true;
}

function eligibleOptions(
  policy: LearningPolicy,
  input: LearningAssignmentInput,
): Partial<Record<LearningAxis, readonly string[]>> {
  const pinned = new Set(input.pinnedAxes ?? []);
  const result: Partial<Record<LearningAxis, readonly string[]>> = {};
  for (const axis of policy.axes) {
    if (pinned.has(axis)) continue;
    const values = [...new Set(input.eligibleArms?.[axis] ?? [])]
      .filter((arm) => IDENTIFIER.test(arm))
      .filter((arm) => arm !== baselineArm(input.baseline, axis))
      .filter((arm) => withinRouteBounds(axis, arm, input, policy))
      .sort();
    if (values.length) result[axis] = Object.freeze(values);
  }
  return Object.freeze(result);
}

function baselineArm(baseline: LearningBaseline, axis: LearningAxis): string {
  if (axis === "model-tier") return baseline.modelTier;
  return baseline[axis];
}

function controlReason(policy: LearningPolicy, input: LearningAssignmentInput, eligibleAxes: readonly LearningAxis[]): string {
  if (policy.mode === "frozen") return "mode:frozen";
  if (!input.risk) return "risk:unknown";
  if (RISK_ORDER.indexOf(input.risk) > RISK_ORDER.indexOf(policy.riskCeiling)) return "risk:above-ceiling";
  if (policy.intensity === 0) return "intensity:zero";
  if (eligibleAxes.length === 0) return "arms:none-eligible";
  return "assignment:control";
}

export function assignLearningEpisode(
  policyValue: LearningPolicy,
  input: LearningAssignmentInput,
  graphTextEligibility: "eligible" | "ineligible" | "pinned-graph" | "pinned-text" = "ineligible",
): LearningAssignment {
  const policy = validateLearningPolicy(policyValue);
  requireIdentifier(input.episodeId, "learning episode id");
  if (!SHA256.test(input.taskSignatureSha256)) throw new Error("task signature must be a SHA-256 digest");
  requireEnum(input.baseline.modelTier, SEMANTIC_TIERS, "baseline model tier");
  requireEnum(input.baseline.effort, REASONING_LEVELS, "baseline effort");
  for (const [axis, value] of Object.entries(input.baseline)) requireIdentifier(value, `baseline ${axis}`);
  const policySha256 = learningPolicySha256(policy);
  const options = eligibleOptions(policy, input);
  const eligibleAxes = Object.keys(options) as LearningAxis[];
  const riskEligible = input.risk !== undefined
    && RISK_ORDER.indexOf(input.risk) <= RISK_ORDER.indexOf(policy.riskCeiling);
  const key = `${policySha256}:${policy.seed}:${policy.epoch}:${input.episodeId}`;
  const explore = policy.mode === "learning" && riskEligible && eligibleAxes.length > 0
    && unitInterval(`${key}:explore`) < policy.intensity;
  const axis = explore ? select(eligibleAxes, unitInterval(`${key}:axis`)) : "control" as const;
  const arms = axis === "control" ? [] : options[axis]!;
  const armId = axis === "control"
    ? "control"
    : select(arms, unitInterval(`${key}:arm:${axis}`));
  const explorePropensity = policy.mode === "learning" && riskEligible && eligibleAxes.length > 0
    ? policy.intensity : 0;
  const axisPropensity = axis === "control" ? 1 : 1 / eligibleAxes.length;
  const armPropensity = axis === "control" ? 1 : 1 / arms.length;
  const assignedPropensity = axis === "control"
    ? 1 - explorePropensity : explorePropensity * axisPropensity * armPropensity;
  const boundedGraphTextEligibility = graphTextEligibility === "eligible"
    && (input.risk === undefined
      || RISK_ORDER.indexOf(input.risk) > RISK_ORDER.indexOf(policy.riskCeiling))
    ? "risk-ineligible" as const : graphTextEligibility;
  const graphTextExperiment = graphTextExperimentAssignment(
    policy, input.episodeId, boundedGraphTextEligibility,
  );
  const base = {
    version: LEARNING_ASSIGNMENT_VERSION,
    policyVersion: LEARNING_POLICY_VERSION,
    policySha256,
    mode: policy.mode,
    evidenceMode: policy.evidenceMode,
    experimentId: input.experimentId
      ? requireIdentifier(input.experimentId, "learning experiment id")
      : `exp-${policySha256.slice(0, 16)}`,
    episodeId: input.episodeId,
    taskSignatureSha256: input.taskSignatureSha256,
    taskSignatureCoverage: input.taskSignatureCoverage,
    risk: input.risk ?? "unknown" as const,
    arm: axis === "control" ? "control" as const : "explore" as const,
    axis,
    armId,
    baseline: Object.freeze({ ...input.baseline }),
    options,
    propensity: Object.freeze({
      assigned: assignedPropensity,
      explore: explorePropensity,
      axis: axisPropensity,
      arm: armPropensity,
    }),
    narrowingReason: axis === "control"
      ? controlReason(policy, input, eligibleAxes)
      : `explore:${axis}:${armId}`,
    graphTextExperiment,
  };
  return Object.freeze({ ...base, manifestSha256: sha256Manifest(base) });
}

export function learningAssignmentFacts(assignment: LearningAssignment): Array<[string, string]> {
  return [
    ["learning_assignment_version", assignment.version],
    ["learning_policy_version", assignment.policyVersion],
    ["learning_policy_sha256", assignment.policySha256],
    ["learning_mode", assignment.mode],
    ["learning_evidence_mode", assignment.evidenceMode],
    ["learning_experiment_id", assignment.experimentId],
    ["learning_episode_id", assignment.episodeId],
    ["learning_task_signature_sha256", assignment.taskSignatureSha256],
    ["learning_task_signature_coverage", assignment.taskSignatureCoverage],
    ["learning_risk", assignment.risk],
    ["learning_arm", assignment.arm],
    ["learning_axis", assignment.axis],
    ["learning_arm_id", assignment.armId],
    ["learning_propensity", assignment.propensity.assigned.toFixed(12)],
    ["learning_explore_propensity", assignment.propensity.explore.toFixed(12)],
    ["learning_narrowing_reason", assignment.narrowingReason],
    ["learning_baseline_sha256", sha256Bytes(canonicalReceiptJson(assignment.baseline))],
    ["learning_options_sha256", sha256Bytes(canonicalReceiptJson(assignment.options))],
    ["learning_assignment_sha256", assignment.manifestSha256],
    ["graph_text_experiment_version", assignment.graphTextExperiment.version],
    ["graph_text_experiment_status", assignment.graphTextExperiment.status],
    ["graph_text_experiment_arm", assignment.graphTextExperiment.arm],
    ["graph_text_experiment_applied", String(assignment.graphTextExperiment.applied)],
    ["graph_text_experiment_reason", assignment.graphTextExperiment.reason],
    ["graph_text_experiment_assignment_sha256", assignment.graphTextExperiment.manifestSha256],
  ];
}
