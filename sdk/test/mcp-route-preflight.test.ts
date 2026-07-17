import { expect, test } from "bun:test";
import type { ResourcePolicy, RoutingDecision } from "../src/providers/types";
import {
  preflightMcpRoutePin, refreshMcpRoutePinUsage, validateConfiguredRoutePin,
} from "../src/mcp-route-preflight";

const policy: ResourcePolicy = {
  version: 1,
  mode: "balanced",
  targets: [
    { id: "claude-proton", provider: "anthropic", authMode: "isolated", profile: "claude-proton" },
    { id: "claude-gmail", provider: "anthropic", authMode: "isolated", profile: "claude-gmail" },
    { id: "codex-proton", provider: "openai", authMode: "isolated", profile: "codex-proton" },
  ],
  targetOrder: ["claude-proton", "claude-gmail", "codex-proton"],
  providerOrder: ["anthropic", "openai"],
  pressures: {},
};

test("MCP exact pins must name a configured target owned by the requested provider", () => {
  expect(validateConfiguredRoutePin({ target: "claude-gmail" }, policy)).toEqual({
    target: "claude-gmail", provider: "anthropic",
  });
  expect(() => validateConfiguredRoutePin({ target: "missing" }, policy)).toThrow("not configured");
  expect(() => validateConfiguredRoutePin({ target: "claude-gmail", provider: "openai" }, policy))
    .toThrow("belongs to anthropic");
});

test("MCP exact pin preflight refuses any selector that changes target or retains sibling fallback", () => {
  const decision = {
    target: "claude-gmail", provider: "anthropic", fallbackTargets: [],
    selectionReason: "exact target=claude-gmail",
  } as RoutingDecision;
  expect(preflightMcpRoutePin({ target: "claude-gmail" }, {
    policy, select: () => decision,
  })).toBe(decision);

  expect(() => preflightMcpRoutePin({ target: "claude-gmail" }, {
    policy,
    select: () => ({ ...decision, fallbackTargets: ["claude-proton"] }),
  })).toThrow("violated pin contract");
});

test("MCP exact pin preflight carries the explicit model into canonical selection", () => {
  const decision = {
    target: "claude-gmail", provider: "anthropic", fallbackTargets: [],
    selectionReason: "exact model-aware target",
  } as RoutingDecision;
  let context: any;
  preflightMcpRoutePin({
    target: "claude-gmail", provider: "anthropic", tier: "senior", reasoning: "high",
    model: "claude-sonnet-5",
  }, {
    policy,
    select: ((_request, _policy, value) => { context = value; return decision; }) as any,
  });
  expect(context).toMatchObject({
    tier: "senior", reasoning: "high", model: "claude-sonnet-5",
  });
});

test("MCP exact pin refreshes only the selected account and telemetry failure is soft", async () => {
  const refreshed: string[][] = [];
  let codexRefreshes = 0;
  const accounts = () => [
    { id: "claude-proton", provider: "anthropic" as const, profile: "claude-proton", authMode: "isolated" as const, root: "/tmp/proton" },
    { id: "claude-gmail", provider: "anthropic" as const, profile: "claude-gmail", authMode: "isolated" as const, root: "/tmp/gmail" },
    { id: "codex-proton", provider: "openai" as const, profile: "codex-proton", authMode: "isolated" as const, root: "/tmp/codex" },
  ];
  await refreshMcpRoutePinUsage({ target: "claude-gmail" }, {
    policy, accounts,
    refreshAccounts: (async ({ accounts }) => { refreshed.push(accounts!.map(({ id }) => id)); return []; }) as any,
    refreshCodex: (async () => { codexRefreshes++; return undefined; }) as any,
  });
  expect(refreshed).toEqual([["claude-gmail"]]);
  expect(codexRefreshes).toBe(0);

  await expect(refreshMcpRoutePinUsage({ target: "claude-gmail" }, {
    policy, accounts,
    refreshAccounts: (async () => { throw new Error("usage surface unavailable"); }) as any,
  })).resolves.toBeUndefined();
});
