import { execFileSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import { resolve } from "node:path";
import {
  LiveFeedStoppedBeforeReadyError,
  LiveFeedStartupTimeoutError,
  subscribeFeed,
  subscribeSettlementFeed,
  type FeedSubscription,
  type FeedMail,
  type InputAdmission,
  type SubscriptionRuntime,
} from "./coordination";
import { beagleStoreBabashkaArguments, beagleStoreEnvironment } from "./beagle-store";
import { trustedNorthBabashkaExecutable } from "./trusted-runtime";
import {
  updateAgentRoute,
  type AgentIdentity,
  type LiveInputState,
  type ManagedWriteResult,
} from "./identity";
import {
  providerPreacceptError, ProviderRetrySafeError,
  type LiveInputCapability,
  type ProviderFallbackTransition,
} from "./providers/types";

export interface ManagedRouteAxes {
  provider: string;
  providerTarget: string;
  liveInput: LiveInputCapability;
  model?: string;
  effort?: string;
}

export interface ManagedContinuationGate {
  readonly signal: AbortSignal;
  throwIfTerminated(): void;
}

type ManagedRouteIdentityBase = Omit<
  AgentIdentity,
  | "provider"
  | "providerTarget"
  | "liveInput"
  | "liveInputState"
  | "liveInputEpoch"
  | "model"
  | "effort"
>;

type FeedSubscriber = (
  recipient: string,
  onMessage: (message: FeedMail) => InputAdmission,
  runtime?: SubscriptionRuntime,
) => FeedSubscription;

type RouteWriter = (
  agentId: string,
  identity: AgentIdentity,
) => ManagedWriteResult | void;

export interface WakeReceiptContext {
  readonly messageId: string;
  readonly attemptId: string;
  readonly target: string;
  readonly routeEpoch: string;
}

type WakeReceiptPhase = "idle" | "turn" | "action" | "failure";
type WakeReceiptWriteResult = "created" | "existing" | "accepted" | "unknown";
interface WakeObservedEvent {
  readonly id: string;
  readonly kind: string;
  readonly modelCallId?: string;
  readonly role?: string;
  readonly stage?: string;
}
interface WakeIdleEvent {
  readonly id: string;
  readonly kind: "model-call.completed";
  readonly modelCallId: string;
}
type WakeReceiptWriter = (
  context: WakeReceiptContext,
  phase: WakeReceiptPhase,
  eventOrReason: string,
  kind?: string,
) => WakeReceiptWriteResult;

const WAKE_RECEIPT = resolve(import.meta.dir, "..", "..", "cli", "wake-receipt-internal.clj");

const writeWakeReceipt: WakeReceiptWriter = (context, phase, eventOrReason, kind) => {
  const port = process.env.NORTH_PORT ?? "7977";
  const result = execFileSync(
    trustedNorthBabashkaExecutable(),
    beagleStoreBabashkaArguments([
      WAKE_RECEIPT, port, phase, context.messageId, context.attemptId,
      context.target, context.routeEpoch, eventOrReason,
      ...(kind === undefined ? [] : [kind]),
    ]),
    {
      encoding: "utf8",
      env: beagleStoreEnvironment(),
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 10_000,
    },
  ).trim();
  if (!["created", "existing", "accepted", "unknown"].includes(result))
    throw new Error("wake receipt writer returned an invalid result");
  return result as WakeReceiptWriteResult;
};

interface PublishedRoute extends ManagedRouteAxes {
  liveInputState: LiveInputState;
  liveInputEpoch: string;
}

function initialState(capability: LiveInputCapability): LiveInputState {
  return capability === "unsupported" ? "frozen" : "pending";
}

function rejectedAdmission(): InputAdmission {
  return {
    consumed: Promise.resolve(false),
    cancel: () => {},
  };
}

function acceptedAdmission(): InputAdmission {
  return {
    consumed: Promise.resolve(true),
    cancel: () => {},
  };
}

function waitForAbort(signal: AbortSignal): {
  promise: Promise<false>;
  detach: () => void;
} {
  const settlement = Promise.withResolvers<false>();
  const abort = () => settlement.resolve(false);
  if (signal.aborted) abort();
  else signal.addEventListener("abort", abort, { once: true });
  return {
    promise: settlement.promise,
    detach: () => signal.removeEventListener("abort", abort),
  };
}

function semanticKey(route: ManagedRouteAxes, state: LiveInputState): string {
  return JSON.stringify([
    route.provider,
    route.providerTarget,
    route.liveInput,
    state,
    route.model ?? "",
    route.effort ?? "",
  ]);
}

function readinessProof(subscription: FeedSubscription): Promise<void> {
  if (
    typeof subscription !== "function"
    || !subscription.ready
    || typeof subscription.ready.then !== "function"
  ) {
    throw new Error("North live-feed subscription did not expose a readiness proof");
  }
  return subscription.ready;
}

/**
 * One managed lane's live-input state machine.
 *
 * The graph route is the public authority boundary. A feed becomes usable only
 * after its coordinator subscription is ready and the matching `armed`
 * generation commits. Freezing commits before unbinding, and every publication
 * mints a new UUIDv4 epoch so a message admitted against an older route can never
 * become valid again after a freeze/re-arm cycle.
 */
export class ManagedLiveInputRoute {
  private published: PublishedRoute;
  private subscription: FeedSubscription | undefined;
  private everArmedStreaming = false;
  private settlementRequired = false;
  private unbindSettlement: Promise<void> = Promise.resolve();
  private readonly settlementFeedSubscriber: FeedSubscriber;
  #followUpState: "open" | "queued" | "consumed" = "open";
  #followUpOwner: object | undefined;
  #followUpQueued = Promise.withResolvers<void>();
  #idleEvent: WakeIdleEvent | undefined;
  #wake: (WakeReceiptContext & {
    idleModelCallId: string;
    turnEventId?: string;
    modelCallId?: string;
    actionEventId?: string;
    hostAccepted: PromiseWithResolvers<boolean>;
  }) | undefined;
  #failedTurn: (WakeReceiptContext & {
    idleModelCallId: string;
    turnEventId: string;
    modelCallId: string;
  }) | undefined;

  #openFollowUpSlot(): void {
    this.#followUpOwner = undefined;
    this.#followUpState = "open";
    this.#followUpQueued = Promise.withResolvers<void>();
  }

  #resetFollowUpSlot(owner: object): void {
    if (this.#followUpOwner !== owner || this.#followUpState !== "queued") return;
    this.#openFollowUpSlot();
  }

  #admitMessage = (message: FeedMail): InputAdmission => {
    if (this.published.liveInput !== "turn-messages") return this.pushMessage(message.summary);
    if (this.#followUpState !== "open") return rejectedAdmission();
    if (!message.wakeAttempt) return rejectedAdmission();

    const wake: WakeReceiptContext = {
      messageId: message.id,
      attemptId: message.wakeAttempt,
      target: this.agentId,
      routeEpoch: this.published.liveInputEpoch,
    };
    const failedTurn = this.#failedTurn;
    if (failedTurn) {
      if (
        failedTurn.messageId !== wake.messageId
        || failedTurn.attemptId !== wake.attemptId
        || failedTurn.target !== wake.target
        || failedTurn.routeEpoch !== wake.routeEpoch
      ) return rejectedAdmission();
      try {
        const retry = this.wakeReceiptWriter(
          wake,
          "turn",
          failedTurn.turnEventId,
        );
        if (retry !== "created" && retry !== "existing") return rejectedAdmission();
        this.#failedTurn = undefined;
        return acceptedAdmission();
      } catch {
        return rejectedAdmission();
      }
    }
    const idleEvent = this.#idleEvent;
    // This write is part of admission. If exact listener/message identity or
    // the Store readback fails, the feed nacks and leaves the message replayable.
    // An exact existing receipt is revalidated even after restart, when this
    // route has no in-memory completion candidate. A new attempt still requires
    // a completion observed through the canonical committed-event seam.
    const intent = this.wakeReceiptWriter(wake, "idle", idleEvent?.id ?? "");
    // A restarted listener never replays a post-intent turn. It may acknowledge
    // only when the typed receipt authority resolves an exact durable host
    // acceptance; an intent without that receipt remains unknown and replayable.
    if (intent === "accepted") return acceptedAdmission();
    if (intent !== "created" || !idleEvent) return rejectedAdmission();

    const owner = {};
    const admission = this.pushMessage(message.summary);
    const hostAccepted = Promise.withResolvers<boolean>();
    this.#followUpOwner = owner;
    this.#followUpState = "queued";
    this.#followUpQueued.resolve();
    const consumed = Promise.resolve(admission.consumed).then(
      (value) => {
        if (value === true) {
          if (this.#followUpOwner === owner) {
            this.#followUpOwner = undefined;
            this.#followUpState = "consumed";
            this.#wake = {
              ...wake,
              idleModelCallId: idleEvent.modelCallId,
              hostAccepted,
            };
          }
          return hostAccepted.promise;
        }
        this.#resetFollowUpSlot(owner);
        return false;
      },
      () => {
        this.#resetFollowUpSlot(owner);
        return false;
      },
    );
    return {
      consumed,
      cancel: () => {
        admission.cancel();
        hostAccepted.resolve(false);
      },
    };
  };

  constructor(
    private readonly agentId: string,
    private readonly identityBase: ManagedRouteIdentityBase,
    initialRoute: ManagedRouteAxes,
    private readonly pushMessage: (message: string) => InputAdmission,
    private readonly feedSubscriber: FeedSubscriber = subscribeFeed,
    private readonly routeWriter: RouteWriter = updateAgentRoute,
    settlementFeedSubscriber?: FeedSubscriber,
    private readonly wakeReceiptWriter: WakeReceiptWriter = writeWakeReceipt,
  ) {
    this.settlementFeedSubscriber = settlementFeedSubscriber
      ?? (feedSubscriber === subscribeFeed
        ? ((recipient) => subscribeSettlementFeed(recipient))
        : feedSubscriber);
    this.published = {
      ...initialRoute,
      liveInputState: initialState(initialRoute.liveInput),
      liveInputEpoch: randomUUID(),
    };
  }

  initialProjection(): Pick<
    PublishedRoute,
    "liveInputState" | "liveInputEpoch"
  > {
    return {
      liveInputState: this.published.liveInputState,
      liveInputEpoch: this.published.liveInputEpoch,
    };
  }

  /** Read-only host gate for an already durable blocker; it grants no route mutation. */
  isArmed(): boolean {
    return this.published.liveInputState === "armed";
  }

  private publish(
    route: ManagedRouteAxes,
    state: LiveInputState,
    required: boolean,
  ): void {
    if (route.liveInput === "unsupported" && state !== "frozen")
      throw new Error("unsupported live-input route cannot be published as non-frozen");
    if (semanticKey(route, state)
        === semanticKey(this.published, this.published.liveInputState))
      return;
    const next: PublishedRoute = {
      ...route,
      liveInputState: state,
      liveInputEpoch: randomUUID(),
    };
    try {
      const acknowledgement = this.routeWriter(
        this.agentId,
        { ...this.identityBase, ...next },
      );
      if (acknowledgement && acknowledgement.status !== "committed") {
        throw new Error(
          `managed route publication ${acknowledgement.status}`
          + (acknowledgement.reason ? `: ${acknowledgement.reason}` : ""),
        );
      }
      this.published = next;
      if (state === "armed" && route.liveInput === "streaming")
        this.everArmedStreaming = this.settlementRequired = true;
    } catch (error) {
      if (required) throw error;
    }
  }

  refresh(route: ManagedRouteAxes, required = false): void {
    this.publish(route, this.published.liveInputState, required);
  }

  private async unbind(): Promise<void> {
    const subscription = this.subscription;
    this.subscription = undefined;
    if (subscription) {
      let currentSettlement: Promise<void>;
      try {
        currentSettlement = Promise.resolve(subscription());
      } catch (error) {
        currentSettlement = Promise.reject(error);
      }
      // Stopping a later transport must never erase an earlier failed child
      // settlement. Repeated callers share this cumulative promise, so a reap
      // timeout remains terminal while successful unbinds stay idempotent.
      void currentSettlement.catch(() => {});
      this.unbindSettlement = this.unbindSettlement.then(
        () => currentSettlement,
        (error) => { throw error; },
      );
      void this.unbindSettlement.catch(() => {});
    }
    await this.unbindSettlement;
  }

  private async drainAndUnbind(): Promise<void> {
    if (!this.settlementRequired) {
      await this.unbind();
      return;
    }
    let subscription = this.subscription;
    if (!subscription) {
      // A fresh settlement feed can recover a failed graph publication or
      // drain only after the prior transport proved that its child reaped.
      // Re-awaiting this shared settlement replays a terminal stop rejection
      // instead of manufacturing success from an absent subscription.
      await this.unbindSettlement;
      // A prior freeze write or drain may have failed after transport teardown.
      // Recovery must earn a fresh barrier rather than treating absence as
      // success: arm a dedicated settlement feed against the now-frozen route.
      try {
        subscription = this.settlementFeedSubscriber(
          this.agentId,
          (mail) => this.pushMessage(mail.summary),
        );
        await readinessProof(subscription);
        this.subscription = subscription;
      } catch (error) {
        if (subscription) await subscription();
        throw error;
      }
    }
    try {
      if (typeof subscription.drain !== "function")
        throw new Error("North live-feed subscription did not expose a drain barrier");
      await subscription.drain(this.published.liveInputEpoch);
      this.settlementRequired = false;
    } finally {
      await this.unbind();
    }
  }

  async #bindFeed(deferredStart = false): Promise<FeedSubscription> {
    const subscription = this.feedSubscriber(
      this.agentId,
      this.#admitMessage,
      deferredStart ? { deferredStart: true } : undefined,
    );
    try {
      await readinessProof(subscription);
      return subscription;
    } catch (error) {
      await subscription();
      throw error;
    }
  }

  /**
   * Called only after provider admission and before provider.query. Supported
   * candidates are published only after the feed proves ready.
   */
  async activate(
    route: ManagedRouteAxes,
    terminalFollowUpCapable = false,
  ): Promise<void> {
    if (route.liveInput === "unsupported") {
      this.publish(route, "frozen", true);
      return;
    }
    if (route.liveInput === "turn-messages" && !terminalFollowUpCapable) {
      this.publish(route, "pending", true);
      return;
    }
    if (this.subscription) {
      // A resumed continuation turn (thread 019f8ec5) re-enters provider
      // admission, whose callback re-invokes activate on the SAME managed route.
      // The turn-1 streaming feed is still bound and armed; minting a second
      // coordinator subscription against one route is the "already has a bound
      // feed" death. An armed feed on a semantically-identical route already IS
      // this turn's feed — reuse it. A genuinely different route while a feed is
      // bound stays a bug (fallback freezes+unbinds before re-activating).
      if (
        this.published.liveInputState === "armed"
        && semanticKey(route, "armed")
           === semanticKey(this.published, this.published.liveInputState)
      ) {
        return;
      }
      throw new Error("managed live-input route already has a bound feed");
    }
    let subscription: FeedSubscription | undefined;
    try {
      subscription = await this.#bindFeed(route.liveInput === "turn-messages");
    } catch (error) {
      if (
        error instanceof LiveFeedStoppedBeforeReadyError
        || error instanceof LiveFeedStartupTimeoutError
      ) {
        throw providerPreacceptError(
          "live_input_feed_unavailable_before_acceptance",
          { cause: error },
        );
      }
      throw error;
    }
    if (!subscription)
      throw new Error("North live-feed subscription was not created");
    try {
      this.publish(route, "armed", true);
      this.subscription = subscription;
    } catch (error) {
      await subscription();
      throw error;
    }
  }

  /**
   * Re-poll the durable inbox at the first successful terminal. Turn-message input
   * is passive: the deferred feed has established its coordinator cursor but has
   * claimed no graph mail until this boundary. The provider cannot acknowledge the
   * admitted message until its retained session asks the channel for a later turn.
   */
  async prepareTerminalFollowUp(
    signal: AbortSignal,
  ): Promise<boolean> {
    if (
      this.published.liveInput !== "turn-messages"
      || this.published.liveInputState !== "armed"
      || signal.aborted
    ) return false;
    if (!this.#idleEvent) return false;
    if (this.#followUpState === "queued") return true;
    const subscription = this.subscription;
    if (!subscription) return false;
    const replayAbort = waitForAbort(signal);
    try {
      if (
        typeof subscription.replay !== "function"
        || !subscription.caughtUp
        || typeof subscription.caughtUp.then !== "function"
      ) throw new Error("North turn-messages feed did not expose a replay barrier");
      if (this.#followUpState === "consumed") {
        return await Promise.race([
          subscription.caughtUp.then(() => false as const),
          replayAbort.promise,
        ]);
      }
      const queued = this.#followUpQueued.promise.then(() => true as const);
      const replayed = subscription.replay().then(() => false as const);
      return await Promise.race([queued, replayed, replayAbort.promise]);
    } catch {
      await this.unbind().catch(() => {});
      return false;
    } finally {
      replayAbort.detach();
    }
  }

  /** Append causal milestones only after their Wire events are durable. */
  observeCommittedEvent(event: WakeObservedEvent): void {
    const wake = this.#wake;
    if (!wake) {
      if (
        event.kind === "model-call.completed"
        && event.modelCallId !== undefined
      ) this.#idleEvent = event as WakeIdleEvent;
      return;
    }
    if (wake.modelCallId !== undefined
        && event.kind === "model-call.completed"
        && event.modelCallId === wake.modelCallId) {
      this.#idleEvent = event as WakeIdleEvent;
      this.#wake = undefined;
      this.#openFollowUpSlot();
      return;
    }
    if (!wake.turnEventId) {
      if (event.kind !== "model-call.started"
          || event.modelCallId === undefined
          || event.modelCallId === wake.idleModelCallId) return;
      // Latch the canonical host boundary before publication. A failed receipt
      // remains unknown and must never be replaced by a later model call.
      wake.turnEventId = event.id;
      wake.modelCallId = event.modelCallId;
      try {
        const result = this.wakeReceiptWriter(wake, "turn", event.id);
        if (result !== "created" && result !== "existing") {
          throw new Error("wake turn receipt did not commit");
        }
        wake.hostAccepted.resolve(true);
      } catch (error) {
        console.error(
          `[wake-receipt] ${wake.attemptId} turn publication failed: ${
            error instanceof Error ? error.message : String(error)
          }`,
        );
        this.#failedTurn = {
          messageId: wake.messageId,
          attemptId: wake.attemptId,
          target: wake.target,
          routeEpoch: wake.routeEpoch,
          idleModelCallId: wake.idleModelCallId,
          turnEventId: event.id,
          modelCallId: event.modelCallId,
        };
        wake.hostAccepted.resolve(false);
        this.#wake = undefined;
        this.#openFollowUpSlot();
      }
      return;
    }
    if (wake.actionEventId) return;
    const actionKind = event.kind === "message.recorded"
      && event.role === "assistant"
      && event.modelCallId === wake.modelCallId
      ? "assistant.message.recorded"
      : event.kind === "tool.admitted"
        && event.modelCallId === wake.modelCallId
        ? "tool.admitted"
        : undefined;
    if (!actionKind) return;
    // The first qualifying event is the only candidate. Latch before I/O so a
    // publication failure cannot silently substitute a later assistant/tool.
    wake.actionEventId = event.id;
    try {
      const result = this.wakeReceiptWriter(wake, "action", event.id, actionKind);
      if (result !== "created" && result !== "existing") {
        throw new Error("wake action receipt did not commit");
      }
    } catch (error) {
      console.error(
        `[wake-receipt] ${wake.attemptId} action publication failed: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }

  /**
   * Freeze the last publicly committed route before attempting any fallback.
   * A route that was ever armed may move to another streaming target, but may
   * never silently lose live-input authority by falling back to unsupported.
   */
  async beforeFallback(
    transition: ProviderFallbackTransition,
    afterFreeze: () => Promise<void>,
  ): Promise<void> {
    if (this.everArmedStreaming && transition.toLiveInput === "unsupported") {
      throw new ProviderRetrySafeError(
        "live_input_fallback_refused_after_streaming_route_armed",
      );
    }
    try {
      this.publish(this.published, "frozen", true);
    } catch (error) {
      // A failed graph write aborts fallback, but retaining a live transport
      // behind a route we could not durably freeze is a worse split-brain. The
      // caller reports the failure loudly and may retry the idempotent durable
      // freeze; transport teardown is unconditional and exactly once.
      await this.unbind();
      throw error;
    }
    // Keep the old feed bound after the frozen publication until it proves all
    // pre-freeze producer-admitted messages are durably settled.
    await this.drainAndUnbind();
    await afterFreeze();
  }

  /**
   * Commit frozen before stopping the transport when the graph is writable.
   * If publication fails, stop the transport anyway and rethrow: callers retry
   * the bounded durable reconciliation and publish a terminal failure if both
   * attempts fail. No graph outage is allowed to leak a live feed.
   */
  async freezeAndUnbind(): Promise<void> {
    try {
      this.publish(this.published, "frozen", true);
    } catch (error) {
      await this.unbind();
      throw error;
    }
    await this.drainAndUnbind();
  }
}

/**
 * The single admission seam shared by spawn and dispatch. The first proof keeps
 * an already-latched token target or deadline from claiming mail; the second
 * closes a deadline/host-signal race while durable replay was in progress.
 */
export async function prepareManagedTerminalFollowUp(
  route: ManagedLiveInputRoute,
  gate: ManagedContinuationGate,
): Promise<boolean> {
  gate.throwIfTerminated();
  const queued = await route.prepareTerminalFollowUp(gate.signal);
  gate.throwIfTerminated();
  return queued;
}
