# Eval B: the load ladder

Measures north's fleet at several workers at once against the same tasks done one at a time in plain T3 Code
(south#107; the registered design and thresholds B-1 to B-5 are in the assessment's `EVALS.md`, section 3).

## Workload

`eval-b workload` writes a synthetic Bun repository from a seed: N tasks, each a small string function in
`src/tNN.ts` whose prepared, skipped test in `test/tNN.test.ts` the task turns on. Every third task also registers
its function in the shared `src/registry.ts`, so concurrent lanes conflict there at a known share. `bun test` is the
whole check and runs in about a second, so a lane costs megabytes. The generator writes the expected test files
beside the repository; the runner compares each landed test file with them (the hidden check).

## North arm

`eval-b ladder --level N` runs one level end to end:

1. Lays out a scratch project at `~/code/eval-b`, under the code directory because the worker sandbox's broker serves a lane's requests only from there, and resets it through git at each start and end: on the laptop a bare origin and the lander's checkout, on every
   build host a mirror origin, a main checkout and one prepared lane per task. While a level runs, the runner pushes the
   laptop origin's main to each host's mirror within a quarter second of each landing, standing in for a shared remote.
2. Fills spares on that host for the level's first launch before the first wave, with `north spare-fill --host HOST --` and that launch's own words, so it takes a warm client; every brief pins `Arm: context-500k`, so the fill and the launches draw the same compaction window, and `conditions.context_arm` says so.
3. Starts a scratch lander (`north land serve --repo ~/code/eval-b/main`, a child of the runner), which pulls each host's outbox like the production landers do.
4. Launches N workers at once through `north fleet launch` (claude-opus-5-5 at medium, `--prompt north`, pinned with `--pin` to
   the first host of `--hosts`, so every arm runs on one machine) once that host has at most two jobs queued in its capacity admission (placement refuses a pinned launch at four); the wait is polled every 2 s for at most five minutes and counts in each task's time. It keeps N in flight until the level's tasks are all resolved, and gives each stalled worker at most
   two scripted nudges as Eval A does.
5. Collects each session's record, spend and transcript, stops and forgets it, checks the landed main, writes one
   row per task to the results file and removes the lanes from every host; the scratch project's origin and main checkout stay for the next run.

Every brief carries `Needs:` with the first host of `--hosts` (cpx62 by default), so placement prefers the build host the ladder runs on. Every task's text
also asks for one line at the end of its source, `export const RUN_ID = "<stamp>-l<level>-<task>";`, and its
acceptance command greps for it, and every brief carries the line
`Commit message: Do ladder task <task> (Run: <stamp>-l<level>-<task>)`, which the launch proposes, judges and keeps
as a real launch does, so a finish call that leaves out its message commits that text and the hand-in finds its
verdict cached; the private-kind judge reads only prose (commit messages, added doc lines, the ref name), so no run
hands in a diff or a message an earlier run judged, which a row's `verdict_s` (0.2 to 0.4 s from the cache, 3 s or more judged) shows; `conditions.unseen` records it and `model_requests` counts the session's distinct
model requests. `--model M` launches every worker of the ladder on M instead of claude-opus-5-5, at the same effort and
with the same brief, and each row names it in `conditions.model`; its rows answer which model the harness may choose
behind the same check, a different comparison from the pinned-model rows, and go in their own results file.
`--account d,e,a` launches the ladder's tasks on those accounts in turn, task i on the account at i modulo the list,
and each row names its account in `conditions.account`; one account, the default d, is the pinned case.
`--effort E` launches every worker at effort E instead of medium, names E in the brief's Model line, and each row
records it in `conditions.effort_requested`; like `--model`, its rows are their own arm.

`--checks-on HOST` runs the landed main's `bun test` and every landed task's acceptance command on HOST inside `depbox run` (no network, home masked) instead of on the laptop, after pushing landed main to a scratch origin there; `conditions.landed_checks_on` names where they ran.

`--parent SESSION` (default `$NORTH_FLEET_SESSION`) names the live fleet session that reads the workers' reports;
north refuses a worker launched from outside a session without one. The scratch lander's queue lives under the live
`land-queue` directory; `eval-b queue --archive DIR` copies it into DIR and removes it, and the same queue on each host,
only when no ticket waits in it and no lander holds it.

Every worker launches with `--gates-aside`: a decision gate it opens (the privacy-refusal task opens one on purpose)
stays out of the owner's queue, digest and `north gate list`, and the runner writes the session's gates beside its
record as `gates.json` (from `north gate list --aside --json`) before it forgets the session. Launch, placement,
hand-in and landing are unchanged; the parent still gets the gate's notice.

A task lands when the lander's landing span for its branch carries a landed commit; seconds to land run from the
launch call to that span's end, and queue wait is the span's `queued` child.

A hand-in is a Bash call running `safe-push --to main` or a call of the `mcp__north__hand-in` or `mcp__north__finish` tool
(`handins_by_tool` counts the tool calls). A row's `conflict` is a refusal for a conflict, before the queue (`cannot
queue the lane: it conflicts`) or by the lander; `reworks` counts hand-ins after such a refusal. Hand-ins a hook
stopped (`hook_blocks`: blind staging, masked pipeline, other) and fetches that lost the ref lock
(`fetch_lock_failures`) have their own fields and are not rework. `eval-b recount --results FILE` recomputes these
fields of every north row from the transcript in its raw directory.

## T3 Code arm

`eval-b t3-tasks` writes the same tasks in Eval A's task format, and Eval A's own runner drives plain T3 Code one task
at a time (`run --arms t3-vanilla`); `eval-b t3-rows` turns those rows into Eval B rows.

## Split arms on the large tasks

`eval-b large --tasks TASKS_TOML --task ID --workers N --plan PLAN --hosts HOST --account A --work DIR --results FILE [--dry]`
runs north's split arms of EVALS section 3a on one build host, N being 1, 3 or 9. The plain competitor is the
one-worker cell's T3 Code rows, which Eval A runs; this mode runs only north.

- It runs on HOST itself, the first of `--hosts`, because the lander's land checks and the replay below are Cargo runs
  that belong on a build host. Its scratch project is `~/code/eval-b-large`, beside the ladder's: a bare origin whose
  only ref is the task's base commit, fetched from `--repo` (default `/home/tom/code/north/main`), with every earlier
  run's refs, reflogs and objects pruned so no earlier attempt is reachable; the lander's checkout `main`; and one lane
  per box, made from the origin's main just before that box launches.
- `TASKS_TOML` is the frozen `eval-large/tasks.toml`; its directory is `EVAL_LARGE` in each check, and the runner
  refuses a directory that holds `answers.toml`. TOML is read with `Bun.TOML.parse`, as Eval A's `tasks-json` does.
- `PLAN` is what `north work split` writes: an optional `split_s` (the split step's seconds, added to the clock),
  `[interface]` with `text` and `files`, then `[[part]]` entries with an optional `name`, `text` and `files`, the parts'
  files pairwise disjoint. With no part the task runs as one session on the acceptance command.
- The interface box launches first and lands alone; then the parts launch, at most N in flight, until every box has
  landed or the 4-hour stop. Each launch is the ladder's: claude-opus-5-5 at medium, `--pin` to HOST, `--gates-aside`,
  the `Commit message:` line `Do large task ID BOX (Run: STAMP-ID-wN-BOX)`, and the same two scripted nudges.
- The scratch lander (`north land serve`) lands on `.safe-push` committed to the scratch main. With no part its check is
  the acceptance command. With parts it is the acceptance command without its first step, which copies the task's spec
  tests over the tree: those tests name what every box adds, so no box but the last could pass them, and section 3a
  counts a main that is red between two landings.
- The clock runs from the split's start to the last box's landing span end, and counts only when the final main passes
  the acceptance command, run after the run. `main_red` is measured then too, off the clock, by replaying each landing
  commit through the land check it landed on, because a Cargo check of 25 to 30 minutes run during the run would load
  the host it times.
- It appends one `row: "run"` row (`time_to_all_landed_and_correct_s`, null on a failed run, `conflicts`, `reworks`,
  `handins`, tokens, API-equivalent `cost_usd`, `main_red` with `main_replays`) and one `row: "box"` row per box (files,
  `launch_order`, `landing_span`, `landed_commit`, the same counts), each session's transcript in its `raw` directory.
- `--dry` lays out the scratch project, prints the launch plan and appends one dry row per box, with its files, launch
  order, lane, session and brief, launching nothing.

## Rerun

```
B=~/eval-data/eval-b
eval-b workload --seed 20261111 --count 60 --lander-host whiterabbit --out $B/workload
eval-b ladder --workload $B/workload --level 1 --count 5 --parent w-eval-b --work $B/work --results $B/results.jsonl
eval-b ladder --workload $B/workload --level 5 --parent w-eval-b --work $B/work --results $B/results.jsonl
eval-b ladder --workload $B/workload --level 21 --parent w-eval-b --work $B/work --results $B/results.jsonl
eval-b t3-tasks --workload $B/workload --count 20 --out $B/t3-tasks.json
~/eval-a/evals/a/run --tasks $B/t3-tasks.json --arms t3-vanilla --runs 1 --account d --results $B/t3-eval-a.jsonl
eval-b t3-rows --workload $B/workload --from $B/t3-eval-a.jsonl --results $B/results.jsonl
eval-b summary --results $B/results.jsonl
eval-b queue --archive $B
```
