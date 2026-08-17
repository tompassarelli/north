import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { resolve } from "node:path";
import {
  beagleStoreBabashkaArguments,
  beagleStoreCoordinatorChildTimeout,
  beagleStoreEnvironment,
} from "./beagle-store";
import type { Fact } from "./north-client";
import {
  boundedDoneBars, canonicalDoneBars, MAX_DELIVERY_BARS,
  MAX_DELIVERY_WRITER_REQUEST_UTF8_BYTES,
  MAX_RUN_RESERVATION_BASELINE_UTF8_BYTES,
  parseRunBarEvidence, sha256, utf8ByteCount, validInstant,
  validAgentEntity, validRunEntity, validThreadEntity, validUnicodeScalars,
  validateUnreservedBarEvidence,
  type RunBarEvidence, type UnreservedBarEvidence,
} from "./delivery-verification";

const REPO = resolve(import.meta.dir, "..", "..");
const WRITER = resolve(REPO, "cli", "delivery-evidence-internal.clj");
export const RUN_RESERVATION_VERSION = "north:run-reservation:v1";
// Must exceed the writer's inner coordinator windows with margin, or it kills a
// healthy writer instead of letting it report its own typed refusal: read-retry
// budget 15s, per-read socket deadline 30s, publication deadline 60s, readback.
export const DELIVERY_RESERVATION_WRITER_TIMEOUT_MS = 240_000;
// Same shape one hop later: read-retry budget 15s, lease-wait budget 15s, and
// up to three fenced round-trips each bounded by the 30s per-read deadline.
export const DELIVERY_EVIDENCE_WRITER_TIMEOUT_MS = 180_000;
const RUN_RESERVATION_BODY = [
  "run_capability_sha256",
  "run_reservation_agent",
  "run_reservation_contract_origin",
  "run_reservation_done_when",
  "run_reservation_thread",
  "run_reservation_version",
  "run_reserved_at",
] as const;

export type DeliveryEvidenceWriterOperation = "reserve" | "record" | "record-unreserved";

export class DeliveryEvidenceRetryableError extends Error {
  readonly retryable = true;

  constructor(message: string) {
    super(message);
    this.name = "DeliveryEvidenceRetryableError";
  }
}

export class DeliveryEvidenceProofTransportFailure extends Error {
  readonly operation = "record";
  readonly reason = "proof-transport-failure";
  readonly retryable = false;

  constructor(message: string) {
    super(message);
    this.name = "DeliveryEvidenceProofTransportFailure";
  }
}

/**
 * A reservation attempt that never received a verdict, so a relaunch with the
 * SAME context is safe: publication is one atomic batch, so the replay either
 * finds its own complete reservation or a fresh run subject. A failure carrying
 * any verdict — refusal, publication deadline, outer-timeout kill — is not one.
 */
export abstract class DeliveryReservationReplayableFailure
  extends DeliveryEvidenceRetryableError {
  readonly operation = "reserve";
}

export class DeliveryReservationWriterProcessFailure
  extends DeliveryReservationReplayableFailure {
  readonly reason = "writer-process-failure";

  constructor(message: string) {
    super(message);
    this.name = "DeliveryReservationWriterProcessFailure";
  }
}

/** The writer's coordinator connection died mid-request: no verdict was read. */
export class DeliveryReservationCoordinatorTransportFailure
  extends DeliveryReservationReplayableFailure {
  readonly reason = "coordinator-transport-failure";

  constructor(message: string) {
    super(message);
    this.name = "DeliveryReservationCoordinatorTransportFailure";
  }
}

// Transport deaths from north.coord's wire client (cli/coord.clj). A response
// the writer never read is not a reservation verdict; anything else it prints,
// including a malformed or oversized frame, stays terminal.
const COORDINATOR_TRANSPORT_FAILURES = [
  "coordinator response deadline exceeded",
  "coordinator closed before sending a response line",
  "coordinator closed during a response line",
] as const;

function coordinatorTransportFailure(reason: string | undefined): boolean {
  return reason !== undefined
    && COORDINATOR_TRANSPORT_FAILURES.some((failure) => reason.includes(failure));
}

// A transport death carries no write verdict; record's bar+observed dedup makes a same-request replay safe.
export class DeliveryEvidenceRecordTransportFailure
  extends DeliveryEvidenceRetryableError {
  readonly operation = "record";
  readonly reason = "coordinator-transport-failure";

  constructor(message: string) {
    super(message);
    this.name = "DeliveryEvidenceRecordTransportFailure";
  }
}

export interface DeliveryRunContext {
  runId: string;
  threadId: string;
  reporterAgentId: string;
  capability: string;
}

export interface DeliveryReservation {
  contractOrigin: "accepted" | "worker-defined";
  baselineDoneWhen: string[];
}

// Reservation publication is the final pre-provider gate. A writer that never
// delivered a verdict is safe to relaunch with the SAME context because the
// coordinator publishes the complete reservation atomically and the writer
// recognizes an exact replay; the relaunch buys fresh coordinator windows, not
// a wait. Keep the policy here so spawn and dispatch cannot drift: one recovery
// attempt, one short backoff, and no retry after any acknowledgement.
export const DELIVERY_RESERVATION_RECOVERY_MAX_ATTEMPTS = 2;
export const DELIVERY_RESERVATION_RECOVERY_BACKOFF_MS = 100;

export interface DeliveryReservationRecoveryOptions {
  attempts?: number;
  backoffMs?: number;
  sleep?: (ms: number) => void;
  onRetry?: (
    error: DeliveryEvidenceRetryableError,
    nextAttempt: number,
    maxAttempts: number,
    backoffMs: number,
  ) => void;
}

export interface DeliveryRunState {
  reservationValid: boolean;
  evidence: RunBarEvidence[];
  /**
   * Set ONLY when the load itself never produced a fact list to judge (reader
   * timeout against a busy coordinator, nonzero exit, unparseable payload).
   * Absent means the facts were read and the verdict in `reservationValid` is a
   * judgment about their CONTENT. Collapsing the two is what made lanes
   * ms1awg94/ms1b7syb finalize unverified while their evidence sat intact on
   * the graph (thread 019f9cc1).
   */
  loadFailure?: string;
}

// A single `north json show` costs ~2.5-3.5s against an IDLE coordinator (bb
// startup dominates), so the old 5s per-attempt ceiling left under 2s of
// headroom and any write churn pushed the read past it. Per attempt gets real
// room, and the whole resolution stays well inside the terminal publication
// budget (90s default) that runs AFTER it.
const RUN_STATE_LOAD_TIMEOUT_MS = 15_000;
const RUN_STATE_LOAD_ATTEMPTS = 3;
const RUN_STATE_LOAD_BUDGET_MS = 40_000;
const RUN_STATE_LOAD_BACKOFF_MS = 500;

export interface DeliveryRunStateLoadOptions {
  attempts?: number;
  budgetMs?: number;
  backoffMs?: number;
  sleep?: (ms: number) => void;
  now?: () => number;
}

export interface DeliveryRunStateResolution {
  /** Last observed state; carries `loadFailure` iff every attempt failed. */
  state: DeliveryRunState;
  attempts: number;
  /** Bounded cause of the final failed load; absent when a load succeeded. */
  transientFailure?: string;
}

/** Sync sleep: the finalize seam is a synchronous chain of execFileSync writes. */
function sleepSync(ms: number): void {
  if (ms <= 0) return;
  const signal = new Int32Array(new SharedArrayBuffer(4));
  Atomics.wait(signal, 0, 0, ms);
}

/**
 * Relaunch a failed reservation writer only at the pre-provider reservation
 * seam. A successful acknowledgement returns immediately; a logical refusal
 * never retries; retry exhaustion rethrows the final typed failure unchanged.
 */
export function reserveDeliveryRunWithRecovery(
  context: DeliveryRunContext,
  reserve: (context: DeliveryRunContext) => DeliveryReservation,
  options: DeliveryReservationRecoveryOptions = {},
): DeliveryReservation {
  const maxAttempts = Math.max(
    1,
    Math.min(
      DELIVERY_RESERVATION_RECOVERY_MAX_ATTEMPTS,
      Math.floor(options.attempts ?? DELIVERY_RESERVATION_RECOVERY_MAX_ATTEMPTS),
    ),
  );
  const backoffMs = Math.max(
    0,
    Math.min(1_000, Math.floor(
      options.backoffMs ?? DELIVERY_RESERVATION_RECOVERY_BACKOFF_MS,
    )),
  );
  const sleep = options.sleep ?? sleepSync;

  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    try {
      return reserve(context);
    } catch (error) {
      if (!(error instanceof DeliveryReservationReplayableFailure)
          || attempt === maxAttempts) {
        throw error;
      }
      const delay = backoffMs * 2 ** (attempt - 1);
      options.onRetry?.(error, attempt + 1, maxAttempts, delay);
      sleep(delay);
    }
  }
  throw new Error("delivery reservation recovery exhausted without an attempt");
}

export function deliveryRunLoadFailureCause(error: unknown): string {
  const detail = error as { code?: unknown; signal?: unknown; status?: unknown };
  if (detail?.code === "ETIMEDOUT" || detail?.signal === "SIGTERM") return "reader timed out";
  if (typeof detail?.code === "string") return `reader failed: ${detail.code}`;
  if (typeof detail?.status === "number") return `reader exited ${detail.status}`;
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/\s+/g, " ").trim().slice(0, 200) || "reader failed";
}

/**
 * Retry a run-state load ONLY while the failure is the load itself. A load that
 * succeeded and found no valid reservation is a content verdict and is returned
 * on the first attempt — fail-closed posture for genuinely invalid reservations
 * is untouched.
 */
export function resolveDeliveryRunState(
  runId: string,
  load: (runId: string) => DeliveryRunState,
  options: DeliveryRunStateLoadOptions = {},
): DeliveryRunStateResolution {
  const attemptCap = Math.max(1, options.attempts ?? RUN_STATE_LOAD_ATTEMPTS);
  const budgetMs = Math.max(0, options.budgetMs ?? RUN_STATE_LOAD_BUDGET_MS);
  const backoffMs = Math.max(0, options.backoffMs ?? RUN_STATE_LOAD_BACKOFF_MS);
  const now = options.now ?? (() => performance.now());
  const sleep = options.sleep ?? sleepSync;
  const startedAt = now();
  let state: DeliveryRunState = { reservationValid: false, evidence: [], loadFailure: "not attempted" };
  let attempts = 0;
  while (attempts < attemptCap) {
    attempts++;
    try {
      state = load(runId);
    } catch (error) {
      state = {
        reservationValid: false,
        evidence: [],
        loadFailure: deliveryRunLoadFailureCause(error),
      };
    }
    if (!state.loadFailure) return { state, attempts };
    if (attempts >= attemptCap) break;
    const backoff = backoffMs * 2 ** (attempts - 1);
    if (now() - startedAt + backoff >= budgetMs) break;
    sleep(backoff);
  }
  return { state, attempts, transientFailure: state.loadFailure };
}

// Same shared budget discipline as the reservation load above (thread
// 019f9e0d, the deferred sibling of 019f9cc1): a contended coordinator read
// of the THREAD's own facts is not a verdict about the thread. `getThreadFacts`
// throws (never returns a partial result) when the reader never spoke or spoke
// garbage; a call that returns is a content read, even an empty one — an
// absent thread is a legitimate verdict, not a load failure, and must stay
// fail-closed on the first attempt.
const THREAD_FACTS_LOAD_ATTEMPTS = 3;
const THREAD_FACTS_LOAD_BUDGET_MS = 40_000;
const THREAD_FACTS_LOAD_BACKOFF_MS = 500;

export type ThreadFactsLoadOptions = DeliveryRunStateLoadOptions;

export interface ThreadFactsResolution {
  /** Facts from the last successful load; absent iff every attempt failed to speak. */
  facts?: readonly Fact[];
  attempts: number;
  /** Bounded cause of the final failed load; absent when a load succeeded. */
  transientFailure?: string;
}

/**
 * Retry a thread-facts load ONLY while the load itself never spoke. Mirrors
 * `resolveDeliveryRunState`: a thrown load (reader timeout/exit/garbage
 * payload) retries inside this budget; a load that returns — even `[]` for a
 * genuinely absent thread — is a content result and is final on attempt 1.
 */
export function resolveThreadFacts(
  threadId: string,
  load: (threadId: string) => readonly Fact[],
  options: ThreadFactsLoadOptions = {},
): ThreadFactsResolution {
  const attemptCap = Math.max(1, options.attempts ?? THREAD_FACTS_LOAD_ATTEMPTS);
  const budgetMs = Math.max(0, options.budgetMs ?? THREAD_FACTS_LOAD_BUDGET_MS);
  const backoffMs = Math.max(0, options.backoffMs ?? THREAD_FACTS_LOAD_BACKOFF_MS);
  const now = options.now ?? (() => performance.now());
  const sleep = options.sleep ?? sleepSync;
  const startedAt = now();
  let attempts = 0;
  let cause = "not attempted";
  while (attempts < attemptCap) {
    attempts++;
    try {
      return { facts: load(threadId), attempts };
    } catch (error) {
      cause = deliveryRunLoadFailureCause(error);
    }
    if (attempts >= attemptCap) break;
    const backoff = backoffMs * 2 ** (attempts - 1);
    if (now() - startedAt + backoff >= budgetMs) break;
    sleep(backoff);
  }
  return { attempts, transientFailure: cause };
}

export function newDeliveryRunContext(
  runId: string,
  threadId: string,
  reporterAgentId: string,
  capability = randomBytes(32).toString("hex"),
): DeliveryRunContext {
  const normalizedRun = runId.replace(/^@/, "");
  const normalizedThread = threadId.replace(/^@/, "");
  const normalizedAgent = reporterAgentId.replace(/^@?agent:/, "");
  if (!validRunEntity(`@${normalizedRun}`)) throw new Error("invalid delivery run id");
  if (!validThreadEntity(`@${normalizedThread}`)) throw new Error("invalid delivery thread id");
  if (!validAgentEntity(`@agent:${normalizedAgent}`)) {
    throw new Error("invalid delivery reporter id");
  }
  if (!/^[0-9a-f]{64}$/.test(capability)) throw new Error("invalid delivery run capability");
  return {
    runId: normalizedRun,
    threadId: normalizedThread,
    reporterAgentId: normalizedAgent,
    capability,
  };
}

function invokeWriter(
  operation: DeliveryEvidenceWriterOperation,
  request: Record<string, string>,
  port = process.env.NORTH_PORT ?? "7977",
): string {
  const invocation = deliveryWriterInvocation(operation, request, port);
  try {
    return execFileSync("bb", beagleStoreBabashkaArguments(invocation.argv), {
      encoding: "utf8",
      env: beagleStoreEnvironment(),
      input: invocation.stdin,
      stdio: ["pipe", "pipe", "pipe"],
      timeout: beagleStoreCoordinatorChildTimeout(operation === "reserve"
        ? DELIVERY_RESERVATION_WRITER_TIMEOUT_MS
        : DELIVERY_EVIDENCE_WRITER_TIMEOUT_MS),
    }).trim();
  } catch (error) {
    // Preserve only the writer's bounded semantic Message line. Even though the
    // live capability now travels on stdin rather than argv, subprocess errors
    // remain an inappropriate place to reflect the request body.
    const stderr = String((error as { stderr?: unknown }).stderr ?? "");
    throw deliveryEvidenceWriterError(operation, stderr, request, error);
  }
}

/** @internal Convert the writer's bounded Message line into caller semantics. */
export function deliveryEvidenceWriterError(
  operation: DeliveryEvidenceWriterOperation,
  stderr: string,
  request: Readonly<Record<string, string>> = {},
  processFailure?: unknown,
): Error & { retryable?: boolean } {
  let reason = stderr.match(/^Message:\s+(.+)$/m)?.[1]?.trim();
  let replayable: ((message: string) => DeliveryReservationReplayableFailure) | undefined;
  if (operation === "reserve" && !reason?.startsWith("run reservation refused:")) {
    const detail = processFailure as { code?: unknown; signal?: unknown };
    const processReason = detail?.code === "ETIMEDOUT" || detail?.signal === "SIGTERM"
      ? "writer-timeout"
      : coordinatorTransportFailure(reason)
        ? "coordinator-transport-failure"
        : reason
          ? "writer-refusal"
          : "writer-process-failure";
    if (processReason === "writer-process-failure") {
      replayable = (message) => new DeliveryReservationWriterProcessFailure(message);
    } else if (processReason === "coordinator-transport-failure") {
      replayable = (message) => new DeliveryReservationCoordinatorTransportFailure(message);
    }
    // The requested holder/run are validated before invocation. Do not include
    // the request body or capability: diagnostics are attributable without
    // turning a subprocess failure into a capability disclosure.
    const semanticDetail = reason ? ` detail=${reason}` : "";
    reason = `run reservation refused: run=@${request.run ?? "unavailable"}`
      + ` holder=@${request.reporter ?? "unavailable"}`
      + ` receipt=unavailable reason=${processReason}${semanticDetail}`;
  }
  if (operation === "record"
    && (!reason || reason.startsWith("PROOF_TRANSPORT_FAILURE:"))) {
    return new DeliveryEvidenceProofTransportFailure(
      "delivery evidence record rejected: PROOF_TRANSPORT_FAILURE:"
      + " run-bound proof publication was not acknowledged;"
      + " the task result remains valid and must not be repeated",
    );
  }
  const message = `delivery evidence ${operation} rejected${reason ? `: ${reason}` : ""}`;
  if (operation === "record" && coordinatorTransportFailure(reason)) {
    return new DeliveryEvidenceRecordTransportFailure(message);
  }
  if (replayable) return replayable(message);
  return reason?.startsWith("RETRYABLE:")
    || (operation === "reserve" && (
      reason?.includes("reason=writer-timeout")
      || reason?.includes("delivery evidence publication deadline exceeded")
    ))
    ? new DeliveryEvidenceRetryableError(message)
    : new Error(message);
}

export function deliveryReservationFailureCause(error: unknown): string {
  const message = error instanceof Error ? error.message : "";
  if (message.includes("delivery evidence publication deadline exceeded")) {
    return "publication deadline exceeded";
  }
  if (message.includes("run subject is not fresh")
    || message.includes("run reservation projection changed before commit")
    || message.includes("run reservation lost singleton/freshness race")
    || message.includes("reason=existing-reservation")) {
    return "reservation conflict";
  }
  if (message.includes("reason=writer-timeout")) return "writer timed out";
  if (message.includes("reason=writer-process-failure")) return "writer process failed";
  if (message.includes("reason=coordinator-transport-failure")) {
    return "coordinator transport failed";
  }
  if (message === "delivery evidence reserve returned a malformed acknowledgement") {
    return "malformed acknowledgement";
  }
  if (message === "delivery evidence reserve returned an invalid acknowledgement") {
    return "invalid acknowledgement";
  }
  if (message === "reservation acknowledgement unavailable") {
    return "acknowledgement unavailable";
  }
  if (message.includes("coordinator rejected delivery evidence write")) {
    return "coordinator rejected write";
  }
  // A read the coordinator never answered is NOT a verdict about the run or the
  // thread; naming it as one is what made an aborted query look like a malformed
  // subject. Keep it separable from "writer rejected reservation".
  if (message.includes("coordinator did not answer a delivery evidence read")) {
    return "coordinator read unavailable";
  }
  return "writer rejected reservation";
}

/** @internal Pure subprocess boundary used by the writer and its secrecy test. */
export function deliveryWriterInvocation(
  operation: DeliveryEvidenceWriterOperation,
  request: Record<string, string>,
  port: string,
): { argv: string[]; stdin: string } {
  const serialized = JSON.stringify(request);
  if (!validUnicodeScalars(serialized)
    || utf8ByteCount(serialized) > MAX_DELIVERY_WRITER_REQUEST_UTF8_BYTES) {
    throw new Error(`delivery evidence ${operation} rejected: request exceeds evidence limits`);
  }
  return { argv: [WRITER, port, operation], stdin: serialized };
}

export function reserveDeliveryRun(context: DeliveryRunContext): DeliveryReservation {
  const raw = invokeWriter("reserve", {
    run: context.runId,
    thread: context.threadId,
    reporter: `agent:${context.reporterAgentId}`,
    capabilitySha256: sha256(context.capability),
  });
  let parsed: Record<string, unknown>;
  try {
    parsed = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    throw new Error("delivery evidence reserve returned a malformed acknowledgement");
  }
  const baseline = parsed.baselineDoneWhen;
  const normalizedBaseline = Array.isArray(baseline)
    ? canonicalDoneBars(baseline)
    : undefined;
  const expectedKeys = [
    "baselineDoneWhen", "contractOrigin", "ok", "reporter", "run", "thread",
  ];
  if (Object.keys(parsed).sort().join("\0") !== expectedKeys.sort().join("\0")
    || parsed.ok !== true
    || parsed.run !== `@${context.runId}`
    || parsed.thread !== `@${context.threadId}`
    || parsed.reporter !== `@agent:${context.reporterAgentId}`
    || (parsed.contractOrigin !== "accepted" && parsed.contractOrigin !== "worker-defined")
    || !normalizedBaseline
    || !boundedDoneBars(normalizedBaseline, true)
    || JSON.stringify(baseline) !== JSON.stringify(normalizedBaseline)
    || (parsed.contractOrigin === "accepted"
      ? normalizedBaseline.length === 0
      : normalizedBaseline.length !== 0)) {
    throw new Error("delivery evidence reserve returned an invalid acknowledgement");
  }
  return {
    contractOrigin: parsed.contractOrigin,
    baselineDoneWhen: normalizedBaseline,
  };
}

export function deliveryRunEnvironment(context: DeliveryRunContext): Record<string, string> {
  return {
    NORTH_RUN_ID: context.runId,
    NORTH_THREAD_ID: context.threadId,
    NORTH_RUN_CAPABILITY: context.capability,
  };
}

export function loadDeliveryRunState(
  runId: string,
  command = process.env.NORTH_BIN ?? "north",
  timeoutMs = RUN_STATE_LOAD_TIMEOUT_MS,
): DeliveryRunState {
  let raw: string;
  try {
    raw = execFileSync(command, ["json", "show", runId.replace(/^@/, "")], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: Math.max(1, Math.floor(timeoutMs)),
    });
  } catch (error) {
    // The reader never spoke; say so instead of impersonating a verdict.
    return {
      reservationValid: false,
      evidence: [],
      loadFailure: deliveryRunLoadFailureCause(error),
    };
  }
  let facts: Array<{ predicate?: unknown; value?: unknown }>;
  try {
    facts = JSON.parse(raw) as Array<{ predicate?: unknown; value?: unknown }>;
  } catch {
    // A payload that is not even a fact list is a reader fault, not a verdict
    // on the reservation's content.
    return { reservationValid: false, evidence: [], loadFailure: "reader payload unparseable" };
  }
  if (!Array.isArray(facts)) {
    return { reservationValid: false, evidence: [], loadFailure: "reader payload not a fact list" };
  }
  if (!runReservationValid(facts)) {
    return { reservationValid: false, evidence: [] };
  }
  const one = (predicate: string): string | undefined => {
    const values = facts
      .filter((fact) => fact.predicate === predicate)
      .map((fact) => fact.value);
    return values.length === 1 && typeof values[0] === "string"
      ? values[0]
      : undefined;
  };
  const run = `@${runId.replace(/^@/, "")}`;
  const thread = one("run_reservation_thread");
  const reporter = one("run_reservation_agent");
  const rawEvidence = facts
    .filter((fact) => fact.predicate === "run_bar_evidence")
    .map((fact) => fact.value);
  if (!thread || !reporter || rawEvidence.length > MAX_DELIVERY_BARS
    || rawEvidence.some((value) => typeof value !== "string")) {
    return { reservationValid: false, evidence: [] };
  }
  const evidence = (rawEvidence as string[]).map(parseRunBarEvidence);
  if (evidence.some((record) =>
    !record
    || record.run !== run
    || record.thread !== thread
    || record.reporter !== reporter)
    || new Set(evidence.map((record) => record?.bar)).size !== evidence.length) {
    return { reservationValid: false, evidence: [] };
  }
  return { reservationValid: true, evidence: evidence as RunBarEvidence[] };
}

export function loadRunBarEvidence(
  runId: string,
  command = process.env.NORTH_BIN ?? "north",
): RunBarEvidence[] {
  return loadDeliveryRunState(runId, command).evidence;
}

export function runReservationValid(
  facts: readonly { predicate?: unknown; value?: unknown }[],
): boolean {
  const singleton = (predicate: string): string | undefined => {
    const values = facts
      .filter((fact) => fact.predicate === predicate)
      .map((fact) => fact.value);
    return values.length === 1 && typeof values[0] === "string" && values[0].length
      ? values[0]
      : undefined;
  };
  const projection = RUN_RESERVATION_BODY.map(
    (predicate) => [predicate, singleton(predicate)] as const,
  );
  if (projection.some(([, value]) => value === undefined)) return false;
  const marker = singleton("run_reservation_manifest_sha256");
  const body = Object.fromEntries(projection) as Record<
    typeof RUN_RESERVATION_BODY[number],
    string
  >;
  let baseline: unknown;
  if (!validUnicodeScalars(body.run_reservation_done_when)
    || utf8ByteCount(body.run_reservation_done_when)
      > MAX_RUN_RESERVATION_BASELINE_UTF8_BYTES) return false;
  try {
    baseline = JSON.parse(body.run_reservation_done_when);
  } catch {
    return false;
  }
  const normalizedBaseline = Array.isArray(baseline)
    ? canonicalDoneBars(baseline)
    : undefined;
  if (!marker
    || body.run_reservation_version !== RUN_RESERVATION_VERSION
    || !validAgentEntity(body.run_reservation_agent)
    || !validThreadEntity(body.run_reservation_thread)
    || !/^[0-9a-f]{64}$/.test(body.run_capability_sha256)
    || (body.run_reservation_contract_origin !== "accepted"
      && body.run_reservation_contract_origin !== "worker-defined")
    || !normalizedBaseline
    || !boundedDoneBars(normalizedBaseline, true)
    || JSON.stringify(baseline) !== JSON.stringify(normalizedBaseline)
    || (body.run_reservation_contract_origin === "accepted"
      ? normalizedBaseline.length === 0
      : normalizedBaseline.length !== 0)
    || !validInstant(body.run_reserved_at)) return false;
  const canonical = projection
    .map(([predicate, value]) => `${predicate}\0${value}\n`)
    .join("");
  return marker === sha256(canonical);
}

export function contextFromEnv(env: NodeJS.ProcessEnv = process.env): DeliveryRunContext {
  return newDeliveryRunContext(
    env.NORTH_RUN_ID ?? "",
    env.NORTH_THREAD_ID ?? "",
    env.AGENT_ID ?? "",
    env.NORTH_RUN_CAPABILITY ?? "",
  );
}

export function recordRunBarEvidence(
  bar: string,
  observed: string,
  env: NodeJS.ProcessEnv = process.env,
): RunBarEvidence {
  const context = contextFromEnv(env);
  const raw = invokeWriter("record", {
    run: context.runId,
    thread: context.threadId,
    reporter: `agent:${context.reporterAgentId}`,
    capability: context.capability,
    bar,
    observed,
  }, env.NORTH_PORT ?? "7977");
  const parsed = parseRunBarEvidence(raw);
  if (!parsed) throw new Error("delivery evidence writer returned a malformed record");
  return parsed;
}

/**
 * Record one THREAD-scoped observation with no run reservation.
 *
 * A lane whose reservation never happened (no NORTH_RUN_ID in its environment)
 * still ran real probes, and erroring the record away loses the observation
 * entirely. This lands it at a visibly lower tier — its own predicate, its own
 * marker, its own acknowledgement shape — so nothing downstream can read it as
 * run-bound verification, and no path upgrades it into one.
 */
export function recordUnreservedBarEvidence(
  threadId: string,
  bar: string,
  observed: string,
  env: NodeJS.ProcessEnv = process.env,
): UnreservedBarEvidence {
  const normalizedThread = threadId.replace(/^@/, "");
  if (!validThreadEntity(`@${normalizedThread}`)) {
    throw new Error("invalid delivery thread id");
  }
  const raw = invokeWriter("record-unreserved", {
    thread: normalizedThread,
    bar,
    observed,
  }, env.NORTH_PORT ?? "7977");
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error("delivery evidence writer returned a malformed record");
  }
  const record = validateUnreservedBarEvidence(parsed);
  if (!record) throw new Error("delivery evidence writer returned a malformed record");
  return record;
}

/** @internal Parsed `north evidence record` argv. */
export function parseEvidenceRecordArgv(
  argv: readonly string[],
): { bar: string; observed: string; thread?: string } | undefined {
  const positional: string[] = [];
  let thread: string | undefined;
  for (let index = 0; index < argv.length; index++) {
    const argument = argv[index]!;
    if (argument === "--thread") {
      const value = argv[++index];
      if (!value || thread) return undefined;
      thread = value;
      continue;
    }
    if (argument.startsWith("--thread=")) {
      const value = argument.slice("--thread=".length);
      if (!value || thread) return undefined;
      thread = value;
      continue;
    }
    positional.push(argument);
  }
  const [verb, bar, observed, ...extra] = positional;
  if (verb !== "record" || !bar || !observed || extra.length) return undefined;
  return { bar, observed, thread };
}

if (import.meta.main) {
  const parsed = parseEvidenceRecordArgv(process.argv.slice(2));
  if (!parsed) {
    console.error(
      "usage: north evidence record [--thread <id>] \"<exact done_when>\" \"<observed result>\"",
    );
    process.exit(2);
  }
  const { bar, observed, thread } = parsed;
  const runId = process.env.NORTH_RUN_ID?.trim();
  try {
    if (runId) {
      // A reserved lane keeps the fail-closed path exactly as it was: --thread
      // may only restate the reserved thread, never redirect the record.
      if (thread
        && thread.replace(/^@/, "") !== (process.env.NORTH_THREAD_ID ?? "").replace(/^@/, "")) {
        throw new Error(
          "--thread cannot redirect evidence away from the reserved thread of this run",
        );
      }
      console.log(JSON.stringify(recordRunBarEvidence(bar, observed)));
    } else {
      const fallbackThread = thread ?? process.env.NORTH_THREAD_ID;
      if (!fallbackThread) {
        throw new Error(
          "no managed run reservation (NORTH_RUN_ID unset) and no thread to record against"
          + " — rerun with: north evidence record --thread <id> \"<bar>\" \"<observed>\"",
        );
      }
      const record = recordUnreservedBarEvidence(fallbackThread, bar, observed);
      console.error(
        `north evidence: UNRESERVED — no run reservation in this environment, so this`
        + ` observation was recorded as thread-scoped evidence on ${record.thread}.`
        + ` It is NOT run-bound delivery verification and is never upgraded to one.`,
      );
      console.log(JSON.stringify(record));
    }
  } catch (error) {
    console.error(`north evidence: ${error instanceof Error ? error.message : String(error)}`);
    process.exit(1);
  }
}
