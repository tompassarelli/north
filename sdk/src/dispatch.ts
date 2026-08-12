import { randomUUID } from "node:crypto";
import {
  getThreadFacts, getChildren, normalizeNorthEntityId, type Fact,
} from "./north-client";
import { deriveManagedDispatchPosture, buildPrompt } from "./posture";
import { SerializedWireEventCommitter, StreamWriter } from "./stream-writer";
import { RunArtifactStore } from "./run-artifacts";
import {
  harnessCompositionEvidence, harnessOptions, renewHarnessPresence, DEFAULT_SYSTEM_PROMPT,
  type Effort, type HarnessCompositionEvidence,
} from "./harness";
import {
  createExecutionActivityEmitter, forwardExecutionActivity,
} from "./execution-activity";
import {
  inputChannel,
  LiveFeedReapTimeoutError,
  subscribeFeed,
} from "./coordination";
import { newRunId, recordWireRunTelemetry } from "./telemetry";
import { runEstimateFromThreadFacts, type RunEstimateSnapshot } from "./run-estimate";
import { publishWireEvents, wireLedgerSummary } from "./run-ledger";
import { causeChain, deathReason, notifyDeath } from "./death";
import {
  describeWatchdogAbortEvidence, withStallWatchdog, stallMs, notifyStall, notifyTurnCap,
  type WatchdogAbortEvidence,
} from "./watchdog";
import { bgContinuationMessage, maxBgContinuations } from "./bgtasks";
import {
  assessChildFinalization, childContinuationMessage, childDispatchMessage, childReductionMessage,
  continuationRaceOutcome, decideChildTurnEnd, initialChildContinuationState, notifyEarlyExitChildren,
  requiredDirectChildCount, settleChildren,
  type ChildSettlement, type OrchestratorContinuationKind,
} from "./children";
import {
  bespokeContractFingerprint, writeAgentFacts, writeAgentTerminal, updateAgentRoute,
  userAnchoredPath,
} from "./identity";
import { BESPOKE_FINGERPRINT_DOMAIN, BESPOKE_FINGERPRINT_VERSION } from "./bespoke-contract";
import {
  formatProviderAuthoritySurface, providerLiveInput, routedQuery, selectProvider,
  selectProviderForExecution, providerRetrySafeTerminalDetail, ProviderRetrySafeError,
  type ProviderAuthoritySurface, type ProviderPreference, type RoutedQueryArguments,
} from "./providers";
import { resolveTier, type SemanticTier } from "./providers/catalog";
import type { RoutingRequest } from "./routing-metadata";
import { admitRoutingRequest, routingRequestFromEnv } from "./routing-admission";
import {
  orchestrationCapabilities,
} from "./orchestration-staffing";
import { refreshAccountUsages } from "./account-usage";
import { resolveDispatchWorkingDirectory } from "./dispatch-context";
import {
  claimDispatchDriver, DispatchDriverReleaseError, type DispatchDriverOptions,
} from "./dispatch-driver";
import {
  admitResourceEnvelope, completeResourceEnvelope, envelopeContextFromEnv,
  reserveResourceEnvelopeRetry, ResourceEnvelopeExceededError, type EnvelopeAdmission,
} from "./resource-envelopes";
import { assertCoordinationAuthority } from "./topology-authority";
import {
  admitManagedDispatchAuthority, admitPinnedProvider,
} from "./execution-admission";
import {
  classifyExecutionTerminal,
  EMPTY_RESULT_OUTCOME,
  isEmptyResultTerminal, NO_PROVIDER_TERMINAL_DETAIL, RUN_TOKEN_BUDGET_LIMITED_OUTCOME,
  wireTerminalDecision,
} from "./execution-outcome";
import {
  emptyResultRepairMode, successfulEmptyResultRepairInput,
} from "./empty-result-repair";
import {
  makeExecutionFold,
  type ExecutionFoldSnapshot,
} from "./execution-fold";
import {
  encodeWireJsonlLine,
  isIntermediateProviderSessionReplacement,
  WireEventWriter,
  wireRunId,
  type WireEvent,
  type WireQuery,
  type WireTurnEvidence,
} from "./wire";
import {
  notifyTerminalSettlement, TerminalPublicationBudget, type TerminalNotification,
} from "./terminal-notification";
import { assessThreadDelivery, type DeliveryAssessment } from "./delivery-verification";
import {
  loadDeliveryRunState, newDeliveryRunContext, reserveDeliveryRun,
  reserveDeliveryRunWithRecovery, resolveDeliveryRunState, resolveThreadFacts,
  type DeliveryReservation, type DeliveryRunContext, type DeliveryRunState,
  type DeliveryReservationRecoveryOptions, type DeliveryRunStateLoadOptions,
  type ThreadFactsLoadOptions,
} from "./delivery-evidence";
import { takeDispatchTestRuntime } from "./internal/test-runtime";
import {
  ManagedLiveInputRoute,
  prepareManagedTerminalFollowUp,
} from "./live-input-route";
import {
  resolveStrugglePolicy,
  assertExpectedStrugglePolicy,
  type StrugglePolicy,
} from "./struggle";
import {
  judgmentGradeFromThreadFacts,
  type JudgmentGradeSnapshot,
} from "./judgment-grade";
import {
  managedRunTokenBudgetHandoff, managedRunTokenTarget, ManagedQueryTermination,
  type HostTerminationRegistrar,
} from "./query-lifecycle";
import {
  admitRoutingEconomics, type AdmittedRoutingEconomics,
  type RoutingAssessment, type RoutingPinEvidence,
} from "./routing-economics";
import { decideManagedLearning } from "./managed-learning";
import type { LearningAssignment } from "./learning-regime";
import {
  publishLearningAssignment,
  type LearningAssignmentPublicationStatus,
} from "./learning-assignment-writer";
import { buildRunEnvelope } from "./composition-receipt";
import { unknownMcpActivity } from "./tool-activity";
import { unknownNativeCommandActivity } from "./native-command-activity";
import {
  wireModelAvailabilityReceipt,
  type WireRunProvenance,
} from "./run-provenance";

const PLAN_TOOLS = ["Read", "Grep", "Glob", "Bash"];
const EXEC_TOOLS = ["Read", "Edit", "Write", "Bash", "Grep", "Glob"];
const SURVEY_TOOLS = ["Read", "Grep", "Glob"];

function latestTurnEvidence(state: ExecutionFoldSnapshot): WireTurnEvidence | undefined {
  return state.turnEvidence[state.turnEvidence.length - 1];
}

interface DispatchResult {
  threadId: string;
  posture: "unplanned" | "atomic" | "composite";
  result: string;
}

export interface DispatchDependencies {
  /** Complete per-subtask request; programmatic callers never inherit ambient routing. */
  routingMetadata: RoutingRequest;
  routingAssessment?: RoutingAssessment;
  pinEvidence?: RoutingPinEvidence;
  /** Explicit child identity for a programmatic handoff; never inferred from a parent. */
  agentId?: string;
  /** Exact-accounted inter-call tripwire; a completing call may overshoot it. */
  tokenTarget?: number;
}

interface DispatchRuntime {
  claimDriver?: typeof claimDispatchDriver;
  driverOptions?: DispatchDriverOptions;
  queryFn?: (args: RoutedQueryArguments) => WireQuery;
  loadThreadFacts?: typeof getThreadFacts;
  loadChildren?: typeof getChildren;
  deliveryRuntime?: {
    reserve: (context: DeliveryRunContext) => DeliveryReservation;
    load: (runId: string) => DeliveryRunState;
    /** Bounded pre-provider writer relaunch shape; tests inject timing only. */
    reserveOptions?: DeliveryReservationRecoveryOptions;
    /** Bounded retry shape for the finalize-time load; tests inject it. */
    loadOptions?: DeliveryRunStateLoadOptions;
  };
  /** Bounded retry shape for the finalize-time thread-facts load; tests inject it. */
  threadFactsLoadOptions?: ThreadFactsLoadOptions;
  childSettlementReader?: (agentId: string) => ChildSettlement;
  feedSubscriber?: typeof subscribeFeed;
  registerTermination?: HostTerminationRegistrar;
  refreshAccountUsages?: typeof refreshAccountUsages;
  admitResourceEnvelope?: typeof admitResourceEnvelope;
  completeResourceEnvelope?: typeof completeResourceEnvelope;
  /** Subprocess to `bin/north`, which resolves babashka off PATH — and a hermetic
   * fixture owns PATH. Production never injects. */
  admitDispatchAuthority?: typeof admitManagedDispatchAuthority;
  releaseDriver?: (driver: DispatchDriverClaim) => boolean | Promise<boolean>;
  publishLearningAssignment?: (
    runId: string, assignment: LearningAssignment,
  ) => Promise<LearningAssignmentPublicationStatus>;
}

interface DispatchDriverClaim {
  release(): boolean;
}

const DISPATCH_DEPENDENCY_FIELDS = new Set([
  "routingMetadata", "routingAssessment", "pinEvidence", "agentId", "tokenTarget",
]);

let bootstrapLegacyPinCompatibilityGranted = false;

function allowlistedDispatchDependencies(
  value: DispatchDependencies,
): DispatchDependencies {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error("managed North dispatch request must be an object");
  const admitted: Record<string, unknown> = {};
  for (const [field, descriptor] of Object.entries(
    Object.getOwnPropertyDescriptors(value),
  )) {
    if (!DISPATCH_DEPENDENCY_FIELDS.has(field))
      throw new Error(`managed North dispatch request has unknown field ${field}`);
    if (descriptor.get || descriptor.set)
      throw new Error(`managed North dispatch request field ${field} must be a data property`);
    admitted[field] = descriptor.value;
  }
  return admitted as unknown as DispatchDependencies;
}

interface DispatchAgentIdOptions {
  agentId?: string;
  driverOptions?: DispatchDriverOptions;
}

export function createDispatchAgentId(threadId: string, now = Date.now(), uuid = randomUUID()): string {
  const threadFragment = threadId.replace(/[^a-z0-9]/gi, "").slice(-12) || "thread";
  return `sdk-${threadFragment}-${now.toString(36)}-${uuid}`;
}

export function selectDispatchAgentId(
  threadId: string,
  dependencies: DispatchAgentIdOptions = {},
): string {
  if (dependencies.agentId) return dependencies.agentId;
  const preclaimed = dependencies.driverOptions?.preclaimed
    ?? process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED === "1";
  if (preclaimed && process.env.AGENT_ID) return process.env.AGENT_ID;
  return createDispatchAgentId(threadId);
}

async function runDispatch(
  threadId: string,
  judgmentGrade: JudgmentGradeSnapshot,
  strugglePolicy: StrugglePolicy,
  runEstimate: RunEstimateSnapshot | undefined,
  envelopeAdmission?: EnvelopeAdmission,
  hydratedMetadata?: RoutingRequest,
  routingEconomics?: AdmittedRoutingEconomics,
  hydratedWorkingDirectory?: string,
  hydratedAgentId?: string,
  queryFn?: (args: RoutedQueryArguments) => WireQuery,
  hydratedFacts?: Fact[],
  hydratedChildren?: string[],
  loadTerminalFacts: typeof getThreadFacts = getThreadFacts,
  deliveryRuntime?: DispatchRuntime["deliveryRuntime"],
  childSettlementReader: (agentId: string) => ChildSettlement = settleChildren,
  feedSubscriber: typeof subscribeFeed = subscribeFeed,
  termination: ManagedQueryTermination = new ManagedQueryTermination(),
  preflightRuntime: Pick<
    DispatchRuntime,
    "refreshAccountUsages" | "threadFactsLoadOptions" | "publishLearningAssignment"
  > = {},
  learningAssignment?: LearningAssignment,
): Promise<DispatchResult> {
  const routingMetadata = hydratedMetadata;
  if (!routingMetadata) throw new Error("managed North dispatch execution requires explicit routingMetadata");
  if (!routingEconomics) throw new Error("managed North dispatch execution requires routing economics admission");
  const role = routingMetadata.role!;
  const capabilities = orchestrationCapabilities(routingMetadata);
  const facts = hydratedFacts ?? getThreadFacts(threadId);
  if (!facts.length) {
    throw new Error(`Thread @${threadId} not found or has no facts`);
  }

  const children = hydratedChildren ?? getChildren(threadId);
  const hasChildren = children.length > 0;
  const posture = deriveManagedDispatchPosture(
    facts, hasChildren, routingMetadata.topology,
  );

  // Judgment grade is the dispatcher's immutable S/M/L estimate of judgment
  // saturation, not the worker's. It feeds aggregate calibration. Warn (teach,
  // never block or inject) when a committed thread lacks it. Bands live in
  // docs/provider-architecture.md.
  if (posture.committed && judgmentGrade.status === "unavailable") {
    console.log(`[dispatch] ⚠ @${threadId} committed but has NO judgment_grade — set s|m|l (S≤3 / M 4-11 / L≥12 expected decision points) so the detector can calibrate`);
  } else if (judgmentGrade.status === "invalid") {
    console.log(`[dispatch] ⚠ @${threadId} has malformed legacy judgment_grade evidence — replace it with exact s|m|l before calibration`);
  }

  if (posture.hasOutcome) {
    return { threadId, posture: "atomic", result: "already done" };
  }

  const workingDirectory = hydratedWorkingDirectory ?? resolveDispatchWorkingDirectory(facts);

  const prompt = buildPrompt(threadId, posture, facts);
  const postureTools = posture.atomic
    ? EXEC_TOOLS
    : posture.planned
      ? SURVEY_TOOLS
      : PLAN_TOOLS;

  const postureLabel = !posture.planned
    ? "unplanned"
    : posture.atomic
      ? "atomic"
      : "composite";

  const agentId = hydratedAgentId ?? createDispatchAgentId(threadId);
  let runId = newRunId(agentId);
  if (!learningAssignment)
    throw new Error("managed North dispatch execution requires a learning assignment");
  const assignmentWriter = preflightRuntime.publishLearningAssignment
    ?? (queryFn ? async () => "recorded" as const : publishLearningAssignment);
  const publishAssignmentForRun = async (assignmentRunId: string): Promise<void> => {
    if (await assignmentWriter(assignmentRunId, learningAssignment) !== "recorded") {
      throw new Error("managed North dispatch requires a durable pre-provider learning assignment");
    }
  };
  await publishAssignmentForRun(runId);
  const runContext = newDeliveryRunContext(runId, threadId, agentId);
  const runtime: DispatchRuntime["deliveryRuntime"] = deliveryRuntime ?? (queryFn ? undefined : {
    reserve: reserveDeliveryRun,
    load: loadDeliveryRunState,
  });
  let deliveryReservation: DeliveryReservation | undefined;
  let deliveryReservationReady = false;
  const requestedTier = routingMetadata.tier ?? process.env.AGENT_TIER as SemanticTier | undefined;
  const requestedReasoning = (routingMetadata.reasoning ?? process.env.AGENT_EFFORT) as Effort | undefined;
  const providerPreference = process.env.AGENT_PROVIDER as ProviderPreference | undefined ?? "auto";
  const targetPreference = process.env.AGENT_TARGET;
  const requestedModel = process.env.AGENT_MODEL;
  const routingRequest = { provider: providerPreference, target: targetPreference };
  if (!queryFn) {
    admitPinnedProvider(providerPreference, capabilities);
  }
  const routingContext = { tier: requestedTier, reasoning: requestedReasoning,
    model: requestedModel, stableKey: agentId, capabilities, signal: termination.signal };
  let routing;
  if (queryFn) {
    routing = selectProvider(routingRequest, undefined, routingContext);
  } else {
    try {
      routing = await selectProviderForExecution(
        routingRequest,
        undefined,
        routingContext,
        preflightRuntime.refreshAccountUsages
          ? { refreshAccountUsages: preflightRuntime.refreshAccountUsages }
          : {},
      );
    } catch (error) {
      // Provider refresh cancellation is an internal control edge. If the host
      // caused it, retain the host signal as the public lifecycle terminal.
      termination.throwIfTerminated();
      throw error;
    }
  }
  termination.throwIfTerminated();
  const resolved = resolveTier(routing.provider, requestedTier,
    requestedModel, requestedReasoning);
  const composition = routingMetadata.composition!;
  const identityBase = {
    kind: "lane",
    role,
    compositionKind: composition.kind,
    compositionId: composition.id,
    compositionOverrides: composition.kind === "preset" ? composition.overrides : undefined,
    compositionOverrideReason: composition.kind === "preset" ? composition.overrideReason : undefined,
    compositionNearestPreset: composition.kind === "bespoke" ? composition.nearestPreset : undefined,
    compositionBespokeReason: composition.kind === "bespoke" ? composition.bespokeReason : undefined,
    compositionPromotionCandidate: composition.kind === "bespoke" ? composition.promotionCandidate : undefined,
    compositionContractFingerprint: composition.kind === "bespoke"
      ? bespokeContractFingerprint(composition.contract) : undefined,
    compositionContractFingerprintVersion: composition.kind === "bespoke"
      ? BESPOKE_FINGERPRINT_VERSION : undefined,
    compositionContractFingerprintDomain: composition.kind === "bespoke"
      ? BESPOKE_FINGERPRINT_DOMAIN : undefined,
    repo: userAnchoredPath(workingDirectory),
    goal: posture.title,
    coordinator: process.env.AGENT_COORDINATOR,
  } as const;
  const initialLiveInput = providerLiveInput(routing.provider);
  const ch = inputChannel(prompt);
  termination.attachInput(() => { try { ch.end(); } catch { /* already closed */ } });
  const liveInputRoute = new ManagedLiveInputRoute(
    agentId,
    identityBase,
    {
      provider: routing.provider,
      providerTarget: routing.target,
      liveInput: initialLiveInput,
      model: resolved.model,
      effort: resolved.effort,
    },
    (message) => ch.push(message),
    feedSubscriber,
  );
  await writeAgentFacts(agentId, { ...identityBase, model: resolved.model,
    provider: routing.provider, providerTarget: routing.target,
    liveInput: initialLiveInput, ...liveInputRoute.initialProjection(),
    effort: resolved.effort });
  const activeRoute = () => ({
    provider: routing.provider,
    providerTarget: routing.target,
    liveInput: providerLiveInput(routing.provider),
    model: routing.resolvedModel ?? resolved.model,
    effort: routing.resolvedEffort ?? resolved.effort,
  });
  const refreshIdentityRoute = (required = false) => {
    liveInputRoute.refresh(activeRoute(), required);
  };

  console.log(`[dispatch] @${threadId} — ${posture.title}`);

  let result = "";
  let outcome = "ran";
  const executionFold = makeExecutionFold(strugglePolicy);
  let wireWriter: WireEventWriter | undefined;
  let stream: StreamWriter | undefined;
  let wireCommitter: SerializedWireEventCommitter | undefined;
  let nextObservedSequence = 0;
  let announcedCompactions = 0;
  const observeWireEvent = async (event: WireEvent) => {
    if (!wireWriter || !wireCommitter) throw new Error("wire event observed before run admission");
    const canonical = wireWriter.events()[nextObservedSequence];
    let matchesCanonical = canonical === event;
    if (!matchesCanonical && canonical) {
      try {
        matchesCanonical = encodeWireJsonlLine(canonical) === encodeWireJsonlLine(event);
      } catch { /* An invalid yielded event cannot equal the writer-owned event. */ }
    }
    if (!canonical || !matchesCanonical) {
      throw new Error("provider yielded an event that differs from its shared writer canonical event");
    }
    await wireCommitter.commitThrough(canonical);
    const observation = executionFold.observe(canonical);
    nextObservedSequence += 1;
    return observation;
  };
  const observeCommittedWireEvents = async (): Promise<void> => {
    if (!wireWriter || !wireCommitter) return;
    const events = wireWriter.events();
    while (nextObservedSequence < events.length) {
      const event = events[nextObservedSequence]!;
      await wireCommitter.commitThrough(event);
      executionFold.observe(event);
      nextObservedSequence += 1;
    }
  };
  const startWireRun = async (): Promise<WireEventWriter> => {
    if (wireWriter) return wireWriter;
    const opened = await StreamWriter.open(agentId);
    const writer = new WireEventWriter({ runId: wireRunId(runId) });
    stream = opened;
    wireWriter = writer;
    wireCommitter = new SerializedWireEventCommitter(writer, opened);
    const started = writer.append({
      kind: "run.started",
      lifecycle: "running",
      owner: agentId,
    });
    await observeWireEvent(started);
    return writer;
  };
  // Public wire detail is North-owned and bounded. Recursive provider causes
  // remain only in local diagnostics below.
  let preflightDetail: string | undefined;
  // Same discipline for a provider_error terminal: the error payload the frame
  // carried, rendered once and carried onto @run (thread 019f9cec).
  let providerErrorDetail: string | undefined;
  let deadlineExceededDetail: string | undefined;

  // Real-time coordination: run the prompt in streaming-input mode so peers can inject
  // pings only when the admitted provider can consume turns after its initial prompt.
  // Stream watchdog (thread 019f4d54): wrap the SDK iterator so a stall (no message for
  // N min while the query is open) is caught — the iterator neither yields nor throws on
  // a hang, so the catch below would never fire. N min silence -> stalled fact + ping;
  // 2N -> abort + outcome=stalled + a durable death fact. Terminal peer wakes
  // are deferred until the terminal and run publications settle.
  const coordHandle = process.env.AGENT_COORDINATOR;
  const window = stallMs();
  const executionActivity = createExecutionActivityEmitter();
  let watchdogAbort: WatchdogAbortEvidence | undefined;
  let stopProviderActivity = () => {};
  let terminalSignal: Pick<TerminalNotification, "detail" | "subject"> = {};
  const terminalAuxiliaryWrites: Array<(timeoutMs: number) => void> = [];
  let liveInputFreezeError: unknown;
  // Background-task refusal (thread 019f4ed2, half a): don't finalize on the first
  // `result` while a harness-tracked background task is live — see bgtasks.ts.
  let bgContinuations = 0;
  const orchestrator = routingMetadata.topology === "orchestrator";
  let childContinuation = initialChildContinuationState(
    requiredDirectChildCount(routingMetadata),
  );
  // Orchestrator continuation race (thread 019f8ec5): the obligation whose
  // continuation was injected at the last turn-end and not yet discharged by a
  // genuine (non-empty) provider result. See the guard in the result loop.
  let pendingContinuation: OrchestratorContinuationKind | undefined;
  let emptyResultRepairAttempted = false;
  let emptyResultRepairContinuation = false;
  // Streaming providers own private continuation identity. North asks the
  // same semantic query for another turn without observing a raw session id.
  const resumeContinuations = orchestrator
    && providerLiveInput(routing.provider) === "streaming";
  let initialComposition: HarnessCompositionEvidence | undefined;
  let admittedRoute: {
    provider: "anthropic" | "openai";
    evidence?: HarnessCompositionEvidence;
    authority?: ProviderAuthoritySurface;
  } | undefined;
  let activeExecutionQuery: WireQuery | undefined;
  let queryCloseError: unknown;
  // Error boundary (thread 019f2800): the SDK runs the turn in a subprocess; if it dies
  // (OOM SIGKILL / parent SIGTERM / idle Transport-closed) the generator THROWS exitError
  // here. catch -> outcome "died" + durable agent_death facts on this thread and @swarm;
  // finally -> ALWAYS stop the feed and close the channel. The peer wake is emitted only
  // after the committed terminal and run publication attempts have settled.
  try {
  try {
    // Reserve only at the last pre-provider seam. Earlier routing/admission
    // failures must not strand undiscoverable reservation-only subjects.
    try {
      if (runtime) {
        deliveryReservation = reserveDeliveryRunWithRecovery(
          runContext,
          runtime.reserve,
          {
            ...runtime.reserveOptions,
            onRetry: (error, nextAttempt, maxAttempts, backoffMs) => {
              console.error(
                `[delivery] @${runId} reservation writer failed before provider; `
                + `relaunching the same reservation identity after ${backoffMs}ms `
                + `(attempt ${nextAttempt}/${maxAttempts}): ${error.message}`,
              );
            },
          },
        );
        if (!deliveryReservation) throw new Error("reservation acknowledgement unavailable");
        deliveryReservationReady = true;
      }
    } catch (error) {
      const abandonedRunId = runId;
      runId = newRunId(agentId);
      await publishAssignmentForRun(runId);
      // Loud + diagnosable (thread 019f9063): surface the writer's exact
      // rejection instead of a uniform "unavailable" that hides freshness,
      // thread-identity, and malformed-ack failures alike.
      console.error(
        `[delivery] @${abandonedRunId} reservation unavailable; rotating telemetry to @${runId} `
        + `and leaving delivery unverified: ${(error as Error)?.message ?? String(error)}`,
      );
    }
    const artifacts = new RunArtifactStore(runId);
    const agentOptions = harnessOptions({
      self: agentId,
      extraTools: postureTools,
      model: resolved.model,
      effort: resolved.effort,
      provider: routing.provider,
      modelAvailability: {
        exactModelPinned: requestedModel !== undefined,
        targetId: routing.target,
        receipt: routing.modelAvailabilityReceipts?.[routing.target],
      },
      routingMetadata,
      role,
      posture: routingMetadata.posture,
      cwd: workingDirectory,
      deliveryRun: deliveryReservationReady ? runContext : undefined,
      artifactDirectory: artifacts.directory,
      systemPrompt: `You are a north agent executing thread @${threadId}. ${DEFAULT_SYSTEM_PROMPT}`,
      abortController: termination.abortController,
    });
    initialComposition = harnessCompositionEvidence(agentOptions);
    console.log(
      `[dispatch] posture: ${postureLabel}, provider: ${routing.provider}, `
      + `target: ${routing.target} (${routing.reason})`,
    );
    if (queryFn && feedSubscriber !== subscribeFeed)
      await liveInputRoute.activate(activeRoute());
    const writer = await startWireRun();
    const queryArgs = {
      input: ch.stream(),
      options: agentOptions,
      writer,
      eventCommitter: wireCommitter,
      artifacts,
    };
    termination.throwIfTerminated();
    const q = queryFn
      ? queryFn(queryArgs)
      : routedQuery(routing, queryArgs, requestedTier,
        async (transition) => {
          await liveInputRoute.beforeFallback(
            transition,
            () => reserveResourceEnvelopeRetry(envelopeAdmission),
          );
        },
        async (decision, evidence, authority) => {
          admittedRoute = {
            provider: decision.provider,
            ...(evidence === undefined ? {} : { evidence }),
            ...(authority === undefined ? {} : { authority }),
          };
          if (authority) {
            await liveInputRoute.activate({
              ...activeRoute(),
              liveInput: authority.liveInput,
            }, authority.liveInput === "turn-framed");
            console.log(
              `[dispatch] effective authority: ${formatProviderAuthoritySurface(authority)}`,
            );
          }
        });
    activeExecutionQuery = q;
    termination.attachQuery(q);
    if (q.executionTransport === "managed-app-server")
      await liveInputRoute.activate(activeRoute(), true);
    stopProviderActivity();
    stopProviderActivity = forwardExecutionActivity(
      q.executionActivity,
      executionActivity,
    );
    turnLoop: while (true) {
    let privateContinuation: string | undefined;
    const watched = withStallWatchdog(q[Symbol.asyncIterator](), {
      stallMs: window,
      onStall: (mins) => notifyStall(agentId, mins, { coordinator: coordHandle }),
      onAbort: (evidence) => { watchdogAbort = evidence; },
      activitySources: [executionActivity.source],
    });
    for await (const event of watched) {
      const observation = await observeWireEvent(event);
      if (observation.activityKind) {
        executionActivity.record("outer", observation.activityKind);
        renewHarnessPresence(agentOptions);
      }
      refreshIdentityRoute();
      if (observation.state.compactions > announcedCompactions) {
        announcedCompactions = observation.state.compactions;
        console.error(
          `[harness] @agent:${agentId} context compaction #${announcedCompactions}`,
        );
      }
      if (observation.backgroundTask?.kind === "settled") bgContinuations = 0;

      const struggleTrigger = observation.struggleTrigger;
      if (struggleTrigger) {
        console.error(
          `[struggle] @agent:${agentId} sensor fired: ${struggleTrigger} `
          + `(model calls ${observation.state.run.usage.lifetime.modelCalls}, `
          + `${observation.state.struggle.errorCount} tool error(s)) `
          + "— recorded as execution-axis evidence, no in-flight change",
        );
      }

      if (event.essential && event.kind === "message.recorded"
          && event.role === "assistant" && event.stage === "delta"
          && typeof event.content === "string") {
        process.stdout.write(event.content);
      }

      if (event.essential && event.kind === "model-call.completed") {
        if (isIntermediateProviderSessionReplacement(event)) continue;
        result = observation.state.lastCompletedAssistantOutput ?? "";
        const tokenBudget = termination.observeCompletedCallUsage(observation.state.run);
        if (tokenBudget?.state === "budget_limited") {
          outcome = RUN_TOKEN_BUDGET_LIMITED_OUTCOME;
          terminalSignal = {
            subject: "TOKEN TARGET",
            detail: JSON.stringify(managedRunTokenBudgetHandoff(tokenBudget)),
          };
          break;
        }
        const cap = event.errorCode === "provider_max_turns"
          ? "provider_max_turns"
          : event.errorCode === "provider_budget_exhausted"
            || event.errorCode === "provider_structured_output_retries_exhausted"
            ? event.errorCode : undefined;
        if (cap !== undefined) {
          outcome = cap === "provider_max_turns" ? "max_turns" : "capped";
          const partial = result.trim()
            ? `partial: ${result.trim().slice(0, 200)}`
            : "no partial result";
          const detail = `${cap} — ${partial}`;
          terminalSignal = { subject: "TURN CAP", detail };
          terminalAuxiliaryWrites.push((timeoutMs) =>
            notifyTurnCap(agentId, detail, {}, timeoutMs)
          );
          break;
        }
        deadlineExceededDetail = observation.state.deadlineExceededDetail;
        if (deadlineExceededDetail) {
          outcome = "deadline_exceeded";
          console.error(
            `[deadline_exceeded] @agent:${agentId} process=deadline_exceeded detail=${deadlineExceededDetail}`,
          );
          terminalSignal = { subject: "DEADLINE EXCEEDED", detail: deadlineExceededDetail };
          break;
        }
        if (event.status !== "succeeded") {
          outcome = "provider_error";
          providerErrorDetail = observation.state.providerErrorDetail
            ?? "model-call terminal failed without diagnostic evidence";
          console.error(`[provider_error] @agent:${agentId} ${providerErrorDetail}`);
          terminalSignal = { subject: "AGENT BLOCKED", detail: providerErrorDetail };
          break;
        }
        // The usage observation above latches L5's token tripwire. The shared gate
        // checks it with the durable hard deadline before and after inbox replay.
        await prepareManagedTerminalFollowUp(liveInputRoute, termination);
        if (ch.pending() === 0) {
          // Orchestrator continuation race (thread 019f8ec5): a continuation
          // injected at a prior turn-end asks the provider for ANOTHER genuine
          // turn, but the Anthropic session may already be tearing down after
          // its final message. The continuation then lands on a closing stream
          // and the provider answers with a degenerate empty-success terminal,
          // which decideChildTurnEnd would read as a completed continuation and
          // finalize as ran_empty. An outstanding continuation is discharged
          // ONLY by a non-empty result; an empty terminal here is the race, so
          // record the obligation-specific blocked outcome loudly instead.
          if (orchestrator && pendingContinuation && result.trim() === "") {
            outcome = continuationRaceOutcome(pendingContinuation);
            console.error(
              `[harness] @agent:${agentId} orchestrator ${pendingContinuation} continuation answered by an empty provider terminal — closing-stream race, recording ${outcome} (never ran_empty)`,
            );
            break;
          }
          pendingContinuation = undefined; // a genuine result discharges the obligation
          // Refuse to exit while background tasks are live (half a) — inject a
          // continuation + keep looping so the SDK auto-continues to task settlement.
          if (observation.state.pendingBackgroundTasks.length > 0
              && bgContinuations < maxBgContinuations()) {
            bgContinuations++;
            const live = observation.state.pendingBackgroundTasks;
            console.error(`[harness] @agent:${agentId} refusing turn-end exit — ${live.length} live background task(s): ${live.join(", ")} (continuation ${bgContinuations}/${maxBgContinuations()})`);
            const continuation = bgContinuationMessage(live);
            if (resumeContinuations) privateContinuation = continuation;
            else ch.push(continuation);
            continue; // do NOT finalize; keep the query loop alive
          }
          if (observation.state.pendingBackgroundTasks.length > 0) {
            console.error(`[harness] @agent:${agentId} continuation cap (${maxBgContinuations()}) reached with ${observation.state.pendingBackgroundTasks.length} task(s) still live — blocking terminal`);
            outcome = "background_tasks_incomplete";
            terminalSignal = {
              subject: "AGENT BLOCKED",
              detail: `${observation.state.pendingBackgroundTasks.length} background task(s) remained open`,
            };
            break;
          }
          if (orchestrator) {
            const decision = decideChildTurnEnd(
              childContinuation,
              childSettlementReader(agentId),
              maxBgContinuations(),
            );
            childContinuation = decision.state;
            if (decision.action === "continue") {
              let continuation: string;
              if (decision.reason === "children_live") {
                console.error(
                  `[harness] @agent:${agentId} refusing orchestrator turn-end — ${decision.live.length} live child lane(s): ${decision.live.join(", ")} (no-progress ${decision.attempt}/${decision.cap})`,
                );
                continuation = childContinuationMessage(decision.live);
              } else if (decision.reason === "child_dispatch_required") {
                console.error(
                  `[harness] @agent:${agentId} requiring direct-child dispatch — ${decision.children.length}/${decision.required} child lane(s) observed (no-progress ${decision.attempt}/${decision.cap})`,
                );
                continuation = childDispatchMessage(decision.children, decision.required);
              } else {
                console.error(
                  `[harness] @agent:${agentId} requiring post-settlement reduction — ${decision.children.length} settled child lane(s): ${decision.children.join(", ")}`,
                );
                continuation = childReductionMessage(decision.children);
              }
              // Remember the outstanding obligation so a degenerate empty
              // terminal on the next turn (a closing-stream race) blocks
              // explicitly rather than falsely discharging the continuation.
              pendingContinuation = decision.reason;
              if (resumeContinuations) {
                if (!q.continueTurn) {
                  outcome = "provider_error";
                  providerErrorDetail = "active provider cannot retain a private continuation turn";
                  terminalSignal = { subject: "AGENT BLOCKED", detail: providerErrorDetail };
                  break;
                }
                console.error(
                  `[harness] @agent:${agentId} opening a provider-neutral resumed continuation turn`,
                );
                privateContinuation = continuation;
                continue;
              }
              ch.push(continuation);
              continue;
            }
            if (decision.action === "block") {
              outcome = decision.reason === "child_reconciliation_unavailable"
                ? "child_reconciliation_unavailable"
                : decision.reason === "child_set_regressed"
                  ? "orchestrator_child_set_inconsistent"
                  : decision.reason === "child_dispatch_continuation_cap"
                    ? "orchestrator_child_obligation_unmet"
                    : "orchestrator_children_incomplete";
              const detail = decision.missing?.length
                ? ` (missing previously observed: ${decision.missing.join(", ")})`
                : decision.live?.length
                  ? ` (${decision.live.join(", ")})`
                  : decision.required !== undefined
                    ? ` (${decision.children?.length ?? 0}/${decision.required} direct children)`
                  : "";
              console.error(
                `[harness] @agent:${agentId} orchestrator completion blocked: ${decision.reason}${detail}`,
              );
              break;
            }
          }
          if (!emptyResultRepairAttempted
              && isEmptyResultTerminal(observation.state.run)
              && termination.emptyResultRepairAllowed()) {
            const repairMode = emptyResultRepairMode(q);
            if (repairMode !== undefined) {
              emptyResultRepairAttempted = true;
              const repairInput = successfulEmptyResultRepairInput();
              console.error(
                `[empty-result] @agent:${agentId} opening one same-session corrective turn`,
              );
              if (repairMode === "streaming") {
                emptyResultRepairContinuation = true;
                privateContinuation = repairInput;
              } else {
                termination.throwIfTerminated();
                ch.push(repairInput);
              }
              continue;
            }
          }
          break; // task done + no pending peer ping -> finish
        }
      }
    }
    if (privateContinuation !== undefined) {
      if (emptyResultRepairContinuation) {
        // The repair is a private same-session turn. Stop public admission and
        // reject queued messages as unconsumed before passing a bare corrective
        // input, otherwise the armed route can strand mail on an unread channel.
        ch.end();
        try {
          await liveInputRoute.freezeAndUnbind();
        } catch (error) {
          liveInputFreezeError ??= error;
          emptyResultRepairContinuation = false;
          break turnLoop;
        }
        if (!q.continueTurn || !termination.emptyResultRepairAllowed()) {
          emptyResultRepairContinuation = false;
          break turnLoop;
        }
        termination.throwIfTerminated();
        try {
          await q.continueTurn(privateContinuation);
        } catch {
          termination.throwIfTerminated();
          emptyResultRepairContinuation = false;
          break turnLoop;
        }
        emptyResultRepairContinuation = false;
      } else {
        if (!q.continueTurn) {
          outcome = "provider_error";
          providerErrorDetail = "active provider cannot retain a private continuation turn";
          terminalSignal = { subject: "AGENT BLOCKED", detail: providerErrorDetail };
          break turnLoop;
        }
        await q.continueTurn(privateContinuation);
      }
      continue turnLoop;
    }
    break;
    }
    await observeCommittedWireEvents();
    const providerState = executionFold.snapshot();
    if (providerState?.latestModelCallTerminal?.status !== "succeeded"
        && outcome === "ran" && !watchdogAbort) {
      // Iterator completion without an explicit provider terminal is not a
      // successful execution, even when the transport itself closed cleanly.
      outcome = "provider_error";
      // A close-time failure is a DEATH below (it overrides this outcome) and is
      // rendered there; otherwise retain the latest typed model failure.
      providerErrorDetail = providerState?.providerErrorDetail ?? NO_PROVIDER_TERMINAL_DETAIL;
      console.error(`[provider_error] @agent:${agentId} ${providerErrorDetail}`);
      terminalSignal = { subject: "AGENT BLOCKED", detail: providerErrorDetail };
    }
    if (!watchdogAbort && outcome === "ran" && providerState
        && isEmptyResultTerminal(providerState.run)) {
      // A provider success terminal with empty result (0b) is a DEGENERATE
      // completion, not a delivery (thread 019f8300): opus-high extended-thinking
      // turns that hit the output-token ceiling truncate before committing any
      // final text, yet the SDK still yields subtype=success/result="". Make it
      // a distinct LOUD terminal so a zero-deliverable lane never masquerades as
      // AGENT COMPLETE.
      outcome = EMPTY_RESULT_OUTCOME;
      const turns = latestTurnEvidence(providerState)?.count ?? "unknown turn count";
      terminalSignal = {
        subject: "AGENT EMPTY RESULT",
        detail: `provider success terminal with empty result (0b) after ${turns} — no deliverable text committed (likely output-token ceiling hit mid extended-thinking/tool_use)`,
      };
      console.error(`[empty-result] @agent:${agentId} provider success terminal carried 0b result — recording process=ran_empty (loud, non-clean)`);
    }
    if (watchdogAbort) {
      outcome = "watchdog_aborted";
      providerErrorDetail = undefined;
      const detail = describeWatchdogAbortEvidence(watchdogAbort);
      console.error(`[watchdog-abort] @agent:${agentId} ${detail}`);
      const err = new Error(detail);
      terminalSignal = { subject: "AGENT DEATH", detail: deathReason(err) };
      terminalAuxiliaryWrites.push((timeoutMs) =>
        notifyDeath(agentId, err, { thread: threadId }, timeoutMs)
      );
      try { await termination.close(); }
      catch (error) { queryCloseError = error; }
    }
  } catch (err) {
    if (termination.hardCapStatus()) {
      outcome = "session_hard_cap";
    } else if (err instanceof ResourceEnvelopeExceededError) {
      outcome = "resource_envelope_exceeded";
      console.error(`[envelope] @agent:${agentId} ${err.message}`);
    } else if (err instanceof ProviderRetrySafeError) {
      // A spend-guard refusal carries its own terminal outcome; every other
      // retry-safe preflight block stays blocked_preflight.
      const carried = (err as { processOutcome?: unknown }).processOutcome;
      outcome = typeof carried === "string" ? carried : "blocked_preflight";
      preflightDetail = providerRetrySafeTerminalDetail(err);
      console.error(`[${outcome}] @agent:${agentId} ${causeChain(err)}`);
    } else {
      outcome = "died";
      terminalSignal = { subject: "AGENT DEATH", detail: deathReason(err) };
      terminalAuxiliaryWrites.push((timeoutMs) =>
        notifyDeath(agentId, err, { thread: threadId }, timeoutMs)
      );
    }
  } finally {
    stopProviderActivity();
    try { await observeCommittedWireEvents(); }
    catch (error) { queryCloseError ??= error; }
    try {
      await liveInputRoute.freezeAndUnbind();
    } catch (error) {
      liveInputFreezeError = error;
    }
    try { ch.end(); } catch { /* already closed */ }
    // Streaming input can keep the provider subprocess alive even after it has
    // emitted a terminal result. Close the active query exactly once so Bun and
    // the provider CLI cannot survive a completed dispatch.
    try { await termination.close(); }
    catch (error) { queryCloseError = error; }
    try { await observeCommittedWireEvents(); }
    catch (error) { queryCloseError ??= error; }
  }

  const sessionHardCap = termination.hardCapStatus();
  const hostSignal = termination.hostSignal();
  if (sessionHardCap && !watchdogAbort) {
    outcome = "session_hard_cap";
    providerErrorDetail = undefined;
    terminalSignal = {
      subject: "SESSION CAP",
      detail: `managed session reached ${sessionHardCap.hardCapMs}ms hard cap; `
        + `handoff=${sessionHardCap.handoffPath ?? "unavailable"}; `
        + `handoff_index=${sessionHardCap.indexed ? "thread" : "outbox"}`,
    };
  } else if (hostSignal && !watchdogAbort) {
    outcome = "died";
    const error = new Error(`host terminated by ${hostSignal}`);
    terminalSignal = { subject: "AGENT DEATH", detail: deathReason(error) };
  } else if (queryCloseError && !watchdogAbort) {
    outcome = "died";
    const error = queryCloseError instanceof Error
      ? queryCloseError : new Error("provider query cleanup failed");
    terminalSignal = { subject: "AGENT DEATH", detail: deathReason(error) };
  }

  if (liveInputFreezeError && !watchdogAbort && !sessionHardCap) {
    let retrySucceeded = false;
    try {
      await liveInputRoute.freezeAndUnbind();
      retrySucceeded = true;
    } catch { /* the original freeze error remains the terminal authority */ }
    const error = liveInputFreezeError instanceof Error
      ? liveInputFreezeError
      : new Error("managed live-input route could not be frozen");
    if (retrySucceeded && !(error instanceof LiveFeedReapTimeoutError)) {
      liveInputFreezeError = undefined;
    } else {
      outcome = "died";
      terminalSignal = { subject: "AGENT DEATH", detail: deathReason(error) };
      terminalAuxiliaryWrites.push((timeoutMs) =>
        notifyDeath(agentId, error, { thread: threadId }, timeoutMs)
      );
    }
  }

  // Final child gate is deliberately adjacent to terminal publication. It
  // catches a child appearing after the last provider result and treats graph
  // unavailability as unknown, never as an empty child set.
  const finalChildren = childSettlementReader(agentId);
  if (orchestrator && outcome === "ran") {
    const finalization = assessChildFinalization(childContinuation, finalChildren);
    if (!finalization.ok) {
      outcome = finalization.outcome;
      if (finalization.outcome === "orchestrator_child_set_inconsistent") {
        console.error(
          `[harness] @agent:${agentId} CHILD SET REGRESSED: missing previously observed coordinator relation(s) ${finalization.missing?.join(", ") ?? "(unknown)"}; terminal cannot be process=ran`,
        );
      }
    }
  }
  if (finalChildren.kind === "live") {
    terminalAuxiliaryWrites.push((timeoutMs) =>
      notifyEarlyExitChildren(agentId, finalChildren.live, {}, timeoutMs)
    );
    const childDetail =
      `${finalChildren.live.length} live child(ren): ${finalChildren.live.join(",")}`;
    terminalSignal = terminalSignal.subject
      ? {
          ...terminalSignal,
          detail: [terminalSignal.detail, childDetail].filter(Boolean).join("; "),
        }
      : { subject: "EARLY EXIT WITH LIVE CHILDREN", detail: childDetail };
  } else if (orchestrator && finalChildren.kind === "unavailable") {
    console.error(
      `[harness] @agent:${agentId} CHILD SETTLEMENT UNAVAILABLE: ${finalChildren.reason}; terminal cannot be process=ran`,
    );
  } else if (orchestrator && outcome === "orchestrator_reduction_incomplete"
             && finalChildren.kind === "settled") {
    console.error(
      `[harness] @agent:${agentId} CHILD RESULTS UNREDUCED: settled set changed or lacked a completed reduction turn (${finalChildren.children.join(", ")}); terminal cannot be process=ran`,
    );
  } else if (orchestrator && outcome === "orchestrator_child_obligation_unmet"
             && finalChildren.kind === "settled") {
    console.error(
      `[harness] @agent:${agentId} DIRECT-CHILD OBLIGATION UNMET: ${finalChildren.children.length}/${childContinuation.requiredChildren} direct child lane(s) observed; terminal cannot be process=ran`,
    );
  }

  const executionBeforeCleanup = executionFold.snapshot();
  if (outcome === "ran" && executionBeforeCleanup?.pendingBackgroundTasks.length) {
    outcome = "background_tasks_incomplete";
    terminalSignal = {
      subject: "AGENT BLOCKED",
      detail: `${executionBeforeCleanup.pendingBackgroundTasks.length} background task(s) remained open`,
    };
  }

  // Commit the lane's process/delivery terminal (SYNC, digest marker last)
  // before exit. Mirrors spawn.ts at the same reap-avoidance seam.
  refreshIdentityRoute();
  let delivery: DeliveryAssessment | undefined;
  if (outcome === "ran") {
    if (!deliveryReservationReady || !deliveryReservation || !runtime) {
      delivery = {
        deliveryOutcome: "unverified",
        deliveryReason: "delivery_reservation_unavailable_at_finalize",
      };
    } else {
      // Same seam as spawn.ts: retry the LOAD (a contended coordinator is not a
      // verdict), fail closed immediately on a read that found no valid
      // reservation. Thread 019f9cc1.
      const resolution = resolveDeliveryRunState(
        runId,
        (id) => runtime.load(id),
        runtime.loadOptions,
      );
      const runState: DeliveryRunState | undefined = resolution.transientFailure
        ? undefined
        : resolution.state;
      if (!runState?.reservationValid) {
        deliveryReservationReady = false;
        console.error(
          `[delivery] @${runId} `
          + (resolution.transientFailure
            ? `reservation unreadable at finalize after ${resolution.attempts} attempt(s) `
              + `(${resolution.transientFailure})`
            : "reservation invalid at finalize")
          + "; retaining the wire run identity and leaving delivery unverified",
        );
        delivery = {
          deliveryOutcome: "unverified",
          deliveryReason: resolution.transientFailure
            ? "delivery_reservation_load_failed_at_finalize"
            : "delivery_reservation_unavailable_at_finalize",
        };
      } else {
        // Same seam as the reservation load above: a contended coordinator
        // read of the thread's own facts is not a verdict on the thread
        // (thread 019f9e0d, deferred sibling of 019f9cc1). Retry only a load
        // that never spoke; a load that returns (even `[]`) is a content
        // result and stays fail-closed via assessThreadDelivery on attempt 1.
        const threadResolution = resolveThreadFacts(
          threadId,
          (id) => loadTerminalFacts(id),
          preflightRuntime.threadFactsLoadOptions,
        );
        if (threadResolution.transientFailure) {
          console.error(
            `[delivery] @${threadId} thread unreadable at finalize after `
            + `${threadResolution.attempts} attempt(s) (${threadResolution.transientFailure}); `
            + "leaving delivery unverified",
          );
          delivery = {
            deliveryOutcome: "unverified",
            deliveryReason: "delivery_thread_load_failed_at_finalize",
          };
        } else {
          delivery = assessThreadDelivery(
            threadId,
            agentId,
            threadResolution.facts ?? [],
            deliveryReservation.baselineDoneWhen.map(
              (value) => ({ predicate: "done_when", value }),
            ),
            runId,
            runState.evidence,
          );
        }
      }
    }
  }
  const terminal = classifyExecutionTerminal(outcome, delivery);
  const terminalDetail = terminalSignal.detail ?? deadlineExceededDetail
    ?? providerErrorDetail ?? preflightDetail ?? outcome;
  const finalWriter = await startWireRun();
  const wireTerminal = wireTerminalDecision(outcome, terminalDetail, watchdogAbort);
  const wireTerminalEvents = finalWriter.terminate(wireTerminal);
  for (const event of wireTerminalEvents) await observeWireEvent(event);
  await wireCommitter?.commitAll();
  const finalExecution = executionFold.snapshot();
  if (!finalExecution || finalExecution.run.lifecycle === "running"
      || finalExecution.run.lifecycle === "waiting") {
    throw new Error("wire run did not reach its outer terminal");
  }
  const publicationBudget = new TerminalPublicationBudget();
  // Publish the lane terminal before any diagnostic side channel. A slow
  // auxiliary writer may consume only what remains after authoritative state.
  const terminalPublication = writeAgentTerminal(
    agentId,
    terminal,
    publicationBudget.publicationTimeout(1),
    undefined,
    threadId,
  );
  for (const [index, writeAuxiliary] of terminalAuxiliaryWrites.entries()) {
    writeAuxiliary(
      publicationBudget.publicationTimeout(
        terminalAuxiliaryWrites.length - index + 2,
      ),
    );
  }

  const wireEvents = finalWriter.events();
  const finalRoute = activeRoute();
  const finalAdmittedRoute = admittedRoute?.provider === routing.provider
    ? admittedRoute : undefined;
  const promptComposition = finalAdmittedRoute?.evidence
    ?? (routing.fallbackCount === 0 ? initialComposition : undefined);
  const promptReceipt = promptComposition?.promptReceipt;
  const environmentReceipt = promptComposition?.environmentReceipt;
  const finalEffort = finalRoute.effort ?? routingMetadata.reasoning;
  const runEnvelopeReceipt = promptReceipt && environmentReceipt
      && routingMetadata.tier && finalEffort
    ? buildRunEnvelope({
        promptReceipt,
        environmentReceipt,
        assignmentSha256: learningAssignment.manifestSha256,
        tier: routingMetadata.tier,
        effort: finalEffort,
        providerAdapterVersion: "north-managed-adapter:v1",
        providerRuntimeVersion: `bun-${Bun.version}`,
      })
    : undefined;
  const mcpActivity = activeExecutionQuery?.mcpActivity?.()
    ?? unknownMcpActivity("provider-activity-unavailable");
  const nativeCommandActivity = activeExecutionQuery?.nativeCommandActivity?.()
    ?? unknownNativeCommandActivity("provider-activity-unavailable");
  const observedTokenBudget = termination.tokenBudgetStatus();
  const tokenBudget = observedTokenBudget?.state === "budget_limited"
    ? observedTokenBudget
    : termination.observeCompletedCallUsage(finalExecution.run);
  const provenance: WireRunProvenance = {
    posture: postureLabel,
    role,
    provider: routing.provider,
    providerTarget: routing.target,
    providerReason: routing.selectionReason,
    ...(routing.modelAvailabilityReceipts?.[routing.target] === undefined
      ? {} : {
          modelAvailability: wireModelAvailabilityReceipt(
            routing.modelAvailabilityReceipts[routing.target],
          ),
        }),
    requestedProvider: routing.requestedProvider,
    ...(routing.requestedTarget === undefined
      ? {} : { requestedTarget: routing.requestedTarget }),
    ...(requestedTier === undefined ? {} : { requestedTier }),
    ...(requestedReasoning === undefined ? {} : { requestedEffort: requestedReasoning }),
    routingMetadata,
    ...(routingEconomics.assessment === undefined
      ? {} : { routingAssessment: routingEconomics.assessment }),
    routingAdmissionReceipt: routingEconomics.receipt,
    ...(routingEconomics.pinEvidence === undefined
      ? {} : { routingPinEvidence: routingEconomics.pinEvidence }),
    ...(promptComposition === undefined ? {} : { promptComposition }),
    learningAssignment,
    ...(promptReceipt === undefined ? {} : { promptReceipt }),
    ...(environmentReceipt === undefined ? {} : { environmentReceipt }),
    ...(runEnvelopeReceipt === undefined ? {} : { runEnvelopeReceipt }),
    mcpActivity,
    nativeCommandActivity,
    executionSource: "north-managed",
    ...(activeExecutionQuery?.executionTransport === undefined
      ? {} : { executionTransport: activeExecutionQuery.executionTransport }),
    ...(finalAdmittedRoute?.authority === undefined
      ? {} : { effectiveAuthority: finalAdmittedRoute.authority }),
    allocationMode: routing.allocationMode,
    entitlementPressure: routing.entitlementPressure,
    ...(routing.allocationEvidenceByTarget === undefined
      ? {} : { allocationEvidence: routing.allocationEvidenceByTarget }),
    fallbackCount: routing.fallbackCount,
    fallbackPath: routing.fallbackPath,
    fallbackTargetPath: routing.fallbackTargetPath,
    fallbackReasons: routing.fallbackReasons,
    ...(envelopeAdmission === undefined ? {} : {
      envelopeScopes: envelopeAdmission.scopes.map(({ id }) => id),
      envelopeRetries: envelopeAdmission.retries,
      envelopeAdvisories: envelopeAdmission.advisories,
    }),
    processOutcome: terminal.processOutcome,
    ...(tokenBudget === undefined ? {} : { tokenBudget }),
    deliveryOutcome: terminal.deliveryOutcome,
    ...(terminal.deliveryReason === undefined ? {} : { deliveryReason: terminal.deliveryReason }),
    ...(terminal.deliveryProof === undefined ? {} : { deliveryProof: terminal.deliveryProof }),
    ...(runEstimate === undefined ? {} : { runEstimate }),
    judgmentGrade,
    struggleObservation: finalExecution.struggle,
  };
  const wireIdentity = {
    thread: threadId,
    agent: agentId,
    ...(process.env.NORTH_THREAD_ID ? { parentThread: process.env.NORTH_THREAD_ID } : {}),
    ...(coordHandle ? { coordinator: coordHandle } : {}),
  };
  const wireLedgerStatus = await publishWireEvents(
    wireIdentity,
    wireEvents,
    publicationBudget.publicationTimeout(2),
  ).catch(() => "unavailable" as const);
  const runLedger = wireLedgerStatus === "recorded"
    ? wireLedgerSummary(wireEvents) : undefined;
  const runPublication = runLedger === undefined
    ? "unavailable" as const
    : await recordWireRunTelemetry(
        wireIdentity,
        finalExecution.run,
        { status: "recorded", summary: runLedger },
        provenance,
        publicationBudget.publicationTimeout(1),
      );
  notifyTerminalSettlement(
    agentId,
    coordHandle,
    {
      outcome,
      terminal,
      terminalPublication,
      runPublication,
      ...terminalSignal,
    },
    publicationBudget.notificationTimeout(),
  );
  console.log(`\n[dispatch] @${threadId} process=${outcome} delivery=${terminal.deliveryOutcome}`);
  return { threadId, posture: postureLabel, result };
  } finally {
    await stream?.close();
  }
}

let bootstrapAuthorityGranted = false;

export async function dispatch(
  threadIdInput: string,
  dependencies: DispatchDependencies,
): Promise<DispatchResult> {
  const injected = takeDispatchTestRuntime<DispatchRuntime>(dependencies) ?? {};
  (injected.admitDispatchAuthority ?? admitManagedDispatchAuthority)();
  const admitted = allowlistedDispatchDependencies(dependencies);
  managedRunTokenTarget(admitted.tokenTarget);
  const callerTopology = process.env.AGENT_TOPOLOGY;
  if (!bootstrapAuthorityGranted) {
    assertCoordinationAuthority("dispatch", callerTopology);
  }
  const threadId = normalizeNorthEntityId(threadIdInput);
  // Routing admission is the first request-dependent boundary. Even a
  // completed thread must not make an incomplete/hostile managed envelope look
  // accepted, and a preclaimed fast path must not touch driver state first.
  let routingMetadata = admitRoutingRequest(
    admitted.routingMetadata ?? {}, "managed North dispatch",
  );
  let routingEconomics = admitRoutingEconomics({
    request: routingMetadata,
    routingAssessment: admitted.routingAssessment,
    pinEvidence: admitted.pinEvidence,
    provider: process.env.AGENT_PROVIDER,
    target: process.env.AGENT_TARGET,
    model: process.env.AGENT_MODEL,
    allowLegacyMissingPinEvidence: bootstrapLegacyPinCompatibilityGranted,
    surface: "managed North dispatch routing economics",
  });
  // The detector policy is an admission input: reject malformed overrides before
  // any graph claim, clock, resource envelope, or provider-selection side effect.
  const strugglePolicy = resolveStrugglePolicy(routingMetadata.topology!);
  assertExpectedStrugglePolicy(strugglePolicy);
  // Avoid charging an admission for an unknown or already-completed thread.
  const facts = (injected.loadThreadFacts ?? getThreadFacts)(threadId);
  if (!facts.length) throw new Error(`Thread @${threadId} not found or has no facts`);
  const runEstimate = runEstimateFromThreadFacts(facts);
  const judgmentGrade = judgmentGradeFromThreadFacts(facts);
  const children = (injected.loadChildren ?? getChildren)(threadId);
  const preflight = deriveManagedDispatchPosture(
    facts, children.length > 0, routingMetadata.topology,
  );
  if (preflight.hasOutcome) {
    const preclaimed = injected.driverOptions?.preclaimed
      ?? process.env.NORTH_DISPATCH_DRIVER_PRECLAIMED === "1";
    if (preclaimed) {
      const agentId = selectDispatchAgentId(threadId, {
        agentId: admitted.agentId,
        driverOptions: injected.driverOptions,
      });
      const driver = (injected.claimDriver ?? claimDispatchDriver)(
        threadId, agentId, injected.driverOptions,
      );
      if (driver.release() === false) throw new DispatchDriverReleaseError(threadId);
    }
    return { threadId, posture: "atomic", result: "already done" };
  }
  const workingDirectory = resolveDispatchWorkingDirectory(facts);
  const agentId = selectDispatchAgentId(threadId, {
    agentId: admitted.agentId,
    driverOptions: injected.driverOptions,
  });
  const learning = decideManagedLearning({
    episodeId: agentId,
    taskSignature: {
      surface: "dispatch",
      threadFacts: facts.map(({ predicate, value }) => [predicate, value])
        .sort(([leftPredicate, leftValue], [rightPredicate, rightValue]) =>
          leftPredicate.localeCompare(rightPredicate) || leftValue.localeCompare(rightValue)),
      role: routingMetadata.role,
      taskGrade: routingMetadata.taskGrade,
      topology: routingMetadata.topology,
      domains: [...routingMetadata.domainRequirements].sort(),
    },
    taskSignatureCoverage: "exact",
    routingMetadata,
    routingAssessment: routingEconomics.assessment,
    pinEvidence: routingEconomics.pinEvidence,
  });
  routingMetadata = learning.routingMetadata;
  routingEconomics = admitRoutingEconomics({
    request: routingMetadata,
    routingAssessment: learning.routingAssessment,
    pinEvidence: admitted.pinEvidence,
    provider: process.env.AGENT_PROVIDER,
    target: process.env.AGENT_TARGET,
    model: process.env.AGENT_MODEL,
    allowLegacyMissingPinEvidence: bootstrapLegacyPinCompatibilityGranted,
    surface: "managed North dispatch learning admission",
  });
  const termination = new ManagedQueryTermination(
    injected.registerTermination,
    {
      agentId,
      threadId,
      goal: preflight.title,
      repo: workingDirectory,
      tokenTarget: admitted.tokenTarget,
    },
  );
  let driver: DispatchDriverClaim | undefined;
  let admission: EnvelopeAdmission | undefined;
  let result!: DispatchResult;
  let failed = false;
  let primaryError: unknown;
  try {
    termination.throwIfTerminated();
    driver = (injected.claimDriver ?? claimDispatchDriver)(
      threadId, agentId, injected.driverOptions,
    );
    const context = envelopeContextFromEnv(workingDirectory);
    termination.throwIfTerminated();
    admission = await (injected.admitResourceEnvelope ?? admitResourceEnvelope)({
      agentId, tier: routingMetadata.tier ?? process.env.AGENT_TIER as SemanticTier | undefined,
      project: context.project, sessionId: context.sessionId,
    });
    termination.throwIfTerminated();
    for (const advisory of admission?.advisories ?? []) console.warn(`[envelope] advisory: ${advisory}`);
    result = await runDispatch(
      threadId, judgmentGrade, strugglePolicy, runEstimate,
      admission, routingMetadata, routingEconomics,
      workingDirectory, agentId, injected.queryFn,
      facts, children, injected.loadThreadFacts ?? getThreadFacts,
      injected.deliveryRuntime,
      injected.childSettlementReader,
      injected.feedSubscriber ?? subscribeFeed,
      termination,
      injected,
      learning.assignment,
    );
  } catch (error) {
    failed = true;
    primaryError = error;
  }
  termination.publicationSettled();
  const cleanupErrors: unknown[] = [];
  try { await (injected.completeResourceEnvelope ?? completeResourceEnvelope)(admission); }
  catch (error) { cleanupErrors.push(error); }
  try {
    const released = driver
      ? await (injected.releaseDriver ?? ((value) => value.release()))(driver)
      : true;
    if (released === false) {
      console.error(`[dispatch] safe driver release unavailable for @${threadId}; liveness reaper remains armed`);
    }
  } catch (error) { cleanupErrors.push(error); }
  finally {
    termination.cleanupSettled();
    termination.release();
  }
  const errors = failed ? [primaryError, ...cleanupErrors] : cleanupErrors;
  if (errors.length === 1) throw errors[0];
  if (errors.length > 1)
    throw new AggregateError(errors, "dispatch execution and outer cleanup failed");
  return result;
}

export async function dispatchParallel(
  threadIds: string[],
  dependencies: DispatchDependencies,
): Promise<DispatchResult[]> {
  assertCoordinationAuthority("dispatchParallel");
  if (dependencies.agentId && threadIds.length > 1)
    throw new Error("dispatchParallel cannot reuse one explicit agentId across multiple children");
  return Promise.all(threadIds.map((id) => dispatch(id, dependencies)));
}

if (import.meta.main) {
  // The Clojure adapter checked the caller before replacing its environment
  // with the composed child identity. Direct library calls retain both checks.
  bootstrapAuthorityGranted = true;
  bootstrapLegacyPinCompatibilityGranted = true;
  const threadId = process.argv[2];
  if (!threadId) {
    console.error("usage: bun run src/dispatch.ts <thread-id>");
    process.exit(1);
  }
  dispatch(threadId, {
    agentId: process.env.AGENT_ID,
    routingMetadata: routingRequestFromEnv("managed North dispatch bootstrap"),
    routingAssessment: process.env.AGENT_ROUTING_ASSESSMENT
      ? JSON.parse(process.env.AGENT_ROUTING_ASSESSMENT) : undefined,
    pinEvidence: process.env.NORTH_ROUTING_PIN_EVIDENCE
      ? JSON.parse(process.env.NORTH_ROUTING_PIN_EVIDENCE) : undefined,
    tokenTarget: managedRunTokenTarget(
      process.env.NORTH_RUN_TOKEN_TARGET === undefined
        ? undefined : Number(process.env.NORTH_RUN_TOKEN_TARGET),
    ),
  })
    .then((result) => {
      console.log(JSON.stringify(result, null, 2));
    })
    .catch((err) => {
      console.error(err);
      process.exit(1);
    });
}
