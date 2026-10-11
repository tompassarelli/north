# Workers: notes

Detail behind `SKILL.md`. Read a section only when its question comes up.

## Tier and ledger experiments

These rules come from the ledger and are experiments. Recheck
`worker-ledger --summary` after every 10 new closures in a category, and change
a rule when its numbers move.

- `worker-ledger --recommend` takes its bar from the Item repo's `[projects]`
  entry in orchestration.toml: prototype 60%, solid 80%, critical 90%, and 80%
  for an unmatched repo. A run counts toward the bar when it reported Done with
  no later `Follows:` run and, if it committed, its commit landed. The command
  names the cheapest tier that meets the bar in the finest cell with at least 5
  runs, backing off Surface, Verify, Scope, then Spec. It changes a cell's pick
  only for a 15-point lead.
- Haiku trial: send other tooling, balance-tuning and bug-known-cause items to
  `worker-haiku` with `Arm: haiku-trial` until the finest facet cell with 5 runs
  has 5 Haiku closures or 2 Haiku failures. Restaff a failure at Opus medium
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
- Token cost: `north:socrates/model-catalog.toml` holds sourced list API prices
  and context thresholds. `agents plan` prints these comparisons separately
  from Tom's subscription value and from measured run outcomes.
- Escalate to the next tier `agents plan` prints only after `Stop: reasoning`:
  a wrong cause or two failed fixes.

## Review lenses and findings

- Lenses: desync or determinism hunter, frame-time profiler, new player at a
  first match, Warcraft engine limits, maintainer six months later, cheapest
  alternative, attacker of the measurement itself. Record which lens found each
  accepted flaw, so the ledger keeps the useful lenses.
- Weigh findings by evidence first and reviewer tier second. A finding with a
  failing scenario, test or measurement counts whichever model raised it.
  Investigate an unevidenced finding from Opus max or xhigh or from Astra
  xhigh. Have a stronger model check an unevidenced finding from a smaller
  model (Gemini, Haiku, Codex medium) once before anyone acts on it or
  dismisses it.
- When the reviewer must come from another family, use Codex `gpt-6-astra`
  xhigh while Codex usage remains, then `gpt-6.1-sol` high, then Opus at a
  different tier. Gemini (`gemini-review`) is the cheap extra reviewer, for
  public diffs only.

## Compute routing (agreed with Tom 2026-10-10)

- A cloud run costs only the plan usage a local worker would, machine
  included. Send code-only Smashcraft and Wisp items to `cloud-workers`
  first: 4 cores per run, no cap on runs.
- Send parallel batch work (balance and CPU fields, suites, soaks) to GitHub
  runners through `github-actions` first. Move it to vast.ai through `vast-job`
  when the farm queue delays a result.
- Run always-on Warcraft clients, the offline LAN pool and GPU work on the
  Hetzner box once it exists, and on the vast.ai VM until then. Keep signed-in
  Definitive clients local unless you move them deliberately.
- Rent no other CPU provider: DigitalOcean and similar charge several times
  vast.ai's price for the same cores.
- Run a job longer than the 45-minute leash (renders, long captures) yourself
  under a capacity lease, then staff a short worker to use its output.

## Landing mechanics

- Autoland repository: the worker ends once `safe-push --to main` has pushed
  its `claude/land-*` branch, and reports `Done: queued <branch>`. Autoland
  lands it, and a Haiku worker ticks its boxes after landing.
- Elsewhere: run `safe-push` in the foreground with a 10-minute timeout. If the
  landing takes longer, report the exact lane so the parent lands it in the
  background.
- While main is red, staff its fix first and keep queuing lanes behind it.
  Never cancel an Autoland run: a cancelled bisect half strands its lanes.
- Before a playtest's last blocker lands, prebuild its map with the fix applied
  and run the frame-cost compare, so the build cannot fail at playtest time.
- Before landing a `Spec: judged` cross-module or cross-repo lane, have a
  reviewer from another model family or tier check the diff for wrong behavior
  and weakened tests, plus Gemini on public repos. Fix what it finds before
  `safe-push`.
- Before an issue closes, `worker-haiku` reruns each Done when check against
  origin/main and quotes the numbers. Close the issue only when every box
  reproduces.

## Recycling and waits

- The worker-handoff hook nudges a worker between tool calls. The
  worker-wait-guard hook keeps workers out of foreground waits, so messages
  reach them at once.
- A recycled worker writes its handoff at a natural checkpoint. Check the fresh
  worker 5 minutes later for work it is rediscovering.
- Expect a report at 45 minutes or twice the ETA, whichever comes first.

## Lead ticks

- `tick` (`agents routines show tick`) runs `threads unowned`, `worker-sweep
  --session` and `capacity-watchdog report --minutes 5 --minutes 30 | jq -c
  '{minutes, flags, incidents}'`. It acts only on flagged rows, so a quiet tick
  spawns no worker.
- `worker-sweep` lists STALLED rows idle more than 300 minutes, but the tick
  ignores them: they are pre-compaction transcripts.
- The tick rebuilds the goal's dependency graph (its GitHub issues and main CI)
  every third tick or on a state change. It requeues a lane whose push log ends in a rebase-and-retry
  message with `scratchpad/rebase-requeue.sh`.
- Count issues with `--limit 500`. Re-arm the origin/main landing monitor when
  it expires.
