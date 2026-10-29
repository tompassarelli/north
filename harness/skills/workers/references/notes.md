# Workers: notes

Detail behind `SKILL.md`. Read a section only when its question comes up.

## Tier and ledger experiments

These rules come from the ledger and are experiments. Recheck
`worker-ledger --summary` after every 10 new closures in a category, and change
a rule when its numbers move.

- `worker-ledger --recommend` uses an 80% measured success
  threshold. A run counts toward the threshold when it reported Done with
  no later `Follows:` run and, if it committed, its commit landed. The command
  names the cheapest tier that meets the threshold in the finest cell with at least 5
  runs, backing off Surface, Verify, Scope, then Spec. It changes a cell's pick
  to a dearer passing tier only for a 15-point lead. When none meets the threshold,
  it selects the sampled tier with the lowest measured $ per success; without
  sampled measured costs it reports insufficient evidence and names no tier.
- Haiku trial: alternate tooling, balance-tuning and bug-known-cause items that
  name their files and have a measured Done when between `worker` and
  `worker-haiku` with `Arm: haiku-trial`, until the finest facet cell with 5
  runs has 5 Haiku closures or 2 Haiku failures. Docs-policy work is never
  trialed on Haiku: Tom wants policy, skill and documentation prose written
  carefully by Opus. Restaff a failure at Opus medium
  with `Follows:`. The ledger's Haiku rows decide the category's default tier.
  Haiku's baseline: 29 of 32 mechanical runs succeeded, at a 1-minute median
  (2026-10-10).
- Keep Haiku away from judged art, bisects, native checks and waits longer than
  2 minutes. On 2026-10-10 it ended its turn mid-run twice, which killed the run.
- Opus high's overall rates (feature 6/25, native-check 0/8) come from
  escalated hard cases, so they do not rank tiers. Alternate the features and
  native-checks that Opus medium leaves unfinished between two arms: escalation
  to Opus high, and the cheaper fix (split a feature into smaller boxes; fix a
  native-check's client, capacity or host cause). Compare closures on those
  leftovers only.
- Use provider benchmarks to pick a tier only until the category has 5 closed
  issues at that tier.
- Token cost: `north:harness/model-catalog.toml` holds sourced list API prices
  and context thresholds. `agents plan` prints these comparisons separately
  from Tom's subscription value and from measured run outcomes.
- Escalate to the next tier `agents plan` prints only after `Stop: reasoning`:
  a wrong cause or two failed fixes.

## Planner effort

- Staff `planner` at Opus high by default. Tom's hypothesis (2026-10-12) is
  that high fits about 90% of plans, xhigh the next 8% and `max` the top 2%.
- Pass `effort: xhigh` or `max` only with evidence from our own data (a high
  plan for the same issue came back Not done, or `worker-ledger` shows high
  plans in that category closing worse) or when Tom asks. Name that evidence
  in the brief, and rank complexity against past planned work in
  `worker-ledger`, not by feel.
- Staff it for a plan, not code, when an issue gets its second Not done, a
  category's last 10 runs fall below 40% closed, or the change is
  architectural.

## Review lenses and findings

- Give each adversarial reviewer one lens no other reviewer has, and have each
  report `Accepted flaws: N`, so the ledger keeps the useful lenses.
- Price rewrite and maintenance effort at AI cost (10–100x cheaper than a
  human), and rank findings and alternatives by leverage, not human effort.
- Lenses: desync or determinism hunter, frame-time profiler, new player at a
  first match, Warcraft engine limits, maintainer six months later, cheapest
  alternative, attacker of the measurement itself. Record which lens found each
  accepted flaw, so the ledger keeps the useful lenses.
- Weigh findings by evidence first and reviewer tier second. A finding with a
  failing scenario, test or measurement counts whichever model raised it.
  Investigate an unevidenced finding from Opus max or xhigh. Have a stronger model check an unevidenced finding from a smaller
  model (Haiku or Sonnet) once before anyone acts on it or
  dismisses it.
- Reviewers are Claude only (Tom's standing order, 2026-10-16): use Opus at a
  different tier from the author's, or Sonnet for an Opus author.

## Compute routing (agreed with Tom 2026-10-10)

- A cloud run costs only the plan usage a local worker would, machine
  included. Send code-only Smashcraft and Wisp items to cloud workers ([cloud.md](cloud.md))
  first: 4 cores per run, no cap on runs.
- Send parallel batch work (balance and CPU fields, suites, soaks) to GitHub
  runners through `github`: the hosted runners and the `farm-big` runners on
  the host south:agents/orchestration.toml names for them.
- Run always-on Warcraft clients, the offline LAN pool and GPU work on the
  hosts south:agents/orchestration.toml names for them. Keep signed-in
  clients local unless you move them deliberately.
- Rent no CPU provider for batch work: the farm covers it, and renting is a
  cost Tom decides.
- Run a job longer than the 45-minute leash (renders, long captures) yourself
  under a capacity lease, then staff a short worker to use its output.

## Landing mechanics

- Autoland repository: the worker ends once `safe-push --to main` has pushed
  its `claude/land-*` branch, and reports `Done: queued <branch>`. Autoland
  lands it, and a Haiku worker ticks its boxes after landing.
- Elsewhere: a lead runs `safe-push` in the foreground with a 10-minute
  timeout. A worker runs it in the background, because worker-wait-guard
  refuses a foreground landing, and reports once it exits. If the landing takes
  longer than 10 minutes, report the exact lane so the parent lands it.
- While main is red, staff its fix first and keep queuing lanes behind it.
  Never cancel an Autoland run: a cancelled bisect half strands its lanes.
- Before a playtest's last blocker lands, prebuild its map with the fix applied
  and run the frame-cost compare, so the build cannot fail at playtest time.
- Before an issue closes, `worker-haiku` reruns each Done when check against
  origin/main and quotes the numbers. Close the issue only when every box
  reproduces.

## Recycling and waits

- The worker-handoff hook nudges an Agent-tool worker between tool calls. A
  fleet worker gets the 150k nudge from its session host only when its brief
  says `Arm: handoff-150k`; north dispatch alternates that arm with
  `Arm: no-handoff` until `worker-ledger --summary --by arm` can compare them
  (north#252). The worker-wait-guard hook keeps workers out of foreground
  waits, so messages reach them at once.
- A recycled worker writes its handoff at a natural checkpoint. Check the fresh
  worker 5 minutes later for work it is rediscovering.
