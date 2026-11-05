# Payback line

Read when staffing a `Category: tooling|infra|ci|harness` box. The pricing method (C, S, R, horizon, thresholds, stop rules) is in the planning skill's "Price an infra box" section; Tom is the operator, his minutes weigh 10x.

## Rate source

`R` is the repository's landings per day: `git log origin/main --first-parent --since=14.days --oneline | wc -l` / 14.

## Brief line

`crates/north-core/src/guard/worker_tier.rs` refuses a `Category: tooling|infra|ci|harness` brief without one of these lines, so the price reaches the worker:

- `Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days`
- `Payback: enables <what>; cost <M> min` for enabling work with no direct saving.

## Direction questions

Ask Tom a direction-ambiguity question through the proxy's picker, because that is where he answers asynchronously; never ask about routine sizing.
