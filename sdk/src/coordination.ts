// Durable real-time coordination for managed SDK agents.
//
// One long-lived North feed process arms the coordinator subscription before it
// replays pending mail. Each machine-readable message is claimed by the feed, then
// acknowledged only after this host admits it into the active input channel.
// Process/feed crashes therefore replay instead of silently losing a message.
import { spawn as procSpawn, type ChildProcess } from "node:child_process";
import { resolve } from "node:path";
import { parseStrictJson } from "./strict-json";
import { trustedNorthBabashkaExecutable } from "./trusted-runtime";
import { beagleStoreBabashkaArguments, beagleStoreEnvironment } from "./beagle-store";
import type { WireUserInputMessage } from "./wire/query";

const REPO = resolve(import.meta.dir, "..", "..");
const LIVE_FEED = `${REPO}/cli/north-live-feed.clj`;
const DEFAULT_PORT = "7977";
const LIVE_FEED_PROTOCOL = "north-live-feed-v1";
const DEFAULT_FEED_MESSAGE_BYTES = 192 * 1024;
const DEFAULT_READY_TIMEOUT_MS = 10_000;
const DEFAULT_STARTUP_TIMEOUT_MS = 30_000;
const DEFAULT_ADMISSION_TIMEOUT_MS = 8_000;
const LIVE_FEED_ACK_TIMEOUT_MS = 10_000;
const DEFAULT_DRAIN_TIMEOUT_MS = 45_000;
const DEFAULT_STOP_KILL_MS = 1_000;
const DEFAULT_STOP_REAP_MS = 5_000;
const DEFAULT_DEDUPE_IDS = 4_096;
const MAX_ID_BYTES = 512;
const MAX_SENDER_BYTES = 1_024;
const MAX_SUBJECT_BYTES = 16 * 1024;
const MAX_BODY_BYTES = 128 * 1024;
const ROUTE_EPOCH =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const NIX_BABASHKA =
  /^\/nix\/store\/[0-9a-z]{32}-babashka(?:-[^/]+)?\/bin\/bb$/;

export interface InputAdmission {
  /** True only when the provider input iterator dequeues this exact turn. */
  readonly consumed: Promise<boolean>;
  /** Withdraw a still-queued turn; a consumed turn cannot be withdrawn. */
  readonly cancel: () => void;
}

type FeedAdmissionValue =
  | void
  | boolean
  | InputAdmission;
type FeedAdmission =
  | FeedAdmissionValue
  | PromiseLike<FeedAdmissionValue>;

export class LiveFeedConfigurationError extends Error {
  readonly code = "NORTH_LIVE_FEED_CONFIGURATION_INVALID";

  constructor() {
    super("trusted North Babashka executable unavailable");
    this.name = "LiveFeedConfigurationError";
  }
}

export class LiveFeedStoppedBeforeReadyError extends Error {
  readonly code = "NORTH_LIVE_FEED_STOPPED_BEFORE_READY";

  constructor() {
    super("North live feed stopped before its coordinator subscription was armed");
    this.name = "LiveFeedStoppedBeforeReadyError";
  }
}

export class LiveFeedStartupTimeoutError extends Error {
  readonly code = "NORTH_LIVE_FEED_STARTUP_TIMEOUT";

  constructor(readonly timeoutMs: number) {
    super("North live feed did not arm within its bounded startup budget");
    this.name = "LiveFeedStartupTimeoutError";
  }
}

export class LiveFeedReapTimeoutError extends Error {
  readonly code = "NORTH_LIVE_FEED_REAP_TIMEOUT";

  constructor(readonly timeoutMs: number) {
    super("North live feed did not reap after bounded termination");
    this.name = "LiveFeedReapTimeoutError";
  }
}

export interface FeedSubscription {
  /** Stop once and resolve only after the direct feed child is reaped. */
  (): Promise<void>;
  /** Resolves after the coordinator cursor is armed; deferred feeds have not replayed yet. */
  readonly ready: Promise<void>;
  /**
   * Resolves after the armed feed has completed its first durable pending-mail
   * replay attempt. An admitted message keeps this barrier open through provider
   * dequeue and the resulting durable graph acknowledgement.
   */
  readonly caughtUp: Promise<void>;
  /** Begin durable replay for a deferred feed and await its first empty scan. */
  readonly replay: () => Promise<void>;
  /**
   * Freeze-side barrier. Resolves only after the still-bound feed has observed
   * the frozen route and terminally settled every producer-admitted message that
   * was ordered before it.
   */
  readonly drain: (frozenRouteEpoch: string) => Promise<void>;
  /**
   * Diagnostic transport state. Once `ready` resolves, live-input capability is
   * durable across recoverable child restarts because graph mail is replayed;
   * callers must not downgrade that public capability merely for transient false.
   */
  readonly isArmed: () => boolean;
}

export interface SubscriptionRuntime {
  spawn?: typeof procSpawn;
  /** Test injection. Production must use the wrapper-owned Nix-store selector. */
  bbExecutable?: string;
  /** Test injection. Production resolves the coordinator port from NORTH_PORT at spawn time. */
  port?: string;
  schedule?: (callback: () => void, delayMs: number) => unknown;
  cancel?: (timer: unknown) => void;
  now?: () => number;
  initialBackoffMs?: number;
  maxBackoffMs?: number;
  healthyResetMs?: number;
  maxMessageBytes?: number;
  readyTimeoutMs?: number;
  startupTimeoutMs?: number;
  admissionTimeoutMs?: number;
  drainTimeoutMs?: number;
  stopKillMs?: number;
  stopReapMs?: number;
  dedupeIds?: number;
  /** Arm the coordinator cursor now, but leave graph mail unclaimed until replay(). */
  deferredStart?: boolean;
}

interface ReadyMessage {
  protocol: typeof LIVE_FEED_PROTOCOL;
  type: "ready";
  recipient: string;
  cursor: number;
}

interface CaughtUpMessage {
  protocol: typeof LIVE_FEED_PROTOCOL;
  type: "caught_up";
  recipient: string;
}

interface MailMessage {
  protocol: typeof LIVE_FEED_PROTOCOL;
  type: "mail";
  id: string;
  from: string;
  subject: string;
  body: string;
  wakeAttempt: string | null;
}

export interface FeedMail {
  readonly id: string;
  readonly wakeAttempt?: string;
  readonly summary: string;
}

interface DrainedMessage {
  protocol: typeof LIVE_FEED_PROTOCOL;
  type: "drained";
  recipient: string;
  epoch: string;
}

interface DrainProgressMessage {
  protocol: typeof LIVE_FEED_PROTOCOL;
  type: "drain_progress";
  recipient: string;
  epoch: string;
  settled: number;
}

interface ErrorMessage {
  protocol: typeof LIVE_FEED_PROTOCOL;
  type: "error";
  code: string;
  id?: string;
}

type FeedMessage =
  | ReadyMessage
  | CaughtUpMessage
  | MailMessage
  | DrainProgressMessage
  | DrainedMessage
  | ErrorMessage;

function positiveInteger(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value <= 0)
    throw new Error(`${label} must be a positive safe integer`);
  return value;
}

function peerBabashkaExecutable(injected: string | undefined): string {
  // An explicit in-process injection (SubscriptionRuntime.bbExecutable) is a
  // test seam — never attacker-reachable env — honored on the canonical
  // store-shape check alone. Production passes undefined and resolves the
  // immutable Nix-store bb from trusted entry pointers under the full realpath +
  // X_OK proof: managed children do not always inherit the wrapper's
  // NORTH_PEER_BB, so the feed's bb must be discoverable from the same immutable
  // system/profile layout as trusted Git, not a bare env assumption.
  if (injected !== undefined) {
    if (!NIX_BABASHKA.test(injected)) throw new LiveFeedConfigurationError();
    return injected;
  }
  try {
    return trustedNorthBabashkaExecutable();
  } catch {
    throw new LiveFeedConfigurationError();
  }
}

function userInputMessage(text: string): WireUserInputMessage {
  return Object.freeze({ kind: "user.input", text });
}

// A controllable input channel. `push` is the durable admission boundary: its
// promise resolves true only when the provider iterator dequeues that exact
// turn. Closing or cancelling first resolves false so graph mail can replay.
export function inputChannel(initial: string) {
  interface QueuedInput {
    message: WireUserInputMessage;
    state: "queued" | "consumed" | "cancelled";
    settle?: (consumed: boolean) => void;
  }
  const queue: QueuedInput[] = [{
    message: userInputMessage(initial),
    state: "queued",
  }];
  let wake: (() => void) | null = null;
  let closed = false;
  let liveMessagesReceived = 0;
  const cancelQueued = (entry: QueuedInput) => {
    if (entry.state !== "queued") return;
    entry.state = "cancelled";
    entry.settle?.(false);
  };
  return {
    push(text: string): InputAdmission {
      if (closed) {
        return {
          consumed: Promise.resolve(false),
          cancel: () => {},
        };
      }
      const { promise: consumed, resolve: settle } = Promise.withResolvers<boolean>();
      const entry: QueuedInput = {
        message: userInputMessage(text),
        state: "queued",
        settle,
      };
      queue.push(entry);
      wake?.();
      wake = null;
      return {
        consumed,
        cancel: () => cancelQueued(entry),
      };
    },
    end() {
      if (closed) return;
      closed = true;
      for (const entry of queue) {
        if (entry.settle) cancelQueued(entry);
      }
      wake?.();
      wake = null;
    },
    pending() {
      return queue.reduce(
        (count, entry) => count + (entry.state === "queued" ? 1 : 0),
        0,
      );
    },
    liveMessagesReceived() { return liveMessagesReceived; },
    async *stream(): AsyncGenerator<WireUserInputMessage> {
      while (true) {
        while (queue.length) {
          const entry = queue.shift()!;
          if (entry.state !== "queued") continue;
          entry.state = "consumed";
          if (entry.settle) {
            liveMessagesReceived++;
            entry.settle(true);
          }
          yield entry.message;
        }
        if (closed) return;
        const waiting = Promise.withResolvers<void>();
        wake = waiting.resolve;
        await waiting.promise;
      }
    },
  };
}

class LiveFeedLines {
  private fragments: Buffer[] = [];
  private bufferedBytes = 0;
  private readonly decoder = new TextDecoder("utf-8", { fatal: true });

  constructor(private readonly maxLineBytes: number) {
    positiveInteger(maxLineBytes, "maxMessageBytes");
  }

  push(value: Buffer | string): readonly string[] {
    const incoming = Buffer.isBuffer(value) ? value : Buffer.from(value);
    const lines: string[] = [];
    let start = 0;
    for (;;) {
      const newline = incoming.indexOf(0x0a, start);
      if (newline < 0) break;
      const segment = incoming.subarray(start, newline);
      if (this.bufferedBytes + segment.byteLength > this.maxLineBytes)
        throw new Error("North live-feed message exceeds its byte bound");
      if (segment.byteLength) {
        this.fragments.push(segment);
        this.bufferedBytes += segment.byteLength;
      }
      const raw = this.fragments.length === 1
        ? this.fragments[0]!
        : Buffer.concat(this.fragments, this.bufferedBytes);
      this.fragments = [];
      this.bufferedBytes = 0;
      let line: string;
      try { line = this.decoder.decode(raw); }
      catch { throw new Error("North live-feed emitted invalid UTF-8"); }
      if (!line.length) throw new Error("North live-feed emitted an empty message");
      lines.push(line);
      start = newline + 1;
    }
    const remainder = incoming.subarray(start);
    if (this.bufferedBytes + remainder.byteLength > this.maxLineBytes)
      throw new Error("North live-feed message exceeds its byte bound");
    if (remainder.byteLength) {
      this.fragments.push(remainder);
      this.bufferedBytes += remainder.byteLength;
    }
    return lines;
  }

  finish(): void {
    if (this.bufferedBytes)
      throw new Error("North live-feed closed with a partial message");
  }
}

function exactKeys(value: Record<string, unknown>, expected: readonly string[]): boolean {
  const keys = Object.keys(value).sort();
  const expectedKeys = [...expected].sort();
  return keys.length === expected.length
    && keys.every((key, index) => key === expectedKeys[index]);
}

function boundedString(
  value: unknown,
  maxBytes: number,
  pattern?: RegExp,
): value is string {
  return typeof value === "string"
    && Buffer.byteLength(value, "utf8") <= maxBytes
    && (pattern === undefined || pattern.test(value));
}

function feedMessage(line: string, maxMessageBytes: number): FeedMessage {
  const parsed = parseStrictJson(line, "North live-feed message", {
    maxBytes: maxMessageBytes,
    maxDepth: 2,
    maxNodes: 16,
  });
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed))
    throw new Error("North live-feed message must be an object");
  const message = parsed as Record<string, unknown>;
  if (message.protocol !== LIVE_FEED_PROTOCOL)
    throw new Error("North live-feed protocol mismatch");

  if (message.type === "ready") {
    if (!exactKeys(message, ["protocol", "type", "recipient", "cursor"])
        || !boundedString(message.recipient, MAX_ID_BYTES, /^[A-Za-z0-9][A-Za-z0-9._:-]*$/)
        || !Number.isSafeInteger(message.cursor)
        || (message.cursor as number) < 0)
      throw new Error("North live-feed ready message is malformed");
    return message as unknown as ReadyMessage;
  }

  if (message.type === "caught_up") {
    if (!exactKeys(message, ["protocol", "type", "recipient"])
        || !boundedString(message.recipient, MAX_ID_BYTES, /^[A-Za-z0-9][A-Za-z0-9._:-]*$/))
      throw new Error("North live-feed caught-up message is malformed");
    return message as unknown as CaughtUpMessage;
  }

  if (message.type === "mail") {
    if (!exactKeys(message, ["protocol", "type", "id", "from", "subject", "body", "wakeAttempt"])
        || !boundedString(message.id, MAX_ID_BYTES, /^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$/)
        || !boundedString(message.from, MAX_SENDER_BYTES)
        || !boundedString(message.subject, MAX_SUBJECT_BYTES)
        || !boundedString(message.body, MAX_BODY_BYTES)
        || (message.wakeAttempt !== null
            && !boundedString(message.wakeAttempt, 69, /^wake:[0-9a-f]{64}$/)))
      throw new Error("North live-feed mail message is malformed");
    return message as unknown as MailMessage;
  }

  if (message.type === "drain_progress") {
    if (!exactKeys(message, ["protocol", "type", "recipient", "epoch", "settled"])
        || !boundedString(message.recipient, MAX_ID_BYTES, /^[A-Za-z0-9][A-Za-z0-9._:-]*$/)
        || !boundedString(message.epoch, 36, ROUTE_EPOCH)
        || !Number.isSafeInteger(message.settled)
        || (message.settled as number) <= 0)
      throw new Error("North live-feed drain progress message is malformed");
    return message as unknown as DrainProgressMessage;
  }

  if (message.type === "drained") {
    if (!exactKeys(message, ["protocol", "type", "recipient", "epoch"])
        || !boundedString(message.recipient, MAX_ID_BYTES, /^[A-Za-z0-9][A-Za-z0-9._:-]*$/)
        || !boundedString(message.epoch, 36, ROUTE_EPOCH))
      throw new Error("North live-feed drained message is malformed");
    return message as unknown as DrainedMessage;
  }

  if (message.type === "error") {
    const keys = message.id === undefined
      ? ["protocol", "type", "code"]
      : ["protocol", "type", "code", "id"];
    if (!exactKeys(message, keys)
        || !boundedString(message.code, 128, /^[a-z][a-z0-9_]*$/)
        || (message.id !== undefined
            && !boundedString(message.id, MAX_ID_BYTES, /^@msg:[A-Za-z0-9][A-Za-z0-9._:-]*$/)))
      throw new Error("North live-feed error message is malformed");
    return message as unknown as ErrorMessage;
  }

  throw new Error("North live-feed message type is unknown");
}

function controlMessage(type: "start"): string;
function controlMessage(type: "drain", epoch: string): string;
function controlMessage(type: "ack" | "nack", id: string): string;
function controlMessage(
  type: "start" | "drain" | "ack" | "nack",
  value?: string,
): string {
  const message = type === "drain"
    ? { type, epoch: value }
    : value === undefined ? { type } : { type, id: value };
  return `${JSON.stringify(message)}\n`;
}

class BoundedRememberedIds {
  private readonly ids = new Set<string>();

  constructor(private readonly max: number) {
    positiveInteger(max, "dedupeIds");
  }

  has(id: string): boolean { return this.ids.has(id); }

  add(id: string): void {
    if (this.ids.delete(id)) this.ids.add(id);
    else {
      this.ids.add(id);
      while (this.ids.size > this.max) {
        const oldest = this.ids.values().next().value as string | undefined;
        if (oldest === undefined) break;
        this.ids.delete(oldest);
      }
    }
  }
}

function writeControl(child: ChildProcess, payload: string): boolean {
  if (!child.stdin || child.stdin.destroyed || !child.stdin.writable) return false;
  try {
    child.stdin.write(payload);
    return true;
  } catch {
    return false;
  }
}

interface NormalizedAdmission {
  readonly consumed: Promise<boolean>;
  readonly cancel: () => void;
}

function normalizeAdmission(value: FeedAdmission): NormalizedAdmission {
  const isInputAdmission = (candidate: unknown): candidate is InputAdmission =>
    typeof candidate === "object"
    && candidate !== null
    && "consumed" in candidate
    && "cancel" in candidate
    && typeof (candidate as InputAdmission).cancel === "function";
  let input: InputAdmission | null = null;
  let cancellationRequested = false;
  const cancelAdmission = () => {
    cancellationRequested = true;
    try { input?.cancel(); } catch { /* cancellation is fail-closed below */ }
  };
  return {
    consumed: Promise.resolve(value).then((admitted) => {
      if (!isInputAdmission(admitted)) return admitted !== false;
      input = admitted;
      if (cancellationRequested) cancelAdmission();
      return Promise.resolve(admitted.consumed).then(
        (consumed) => consumed === true,
        () => false,
      );
    }, () => false),
    cancel: cancelAdmission,
  };
}

function awaitAdmission(
  admission: NormalizedAdmission,
  timeoutMs: number,
  schedule: (callback: () => void, delayMs: number) => unknown,
  cancelTimer: (timer: unknown) => void,
): Promise<boolean> {
  const settlement = Promise.withResolvers<boolean>();
  let settled = false;
  let timer: unknown = null;
  const finish = (consumed: boolean) => {
    if (settled) return;
    settled = true;
    if (timer !== null) {
      cancelTimer(timer);
      timer = null;
    }
    settlement.resolve(consumed);
  };
  timer = schedule(() => {
    timer = null;
    admission.cancel();
    finish(false);
  }, timeoutMs);
  admission.consumed.then(
    (consumed) => finish(consumed),
    () => finish(false),
  );
  return settlement.promise;
}

// Subscribe one managed lane to its durable North mail feed. The callback is an
// admission operation. InputAdmission acknowledges only after the provider
// iterator dequeues the turn; end/cancel/error before dequeue nacks the claim.
// The host remembers consumed IDs across feed process restarts so
// crash-between-dequeue-and-graph-ack replay is acked without a second push.
function subscribeFeedMode(
  self: string,
  onMail: (mail: FeedMail) => FeedAdmission,
  runtime: SubscriptionRuntime,
  settlementOnly: boolean,
): FeedSubscription {
  const spawn = runtime.spawn ?? procSpawn;
  const bbExecutable = peerBabashkaExecutable(runtime.bbExecutable);
  // Resolve at spawn time so a per-lane / restarted coordinator port is honored,
  // and so hermetic suites pin it deterministically instead of racing ambient env.
  const feedPort = runtime.port ?? process.env.NORTH_PORT ?? DEFAULT_PORT;
  const schedule = runtime.schedule
    ?? ((callback: () => void, delayMs: number) => setTimeout(callback, delayMs));
  const cancel = runtime.cancel
    ?? ((timer: unknown) => clearTimeout(timer as NodeJS.Timeout));
  const now = runtime.now ?? Date.now;
  const initialBackoffMs = positiveInteger(runtime.initialBackoffMs ?? 250, "initialBackoffMs");
  const maxBackoffMs = positiveInteger(runtime.maxBackoffMs ?? 5_000, "maxBackoffMs");
  const healthyResetMs = positiveInteger(runtime.healthyResetMs ?? 30_000, "healthyResetMs");
  const maxMessageBytes = positiveInteger(
    runtime.maxMessageBytes ?? DEFAULT_FEED_MESSAGE_BYTES,
    "maxMessageBytes",
  );
  const readyTimeoutMs = positiveInteger(
    runtime.readyTimeoutMs ?? DEFAULT_READY_TIMEOUT_MS,
    "readyTimeoutMs",
  );
  const startupTimeoutMs = positiveInteger(
    runtime.startupTimeoutMs ?? DEFAULT_STARTUP_TIMEOUT_MS,
    "startupTimeoutMs",
  );
  const admissionTimeoutMs = positiveInteger(
    runtime.admissionTimeoutMs ?? DEFAULT_ADMISSION_TIMEOUT_MS,
    "admissionTimeoutMs",
  );
  if (admissionTimeoutMs >= LIVE_FEED_ACK_TIMEOUT_MS) {
    throw new Error(
      "admissionTimeoutMs must be smaller than the live-feed acknowledgement timeout",
    );
  }
  const drainTimeoutMs = positiveInteger(
    runtime.drainTimeoutMs ?? DEFAULT_DRAIN_TIMEOUT_MS,
    "drainTimeoutMs",
  );
  const stopKillMs = positiveInteger(runtime.stopKillMs ?? DEFAULT_STOP_KILL_MS, "stopKillMs");
  const stopReapMs = positiveInteger(runtime.stopReapMs ?? DEFAULT_STOP_REAP_MS, "stopReapMs");
  if (stopReapMs <= stopKillMs)
    throw new Error("stopReapMs must be greater than stopKillMs");
  const admittedIds = new BoundedRememberedIds(runtime.dedupeIds ?? DEFAULT_DEDUPE_IDS);
  const deferredStart = runtime.deferredStart === true;
  let stopped = false;
  let current: ChildProcess | null = null;
  let currentSettlement: Promise<void> | null = null;
  let retryTimer: unknown = null;
  let stopKillTimer: unknown = null;
  let stopReapTimer: unknown = null;
  let startupTimer: unknown = null;
  let cancelCurrentReadyTimer: (() => void) | null = null;
  let cancelCurrentAdmission: (() => void) | null = null;
  let requestCurrentDrain: (() => void) | null = null;
  let rapidFailures = 0;
  let armed = false;
  let replayRequested = !deferredStart;
  let drainRequested = false;
  let drainEpoch: string | null = null;
  let drainSettled = false;
  let drainTimer: unknown = null;
  const drainSettlement = Promise.withResolvers<void>();
  const drained = drainSettlement.promise;
  const resolveDrain = drainSettlement.resolve;
  const rejectDrain = drainSettlement.reject;
  void drained.catch(() => {});
  let readinessSettled = false;
  const readinessSettlement = Promise.withResolvers<void>();
  const readiness = readinessSettlement.promise;
  const resolveReadiness = readinessSettlement.resolve;
  const rejectReadiness = readinessSettlement.reject;
  // Existing stop-only callers need not install a rejection handler. Awaiters
  // still observe the original promise's typed stop-before-ready rejection.
  void readiness.catch(() => {});
  let caughtUpSettled = false;
  const caughtUpSettlement = Promise.withResolvers<void>();
  const caughtUp = caughtUpSettlement.promise;
  const resolveCaughtUp = caughtUpSettlement.resolve;
  const rejectCaughtUp = caughtUpSettlement.reject;
  void caughtUp.catch(() => {});
  let stopSettled = false;
  const stopping = Promise.withResolvers<void>();
  const stopSettlement = stopping.promise;
  const resolveStop = stopping.resolve;
  const rejectStop = stopping.reject;
  // Automatic startup-timeout termination has no caller waiting on stop().
  // A later explicit stop still receives this original settlement promise.
  void stopSettlement.catch(() => {});

  const settleStop = (error?: Error) => {
    if (stopSettled) return;
    stopSettled = true;
    if (stopKillTimer !== null) {
      cancel(stopKillTimer);
      stopKillTimer = null;
    }
    if (stopReapTimer !== null) {
      cancel(stopReapTimer);
      stopReapTimer = null;
    }
    if (error) rejectStop(error);
    else resolveStop();
  };

  const armDrainDeadline = () => {
    if (drainTimer !== null) cancel(drainTimer);
    drainTimer = schedule(() => {
      drainTimer = null;
      if (drainSettled) return;
      drainSettled = true;
      rejectDrain(new Error("North live-feed terminal drain timed out"));
    }, drainTimeoutMs);
  };

  const backoff = (): number =>
    Math.min(maxBackoffMs, initialBackoffMs * (2 ** Math.min(rapidFailures - 1, 20)));

  const start = () => {
    if (stopped) return;
    const startedAt = now();
    let child: ChildProcess;
    try {
      const childEnv = beagleStoreEnvironment();
      child = spawn(bbExecutable, beagleStoreBabashkaArguments([
        LIVE_FEED,
        feedPort,
        self,
        "--ack-timeout-ms",
        String(LIVE_FEED_ACK_TIMEOUT_MS),
        ...(settlementOnly ? ["--settlement-only", "true"] : []),
        ...(deferredStart ? ["--deferred-start", "true"] : []),
      ], childEnv), {
        env: childEnv,
        stdio: ["pipe", "pipe", "ignore"],
      });
    } catch {
      rapidFailures++;
      retryTimer = schedule(() => {
        retryTimer = null;
        start();
      }, backoff());
      return;
    }
    current = child;
    let childSettled = false;
    const childStopping = Promise.withResolvers<void>();
    const childSettlement = childStopping.promise;
    const resolveChildSettlement = childStopping.resolve;
    currentSettlement = childSettlement;
    child.stdin?.on("error", () => { /* close/replay is the recovery path */ });
    const lines = new LiveFeedLines(maxMessageBytes);
    let ready = false;
    let closed = false;
    let recoveryScheduled = false;
    let protocolFailed = false;
    let readyTimer: unknown = null;
    let activeAdmission: NormalizedAdmission | null = null;
    let drainSent = false;
    let lastDrainProgress = 0;
    let processing: Promise<void> = Promise.resolve();
    const clearReadyTimer = () => {
      if (readyTimer !== null) {
        cancel(readyTimer);
        readyTimer = null;
      }
      if (cancelCurrentReadyTimer === clearReadyTimer)
        cancelCurrentReadyTimer = null;
    };
    readyTimer = schedule(() => {
      readyTimer = null;
      if (cancelCurrentReadyTimer === clearReadyTimer)
        cancelCurrentReadyTimer = null;
      if (stopped || ready || closed) return;
      protocolFailed = true;
      try { child.kill("SIGKILL"); } catch { /* close schedules recovery */ }
    }, readyTimeoutMs);
    cancelCurrentReadyTimer = clearReadyTimer;

    const failProtocol = () => {
      if (protocolFailed || closed) return;
      protocolFailed = true;
      armed = false;
      activeAdmission?.cancel();
      try { child.kill("SIGKILL"); } catch { /* close schedules recovery */ }
    };

    const sendDrain = () => {
      if (
        !drainRequested
        || drainSettled
        || drainSent
        || !ready
        || stopped
        || protocolFailed
        || closed
      ) return;
      if (drainEpoch === null) {
        failProtocol();
        return;
      }
      drainSent = true;
      activeAdmission?.cancel();
      if (!writeControl(child, controlMessage("drain", drainEpoch)))
        failProtocol();
    };
    requestCurrentDrain = sendDrain;

    const handleMessage = async (message: FeedMessage): Promise<void> => {
      if (stopped || protocolFailed || closed) return;
      if (message.type === "ready") {
        if (ready || message.recipient !== self)
          throw new Error("North live-feed readiness is contradictory");
        ready = true;
        clearReadyTimer();
        if (replayRequested && !writeControl(child, controlMessage("start")))
          throw new Error("North live-feed start acknowledgement failed");
        armed = true;
        if (!readinessSettled) {
          readinessSettled = true;
          if (startupTimer !== null) {
            cancel(startupTimer);
            startupTimer = null;
          }
          resolveReadiness();
        }
        sendDrain();
        return;
      }
      if (!ready) throw new Error("North live-feed delivered before readiness");
      if (message.type === "caught_up") {
        if (!deferredStart || !replayRequested || message.recipient !== self)
          throw new Error("North live-feed caught-up state is contradictory");
        if (!caughtUpSettled) {
          caughtUpSettled = true;
          resolveCaughtUp();
        }
        return;
      }
      if (message.type === "drain_progress") {
        if (
          message.recipient !== self
          || message.epoch !== drainEpoch
          || !drainRequested
          || drainSettled
          || !drainSent
          || message.settled <= lastDrainProgress
        ) {
          throw new Error("North live-feed drain progress is contradictory");
        }
        lastDrainProgress = message.settled;
        armDrainDeadline();
        return;
      }
      if (message.type === "drained") {
        if (
          message.recipient !== self
          || message.epoch !== drainEpoch
          || !drainRequested
          || drainSettled
          || !drainSent
        ) {
          throw new Error("North live-feed drain acknowledgement is contradictory");
        }
        drainSettled = true;
        if (drainTimer !== null) {
          cancel(drainTimer);
          drainTimer = null;
        }
        resolveDrain();
        return;
      }
      if (message.type === "error") {
        console.error(
          `[north-feed] ${message.code}${message.id ? ` (${message.id})` : ""}`,
        );
        return;
      }

      if (drainRequested) {
        // The route is already frozen. Cancel any message that crossed the pipe
        // just before the freeze; the feed's terminal scan will reject managed
        // messages durably instead of admitting them into a dead provider input.
        if (!writeControl(child, controlMessage("nack", message.id)))
          throw new Error("North live-feed drain rejection failed");
        return;
      }

      if (admittedIds.has(message.id) && message.wakeAttempt === null) {
        // A prior feed died after provider dequeue but before durable graph ack.
        // Ordinary streaming input has no message-bound receipt authority, so
        // its remembered dequeue remains the only duplicate fence. A retained-
        // session wake must re-enter onMail: the typed receipt writer alone may
        // prove that its exact turn receipt still exists before graph ack.
        if (!writeControl(child, controlMessage("ack", message.id)))
          throw new Error("North live-feed replay acknowledgement failed");
        return;
      }

      let rawAdmission: FeedAdmission;
      try {
        rawAdmission = onMail({
          id: message.id,
          ...(message.wakeAttempt === null ? {} : { wakeAttempt: message.wakeAttempt }),
          summary: `[north real-time ping from ${message.from} — ${message.subject}]\n${message.body}`,
        });
      } catch {
        if (!writeControl(child, controlMessage("nack", message.id)))
          throw new Error("North live-feed rejection acknowledgement failed");
        return;
      }

      const admission = normalizeAdmission(rawAdmission);
      activeAdmission = admission;
      const cancelAdmission = () => admission.cancel();
      cancelCurrentAdmission = cancelAdmission;
      const consumed = await awaitAdmission(
        admission,
        admissionTimeoutMs,
        schedule,
        cancel,
      );
      if (activeAdmission === admission) activeAdmission = null;
      if (cancelCurrentAdmission === cancelAdmission)
        cancelCurrentAdmission = null;

      if (!consumed) {
        if (!writeControl(child, controlMessage("nack", message.id)))
          throw new Error("North live-feed rejection acknowledgement failed");
        return;
      }

      // Remember before the graph ack. If the feed dies on this write, its next
      // claim is acked without delivering the already-dequeued turn twice.
      admittedIds.add(message.id);
      if (!writeControl(child, controlMessage("ack", message.id)))
        throw new Error("North live-feed delivery acknowledgement failed");
    };

    const enqueueMessage = (message: FeedMessage) => {
      processing = processing
        .then(() => handleMessage(message))
        .catch(() => { failProtocol(); });
    };

    child.stdout?.on("data", (value: Buffer | string) => {
      if (stopped || protocolFailed || closed) return;
      try {
        // Validate the complete chunk before queueing any of it. A malformed
        // sibling message therefore cannot leave a valid prefix half-admitted.
        const messages = lines.push(value)
          .map((line) => feedMessage(line, maxMessageBytes));
        for (const message of messages) enqueueMessage(message);
      } catch {
        failProtocol();
      }
    });

    child.once("error", () => { /* `close` is the single recovery edge */ });
    child.once("close", () => {
      if (closed) return;
      closed = true;
      const closedAt = now();
      clearReadyTimer();
      armed = false;
      activeAdmission?.cancel();
      try { lines.finish(); }
      catch { protocolFailed = true; }
      if (current === child) current = null;
      if (requestCurrentDrain === sendDrain) requestCurrentDrain = null;
      if (stopKillTimer !== null) {
        cancel(stopKillTimer);
        stopKillTimer = null;
      }
      if (stopReapTimer !== null) {
        cancel(stopReapTimer);
        stopReapTimer = null;
      }
      void processing.then(() => {
        if (!childSettled) {
          childSettled = true;
          resolveChildSettlement();
        }
        if (stopped) {
          settleStop();
          return;
        }
        if (recoveryScheduled) return;
        recoveryScheduled = true;
        if (stopped) return;
        // A short-lived child always backs off, even when it delivered a message.
        // Otherwise a durable-ack failure could produce a zero-delay crash loop.
        const healthy = closedAt - startedAt >= healthyResetMs;
        if (healthy && !protocolFailed) rapidFailures = 0;
        else rapidFailures++;
        retryTimer = schedule(() => {
          retryTimer = null;
          start();
        }, healthy && !protocolFailed ? 0 : backoff());
      });
    });
  };

  const terminate = (readinessError: Error): Promise<void> => {
    if (stopped) return stopSettlement;
    stopped = true;
    armed = false;
    if (!readinessSettled) {
      readinessSettled = true;
      rejectReadiness(readinessError);
    }
    if (!caughtUpSettled) {
      caughtUpSettled = true;
      rejectCaughtUp(readinessError);
    }
    if (startupTimer !== null) {
      cancel(startupTimer);
      startupTimer = null;
    }
    if (retryTimer !== null) {
      cancel(retryTimer);
      retryTimer = null;
    }
    cancelCurrentReadyTimer?.();
    cancelCurrentAdmission?.();
    requestCurrentDrain = null;
    if (!drainSettled) {
      drainSettled = true;
      if (drainTimer !== null) {
        cancel(drainTimer);
        drainTimer = null;
      }
      rejectDrain(readinessError);
    }
    const child = current;
    const childSettlement = currentSettlement;
    if (childSettlement === null) {
      settleStop();
      return stopSettlement;
    }
    void childSettlement.then(() => settleStop());
    if (!child) return stopSettlement;
    try { child.stdin?.end(); } catch { /* termination signal remains authoritative */ }
    try { child.kill("SIGTERM"); } catch { /* close/settlement decides the outcome */ }
    stopKillTimer = schedule(() => {
      stopKillTimer = null;
      if (stopSettled || current !== child) return;
      try { child.kill("SIGKILL"); } catch { /* reap deadline decides the outcome */ }
    }, stopKillMs);
    stopReapTimer = schedule(() => {
      stopReapTimer = null;
      if (stopSettled) return;
      try { child.stdin?.destroy(); } catch { /* release local process handles */ }
      try { child.stdout?.destroy(); } catch { /* release local process handles */ }
      try { child.stderr?.destroy(); } catch { /* release local process handles */ }
      try { child.unref(); } catch { /* release what the runtime exposes */ }
      settleStop(new LiveFeedReapTimeoutError(stopReapMs));
    }, stopReapMs);
    return stopSettlement;
  };
  const stop = (() => terminate(new LiveFeedStoppedBeforeReadyError())) as FeedSubscription;
  const replay = (): Promise<void> => {
    if (stopped) return Promise.reject(new Error("North live feed is already stopped"));
    if (!replayRequested) {
      replayRequested = true;
      const child = current;
      if (armed && child && !writeControl(child, controlMessage("start"))) {
        try { child.kill("SIGKILL"); } catch { /* close/replay is the recovery path */ }
      }
    }
    return caughtUp;
  };
  const drain = (frozenRouteEpoch: string) => {
    if (drainRequested) {
      return frozenRouteEpoch === drainEpoch
        ? drained
        : Promise.reject(new Error("North live-feed drain epoch changed"));
    }
    if (stopped) return Promise.reject(new Error("North live feed is already stopped"));
    if (!ROUTE_EPOCH.test(frozenRouteEpoch))
      return Promise.reject(new Error("North live-feed drain epoch is malformed"));
    drainRequested = true;
    drainEpoch = frozenRouteEpoch;
    cancelCurrentAdmission?.();
    armDrainDeadline();
    requestCurrentDrain?.();
    return drained;
  };
  Object.defineProperties(stop, {
    ready: { value: readiness, enumerable: true },
    caughtUp: { value: caughtUp, enumerable: true },
    replay: { value: replay, enumerable: true },
    drain: { value: drain, enumerable: true },
    isArmed: { value: () => armed, enumerable: true },
  });
  startupTimer = schedule(() => {
    startupTimer = null;
    void terminate(new LiveFeedStartupTimeoutError(startupTimeoutMs));
  }, startupTimeoutMs);
  start();
  return stop;
}

export function subscribeFeed(
  self: string,
  onMail: (mail: FeedMail) => FeedAdmission,
  runtime: SubscriptionRuntime = {},
): FeedSubscription {
  return subscribeFeedMode(self, onMail, runtime, false);
}

/**
 * Arm a feed that never admits ordinary mail and exists only to settle
 * manifest-marked messages against an already-frozen managed route.
 */
export function subscribeSettlementFeed(
  self: string,
  runtime: SubscriptionRuntime = {},
): FeedSubscription {
  return subscribeFeedMode(self, () => false, runtime, true);
}
