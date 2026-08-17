import {
  assignLearningEpisode, loadLearningPolicy, type LearningAssignment,
  type LearningAxis, type LearningRisk,
} from "./learning-regime";
import {
  REASONING_LEVELS, SEMANTIC_TIERS,
  type ReasoningLevel, type RoutingRequest, type RoutingTier,
} from "./routing-metadata";
import type { RoutingAssessment, RoutingPinEvidence } from "./routing-economics";
import { requireProviderNeutralRoute } from "./provider-neutral-route";
import { sha256Manifest, type ReceiptCoverage } from "./composition-receipt";

export interface ManagedLearningInput {
  episodeId: string;
  taskSignature: unknown;
  taskSignatureCoverage: ReceiptCoverage;
  routingMetadata: RoutingRequest;
  routingAssessment?: RoutingAssessment;
  pinEvidence?: RoutingPinEvidence;
  promptArms?: readonly string[];
  authoringArms?: readonly string[];
  historyArms?: readonly string[];
}

export interface ManagedLearningDecision {
  assignment: LearningAssignment;
  routingMetadata: RoutingRequest;
  routingAssessment?: RoutingAssessment;
}

export function learningRiskFromAssessment(
  assessment?: RoutingAssessment,
): LearningRisk | undefined {
  if (!assessment) return undefined;
  if (assessment.signals.errorExposure === "contained-reversible") return "p1";
  if (assessment.signals.errorExposure === "material-recoverable") return "p2";
  return "p3";
}

function routeArms(request: RoutingRequest): Partial<Record<LearningAxis, readonly string[]>> {
  return {
    "model-tier": SEMANTIC_TIERS.filter((tier) => {
      try { requireProviderNeutralRoute(tier, request.reasoning); return true; }
      catch { return false; }
    }),
    effort: REASONING_LEVELS.filter((reasoning) => {
      if (reasoning === "max" && request.reasoning !== "max") return false;
      try { requireProviderNeutralRoute(request.tier, reasoning); return true; }
      catch { return false; }
    }),
  };
}

function pinnedAxes(
  assessment: RoutingAssessment | undefined,
  pin: RoutingPinEvidence | undefined,
): LearningAxis[] {
  const pinned: LearningAxis[] = [];
  if (assessment?.exception && assessment.exception.code !== "calibration-experiment")
    pinned.push("model-tier", "effort");
  if (pin?.pins.some(({ kind }) => kind === "model")) pinned.push("model-tier");
  return [...new Set(pinned)];
}

function applyRouteAssignment(
  request: RoutingRequest,
  assessment: RoutingAssessment | undefined,
  assignment: LearningAssignment,
): Pick<ManagedLearningDecision, "routingMetadata" | "routingAssessment"> {
  if (assignment.arm !== "explore"
      || (assignment.axis !== "model-tier" && assignment.axis !== "effort")) {
    return { routingMetadata: request, ...(assessment ? { routingAssessment: assessment } : {}) };
  }
  const field = assignment.axis === "model-tier" ? "tier" : "reasoning";
  const nextValue = assignment.armId as RoutingTier | ReasoningLevel;
  const composition = request.composition.kind === "template"
    ? {
      ...request.composition,
      overrides: [...new Set([...request.composition.overrides, field])],
      overrideReason: `learning assignment ${assignment.manifestSha256.slice(0, 16)}`,
    }
    : request.composition;
  const routingMetadata = {
    ...request,
    [field]: nextValue,
    composition,
  } as RoutingRequest;
  if (!assessment) return { routingMetadata };
  const selected = {
    tier: routingMetadata.tier,
    reasoning: routingMetadata.reasoning,
  };
  const changed = selected.tier !== assessment.derived.minimumTier
    || selected.reasoning !== assessment.derived.minimumReasoning;
  const {
    exception: _priorException,
    exceptionalDeliberation: _priorExceptionalDeliberation,
    ...assessmentBase
  } = assessment;
  const routingAssessment: RoutingAssessment = {
    ...assessmentBase,
    selected,
    ...(changed ? {
      exception: {
        code: "calibration-experiment",
        detail: `deterministic learning assignment ${assignment.manifestSha256.slice(0, 16)}`,
      },
    } : {}),
    ...(selected.reasoning === "max" || assessment.derived.minimumReasoning === "max"
      ? { exceptionalDeliberation: assessment.exceptionalDeliberation
          ?? "deterministic learning evaluation requires the admitted maximum-deliberation arm" }
      : {}),
  };
  return { routingMetadata, routingAssessment };
}

export function decideManagedLearning(input: ManagedLearningInput): ManagedLearningDecision {
  const policy = loadLearningPolicy();
  const assessment = input.routingAssessment;
  const arms = routeArms(input.routingMetadata);
  if (input.promptArms?.length) arms.prompt = input.promptArms;
  if (input.authoringArms?.length) arms.authoring = input.authoringArms;
  if (input.historyArms?.length) arms.history = input.historyArms;
  const assignment = assignLearningEpisode(policy, {
    episodeId: input.episodeId,
    taskSignatureSha256: sha256Manifest(input.taskSignature),
    taskSignatureCoverage: input.taskSignatureCoverage,
    risk: learningRiskFromAssessment(assessment),
    baseline: {
      modelTier: input.routingMetadata.tier,
      effort: input.routingMetadata.reasoning,
      prompt: "managed-baseline",
      authoring: "managed-capability-contract",
      history: "git",
    },
    hardFloor: {
      modelTier: assessment?.derived.minimumTier ?? input.routingMetadata.tier,
      effort: assessment?.derived.minimumReasoning ?? input.routingMetadata.reasoning,
    },
    eligibleArms: arms,
    pinnedAxes: pinnedAxes(assessment, input.pinEvidence),
  });
  return { assignment, ...applyRouteAssignment(input.routingMetadata, assessment, assignment) };
}
