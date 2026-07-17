// Provider-local, catalog-derived escalation. Gaffer owns the semantic model
// hierarchy; North applies transport limits and changes one rung in flight.
// Provider changes remain a pre-side-effect routing concern and never occur
// after a lane has begun work.
import type { Effort } from "./harness";
import { fableWindowOpen } from "./fable-window";
import {
  catalogEscalationRungs, resolveModelAlias, resolveTier, supportedReasoning,
  type CatalogEscalationRung,
} from "./providers/catalog";
import {
  ProviderEscalationUnsupportedError,
  type AgentQuery, type ProviderId,
} from "./providers/types";

export type LadderRung = CatalogEscalationRung;
export type AppliedEscalationRoute = { model?: string; effort?: Effort };

// Claude Agent SDK 0.3.195 can change effort only through xhigh in flight.
// `max` remains a valid Gaffer admission route, but a run admitted there is at
// its live-control ceiling. OpenAI's adapter reports no live control at all;
// its catalog ladder is still projected so current-route identity is exact and
// the adapter can reject before any fabricated upgrade is recorded.
const LIVE_EFFORTS: Partial<Record<ProviderId, ReadonlySet<Effort>>> = {
  anthropic: new Set(["low", "medium", "high", "xhigh"]),
};

export function baseLadder(provider: ProviderId): LadderRung[] {
  const allowed = LIVE_EFFORTS[provider];
  const catalog = catalogEscalationRungs(provider);
  return allowed ? catalog.filter(({ effort }) => allowed.has(effort)) : catalog;
}

function temporaryFableRungs(): LadderRung[] {
  if (!fableWindowOpen()) return [];
  const live = LIVE_EFFORTS.anthropic!;
  return supportedReasoning("anthropic", "frontier")
    .filter((effort) => live.has(effort))
    .map((effort) => resolveTier("anthropic", "frontier", undefined, effort))
    .flatMap((resolved) => resolved.model && resolved.effort ? [{
      provider: "anthropic" as const, tier: "frontier" as const,
      model: resolved.model, effort: resolved.effort,
    }] : []);
}

/** Snapshot the ladder for the provider actually admitted to this run. */
export function activeLadder(provider: ProviderId): LadderRung[] {
  const base = baseLadder(provider);
  const promoted = provider === "anthropic" ? temporaryFableRungs() : [];
  return [...base, ...promoted.filter((candidate) =>
    !base.some(({ model, effort }) => model === candidate.model && effort === candidate.effort))];
}

// Compatibility export for fixed-ladder consumers: concrete Anthropic catalog
// routes outside the temporary window, never aliases or invented model families.
export const LADDER = baseLadder("anthropic");

function sameModel(provider: ProviderId, left?: string, right?: string): boolean {
  if (!left || !right) return false;
  // Every ladder model is already concrete. A caller's model may be a declared
  // family alias, so resolve it through the same provider catalog.
  return resolveModelAlias(provider, left) === right;
}

/**
 * Locate the accepted route without ever downgrading an unknown/pinned model to
 * an arbitrary default. Unknown or transport-unsettable routes start at the
 * ceiling, making the next struggle explicit rather than fabricating a route.
 */
export function tierIndexOf(
  provider: ProviderId,
  model: string | undefined,
  effort: Effort | undefined,
  ladder: readonly LadderRung[] = activeLadder(provider),
): number {
  if (ladder.length === 0) throw new Error(`provider ${provider} has no escalation ladder`);
  if (model) {
    const exact = ladder.findIndex((rung) => sameModel(provider, model, rung.model)
      && (!effort || rung.effort === effort));
    return exact >= 0 ? exact : ladder.length - 1;
  }
  const standard = resolveTier(provider, "standard");
  const index = ladder.findIndex(({ model: rungModel, effort: rungEffort }) =>
    rungModel === standard.model && rungEffort === standard.effort);
  return index >= 0 ? index : ladder.length - 1;
}

export type EscDecision =
  | { kind: "escalate"; toTier: number }
  | { kind: "struggle_ceiling" };

export function decideEscalation(
  tier: number,
  ladder: readonly LadderRung[],
): EscDecision {
  if (tier < 0 || tier >= ladder.length - 1) return { kind: "struggle_ceiling" };
  return { kind: "escalate", toTier: tier + 1 };
}

export async function escalateInFlight(
  provider: ProviderId,
  q: AgentQuery,
  ch: { push: (t: string) => void },
  rung: LadderRung,
  reason: string,
  onApplied?: (route: AppliedEscalationRoute) => void,
): Promise<void> {
  if (rung.provider !== provider) {
    throw new ProviderEscalationUnsupportedError(
      `in-flight escalation cannot cross provider boundary ${provider} -> ${rung.provider}`,
    );
  }
  if (!q.setModel || !q.applyFlagSettings || q.supportsInFlightEscalation?.() === false) {
    throw new ProviderEscalationUnsupportedError(
      `provider ${provider} cannot change both model and effort in flight`,
    );
  }
  await q.setModel(rung.model);
  onApplied?.({ model: rung.model });
  await q.applyFlagSettings({ effortLevel: rung.effort });
  onApplied?.({ effort: rung.effort });
  ch.push(
    `[escalation] You appear stuck (${reason}). Upgraded to ${rung.model}/${rung.effort}. `
      + "Step back, reconsider your approach from first principles, then proceed.",
  );
}
