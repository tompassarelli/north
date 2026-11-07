# Payback line

Read when staffing a `Category: tooling|infra|ci|harness` box. The pricing method (C, S, R, horizon, thresholds, stop rules) is in the planning skill's "Price an infra box" section; Tom is the operator, his minutes weigh 10x. The brief guard (`crates/north-guard/src/guard/worker_tier.rs`) refuses such a brief without a `Payback:` line and prints its two forms.

## Rate source

`R` is the repository's landings per day: `git log origin/main --first-parent --since=14.days --oneline | wc -l` / 14.

## Landing tempo

The harness outcome number is north's first commit->main p50 over the last 24 hours, target 10 minutes: `north ledger tempo --window 1440` prints it with landed, queued->main p50 and p90, refused alone and queued now, and the hourly `north ledger tempo --record` keeps one row per hour in the ledger table `land_tempo`. A box on the landing path (lander, checks, queue, review, staffing latency) states its `saves <N> min/change` as the minutes it takes off that p50 and its `<C> changes/day` from the 1440-minute `landed` count; after it lands, the next day's rows say whether the saving arrived.
