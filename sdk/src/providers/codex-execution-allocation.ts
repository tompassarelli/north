import { accountsRoot } from "../accounts";
import {
  accountAvailabilityRowIsUsable,
  readAccountAvailability,
  type AccountAvailabilityRow,
} from "../account-availability";
import { readOpenAISessionActivity, type OpenAISessionActivity } from "../openai-session-activity";
import { resolveTier, type SemanticTier } from "./catalog";
import type { Effort } from "../harness";
import type { ProviderAvailability, RoutingTarget } from "./types";

export interface CodexExecutionAllocation {
  target: RoutingTarget;
  model: string;
  effort?: Effort;
}

export interface CodexExecutionAllocatorDependencies {
  readAvailability?: (targets: readonly RoutingTarget[]) => AccountAvailabilityRow[];
  readActivity?: (target: RoutingTarget) => Promise<OpenAISessionActivity>;
}

function isExecutionTarget(target: RoutingTarget): boolean {
  if (target.provider !== "openai" || target.authMode !== "isolated" || !target.profile) return false;
  return !/(?:^|[-_])pm$/.test(target.id) && !/(?:^|[-_])pm$/.test(target.profile);
}

function ready(availability: readonly ProviderAvailability[], target: RoutingTarget): boolean {
  return availability.some((entry) => entry.targetId === target.id
    && entry.provider === "openai" && entry.available);
}

function defaultAvailability(targets: readonly RoutingTarget[]): AccountAvailabilityRow[] {
  return readAccountAvailability({
    accounts: targets.map(({ id, provider }) => ({ id, provider })),
  });
}

function defaultActivity(target: RoutingTarget): Promise<OpenAISessionActivity> {
  return readOpenAISessionActivity({
    accountRoot: `${accountsRoot()}/openai/${target.profile!}`,
  });
}

/**
 * Resolve provider-neutral Orchestration work to the one supported Codex model
 * and choose the least-pressured subscription-backed execution account. The
 * PM account is oversight-only and deliberately cannot enter this candidate set.
 */
export async function allocateCodexExecutionAccount(
  targets: readonly RoutingTarget[],
  availability: readonly ProviderAvailability[],
  tier: SemanticTier | undefined,
  reasoning: Effort | undefined,
  dependencies: CodexExecutionAllocatorDependencies = {},
): Promise<CodexExecutionAllocation | undefined> {
  const route = resolveTier("openai", tier, undefined, reasoning);
  if (!route.model) return undefined;
  const candidates = targets.filter((target) => isExecutionTarget(target) && ready(availability, target));
  if (!candidates.length) return undefined;

  const rows = new Map((dependencies.readAvailability ?? defaultAvailability)(candidates)
    .map((row) => [row.account, row]));
  const ranked = await Promise.all(candidates.map(async (target, order) => {
    const row = rows.get(target.id);
    if (!row || row.stale || !accountAvailabilityRowIsUsable(row)) return undefined;
    const activity = await (dependencies.readActivity ?? defaultActivity)(target);
    const headroomPressure = row.rungs.window?.pct ?? 100;
    return { target, order, pressure: headroomPressure + activity.live * 25 };
  }));
  const chosen = ranked.filter((entry): entry is NonNullable<typeof entry> => entry !== undefined)
    .sort((left, right) => left.pressure - right.pressure || left.order - right.order)[0];
  return chosen ? { target: chosen.target, model: route.model, effort: route.effort } : undefined;
}
