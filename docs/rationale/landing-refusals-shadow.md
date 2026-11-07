# Landing refusals in shadow: only a broken build, failing checks, secrets and privacy refuse a lane

`north land gate-stats --since 7d` on the lander, read on 2026-11-07, showed most landing refusals protecting nothing: test admission refused 136 lanes and 127 of them landed later (20 worker-hours), the debt ceiling 46 (43 later, 33 with the same patch, 31.8 worker-hours), the language gate 46 (42 later), the revert guard 33 (21 unchanged after a rebase), the runtime-floor gate 14 (13 unchanged) and the coverage test 8. A refusal that the same patch later passes costs a worker's turn and the queue wait it repeats, and returns nothing (north#729).

## The rule

A lane is refused only for a broken build, clippy, failing tests (the `land.check` lines and the checks a lane adds), the pick itself (a conflict, dropped commits, an index that cannot be regenerated), a secret, or a privacy leak (the deny-list, commit-email and native-item scans). Every other landing gate follows one switch, `landing_refusals` in `harness/defaults.toml`. Under `shadow` the gate still runs; its would-be refusal is appended to `$XDG_STATE_HOME/north/gates/landing-shadow.jsonl` on the host that judged it, with the place (`enqueue`, `pick` or `candidate`), the lane and the gate's own text, and the lane goes on. `north land gate-stats` prints those rows beside the real refusals.

This is the shadow-first rule of `harness/gates.toml` (north#706) run in reverse: a new gate starts in shadow and earns enforcement with 24 h of would-be refusals a reviewer reads; here every existing gate outside the list goes back to shadow and earns refusal back the same way, gate by gate, with a would-be refusal that names a real fault.

The shadow switch reaches a host with its runtime: the lander's pick and candidate scans read the lander's runtime tree, `north land enqueue` the worker host's.

In shadow, `north land enqueue` skips the cheap `@enqueue` after-landing lines and runs clippy and the `@enqueue` `land.check` lines (the policy gate for a lane that edits harness text, whose refusal held every host's policy generation for about 4 hours on 2026-11-07) before queueing: the after-landing lines run after landing anyway, where a failure files debt instead of refusing.

## Deleted outright

- The debt-ceiling lane gate went on 2026-11-06 (21c259331): debt orders the queue and refuses nothing. Its lander-drill fault went with it on 2026-11-07.
- The runtime-floor gate (`north runtime-floor-gate`, the lane gate and the language-scan step): 13 of its 14 refused lanes landed unchanged, and an invocation a deployed runtime lacks fails at the call, where the runtime update fixes it.

## The revert guard judges the lane's patch on main

The guard counted any line the landing removed that a commit "the lane never saw" had written as a revert of that commit, and drew its window from the lane's fork point. A lane is always landed by a three-way pick, which cannot remove a line its own patch never touched, so those hits were diff and blame artifacts of an old fork point, and the same patch passed once the worker rebased: 21 of the 33 refusals. The guard now judges every lane as if it were rebased: the landing is the lane merged onto main (`stale_revert::onto` for the dry run and enqueue, the cherry-picked candidate in the lander), the window is main's last 48 h, and a commit counts as undone only when the lane restores half the lines it removed or removes four fifths of the lines it added from a file that stays. Its verdict depends on main, the merged tree and the lane's own patches, so a rebase no longer changes it.
