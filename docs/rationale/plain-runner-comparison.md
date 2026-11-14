# Plain-runner comparison: north against T3 Code on four kinds of work

North against plain T3 Code with the model held equal, on small tasks at one to twenty at once, single real tasks, large tasks split across workers and an outside benchmark: for each kind the tasks completed, the time and the cost of each arm, the line it is read against, and where its rows are kept.

Status (2026-11-14): draft, not approved for publication. On the release runtime, after the hand-in stage that cost north about 180 s was fixed, the valid runs give R 0.79 in the small one-worker cell (one run) and 0.64 in the small five-at-once cell (two runs, 0.65 and 0.61) against the faster T3 Code arm, at about 0.3 of its cost per task. Every cell is undecided until T3 Code's noise band is measured and has three valid runs; the large cells, which the general claims need, have not run.

## The line

Written on north#1281 before the release runs; a kind that fails is fixed at its cause and run again, or stated here as a loss, and no line moves to fit a number.

- Alone, north is no worse: on single real tasks and on the outside benchmark it completes at least as many tasks as plain T3 Code and is at most 10 percent slower.
- At scale it is clearly better: on small tasks at five, ten and twenty at once it completes at least as many and is at least 20 percent faster, and on large tasks it meets line C3 registered in north#1236 (at its best worker count at least 20 percent faster on three of five tasks and more than 10 percent slower on none, equally correct).
- The small-task bar registered in north#1239 holds on the release runtime: north's time is at most 0.67 of the stronger T3 Code arm's and outside that arm's noise band.

## Standing by kind

| Kind | Completed, north / T3 Code | Time, north / T3 Code | Cost per task, north / T3 Code | Against its line | Runtime and runs | Rows |
|---|---|---|---|---|---|---|
| Small tasks, one worker | 15 of 15 / 15 of 15 | run medians 9 to 11 s / 15 to 17 s; R 0.65 | $0.026 to $0.040 / $0.135 to $0.205 | 0.67 bar met, low confidence | 2026-11-12 (order 31), 3 runs | lost; figures as recorded |
| Small tasks, five at once | 15 of 15 / 15 of 15 | run medians 10 s / 17 to 22 s; R 0.51 | as above | met: 49 percent faster | 2026-11-12, 3 runs | lost; figures as recorded |
| Small tasks, ten at once | 10 of 10 / 10 of 10 | all landed 37 s / 46 s (0.80); median 15 s / 20.5 s (0.73) | $0.049 / $0.173 | at the line on time to all landed, met on the median | 2026-11-12, 1 run | lost; figures as recorded |
| Small tasks, twenty at once | 20 of 20 / 19 of 20 | all landed 60 s / 63 s (0.95); median 17 s / 31 s (0.55) | $0.050 / $0.174 | loss on time to all landed; met on the median and on tasks completed | 2026-11-12, 1 run | lost; figures as recorded |
| Small one-worker cell, release runtime after the hand-in fix | 5 of 5 / 5 of 5 | median 15 s / 18 s; R 0.79 (80% 0.71 to 0.83) | $0.050 / $0.172 | undecided: one valid run, no noise band; the 0.67 line is not met | 2026-11-14 | kept, and committed as docs/readme/comparison.toml |
| Small five-at-once cell, release runtime after the hand-in fix | 10 of 10 / 10 of 10 | median 13.5 s / 21 s; R 0.64 (80% 0.60 to 0.67) | $0.054 / $0.191 | undecided: two valid runs, no noise band | 2026-11-14 | kept, and committed as docs/readme/comparison.toml |
| Small tasks, one worker, release runtime | 5 of 5 / 5 of 5 | median 199 s / 17 s | $0.072 / $0.155 | loss: 11.7 times T3 Code's time, against a bar of 0.67 | 4903316be5, 2026-11-13, 1 run | kept |
| Small tasks, five at once, release runtime | 5 of 5 / 5 of 5 | all landed 197 s / 27 s; median 197 s / 24 s | $0.072 / $0.186 | loss: 7 to 8 times T3 Code's time, against at least 20 percent faster | 4903316be5, 2026-11-14, 1 run | kept |
| Small tasks, ten at once, release runtime | 8 of 10 / 10 of 10 | all landed 201 s (the eight) / 66 s; median 199.5 s / 32 s | $0.102 / $0.196 | loss on time; the two tasks not landed were lost to the test setup, not to north, so tasks completed is not read | 4903316be5, 2026-11-14, 1 run | kept |
| Small tasks, twenty at once, release runtime | not run | | | not run | | |
| Single real tasks | 10 of 10 / 10 of 10 | median 97 s / 89 s; median ratio over tasks 1.09 (0.30 to 1.81) | $0.61 / $0.50 | at the line, low confidence; an earlier set the same day read 1.34 | 2026-11-11, 5 tasks, 2 runs per arm | kept |
| Single real tasks, release runtime | 1 of 1 / 2 of 2 | 249 s / 42 and 45 s | not read | loss on the one task run; the pilot was stopped there | 4903316be5, 2026-11-13, 1 task | kept |
| Large tasks | not run | | | not run | | |
| Outside benchmark | 9 of 12 / 9 of 12, then 5 of 8 / 5 of 8 | second slice, the five tasks both solved: 297 s / 330 s in sum | not read | equal on tasks completed; uncounted | 2026-11-12 and 13, probe slices | kept |

T3 Code's figures in the small-task rows are those of its stronger arm for that measure. R is defined under the method below.

## Small tasks

### Method

- **Arms.** North, a normal worker launch. T3 Code, plain: its server and its own agent loop with the task's brief as the thread's text. T3 Code, with file text: the same, with the task's file text pasted into the first message. The bar is judged against the stronger of the two T3 Code arms.
- **Model and effort.** claude-opus-5-5 in every arm; north at medium effort; T3 Code 0.1.8, which has no effort setting. One subscription account for every session of every arm; no API key.
- **Tasks.** Small tasks from a synthetic repository that `evals/b` generates from a seed (each turns on one prepared, skipped test for a small string function). Every third task also registers its function in one shared file, so lanes conflict there at ten and twenty at once. Each task text carries a marker not seen before, so no landing reuses an earlier verdict.
- **Load.** The registered rule is a build host under a one-minute load of 8 at each level's start. Every row carries the loads of the hosts it touched.
- **Statistic.** R is the median over tasks of north's task mean over T3 Code's task mean across the runs. The noise band N is the median over tasks of T3 Code's slowest over fastest time across the runs; outside the band means R below 1/N. The 80 percent interval is a bootstrap of 10,000 resamples over tasks and over runs within tasks, with a fixed seed.
- **Cost.** From each session's token counts at list price. No API key was set; the figures are what the usage would cost at list.

### Runtime of 2026-11-12 (order 31, d8b8c6bae291), three runs at one and five

Against the stronger arm (T3 Code with file text):

| Level | R | 80% interval | N | 1/N | At or below 0.67 | Outside the band |
|---|---|---|---|---|---|---|
| 1 worker | 0.65 | 0.54 to 0.68 | 1.31 | 0.76 | yes, low confidence (the interval's top crosses 0.67) | yes |
| 5 at once | 0.51 | 0.48 to 0.58 | 1.28 | 0.78 | yes | yes |

Against plain T3 Code:

| Level | R | 80% interval | N | 1/N | At or below 0.67 | Outside the band |
|---|---|---|---|---|---|---|
| 1 worker | 0.50 | 0.46 to 0.60 | 1.40 | 0.71 | yes | yes |
| 5 at once | 0.51 | 0.47 to 0.55 | 1.50 | 0.67 | yes | yes |

Median seconds per run (north / plain / file text): at one worker 11 / 21 / 17, 10 / 15 / 15 and 9 / 19 / 17; at five 10 / 19 / 19, 10 / 19 / 17 and 10 / 23 / 22. Every cell landed 5 of 5 in every arm, north's hidden check passed 5 of 5 in every cell, and no arm needed an intervention.

Where north's time went at one worker, as median seconds after launch: first request 2.4 s, finish call 8.2 s, checks done 10.3 s, ticket queued 10.3 s, landed 11 s.

Ten and twenty at once ran once each, with three and six tasks on the shared file. North landed 10 of 10 and 20 of 20; each T3 Code arm landed 10 of 10 and 19 of 20, losing one task to a conflict after three hand-ins. North's time to all landed is set by the lanes that share one file and hand in again after a conflict (33 to 37 s at ten, 34 to 60 s at twenty); its other lanes landed in 14 to 19 s.

Two runs of an earlier runtime failed the bar, and one before that read 0.96 at one worker and 0.88 at five. They are not part of the figures above.

### Release runtime, 2026-11-14, after the hand-in fix

Valid runs on 2026-11-14 on the runtime that carries the fix to the hand-in stage described in the next section: one run of the small one-worker cell (runs 2 and 3 of that cell are void under the protocol's load rule and are not counted) and two runs of the small five-at-once cell. Every session of all three arms ran on one build host whose one-minute load was under 2 at each start; north's land checks ran on the operator's own machine, as in a normal fleet. Tasks t01 to t05 of the ladder, the same task text in every arm, claude-opus-5-5 at medium effort in north and T3 Code 0.1.8's default in its arms. Every counted row landed and north's hidden check passed on every task.

The counted rows are committed as [docs/readme/comparison.toml](../readme/comparison.toml), one line per task, arm and run with its seconds, tokens and list-price cost, so every figure below can be recomputed.

| Cell | Arm | Landed | Median | Cost per task | Tokens per task |
|---|---|---|---|---|---|
| small one-worker | north | 5 of 5 | 15 s | $0.050 | 16,289 |
| small one-worker | T3 Code, with file text | 5 of 5 | 18 s | $0.172 | 227,526 |
| small one-worker | T3 Code, plain | 5 of 5 | 20 s | $0.171 | 234,367 |
| small five-at-once | north | 10 of 10 | 13.5 s | $0.054 | 22,984 |
| small five-at-once | T3 Code, with file text | 10 of 10 | 21 s | $0.191 | 214,914 |
| small five-at-once | T3 Code, plain | 10 of 10 | 22.5 s | $0.191 | 241,247 |

R against the faster T3 Code arm, with its 80 percent interval (10,000 resamples over tasks and over runs within tasks, seed 20261114):

| Cell | Valid runs | R per run | R, all valid runs | 80% interval | Reading |
|---|---|---|---|---|---|
| small one-worker | 1 | 0.79 (with file text) | 0.79 | 0.71 to 0.83 | undecided: T3 Code's noise band is not measured; the 0.67 line is not met |
| small five-at-once | 2 | 0.65 (plain), 0.61 (with file text) | 0.64 | 0.60 to 0.67 | undecided: T3 Code's noise band is not measured |

Each reading holds for its cell alone. The protocol reads a cell only after its third valid run and against T3 Code's noise band, and its general sentences (north ahead of T3 Code on time or on cost, at one worker or at five) need the large cells too, which have not run; so nothing here says north is ahead in general.

Where north's time goes now: the first model request 8 to 18 s after launch, the hand-in 2 to 5 s after that, then a queue wait under 1.5 s in all but one task. North's worker makes one or two model requests from a short brief, which is where its lower token count and cost come from.

### Results north did not meet

Losses are reported beside the wins:

- 2026-11-12, the same tasks on one build host for every arm: north's median against the faster T3 Code arm was 1.0 at one worker (13 s against 13 s) and 0.83 at five at once (15 s against 18 s); the 0.67 line was not met at either level, and those verdicts came from a cache.
- 2026-11-12, small model first on a mixed workload of ten tasks: not met on every line. North landed 7 of 10 correct against 10 of 10 for each plain T3 Code arm, its time per correct task was 4.86 times the small-model arm's, its cost per correct task 0.75 of it, and the task set did not turn out mixed, so the run claims no pass line.


### Release runtime (4903316be5), 2026-11-13, before the hand-in fix

One run per cell on 2026-11-13 and 14, every session of all three arms on one build host that was also carrying other fleet work. The registered load rule (a one-minute load under 8) was not met: the load was 21, 21 and 16 at the three level starts, recorded on each row and sampled every 15 s through the run. Every north launch was held to that host, and any wait for room there counts in north's time; none was needed.

| Level | Arm | Landed | Median | All landed | Cost per task |
|---|---|---|---|---|---|
| 1 worker | north | 5 of 5 | 199 s | 208 s (slowest task) | $0.072 |
| 1 worker | T3 Code, with file text | 5 of 5 | 17 s | 19 s (slowest task) | $0.155 |
| 1 worker | T3 Code, plain | 5 of 5 | 19 s | 38 s (slowest task) | $0.176 |
| 5 at once | north | 5 of 5 | 197 s | 197 s | $0.072 |
| 5 at once | T3 Code, with file text | 5 of 5 | 24 s | 28 s | $0.186 |
| 5 at once | T3 Code, plain | 5 of 5 | 25 s | 27 s | $0.206 |
| 10 at once | north | 8 of 10 | 199.5 s | 201 s (the eight) | $0.102 |
| 10 at once | T3 Code, with file text | 10 of 10 | 34.5 s | 77 s | $0.196 |
| 10 at once | T3 Code, plain | 10 of 10 | 32 s | 66 s | $0.203 |

North's hidden check passed on every task it landed. With one run per cell there is no R, noise band or interval; the ratios in the standing table are of medians. They are far outside any band the earlier runs showed.

Where north's time went, the same in all 20 tasks: the first model request at 10 to 15 s, the hand-in called within seconds of that, then 174 to 184 s inside the hand-in before the ticket reached the queue, then under 13 s of queue and landing. The hand-in's own timing line puts 179.9 s of a 180.3 s hand-in in one stage, the check that a change carries no private text; its verdict came from a cache, and every other stage took under a second. On the control host the same stage takes under 2 s and the same tasks landed in 9 to 10 s (five north-only tasks, the evening before, four of them placed there). A stage that takes the same 180 s every time behaves like a limit being waited out; the cause is not yet found and is with the lead who owns the landing path. Until it is fixed, north on a build host loses to T3 Code on time at every level, and this is stated here as the loss it is.

Two smaller things the runs found:

- In 16 of the 20 tasks the worker's first hand-in call was refused on the build host and it handed in again by the second route, a round of about 10 s. The defect's fix is on main and not yet in the release runtime.
- At ten at once, three tasks edit one shared file. One landed; the other two were refused for the conflict and could not rebase, because the ladder's own setup had stopped copying each landing to the build host. That is a fault of the test, fixed in the runner since, so north's 8 of 10 is not a reading of how north handles conflicts. T3 Code's arms landed 10 of 10, two lanes each after a second hand-in.

Before these runs the ladder could not start as it stood: two changes made to north since the earlier runs stopped its scratch project before any timing, and both were fixed in the runner and its setup. Neither is part of what the comparison measures.

Twenty at once did not run: the host was held for a fixed span and the three levels above used it.

## Single real tasks

Five tasks from a private game repository, each a real landed change with its own acceptance check and hidden checks, two runs per arm, one at a time, on 2026-11-11 on the control host. Both arms merged 10 of 10 with the hidden checks passing. The median over tasks of north's mean time over T3 Code's was 1.09 (per task 0.30, 0.64, 1.09, 1.61 and 1.81); an earlier set the same day read 1.34. North is behind on the two shortest tasks and ahead on the two longest.

On the release runtime a pilot on the build host ran one task before it was stopped for the small-task ladder: north merged it in 249 s, T3 Code in 42 and 45 s. The excess is the size of the hand-in stage described under the small tasks, so the pilot waits for that fix; running the other tasks first would measure the same 180 s again.

## Large tasks

Five change sets from north's own history, each at least ten files across three modules, frozen with their checks in north#1236 before any run. Nothing has run. The plain arm and north at one worker have a runner; the arms that split a task across three and nine workers do not yet.

## Outside benchmark

Terminal-Bench 2.0, with both harnesses on the host and only tool calls reaching the task's container, so no row is comparable with the benchmark's public table. Two uncounted probe slices ran, to find where north loses before the counted run. On the first twelve tasks north solved 7 and T3 Code 9 at first attempt; after two north causes were fixed and their tasks rerun, 9 and 9. Eight reruns after two north changes gave 5 and 5; on the five tasks both arms solved, north took 297 s in sum against 330 s. The counted run (a noise floor of 45 trials, then 178) has not started.

## Where the rows are

- Small tasks, 2026-11-12: the row files were written under a scratch directory on the control host, which an hourly cleanup removed that evening. What remains is the figures as read from the rows at the time, the scripts that read them, and T3 Code's host-side rows. They are recorded, not recomputable.
- Small tasks, release runtime after the fix: committed in this repository as docs/readme/comparison.toml; the full rows, with each session's transcript, stay in the eval data directory beside the earlier ones.
- Small tasks, release runtime: the control host's eval data directory, outside every cleanup, as `results-north-rel-1113-r5.jsonl` and `results-t3-rel-1113-r5.jsonl` (the paired levels), `results-north-rel-1113-r3.jsonl` (the five north-only tasks), the host's load samples, and each session's transcript and the landing queue's logs beside them.
- Single real tasks: the operator's private repository, beside the runner; the release-runtime pilot row is in the control host's eval data directory.
- Outside benchmark: the eval data directory of the build host that ran it.

## What this does not show

- Three valid runs of each small cell after the hand-in fix and T3 Code's noise band, which the protocol needs before a cell is read.
- Ten and twenty at once after the fix, or how north handles conflicting lanes there.
- Large tasks at all, and the benchmark beyond twenty probe tasks.
- More than one model and effort, or more than one subscription account. The 2026-11-13 rows ran beside other fleet work; the 2026-11-14 rows on a quiet host; each row carries its loads.
- A grading of a change's quality beyond the checks a landing already runs and each task's hidden check.
- Anything about T3 Code beyond these tasks, or about north's whole workload.

## What would settle it

- A third run of one and five at once on the same host and tasks, then ten and twenty.
- The single real tasks on the release runtime after that fix, with where north's seconds go on the short ones.
- The large tasks: the plain arm and north at one worker first, then the split arms.
- The benchmark's counted run on a host held for it.
