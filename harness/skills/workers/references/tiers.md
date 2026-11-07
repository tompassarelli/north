# Tier routing in full

Read this when the routing in `SKILL.md` does not settle a tier, an arm trial applies, or the next tier cannot run here. Ledger experiments behind these rules are in [notes.md](notes.md#tier-and-ledger-experiments).

- `north ledger worker --summary` shows each category's success; `--recommend` picks the tier.
- Run tooling, features, known-cause bugs, native checks and balance tuning as `Arm: sonnet-high` or `Arm: worker` in turn until each arm has 15 runs stamped with it in the category and 15 closed boxes whose first run carried it (unstamped runs and boxes do not count), then at the arm's lower API-equivalent spend per closed arm box over 30 days, which stands for the cheapest tier whose landed work does not raise rework (follow-up fixes, reverts, red-main culprits and review findings; north docs/rationale/tier-routing.md says which of those terms are measured yet).
- Restaff every stop at the same tier with a `Follows:` brief carrying the failed evidence, and escalate one tier only after a second `Stop: reasoning` on the same box, because most stops are brief or capacity faults. When the next tier cannot run here, bring the supervisor one recommendation.
- Mechanical work is exact, fully specified edits, box ticks, rebases, regeneration, conflicts and running a named check. Bisects and native checks go to `worker` or stay with the lead. Run the haiku trial in [notes](notes.md).
