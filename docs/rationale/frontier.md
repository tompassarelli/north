# Frontier scoreboard: north against plain Claude Code on the same boxes

`north ledger scoreboard` set every number against north's own targets or its own earlier week, and [scoreboard.md](scoreboard.md) says no published figure shares its estimand. North#730 (2026-11-07) adds the comparison that can say whether the harness does better than the same model without it: both arms get the same brief, binary, model and effort on a frozen task, and a grader that does not know the arm reads the final tree.

## What shipped

- `crates/north-frontier`: the run row (`docs/rationale/frontier/results.jsonl`, one JSON line per arm and task), the paired metrics and the interval arithmetic.
- `north ledger scoreboard --frontier`: prints the section below from the stored rows.

## What it reads

Only tasks both arms ran, with neither transcript touching the landed commit, enter the headline; the line says how many were left out.

- Correct on first pass, and correct after the runner's continuations, each apart: a Wilson interval per arm; the difference north minus plain with Newcombe's paired interval (method 10) and the exact two-sided McNemar test on the discordant tasks.
- Dollars per correct task: an arm's cost over its correct tasks, the difference with a percentile bootstrap over tasks (10,000 resamples, fixed seed).
- Wall time to a correct result: the median over correct runs with a bootstrap interval, and the median paired difference over tasks both arms passed.
- Interventions per task: the mean with a bootstrap interval.

## Design

The design, its review round and each disposition are in north#730 and its first comment. Its accepted flaws stay in the result text: one model family on per-token billing, north's own code tasks with tests only, no production land checks in either arm, pilot power and white-box test coupling.

## Results

None yet: `results.jsonl` is empty until a first pass runs. A first pass runs the arms on paid Claude Code runs, so it waits for the owner's approval of its cost.
