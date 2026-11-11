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

1. Lays out a scratch project at `/tmp/eval-b`: on the laptop a bare origin and the lander's checkout, on every
   build host a mirror origin, a main checkout and one prepared lane per task. The laptop origin's post-receive hook
   pushes main to each host's mirror, standing in for a shared remote.
2. Starts a scratch lander (`north land serve --repo /tmp/eval-b/main`, a child of the runner), which pulls each host's outbox like the production landers do.
3. Launches N workers at once through `north fleet launch` (claude-opus-5-5 at medium, `--prompt north`, placement
   left to north), keeps N in flight until the level's tasks are all resolved, and gives each stalled worker at most
   two scripted nudges as Eval A does.
4. Collects each session's record, spend and transcript, stops and forgets it, checks the landed main, writes one
   row per task to the results file and removes the scratch project from every host.

`--parent SESSION` (default `$NORTH_FLEET_SESSION`) names the live fleet session that reads the workers' reports;
north refuses a worker launched from outside a session without one. The scratch lander's queue lives under the live
`land-queue` directory; `eval-b queue --archive DIR` copies it into DIR and removes it, and the same queue on each host,
only when no ticket waits in it and no lander holds it.

A task lands when the lander's landing span for its branch carries a landed commit; seconds to land run from the
launch call to that span's end, and queue wait is the span's `queued` child.

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
