import { markCoordinationOptional } from "../execution-admission";
import { harnessOptions } from "../harness";
import type { ProviderModelAdmissionReceipt } from "../provider-model-observation-store";
import { applyOrchestrationStaffing } from "../orchestration-staffing";
import * as providerRouting from "../provider-routing";
import { anthropicProvider } from "../providers/anthropic";
import { openaiProvider } from "../providers/openai";
import type { AgentProvider, RoutingTarget } from "../providers/types";
import { resolveTier } from "../providers/catalog";
import { RunArtifactStore } from "../run-artifacts";
import {
  wireQueryRoute,
  type WireEvent,
  type WireEventWriter,
  type WireQuery,
} from "../wire";
import directorPrompt from "./director-prompt.md" with { type: "text" };
import implementerPrompt from "./implementer-prompt.md" with { type: "text" };
import type {
  BridgeLaunchProvider, BridgeLaunchRole, BridgeLaunchSelection,
} from "./protocol";
import type { BridgeAttemptRouteAuthority } from "./command-receipts";

export interface BridgeSessionPresentation {
  model?: string;
  effort?: string;
  cwd: string;
  permissionMode?: string;
}

export interface BridgeProviderSession {
  presentation?: BridgeSessionPresentation;
  submitInput(input: string): Promise<void>;
  interruptTurn(): Promise<void>;
  terminateSession(): Promise<void>;
  forceTerminateSession?(): void;
  events(): AsyncIterable<WireEvent>;
}

export interface BridgeProviderOpenContext extends BridgeLaunchSelection {
  executionId: string;
  prompt: string;
  cwd: string;
  role: BridgeLaunchRole;
  provider: BridgeLaunchProvider;
  attemptRoute: BridgeAttemptRouteAuthority;
  signal: AbortSignal;
  /** Shared writer whose run.started event is already durable. */
  writer: WireEventWriter;
}

export interface BridgeAutomaticProviderSelection
  extends Omit<BridgeLaunchSelection, "provider"> {
  role: BridgeLaunchRole;
}

export function resolveBridgeLaunchSelection(
  provider: BridgeLaunchProvider,
  role: BridgeLaunchRole,
  selection: Omit<BridgeLaunchSelection, "provider">,
) {
  const base = applyOrchestrationStaffing({ role });
  const tier = selection.tier ?? base.tier;
  const automatic = resolveTier(provider, tier);
  const pinned = selection.model
    ? resolveTier(provider, tier, selection.model, selection.effort)
    : undefined;
  const effort = pinned?.effort ?? selection.effort ?? automatic.effort ?? base.reasoning;
  const resolved = pinned ?? resolveTier(provider, tier, undefined, effort);
  // A concrete model pin may deliberately use reasoning outside the unpinned
  // semantic ramp. Keep the staffing receipt provider-neutral.
  const semanticEffort = pinned ? automatic.effort ?? base.reasoning : effort;
  const overrides = [
    ...(tier !== base.tier ? ["tier" as const] : []),
    ...(semanticEffort !== base.reasoning ? ["reasoning" as const] : []),
  ];
  const routingMetadata = overrides.length === 0 ? base : applyOrchestrationStaffing({
    role,
    tier,
    reasoning: semanticEffort,
    composition: {
      kind: "template", id: base.role, overrides,
      overrideReason: "Bridge launch selection",
    },
  });
  return { routingMetadata, resolved };
}

export interface BridgeProviderExecution {
  open(context: BridgeProviderOpenContext): Promise<BridgeProviderSession>;
}

export function bridgeSystemPrompt(role: BridgeLaunchRole): string {
  return role === "director" ? directorPrompt.trim() : implementerPrompt.trim();
}

const BRIDGE_QUERY_TEARDOWN_TIMEOUT_MS = 1_000;

interface BridgeContinuation {
  settled: PromiseWithResolvers<void>;
  accepted: boolean;
  observed: boolean;
  iterationStarted: boolean;
}

export class BridgeProviderTeardownTimeoutError extends Error {
  constructor(timeoutMs: number) {
    super(`provider session teardown timed out after ${timeoutMs}ms`);
    this.name = "BridgeProviderTeardownTimeoutError";
  }
}

async function boundedQueryTeardown(query: WireQuery, timeoutMs: number): Promise<void> {
  const tasks: Promise<void>[] = [];
  try {
    if (query.interrupt) tasks.push(Promise.resolve(query.interrupt()));
  } catch (error) {
    tasks.push(Promise.reject(error));
  }
  try {
    if (query.close) tasks.push(Promise.resolve(query.close()));
  } catch (error) {
    tasks.push(Promise.reject(error));
  }
  if (tasks.length === 0) return;

  const timeout = Promise.withResolvers<never>();
  const timer = setTimeout(
    () => timeout.reject(new BridgeProviderTeardownTimeoutError(timeoutMs)),
    timeoutMs,
  );
  try {
    const results = await Promise.race([
      Promise.allSettled(tasks),
      timeout.promise,
    ]);
    const failures = results
      .filter((result): result is PromiseRejectedResult => result.status === "rejected")
      .map((result) => result.reason);
    if (failures.length === 1) throw failures[0];
    if (failures.length > 1) throw new AggregateError(failures);
  } catch (error) {
    if (error instanceof BridgeProviderTeardownTimeoutError) query.forceClose?.();
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

export class BridgeWireSession implements BridgeProviderSession {
  #query: WireQuery;
  #abort: AbortController;
  #signal: AbortSignal;
  #signalAbort: () => void;
  #continuation?: BridgeContinuation;
  #continuationWaiting?: PromiseWithResolvers<void>;
  #submitting = false;
  #eventsConsumed = false;
  #closed = false;
  #termination?: Promise<void>;
  readonly presentation?: BridgeSessionPresentation;

  constructor(
    query: WireQuery,
    abort: AbortController,
    signal: AbortSignal,
    presentation?: BridgeSessionPresentation,
  ) {
    this.#query = query;
    this.#abort = abort;
    this.#signal = signal;
    this.presentation = presentation;
    this.#signalAbort = () => { void this.terminateSession().catch(() => {}); };
    signal.addEventListener("abort", this.#signalAbort, { once: true });
    if (signal.aborted) this.#signalAbort();
  }

  #wakeContinuationWaiter(): void {
    const waiting = this.#continuationWaiting;
    this.#continuationWaiting = undefined;
    waiting?.resolve();
  }

  async submitInput(input: string): Promise<void> {
    if (this.#closed) throw new Error("provider session is closed");
    if (this.#submitting || this.#continuation) {
      throw new Error("provider session already has a continuation in flight");
    }
    if (!this.#query.continueTurn) {
      throw new Error("provider does not support provider-neutral continuation");
    }
    const continuation: BridgeContinuation = {
      settled: Promise.withResolvers<void>(),
      accepted: false,
      observed: false,
      iterationStarted: false,
    };
    this.#submitting = true;
    this.#continuation = continuation;
    this.#wakeContinuationWaiter();
    try {
      await this.#query.continueTurn(input);
      continuation.accepted = true;
    } catch (error) {
      if (this.#continuation === continuation) this.#continuation = undefined;
      throw error;
    } finally {
      this.#submitting = false;
      continuation.settled.resolve();
      this.#wakeContinuationWaiter();
    }
  }

  async interruptTurn(): Promise<void> {
    if (!this.#query.interruptTurn) {
      throw new Error("provider does not support turn interruption");
    }
    await this.#query.interruptTurn();
  }

  terminateSession(): Promise<void> {
    if (this.#termination) return this.#termination;
    this.#closed = true;
    this.#signal.removeEventListener("abort", this.#signalAbort);
    this.#abort.abort();
    this.#wakeContinuationWaiter();
    this.#termination = boundedQueryTeardown(this.#query, BRIDGE_QUERY_TEARDOWN_TIMEOUT_MS);
    return this.#termination;
  }

  forceTerminateSession(): void {
    this.#closed = true;
    this.#signal.removeEventListener("abort", this.#signalAbort);
    this.#abort.abort();
    this.#wakeContinuationWaiter();
    this.#query.forceClose?.();
  }

  #observeContinuation(): void {
    const continuation = this.#continuation;
    if (!continuation) return;
    continuation.observed = true;
    this.#continuation = undefined;
  }

  async #awaitContinuationIterator(): Promise<boolean> {
    while (!this.#closed) {
      const continuation = this.#continuation;
      if (!continuation) {
        this.#continuationWaiting = Promise.withResolvers<void>();
        await this.#continuationWaiting.promise;
        this.#continuationWaiting = undefined;
        continue;
      }
      await continuation.settled.promise;
      if (this.#closed) return false;
      if (!continuation.accepted || continuation.observed) {
        if (this.#continuation === continuation) this.#continuation = undefined;
        continue;
      }
      if (continuation.iterationStarted) {
        if (this.#continuation === continuation) this.#continuation = undefined;
        continue;
      }
      continuation.iterationStarted = true;
      return true;
    }
    return false;
  }

  async *events(): AsyncGenerator<WireEvent, void, unknown> {
    if (this.#eventsConsumed) throw new Error("provider event stream is single-consumer");
    this.#eventsConsumed = true;
    try {
      let first = true;
      while (!this.#closed) {
        if (!first && !await this.#awaitContinuationIterator()) return;
        first = false;
        for await (const event of this.#query) {
          this.#observeContinuation();
          yield event;
        }
      }
    } finally {
      this.#signal.removeEventListener("abort", this.#signalAbort);
      if (!this.#closed) await this.terminateSession();
    }
  }
}

export type BridgeProviderRouting = Pick<typeof providerRouting,
  "selectProviderFromCachedState" | "refreshProviderRoutingInBackground"
  | "selectProviderForExecution" | "configuredDefaultTarget" | "BOOT_ROUTING_TIMEOUT_MS">
  & Partial<Pick<typeof providerRouting, "resourcePolicyFromEnv">>;

function attemptCredentialTarget(
  routing: BridgeProviderRouting,
  authority: BridgeAttemptRouteAuthority,
): RoutingTarget {
  const policy = (routing.resourcePolicyFromEnv ?? providerRouting.resourcePolicyFromEnv)();
  const matches = (policy.targets ?? []).filter((target) =>
    target.id === authority.accountId && target.provider === authority.provider);
  if (matches.length !== 1) {
    throw new Error("Bridge Store attempt account has no unique configured credential locator");
  }
  const target = matches[0]!;
  if (authority.provider === "openai"
    && (target.authMode !== "isolated" || !target.profile
      || target.profile !== authority.credentialProfile)) {
    throw new Error("Bridge Store-authorized OpenAI account lacks an isolated credential locator");
  }
  return target;
}

/** Select an authenticated target without making Bridge's open path unbounded. */
export async function bridgeRoute(
  routing: BridgeProviderRouting,
  provider: BridgeLaunchProvider,
  context: {
    tier?: BridgeLaunchSelection["tier"];
    reasoning?: BridgeLaunchSelection["effort"];
    model?: string;
  } = {},
): Promise<{ target?: RoutingTarget; receipt?: ProviderModelAdmissionReceipt }> {
  const cached = await routing.selectProviderFromCachedState({ provider }, undefined, context);
  if (cached) {
    void routing.refreshProviderRoutingInBackground({ provider });
    return {
      target: cached.routingTargets[cached.target],
      ...(cached.modelAvailabilityReceipts?.[cached.target]
        ? { receipt: cached.modelAvailabilityReceipts[cached.target] }
        : {}),
    };
  }
  try {
    const decision = await routing.selectProviderForExecution({ provider }, undefined, {
      ...context,
      signal: AbortSignal.timeout(routing.BOOT_ROUTING_TIMEOUT_MS),
    });
    return {
      target: decision.routingTargets[decision.target],
      ...(decision.modelAvailabilityReceipts?.[decision.target]
        ? { receipt: decision.modelAvailabilityReceipts[decision.target] }
        : {}),
    };
  } catch {
    // An explicit model is exact authority. A static account default cannot
    // prove it and must not reach an adapter without a receipt.
    if (context.model) return {};
    const fallback = routing.configuredDefaultTarget(provider);
    return fallback ? { target: fallback } : {};
  }
}

type BridgeAgentProviders = Readonly<Record<BridgeLaunchProvider, AgentProvider>>;

export function bridgeProviderWithDependenciesForTest(
  providers: BridgeAgentProviders,
  routing: BridgeProviderRouting,
): BridgeProviderExecution {
  return Object.freeze({
    async open(context: BridgeProviderOpenContext): Promise<BridgeProviderSession> {
      const authority = context.attemptRoute;
      if (context.provider !== authority.provider || context.model !== authority.model) {
        throw new Error("Bridge provider context conflicts with its Store attempt authority");
      }
      const agentProvider = providers[authority.provider];
      const model = authority.model;
      const target = attemptCredentialTarget(routing, authority);
      const selection = resolveBridgeLaunchSelection(authority.provider, context.role, {
        ...(context.tier ? { tier: context.tier } : {}),
        model,
        ...(context.effort ? { effort: context.effort } : {}),
      });
      if (selection.resolved.model !== model) {
        throw new Error("Bridge Store attempt model is not an exact canonical route");
      }
      const routingMetadata = selection.routingMetadata;
      const abortController = new AbortController();
      const artifacts = new RunArtifactStore(context.writer.runId);
      const options = harnessOptions({
        self: `bridge-${context.executionId}`,
        provider: authority.provider,
        routingMetadata,
        cwd: context.cwd,
        model,
        modelAvailability: {
          exactModelPinned: false,
          targetId: target.id,
        },
        presenceRegistrar: false,
        presenceRenewer: false,
        systemPrompt: bridgeSystemPrompt(context.role),
        abortController,
        artifactDirectory: artifacts.directory,
      });
      markCoordinationOptional(options);
      await agentProvider.admit?.({ options, target });
      const effort = options.effort ?? "high";
      const query = agentProvider.query({
        input: context.prompt,
        options,
        target,
        context: {
          writer: context.writer,
          artifacts,
          route: wireQueryRoute({
            model: {
              provider: authority.provider,
              tier: routingMetadata.tier,
              capabilityClass: context.role === "director" ? "orchestrator" : "authoring",
            },
            effort,
            attempt: 1,
          }),
        },
      });
      return new BridgeWireSession(query, abortController, context.signal, Object.freeze({
        model: options.model,
        effort: options.effort,
        cwd: options.cwd ?? context.cwd,
        // Managed Codex is launched with approvalPolicy=never. The operator's
        // established name for that no-prompt mode is the banner's YOLO mode.
        permissionMode: authority.provider === "openai"
          ? "bypassPermissions"
          : options.permissionMode,
      }));
    },
  });
}

export const bridgeProvider: BridgeProviderExecution = bridgeProviderWithDependenciesForTest(
  { anthropic: anthropicProvider, openai: openaiProvider },
  providerRouting,
);

const BRIDGE_PRESSURE_RANK: Record<string, number> = {
  plenty: 4, normal: 3, low: 2, unknown: 1, exhausted: 0,
};

const BRIDGE_PROVIDER_ORDER: BridgeLaunchProvider[] = ["anthropic", "openai"];

export async function selectBridgeProvider(
  selection: BridgeAutomaticProviderSelection = { role: "implementer" },
): Promise<BridgeLaunchProvider> {
  const routeRequested = selection.tier !== undefined
    || selection.model !== undefined
    || selection.effort !== undefined;
  const compatible = routeRequested
    ? BRIDGE_PROVIDER_ORDER.filter((provider) => {
      try {
        resolveBridgeLaunchSelection(provider, selection.role, selection);
        return true;
      } catch {
        return false;
      }
    })
    : BRIDGE_PROVIDER_ORDER;
  if (compatible.length === 0)
    throw new Error("no Bridge provider supports the requested launch route");

  const fallback = compatible.includes("openai") ? "openai" : compatible[0]!;
  try {
    const routing = providerRouting.cachedTargetRouting();
    let best: { provider: BridgeLaunchProvider; rank: number } | undefined;
    for (const provider of compatible) {
      for (const { target, eligible, headroom } of routing) {
        if (!eligible || target.provider !== provider) continue;
        const rank = BRIDGE_PRESSURE_RANK[headroom] ?? 0;
        if (!best || rank > best.rank) best = { provider, rank };
      }
    }
    return best?.provider ?? fallback;
  } catch {
    return fallback;
  }
}
