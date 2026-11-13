# Tier routing in full

Read this when the routing in `SKILL.md` does not settle a tier, an arm trial applies, or the next tier cannot run here. Ledger experiments behind these rules are in [notes.md](notes.md#tier-and-ledger-experiments).

- `north ledger worker --summary` shows each category's success; `--recommend` picks the tier.
- Every category starts on the fixed tier its staffing rule names: mechanical on Haiku with its Sonnet retry, art and the rest on Sonnet, and the categories the rule sends to Opus. The cell draw then takes the model and effort from `harness/routing-cells.toml`; the Sonnet-against-Opus arm alternation and its tier route are deleted (docs/rationale/spend-rule.md, north#702), so no lead alternates a category's tier by hand.
- Restaff every stop at the same tier with a `Follows:` brief carrying the failed evidence; dispatch climbs one rung of the ladder on a worker-caused spin (a not-done run with no recorded harness cause, a repeated lander refusal, a reopened box, or twice the stated ETA) and leaves other stops on their rung, because most stops are brief or capacity faults; no `Stop:` line decides it (docs/rationale/nearest-routing.md section 5). When the next tier cannot run here, bring the supervisor one recommendation.
- Mechanical work is exact, fully specified edits, box ticks, rebases, regeneration, conflicts and running a named check. Bisects and native checks go to `worker` or stay with the lead. Run the haiku trial in [notes](notes.md).
