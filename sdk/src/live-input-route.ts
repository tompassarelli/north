import { randomUUID } from "node:crypto";
import {
  LiveFeedStoppedBeforeReadyError,
  LiveFeedStartupTimeoutError,
  subscribeFeed,
  subscribeSettlementFeed,
  type FeedSubscription,
  type InputAdmission,
  type SubscriptionRuntime,
} from "./coordination";
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
  onMessage: (message: string) => InputAdmission,
  runtime?: SubscriptionRuntime,
) => FeedSubscription;

type RouteWriter = (
  agentId: string,
  identity: AgentIdentity,
) => ManagedWriteResult | void;

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

  #resetFollowUpSlot(owner: object): void {
    if (this.#followUpOwner !== owner || this.#followUpState !== "queued") return;
    this.#followUpOwner = undefined;
    this.#followUpState = "open";
    this.#followUpQueued = Promise.withResolvers<void>();
  }

  #admitMessage = (message: string): InputAdmission => {
    if (this.published.liveInput !== "turn-framed") return this.pushMessage(message);
    if (this.#followUpState !== "open") return rejectedAdmission();

    const owner = {};
    const admission = this.pushMessage(message);
    this.#followUpOwner = owner;
    this.#followUpState = "queued";
    this.#followUpQueued.resolve();
    const consumed = Promise.resolve(admission.consumed).then(
      (value) => {
        if (value === true) {
          if (this.#followUpOwner === owner) {
            this.#followUpOwner = undefined;
            this.#followUpState = "consumed";
          }
          return true;
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
      cancel: () => admission.cancel(),
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
          this.pushMessage,
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
    if (route.liveInput === "turn-framed" && !terminalFollowUpCapable) {
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
      subscription = await this.#bindFeed(route.liveInput === "turn-framed");
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
   * Re-poll the durable inbox at the first successful terminal. Turn-framed input
   * is passive: the deferred feed has established its coordinator cursor but has
   * claimed no graph mail until this boundary. The provider cannot acknowledge the
   * admitted frame until its retained session asks the channel for a later turn.
   */
  async prepareTerminalFollowUp(signal: AbortSignal): Promise<boolean> {
    if (
      this.published.liveInput !== "turn-framed"
      || this.published.liveInputState !== "armed"
      || signal.aborted
    ) return false;
    if (this.#followUpState === "queued") return true;
    const subscription = this.subscription;
    if (!subscription) return false;
    const replayAbort = waitForAbort(signal);
    try {
      if (
        typeof subscription.replay !== "function"
        || !subscription.caughtUp
        || typeof subscription.caughtUp.then !== "function"
      ) throw new Error("North turn-framed feed did not expose a replay barrier");
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
