---
name: planning
description: >-
  Plan costly-to-reverse changes (interfaces, state, dependencies, boundaries); consequential briefs; prior art; hardening, security or compatibility level; price infra, CI or harness work vs features; code-health review size; delivery scope, stalled work, merging issues; resist extra process. Skip routine changes.
grounded: 2026-10-15
written: 2026-10-15
---

# Planning

## Plan a costly decision

1. Define outcome, scope, preserved behavior, consequential decisions and completion evidence, because a plan exists to settle what is costly to reverse.
2. Choose the shortest path with an early falsifying check of its central risk, because a wrong premise found late wastes everything built on it.
3. Add migration, recovery, compatibility or rollout only for an affected boundary, because lifecycle machinery without a consumer is ceremony.
4. Revise the remaining route when evidence breaks a premise and stop when more prose changes no action, because a stale plan item is not work.

A consequential brief carries the behavior trace (what the current system actually does, observed) and the rationale for the current design (why it is that way), because a brief without them lets the worker reverse a decision blind.

Read [plan outline](references/plan-outline.md) when a durable decision needs a written plan.

## Research prior art

1. Name the decision, constraints, uncertain claim and sufficient evidence, because search without a decision accumulates context and changes nothing.
2. Inspect local implementation, callers, dependencies, tests and history first, because local fit outranks general popularity.
3. Compare authoritative sources for the conventional solution and one distinct viable alternative, because custom infrastructure must beat the convention it replaces.
4. Use `external-code` before deriving from outside code, because licensing precedes use as a reference; a departure from convention needs a forcing need and a falsifying check.
5. Report the choice, deciding evidence, tradeoff, sources and material uncertainty, then stop, because research ends when evidence selects the next action.

Read [prior-art notes](references/prior-art.md) when structuring research.

## Budget ceremony

1. Take baseline rigor from the goal's spec and Done when and add nothing they do not name, because discomfort is not evidence.
2. Escalate one mechanism only for a named current consumer, failure and failure cost, because an agent can justify almost any addition otherwise; one addition never justifies an adjacent one.
3. Keep changeability, correctness, robustness, security and operations as independent decisions, because strength in one does not require the others.
4. To harden, state the named guarantee, allowed degradation and surviving state first, because a claim without failure assumptions cannot be falsified.
5. Repair the weakest resource, retry, cancellation or persistence boundary with bounded retries, and idempotence where repeated effects cause harm, because that is where the named guarantee breaks.
6. Exercise the named failure without weakening existing gates, because a lowered gate hides the defect. Safety, secrets and destructive-operation boundaries never trade against speed.

Read [hardening](references/hardening.md) when choosing failure paths, or [rationale](references/ceremony-rationale.md) when revising this section or a case seems to slip past it.

## Price an infra box

1. Price it, because a speedup may cost no more than it saves.
   - Cost `C` = agent-minutes to build, review and land + 10 x operator-minutes + upkeep.
   - Saving per change `S` = 10 x operator-blocked wait minutes saved (ask to usable build) + 1 x agent-minutes saved; weight agent-minutes 0.5 when the usage cap is not binding. Count the two savings separately before weighting.
   - Rate `R` = changes/day over the trailing 7-14 days, from the project's change ledger, else commits in 14 days / 14.
   - Payback days = `C / (S x R)`.
2. Extrapolate the trend at most 14 days (7 in maintenance mode), because volume forecasts decay fast; the 30-day extension rule is in pricing.
3. Staff at payback <= 3 days (1-2 in maintenance mode); 3-7 days only if it also removes the top bottleneck stage, removes flakes or enables a queued roadmap item; above 7 only if it beats the top feature on cost of delay per duration, else defer, because feature work is the default use of capacity.
4. Re-profile after each fix, stop optimizing a stage when its share of operator wait is under 25%, and fix variance before mean, because the constraint moves; the other stop rules are in pricing.
5. Size the recurring code-health review the same way (effort `C`, hotspot interest removed `S`, touched area change rate `R`), because debt costs only when touched.

Read [pricing sources](references/pricing.md) when a price is disputed, and [notes](references/notes.md) before asking the operator about one.

## Steward delivery

1. Separate required product outcomes from advisory targets and optional coverage, because blending them lets every new case reopen the work.
2. Preserve independently finishable outcomes and their evidence when consolidating issues, because merged completion predicates hide progress.
3. When completion stalls, reconcile passed criteria, remaining blockers and additional coverage, and record finished outcomes as complete, because starvation looks like busy progress.
4. Choose the next repair that advances an unmet outcome, never substituting synthetic checks for human judgment or unavailable hardware, because a proxy cannot supply that observation.
5. Record a consequential deferral or a landing over a soft target with `Target-debt: <bar>; measured <value>; owner <owner>; revisit <trigger>`, because a deferral without a trigger never returns.
6. Name a gating bar's source in its box or rationale doc; a landing needs no `Provenance:` trailer (retired 2026-11-04), because a rework signal now watches quality.

Read [stewardship notes](references/stewardship.md) when resolving stalled delivery, a deferral or the retired Provenance gate.
