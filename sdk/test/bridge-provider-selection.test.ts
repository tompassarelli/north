import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import {
  MemoryBridgeCommandReceipts,
  type BridgeAttemptRouteAuthority,
} from "../src/bridge/command-receipts";
import {
  "parse-bridge-launch-arguments!" as parseBridgeLaunchArguments,
} from "../src/bridge/generated/north/bridge/cli.js";
import {
  "parse-bridge-request!" as parseBridgeRequest,
  type BridgeServerMessage,
} from "../src/bridge/generated/north/bridge/protocol.js";
import {
  bridgeProviderWithDependenciesForTest, bridgeRoute,
  resolveBridgeLaunchSelection, selectBridgeProvider,
  type BridgeProviderExecution,
  type BridgeProviderOpenContext,
} from "../src/bridge/provider";
import { authCacheKey, writeAuthState } from "../src/provider-auth-cache";
import {
  BOOT_ROUTING_TIMEOUT_MS, refreshProviderRoutingInBackground,
  selectProviderFromCachedState,
} from "../src/provider-routing";
import type { AgentProvider, AgentProviderQuery, RoutingDecision } from "../src/providers/types";
import { WireEventWriter, wireRunId, type WireEvent, type WireQuery } from "../src/wire";

const cleanups: Array<() => Promise<void> | void> = [];
const ATTEMPT_ID = `@attempt:${"a".repeat(64)}`;
const STORE_ROUTE = Object.freeze({
  attemptId: ATTEMPT_ID,
  provider: "openai",
  accountId: "codex-store",
  credentialProfile: "store-profile",
  model: "gpt-5.6-terra",
  accountAuthorityReceiptSha256: "b".repeat(64),
  routeObservationReceiptSha256: "c".repeat(64),
  launchIntentSha256: "d".repeat(64),
} satisfies BridgeAttemptRouteAuthority);

class EmptyQuery implements WireQuery {
  async interrupt(): Promise<void> {}
  async close(): Promise<void> {}
  async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {}
}
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

test("--claude and --openai pin a provider, and bare launches leave it unset", () => {
  expect(parseBridgeLaunchArguments(["--attempt", ATTEMPT_ID, "--claude", "go"]))
    .toEqual({ role: "implementer", attemptId: ATTEMPT_ID, provider: "anthropic", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--openai", "--attempt", ATTEMPT_ID, "go"]))
    .toEqual({ role: "implementer", attemptId: ATTEMPT_ID, provider: "openai", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--attempt", ATTEMPT_ID, "go"]))
    .toEqual({ role: "implementer", attemptId: ATTEMPT_ID, promptArguments: ["go"] });
});

test("a provider flag composes with --role in either order", () => {
  expect(parseBridgeLaunchArguments(["--role", "director", "--attempt", ATTEMPT_ID, "--claude", "go"]))
    .toEqual({ role: "director", attemptId: ATTEMPT_ID, provider: "anthropic", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--claude", "--attempt", ATTEMPT_ID, "--role", "director", "go"]))
    .toEqual({ role: "director", attemptId: ATTEMPT_ID, provider: "anthropic", promptArguments: ["go"] });
});

test("launch route flags compose before the prompt", () => {
  expect(parseBridgeLaunchArguments([
    "--attempt", ATTEMPT_ID, "--provider", "openai", "--tier", "frontier", "--effort", "max", "go",
  ])).toEqual({
    role: "implementer", attemptId: ATTEMPT_ID,
    provider: "openai", tier: "frontier", effort: "max",
    promptArguments: ["go"],
  });
  expect(parseBridgeLaunchArguments([
    "--role", "director", "--attempt", ATTEMPT_ID, "--model", "gpt-5.6-sol", "review",
  ])).toEqual({
    role: "director", attemptId: ATTEMPT_ID,
    model: "gpt-5.6-sol", promptArguments: ["review"],
  });
});

test("the wire rejects an unknown provider", () => {
  expect(() => parseBridgeRequest({ op: "launch", prompt: "go", cwd: "/", provider: "gemini" }))
    .toThrow("bridge launch provider must be anthropic or openai");
});

test("the wire validates and preserves every launch route override", () => {
  expect(parseBridgeRequest({
    op: "launch", prompt: "go", cwd: "/", role: "director",
    attemptId: ATTEMPT_ID,
    provider: "openai", tier: "frontier", model: "gpt-5.6-sol", effort: "max",
  })).toEqual({
    op: "launch", prompt: "go", cwd: "/", role: "director",
    attemptId: ATTEMPT_ID,
    provider: "openai", tier: "frontier", model: "gpt-5.6-sol", effort: "max",
  });
  expect(() => parseBridgeRequest({ op: "launch", prompt: "go", cwd: "/", tier: "extreme" }))
    .toThrow("bridge launch tier must be one of");
  expect(() => parseBridgeRequest({ op: "launch", prompt: "go", cwd: "/", model: "two words" }))
    .toThrow("bridge launch model must be a model id without whitespace");
  expect(() => parseBridgeRequest({ op: "launch", prompt: "go", cwd: "/", effort: "ultra" }))
    .toThrow("bridge launch effort must be one of");
});

test("automatic effort follows the selected provider and tier", () => {
  const frontier = resolveBridgeLaunchSelection("openai", "implementer", {
    tier: "frontier",
  });
  expect(frontier.resolved).toEqual({
    tier: "frontier", model: "gpt-5.6-sol", effort: "xhigh",
  });
  expect(frontier.routingMetadata).toMatchObject({
    role: "implementer", tier: "frontier", reasoning: "xhigh",
    composition: { overrides: ["tier", "reasoning"] },
  });

  const exact = resolveBridgeLaunchSelection("openai", "implementer", { model: "terra" });
  expect(exact.resolved).toEqual({
    tier: "standard", model: "gpt-5.6-terra", effort: "high",
  });
  expect(exact.routingMetadata).toMatchObject({
    role: "implementer", tier: "standard", reasoning: "medium",
    composition: { overrides: [] },
  });

  expect(resolveBridgeLaunchSelection("openai", "implementer", {}).resolved).toEqual({
    tier: "standard", model: "gpt-5.6-terra", effort: "medium",
  });

  const explicit = resolveBridgeLaunchSelection("openai", "director", { effort: "max" });
  expect(explicit.resolved).toEqual({
    tier: "frontier", model: "gpt-5.6-sol", effort: "max",
  });
  expect(explicit.routingMetadata).toMatchObject({
    role: "director", tier: "frontier", reasoning: "max",
    composition: { overrides: ["reasoning"] },
  });
  expect(() => resolveBridgeLaunchSelection("anthropic", "director", { effort: "max" }))
    .toThrow("cannot resolve semantic tier frontier with reasoning max");
});

async function launched(
  request: object,
  authority: BridgeAttemptRouteAuthority = STORE_ROUTE,
): Promise<BridgeProviderOpenContext | undefined> {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-select-"));
  const socketPath = join(root, "northd.sock");
  const opened: BridgeProviderOpenContext[] = [];
  const provider: BridgeProviderExecution = {
    async open(context) {
      opened.push(context);
      throw new Error("stop after selection");
    },
  };
  const northd = new Northd({
    socketPath, journalRoot: join(root, "journal"), provider,
    sourceIdentity: () => undefined,
    commandReceipts: new MemoryBridgeCommandReceipts([authority]),
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const socket: Socket = connect(socketPath);
  const connected = Promise.withResolvers<void>();
  socket.once("connect", () => connected.resolve());
  socket.once("error", connected.reject);
  await connected.promise;
  const messages: BridgeServerMessage[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      messages.push(JSON.parse(buffer.slice(0, newline)) as BridgeServerMessage);
      buffer = buffer.slice(newline + 1);
    }
  });
  const closed = Promise.withResolvers<void>();
  socket.once("close", () => closed.resolve());
  socket.write(`${JSON.stringify({ ...request, attemptId: authority.attemptId })}\n`);
  await closed.promise;
  return opened[0];
}

test("the host refuses request route conflicts and forwards exact Store authority", async () => {
  expect(await launched({
    op: "launch", prompt: "wrong", cwd: "/", role: "implementer",
    provider: "anthropic", model: "claude-sonnet-4-6",
  })).toBeUndefined();
  const result = await launched({
    op: "launch", prompt: "go", cwd: "/", role: "implementer",
    provider: "openai", model: "gpt-5.6-terra",
  });
  expect(result).toMatchObject({
    prompt: "go", cwd: "/", role: "implementer", provider: "openai",
    model: "gpt-5.6-terra", attemptRoute: STORE_ROUTE,
  });

  let dynamicRouteReads = 0;
  let admittedTarget: unknown;
  let query: AgentProviderQuery | undefined;
  const adapter: AgentProvider = {
    id: "openai",
    liveInput: "turn-messages",
    probe: () => ({ provider: "openai", available: true, reason: "ready" }),
    admit: ({ target }) => { admittedTarget = target; },
    query: (args) => { query = args; return new EmptyQuery(); },
  };
  const exactTarget = {
    id: "codex-store", provider: "openai" as const,
    authMode: "isolated" as const, profile: "store-profile",
  };
  const bridge = bridgeProviderWithDependenciesForTest(
    { anthropic: adapter, openai: adapter },
    {
      BOOT_ROUTING_TIMEOUT_MS,
      selectProviderFromCachedState: async () => {
        dynamicRouteReads += 1;
        return undefined;
      },
      refreshProviderRoutingInBackground: () => {
        dynamicRouteReads += 1;
        return Promise.resolve();
      },
      selectProviderForExecution: async () => {
        dynamicRouteReads += 1;
        throw new Error("dynamic route must not run");
      },
      configuredDefaultTarget: () => {
        dynamicRouteReads += 1;
        return { id: "wrong", provider: "openai" };
      },
      resourcePolicyFromEnv: () => ({
        version: 1,
        mode: "preferential",
        targets: [exactTarget, { id: "wrong", provider: "openai", authMode: "ambient" }],
        targetOrder: ["wrong", exactTarget.id],
        providerOrder: ["anthropic", "openai"],
        envelopes: {},
      }),
    },
  );
  const writer = new WireEventWriter({ runId: wireRunId("run:bridge-store-route") });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:implementer" });
  const session = await bridge.open({
    executionId: "store-route",
    prompt: "go",
    cwd: "/",
    role: "implementer",
    provider: "openai",
    model: "gpt-5.6-terra",
    attemptRoute: STORE_ROUTE,
    signal: new AbortController().signal,
    writer,
  });
  expect(dynamicRouteReads).toBe(0);
  expect(admittedTarget).toEqual(exactTarget);
  expect(query?.target).toEqual(exactTarget);
  expect(query?.options.model).toBe("gpt-5.6-terra");
  await session.terminateSession();
});

// --- Boot routing -----------------------------------------------------------
//
// Opening a session used to wait on live entitlement probes twice: once to pick
// a provider and once to pick that provider's account. Both are network round
// trips, both happen before anything is on screen, and together they were most
// of the wall time between "north bridge" and a usable session.

function routingSandbox(): string {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-routing-"));
  saved.push(...([
    "NORTH_ROUTING_POLICY", "NORTH_AUTH_STATE_CACHE", "NORTH_PROVIDER_OBSERVATIONS",
    "NORTH_PROVIDER_MODEL_OBSERVATIONS", "NORTH_CLAUDE_BIN", "NORTH_CODEX_BIN",
    "AGENT_PROVIDER",
  ] as const).map((key) => [key, process.env[key]] as const));
  writeFileSync(join(root, "routing-policy.json"), JSON.stringify({
    version: 1,
    mode: "preferential",
    targets: [
      { id: "claude-a", provider: "anthropic", authMode: "ambient" },
      { id: "codex-a", provider: "openai", authMode: "ambient" },
    ],
    targetOrder: ["codex-a", "claude-a"],
    envelopes: {},
  }));
  process.env.NORTH_ROUTING_POLICY = join(root, "routing-policy.json");
  process.env.NORTH_AUTH_STATE_CACHE = join(root, "auth-state.json");
  process.env.NORTH_PROVIDER_OBSERVATIONS = join(root, "observations.json");
  process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = join(root, "model-observations.json");
  // Nothing here may run: if the boot path probes rather than reads, the verdict
  // comes back command_missing and every assertion below fails.
  process.env.NORTH_CLAUDE_BIN = join(root, "no-such-claude");
  process.env.NORTH_CODEX_BIN = join(root, "no-such-codex");
  delete process.env.AGENT_PROVIDER;
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

const saved: Array<readonly [string, string | undefined]> = [];
afterEach(() => {
  for (const [key, value] of saved.splice(0))
    if (value === undefined) delete process.env[key]; else process.env[key] = value;
});

function ready(provider: "anthropic" | "openai") {
  return {
    provider, installed: true, authenticated: true, available: true,
    reason: "ready" as const, at: Date.now(),
  };
}

test("boot routes from the persisted verdicts, without spawning a probe", async () => {
  routingSandbox();
  const cache = process.env.NORTH_AUTH_STATE_CACHE!;
  writeAuthState(cache, authCacheKey("anthropic", "claude-a"), ready("anthropic"));
  writeAuthState(cache, authCacheKey("openai", "codex-a"), ready("openai"));

  const decision = await selectProviderFromCachedState({ provider: "anthropic" });
  expect(decision?.target).toBe("claude-a");
  expect(decision?.provider).toBe("anthropic");

  // The unpinned provider choice reads the same cache, and still follows
  // capacity: two accounts at equal headroom keep the order the bridge has
  // always opened on.
  expect(await selectBridgeProvider()).toBe("anthropic");
});

test("automatic provider choice filters route compatibility before headroom", async () => {
  routingSandbox();
  const cache = process.env.NORTH_AUTH_STATE_CACHE!;
  writeAuthState(cache, authCacheKey("anthropic", "claude-a"), ready("anthropic"));
  writeAuthState(cache, authCacheKey("openai", "codex-a"), ready("openai"));

  expect(await selectBridgeProvider({ role: "implementer", model: "terra" }))
    .toBe("openai");
  expect(await selectBridgeProvider({ role: "implementer", model: "sonnet", effort: "medium" }))
    .toBe("anthropic");
  expect(await selectBridgeProvider({ role: "director", effort: "max" }))
    .toBe("openai");
  await expect(selectBridgeProvider({
    role: "implementer", tier: "frontier", effort: "low",
  })).rejects.toThrow("no Bridge provider supports the requested launch route");
});

test("a verdict nobody ever recorded is not a route", async () => {
  routingSandbox();
  // Empty cache: the boot selector says so rather than inventing availability.
  expect(await selectProviderFromCachedState({ provider: "anthropic" })).toBeUndefined();
  // And an account that is cached as logged out is not routable either.
  writeAuthState(process.env.NORTH_AUTH_STATE_CACHE!, authCacheKey("anthropic", "claude-a"), {
    ...ready("anthropic"), authenticated: false, available: false,
    reason: "authentication_missing" as const,
  });
  expect(await selectProviderFromCachedState({ provider: "anthropic" })).toBeUndefined();
  // With nothing eligible anywhere, the unpinned choice falls back rather than
  // failing a launch outright.
  expect(await selectBridgeProvider()).toBe("openai");
});

test("a cached route opens the session and refreshes behind it", async () => {
  const routed = {
    target: "claude-a",
    routingTargets: { "claude-a": { id: "claude-a", provider: "anthropic", authMode: "ambient" } },
  };
  let refreshed = 0;
  let probed = 0;
  const route = await bridgeRoute({
    BOOT_ROUTING_TIMEOUT_MS,
    selectProviderFromCachedState: async () => routed as RoutingDecision,
    refreshProviderRoutingInBackground: () => { refreshed += 1; return Promise.resolve(); },
    selectProviderForExecution: async () => { probed += 1; throw new Error("boot must not probe"); },
    configuredDefaultTarget: () => undefined,
  }, "anthropic");

  expect(route.target?.id).toBe("claude-a");
  // The route came from disk, and the probe that proves it right runs behind
  // the session rather than in front of it.
  expect(probed).toBe(0);
  expect(refreshed).toBe(1);
});

test("a cold machine probes under a boot budget, then takes the configured default", async () => {
  const fallback = { id: "claude-a", provider: "anthropic" as const, authMode: "ambient" as const };
  const started = Date.now();
  const route = await bridgeRoute({
    BOOT_ROUTING_TIMEOUT_MS,
    selectProviderFromCachedState: async () => undefined,
    refreshProviderRoutingInBackground: () => Promise.resolve(),
    // A probe that answers only when the caller stops waiting: the boot budget
    // is the abort signal, and this is what a dead network looks like.
    selectProviderForExecution: (_preference, _policy, context) => {
      const result = Promise.withResolvers<never>();
      context!.signal!.addEventListener(
        "abort", () => result.reject(new Error("probe cancelled")), { once: true },
      );
      return result.promise;
    },
    configuredDefaultTarget: () => fallback,
  }, "anthropic");

  // Bounded by the budget, not by the provider.
  expect(Date.now() - started).toBeLessThan(BOOT_ROUTING_TIMEOUT_MS * 3);
  // And routed to somebody's account: without a target the adapter falls back to
  // ambient credentials, which are nobody's.
  expect(route.target).toEqual(fallback);
});

test("an explicit Bridge model is selected with its exact receipt and never falls back statically", async () => {
  const target = { id: "codex-a", provider: "openai" as const, authMode: "ambient" as const };
  const receipt = {
    provider: "openai" as const,
    targetId: target.id,
    authMode: "ambient" as const,
    model: "gpt-5.6-sol",
    observedAt: new Date().toISOString(),
    source: "codex-app-server:model-list" as const,
    observationDigest: "0".repeat(64),
  };
  let observedContext: unknown;
  let defaults = 0;
  const route = await bridgeRoute({
    BOOT_ROUTING_TIMEOUT_MS,
    selectProviderFromCachedState: async (_preference, _policy, context) => {
      observedContext = context;
      return {
        target: target.id,
        routingTargets: { [target.id]: target },
        modelAvailabilityReceipts: { [target.id]: receipt },
      } as RoutingDecision;
    },
    refreshProviderRoutingInBackground: () => Promise.resolve(),
    selectProviderForExecution: async () => { throw new Error("must not probe"); },
    configuredDefaultTarget: () => { defaults++; return target; },
  }, "openai", { tier: "frontier", reasoning: "xhigh", model: "gpt-5.6-sol" });
  expect(observedContext).toEqual({
    tier: "frontier", reasoning: "xhigh", model: "gpt-5.6-sol",
  });
  expect(route).toEqual({ target, receipt });
  expect(defaults).toBe(0);

  const blocked = await bridgeRoute({
    BOOT_ROUTING_TIMEOUT_MS,
    selectProviderFromCachedState: async () => undefined,
    refreshProviderRoutingInBackground: () => Promise.resolve(),
    selectProviderForExecution: async () => { throw new Error("model not observed"); },
    configuredDefaultTarget: () => { defaults++; return target; },
  }, "openai", { tier: "frontier", reasoning: "xhigh", model: "gpt-5.6-sol" });
  expect(blocked).toEqual({});
  expect(defaults).toBe(0);
});

test("an automatic model routes by resolved tier and effort without becoming an exact pin", async () => {
  const target = { id: "codex-a", provider: "openai" as const, authMode: "ambient" as const };
  let observedContext: unknown;
  const route = await bridgeRoute({
    BOOT_ROUTING_TIMEOUT_MS,
    selectProviderFromCachedState: async (_preference, _policy, context) => {
      observedContext = context;
      return undefined;
    },
    refreshProviderRoutingInBackground: () => Promise.resolve(),
    selectProviderForExecution: async (_preference, _policy, context) => {
      observedContext = context;
      return {
        target: target.id,
        routingTargets: { [target.id]: target },
      } as RoutingDecision;
    },
    configuredDefaultTarget: () => target,
  }, "openai", { tier: "frontier", reasoning: "xhigh" });

  expect(observedContext).toMatchObject({ tier: "frontier", reasoning: "xhigh" });
  expect(observedContext).not.toHaveProperty("model");
  expect(route).toEqual({ target });
});

test("the background refresh collapses, never rejects, and never blocks", async () => {
  let refreshes = 0;
  const failing = async () => { refreshes += 1; throw new Error("provider unreachable"); };
  const first = refreshProviderRoutingInBackground({ provider: "anthropic" }, failing);
  const second = refreshProviderRoutingInBackground({ provider: "anthropic" }, failing);
  expect(second).toBe(first);
  await expect(first).resolves.toBeUndefined();
  expect(refreshes).toBe(1);

  // A settled refresh releases the slot for the next boot.
  await refreshProviderRoutingInBackground({ provider: "anthropic" }, failing);
  expect(refreshes).toBe(2);
});
