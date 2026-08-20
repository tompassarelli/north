import { expect, test } from "bun:test";
import { allocateCodexExecutionAccount } from "../src/providers/codex-execution-allocation";
import type { AccountAvailabilityRow } from "../src/account-availability";
import type { RoutingTarget } from "../src/providers/types";

const targets: RoutingTarget[] = [
  { id: "codex-apple", provider: "openai", authMode: "isolated", profile: "apple" },
  { id: "codex-proton", provider: "openai", authMode: "isolated", profile: "proton" },
  { id: "codex-gmail", provider: "openai", authMode: "isolated", profile: "gmail" },
  { id: "codex-pm", provider: "openai", authMode: "isolated", profile: "pm" },
];

function availability(account: string, pct: number): AccountAvailabilityRow {
  return {
    account, provider: "openai", observedAt: "2026-08-20T10:00:00.000Z", stale: false,
    rungs: { window: { name: "primary", pct, resetsAt: "2026-08-27T10:00:00.000Z" }, week: null, models: {} },
    verdict: "available", usableModels: [],
  };
}

test("allocates supported Luna, Terra, and Sol work across fresh execution accounts, never PM", async () => {
  const runtime = targets.map((target) => ({
    targetId: target.id, provider: "openai" as const, available: true, reason: "ready" as const,
  }));
  const rows = [
    availability("codex-apple", 30),
    availability("codex-proton", 98),
    availability("codex-gmail", 10),
    availability("codex-pm", 0),
  ];
  const dependencies = {
    readAvailability: () => rows,
    readActivity: async (target: RoutingTarget) => ({
      hours: 24, sessions: 1, live: target.id === "codex-gmail" ? 2 : 0, outputTokens: 0,
    }),
  };

  await expect(allocateCodexExecutionAccount(targets, runtime, "economy", "low", dependencies))
    .resolves.toMatchObject({ target: { id: "codex-apple" }, model: "gpt-5.6-luna", effort: "low" });
  await expect(allocateCodexExecutionAccount(targets, runtime, "standard", "medium", dependencies))
    .resolves.toMatchObject({ target: { id: "codex-apple" }, model: "gpt-5.6-terra", effort: "medium" });
  await expect(allocateCodexExecutionAccount(targets, runtime, "senior", "high", dependencies))
    .resolves.toMatchObject({ target: { id: "codex-apple" }, model: "gpt-5.6-sol", effort: "high" });
});
