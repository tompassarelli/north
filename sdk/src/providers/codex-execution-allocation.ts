import {
  accountsRoot,
  readCodexAccountAuthority,
  type CodexAccountAuthority,
  type StoreAccountAuthorityReceipt,
} from "../accounts";
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
  receipt: Readonly<{
    accountAuthority: StoreAccountAuthorityReceipt;
    authentication: Pick<ProviderAvailability, "provider" | "targetId" | "available" | "reason">;
    quota: AccountAvailabilityRow;
  }>;
}

export interface CodexExecutionAllocatorDependencies {
  readAvailability?: (targets: readonly RoutingTarget[]) => AccountAvailabilityRow[];
  readActivity?: (target: RoutingTarget) => Promise<OpenAISessionActivity>;
  readAuthority?: (target: RoutingTarget) => Promise<CodexAccountAuthority | undefined>;
}

function isCredentialLocator(target: RoutingTarget): boolean {
  return target.provider === "openai" && target.authMode === "isolated" && Boolean(target.profile);
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
 * and choose the least-pressured Store-admitted subscription execution account.
 * Local routing configuration identifies credential roots only; Store role and
 * eligibility facts decide whether the account is a candidate.
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
  const admitted = await Promise.all(targets.map(async (target) => ({
    target,
    authority: isCredentialLocator(target) && ready(availability, target)
      ? await (dependencies.readAuthority ?? readCodexAccountAuthority)(target)
      : undefined,
  })));
  const candidates = admitted.filter((entry) => entry.authority?.role === "execution"
    && entry.authority.executionEligible)
    .map((entry) => entry as typeof entry & { authority: CodexAccountAuthority });
  if (!candidates.length) return undefined;

  const candidateTargets = candidates.map(({ target }) => target);
  const rows = new Map((dependencies.readAvailability ?? defaultAvailability)(candidateTargets)
    .map((row) => [row.account, row]));
  const ranked = await Promise.all(candidates.map(async ({ target, authority }, order) => {
    const row = rows.get(target.id);
    if (!row || row.stale || !accountAvailabilityRowIsUsable(row)) return undefined;
    const activity = await (dependencies.readActivity ?? defaultActivity)(target);
    const headroomPressure = row.rungs.window?.pct ?? 100;
    const authentication = availability.find((entry) => entry.targetId === target.id
      && entry.provider === "openai" && entry.available);
    if (!authentication) return undefined;
    return { target, authority, row, authentication, order, pressure: headroomPressure + activity.live * 25 };
  }));
  const chosen = ranked.filter((entry): entry is NonNullable<typeof entry> => entry !== undefined)
    .sort((left, right) => left.pressure - right.pressure || left.order - right.order)[0];
  return chosen ? Object.freeze({
    target: chosen.target,
    model: route.model,
    effort: route.effort,
    receipt: Object.freeze({
      accountAuthority: chosen.authority.receipt,
      authentication: Object.freeze({
        provider: chosen.authentication.provider,
        targetId: chosen.authentication.targetId,
        available: chosen.authentication.available,
        reason: chosen.authentication.reason,
      }),
      quota: chosen.row,
    }),
  }) : undefined;
}
