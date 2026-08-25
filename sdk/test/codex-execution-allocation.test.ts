import { expect, test } from "bun:test";
import { allocateCodexExecutionAccount } from "../src/providers/codex-execution-allocation";
import { readCodexAccountAuthority, type CodexAccountAuthority } from "../src/accounts";
import { StoreTriple } from "../src/store-rpc-codec";
import type { RoutingTarget } from "../src/providers/types";
import type { StoreObservationSnapshot } from "../src/store-observation-adapter";
import type { ProviderUsageObservation } from "../src/providers/types";

const targets: RoutingTarget[] = [
  { id: "codex-apple", provider: "openai", authMode: "isolated", profile: "apple" },
  { id: "codex-proton", provider: "openai", authMode: "isolated", profile: "proton" },
  { id: "codex-gmail", provider: "openai", authMode: "isolated", profile: "gmail" },
  { id: "codex-pm", provider: "openai", authMode: "isolated", profile: "pm" },
];

function usage(account: string, pct: number): StoreObservationSnapshot<ProviderUsageObservation> {
  return {
    observation: {
      targetId: account, provider: "openai", source: "codex-app-server:account-rate-limits",
      observedAt: "2026-08-20T10:00:00.000Z",
      windows: [{ limitId: "codex:primary", usedPercent: pct, resetsAt: "2026-08-27T10:00:00.000Z" }],
    },
    receipt: {
      version: "north:provider-observation:v1", subject: `@provider-observation:usage:${account}`,
      digest: "usage-store-digest", servedVersion: 43,
    },
  };
}

function authority(
  target: RoutingTarget,
  role: "execution" | "oversight",
  executionEligible = role === "execution",
): CodexAccountAuthority {
  const subject = `@account:${target.id}`;
  return {
    role,
    executionEligible,
    receipt: {
      version: "north:codex-account-authority:v1",
      subject,
      servedVersion: 42,
      facts: [
        { predicate: "kind", value: "provider_account" },
        { predicate: "account_id", value: target.id },
        { predicate: "provider", value: "openai" },
        { predicate: "provider_profile", value: target.profile! },
        { predicate: "account_role", value: role },
        { predicate: "execution_eligible", value: String(executionEligible) },
      ],
      digest: "store-facts-digest",
    },
  };
}

test("allocates supported Luna, Terra, and Sol work only from Store-admitted execution accounts", async () => {
  const runtime = targets.map((target) => ({
    targetId: target.id, provider: "openai" as const, available: true, reason: "ready" as const,
  }));
  const rows = new Map([
    ["codex-apple", usage("codex-apple", 30)],
    ["codex-proton", usage("codex-proton", 98)],
    ["codex-gmail", usage("codex-gmail", 10)],
    ["codex-pm", usage("codex-pm", 0)],
  ]);
  const dependencies = {
    loadUsage: async (target: RoutingTarget) => rows.get(target.id),
    readAuthority: async (target: RoutingTarget) => authority(
      target,
      target.id === "codex-pm" ? "oversight" : "execution",
    ),
  };

  await expect(allocateCodexExecutionAccount(targets, runtime, "economy", "low", dependencies))
    .resolves.toMatchObject({ target: { id: "codex-gmail" }, model: "gpt-5.6-sol", effort: "low",
      receipt: { accountAuthority: { subject: "@account:codex-gmail" } } });
  await expect(allocateCodexExecutionAccount(targets, runtime, "standard", "medium", dependencies))
    .resolves.toMatchObject({ target: { id: "codex-gmail" }, model: "gpt-5.6-sol", effort: "medium" });
  await expect(allocateCodexExecutionAccount(targets, runtime, "senior", "high", dependencies))
    .resolves.toMatchObject({ target: { id: "codex-gmail" }, model: "gpt-5.6-sol", effort: "high" });
});

test("rejects Store role and eligibility conflicts", async () => {
  const target = targets.find(({ id }) => id === "codex-pm")!;
  const facts = [
    ["kind", "provider_account"], ["account_id", target.id], ["provider", "openai"],
    ["provider_profile", target.profile!], ["account_role", "oversight"], ["execution_eligible", "true"],
  ] as const;
  const client = {
    scanAll: async () => ({
      rows: facts.map(([predicate, value]) => new StoreTriple(`@account:${target.id}`, predicate, value)),
      servedVersion: 7, pages: 1, attempts: 1,
    }),
    close: () => {},
  };
  await expect(readCodexAccountAuthority(target, { client })).resolves.toBeUndefined();
});
