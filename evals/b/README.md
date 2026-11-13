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
2. Starts a scratch lander (`north land serve --repo ~/code/eval-b/main`, a child of the runner), which pulls each host's outbox like the production landers do.
3. Launches N workers at once through `north fleet launch` (claude-opus-5-5 at medium, `--prompt north`, pinned with `--pin` to
   the first host of `--hosts`, so every arm runs on one machine) once that host has at most two jobs queued in its capacity admission (placement refuses a pinned launch at four); the wait is polled every 2 s for at most five minutes and counts in each task's time. It keeps N in flight until the level's tasks are all resolved, and gives each stalled worker at most
   two scripted nudges as Eval A does.
4. Collects each session's record, spend and transcript, stops and forgets it, checks the landed main, writes one
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

## Rerun

```
B=/tmp/north-assessment/evals/b
eval-b workload --seed 20261111 --count 60 --lander-host whiterabbit --out $B/workload
eval-b ladder --workload $B/workload --level 1 --count 5 --parent w-eval-b --work $B/work --results $B/results.jsonl
eval-b ladder --workload $B/workload --level 5 --parent w-eval-b --work $B/work --results $B/results.jsonl
eval-b ladder --workload $B/workload --level 21 --parent w-eval-b --work $B/work --results $B/results.jsonl
eval-b t3-tasks --workload $B/workload --count 20 --out $B/t3-tasks.json
/tmp/north-assessment/evals/a/run --tasks $B/t3-tasks.json --arms t3-vanilla --runs 1 --account d --results $B/t3-eval-a.jsonl
eval-b t3-rows --workload $B/workload --from $B/t3-eval-a.jsonl --results $B/results.jsonl
eval-b summary --results $B/results.jsonl
eval-b queue --archive $B
```
