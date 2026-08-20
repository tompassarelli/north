import {
  readCodexAccountAuthority,
  type CodexAccountAuthority,
  type StoreAccountAuthorityReceipt,
} from "../accounts";
import {
  loadStoreProviderUsageObservation,
} from "../provider-observation-store";
import type { StoreObservationSnapshot } from "../store-observation-adapter";
import { resolveTier, type SemanticTier } from "./catalog";
import type { Effort } from "../harness";
import type { ProviderAvailability, ProviderUsageObservation, RoutingTarget } from "./types";

export interface CodexExecutionAllocation {
  target: RoutingTarget;
  model: string;
  effort?: Effort;
  receipt: Readonly<{
    accountAuthority: StoreAccountAuthorityReceipt;
    usage: StoreObservationSnapshot<ProviderUsageObservation>;
  }>;
}

export interface CodexExecutionAllocatorDependencies {
  readAuthority?: (target: RoutingTarget) => Promise<CodexAccountAuthority | undefined>;
  loadUsage?: (target: RoutingTarget) => Promise<StoreObservationSnapshot<ProviderUsageObservation> | undefined>;
}

function isCredentialLocator(target: RoutingTarget): boolean {
  return target.provider === "openai" && target.authMode === "isolated" && Boolean(target.profile);
}

function defaultUsage(target: RoutingTarget): Promise<StoreObservationSnapshot<ProviderUsageObservation> | undefined> {
  return loadStoreProviderUsageObservation({
    targetId: target.id,
    provider: "openai",
    source: "codex-app-server:account-rate-limits",
  });
}

function quotaUsagePercent(observation: ProviderUsageObservation, now = Date.now()): number | undefined {
  if (observation.provider !== "openai" || observation.source !== "codex-app-server:account-rate-limits") return undefined;
  const primary = observation.windows?.find((window) => window.limitId === "codex:primary");
  if (!primary || !("resetsAt" in primary) || !Number.isFinite(primary.usedPercent)
      || primary.usedPercent < 0 || primary.usedPercent >= 100 || Date.parse(primary.resetsAt) <= now) return undefined;
  return primary.usedPercent;
}

/**
 * Resolve provider-neutral Orchestration work to the one supported Codex model
 * and choose the least-pressured Store-admitted subscription execution account.
 * Local routing configuration identifies credential roots only; Store role and
 * eligibility facts decide whether the account is a candidate.
 */
export async function allocateCodexExecutionAccount(
  targets: readonly RoutingTarget[],
  _availability: readonly ProviderAvailability[],
  tier: SemanticTier | undefined,
  reasoning: Effort | undefined,
  dependencies: CodexExecutionAllocatorDependencies = {},
): Promise<CodexExecutionAllocation | undefined> {
  const route = resolveTier("openai", tier, undefined, reasoning);
  if (!route.model) return undefined;
  const admitted = await Promise.all(targets.map(async (target) => ({
    target,
    authority: isCredentialLocator(target)
      ? await (dependencies.readAuthority ?? readCodexAccountAuthority)(target)
      : undefined,
  })));
  const candidates = admitted.filter((entry) => entry.authority?.role === "execution"
    && entry.authority.executionEligible)
    .map((entry) => entry as typeof entry & { authority: CodexAccountAuthority });
  if (!candidates.length) return undefined;

  const ranked = await Promise.all(candidates.map(async ({ target, authority }, order) => {
    const usage = await (dependencies.loadUsage ?? defaultUsage)(target);
    const usedPercent = usage && quotaUsagePercent(usage.observation);
    if (usedPercent === undefined) return undefined;
    return { target, authority, usage, order, pressure: usedPercent };
  }));
  const chosen = ranked.filter((entry): entry is NonNullable<typeof entry> => entry !== undefined)
    .sort((left, right) => left.pressure - right.pressure || left.order - right.order)[0];
  return chosen ? Object.freeze({
    target: chosen.target,
    model: route.model,
    effort: route.effort,
    receipt: Object.freeze({
      accountAuthority: chosen.authority.receipt,
      usage: chosen.usage,
    }),
  }) : undefined;
}
