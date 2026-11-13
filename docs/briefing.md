# Briefing a worker

A lead turns a box of work into a running worker in seven steps. The [workers skill](../harness/skills/workers/SKILL.md) is the procedure and holds the tier table; this page names the order and where each step's facts live. The words follow [terms.md](terms.md).

1. **Pick the work.** `north threads ARGV` runs the threads commands on the ledger host: `unowned` lists priority items nobody holds, `ready` lists ready work and `show REPO#N` reads an item's Done when boxes. A held item is never duplicated.
2. **Pick the provider and account.** `north config agents plan` reads the signed-in providers, the operator repository's `orchestration.toml` and live usage, and routes each tier to an account.
3. **Pick the tier and ETA.** `north ledger worker --recommend` picks the cheapest tier whose measured success meets the `success_threshold` key in [harness/defaults.toml](../harness/defaults.toml) and prints the ETA.
4. **Write the brief.** Carry Item, Category, facet lines, files, Done when and ETA. A retry or follow-up adds `Follows:`.
5. **Launch through the fleet.** `north fleet launch` starts the session with the brief as its system prompt and records the parent link. Before anything starts it runs the placement gate, and a Claude launch that holds an issue starts on the tier its brief earns, so leave the model and effort to it. The "Placement gate" and "Launch tier" entries in [features/03-fleet-sessions/](features/03-fleet-sessions/) and `north fleet launch --help` give the verdicts and the recorded overrides.
6. **Queue what waits.** A box that waits on another lane is queued with `north threads ARGV` (`queue follow`); `north follows-release` starts it once its predecessor lands on main.
7. **Record.** The fleet's receipts capture delivery and completion. `north ledger worker` records each run's outcome, cost and duration, and those records feed the next recommendation ([feedback-loop.md](feedback-loop.md)).

## Messages between fleet sessions

`north fleet send NAME TEXT...` sends a message and `north fleet ask NAME QUESTION...` waits for a reply. Use `--parent NAME` when launching a child session so `north fleet tree` shows the relationship. A worker's final message is its report; the fleet records delivery and session completion.

Once the worker has a change, [landing.md](landing.md) follows it to main.
