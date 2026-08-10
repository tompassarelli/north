import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { connect, type Socket } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Northd } from "../src/bridge/host";
import { parseBridgeLaunchArguments } from "../src/bridge/cli";
import { parseBridgeRequest } from "../src/bridge/protocol";
import {
  bridgeRoute, selectBridgeProvider, type BridgeProviderExecution,
} from "../src/bridge/provider";
import { authCacheKey, writeAuthState } from "../src/provider-auth-cache";
import {
  BOOT_ROUTING_TIMEOUT_MS, refreshProviderRoutingInBackground,
  selectProviderFromCachedState,
} from "../src/provider-routing";

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

test("--claude and --openai pin a provider, and bare launches leave it unset", () => {
  expect(parseBridgeLaunchArguments(["--claude", "go"]))
    .toEqual({ role: "implementer", provider: "anthropic", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--openai", "go"]))
    .toEqual({ role: "implementer", provider: "openai", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["go"]))
    .toEqual({ role: "implementer", promptArguments: ["go"] });
});

test("a provider flag composes with --role in either order", () => {
  expect(parseBridgeLaunchArguments(["--role", "director", "--claude", "go"]))
    .toEqual({ role: "director", provider: "anthropic", promptArguments: ["go"] });
  expect(parseBridgeLaunchArguments(["--claude", "--role", "director", "go"]))
    .toEqual({ role: "director", provider: "anthropic", promptArguments: ["go"] });
});

test("the wire rejects an unknown provider", () => {
  expect(() => parseBridgeRequest({ op: "launch", prompt: "go", cwd: "/", provider: "gemini" }))
    .toThrow("bridge launch provider must be anthropic or openai");
});

async function launched(request: object, selectProvider: () => Promise<any>): Promise<any> {
  const root = mkdtempSync(join(tmpdir(), "north-bridge-select-"));
  const socketPath = join(root, "northd.sock");
  const opened: any[] = [];
  const provider: BridgeProviderExecution = {
    async open(context) {
      opened.push(context.provider);
      throw new Error("stop after selection");
    },
  };
  const northd = new Northd({
    socketPath, journalRoot: join(root, "journal"), provider,
    sourceIdentity: () => undefined, selectProvider,
  });
  await northd.listen();
  cleanups.push(() => rmSync(root, { recursive: true, force: true }));
  cleanups.push(() => northd.close());

  const socket: Socket = connect(socketPath);
  await new Promise<void>((resolve, reject) => {
    socket.once("connect", resolve);
    socket.once("error", reject);
  });
  const messages: any[] = [];
  let buffer = "";
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    buffer += chunk;
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      messages.push(JSON.parse(buffer.slice(0, newline)));
      buffer = buffer.slice(newline + 1);
    }
  });
  const closed = new Promise<void>((resolve) => socket.once("close", () => resolve()));
  socket.write(`${JSON.stringify(request)}\n`);
  await closed;
  const starting = messages.find((message) =>
    message.type === "event" && message.record.kind === "provider.starting");
  return { chosen: opened[0], starting: starting?.record.data };
}

test("an unpinned launch takes the headroom selection", async () => {
  const result = await launched(
    { op: "launch", prompt: "go", cwd: "/" },
    async () => "anthropic",
  );
  expect(result.chosen).toBe("anthropic");
  expect(result.starting.provider).toBe("anthropic");
  expect(result.starting.selection).toBe("headroom");
});

test("a pinned launch never consults headroom", async () => {
  let consulted = 0;
  const result = await launched(
    { op: "launch", prompt: "go", cwd: "/", provider: "openai" },
    async () => { consulted += 1; return "anthropic"; },
  );
  expect(result.chosen).toBe("openai");
  expect(result.starting.selection).toBe("pinned");
  expect(consulted).toBe(0);
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
    weights: {}, pressures: {}, envelopes: {},
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
    selectProviderFromCachedState: async () => routed as any,
    refreshProviderRoutingInBackground: () => { refreshed += 1; return Promise.resolve(); },
    selectProviderForExecution: async () => { probed += 1; throw new Error("boot must not probe"); },
    configuredDefaultTarget: () => undefined,
  }, "anthropic");

  expect(route.id).toBe("claude-a");
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
    selectProviderForExecution: (_preference, _policy, context) =>
      new Promise((_resolve, reject) => {
        context!.signal!.addEventListener(
          "abort", () => reject(new Error("probe cancelled")), { once: true },
        );
      }),
    configuredDefaultTarget: () => fallback,
  }, "anthropic");

  // Bounded by the budget, not by the provider.
  expect(Date.now() - started).toBeLessThan(BOOT_ROUTING_TIMEOUT_MS * 3);
  // And routed to somebody's account: without a target the adapter falls back to
  // ambient credentials, which are nobody's.
  expect(route.target).toEqual(fallback);
  expect(route.id).toBe("claude-a");
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
