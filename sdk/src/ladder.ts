// Escalation ladder for escalate-not-kill (thread 019f1194-ca57). On struggle an
// agent climbs ONE rung — a smarter model and/or higher effort — IN-FLIGHT (no
// respawn, no context re-serialization), instead of being killed at a turn cap.
//
// Ceiling is opus/xhigh (raised from opus/high): applyFlagSettings accepts
// Settings.effortLevel up to "xhigh" (sdk.d.ts), so xhigh IS settable mid-run — only
// "max" effort needs the deferred Option-B interrupt+resume. The Fable rung sits ABOVE
// opus/xhigh and is present ONLY inside the owner-ordered window (fable-window.ts): a
// lane escalates to Fable exactly when opus/xhigh demonstrably spins. fable/high is
// also <= xhigh effort, so setModel + applyFlagSettings apply it cleanly in-flight.
import type { Effort } from "./harness";
import { resolveModel } from "./harness";
import { fableWindowOpen } from "./fable-window";
import { ProviderEscalationUnsupportedError, type AgentQuery } from "./providers/types";

const BASE_LADDER: Array<{ model: string; effort: Effort }> = [
  { model: "haiku", effort: "low" },     // 0 cheap baseline
  { model: "haiku", effort: "high" },    // 1 haiku ceiling
  { model: "sonnet", effort: "medium" }, // 2 default start / first model jump
  { model: "sonnet", effort: "high" },   // 3 workhorse
  { model: "sonnet", effort: "xhigh" },  // 4 sonnet ceiling
  { model: "opus", effort: "high" },     // 5 judgment
  { model: "opus", effort: "xhigh" },    // 6 opus ceiling (standing in-flight top)
];

const FABLE_RUNG: { model: string; effort: Effort } = { model: "fable", effort: "high" };
export type LadderRung = { model: string; effort: Effort };
export type AppliedEscalationRoute = { model?: string; effort?: Effort };

// The ladder in effect NOW: base ramp, plus the Fable rung while the window is open.
// Date-dependent, computed per call so the rung vanishes the instant the window closes —
// the mechanical half of the gate (fable-window.ts is the other).
export function activeLadder(): LadderRung[] {
  return fableWindowOpen() ? [...BASE_LADDER, FABLE_RUNG] : BASE_LADDER;
}

// Back-compat: the base ramp as a constant (for importers indexing a fixed tier).
export const LADDER = BASE_LADDER;

// Default starting rung when escalation is on but no model is pinned: sonnet/medium.
export const DEFAULT_START_TIER = 2;

export function tierIndexOf(model?: string, effort?: Effort, ladder: readonly LadderRung[] = activeLadder()): number {
  if (!model) return DEFAULT_START_TIER;
  const i = ladder.findIndex((t) => t.model === model && (!effort || t.effort === effort));
  return i >= 0 ? i : DEFAULT_START_TIER;
}

export type EscDecision =
  | { kind: "escalate"; toTier: number }
  | { kind: "struggle_ceiling" };

// Resource-envelope admission and provider-pressure routing happen before the
// provider boundary. Once a run has side effects, escalation is therefore a
// pure active-ladder decision: climb one rung or report the ceiling.
export function decideEscalation(tier: number, ladder: readonly LadderRung[] = activeLadder()): EscDecision {
  if (tier >= ladder.length - 1) return { kind: "struggle_ceiling" };
  return { kind: "escalate", toTier: tier + 1 };
}

// In-flight rung change (Option A): swap model + effort on the live query and nudge
// the agent. setModel applies to the NEXT turn ("subsequent responses"); the current
// turn finishes on the old tier. All v1 efforts are representable by the live
// control, but the provider may still reject either operation; onApplied reports
// each completed control so the caller can preserve a truthful partial route.
export async function escalateInFlight(
  q: AgentQuery,
  ch: { push: (t: string) => void },
  rung: { model: string; effort: Effort },
  reason: string,
  onApplied?: (route: AppliedEscalationRoute) => void,
): Promise<void> {
  if (!q.setModel || !q.applyFlagSettings || q.supportsInFlightEscalation?.() === false) {
    throw new ProviderEscalationUnsupportedError(
      "active provider cannot change both model and effort in flight",
    );
  }
  const model = resolveModel(rung.model)!;
  await q.setModel(model);
  onApplied?.({ model });
  await q.applyFlagSettings({ effortLevel: rung.effort });
  onApplied?.({ effort: rung.effort });
  ch.push(
    `[escalation] You appear stuck (${reason}). Upgraded to ${rung.model}/${rung.effort}. ` +
      `Step back, reconsider your approach from first principles, then proceed.`,
  );
}
