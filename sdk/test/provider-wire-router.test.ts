import { expect, test } from "bun:test";
import type { Options } from "@anthropic-ai/claude-agent-sdk";
import type { CodexAccountAuthority } from "../src/accounts";
import { createExecutionActivityEmitter } from "../src/execution-activity";
import { selectProviderForExecution } from "../src/provider-routing";
import { routedQueryWithRegistry } from "../src/providers/internal-router";
import { resolveTier } from "../src/providers/catalog";
import {
  ProviderRetrySafeError,
  type AgentProvider,
  type AgentProviderQuery,
  type ProviderId,
  type ProviderUsageObservation,
  type ResourcePolicy,
  type RoutingDecision,
  type RoutingTarget,
} from "../src/providers/types";
import type { StoreObservationSnapshot } from "../src/store-observation-adapter";
import type { WireEvent, WireModelSelection } from "../src/wire/events";
import { wireEventId, wireRunId } from "../src/wire/ids";
import type { WireQuery, WireQueryInput, WireUserInputMessage } from "../src/wire/query";
import { WireEventWriter } from "../src/wire/writer";

const NOW = "2026-08-10T00:00:00.000Z";

function startedWriter(label: string): WireEventWriter {
  const writer = new WireEventWriter({
    runId: wireRunId(`run:${label}`),
    eventId: (sequence) => wireEventId(`event:${label}:${sequence}`),
    now: () => NOW,
  });
  writer.append({ kind: "run.started", lifecycle: "running" });
  return writer;
}

function decision(): RoutingDecision {
  return {
    requestedProvider: "auto",
    target: "claude-personal",
    provider: "anthropic",
    routingTargets: {
      "claude-personal": { id: "claude-personal", provider: "anthropic" },
      "codex-personal": { id: "codex-personal", provider: "openai" },
    },
    selectionReason: "test allocation",
    availability: [],
    fallbackTargets: ["codex-personal"],
    fallbackTargetPath: ["claude-personal"],
    fallbackProviders: ["openai"],
    fallbackCount: 0,
    fallbackPath: ["anthropic"],
    fallbackReasons: [],
    allocationMode: "preferential",
    entitlementPressure: "normal",
    targetEntitlementPressures: {
      "claude-personal": "normal",
      "codex-personal": "normal",
    },
  };
}

function provedUnsent(message: string): ProviderRetrySafeError {
  return ProviderRetrySafeError.provedUnsent(message, {
    mode: "managed",
    source: "adapter_preflight",
    requestBytesPrepared: 0,
  });
}

function provider(
  id: ProviderId,
  query: (args: AgentProviderQuery) => WireQuery,
): AgentProvider {
  return {
    id,
    liveInput: id === "anthropic" ? "streaming" : "turn-messages",
    probe: () => ({ provider: id, available: true, reason: "ready" }),
    query,
  };
}

async function eventsOf(query: WireQuery): Promise<WireEvent[]> {
  const events: WireEvent[] = [];
  for await (const event of query) events.push(event);
  return events;
}

async function firstText(input: WireQueryInput): Promise<string> {
  if (typeof input === "string") return input;
  const first = await input[Symbol.asyncIterator]().next();
  if (first.done) throw new Error("wire query input ended before its first message");
  return first.value.text;
}

test("execution receipt stays immutable while routed attribution and fallback remain live", async () => {
  const openaiTarget: RoutingTarget = {
    id: "codex-receipt", provider: "openai", authMode: "isolated", profile: "receipt",
  };
  const anthropicTarget: RoutingTarget = {
    id: "claude-fallback", provider: "anthropic", authMode: "ambient",
  };
  const policy: ResourcePolicy = {
    mode: "preferential",
    providerOrder: ["openai", "anthropic"],
    targets: [openaiTarget, anthropicTarget],
    targetOrder: [openaiTarget.id, anthropicTarget.id],
    targetPressures: {
      [openaiTarget.id]: "normal",
      [anthropicTarget.id]: "normal",
    },
  };
  const authority: CodexAccountAuthority = Object.freeze({
    role: "execution",
    executionEligible: true,
    receipt: Object.freeze({
      version: "north:codex-account-authority:v1" as const,
      subject: `@account:${openaiTarget.id}`,
      servedVersion: 41,
      facts: Object.freeze([
        { predicate: "kind" as const, value: "provider_account" },
        { predicate: "account_id" as const, value: openaiTarget.id },
        { predicate: "provider" as const, value: "openai" },
        { predicate: "provider_profile" as const, value: openaiTarget.profile! },
        { predicate: "account_role" as const, value: "execution" },
        { predicate: "execution_eligible" as const, value: "true" },
      ]),
      digest: "a".repeat(64),
    }),
  });
  const usage: StoreObservationSnapshot<ProviderUsageObservation> = Object.freeze({
    observation: Object.freeze({
      targetId: openaiTarget.id,
      provider: "openai" as const,
      source: "codex-app-server:account-rate-limits" as const,
      observedAt: NOW,
      windows: Object.freeze([Object.freeze({
        limitId: "codex:primary",
        usedPercent: 12,
        resetsAt: "2099-01-01T00:00:00.000Z",
      })]),
    }),
    receipt: Object.freeze({
      version: "north:provider-observation:v1" as const,
      subject: `@provider-observation:usage:${openaiTarget.id}`,
      digest: "b".repeat(64),
      servedVersion: 42,
    }),
  });
  const routing = await selectProviderForExecution(
    "auto",
    policy,
    { tier: "standard", reasoning: "medium", stableKey: "live-receipt-decision" },
    {
      probeAnthropic: () => ({
        targetId: anthropicTarget.id,
        provider: "anthropic",
        available: true,
        reason: "ready",
      }),
      readCodexAccountAuthority: async () => authority,
      loadCodexUsage: async () => usage,
      refreshAccountUsages: async () => [],
    },
  );
  const accountReceipt = routing.executionAccountReceipt;
  const receiptDescriptor = Object.getOwnPropertyDescriptor(routing, "executionAccountReceipt");

  expect(routing).toMatchObject({ provider: "openai", target: openaiTarget.id });
  expect(Object.isExtensible(routing)).toBe(true);
  expect(receiptDescriptor).toMatchObject({
    value: accountReceipt,
    enumerable: true,
    writable: false,
    configurable: false,
  });
  expect(Object.getOwnPropertyDescriptor(routing, "selectionReason")?.writable).toBe(false);
  expect(Object.getOwnPropertyDescriptor(routing, "routingTargets")?.writable).toBe(false);

  const reached: string[] = [];
  const registry = {
    openai: provider("openai", (args) => {
      reached.push(args.target?.id ?? "missing-target");
      return {
        async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
          throw provedUnsent("OpenAI unavailable before acceptance");
        },
      };
    }),
    anthropic: provider("anthropic", (args) => {
      reached.push(args.target?.id ?? "missing-target");
      return {
        async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
          yield args.context.writer.append({
            kind: "run.progress",
            lifecycle: "running",
            progress: { currentAction: "fallback provider reached" },
          });
        },
      };
    }),
  };
  const events = await eventsOf(routedQueryWithRegistry(
    routing,
    {
      input: "turn",
      options: { effort: "medium" } as Options,
      writer: startedWriter("live-receipt-decision"),
    },
    "standard",
    registry,
  ));

  expect(reached).toEqual([openaiTarget.id, anthropicTarget.id]);
  expect(events.map(({ kind }) => kind)).toEqual(["run.progress", "run.progress"]);
  expect(routing).toMatchObject({
    provider: "anthropic",
    target: anthropicTarget.id,
    fallbackCount: 1,
    resolvedModel: resolveTier("anthropic", "standard").model,
    resolvedEffort: "medium",
  });
  expect(routing.fallbackTargets).toEqual([]);
  expect(routing.fallbackProviders).toEqual([]);
  expect(routing.fallbackTargetPath).toEqual([openaiTarget.id, anthropicTarget.id]);
  expect(routing.fallbackPath).toEqual(["openai", "anthropic"]);
  expect(routing.executionAccountReceipt).toBe(accountReceipt);
  expect(Object.getOwnPropertyDescriptor(routing, "executionAccountReceipt"))
    .toEqual(receiptDescriptor);
});

test("proof-carrying fallback replays typed input and emits semantic progress", async () => {
  const writer = startedWriter("fallback");
  const routing = decision();
  const received: Partial<Record<ProviderId, string>> = {};
  const routes: AgentProviderQuery[] = [];
  const input: AsyncIterable<WireUserInputMessage> = {
    async *[Symbol.asyncIterator](): AsyncIterator<WireUserInputMessage> {
      yield { kind: "user.input", text: "same semantic turn" };
    },
  };
  const registry = {
    anthropic: provider("anthropic", (args) => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        routes.push(args);
        received.anthropic = await firstText(args.input);
        throw provedUnsent("anthropic preaccept refusal");
      },
    })),
    openai: provider("openai", (args) => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        routes.push(args);
        received.openai = await firstText(args.input);
        yield args.context.writer.append({
          kind: "run.progress",
          lifecycle: "running",
          progress: { currentAction: "openai accepted the semantic turn" },
        });
      },
    })),
  };

  const events = await eventsOf(routedQueryWithRegistry(
    routing,
    { input, options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  ));

  expect(received).toEqual({
    anthropic: "same semantic turn",
    openai: "same semantic turn",
  });
  expect(events.map((event) => event.kind)).toEqual([
    "run.progress",
    "run.progress",
  ]);
  expect(events[0]).toMatchObject({
    kind: "run.progress",
    progress: {
      fallback: {
        fromProvider: "anthropic",
        toProvider: "openai",
        reason: "provider_retry_safe_before_acceptance",
        phase: "preaccept",
      },
    },
  });
  expect(routes[0]!.context.route).toEqual({
    model: { provider: "anthropic", tier: "standard", capabilityClass: "unknown" },
    effort: "medium",
    attempt: 1,
  });
  expect(routes[1]!.context.route).toMatchObject({
    model: { provider: "openai", tier: "standard", capabilityClass: "unknown" },
    effort: "medium",
    attempt: 2,
  });
  expect(routes[1]!.context.route.contextWindow).toBeGreaterThan(0);
  expect(JSON.stringify(events)).not.toContain("gpt-5.6");
  expect(routing.fallbackPath).toEqual(["anthropic", "openai"]);
});

test("throwing event subscribers cannot abort a proof-carrying fallback", async () => {
  const writer = startedWriter("fallback-observer-isolation");
  const routing = decision();
  let fallbackQueries = 0;
  const registry = {
    anthropic: provider("anthropic", () => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        throw provedUnsent("primary unavailable before acceptance");
      },
    })),
    openai: provider("openai", (args) => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        fallbackQueries++;
        yield args.context.writer.append({
          kind: "run.progress",
          lifecycle: "running",
          progress: { currentAction: "fallback provider accepted the turn" },
        });
      },
    })),
  };
  const query = routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  );
  const observed: WireEvent[] = [];
  query.subscribeProviderEvents!((event) => {
    observed.push(event);
    throw new Error("presentation observer failed");
  });

  const events = await eventsOf(query);

  expect(fallbackQueries).toBe(1);
  expect(events.map((event) => event.kind)).toEqual(["run.progress", "run.progress"]);
  expect(observed).toEqual(events);
  expect(routing.fallbackCount).toBe(1);
  expect(writer.events()).toHaveLength(3);
});

test("fallback is forbidden when an adapter changes the shared checkpoint", async () => {
  const writer = startedWriter("checkpoint");
  const routing = decision();
  let fallbackQueries = 0;
  const registry = {
    anthropic: provider("anthropic", (args) => {
      args.context.writer.append({
        kind: "run.progress",
        lifecycle: "running",
        progress: { currentAction: "observable adapter activity" },
      });
      return {
        async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
          throw provedUnsent("proof cannot erase semantic activity");
        },
      };
    }),
    openai: provider("openai", () => {
      fallbackQueries++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };

  await expect(eventsOf(routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  ))).rejects.toThrow("proof cannot erase semantic activity");
  expect(fallbackQueries).toBe(0);
  expect(routing.fallbackCount).toBe(0);
  expect(writer.snapshot()?.lastSequence).toBe(1);
});

test("fallback is forbidden after an adapter yields an observable event", async () => {
  const writer = startedWriter("yielded");
  const routing = decision();
  let fallbackQueries = 0;
  const alreadyCommitted = writer.events()[0]!;
  const registry = {
    anthropic: provider("anthropic", () => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        yield alreadyCommitted;
        throw provedUnsent("yielded provider activity cannot be replayed");
      },
    })),
    openai: provider("openai", () => {
      fallbackQueries++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };
  const seen: WireEvent[] = [];

  await expect(async () => {
    for await (const event of routedQueryWithRegistry(
      routing,
      { input: "turn", options: { effort: "medium" } as Options, writer },
      "standard",
      registry,
    )) seen.push(event);
  }).toThrow("yielded provider activity cannot be replayed");
  expect(seen).toEqual([alreadyCommitted]);
  expect(fallbackQueries).toBe(0);
  expect(routing.fallbackCount).toBe(0);
});

test("fallback is forbidden after an adapter publishes an observable event", async () => {
  const writer = startedWriter("subscription-checkpoint");
  const routing = decision();
  let fallbackQueries = 0;
  const registry = {
    anthropic: provider("anthropic", (args) => {
      const listeners = new Set<(event: WireEvent) => void>();
      return {
        subscribeProviderEvents(listener: (event: WireEvent) => void): () => void {
          listeners.add(listener);
          return () => listeners.delete(listener);
        },
        async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
          const event = args.context.writer.events()[0];
          if (!event) throw new Error("started writer lost its first event");
          for (const listener of listeners) listener(event);
          throw provedUnsent("subscription publication forbids fallback");
        },
      };
    }),
    openai: provider("openai", () => {
      fallbackQueries++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };

  await expect(eventsOf(routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  ))).rejects.toThrow("subscription publication forbids fallback");
  expect(fallbackQueries).toBe(0);
  expect(routing.fallbackCount).toBe(0);
});

test("router preserves semantic controls, observations, and event subscriptions", async () => {
  const writer = startedWriter("controls");
  const routing = decision();
  routing.requestedProvider = "anthropic";
  routing.fallbackTargets = [];
  routing.fallbackProviders = [];
  const calls: string[] = [];
  let continuedInput = "";
  let selection: WireModelSelection | undefined;
  const registry = {
    anthropic: provider("anthropic", (args) => ({
      executionTransport: "sdk-stream",
      async continueTurn(nextInput: WireQueryInput): Promise<void> {
        calls.push("continueTurn");
        continuedInput = await firstText(nextInput);
      },
      async interruptTurn(): Promise<void> { calls.push("interruptTurn"); },
      async interrupt(): Promise<void> { calls.push("interrupt"); },
      async close(): Promise<void> { calls.push("close"); },
      forceClose(): void { calls.push("forceClose"); },
      setModel(nextSelection: WireModelSelection): void {
        calls.push("setModel");
        selection = nextSelection;
      },
      applyFlagSettings(settings): void {
        calls.push(`effort:${settings.effortLevel}`);
      },
      supportsInFlightEscalation: () => true,
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        yield args.context.writer.append({
          kind: "run.progress",
          lifecycle: "waiting",
          progress: { currentAction: "waiting" },
        });
      },
    })),
    openai: provider("openai", () => {
      throw new Error("unused provider");
    }),
  };
  const query = routedQueryWithRegistry(
    routing,
    { input: "initial", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  );
  const observed: WireEvent[] = [];
  const unsubscribe = query.subscribeProviderEvents!((event) => observed.push(event));

  const iterated = await eventsOf(query);
  await query.continueTurn!({
    async *[Symbol.asyncIterator](): AsyncIterator<WireUserInputMessage> {
      yield { kind: "user.input", text: "continued" };
    },
  });
  const continuedEvents = await eventsOf(query);
  await query.interruptTurn!();
  await query.interrupt!();
  await query.setModel!({ provider: "anthropic", tier: "senior" });
  await query.applyFlagSettings!({ effortLevel: "high" });
  expect(query.executionTransport).toBe("sdk-stream");
  expect(query.supportsInFlightEscalation!()).toBe(true);
  await query.close!();
  unsubscribe();

  expect(observed).toEqual([...iterated, ...continuedEvents]);
  expect(continuedInput).toBe("continued");
  expect(selection).toEqual({ provider: "anthropic", tier: "senior" });
  expect(routing.resolvedModel).toBe(resolveTier("anthropic", "senior").model);
  expect(routing.resolvedEffort).toBe("high");
  expect(calls).toEqual([
    "continueTurn",
    "interruptTurn",
    "interrupt",
    "setModel",
    "effort:high",
    "close",
  ]);
});

test("router rejects an unstarted writer before constructing a provider", () => {
  const writer = new WireEventWriter({ runId: wireRunId("run:unstarted") });
  let constructions = 0;
  const registry = {
    anthropic: provider("anthropic", () => {
      constructions++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
    openai: provider("openai", () => {
      constructions++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };

  expect(() => routedQueryWithRegistry(
    decision(),
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  )).toThrow("already-started running writer");
  expect(constructions).toBe(0);
});

test("closing a routed query before iteration constructs no provider", async () => {
  const writer = startedWriter("closed-before-iteration");
  let constructions = 0;
  const registry = {
    anthropic: provider("anthropic", () => {
      constructions++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
    openai: provider("openai", () => {
      constructions++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };
  const query = routedQueryWithRegistry(
    decision(),
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  );

  await query.close!();

  expect(await eventsOf(query)).toEqual([]);
  expect(constructions).toBe(0);
});

test("an exact target pin forbids fallback even with proved-unsent evidence", async () => {
  const writer = startedWriter("exact-target");
  const routing = decision();
  routing.requestedTarget = "claude-personal";
  let fallbackQueries = 0;
  const registry = {
    anthropic: provider("anthropic", () => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        throw provedUnsent("exact target unavailable before acceptance");
      },
    })),
    openai: provider("openai", () => {
      fallbackQueries++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };

  await expect(eventsOf(routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  ))).rejects.toThrow("exact target unavailable");
  expect(fallbackQueries).toBe(0);
  expect(routing.fallbackCount).toBe(0);
  expect(writer.events()).toHaveLength(1);
});

test("ordinary provider failures never infer preaccept retry safety from prose", async () => {
  const writer = startedWriter("unproved-failure");
  const routing = decision();
  let fallbackQueries = 0;
  const registry = {
    anthropic: provider("anthropic", () => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        throw new Error("authentication required before acceptance");
      },
    })),
    openai: provider("openai", () => {
      fallbackQueries++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };

  await expect(eventsOf(routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  ))).rejects.toThrow("authentication required");
  expect(fallbackQueries).toBe(0);
  expect(routing.fallbackCount).toBe(0);
  expect(writer.events()).toHaveLength(1);
});

test("fallback admission completes before the fallback provider is constructed", async () => {
  const writer = startedWriter("fallback-admission");
  const routing = decision();
  let fallbackQueries = 0;
  const registry = {
    anthropic: provider("anthropic", () => ({
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        throw provedUnsent("primary unavailable before acceptance");
      },
    })),
    openai: provider("openai", () => {
      fallbackQueries++;
      return { async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {} };
    }),
  };

  await expect(eventsOf(routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
    async () => { throw new Error("resource admission denied"); },
  ))).rejects.toThrow("resource admission denied");
  expect(fallbackQueries).toBe(0);
  expect(routing.fallbackCount).toBe(0);
  expect(writer.events()).toHaveLength(1);
});

test("failed semantic controls leave the active route unchanged", async () => {
  const writer = startedWriter("failed-controls");
  const routing = decision();
  routing.requestedProvider = "anthropic";
  routing.fallbackTargets = [];
  routing.fallbackProviders = [];
  const registry = {
    anthropic: provider("anthropic", (args) => ({
      setModel: async () => { throw new Error("model control rejected"); },
      applyFlagSettings: async () => { throw new Error("effort control rejected"); },
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        yield args.context.writer.append({
          kind: "run.progress",
          lifecycle: "waiting",
          progress: { currentAction: "waiting" },
        });
      },
    })),
    openai: provider("openai", () => {
      throw new Error("unused provider");
    }),
  };
  const query = routedQueryWithRegistry(
    routing,
    { input: "turn", options: { model: "opus", effort: "medium" } as Options, writer },
    "standard",
    registry,
  );
  await eventsOf(query);
  const resolvedModel = routing.resolvedModel;
  const resolvedEffort = routing.resolvedEffort;

  await expect(query.setModel!({ provider: "anthropic", tier: "senior" }))
    .rejects.toThrow("model control rejected");
  await expect(query.applyFlagSettings!({ effortLevel: "high" }))
    .rejects.toThrow("effort control rejected");

  expect(routing.resolvedModel).toBe(resolvedModel);
  expect(routing.resolvedEffort).toBe(resolvedEffort);
});

test("router forwards provider execution activity through its stable source", async () => {
  const writer = startedWriter("activity-forwarding");
  const routing = decision();
  routing.requestedProvider = "anthropic";
  routing.fallbackTargets = [];
  routing.fallbackProviders = [];
  const activity = createExecutionActivityEmitter();
  const registry = {
    anthropic: provider("anthropic", (args) => ({
      executionActivity: activity.source,
      async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        activity.record("provider", "provider.tool.completed");
        yield args.context.writer.append({
          kind: "run.progress",
          lifecycle: "waiting",
          progress: { currentAction: "waiting" },
        });
      },
    })),
    openai: provider("openai", () => {
      throw new Error("unused provider");
    }),
  };
  const query = routedQueryWithRegistry(
    routing,
    { input: "turn", options: { effort: "medium" } as Options, writer },
    "standard",
    registry,
  );
  const stableSource = query.executionActivity;
  let notifications = 0;
  const unsubscribe = stableSource!.subscribe(() => { notifications++; });

  await eventsOf(query);
  unsubscribe();

  expect(query.executionActivity).toBe(stableSource);
  expect(notifications).toBe(1);
  expect(stableSource!.snapshot()).toMatchObject({
    sequence: 1,
    lastProvider: { origin: "provider", kind: "provider.tool.completed" },
  });
});
