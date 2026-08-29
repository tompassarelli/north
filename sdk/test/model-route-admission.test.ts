import { afterAll, expect, test } from "bun:test";
import { watch } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { refreshAccountUsages } from "../src/account-usage";
import {
  collectExecutionModelRefreshAttempts,
  selectProviderFromAvailability,
  selectProviderForExecution,
} from "../src/provider-routing";
import {
  modelAdmissionReceipt,
  PROVIDER_MODEL_OBSERVATION_TTL_MS,
  readProviderModelObservations,
  type ProviderModelObservation,
  validateModelAdmissionReceipt,
  writeProviderModelObservation,
} from "../src/provider-model-observation-store";
import type { StoreObservationSnapshot } from "../src/store-observation-adapter";
import { ProviderRefreshCancelledError } from "../src/provider-cancellation";
import { acquireFileLease } from "../src/file-lease";
import {
  canonicalWriteModel,
  providerSupportsRoute,
  resolveModelAlias,
  resolveTier,
} from "../src/providers/catalog";
import { agentRouteFacts } from "../src/identity";
import { normalizeAnthropicSupportedModels } from "../src/providers/anthropic-models";
import { normalizeCodexSupportedModels } from "../src/providers/codex-models";
import type { CodexAccountAuthority } from "../src/accounts";
import { anthropicProvider } from "../src/providers/anthropic";
import {
  canonicalHarnessModelAvailability,
  harnessOptions,
} from "../src/harness";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import { ProviderRetrySafeError } from "../src/providers/types";
import type { StartAnthropicControl } from "../src/providers/anthropic-control";
import type {
  ProviderAvailability,
  ProviderUsageObservation,
  ResourcePolicy,
  RoutingTarget,
} from "../src/providers/types";

const roots: string[] = [];
afterAll(async () => Promise.all(roots.map((root) => rm(root, { recursive: true, force: true }))));

const now = new Date("2026-07-20T10:00:00.000Z");
const anthropic: RoutingTarget = {
  id: "claude-personal", provider: "anthropic", authMode: "ambient",
};
const isolated: RoutingTarget = {
  id: "claude-work", provider: "anthropic", authMode: "isolated", profile: "work",
};
const openai: RoutingTarget = {
  id: "codex-personal", provider: "openai", authMode: "isolated", profile: "personal",
};
const openaiWork: RoutingTarget = {
  id: "codex-work", provider: "openai", authMode: "isolated", profile: "work",
};
const availability: ProviderAvailability[] = [
  { targetId: anthropic.id, provider: "anthropic", available: true, reason: "ready" },
  { targetId: isolated.id, provider: "anthropic", available: true, reason: "ready" },
  { targetId: openai.id, provider: "openai", available: true, reason: "ready" },
];

function policy(targets: RoutingTarget[] = [anthropic, openai]): ResourcePolicy {
  return {
    mode: "preferential",
    providerOrder: ["anthropic", "openai"],
    targets,
    targetOrder: targets.map(({ id }) => id),
    targetPressures: Object.fromEntries(targets.map(({ id }) => [id, "normal" as const])),
  };
}

function observed(target: RoutingTarget, values: string[]) {
  return target.provider === "anthropic"
    ? normalizeAnthropicSupportedModels(values.map((value) => ({ value })), target, now)
    : normalizeCodexSupportedModels(values, target, now);
}

function codexAuthority(
  target: RoutingTarget,
  role: "execution" | "oversight" = "execution",
): CodexAccountAuthority {
  const executionEligible = role === "execution";
  return Object.freeze({
    role,
    executionEligible,
    receipt: Object.freeze({
      version: "north:codex-account-authority:v1" as const,
      subject: `@account:${target.id}`,
      servedVersion: 41,
      facts: Object.freeze([
        { predicate: "kind" as const, value: "provider_account" },
        { predicate: "account_id" as const, value: target.id },
        { predicate: "provider" as const, value: "openai" },
        { predicate: "provider_profile" as const, value: target.profile! },
        { predicate: "account_role" as const, value: role },
        { predicate: "execution_eligible" as const, value: executionEligible ? "true" : "false" },
      ]),
      digest: "a".repeat(64),
    }),
  });
}

function codexUsage(target: RoutingTarget): StoreObservationSnapshot<ProviderUsageObservation> {
  return Object.freeze({
    observation: Object.freeze({
      targetId: target.id,
      provider: "openai" as const,
      source: "codex-app-server:account-rate-limits" as const,
      observedAt: new Date().toISOString(),
      windows: Object.freeze([Object.freeze({
        limitId: "codex:primary",
        usedPercent: 12,
        resetsAt: "2099-01-01T00:00:00.000Z",
      })]),
    }),
    receipt: Object.freeze({
      version: "north:provider-observation:v1" as const,
      subject: `@provider-observation:usage:${target.id}`,
      digest: "b".repeat(64),
      servedVersion: 42,
    }),
  });
}

function modelSnapshot(observation: ProviderModelObservation): StoreObservationSnapshot<ProviderModelObservation> {
  return Object.freeze({
    observation,
    receipt: Object.freeze({
      version: "north:provider-observation:v1" as const,
      subject: `@provider-observation:model:${observation.targetId}`,
      digest: "c".repeat(64),
      servedVersion: 43,
    }),
  });
}

function usageResponse() {
  return {
    subscription_type: "max", rate_limits_available: true,
    rate_limits: {
      five_hour: { utilization: 12, resets_at: "2099-01-01T00:00:00Z" },
    },
  };
}

interface ControlCounters {
  startups: number;
  queries: number;
  usage: number;
  models: number;
  prompts: number;
  promptSettled?: Promise<void>;
}

function successfulControl(counters?: ControlCounters): StartAnthropicControl {
  return async () => {
    if (counters) counters.startups++;
    return {
      query(prompt) {
        if (counters) {
          counters.queries++;
          counters.promptSettled = (async () => {
            for await (const _message of prompt) counters.prompts++;
          })();
        }
        return {
          async usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET() {
            if (counters) counters.usage++;
            return usageResponse();
          },
          async supportedModels() {
            if (counters) counters.models++;
            return [{ value: "fable" }];
          },
          close() {},
        };
      },
      close() {},
    };
  };
}

function nextLeaseContender(directory: string, lockPath: string): {
  seen: Promise<void>;
  close(): void;
} {
  const prefix = `${basename(lockPath)}.candidate.`;
  let close = () => {};
  const seen = new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      close();
      reject(new Error(`timed out waiting for lease contender ${prefix}`));
    }, 2_000);
    const watcher = watch(directory, (_event, filename) => {
      if (!filename?.toString().startsWith(prefix)) return;
      clearTimeout(timeout);
      watcher.close();
      resolve();
    });
    close = () => {
      clearTimeout(timeout);
      watcher.close();
    };
  });
  return { seen, close };
}

test("Orchestration exact routes do not cross-product tier defaults", () => {
  expect(resolveTier("anthropic", "frontier", "fable", "xhigh")).toEqual({
    tier: "frontier", model: "claude-fable-5", effort: "xhigh",
  });
  expect(resolveTier("anthropic", "frontier", "fable", "high")).toEqual({
    tier: "frontier", model: "claude-fable-5", effort: "high",
  });
  expect(() => resolveTier("anthropic", "frontier", "fable", "max"))
    .toThrow("does not support reasoning max at semantic tier frontier");
  expect(providerSupportsRoute("anthropic", "frontier", "high", "fable")).toBe(true);
  expect(providerSupportsRoute("anthropic", "frontier", "xhigh", "fable")).toBe(true);
  expect(resolveModelAlias("anthropic", "__proto__")).toBe("__proto__");
  expect(() => resolveTier("anthropic", "frontier", "__proto__", "xhigh"))
    .toThrow("does not declare model __proto__");
});

test("canonicalWriteModel resolves aliases, guards cross-provider phantoms, and never crashes", () => {
  // Bare family alias -> concrete catalog id. A `model` fact must never be a raw
  // tier name (opus/sonnet/fable/luna/terra/sol).
  expect(canonicalWriteModel("anthropic", "opus")).toBe("claude-opus-5");
  expect(canonicalWriteModel("anthropic", "sonnet")).toBe("claude-sonnet-5");
  expect(canonicalWriteModel("openai", "sol")).toBe("gpt-5.6-sol");
  // Already-concrete id passes through unchanged.
  expect(canonicalWriteModel("anthropic", "claude-opus-4-8")).toBe("claude-opus-4-8");
  // Cross-provider phantom (fallback-death lag, e.g. lane-mrtcfwgj: a gpt-* id
  // recorded against provider=anthropic) writes NO model rather than the stale
  // routed one.
  expect(canonicalWriteModel("anthropic", "gpt-5.6-sol")).toBeUndefined();
  expect(canonicalWriteModel("openai", "claude-opus-4-8")).toBeUndefined();
  // Missing provider or model -> no fact.
  expect(canonicalWriteModel(undefined, "opus")).toBeUndefined();
  expect(canonicalWriteModel("anthropic", undefined)).toBeUndefined();
  // A provider North has no catalog for (native/interactive session) cannot be
  // canonicalized, but must not crash the write nor drop the datum.
  expect(canonicalWriteModel("google" as any, "gemini-3-pro")).toBe("gemini-3-pro");
});

test("lane-identity model facts are canonicalized at write, not left as bare aliases", () => {
  // agentRouteFacts is the second durable model-fact write path; a tier name as
  // spawned must land as a concrete id, matching the @run write path.
  const facts = Object.fromEntries(
    agentRouteFacts("agent-x", { kind: "lane", provider: "anthropic", model: "opus" }),
  );
  expect(facts.model).toBe("claude-opus-5");
  // Cross-provider phantom drops the model fact on the identity path too.
  const phantom = Object.fromEntries(
    agentRouteFacts("agent-y", { kind: "lane", provider: "anthropic", model: "gpt-5.6-sol" }),
  );
  expect(phantom.model).toBeUndefined();
});

test("unpinned canonical tier defaults are static, while an explicit pin to the same model needs live evidence", () => {
  const onlyAnthropic = policy([anthropic]);
  expect(selectProviderFromAvailability(
    { target: anthropic.id }, availability, onlyAnthropic,
    "frontier", "canonical", "xhigh",
  ).provider).toBe("anthropic");
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, onlyAnthropic,
    "frontier", "explicit-default", "xhigh", "claude-fable-5",
  )).toThrow("lacks fresh positive exact-model availability evidence");
});

test("explicit Fable selection needs fresh target-scoped positive membership", () => {
  const store = { version: 1 as const, observations: [observed(anthropic, ["fable"])] };
  const decision = selectProviderFromAvailability(
    { target: anthropic.id }, availability, policy([anthropic]),
    "frontier", "fable", "xhigh", "fable", undefined,
    { store, now },
  );
  expect(decision.provider).toBe("anthropic");
  expect(decision.modelAvailabilityReceipts?.[anthropic.id]).toMatchObject({
    targetId: anthropic.id, model: "claude-fable-5",
    source: "claude-agent-sdk:Query.supportedModels",
  });
  expect(decision.selectionReason).toContain("model-evidence=claude-agent-sdk:Query.supportedModels@");
  expect(Object.isFrozen(decision.modelAvailabilityReceipts)).toBe(true);
  expect(Object.isFrozen(decision.modelAvailabilityRequiredTargets)).toBe(true);
  expect(Object.isFrozen(decision.routingTargets)).toBe(true);
  expect(() => (decision as any).modelAvailabilityRequiredTargets = []).toThrow();
  expect(() => delete (decision.modelAvailabilityReceipts as any)[anthropic.id]).toThrow();
});

test("explicit Codex pins need fresh selected-target membership while unpinned defaults stay static", () => {
  const onlyOpenAI = policy([openai]);
  expect(selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-default", "xhigh",
  )).toMatchObject({ provider: "openai", target: openai.id });
  expect(() => selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-pin-missing", "xhigh", "gpt-5.6-sol",
  )).toThrow("lacks fresh positive exact-model availability evidence");

  const positive = observed(openai, ["gpt-5.6-sol"]);
  const decision = selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-pin", "xhigh", "gpt-5.6-sol", undefined,
    { store: { version: 1, observations: [positive] }, now },
  );
  expect(decision.modelAvailabilityReceipts?.[openai.id]).toMatchObject({
    provider: "openai",
    targetId: openai.id,
    model: "gpt-5.6-sol",
    source: "codex-app-server:model-list",
  });
  expect(() => selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-wrong-profile", "xhigh", "gpt-5.6-sol", undefined,
    { store: { version: 1, observations: [observed(openaiWork, ["gpt-5.6-sol"])] }, now },
  )).toThrow("lacks fresh positive exact-model availability evidence");
  expect(() => selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-empty", "xhigh", "gpt-5.6-sol", undefined,
    { store: { version: 1, observations: [observed(openai, [])] }, now },
  )).toThrow("lacks fresh positive exact-model availability evidence");
  const staleAt = new Date(now.getTime() - PROVIDER_MODEL_OBSERVATION_TTL_MS - 1);
  expect(() => selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-stale", "xhigh", "gpt-5.6-sol", undefined,
    {
      store: { version: 1, observations: [
        normalizeCodexSupportedModels(["gpt-5.6-sol"], openai, staleAt),
      ] },
      now,
    },
  )).toThrow("lacks fresh positive exact-model availability evidence");
  expect(() => selectProviderFromAvailability(
    { target: openai.id }, availability, onlyOpenAI,
    "frontier", "codex-failed", "xhigh", "gpt-5.6-sol", undefined,
    {
      store: { version: 1, observations: [positive] },
      currentAttempts: [{
        status: "unavailable",
        targetId: openai.id,
        attemptedAt: now.toISOString(),
        reason: "codex_models_refresh_unavailable",
      }],
      now,
    },
  )).toThrow("lacks fresh positive exact-model availability evidence");
});

test("fresh empty, wrong target/auth, and usage windows never prove exact-model availability", () => {
  const onlyAnthropic = policy([anthropic]);
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, onlyAnthropic,
    "frontier", "empty", "xhigh", "fable", undefined,
    { store: { version: 1, observations: [observed(anthropic, [])] }, now },
  )).toThrow("lacks fresh positive");
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, onlyAnthropic,
    "frontier", "wrong-target", "xhigh", "fable", undefined,
    { store: { version: 1, observations: [observed(isolated, ["fable"])] }, now },
  )).toThrow("lacks fresh positive");

  const usageOnly = policy([anthropic]);
  usageOnly.automatedPressureObservationSets = {
    [anthropic.id]: [{
      targetId: anthropic.id, provider: "anthropic",
      source: "claude-agent-sdk:usage-control-experimental",
      observedAt: now.toISOString(),
      windows: [{ limitId: "claude:model:fable", usedPercent: 1, resetsAt: "2099-01-01T00:00:00Z" }],
    }],
  };
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, usageOnly,
    "frontier", "usage-is-not-availability", "xhigh", "fable",
  )).toThrow("lacks fresh positive");
});

test("a current failed persistence attempt shadows an older fresh positive", () => {
  const positive = observed(anthropic, ["fable"]);
  expect(modelAdmissionReceipt(positive, anthropic, "claude-fable-5", now)).toBeDefined();
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, policy([anthropic]),
    "frontier", "failed-write", "xhigh", "fable", undefined,
    {
      store: { version: 1, observations: [positive] }, now,
      currentAttempts: [{
        status: "unavailable", targetId: anthropic.id,
        attemptedAt: now.toISOString(),
        reason: "anthropic_models_observation_store_unavailable",
      }],
    },
  )).toThrow("lacks fresh positive");
});

test("outer account-usage lease failure emits an unavailable current model attempt", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-model-lease-failure-"));
  roots.push(root);
  const modelStorePath = join(root, "models.json");
  await writeProviderModelObservation(observed(anthropic, ["fable"]), modelStorePath, now);
  const [report] = await refreshAccountUsages({
    accounts: [anthropic], observeAnthropicModels: true, force: true, now,
    // /proc cannot host the usage-store lease, so control startup is never reached.
    storePath: `/proc/north-model-lease-failure-${process.pid}/usage.json`,
    modelStorePath,
  });
  expect(report.modelAvailabilityAttempt).toMatchObject({
    status: "unavailable", targetId: anthropic.id,
    reason: "anthropic_models_refresh_unavailable",
  });
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, policy([anthropic]),
    "frontier", "lease-failure", "xhigh", "fable", undefined,
    {
      store: { version: 1, observations: [observed(anthropic, ["fable"])] }, now,
      currentAttempts: report.modelAvailabilityAttempt ? [report.modelAvailabilityAttempt] : [],
    },
  )).toThrow("lacks fresh positive");
});

test("aggregate refresh rejection shadows prior positives for every required target", async () => {
  const attempts = await collectExecutionModelRefreshAttempts(
    [anthropic, openai],
    [anthropic],
    "auto",
    async () => { throw new Error("PRIVATE AGGREGATE FAILURE"); },
  );
  expect(attempts).toEqual([{
    status: "unavailable",
    targetId: anthropic.id,
    attemptedAt: expect.any(String),
    reason: "anthropic_models_refresh_unavailable",
  }]);
  expect(JSON.stringify(attempts)).not.toContain("PRIVATE");
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, policy([anthropic]),
    "frontier", "aggregate-failure", "xhigh", "fable", undefined,
    {
      store: { version: 1, observations: [observed(anthropic, ["fable"])] }, now,
      currentAttempts: attempts,
    },
  )).toThrow("lacks fresh positive");
});

test("execution selector owns one exact-target Anthropic refresh and one warm Query", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-model-shared-control-"));
  roots.push(root);
  const observedAt = new Date();
  const counters: ControlCounters = {
    startups: 0, queries: 0, usage: 0, models: 0, prompts: 0,
  };
  let refreshes = 0;
  const decision = await selectProviderForExecution(
    { target: anthropic.id },
    policy([anthropic]),
    {
      tier: "frontier", reasoning: "xhigh", model: "fable",
      stableKey: "one-anthropic-refresh",
    },
    {
      probeAnthropic: () => ({
        targetId: anthropic.id, provider: "anthropic", available: true, reason: "ready",
      }),
      refreshAccountUsages: async (options) => {
        refreshes++;
        return refreshAccountUsages({
          ...options,
          force: true,
          now: observedAt,
          storePath: join(root, "usage.json"),
          modelStorePath: join(root, "models.json"),
          startAnthropicControl: successfulControl(counters),
        });
      },
    },
  );
  await counters.promptSettled;
  expect(refreshes).toBe(1);
  expect(counters).toMatchObject({
    startups: 1, queries: 1, usage: 1, models: 1, prompts: 0,
  });
  expect(decision).toMatchObject({
    provider: "anthropic",
    target: anthropic.id,
    modelAvailabilityReceipts: {
      [anthropic.id]: { model: "claude-fable-5" },
    },
  });
});

test("execution ignores JSON-only model evidence and admits only a Store snapshot", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-store-model-routing-"));
  roots.push(root);
  const path = join(root, "models.json");
  const positive = normalizeAnthropicSupportedModels([{ value: "fable" }], anthropic, new Date());
  await writeFile(path, `${JSON.stringify({ version: 1, observations: [positive] })}\n`);
  const saved = process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS;
  process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = path;
  const context = { tier: "frontier" as const, reasoning: "xhigh" as const, model: "fable" };
  const base = {
    probeAnthropic: () => ({
      targetId: anthropic.id, provider: "anthropic" as const, available: true, reason: "ready" as const,
    }),
    refreshAccountUsages: async () => [],
  };
  try {
    await expect(selectProviderForExecution({ target: anthropic.id }, policy([anthropic]), context, {
      ...base,
      loadProviderModelObservation: async () => undefined,
    })).rejects.toThrow("lacks fresh positive exact-model availability evidence");

    const snapshot: StoreObservationSnapshot<ProviderModelObservation> = Object.freeze({
      observation: positive,
      receipt: Object.freeze({
        version: "north:provider-observation:v1" as const,
        subject: "@provider-observation:model:test",
        digest: "a".repeat(64),
        servedVersion: 41,
      }),
    });
    await expect(selectProviderForExecution({ target: anthropic.id }, policy([anthropic]), context, {
      ...base,
      loadProviderModelObservation: async () => snapshot,
    })).resolves.toMatchObject({ provider: "anthropic", target: anthropic.id });
  } finally {
    if (saved === undefined) delete process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS;
    else process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = saved;
  }
});

test("fresh model collection failure blocks without probe storms and retries after TTL", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-model-negative-cache-"));
  roots.push(root);
  const observedAt = new Date();
  const usagePath = join(root, "usage.json");
  const modelPath = join(root, "models.json");
  const counters: ControlCounters = {
    startups: 0, queries: 0, usage: 0, models: 0, prompts: 0,
  };
  const failingModels: StartAnthropicControl = async () => {
    counters.startups++;
    return {
      query() {
        counters.queries++;
        return {
          async usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET() {
            counters.usage++;
            return usageResponse();
          },
          async supportedModels() {
            counters.models++;
            throw new Error("PRIVATE SUPPORTED MODELS FAILURE");
          },
          close() {},
        };
      },
      close() {},
    };
  };
  const refresh = (at: Date) => refreshAccountUsages({
    accounts: [anthropic],
    observeAnthropicModels: true,
    now: at,
    storePath: usagePath,
    modelStorePath: modelPath,
    startAnthropicControl: failingModels,
  });

  const [first] = await refresh(observedAt);
  expect(first).toMatchObject({
    status: "observed",
    modelAvailabilityAttempt: {
      status: "persisted",
      observation: { collectionFailure: { reason: "anthropic_models_probe_failed" } },
    },
  });
  const failedStore = await readProviderModelObservations(modelPath, observedAt);
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, policy([anthropic]),
    "frontier", "negative-cache", "xhigh", "fable", undefined,
    { store: failedStore, now: observedAt },
  )).toThrow("lacks fresh positive exact-model availability evidence");

  const [second] = await refresh(new Date(observedAt.getTime() + 1_000));
  expect(second.cached).toBe(true);
  expect(counters).toMatchObject({ startups: 1, queries: 1, usage: 1, models: 1 });

  await refresh(new Date(observedAt.getTime() + PROVIDER_MODEL_OBSERVATION_TTL_MS + 1));
  expect(counters).toMatchObject({ startups: 2, queries: 2, usage: 2, models: 2 });
});

test("account refresh persists Codex model authority and a later failure revokes it", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-codex-model-refresh-"));
  roots.push(root);
  const usagePath = join(root, "usage.json");
  const modelPath = join(root, "models.json");
  const positive = observed(openai, ["gpt-5.6-sol"]);
  const usageObservation = {
    targetId: openai.id,
    provider: "openai" as const,
    source: "codex-app-server:account-rate-limits" as const,
    observedAt: now.toISOString(),
    state: "normal" as const,
  };
  const [first] = await refreshAccountUsages({
    accounts: [openai],
    observeCodexModels: true,
    modelObservationTargetIds: [openai.id],
    force: true,
    now,
    storePath: usagePath,
    modelStorePath: modelPath,
    readCodexControl: async () => ({
      observation: usageObservation,
      models: { ok: true, observation: positive },
    }),
  });
  expect(first.modelAvailabilityAttempt).toEqual({
    status: "persisted", observation: positive,
  });
  const receipt = modelAdmissionReceipt(positive, openai, "gpt-5.6-sol", now)!;
  expect(await validateModelAdmissionReceipt(
    receipt, openai, "gpt-5.6-sol", modelPath, now,
  )).toBe(true);

  const failedAt = new Date(now.getTime() + 1_000);
  const [second] = await refreshAccountUsages({
    accounts: [openai],
    observeCodexModels: true,
    modelObservationTargetIds: [openai.id],
    force: true,
    now: failedAt,
    storePath: usagePath,
    modelStorePath: modelPath,
    readCodexControl: async () => ({
      observation: { ...usageObservation, observedAt: failedAt.toISOString() },
      models: {
        ok: false,
        attemptedAt: failedAt.toISOString(),
        reason: "codex_models_probe_failed",
      },
    }),
  });
  expect(second.modelAvailabilityAttempt).toMatchObject({
    status: "persisted",
    observation: {
      targetId: openai.id,
      models: [],
      collectionFailure: { reason: "codex_models_probe_failed" },
    },
  });
  expect(await validateModelAdmissionReceipt(
    receipt, openai, "gpt-5.6-sol", modelPath, failedAt,
  )).toBe(false);
});

test("execution selector owns exactly one OpenAI account refresh", async () => {
  let refreshes = 0;
  let refreshedAccounts: unknown;
  const decision = await selectProviderForExecution(
    { target: openai.id },
    policy([openai]),
    { tier: "standard", reasoning: "medium", stableKey: "one-openai-refresh" },
    {
      probeOpenAI: () => ({
        targetId: openai.id, provider: "openai", available: true, reason: "ready",
      }),
      readCodexAccountAuthority: async (target) => codexAuthority(target),
      loadCodexUsage: async (target) => codexUsage(target),
      refreshAccountUsages: async (options) => {
        refreshes++;
        refreshedAccounts = options.accounts;
        return [];
      },
    },
  );
  expect(decision).toMatchObject({ provider: "openai", target: openai.id });
  expect(refreshes).toBe(1);
  expect(refreshedAccounts).toEqual([openai]);
});

test("complete explicit-human provider, account, and model pins bypass Store selection", async () => {
  let storeReads = 0;
  let refreshes = 0;
  const decision = await selectProviderForExecution(
    { provider: "openai", target: openai.id },
    policy([openai]),
    {
      tier: "frontier", reasoning: "xhigh", model: "gpt-5.6-sol",
      stableKey: "complete-human-pin",
      pinEvidence: {
        policyVersion: "north-routing-pin-v1",
        issuedAt: "2026-07-20T09:00:00.000Z", expiresAt: "2026-07-20T11:00:00.000Z",
        reasonCode: "explicit-human-request", detail: "operator-selected exact route",
        pins: [
          { kind: "provider", value: "openai" },
          { kind: "account", value: openai.id },
          { kind: "model", value: "gpt-5.6-sol" },
        ],
      },
    },
    {
      readCodexAccountAuthority: async () => { storeReads++; throw new Error("Store must not be read"); },
      loadCodexUsage: () => { storeReads++; throw new Error("Store must not be read"); },
      loadProviderModelObservation: async () => { storeReads++; throw new Error("Store must not be read"); },
      refreshAccountUsages: async () => { refreshes++; throw new Error("usage must not be refreshed"); },
    },
  );
  expect(decision).toMatchObject({ provider: "openai", target: openai.id });
  expect({ storeReads, refreshes }).toEqual({ storeReads: 0, refreshes: 0 });
});

test("partial explicit pins keep Store-backed execution selection fail-closed", async () => {
  let authorityReads = 0;
  await expect(selectProviderForExecution(
    { provider: "openai", target: openai.id },
    policy([openai]),
    {
      tier: "frontier", reasoning: "xhigh", model: "gpt-5.6-sol",
      pinEvidence: {
        policyVersion: "north-routing-pin-v1",
        issuedAt: "2026-07-20T09:00:00.000Z", expiresAt: "2026-07-20T11:00:00.000Z",
        reasonCode: "explicit-human-request", detail: "missing exact model pin",
        pins: [
          { kind: "provider", value: "openai" },
          { kind: "account", value: openai.id },
        ],
      },
    },
    {
      readCodexAccountAuthority: async () => {
        authorityReads++;
        throw new Error("truncated Store authority");
      },
    },
  )).rejects.toThrow("truncated Store authority");
  expect(authorityReads).toBe(1);
});

test("execution selector collects exact-model evidence only for the selected OpenAI target", async () => {
  let refreshOptions: Parameters<typeof refreshAccountUsages>[0] | undefined;
  let modelListObservations = 0;
  let selectedObservation: ProviderModelObservation | undefined;
  const decision = await selectProviderForExecution(
    "openai",
    policy([openai, openaiWork]),
    {
      tier: "frontier", reasoning: "xhigh", model: "gpt-5.6-sol",
      stableKey: "selected-codex-model-list",
    },
    {
      probeOpenAI: (target) => ({
        targetId: target?.id,
        provider: "openai",
        available: true,
        reason: "ready",
      }),
      readCodexAccountAuthority: async (target) => codexAuthority(target),
      loadCodexUsage: async (target) => codexUsage(target),
      refreshAccountUsages: async (options) => {
        refreshOptions = options;
        modelListObservations++;
        const observedAt = new Date();
        const modelObservation = normalizeCodexSupportedModels(
          ["gpt-5.6-sol"], openai, observedAt,
        );
        selectedObservation = modelObservation;
        return [{
          accountId: openai.id,
          provider: "openai",
          source: "codex-app-server:account-rate-limits",
          observedAt: observedAt.toISOString(),
          status: "observed",
          cached: false,
          observation: {
            targetId: openai.id,
            provider: "openai",
            source: "codex-app-server:account-rate-limits",
            observedAt: observedAt.toISOString(),
            state: "normal",
          },
          unavailableComponents: [],
          modelAvailabilityAttempt: { status: "persisted", observation: modelObservation },
        }];
      },
      loadProviderModelObservation: async (target) =>
        target.id === openai.id && selectedObservation
          ? modelSnapshot(selectedObservation)
          : undefined,
    },
  );
  expect(refreshOptions).toMatchObject({
    accounts: [openai, openaiWork],
    observeAnthropicModels: false,
    observeCodexModels: true,
    modelObservationTargetIds: [openai.id],
  });
  expect(decision).toMatchObject({
    provider: "openai",
    target: openai.id,
    modelAvailabilityRequiredTargets: [openai.id, openaiWork.id],
    modelAvailabilityReceipts: {
      [openai.id]: { model: "gpt-5.6-sol" },
    },
  });
  expect(modelListObservations).toBe(1);
});

test("selected OpenAI exact-model admission stays blocked on missing, stale, or failed evidence", async () => {
  const targetPolicy = policy([openai]);
  const context = {
    tier: "frontier" as const,
    reasoning: "xhigh" as const,
    model: "gpt-5.6-sol",
    stableKey: "codex-model-fail-closed",
  };
  const dependencies = {
    readCodexAccountAuthority: async (target: RoutingTarget) => codexAuthority(target),
    loadCodexUsage: async (target: RoutingTarget) => codexUsage(target),
  };
  const blocked = { kind: "blocked_preflight", preSideEffect: true, processOutcome: "blocked_preflight" };

  await expect(selectProviderForExecution(
    { target: openai.id }, targetPolicy, context,
    { ...dependencies, refreshAccountUsages: async () => [], loadProviderModelObservation: async () => undefined },
  )).rejects.toMatchObject(blocked);

  const stale = normalizeCodexSupportedModels(
    ["gpt-5.6-sol"], openai,
    new Date(Date.now() - PROVIDER_MODEL_OBSERVATION_TTL_MS - 1),
  );
  await expect(selectProviderForExecution(
    { target: openai.id }, targetPolicy, context,
    {
      ...dependencies,
      refreshAccountUsages: async () => [],
      loadProviderModelObservation: async () => modelSnapshot(stale),
    },
  )).rejects.toMatchObject(blocked);

  const positive = normalizeCodexSupportedModels(["gpt-5.6-sol"], openai, new Date());
  await expect(selectProviderForExecution(
    { target: openai.id }, targetPolicy, context,
    {
      ...dependencies,
      refreshAccountUsages: async () => [{
        accountId: openai.id,
        provider: "openai",
        source: "codex-app-server:account-rate-limits",
        observedAt: new Date().toISOString(),
        status: "unavailable",
        cached: false,
        observation: codexUsage(openai).observation,
        unavailableComponents: [],
        modelAvailabilityAttempt: {
          status: "unavailable",
          targetId: openai.id,
          attemptedAt: new Date().toISOString(),
          reason: "codex_models_refresh_unavailable",
        },
      }],
      loadProviderModelObservation: async () => modelSnapshot(positive),
    },
  )).rejects.toMatchObject(blocked);
});

test("Store oversight role never enters OpenAI preliminary execution eligibility", async () => {
  let refreshes = 0;
  await expect(selectProviderForExecution(
    { target: openai.id }, policy([openai]),
    { tier: "standard", reasoning: "medium", stableKey: "oversight-exclusion" },
    {
      readCodexAccountAuthority: async (target) => codexAuthority(target, "oversight"),
      loadCodexUsage: async (target) => codexUsage(target),
      refreshAccountUsages: async () => { refreshes++; return []; },
    },
  )).rejects.toMatchObject({ kind: "provider_unavailable", preSideEffect: true });
  expect(refreshes).toBe(0);
});

test("selector cancellation is pre-side-effect and cannot return a fallback route", async () => {
  const before = new AbortController();
  before.abort(new Error("PRIVATE PRE-SELECTION ABORT"));
  let probes = 0;
  let refreshes = 0;
  await expect(selectProviderForExecution(
    "auto", policy(), { tier: "standard", reasoning: "medium", signal: before.signal },
    {
      probeAnthropic: () => { probes++; return { provider: "anthropic", available: true, reason: "ready" }; },
      probeOpenAI: () => { probes++; return { provider: "openai", available: true, reason: "ready" }; },
      refreshAccountUsages: async () => { refreshes++; return []; },
    },
  )).rejects.toBeInstanceOf(ProviderRefreshCancelledError);
  expect({ probes, refreshes }).toEqual({ probes: 0, refreshes: 0 });

  const during = new AbortController();
  await expect(selectProviderForExecution(
    { target: openai.id }, policy([openai]),
    { tier: "standard", reasoning: "medium", signal: during.signal },
    {
      probeOpenAI: () => ({ provider: "openai", available: true, reason: "ready" }),
      readCodexAccountAuthority: async (target) => codexAuthority(target),
      loadCodexUsage: async (target) => codexUsage(target),
      refreshAccountUsages: async () => {
        during.abort(new Error("PRIVATE POST-REFRESH ABORT"));
        return [];
      },
    },
  )).rejects.toMatchObject({
    code: "NORTH_PROVIDER_REFRESH_CANCELLED", preSideEffect: true,
  });
});

test("cancelled refresh is run-local and the next run probes immediately", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-model-cancelled-refresh-"));
  roots.push(root);
  const usagePath = join(root, "usage.json");
  const modelPath = join(root, "models.json");
  const staleAt = new Date(now.getTime() - 5 * 60 * 1000 - 1);
  await writeProviderModelObservation(
    normalizeAnthropicSupportedModels([{ value: "fable" }], anthropic, staleAt),
    modelPath,
    now,
  );
  const before = await readFile(modelPath, "utf8");
  const cancelled = new AbortController();
  cancelled.abort(new Error("PRIVATE CANCELLED REFRESH"));
  let cancelledStartups = 0;
  await expect(refreshAccountUsages({
    accounts: [anthropic],
    observeAnthropicModels: true,
    now,
    storePath: usagePath,
    modelStorePath: modelPath,
    signal: cancelled.signal,
    startAnthropicControl: async () => {
      cancelledStartups++;
      return await new Promise<never>(() => {});
    },
  })).rejects.toBeInstanceOf(ProviderRefreshCancelledError);
  expect(cancelledStartups).toBe(0);
  expect(await readFile(modelPath, "utf8")).toBe(before);
  await expect(readFile(usagePath, "utf8")).rejects.toMatchObject({ code: "ENOENT" });

  const during = new AbortController();
  let inFlightStartups = 0;
  let controlCalls = 0;
  const never = () => {
    controlCalls++;
    if (controlCalls === 2)
      queueMicrotask(() => during.abort(new Error("PRIVATE IN-FLIGHT CANCEL")));
    return new Promise<never>(() => {});
  };
  await expect(refreshAccountUsages({
    accounts: [anthropic],
    observeAnthropicModels: true,
    now,
    storePath: usagePath,
    modelStorePath: modelPath,
    signal: during.signal,
    startAnthropicControl: async () => {
      inFlightStartups++;
      return {
        query() {
          return {
            usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET: never,
            supportedModels: never,
            close() {},
          };
        },
        close() {},
      };
    },
  })).rejects.toBeInstanceOf(ProviderRefreshCancelledError);
  expect({ inFlightStartups, controlCalls }).toEqual({ inFlightStartups: 1, controlCalls: 2 });
  expect(await readFile(modelPath, "utf8")).toBe(before);
  await expect(readFile(usagePath, "utf8")).rejects.toMatchObject({ code: "ENOENT" });

  const counters: ControlCounters = {
    startups: 0, queries: 0, usage: 0, models: 0, prompts: 0,
  };
  const [next] = await refreshAccountUsages({
    accounts: [anthropic],
    observeAnthropicModels: true,
    now,
    storePath: usagePath,
    modelStorePath: modelPath,
    startAnthropicControl: successfulControl(counters),
  });
  await counters.promptSettled;
  expect(counters.startups).toBe(1);
  expect(next.status).toBe("observed");
  expect((await readProviderModelObservations(modelPath, now))?.observations[0].collectionFailure)
    .toBeUndefined();
});

test("cancellation while model persistence waits commits neither positive nor failed observations", async () => {
  for (const response of ["positive", "invalid"] as const) {
    const root = await mkdtemp(join(tmpdir(), `north-model-cancelled-write-${response}-`));
    roots.push(root);
    const usagePath = join(root, "usage.json");
    const modelPath = join(root, "models.json");
    const held = await acquireFileLease(`${modelPath}.lock`);
    const contender = nextLeaseContender(root, `${modelPath}.lock`);
    const controller = new AbortController();
    const refresh = refreshAccountUsages({
      accounts: [anthropic],
      observeAnthropicModels: true,
      force: true,
      now,
      storePath: usagePath,
      modelStorePath: modelPath,
      signal: controller.signal,
      startAnthropicControl: async () => ({
        query(prompt) {
          void (async () => { for await (const _message of prompt) { /* no prompt turns */ } })();
          return {
            async usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET() {
              return usageResponse();
            },
            async supportedModels() {
              return response === "positive" ? [{ value: "fable" }] : [{ label: "not-a-model" }] as any;
            },
            close() {},
          };
        },
        close() {},
      }),
    });
    try {
      await contender.seen;
      controller.abort(new Error(`PRIVATE ${response.toUpperCase()} WRITE CANCEL`));
      await held.release();
      await expect(refresh).rejects.toBeInstanceOf(ProviderRefreshCancelledError);
    } finally {
      contender.close();
      await held.release();
    }
    await expect(readFile(modelPath, "utf8")).rejects.toMatchObject({ code: "ENOENT" });
    await expect(readFile(usagePath, "utf8")).rejects.toMatchObject({ code: "ENOENT" });
  }
});

test("cancellation while usage persistence waits commits neither successful nor failed observations", async () => {
  for (const response of ["observed", "failed"] as const) {
    const root = await mkdtemp(join(tmpdir(), `north-usage-cancelled-write-${response}-`));
    roots.push(root);
    const usagePath = join(root, "usage.json");
    const held = await acquireFileLease(`${usagePath}.lock`);
    const contender = nextLeaseContender(root, `${usagePath}.lock`);
    const controller = new AbortController();
    const refresh = refreshAccountUsages({
      accounts: [anthropic],
      force: true,
      now,
      storePath: usagePath,
      signal: controller.signal,
      readAnthropic: async () => {
        if (response === "failed") throw new Error("PRIVATE USAGE COLLECTION FAILURE");
        return {
          source: "claude-agent-sdk:usage-control-experimental",
          observation: {
            targetId: anthropic.id,
            provider: "anthropic",
            source: "claude-agent-sdk:usage-control-experimental",
            observedAt: now.toISOString(),
            windows: [{ usedPercent: 12, resetsAt: "2099-01-01T00:00:00.000Z" }],
          },
          unavailableComponents: [],
        };
      },
    });
    try {
      await contender.seen;
      controller.abort(new Error(`PRIVATE ${response.toUpperCase()} USAGE WRITE CANCEL`));
      await held.release();
      await expect(refresh).rejects.toBeInstanceOf(ProviderRefreshCancelledError);
    } finally {
      contender.close();
      await held.release();
    }
    await expect(readFile(usagePath, "utf8")).rejects.toMatchObject({ code: "ENOENT" });
  }
});

test("lifecycle settlement failure becomes explicit unavailable model evidence", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-model-lifecycle-failure-"));
  roots.push(root);
  const observedAt = new Date();
  const usagePath = join(root, "usage.json");
  const modelPath = join(root, "models.json");
  await writeProviderModelObservation(
    normalizeAnthropicSupportedModels([{ value: "fable" }], anthropic, observedAt),
    modelPath,
    observedAt,
  );
  const counters: ControlCounters = {
    startups: 0, queries: 0, usage: 0, models: 0, prompts: 0,
  };
  const [report] = await refreshAccountUsages({
    accounts: [anthropic],
    observeAnthropicModels: true,
    force: true,
    now: observedAt,
    storePath: usagePath,
    modelStorePath: modelPath,
    startAnthropicControl: successfulControl(counters),
    createAnthropicControlLifecycle: () => ({
      spawnClaudeCodeProcess: () => { throw new Error("not used"); },
      settle: async () => { throw new Error("PRIVATE REAP FAILURE"); },
      forceKill() {},
      started: () => false,
    }),
  });
  expect(report).toMatchObject({
    status: "unavailable",
    modelAvailabilityAttempt: {
      status: "persisted",
      observation: {
        targetId: anthropic.id,
        models: [],
        collectionFailure: { reason: "anthropic_models_probe_failed" },
      },
    },
  });
  expect(JSON.stringify(report)).not.toContain("PRIVATE");
  const failedStore = await readProviderModelObservations(modelPath, observedAt);
  expect(failedStore?.observations[0]).toMatchObject({
    models: [], collectionFailure: { reason: "anthropic_models_probe_failed" },
  });
  expect(() => selectProviderFromAvailability(
    { target: anthropic.id }, availability, policy([anthropic]),
    "frontier", "no-resurrection", "xhigh", "fable", undefined,
    { store: failedStore, now: observedAt },
  )).toThrow("lacks fresh positive exact-model availability evidence");

  const [cached] = await refreshAccountUsages({
    accounts: [anthropic],
    observeAnthropicModels: true,
    now: new Date(observedAt.getTime() + 1_000),
    storePath: usagePath,
    modelStorePath: modelPath,
    startAnthropicControl: successfulControl(counters),
  });
  expect(cached.cached).toBe(true);
  expect(counters.startups).toBe(1);
});

test("direct managed exact-model adapter calls cannot bypass sealed availability authority", async () => {
  const direct = harnessOptions({
    self: "direct-exact-model-admission",
    provider: "anthropic",
    model: "claude-fable-5",
    effort: "xhigh",
    routingMetadata: applyOrchestrationStaffing({ role: "designer" }),
    presenceRegistrar: false,
  }) as any;
  expect(canonicalHarnessModelAvailability(direct, "anthropic")).toMatchObject({
    required: true, targetId: "anthropic", model: "claude-fable-5",
  });
  await expect(anthropicProvider.admit!({ options: direct, target: anthropic }))
    .rejects.toThrow("anthropic_model_availability_unproven");
  expect(() => { direct.northModelAvailability.required = false; }).toThrow();
  const replaced = { ...direct, northModelAvailability: {
    ...direct.northModelAvailability, required: false,
  } };
  let caught: unknown;
  try { await anthropicProvider.admit!({ options: replaced, target: anthropic }); }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ProviderRetrySafeError);
  expect((caught as Error).message).toBe("anthropic_harness_authority_seal_missing");
});

test("Anthropic adapter revalidates a sealed receipt and rejects TOCTOU revocation", async () => {
  const root = await mkdtemp(join(tmpdir(), "north-model-toctou-"));
  roots.push(root);
  const path = join(root, "models.json");
  const copiedPositivePath = join(root, "copied-old-positive.json");
  const observedAt = new Date();
  const positive = normalizeAnthropicSupportedModels([{ value: "fable" }], anthropic, observedAt);
  await writeProviderModelObservation(positive, path, observedAt);
  await writeProviderModelObservation(positive, copiedPositivePath, observedAt);
  const receipt = modelAdmissionReceipt(positive, anthropic, "claude-fable-5", observedAt)!;
  const saved = process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS;
  process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = path;
  try {
    const options = harnessOptions({
      self: "exact-model-toctou",
      provider: "anthropic",
      model: "claude-fable-5",
      effort: "xhigh",
      modelAvailability: {
        exactModelPinned: true,
        targetId: anthropic.id,
        receipt,
      },
      routingMetadata: applyOrchestrationStaffing({ role: "designer" }),
      presenceRegistrar: false,
    });
    expect(canonicalHarnessModelAvailability(options, "anthropic")?.observationPath)
      .toBe(path);
    await writeProviderModelObservation(
      normalizeAnthropicSupportedModels([], anthropic, observedAt), path, observedAt,
    );
    expect(() => {
      (options.env as any).NORTH_PROVIDER_MODEL_OBSERVATIONS = copiedPositivePath;
    }).toThrow();
    await expect(anthropicProvider.admit!({ options, target: anthropic }))
      .rejects.toThrow("anthropic_model_availability_unproven");
  } finally {
    if (saved === undefined) delete process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS;
    else process.env.NORTH_PROVIDER_MODEL_OBSERVATIONS = saved;
  }
});
