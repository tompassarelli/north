import { resolve as pathResolve } from "node:path";
import { join as pathJoin } from "node:path";
import { mkdirSync, renameSync, writeFileSync, writeSync } from "node:fs";
import { randomUUID } from "node:crypto";
const REPO_ROOT = pathResolve(import.meta.dir, "..", "..");

let spawnTerminalLineWritten = false;

function terminalCause(value: unknown): string {
  const detail = value instanceof Error
    ? `${value.name}: ${value.message}`
    : String(value);
  return detail.replace(/\s+/g, " ").trim() || "unknown";
}

function latestTurnEvidence(state: ExecutionFoldSnapshot): WireTurnEvidence | undefined {
  return state.turnEvidence[state.turnEvidence.length - 1];
}

export function appendSpawnTerminalLine(kind: string, cause?: unknown): void {
  if (spawnTerminalLineWritten) return;
  spawnTerminalLineWritten = true;
  const detail = cause === undefined ? kind : `${kind}: ${terminalCause(cause)}`;
  try {
    writeSync(2, `[spawn] terminal ${detail}\n`);
  } catch {
    // The wrapper heartbeat remains the hard-kill proof if stderr itself is gone.
  }
}

export function installSpawnTerminalHandlers(): void {
  const signals = [
    ["SIGHUP", 129], ["SIGINT", 130], ["SIGTERM", 143],
  ] as const;
  for (const [signal, exitCode] of signals) {
    process.once(signal, () => {
      appendSpawnTerminalLine(`signal=${signal}`);
      process.exit(exitCode);
    });
  }
  process.once("uncaughtException", (error) => {
    appendSpawnTerminalLine("uncaughtException", error);
    process.exit(1);
  });
  process.once("unhandledRejection", (reason) => {
    appendSpawnTerminalLine("unhandledRejection", reason);
    process.exit(1);
  });
}

function writeLaneMeta(agentId: string, meta: Record<string, unknown>): void {
  try {
    const dir = process.env.NORTH_AGENT_LOGS_DIR
      ?? pathJoin(process.env.HOME ?? "", ".local/state/north/agents");
    mkdirSync(dir, { recursive: true });
    const file = agentId.startsWith("lane-") ? agentId : `lane-${agentId}`;
    const target = pathJoin(dir, `${file}.meta.json`);
    const temporary = `${target}.${randomUUID()}.tmp`;
    writeFileSync(temporary, `${JSON.stringify(meta)}\n`, "utf8");
    renameSync(temporary, target);
  } catch {
    // Lane discovery metadata is advisory and must never make spawning fatal.
  }
}
import { StreamWriter } from "./stream-writer";
import {
  DEFAULT_SYSTEM_PROMPT, harnessCompositionEvidence, harnessOptions, renewHarnessPresence,
  type Effort, type HarnessCompositionEvidence,
} from "./harness";
import {
  createExecutionActivityEmitter, forwardExecutionActivity,
} from "./execution-activity";
import {
  provisionWorktree, recordWorktreeAuthorityProfile, recordWorktreeRunRotation,
  resolvedWorktreeAuthorityProfile, rollbackProvisionedWorktree,
  worktreeFinalize, worktreePayload,
  type ProvisionedWorktree, type WorktreeAllocationWriter, type WorktreeHarvest,
  type WorktreeTerminalFailure,
} from "./worktree";
import {
  newRunId, recordWireRunTelemetry,
} from "./telemetry";
import { publishWireEvents, wireLedgerSummary } from "./run-ledger";
import { causeChain, deathReason, notifyDeath } from "./death";
import {
  inputChannel,
  LiveFeedReapTimeoutError,
  subscribeFeed,
} from "./coordination";
import {
  bespokeContractFingerprint, writeAgentFacts, writeAgentTerminal, updateAgentRoute, goalFromPrompt,
  userAnchoredPath,
} from "./identity";
import { BESPOKE_FINGERPRINT_DOMAIN, BESPOKE_FINGERPRINT_VERSION } from "./bespoke-contract";
import {
  resolveStrugglePolicy,
  assertExpectedStrugglePolicy,
  type StrugglePolicy,
} from "./struggle";
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
  formatProviderAuthoritySurface, providerLiveInput, routedQuery, selectProvider,
  selectProviderForExecution,
  providerRetrySafeTerminalDetail, ProviderRetrySafeError,
  type ProviderAuthoritySurface, type ProviderPreference, type RoutedQueryArguments,
} from "./providers";
import { resolveTier, type SemanticTier } from "./providers/catalog";
import type { RoutingRequest } from "./routing-metadata";
import { admitRoutingRequest, routingRequestFromEnv } from "./routing-admission";
import {
  orchestrationCapabilities,
} from "./orchestration-staffing";
import {
  hasAuthoringCapability,
} from "./orchestration-capabilities";
import { refreshAccountUsages } from "./account-usage";
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
  isEmptyResultTerminal, NO_PROVIDER_TERMINAL_DETAIL, PROVIDER_PROCESS_DEATH_OUTCOME,
  wireTerminalDecision,
} from "./execution-outcome";
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
  type WireRunId,
  type WireTurnEvidence,
} from "./wire";
import { ManagedLiveInputRoute } from "./live-input-route";
import {
  admitRoutingEconomics, type AdmittedRoutingEconomics,
  type RoutingAssessment, type RoutingPinEvidence,
} from "./routing-economics";
import {
  notifyTerminalSettlement, TerminalPublicationBudget, type TerminalNotification,
} from "./terminal-notification";
import { assessThreadDelivery, type DeliveryAssessment } from "./delivery-verification";
import { getThreadFacts, normalizeNorthEntityId } from "./north-client";
import {
  loadDeliveryRunState, newDeliveryRunContext, reserveDeliveryRun,
  reserveDeliveryRunWithRecovery, resolveDeliveryRunState, resolveThreadFacts,
  type DeliveryReservation, type DeliveryRunContext, type DeliveryRunState,
  type DeliveryReservationRecoveryOptions, type DeliveryRunStateLoadOptions,
  type ThreadFactsLoadOptions,
} from "./delivery-evidence";
import { takeSpawnTestRuntime } from "./internal/test-runtime";
import {
  adHocJudgmentGrade, judgmentGradeFromThreadFacts,
  type JudgmentGradeSnapshot,
} from "./judgment-grade";
import {
  ManagedQueryTermination, type HostTerminationRegistrar,
  type ManagedSessionHardCapOptions,
} from "./query-lifecycle";
import { decideManagedLearning } from "./managed-learning";
import type { LearningAssignment } from "./learning-regime";
import {
  publishLearningAssignment,
  type LearningAssignmentPublicationStatus,
} from "./learning-assignment-writer";
import { buildRunEnvelope, sha256Bytes } from "./composition-receipt";
import { unknownMcpActivity } from "./tool-activity";
import { unknownNativeCommandActivity } from "./native-command-activity";
import {
  wireModelAvailabilityReceipt,
  type WireRunProvenance,
} from "./run-provenance";
import { bridgeJournalRoot } from "./bridge/protocol";
import { ExecutionJournal, LANE_LIFECYCLE_KINDS } from "./bridge/journal";

export interface SpawnOptions {
  prompt: string;
  agentId?: string;
  model?: string;
  effort?: Effort;
  tools?: string[];
  systemPrompt?: string;
  maxTurns?: number;
  /** Equality-only compatibility alias; routingMetadata remains authoritative. */
  role?: string;
  posture?: string;
  thread?: string; // exact work/evidence thread.
  concern?: string; // exact physical-allocation concern owner; absent is explicitly unattributed.
  coordinator?: string; // spawning coordinator handle -> gets a direct peer ping on death
  provider?: ProviderPreference;
  target?: string;
  tier?: SemanticTier;
  routingMetadata: RoutingRequest;
  /** Orchestration-owned minimum-sufficient assessment; separate from the eight-field request. */
  routingAssessment?: RoutingAssessment;
  /** North-owned evidence for explicit provider/account/model pins. */
  pinEvidence?: RoutingPinEvidence;
  project?: string;
  sessionId?: string;
  worktree?: boolean; // explicit lane isolation choice; mutation-capable compositions default ON
  setupCmd?: string; // optional repo-setup hook run in the fresh worktree (e.g. `bun install`); repo-specific, never baked into north
}

interface SpawnRuntime {
  queryFn?: (args: RoutedQueryArguments) => WireQuery;
  deliveryRuntime?: {
    reserve: (context: DeliveryRunContext) => DeliveryReservation;
    load: (runId: string) => DeliveryRunState;
    /** Bounded pre-provider writer relaunch shape; tests inject timing only. */
    reserveOptions?: DeliveryReservationRecoveryOptions;
    /** Bounded retry shape for the finalize-time load; tests inject it. */
    loadOptions?: DeliveryRunStateLoadOptions;
  };
  loadThreadFacts?: typeof getThreadFacts;
  /** Bounded retry shape for the finalize-time thread-facts load; tests inject it. */
  threadFactsLoadOptions?: ThreadFactsLoadOptions;
  childSettlementReader?: (agentId: string) => ChildSettlement;
  feedSubscriber?: typeof subscribeFeed;
  registerTermination?: HostTerminationRegistrar;
  sessionHardCapRuntime?: Pick<
    ManagedSessionHardCapOptions,
    "hardCapMs" | "schedule" | "cancel" | "writeHandoff" | "replayHandoffs"
      | "stateDirectory" | "now"
  >;
  refreshAccountUsages?: typeof refreshAccountUsages;
  admitResourceEnvelope?: typeof admitResourceEnvelope;
  completeResourceEnvelope?: typeof completeResourceEnvelope;
  /** Subprocess to `bin/north`, which resolves babashka off PATH — and a hermetic
   * fixture owns PATH. Production never injects. */
  admitDispatchAuthority?: typeof admitManagedDispatchAuthority;
  worktreeAllocationWriter?: WorktreeAllocationWriter;
  publishLearningAssignment?: (
    runId: string, assignment: LearningAssignment,
  ) => Promise<LearningAssignmentPublicationStatus>;
  /** Hermetic lifecycle journal root. Injected providers perform no journal writes unless set. */
  journalRoot?: string;
}

const SPAWN_OPTION_FIELDS = new Set([
  "prompt", "agentId", "model", "effort", "tools", "systemPrompt", "maxTurns",
  "role", "posture", "thread", "concern", "coordinator", "provider",
  "target", "tier", "routingMetadata", "project", "sessionId", "worktree", "setupCmd",
  "routingAssessment", "pinEvidence",
]);

function allowlistedSpawnOptions(value: SpawnOptions): SpawnOptions {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error("managed North spawn request must be an object");
  const admitted: Record<string, unknown> = {};
  for (const [field, descriptor] of Object.entries(
    Object.getOwnPropertyDescriptors(value),
  )) {
    if (!SPAWN_OPTION_FIELDS.has(field))
      throw new Error(`managed North spawn request has unknown field ${field}`);
    if (descriptor.get || descriptor.set)
      throw new Error(`managed North spawn request field ${field} must be a data property`);
    admitted[field] = descriptor.value;
  }
  return admitted as unknown as SpawnOptions;
}

export function createSpawnAgentId(now = Date.now(), uuid = randomUUID()): string {
  return `lane-${now.toString(36)}-${uuid}`;
}

interface ManagedWorktreeLease extends ProvisionedWorktree {
  finalized: boolean;
}

// Bounded auto-retry for retry-safe provider-process deaths (thread 019f8f81,
// 2026-07-23 gen-1018 cluster: 4x openai_provider_execution_failed, zero retry).
// Constant-in-code, deliberately not env-tunable — a single fresh-run retry,
// never a loop. Terminal truthfulness: if the retry also dies, BOTH runs are
// recorded and the original death fact is never rewritten (see the terminal
// retryOfRun/retryAttempt provenance below).
const PROVIDER_PROCESS_DEATH_MAX_RETRIES = 1;

export function applyCodexTurnDeadlineFromReasoning(
  env: NodeJS.ProcessEnv = process.env,
): void {
  if (env.NORTH_CODEX_TURN_DEADLINE_MS !== undefined) return;
  let deadlineMs: number | undefined;
  switch (env.AGENT_REASONING) {
    case "low": deadlineMs = 600_000; break;
    case "medium": deadlineMs = 900_000; break;
    case "high": deadlineMs = 1_500_000; break;
    case "xhigh":
    case "max": deadlineMs = 2_400_000; break;
  }
  if (deadlineMs !== undefined)
    env.NORTH_CODEX_TURN_DEADLINE_MS = String(deadlineMs);
}

/** A start-of-stream overload is safe to re-route before any provider turn ran. */
export function eligibleForLaneStartProviderRetry(
  outcome: string,
  providerErrorDetail: string | undefined,
  numTurns: number | undefined,
  siblingTarget: string | undefined,
): boolean {
  if (outcome !== "provider_error" || !siblingTarget) return false;
  // A terminal-less stream is necessarily pre-turn. Anthropic's 529 terminal
  // reports zero turns; do not infer retry safety from arbitrary error prose.
  if (providerErrorDetail === NO_PROVIDER_TERMINAL_DETAIL) return true;
  return numTurns === 0 && /\b529\b|overloaded/i.test(providerErrorDetail ?? "");
}

/**
 * All three must hold before a provider-process death is retried:
 *  - the death class is provider-process-level (PROVIDER_PROCESS_DEATH_OUTCOME,
 *    i.e. openai_provider_execution_failed / provider_process_died), never a
 *    preflight block, stall, cap, or resource-envelope refusal;
 *  - topology is worker — an orchestrator's live child obligations make retry
 *    semantics wrong (a fresh run cannot honestly re-inherit them);
 *  - the lane's capability surface is read-only (no filesystem.write or shell) —
 *    a writable lane may already have mutated the checkout, so re-running it
 *    is unsafe.
 */
export function eligibleForProviderProcessDeathRetry(
  outcome: string,
  topology: string | undefined,
  capabilities: readonly string[],
): boolean {
  if (outcome !== PROVIDER_PROCESS_DEATH_OUTCOME) return false;
  if (topology !== "worker") return false;
  if (hasAuthoringCapability(capabilities)) return false;
  return true;
}

interface RetryContext {
  retryOfRun: string;
  retryAttempt: number;
  // Bare agent id of the terminal-committed lane this fresh identity follows.
  // Terminal identities are immutable — the retry mints its OWN agent id
  // (opts.agentId is overridden by the caller before runSpawn) and links back
  // to the original via this provenance field rather than reusing its subject.
  retryOfAgent: string;
}

// Only the executable bootstrap can classify selectors inherited from a
// serialized pre-evidence envelope as legacy. Programmatic callers cannot opt
// themselves into the compatibility warning path.
let bootstrapLegacyPinCompatibilityGranted = false;

function composeSpawnOptions(opts: SpawnOptions): SpawnOptions & {
  routingMetadata: RoutingRequest;
  routingEconomics: AdmittedRoutingEconomics;
} {
  const routingMetadata = admitRoutingRequest(
    opts.routingMetadata ?? {}, "managed North spawn",
  );
  const aliases = [
    ["role", opts.role, routingMetadata.role],
    ["tier", opts.tier, routingMetadata.tier],
    ["effort", opts.effort, routingMetadata.reasoning],
    ["posture", opts.posture, routingMetadata.posture],
  ] as const;
  for (const [field, supplied, canonical] of aliases) {
    if (supplied !== undefined && supplied !== canonical) {
      throw new Error(
        `managed North spawn ${field} compatibility alias must equal routingMetadata `
        + `(${JSON.stringify(supplied)} != ${JSON.stringify(canonical)})`,
      );
    }
  }
  const routingEconomics = admitRoutingEconomics({
    request: routingMetadata,
    routingAssessment: opts.routingAssessment,
    pinEvidence: opts.pinEvidence,
    provider: opts.provider,
    target: opts.target,
    model: opts.model,
    allowLegacyMissingPinEvidence: bootstrapLegacyPinCompatibilityGranted,
    surface: "managed North spawn routing economics",
  });
  const worktree = opts.worktree
    ?? (process.env.AGENT_WORKTREE === "1"
      || hasAuthoringCapability(orchestrationCapabilities(routingMetadata)));
  return {
    ...opts,
    routingMetadata,
    routingAssessment: routingEconomics.assessment,
    pinEvidence: routingEconomics.pinEvidence,
    routingEconomics,
    role: routingMetadata.role,
    tier: routingMetadata.tier,
    effort: routingMetadata.reasoning as Effort | undefined,
    posture: routingMetadata.posture,
    worktree,
  };
}

async function runSpawn(
  opts: SpawnOptions & {
    routingMetadata: RoutingRequest;
    routingEconomics: AdmittedRoutingEconomics;
  },
  judgmentGrade: JudgmentGradeSnapshot,
  strugglePolicy: StrugglePolicy,
  envelopeAdmission?: EnvelopeAdmission,
  injected: SpawnRuntime = {},
  termination: ManagedQueryTermination = new ManagedQueryTermination(),
  worktreeLease?: ManagedWorktreeLease,
  retryContext?: RetryContext,
  retryTarget?: string,
  parentRunId?: WireRunId,
  learningAssignment?: LearningAssignment,
  lifecycleJournal?: ExecutionJournal,
): Promise<{
  result: string; outcome: string; runId: string; providerErrorDetail?: string;
  numTurns?: number; provider: ProviderPreference; siblingTarget?: string;
}> {
  // Composition is deliberately complete before admission and stays immutable
  // through routing, identity, provider execution, and terminal telemetry.
  const routingMetadata = opts.routingMetadata;
  const capabilities = orchestrationCapabilities(routingMetadata);
  const requested = { provider: opts.provider, target: opts.target,
    tier: opts.tier, model: opts.model, effort: opts.effort };
  const agentId = opts.agentId ?? createSpawnAgentId();
  const repoRoot = worktreeLease?.repoRoot ?? process.cwd();
  const wt = worktreeLease;
  let runId = worktreeLease?.allocation.runId ?? newRunId(agentId);
  if (!learningAssignment)
    throw new Error("managed North spawn execution requires a learning assignment");
  // The injected query boundary is hermetic and owns all external writers.
  // Production must durably acknowledge the assignment before even selecting
  // a provider, so a failed recorder cannot move the arm after side effects.
  const assignmentWriter = injected.publishLearningAssignment
    ?? (injected.queryFn ? async () => "recorded" as const : publishLearningAssignment);
  const publishAssignmentForRun = async (assignmentRunId: string): Promise<void> => {
    if (await assignmentWriter(assignmentRunId, learningAssignment) !== "recorded") {
      throw new Error("managed North spawn requires a durable pre-provider learning assignment");
    }
  };
  await publishAssignmentForRun(runId);
  // ONE thread resolution, used by both the delivery reservation here and the
  // run ledger at terminal. They disagreed: the reservation read opts.thread
  // while the ledger also accepted AGENT_THREAD, and `north spawn --thread`
  // delivers the binding through the environment. So a CLI-spawned lane
  // attributed its telemetry correctly and simultaneously got NO reservation --
  // which meant no NORTH_RUN_ID in its environment, and `north evidence record`
  // failing inside it with "invalid delivery run id". Two lanes lost their bar
  // evidence to this before it was traced.
  //
  // Absent is undefined, never the string "(ad-hoc)": that is a legible marker
  // for an unbound run, not a thread id, and reserving against it would mint a
  // reservation bound to nothing.
  const boundThreadId = opts.thread ?? process.env.AGENT_THREAD ?? undefined;
  const runContext = boundThreadId
    ? newDeliveryRunContext(runId, boundThreadId, agentId)
    : undefined;
  const deliveryRuntime: SpawnRuntime["deliveryRuntime"] = injected.deliveryRuntime
    ?? (injected.queryFn ? undefined : {
      reserve: reserveDeliveryRun,
      load: loadDeliveryRunState,
    });
  let deliveryReservation: DeliveryReservation | undefined;
  let deliveryReservationReady = false;
  const requestedTier = opts.tier;
  const requestedReasoning = opts.effort;
  const providerPreference = opts.provider ?? "auto";
  const targetPreference = opts.target;
  const routingRequest = { provider: providerPreference, target: retryTarget ?? targetPreference };
  if (!injected.queryFn) admitPinnedProvider(opts.provider, capabilities);
  // Injected query functions own their entire provider boundary; keeping the
  // refresh out of that path makes tests and alternative adapters hermetic.
  const routingContext = {
    tier: requestedTier, reasoning: requestedReasoning, model: opts.model,
    stableKey: agentId, capabilities, signal: termination.signal,
  };
  let routing;
  if (injected.queryFn) {
    routing = selectProvider(routingRequest, undefined, routingContext);
  } else {
    try {
      routing = await selectProviderForExecution(
        routingRequest,
        undefined,
        routingContext,
        injected.refreshAccountUsages
          ? { refreshAccountUsages: injected.refreshAccountUsages }
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
  if (wt && injected.queryFn) {
    // An injected provider owns its boundary and never enters routedQuery's
    // attempt hook, so publish its selected authority at the same pre-query seam.
    recordWorktreeAuthorityProfile(
      wt.allocation,
      resolvedWorktreeAuthorityProfile(routing),
    );
  }
  const resolved = resolveTier(routing.provider, requestedTier, opts.model, opts.effort);
  opts.model = resolved.model;
  opts.effort = resolved.effort;
  // The hydrated Orchestration selection is canonical. Never let an inherited parent
  // env relabel this child as an alias or a different role.
  const identityRole = routingMetadata.role!;
  writeLaneMeta(agentId, {
    thread: boundThreadId ?? null,
    role: identityRole,
    tier: resolved.tier,
    effort: routing.resolvedEffort ?? resolved.effort ?? routingMetadata.reasoning,
    model: resolved.model,
    provider: routing.provider,
    startedAt: new Date().toISOString(),
  });
  const composition = routingMetadata.composition!;
  const identityBase = {
    kind: "lane" as const,
    role: identityRole,
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
    repo: userAnchoredPath(process.cwd()),
    goal: goalFromPrompt(opts.prompt),
    coordinator: opts.coordinator,
    worktree: wt?.path,
    branch: wt?.branch,
    retryOfAgent: retryContext?.retryOfAgent,
  };
  const initialLiveInput = providerLiveInput(routing.provider);
  const ch = inputChannel(opts.prompt); // streaming input keeps the managed live-messaging route open
  termination.attachInput(() => { try { ch.end(); } catch { /* already closed */ } });
  const liveInputRoute = new ManagedLiveInputRoute(
    agentId,
    identityBase,
    {
      provider: routing.provider,
      providerTarget: routing.target,
      liveInput: initialLiveInput,
      model: opts.model,
      effort: opts.effort,
    },
    (message) => ch.push(message),
    injected.feedSubscriber ?? subscribeFeed,
  );
  await writeAgentFacts(agentId, {
    ...identityBase,
    model: opts.model,
    provider: routing.provider,
    providerTarget: routing.target,
    liveInput: initialLiveInput,
    ...liveInputRoute.initialProjection(),
    effort: opts.effort,
  });
  lifecycleJournal?.append(LANE_LIFECYCLE_KINDS.identityAdmitted, {
    thread: boundThreadId ?? null,
    role: identityRole,
    provider: routing.provider,
    target: routing.target,
    tier: resolved.tier,
    effort: routing.resolvedEffort ?? opts.effort ?? null,
    model: routing.resolvedModel ?? opts.model ?? null,
    worktree: wt?.path ?? null,
    branch: wt?.branch ?? null,
  });
  // The injected route may resolve its concrete model after the initial sidecar
  // write; enrich discovery metadata at the identity publication boundary.
  writeLaneMeta(agentId, {
    thread: boundThreadId ?? null,
    role: identityRole,
    tier: resolved.tier,
    effort: routing.resolvedEffort ?? opts.effort,
    model: routing.resolvedModel ?? opts.model,
    provider: routing.provider,
    startedAt: new Date().toISOString(),
  });
  const activeRoute = () => ({
    provider: routing.provider,
    providerTarget: routing.target,
    liveInput: providerLiveInput(routing.provider),
    model: routing.resolvedModel ?? opts.model,
    effort: routing.resolvedEffort ?? opts.effort,
  });
  const refreshIdentityRoute = (required = false) => {
    liveInputRoute.refresh(activeRoute(), required);
  };
  const executionFold = makeExecutionFold(strugglePolicy);
  let wireWriter: WireEventWriter | undefined;
  let stream: StreamWriter | undefined;
  let nextObservedSequence = 0;
  let nextPersistedSequence = 0;
  let announcedCompactions = 0;
  const flushWireEvents = async (): Promise<void> => {
    if (!wireWriter || !stream) return;
    const events = wireWriter.events();
    while (nextPersistedSequence < events.length) {
      const event = events[nextPersistedSequence]!;
      if (event.sequence !== nextPersistedSequence) {
        throw new Error("wire writer persistence sequence diverged");
      }
      await stream.writeWireEvent(event);
      nextPersistedSequence += 1;
    }
  };
  const observeWireEvent = async (event: WireEvent) => {
    if (!wireWriter) throw new Error("wire event observed before run admission");
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
    const observation = executionFold.observe(canonical);
    nextObservedSequence += 1;
    await flushWireEvents();
    return observation;
  };
  const observeCommittedWireEvents = async (): Promise<void> => {
    if (!wireWriter) return;
    const events = wireWriter.events();
    while (nextObservedSequence < events.length) {
      executionFold.observe(events[nextObservedSequence]!);
      nextObservedSequence += 1;
    }
    await flushWireEvents();
  };
  const startWireRun = async (): Promise<WireEventWriter> => {
    if (wireWriter) return wireWriter;
    const opened = await StreamWriter.open(agentId);
    const writer = new WireEventWriter({ runId: wireRunId(runId) });
    stream = opened;
    wireWriter = writer;
    const started = writer.append({
      kind: "run.started",
      lifecycle: "running",
      owner: agentId,
      ...(parentRunId === undefined ? {} : { parentRunId }),
    });
    await observeWireEvent(started);
    return writer;
  };

  let result = "", outcome = "ran";
  // Public wire detail is North-owned and bounded. Recursive provider causes
  // remain only in local diagnostics below.
  let preflightDetail: string | undefined;
  // Same discipline for a provider_error terminal: the error payload the frame
  // carried, rendered once and carried onto @run (thread 019f9cec).
  let providerErrorDetail: string | undefined;
  let deadlineExceededDetail: string | undefined;
  let worktreeTerminalFailure: WorktreeTerminalFailure | undefined;
  const end = (oc: string) => { outcome = oc; try { ch.end(); } catch { /* already closed */ } };

  let initialComposition: HarnessCompositionEvidence | undefined;
  let admittedRoute: {
    provider: "anthropic" | "openai";
    evidence?: HarnessCompositionEvidence;
    authority?: ProviderAuthoritySurface;
  } | undefined;
  const queryFn = injected.queryFn ?? ((args: RoutedQueryArguments) => routedQuery(
    routing, args, requestedTier, async (transition) => {
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
        });
        console.log(`[spawn] effective authority: ${formatProviderAuthoritySurface(authority)}`);
      }
    },
    (decision) => {
      if (wt) {
        recordWorktreeAuthorityProfile(
          wt.allocation,
          resolvedWorktreeAuthorityProfile(decision),
        );
      }
    },
  ));
  // This boundary distinguishes North/Orchestration prompt assembly from the provider
  // query itself. A throw before construction cannot honestly be a provider
  // process death because no provider query has been created or accepted.
  let providerQueryConstructionStarted = false;
  let queryCloseError: unknown;

  // Error boundary (thread 019f2800): the SDK runs the turn in a subprocess; if it dies
  // (OOM SIGKILL / parent SIGTERM / idle Transport-closed) readMessages() THROWS exitError
  // here. Without this try/catch the throw escaped -> publication skipped, no death signal,
  // channel leaked. Now: catch -> outcome "died" + durable death fact; finally
  // -> ALWAYS end the channel + record the run; return the PARTIAL result (supervision, not
  // fail-fast) so one worker's death never rejects a spawnParallel Promise.all batch.
  // Stream watchdog (thread 019f4d54): a stall (no SDK message for N min while the
  // query is open) is otherwise INVISIBLE — the iterator neither yields nor throws, so
  // the catch below never fires. Wrap the iterator: N min silence -> stalled fact +
  // coordinator ping (diagnostic); 2N -> abort + outcome=stalled + durable death fact.
  // Every terminal peer wake is deferred until the terminal and run publications settle.
  const coordHandle = opts.coordinator;
  const window = stallMs();
  const executionActivity = createExecutionActivityEmitter();
  let watchdogAbort: WatchdogAbortEvidence | undefined;
  let stopProviderActivity = () => {};
  let terminalSignal: Pick<TerminalNotification, "detail" | "subject"> = {};
  const terminalAuxiliaryWrites: Array<(timeoutMs: number) => void> = [];
  let liveInputFreezeError: unknown;
  // Background-task refusal (thread 019f4ed2): a lane that ends its turn while a
  // harness-tracked background Bash task is live must NOT finalize — the SDK
  // auto-continues the model on task settlement, but only if we keep the loop alive
  // instead of breaking on the first `result`. Track the live set; bgContinuations
  // counts CONSECUTIVE no-progress refusals (reset on settlement) for the stuck-lane cap.
  let bgContinuations = 0;
  const orchestrator = routingMetadata.topology === "orchestrator";
  const readChildSettlement = injected.childSettlementReader ?? settleChildren;
  let childContinuation = initialChildContinuationState(
    requiredDirectChildCount(routingMetadata),
  );
  // Orchestrator continuation race (thread 019f8ec5): the obligation whose
  // continuation was injected at the last turn-end and not yet discharged by a
  // genuine (non-empty) provider result. If the next result is a degenerate
  // empty terminal (the continuation raced the Anthropic session's teardown),
  // this drives an explicit blocked outcome instead of a ran_empty masquerade.
  let pendingContinuation: OrchestratorContinuationKind | undefined;
  // Streaming providers own private continuation identity. North asks the
  // same semantic query for another turn without observing a raw session id.
  const resumeContinuations = orchestrator
    && providerLiveInput(routing.provider) === "streaming";
  let activeQuery: WireQuery | undefined;
  try {
  try {
  termination.throwIfTerminated();
  console.log(`[spawn] @agent:${agentId} starting provider=${routing.provider} target=${routing.target}${resolved.tier ? ` tier=${resolved.tier}` : ""} (${routing.reason})`);
  // Reserve only at the last pre-provider seam. Earlier routing/admission
  // failures must not strand undiscoverable reservation-only subjects.
  if (runContext) {
    try {
      if (deliveryRuntime) {
        deliveryReservation = reserveDeliveryRunWithRecovery(
          runContext,
          deliveryRuntime.reserve,
          {
            ...deliveryRuntime.reserveOptions,
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
      if (!deliveryReservationReady) {
        const attemptedRunId = runId;
        runId = newRunId(agentId);
        if (wt) recordWorktreeRunRotation(wt.allocation, runId);
        console.error(
          `[delivery] @${attemptedRunId} reservation unavailable; rotating blocked telemetry `
          + `to fresh non-reservation @${runId}: `
          + `${(error as Error)?.message ?? String(error)}`,
        );
        await publishAssignmentForRun(runId);
        throw error;
      }
    }
  }
  const agentOptions = harnessOptions({
    self: agentId,
    extraTools: opts.tools ?? ["Read", "Edit", "Write", "Bash", "Grep", "Glob"],
    model: opts.model, effort: opts.effort,
    provider: routing.provider,
    modelAvailability: {
      exactModelPinned: requested.model !== undefined,
      targetId: routing.target,
      receipt: routing.modelAvailabilityReceipts?.[routing.target],
    },
    routingMetadata,
    // Worktree lane: run tools IN the worktree (cwd) and append the
    // isolation+landing+verify protocol to the prompt. Composed HERE so
    // harness.ts stays a thin cwd knob.
    systemPrompt: wt
      ? (opts.systemPrompt ?? DEFAULT_SYSTEM_PROMPT)
        + worktreePayload({ path: wt.path, branch: wt.branch, mainReportsDir: repoRoot + "/docs/private" })
      : opts.systemPrompt,
    maxTurns: opts.maxTurns,
    abortController: termination.abortController,
    role: opts.role, posture: opts.posture,
    cwd: wt?.path ?? process.cwd(),
    deliveryRun: deliveryReservationReady ? runContext : undefined,
  });
  initialComposition = harnessCompositionEvidence(agentOptions);
  if (injected.queryFn && injected.feedSubscriber)
    await liveInputRoute.activate(activeRoute());
  termination.throwIfTerminated();
  providerQueryConstructionStarted = true;
  const writer = await startWireRun();
  activeQuery = queryFn({
    input: ch.stream(),
    options: agentOptions,
    writer,
  });
  termination.attachQuery(activeQuery);
  stopProviderActivity();
  stopProviderActivity = forwardExecutionActivity(
    activeQuery.executionActivity,
    executionActivity,
  );
  turnLoop: while (true) {
  let privateContinuation: string | undefined;
  const watched = withStallWatchdog(activeQuery[Symbol.asyncIterator](), {
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
    // routedQuery mutates the decision before the first fallback-provider event.
    // Refresh from that structured decision before the event is exposed.
    refreshIdentityRoute();
    if (observation.state.compactions > announcedCompactions) {
      announcedCompactions = observation.state.compactions;
      console.error(
        `[harness] @agent:${agentId} context compaction #${announcedCompactions}`,
      );
    }
    if (observation.backgroundTask?.kind === "settled") bgContinuations = 0;

    // Struggle sensors are OBSERVE-ONLY now: fold the event, and on the first
    // occurrence of each trigger leave a stderr breadcrumb. The run does NOT change
    // route or terminate — the accumulated triggers become terminal `struggle` run
    // facts below, feeding D2's execution-axis diagnosis without any in-flight swap.
    const trigger = observation.struggleTrigger;
    if (trigger) {
      console.error(
        `[struggle] @agent:${agentId} sensor fired: ${trigger} `
        + `(model calls ${observation.state.run.usage.lifetime.modelCalls}, `
        + `${observation.state.struggle.errorCount} tool error(s)) `
        + "— recorded as execution-axis evidence, no in-flight change",
      );
    }

    if (event.essential && event.kind === "model-call.completed") {
      const turnResult = observation.state.lastCompletedAssistantOutput ?? "";
      const turnEvidence = event.evidence?.turns;
      lifecycleJournal?.append(LANE_LIFECYCLE_KINDS.turnBoundary, {
        status: event.status,
        errorCode: event.errorCode ?? null,
        turnUnit: turnEvidence?.unit ?? null,
        turnCount: turnEvidence?.count ?? null,
        resultBytes: Buffer.byteLength(turnResult),
      });
      if (isIntermediateProviderSessionReplacement(event)) continue;
      result = turnResult;
      const cap = event.errorCode === "provider_max_turns"
        ? "provider_max_turns"
        : event.errorCode === "provider_budget_exhausted"
          || event.errorCode === "provider_structured_output_retries_exhausted"
          ? event.errorCode : undefined;
      if (cap !== undefined) {
        end(cap === "provider_max_turns" ? "max_turns" : "capped");
        const partial = result.trim() ? `partial: ${result.trim().slice(0, 200)}` : "no partial result";
        const detail = `${cap} — ${partial}`;
        terminalSignal = { subject: "TURN CAP", detail };
        terminalAuxiliaryWrites.push((timeoutMs) =>
          notifyTurnCap(agentId, detail, {}, timeoutMs)
        );
        break;
      }
      deadlineExceededDetail = observation.state.deadlineExceededDetail;
      if (deadlineExceededDetail) {
        end("deadline_exceeded");
        console.error(
          `[deadline_exceeded] @agent:${agentId} process=deadline_exceeded detail=${deadlineExceededDetail}`,
        );
        terminalSignal = { subject: "DEADLINE EXCEEDED", detail: deadlineExceededDetail };
        break;
      }
      if (event.status !== "succeeded") {
        end("provider_error");
        providerErrorDetail = observation.state.providerErrorDetail
          ?? "model-call terminal failed without diagnostic evidence";
        console.error(`[provider_error] @agent:${agentId} ${providerErrorDetail}`);
        terminalSignal = { subject: "AGENT BLOCKED", detail: providerErrorDetail };
        break;
      }
      if (ch.pending() === 0) {
        // Orchestrator continuation race (thread 019f8ec5): a continuation
        // injected at a prior turn-end asks the provider for ANOTHER genuine
        // turn, but the Anthropic session may already be tearing down after its
        // final message — the continuation then lands on a closing stream and
        // the provider answers with a degenerate empty-success terminal.
        // decideChildTurnEnd would otherwise read that empty result as a
        // completed continuation (acknowledging a reduction that never ran) and
        // finalize it as ran_empty. An outstanding continuation is discharged
        // ONLY by a non-empty result; an empty terminal here is the race, so
        // record the obligation-specific blocked outcome loudly instead.
        if (orchestrator && pendingContinuation && result.trim() === "") {
          const raced = continuationRaceOutcome(pendingContinuation);
          console.error(
            `[harness] @agent:${agentId} orchestrator ${pendingContinuation} continuation answered by an empty provider terminal — closing-stream race, recording ${raced} (never ran_empty)`,
          );
          end(raced);
          break;
        }
        pendingContinuation = undefined; // a genuine result discharges the obligation
        // Refuse to exit while harness-tracked background tasks are live (thread 019f4ed2,
        // half a). Inject a continuation message + keep looping: the SDK re-invokes the
        // model, the task settles (task_notification / task_updated), bgContinuations
        // resets, and a later result with an empty live set finalizes clean. The cap
        // (default 5 consecutive no-progress refusals) prevents infinite-looping a stuck
        // lane — it then falls through to finalize, and the after-loop early-exit check
        // makes the abandoned work loud.
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
          end("background_tasks_incomplete");
          break;
        }
        if (orchestrator) {
          const decision = decideChildTurnEnd(
            childContinuation,
            readChildSettlement(agentId),
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
            // Remember the outstanding obligation so a degenerate empty terminal
            // on the next turn (a closing-stream race) blocks explicitly rather
            // than falsely discharging the continuation.
            pendingContinuation = decision.reason;
            if (resumeContinuations) {
              if (!activeQuery.continueTurn) {
                end("provider_error");
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
            const blockedOutcome = decision.reason === "child_reconciliation_unavailable"
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
            end(blockedOutcome);
            break;
          }
        }
        end("ran");
        break; // MUST end the channel or the query hangs
      }
    }
  }
  if (privateContinuation !== undefined) {
    if (!activeQuery.continueTurn) {
      end("provider_error");
      providerErrorDetail = "active provider cannot retain a private continuation turn";
      terminalSignal = { subject: "AGENT BLOCKED", detail: providerErrorDetail };
      break turnLoop;
    }
    await activeQuery.continueTurn(privateContinuation);
    continue turnLoop;
  }
  break;
  }
  await observeCommittedWireEvents();
  const providerState = executionFold.snapshot();
  if (providerState?.latestModelCallTerminal?.status !== "succeeded"
      && outcome === "ran" && !watchdogAbort) {
    // A clean iterator close is transport completion, not provider success.
    // Only an explicit terminal result may establish process=ran.
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
    // final text — the last assistant block is an unanswered tool_use or a
    // terminal thinking block — yet the SDK still yields subtype=success/
    // result="". Recording it as process=ran read as a clean no-op. Make it a
    // distinct LOUD terminal so a zero-deliverable lane never masquerades as
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
    // North initiated this termination. Preserve that cause before interrupting
    // the provider; close-time fallout is cleanup evidence, not a provider death.
    outcome = "watchdog_aborted";
    providerErrorDetail = undefined;
    const detail = describeWatchdogAbortEvidence(watchdogAbort);
    console.error(`[watchdog-abort] @agent:${agentId} ${detail}`);
    const err = new Error(detail);
    terminalSignal = { subject: "AGENT DEATH", detail: deathReason(err) };
    terminalAuxiliaryWrites.push((timeoutMs) =>
      notifyDeath(agentId, err, { thread: undefined }, timeoutMs)
    );
    try { await termination.closeQuery(activeQuery); }
    catch (error) { queryCloseError = error; }
  }
  } catch (err) {
    if (termination.hardCapStatus()) {
      outcome = "session_hard_cap";
      worktreeTerminalFailure = {
        code: "session_hard_cap",
        phase: "provider_execution",
      };
    } else if (err instanceof ResourceEnvelopeExceededError) {
      outcome = "resource_envelope_exceeded";
      worktreeTerminalFailure = {
        code: "resource_envelope_retry_refused",
        phase: "provider_preflight",
      };
      console.error(`[envelope] @agent:${agentId} ${err.message}`);
    } else if (err instanceof ProviderRetrySafeError) {
      // A spend-guard refusal carries its own terminal outcome; every other
      // retry-safe preflight block stays blocked_preflight.
      const carried = (err as { processOutcome?: unknown }).processOutcome;
      outcome = typeof carried === "string" ? carried : "blocked_preflight";
      worktreeTerminalFailure = {
        code: "provider_preflight_refused",
        phase: "provider_admission",
      };
      preflightDetail = providerRetrySafeTerminalDetail(err);
      console.error(`[${outcome}] @agent:${agentId} ${causeChain(err)}`);
    } else if (!providerQueryConstructionStarted) {
      outcome = "blocked_preflight";
      worktreeTerminalFailure = {
        code: "spawn_pre_provider_setup_failed",
        phase: "provider_preflight",
      };
      preflightDetail = "spawn failed during North pre-provider setup";
      console.error(
        `[blocked_preflight] @agent:${agentId} spawn_pre_provider_setup_failed: ${causeChain(err)}`,
      );
    } else {
      outcome = "died";
      terminalSignal = { subject: "AGENT DEATH", detail: deathReason(err) };
      terminalAuxiliaryWrites.push((timeoutMs) =>
        notifyDeath(agentId, err, { thread: undefined }, timeoutMs)
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
    end(outcome); // idempotent: close the channel so the query + any leak unwinds
    // A terminal SDK result does not guarantee the provider subprocess has
    // exited while streaming input remains open. Interrupt exactly once after
    // closing input so a completed lane cannot retain its Bun/CLI process tree.
    try { await termination.closeQuery(activeQuery); }
    catch (error) { queryCloseError = error; }
    try { await observeCommittedWireEvents(); }
    catch (error) { queryCloseError ??= error; }
  }

  // Snapshot BEFORE any cleanup-only failure below can touch outcome: this is
  // the provider's own terminal, not a cleanup verdict. A lane that already
  // reached its success terminal (a real result was read) has done the work;
  // teardown of the terminal live-feed drain afterward is usually best-effort
  // cleanup, not part of the provider turn. The exception is a typed direct-
  // child reap timeout: an unreaped managed child means the process itself has
  // not earned a clean terminal. A drain failure that happens BEFORE a success
  // terminal (no result yet) also stays fail-closed via the branches below.
  const reachedProviderSuccessTerminal = outcome === "ran";

  const sessionHardCap = termination.hardCapStatus();
  const hostSignal = termination.hostSignal();
  if (sessionHardCap && !watchdogAbort) {
    outcome = "session_hard_cap";
    providerErrorDetail = undefined;
    worktreeTerminalFailure = {
      code: "session_hard_cap",
      phase: "provider_execution",
    };
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
    const terminalSettlementFailed = error instanceof LiveFeedReapTimeoutError;
    if (retrySucceeded && !terminalSettlementFailed) {
      liveInputFreezeError = undefined;
    } else {
      if (reachedProviderSuccessTerminal && !terminalSettlementFailed) {
        // Completed provider turn: the feed leak is real (best-effort, logged
        // for operator follow-up) but it is cleanup after success — process/
        // delivery already earned by the completed turn stays as recorded.
        // No AGENT DEATH fact/ping: that channel means "the provider died",
        // which did not happen here and must not be asserted.
        console.error(
          `[live-input] @agent:${agentId} terminal live-feed drain failed after a completed provider turn — process/delivery preserved (${error.message})`,
        );
      } else {
        // A direct child that cannot be reaped is not a completed managed
        // process, even if the provider emitted a result first. Keep the
        // original typed settlement error and prohibit a clean terminal.
        outcome = "died";
        terminalSignal = { subject: "AGENT DEATH", detail: deathReason(error) };
        terminalAuxiliaryWrites.push((timeoutMs) =>
          notifyDeath(agentId, error, { thread: undefined }, timeoutMs)
        );
      }
    }
  }

  // Belt-and-suspenders terminal gate. A child may appear after the last
  // provider result, and an unavailable graph is not evidence of zero children.
  // Workers keep historical best-effort notification semantics; only a
  // successful orchestrator is prevented from publishing process=ran.
  const finalChildren = readChildSettlement(agentId);
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

  // Salvage-gated worktree cleanup (only if this spawn provisioned one): remove
  // on a clean ran, KEEP + surface a worktree_orphaned fact on any
  // crash/cap/dirty tail. Fail-open.
  let worktreeHarvest: WorktreeHarvest | undefined;
  if (wt) {
    worktreeHarvest = worktreeFinalize(agentId, outcome, wt, worktreeTerminalFailure);
    wt.finalized = true;
  }

  // Commit the lane's process/delivery terminal (SYNC, digest marker last)
  // before exit so the lifecycle janitor cannot mistake a completed lane for silence.
  refreshIdentityRoute();
  let delivery: DeliveryAssessment | undefined;
  if (outcome === "ran" && opts.thread) {
    if (!deliveryReservationReady || !deliveryReservation || !deliveryRuntime) {
      delivery = {
        deliveryOutcome: "unverified",
        deliveryReason: "delivery_reservation_unavailable_at_finalize",
      };
    } else {
      // A busy coordinator used to be indistinguishable from a bad reservation:
      // the reader timed out, the state collapsed to invalid, and a lane whose
      // evidence was intact on the graph finalized unverified (thread 019f9cc1).
      // Retry only the LOAD; a successful read that finds no valid reservation
      // still fails closed on the first attempt.
      const resolution = resolveDeliveryRunState(
        runId,
        (id) => deliveryRuntime.load(id),
        deliveryRuntime.loadOptions,
      );
      const runState: DeliveryRunState | undefined = resolution.transientFailure
        ? undefined
        : resolution.state;
      if (!runState?.reservationValid) {
        deliveryReservationReady = false;
        // Loud + diagnosable (thread 019f9063): a load failure and a load that
        // simply found no valid reservation both used to read identically.
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
          opts.thread,
          (id) => (injected.loadThreadFacts ?? getThreadFacts)(id),
          injected.threadFactsLoadOptions,
        );
        if (threadResolution.transientFailure) {
          console.error(
            `[delivery] @${opts.thread} thread unreadable at finalize after `
            + `${threadResolution.attempts} attempt(s) (${threadResolution.transientFailure}); `
            + "leaving delivery unverified",
          );
          delivery = {
            deliveryOutcome: "unverified",
            deliveryReason: "delivery_thread_load_failed_at_finalize",
          };
        } else {
          delivery = assessThreadDelivery(
            opts.thread,
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
  await flushWireEvents();
  const finalExecution = executionFold.snapshot();
  if (!finalExecution || finalExecution.run.lifecycle === "running"
      || finalExecution.run.lifecycle === "waiting") {
    throw new Error("wire run did not reach its outer terminal");
  }
  const finalTurn = latestTurnEvidence(finalExecution);
  const numTurns = finalTurn?.unit === "assistant-turn"
    ? finalTurn.count
    // A retry-safe preflight block proves the provider accepted no turn. This
    // zero is North-observed; every other missing provider value stays absent.
    : terminal.processOutcome === "blocked_preflight"
      || terminal.processOutcome === "blocked_spend_guard" ? 0 : undefined;
  let terminalJournalError: unknown;
  try {
    lifecycleJournal?.append(LANE_LIFECYCLE_KINDS.terminal, {
      outcome,
      processOutcome: terminal.processOutcome,
      deliveryOutcome: terminal.deliveryOutcome,
      deliveryReason: terminal.deliveryReason,
      deliveryProof: terminal.deliveryProof ?? null,
      numTurns: numTurns ?? null,
      resultBytes: Buffer.byteLength(result),
    });
    lifecycleJournal?.append(LANE_LIFECYCLE_KINDS.harvest, worktreeHarvest
      ? {
          status: worktreeHarvest.status,
          branch: wt!.branch,
          sha: worktreeHarvest.headOid ?? null,
          ref: worktreeHarvest.ref,
          commits: worktreeHarvest.commits ?? null,
          reason: worktreeHarvest.reason ?? null,
        }
      : { status: "not-applicable", branch: null, sha: null });
  } catch (error) {
    terminalJournalError = error;
  }
  const publicationBudget = new TerminalPublicationBudget();
  // The graph terminal is the authoritative identity boundary and retains the
  // first publication slice; the journal above independently owns execution history.
  const terminalPublication = writeAgentTerminal(
    agentId,
    terminal,
    publicationBudget.publicationTimeout(1),
  );
  for (const [index, writeAuxiliary] of terminalAuxiliaryWrites.entries()) {
    writeAuxiliary(
      publicationBudget.publicationTimeout(
        terminalAuxiliaryWrites.length - index + 2,
      ),
    );
  }

  // Thread attribution. The CLI refuses a managed spawn that names neither a
  // thread nor --ad-hoc, so by the time we get here the choice was deliberate —
  // but a direct SDK caller can still omit both, and the honest record of that
  // is "(ad-hoc)" WITH its provenance, never a silent default that looks bound.
  const boundThread = boundThreadId ?? "(ad-hoc)";
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
  const mcpActivity = activeQuery?.mcpActivity?.()
    ?? unknownMcpActivity("provider-activity-unavailable");
  const nativeCommandActivity = activeQuery?.nativeCommandActivity?.()
    ?? unknownNativeCommandActivity("provider-activity-unavailable");
  const provenance: WireRunProvenance = {
    posture: "spawn",
    ...(routingMetadata.role === undefined ? {} : { role: routingMetadata.role }),
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
    ...(requested.target === undefined ? {} : { requestedTarget: requested.target }),
    ...(requested.tier === undefined ? {} : { requestedTier: requested.tier }),
    ...(requested.effort === undefined ? {} : { requestedEffort: requested.effort }),
    routingMetadata,
    ...(opts.routingEconomics.assessment === undefined
      ? {} : { routingAssessment: opts.routingEconomics.assessment }),
    routingAdmissionReceipt: opts.routingEconomics.receipt,
    ...(opts.routingEconomics.pinEvidence === undefined
      ? {} : { routingPinEvidence: opts.routingEconomics.pinEvidence }),
    ...(promptComposition === undefined ? {} : { promptComposition }),
    learningAssignment,
    ...(promptReceipt === undefined ? {} : { promptReceipt }),
    ...(environmentReceipt === undefined ? {} : { environmentReceipt }),
    ...(runEnvelopeReceipt === undefined ? {} : { runEnvelopeReceipt }),
    mcpActivity,
    nativeCommandActivity,
    executionSource: "north-managed",
    ...(activeQuery?.executionTransport === undefined
      ? {} : { executionTransport: activeQuery.executionTransport }),
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
    deliveryOutcome: terminal.deliveryOutcome,
    ...(terminal.deliveryReason === undefined ? {} : { deliveryReason: terminal.deliveryReason }),
    ...(terminal.deliveryProof === undefined ? {} : { deliveryProof: terminal.deliveryProof }),
    ...(retryContext === undefined ? {} : {
      retryOfRun: retryContext.retryOfRun,
      retryAttempt: retryContext.retryAttempt,
    }),
    judgmentGrade,
    struggleObservation: finalExecution.struggle,
  };
  const wireIdentity = {
    thread: boundThread,
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
  if (terminalJournalError) throw terminalJournalError;
  const struggleSnapshot = finalExecution.struggle;
  // Include turns + result size on the completion line. The banner-only stdout
  // .log is the artifact operators skim; without a work signal here a lane that
  // ran dozens of turns reads as identical to a zero-turn no-op (the 2026-07-21
  // "instant-DOA" misdiagnosis, where 33-47-turn process=ran lanes were reported
  // as dead because their work lives in the .stream.jsonl transcript, not stdout).
  const turnsLabel = numTurns != null
    ? `${numTurns}`
    : finalTurn?.unit === "provider-turn"
      ? `${finalTurn.count} provider turn(s)${finalTurn.toolItems != null ? `/${finalTurn.toolItems} items` : ""}`
      : "?";
  console.log(`[spawn] @agent:${agentId} complete (process=${outcome}, delivery=${terminal.deliveryOutcome}` +
    `, turns=${turnsLabel}, result=${result.length}b` +
    `${struggleSnapshot.triggers.length ? `, struggle: ${struggleSnapshot.triggers.join(",")}` : ""})`);
  const siblingTarget = routing.fallbackTargets.find((target) =>
    routing.routingTargets[target]?.provider === routing.provider,
  );
  return { result, outcome, runId, providerErrorDetail, numTurns, provider: routing.provider, siblingTarget };
  } finally {
    await stream?.close();
  }
}

// TRUE only in the import.meta.main adapter bootstrap below: that process runs
// under the CHILD's composed identity env (AGENT_TOPOLOGY=worker etc.), and the
// invoking adapter (bb agents-cli) already enforced the real caller's authority
// BEFORE composing it. Re-asserting here would read the child's topology as the
// caller's and deny every managed delegate (the 2026-07-17 self-deny bug).
let bootstrapAuthorityGranted = false;

export class RecursiveChildBindingError extends Error {
  readonly code = "NORTH_RECURSIVE_CHILD_BINDING_REQUIRED";
  readonly preSideEffect = true;

  constructor(message: string) {
    super(message);
    this.name = "RecursiveChildBindingError";
  }
}

function assertRecursiveChildBinding(
  composed: SpawnOptions,
  callerTopology: string | undefined,
  loadThreadFacts: typeof getThreadFacts,
): WireRunId | undefined {
  if (callerTopology !== "orchestrator") return undefined;
  const parentThread = process.env.NORTH_THREAD_ID;
  const parentRun = process.env.NORTH_RUN_ID;
  const parentCapability = process.env.NORTH_RUN_CAPABILITY;
  if (!parentThread || !parentRun || !parentCapability || !composed.thread) {
    throw new RecursiveChildBindingError(
      "recursive SDK spawn requires an exact managed parent run and a fresh child thread",
    );
  }
  let parentRunId: WireRunId;
  try {
    parentRunId = wireRunId(parentRun);
  } catch {
    throw new RecursiveChildBindingError(
      "recursive SDK spawn received an invalid parent run id",
    );
  }
  let child: string;
  let parent: string;
  try {
    child = normalizeNorthEntityId(composed.thread);
    parent = normalizeNorthEntityId(parentThread);
  } catch {
    throw new RecursiveChildBindingError(
      "recursive SDK spawn received an invalid parent or child thread id",
    );
  }
  if (child === parent) {
    throw new RecursiveChildBindingError(
      "recursive SDK spawn cannot reuse the parent thread as the child thread",
    );
  }
  let parents: string[];
  try {
    parents = loadThreadFacts(child)
      .filter((fact) => fact.predicate === "part_of")
      .map((fact) => normalizeNorthEntityId(fact.value));
  } catch {
    throw new RecursiveChildBindingError(
      "recursive SDK spawn could not verify the child thread parent link",
    );
  }
  if (parents.length !== 1 || parents[0] !== parent) {
    throw new RecursiveChildBindingError(
      "recursive SDK spawn requires exactly one child part_of link to its immediate parent thread",
    );
  }
  return parentRunId;
}

export async function spawn(opts: SpawnOptions): Promise<string> {
  const injected = takeSpawnTestRuntime<SpawnRuntime>(opts) ?? {};
  (injected.admitDispatchAuthority ?? admitManagedDispatchAuthority)();
  const admitted = allowlistedSpawnOptions(opts);
  const callerTopology = process.env.AGENT_TOPOLOGY;
  if (!bootstrapAuthorityGranted) assertCoordinationAuthority("spawn", callerTopology);
  const composed = composeSpawnOptions(admitted);
  let parentRunId: WireRunId | undefined;
  if (!bootstrapAuthorityGranted) {
    parentRunId = assertRecursiveChildBinding(
      composed, callerTopology, injected.loadThreadFacts ?? getThreadFacts,
    );
  }
  const requestedCapabilities = orchestrationCapabilities(composed.routingMetadata);
  const requestsMutation = hasAuthoringCapability(requestedCapabilities);
  if (requestsMutation && composed.worktree === false) {
    throw new Error(
      "managed mutation cannot opt out of a registered worktree allocation: remove worktree:false to use the default managed worktree lane, or drop mutation capabilities for a read-only lane; canonical checkout mutation denied",
    );
  }
  // Resolve the exact observer policy and immutable dispatcher grade before
  // clock/resource/provider side effects. Thread-backed spawns snapshot the
  // admission projection; raw ad-hoc work is explicitly unavailable.
  const strugglePolicy = resolveStrugglePolicy(composed.routingMetadata.topology!);
  assertExpectedStrugglePolicy(strugglePolicy);
  let judgmentGrade = adHocJudgmentGrade();
  if (composed.thread) {
    try {
      judgmentGrade = judgmentGradeFromThreadFacts(
        (injected.loadThreadFacts ?? getThreadFacts)(composed.thread),
      );
    } catch {
      judgmentGrade = judgmentGradeFromThreadFacts([]);
    }
  }
  const context = envelopeContextFromEnv();
  const agentId = composed.agentId ?? createSpawnAgentId();
  // Pin the generated id so admission, telemetry, and the provider run name the
  // same lane. Admission completes before entitlement refresh or provider query.
  composed.agentId = agentId;
  composed.sessionId = composed.sessionId ?? context.sessionId;
  const learning = decideManagedLearning({
    episodeId: agentId,
    taskSignature: {
      surface: "spawn",
      promptSha256: sha256Bytes(composed.prompt),
      role: composed.routingMetadata.role,
      taskGrade: composed.routingMetadata.taskGrade,
      topology: composed.routingMetadata.topology,
      domains: [...composed.routingMetadata.domainRequirements].sort(),
    },
    taskSignatureCoverage: "exact",
    routingMetadata: composed.routingMetadata,
    routingAssessment: composed.routingEconomics.assessment,
    pinEvidence: composed.routingEconomics.pinEvidence,
  });
  composed.routingMetadata = learning.routingMetadata;
  composed.routingAssessment = learning.routingAssessment;
  composed.tier = learning.routingMetadata.tier;
  composed.effort = learning.routingMetadata.reasoning;
  composed.routingEconomics = admitRoutingEconomics({
    request: learning.routingMetadata,
    routingAssessment: learning.routingAssessment,
    pinEvidence: composed.pinEvidence,
    provider: composed.provider,
    target: composed.target,
    model: composed.model,
    allowLegacyMissingPinEvidence: bootstrapLegacyPinCompatibilityGranted,
    surface: "managed North spawn learning admission",
  });
  const requestedTier = composed.tier;
  // Explicit isolation is an admission requirement, not a preference. Provision
  // before clocks, resource reservations, provider probes, stream/identity facts,
  // run reservations, or the provider query. A failure rejects this spawn and can
  // never silently execute in the shared checkout.
  let worktreeLease: ManagedWorktreeLease | undefined;
  if (composed.worktree) {
    const repoRoot = process.cwd();
    const allocationRunId = newRunId(agentId);
    try {
      const provisioned = provisionWorktree(agentId, {
        repoRoot,
        setupCmd: composed.setupCmd ?? process.env.AGENT_SETUP_CMD,
        runId: allocationRunId,
        thread: composed.thread,
        concern: composed.concern ?? process.env.NORTH_CONCERN_ID,
        provider: composed.provider,
        target: composed.target,
        writer: injected.worktreeAllocationWriter,
      });
      worktreeLease = { ...provisioned, finalized: false };
      console.log(
        `[spawn] @agent:${agentId} worktree ${provisioned.path} on ${provisioned.branch}`,
      );
    } catch (error) {
      throw new Error(
        `[spawn] @agent:${agentId} explicit worktree provisioning failed; `
        + `spawn aborted before provider execution: ${error instanceof Error ? error.message : String(error)}`,
        { cause: error },
      );
    }
  }
  const termination = new ManagedQueryTermination(
    injected?.registerTermination,
    {
      ...injected.sessionHardCapRuntime,
      agentId,
      threadId: composed.thread,
      goal: goalFromPrompt(composed.prompt),
      repo: process.cwd(),
      worktree: worktreeLease?.path,
      branch: worktreeLease?.branch,
    },
  );
  const lifecycleRoot = injected.journalRoot
    ?? (injected.queryFn ? undefined : bridgeJournalRoot());
  const openLifecycleJournal = (
    id: string,
    retryOfAgent?: string,
  ): ExecutionJournal | undefined => {
    if (!lifecycleRoot) return undefined;
    const journal = new ExecutionJournal(lifecycleRoot, id);
    journal.append(LANE_LIFECYCLE_KINDS.spawnStart, {
      prompt: composed.prompt,
      cwd: process.cwd(),
      thread: composed.thread ?? null,
      role: composed.routingMetadata.role,
      topology: composed.routingMetadata.topology,
      worktree: worktreeLease?.path ?? null,
      branch: worktreeLease?.branch ?? null,
      retryOfAgent: retryOfAgent ?? null,
    });
    return journal;
  };
  let lifecycleJournal = openLifecycleJournal(agentId);
  let admission: EnvelopeAdmission | undefined;
  let result!: string;
  let failed = false;
  let primaryError: unknown;
  try {
    termination.throwIfTerminated();
    admission = await (injected?.admitResourceEnvelope ?? admitResourceEnvelope)({
      agentId, tier: requestedTier, project: composed.project ?? context.project,
      sessionId: composed.sessionId ?? context.sessionId,
    });
    termination.throwIfTerminated();
    for (const advisory of admission?.advisories ?? [])
      console.warn(`[envelope] advisory: ${advisory}`);
    // Each attempt gets its OWN shallow copy: runSpawn resolves opts.model/
    // opts.effort onto its argument in place, and a retry must re-resolve from
    // the original request, not inherit the prior attempt's pinned resolution.
    let attempt = await runSpawn(
      { ...composed }, judgmentGrade, strugglePolicy,
      admission, injected, termination, worktreeLease,
      undefined, undefined, parentRunId, learning.assignment, lifecycleJournal,
    );
    let retries = 0;
    // The lane whose identity is terminal-committed by the attempt that just
    // finished; a retry mints a FRESH agent id rather than reusing it. Terminal
    // identities are immutable by design (identity.ts writeAgentTerminal) — a
    // second publish against the same @agent: subject is durably rejected
    // (status=not_committed reason=terminal_committed), so reuse is not an
    // option here, only a fresh mint linked back by provenance.
    let deadAgentId = agentId;
    while (retries < PROVIDER_PROCESS_DEATH_MAX_RETRIES) {
      const processDeathRetry = eligibleForProviderProcessDeathRetry(
        attempt.outcome, composed.routingMetadata.topology, requestedCapabilities,
      );
      const laneStartRetry = eligibleForLaneStartProviderRetry(
        attempt.outcome, attempt.providerErrorDetail, attempt.numTurns, attempt.siblingTarget,
      );
      if (!processDeathRetry && !laneStartRetry) break;
      retries++;
      const deadRunId = attempt.runId;
      const retryAgentId = createSpawnAgentId();
      const retryTarget = laneStartRetry ? attempt.siblingTarget : undefined;
      console.error(
        `[spawn] @agent:${deadAgentId} ${laneStartRetry ? "start-of-stream provider failure" : "provider-process death"} `
        + `(run @${deadRunId}) is retry-safe — retrying once as a fresh run${retryTarget ? ` on sibling target=${retryTarget}` : ""} on a fresh `
        + `@agent:${retryAgentId} (attempt ${retries})`,
      );
      termination.throwIfTerminated();
      lifecycleJournal?.close();
      lifecycleJournal = openLifecycleJournal(retryAgentId, deadAgentId);
      attempt = await runSpawn(
        { ...composed, agentId: retryAgentId }, judgmentGrade, strugglePolicy,
        admission, injected, termination, worktreeLease,
        { retryOfRun: deadRunId, retryAttempt: retries, retryOfAgent: deadAgentId }, retryTarget, parentRunId,
        learning.assignment, lifecycleJournal,
      );
      deadAgentId = retryAgentId;
    }
    result = attempt.result;
  } catch (error) {
    failed = true;
    primaryError = error;
  }
  try { await termination.close(); }
  catch (error) {
    if (!failed) {
      failed = true;
      primaryError = error;
    } else {
      primaryError = new AggregateError(
        [primaryError, error],
        "spawn execution and managed resource cleanup failed",
      );
    }
  }
  // Awaiting runSpawn proves every terminal/run publication attempt either
  // settled or was never reached. Keep the host barrier closed through every
  // outer cleanup that can otherwise be cut off by process.exit.
  termination.publicationSettled();
  const cleanupErrors: unknown[] = [];
  try { await (injected?.completeResourceEnvelope ?? completeResourceEnvelope)(admission); }
  catch (error) { cleanupErrors.push(error); }
  finally {
    termination.cleanupSettled();
    termination.release();
  }
  if (worktreeLease && !worktreeLease.finalized) {
    try { rollbackProvisionedWorktree(agentId, worktreeLease); }
    catch (error) { cleanupErrors.push(error); }
    finally { worktreeLease.finalized = true; }
  }
  if (failed && lifecycleJournal) {
    try {
      if (!lifecycleJournal.scan().records.some(
        (record) => record.kind === LANE_LIFECYCLE_KINDS.terminal,
      )) {
        lifecycleJournal.append(LANE_LIFECYCLE_KINDS.terminal, {
          outcome: "rejected",
          processOutcome: "blocked_preflight",
          deliveryOutcome: "blocked",
          deliveryReason: "spawn_rejected_before_terminal_publication",
          detail: terminalCause(primaryError),
        });
        lifecycleJournal.append(LANE_LIFECYCLE_KINDS.harvest, {
          status: "unavailable",
          branch: worktreeLease?.branch ?? null,
          sha: null,
          reason: "spawn rejected before terminal harvest",
        });
      }
    } catch { /* The primary spawn error remains authoritative. */ }
  }
  lifecycleJournal?.close();
  const errors = failed ? [primaryError, ...cleanupErrors] : cleanupErrors;
  if (errors.length === 1) throw errors[0];
  if (errors.length > 1)
    throw new AggregateError(errors, "spawn execution and outer cleanup failed");
  return result;
}

// Spawn multiple agents in parallel — the core win over the bash swarm.
export async function spawnParallel(
  tasks: SpawnOptions[]
): Promise<string[]> {
  assertCoordinationAuthority("spawnParallel");
  return Promise.all(tasks.map((t) => spawn(t)));
}

if (import.meta.main) {
  installSpawnTerminalHandlers();
  // Caller authority was enforced by the invoking adapter before it composed
  // this process's env with the child identity — see bootstrapAuthorityGranted.
  bootstrapAuthorityGranted = true;
  bootstrapLegacyPinCompatibilityGranted = true;
  const prompt = process.argv.slice(2).join(" ");
  if (!prompt) {
    console.error("usage: bun run src/spawn.ts <prompt>");
    process.exit(1);
  }
  // Each CLI/MCP launch owns one lane process. Default here because spawnParallel
  // may mix reasoning tiers inside one process.
  applyCodexTurnDeadlineFromReasoning();
  const rawDelegateThread = process.env.NORTH_DELEGATE_THREAD_ID;
  delete process.env.NORTH_DELEGATE_THREAD_ID;
  let delegateThread: string | undefined;
  if (rawDelegateThread !== undefined) {
    try {
      delegateThread = normalizeNorthEntityId(rawDelegateThread);
    } catch {
      console.error("managed delegate bootstrap received an invalid exact North thread id");
      process.exit(1);
    }
  }

  spawn({
    prompt,
    agentId: process.env.AGENT_ID,
    model: process.env.AGENT_MODEL,
    effort: process.env.AGENT_EFFORT as Effort | undefined,
    provider: process.env.AGENT_PROVIDER as ProviderPreference | undefined,
    target: process.env.AGENT_TARGET,
    tier: process.env.AGENT_TIER as SemanticTier | undefined,
    thread: delegateThread,
    coordinator: process.env.AGENT_COORDINATOR,
    routingMetadata: routingRequestFromEnv("managed North spawn bootstrap"),
    routingAssessment: process.env.AGENT_ROUTING_ASSESSMENT
      ? JSON.parse(process.env.AGENT_ROUTING_ASSESSMENT) : undefined,
    pinEvidence: process.env.NORTH_ROUTING_PIN_EVIDENCE
      ? JSON.parse(process.env.NORTH_ROUTING_PIN_EVIDENCE) : undefined,
  })
    .then((result) => console.log(result))
    .catch((err) => {
      appendSpawnTerminalLine("rejected", err);
      console.error(err);
      process.exit(1);
    });
}
