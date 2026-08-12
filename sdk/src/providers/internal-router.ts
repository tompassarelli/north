import type { Options } from "@anthropic-ai/claude-agent-sdk";
import { markExecutionAdmission } from "../execution-admission";
import {
  createExecutionActivityEmitter, forwardExecutionActivity,
} from "../execution-activity";
import {
  applyHarnessRoute, harnessRouteSeed,
  type Effort, type HarnessCompositionEvidence,
} from "../harness";
import type { WireCapabilityClass, WireEvent, WireModelSelection } from "../wire/events";
import {
  wireQueryRoute,
  type WireArtifactSink,
  type WireEventCommitBarrier,
  type WireEventListener,
  type WireQuery,
  type WireQueryInput,
  type WireUserInputFrame,
} from "../wire/query";
import type { WireRunSnapshot } from "../wire/reducer";
import type { WireEventWriter } from "../wire/writer";
import {
  compileProviderAuthoritySurface, type ProviderAuthoritySurface,
} from "./authority";
import {
  observeProviderContextWindow, resolveTier, type SemanticTier,
} from "./catalog";
import {
  isProvedUnsentPreacceptFailure,
  ProviderEscalationUnsupportedError,
  ProviderRetrySafeError,
  type AgentProvider,
  type ProviderFallbackTransition,
  type ProviderId,
  type RoutingDecision,
} from "./types";

export interface RoutedQueryArguments {
  input: WireQueryInput;
  options: Options;
  /** Shared writer whose run.started event has already been committed. */
  writer: WireEventWriter;
  artifacts?: WireArtifactSink;
  eventCommitter?: WireEventCommitBarrier;
}

function replayableInput(input: WireQueryInput): WireQueryInput {
  if (typeof input === "string") return input;
  const source = input[Symbol.asyncIterator]();
  const cache: WireUserInputFrame[] = [];
  let done = false;
  let pending: Promise<IteratorResult<WireUserInputFrame>> | undefined;
  const readNext = async (): Promise<IteratorResult<WireUserInputFrame>> => {
    if (done) return { done: true, value: undefined };
    pending ??= source.next().finally(() => { pending = undefined; });
    const item = await pending;
    if (item.done) {
      done = true;
      return item;
    }
    if (item.value.kind !== "user.input" || typeof item.value.text !== "string") {
      throw new TypeError("wire query input frame must contain kind=user.input and text");
    }
    const frame = Object.freeze({ kind: "user.input" as const, text: item.value.text });
    cache.push(frame);
    return { done: false, value: frame };
  };
  return {
    async *[Symbol.asyncIterator](): AsyncIterator<WireUserInputFrame> {
      let index = 0;
      while (true) {
        if (index < cache.length) {
          yield cache[index++]!;
          continue;
        }
        const item = await readNext();
        if (item.done) return;
        index++;
        yield item.value;
      }
    },
  };
}

function capabilityClass(
  authority: ProviderAuthoritySurface | undefined,
): WireCapabilityClass {
  const capabilities = authority?.capabilities ?? [];
  if (capabilities.includes("coordination")) return "orchestrator";
  if (capabilities.includes("filesystem.write") || capabilities.includes("shell")) {
    return "authoring";
  }
  if (capabilities.includes("web")) return "readonly-web";
  if (capabilities.some((capability) =>
    capability === "filesystem.read"
    || capability === "filesystem.search"
    || capability === "shell.readonly")) {
    return "readonly";
  }
  return "unknown";
}

function checkpointUnchanged(
  writer: WireEventWriter,
  checkpoint: WireRunSnapshot,
): boolean {
  const current = writer.snapshot();
  return current === checkpoint
    || Boolean(current
      && current.lastSequence === checkpoint.lastSequence
      && current.lastEventId === checkpoint.lastEventId);
}

/**
 * Internal router harness. The injectable registry exists solely for hermetic
 * adapter/fallback tests and is deliberately not re-exported by providers/index.
 * Production callers receive routedQuery, which closes over the canonical
 * Anthropic/OpenAI registry.
 */
export function routedQueryWithRegistry(
  decision: RoutingDecision,
  args: RoutedQueryArguments,
  tier: SemanticTier | undefined,
  providerRegistry: Readonly<Record<ProviderId, AgentProvider>>,
  beforeFallback?: (transition: ProviderFallbackTransition) => Promise<void>,
  onRoute?: (
    decision: RoutingDecision,
    evidence: HarnessCompositionEvidence | undefined,
    authority: ProviderAuthoritySurface | undefined,
  ) => Promise<void> | void,
  onRouteAttempt?: (decision: RoutingDecision) => void,
): WireQuery {
  const initialSnapshot = args.writer.snapshot();
  if (!initialSnapshot || initialSnapshot.lifecycle !== "running") {
    throw new Error("routed wire query requires an already-started running writer");
  }

  let active: WireQuery | undefined;
  const activity = createExecutionActivityEmitter();
  const providerEventListeners = new Set<WireEventListener>();
  let stopActivity = () => {};
  let stopProviderEvents = () => {};
  let activePublishesEvents = false;
  let continuationReady = false;
  let turnStreaming = false;
  let closed = false;
  let closePromise: Promise<void> | undefined;
  const input = replayableInput(args.input);
  const requestedReasoning = args.options.effort as Effort | undefined;
  const seed = harnessRouteSeed(args.options);

  const publish = (event: WireEvent): void => {
    for (const listener of providerEventListeners) {
      try { listener(event); }
      catch { /* Observation subscribers cannot change routed execution. */ }
    }
  };
  const detachActiveObservers = (): void => {
    stopActivity();
    stopActivity = () => {};
    stopProviderEvents();
    stopProviderEvents = () => {};
    activePublishesEvents = false;
  };
  const attachActiveObservers = (
    query: WireQuery,
    onProviderEvent: () => void,
  ): void => {
    detachActiveObservers();
    stopActivity = forwardExecutionActivity(query.executionActivity, activity);
    activePublishesEvents = query.subscribeProviderEvents !== undefined;
    if (query.subscribeProviderEvents) {
      stopProviderEvents = query.subscribeProviderEvents((event) => {
        onProviderEvent();
        publish(event);
      });
    }
  };
  const streamActiveTurn = async function* (): AsyncGenerator<WireEvent> {
    if (!active) throw new Error("routed wire query has no active provider turn");
    if (turnStreaming) throw new Error("routed wire query turn is already streaming");
    turnStreaming = true;
    try {
      for await (const event of active) {
        if (!activePublishesEvents) publish(event);
        yield event;
      }
    } finally {
      turnStreaming = false;
    }
  };

  const optionsFor = (
    provider: ProviderId,
  ): { options: Options; evidence?: HarnessCompositionEvidence } => {
    const preserveSeed = decision.fallbackCount === 0 || seed?.provider === provider;
    const resolved = preserveSeed
      ? { model: seed?.model, effort: requestedReasoning }
      : resolveTier(provider, tier, undefined, requestedReasoning);
    const rebuilt = applyHarnessRoute(
      args.options, provider, resolved.model, resolved.effort,
      {
        targetId: decision.target,
        receipt: decision.modelAvailabilityReceipts?.[decision.target],
      },
    );
    const options: Options = rebuilt.options === args.options
      ? { ...rebuilt.options }
      : rebuilt.options;
    if (rebuilt.options === args.options && resolved.model) options.model = resolved.model;
    if (rebuilt.options === args.options && resolved.effort) options.effort = resolved.effort;
    decision.resolvedModel = options.model ?? resolved.model;
    decision.resolvedEffort = options.effort;
    return { options, evidence: rebuilt.evidence };
  };

  const query: WireQuery = {
    get executionTransport() { return active?.executionTransport; },
    executionActivity: activity.source,
    subscribeProviderEvents(listener: WireEventListener): () => void {
      providerEventListeners.add(listener);
      return () => providerEventListeners.delete(listener);
    },
    mcpActivity: () => active?.mcpActivity?.() ?? {
      source: "provider-route-unavailable", coverage: "unknown", tools: [],
      operationReceipts: [], operationAggregates: [],
    },
    nativeCommandActivity: () => active?.nativeCommandActivity?.() ?? {
      source: "provider-route-unavailable", coverage: "unknown",
      northBinaryProbe: "not_observed", completions: [],
    },
    async continueTurn(continuedInput: WireQueryInput): Promise<void> {
      if (closed) throw new Error("routed wire query is closed");
      if (turnStreaming) {
        throw new Error("routed wire query cannot continue while a turn is streaming");
      }
      if (continuationReady) {
        throw new Error("routed wire query already has a continued turn ready");
      }
      if (!active?.continueTurn) {
        throw new ProviderEscalationUnsupportedError(
          `provider ${decision.provider} does not support provider-neutral continuation`,
        );
      }
      await active.continueTurn(continuedInput);
      continuationReady = true;
    },
    interruptTurn: async () => {
      if (active?.interruptTurn) await active.interruptTurn();
      else await active?.interrupt?.();
    },
    interrupt: async () => { await active?.interrupt?.(); },
    close: () => closePromise ??= (async () => {
      closed = true;
      detachActiveObservers();
      await active?.close?.();
    })(),
    forceClose: () => {
      closed = true;
      detachActiveObservers();
      active?.forceClose?.();
    },
    supportsInFlightEscalation: () => Boolean(
      active?.setModel && active?.applyFlagSettings
      && (active.supportsInFlightEscalation?.() ?? true),
    ),
    setModel: async (selection: WireModelSelection) => {
      if (selection.provider !== decision.provider) {
        throw new ProviderEscalationUnsupportedError(
          `active provider ${decision.provider} cannot apply a ${selection.provider} model selection`,
        );
      }
      if (!active?.setModel) {
        throw new ProviderEscalationUnsupportedError(
          `provider ${decision.provider} does not support in-flight model escalation`,
        );
      }
      const resolved = resolveTier(decision.provider, selection.tier);
      await active.setModel(selection);
      if (resolved.model !== undefined) decision.resolvedModel = resolved.model;
    },
    applyFlagSettings: async (settings) => {
      if (!active?.applyFlagSettings) {
        throw new ProviderEscalationUnsupportedError(
          `provider ${decision.provider} does not support in-flight effort escalation`,
        );
      }
      await active.applyFlagSettings(settings);
      if (settings.effortLevel !== undefined && settings.effortLevel !== null) {
        decision.resolvedEffort = settings.effortLevel;
      }
    },
    async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      if (continuationReady) {
        continuationReady = false;
        yield* streamActiveTurn();
        return;
      }
      if (active) {
        throw new Error("routed wire query requires continueTurn before another turn");
      }
      while (true) {
        if (closed) return;
        const checkpoint = args.writer.snapshot();
        if (!checkpoint || checkpoint.lifecycle !== "running") {
          throw new Error("routed wire query writer is no longer running");
        }
        let adapterEvents = 0;
        try {
          onRouteAttempt?.(decision);
          const route = optionsFor(decision.provider);
          const options = route.options;
          const provider = providerRegistry[decision.provider];
          const managed = "northCapabilities" in options
            && options.northCapabilities !== undefined;
          if (managed && !provider.admit) {
            throw ProviderRetrySafeError.provedUnsent(
              "managed_provider_admission_unavailable",
              {
                mode: "managed",
                source: "adapter_preflight",
                requestBytesPrepared: 0,
              },
            );
          }
          if (provider.admit) {
            await provider.admit({
              options,
              target: decision.routingTargets[decision.target],
            });
            markExecutionAdmission(decision.provider, options);
          }
          const authority = !managed
            ? undefined
            : compileProviderAuthoritySurface(decision.provider, options);
          if (authority && authority.provider !== decision.provider) {
            throw ProviderRetrySafeError.provedUnsent(
              "provider_authority_route_mismatch",
              {
                mode: "managed",
                source: "adapter_preflight",
                requestBytesPrepared: 0,
              },
            );
          }
          await onRoute?.(decision, route.evidence, authority);
          if (closed) return;

          const semanticEffort = options.effort ?? route.options.effort ?? requestedReasoning;
          if (semanticEffort === undefined) {
            throw ProviderRetrySafeError.provedUnsent(
              "provider_semantic_effort_unresolved",
              {
                mode: "managed",
                source: "adapter_preflight",
                requestBytesPrepared: 0,
              },
            );
          }
          const model: WireModelSelection = Object.freeze({
            provider: decision.provider,
            ...(tier === undefined ? {} : { tier }),
            capabilityClass: capabilityClass(authority),
          });
          const contextWindow = observeProviderContextWindow(
            decision.provider,
            options.model,
          )?.tokens;
          const context = {
            writer: args.writer,
            route: wireQueryRoute({
              model,
              effort: semanticEffort,
              attempt: decision.fallbackCount + 1,
              ...(contextWindow === undefined ? {} : { contextWindow }),
            }),
            ...(args.artifacts === undefined ? {} : { artifacts: args.artifacts }),
            ...(args.eventCommitter === undefined ? {} : { eventCommitter: args.eventCommitter }),
          };
          active = provider.query({
            input,
            options,
            target: decision.routingTargets[decision.target],
            context,
          });
          attachActiveObservers(active, () => { adapterEvents++; });
          if (closed) {
            detachActiveObservers();
            await active.close?.();
            return;
          }
          for await (const event of streamActiveTurn()) {
            adapterEvents++;
            yield event;
          }
          return;
        } catch (error) {
          const fallbackTarget = decision.fallbackTargets[0];
          const fallbackProvider = decision.fallbackProviders[0];
          const proofAllowsFallback = decision.requestedTarget === undefined
            && adapterEvents === 0
            && fallbackTarget !== undefined
            && fallbackProvider !== undefined
            && isProvedUnsentPreacceptFailure(error);
          if (proofAllowsFallback) {
            // The proof describes provider transport. Reaping the failed route
            // must also leave the shared semantic checkpoint unchanged.
            detachActiveObservers();
            await active?.close?.();
          }
          if (proofAllowsFallback
              && checkpointUnchanged(args.writer, checkpoint)) {
            const previousTarget = decision.target;
            const previousProvider = decision.provider;
            await beforeFallback?.({
              fromTarget: previousTarget,
              fromProvider: previousProvider,
              fromLiveInput: providerRegistry[previousProvider].liveInput,
              toTarget: fallbackTarget,
              toProvider: fallbackProvider,
              toLiveInput: providerRegistry[fallbackProvider].liveInput,
            });
            if (!checkpointUnchanged(args.writer, checkpoint)) throw error;
            decision.fallbackTargets.shift();
            decision.fallbackProviders.shift();
            decision.target = fallbackTarget;
            decision.provider = fallbackProvider;
            decision.entitlementPressure =
              decision.targetEntitlementPressures[fallbackTarget] ?? "unknown";
            decision.fallbackCount++;
            decision.fallbackTargetPath.push(fallbackTarget);
            decision.fallbackPath.push(fallbackProvider);
            decision.fallbackReasons.push(Object.freeze({
              sequence: decision.fallbackCount,
              reason: "provider_retry_safe_before_acceptance",
              fromTarget: previousTarget,
              fromProvider: previousProvider,
              toTarget: fallbackTarget,
              toProvider: fallbackProvider,
              phase: "preaccept",
              replay: "proved_unsent",
              proof: error.unsentProof,
            }));
            const fallbackEvent = args.writer.append({
              kind: "run.progress",
              lifecycle: "running",
              progress: {
                fallback: {
                  fromProvider: previousProvider,
                  toProvider: fallbackProvider,
                  reason: "provider_retry_safe_before_acceptance",
                  phase: "preaccept",
                },
              },
            });
            active = undefined;
            publish(fallbackEvent);
            yield fallbackEvent;
            continue;
          }
          throw error;
        }
      }
    },
  };
  return query;
}
