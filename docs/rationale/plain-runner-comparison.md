# Plain-runner comparison: north against T3 Code on small tasks

North's time from launch to a change on main, against T3 Code's plain runner and T3 Code given the task's file text, on the same small tasks with the model held equal; the bar was registered in north#1239 before the runs, and this page gives the method and the three recorded runs.

Status (2026-11-13): draft. The medians below are the ones north#1239 records; the intervals and the row counts per cell are not computed yet, and the bar is not yet called met (see [Status against the bar](#status-against-the-bar)). Every run below is of the runtime of 2026-11-12 with north's sessions on the control host, and none is a reading of the release runtime: in the one paired run on it so far (2026-11-13 and 14, every session on a build host) north took longer than T3 Code at one, five and ten at once. That reading, its cause and the rerun after the fix replace this draft together.

## Question

On a small task, with the model and effort the same in every arm, how long does each arm take from launch to the change landed on main? The registered bar is that north's median is at most 0.67 of the comparison arm's median at each of two load levels.

## Method

- **Arms.** North, a normal worker launch. T3 Code, plain: its server and its own agent loop with the task's brief as the thread's text. T3 Code, with file text: the same, with the task's file text pasted into the first message. (The file-text arm is what the registration calls the plain competitor "given the same file text".)
- **Model and effort.** The same in both arms of a row, as each run's records state.
- **Account.** One subscription account for every row of the three runs.
- **Load levels.** One worker, and five workers at once. Every level-1 task in the 4:36 PM run took one model request, and at level 5 three tasks took one request and two took two.
- **Load gate.** A run counts only when the operator's laptop's one-minute load is below 6 and the control host's is below 8 at each level. The gate limits the load; it does not hold it fixed.
- **Clock.** From launch to landed, as each run's rows record it. The statistic is the median of each cell, with the 95th percentile beside it.
- **Ratio and bar.** North's median divided by the arm's median, at each level; the bar is 0.67 or less against both arms.
- **Noise rule.** Three runs of an unchanged setup before the bar is called met (north#1239, the 4:36 PM run's note).
- **Rows.** Each run writes one row per task and arm to a row file on the control host. The row files are not in this repository.

## Results

Each run wrote 30 rows, all of which landed; the lander refused none of north's. The split of those rows across arms and levels is in the row files.

| Run | Level | North | T3 Code, plain | Ratio to plain | T3 Code, file text | Ratio to file text | Bar met (plain / file text) |
|---|---|---|---|---|---|---|---|
| 1:00 PM, 2026-11-12, order 29 (runtime d21582cf30) | 1 worker | 17 s (p95 18) | 19 s (30) | 0.89 | 19 s (23) | 0.89 | no / no |
| same | 5 at once | 17 s (19) | 22 s (24) | 0.77 | 20 s (24) | 0.85 | no / no |
| 1:29 PM, 2026-11-12, order 29 (runtime d21582cf30), each brief proposes its commit message | 1 worker | 14 s (20) | 18 s (19) | 0.78 | 14 s (23) | 1.00 | no / no |
| same | 5 at once | 15 s (16) | 23 s (26) | 0.65 | 22 s (23) | 0.68 | yes / no |
| 4:36 PM, 2026-11-12, order 31 (runtime d8b8c6bae291), first run under the load gate | 1 worker | 11 s (13) | 21 s (32) | 0.52 | 17 s (19) | 0.65 | yes / yes |
| same | 5 at once | 10 s (12) | 19 s (23) | 0.53 | 19 s (23) | 0.53 | yes / yes |

An earlier order-25 run is recorded in north#1239 only as its ratios to plain, 0.96 at one worker and 0.88 at five; it has no medians in this note.

Where north's time goes, at level 1 in the 4:36 PM run, is the median seconds after launch: first request 2.4 s, finish call 8.2 s, checks done 10.3 s, ticket queued 10.3 s, landed 11 s. The largest part left is the model's own work between the first request and the finish call, about 5 to 6 s.

## Status against the bar

The 4:36 PM run is the only one of the three that passes at both levels against both arms. The 1:00 PM run fails at both levels against both arms, and the 1:29 PM run passes only at five workers against plain. The first two ran under the same runtime as each other, and the third under a later one, so the rows are not one setup.

The noise rule needs three runs of an unchanged setup before the bar is called met. The setup of the 4:36 PM run has one run, so the bar is not yet called met.

## Intervals (not yet computed)

No 95% interval is given here. The intervals need the row files, which are on the control host and were not readable from the sandbox that wrote this draft on 2026-11-13. The method is the one [frontier](frontier.md) states: a percentile bootstrap over each cell's rows, 10,000 resamples, a fixed seed. Until the intervals and each cell's row count are added, every number above is a median from the run's recorded comment, not a measured interval.

## What this does not show

- Small tasks from the ladder's own set. No large task, no bug fix and no grading of the change's quality beyond the checks a landing already runs.
- One model and effort, one subscription account and one control host.
- Three runs, one of which counts toward the bar.
- The load is gated, not held fixed: the laptop may run up to just under 6 and the control host up to just under 8, and the rows carry each run's load rather than a single value.
- The clock starts at launch for each arm. The recorded comments do not say whether the plain arm's client was already running at launch, so any warm-up cost the plain arm paid is outside these numbers.
- Anything about T3 Code beyond these tasks, or about north's whole workload.

## What would settle it

- The intervals and row counts, from the row files on the control host.
- Two more runs of the 4:36 PM setup under the load gate, to reach the three runs the noise rule needs.
- A run on a second account, so one account's limits are not part of every row.
