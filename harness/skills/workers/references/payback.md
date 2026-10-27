# Payback line and iteration ledger

Read when staffing a `Category: tooling|infra|ci|harness` box or refreshing the bottleneck page. The pricing method (C, S, R, horizon, thresholds, stop rules) is in the planning skill's "Price an infra box" section; Tom is the operator, his minutes weigh 10x.

## Rate source

`R` comes from `~/.local/state/agents/iteration-ledger/changes.json`, written by `bin/iteration-ledger`; without it use `git log --since=14.days --oneline | wc -l` / 14.

## Brief line

`src/guard/worker_tier.rs` refuses a `Category: tooling|infra|ci|harness` brief without one of these lines, so the price reaches the worker:

- `Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days`
- `Payback: enables <what>; cost <M> min` for enabling work with no direct saving.

## Direction questions

Ask Tom a direction-ambiguity question through the proxy's picker, because that is where he answers asynchronously; never ask about routine sizing.

## Feature Bottlenecks page

Tom's Feature Bottlenecks page (a Dashboard artifact whose link is in south:agents/skills/claude-accounts/) is refreshed from a Claude session on its tick, because only a Claude session can write the artifact:

1. Run `bin/iteration-ledger`, then `bin/iteration-ledger-page`.
2. Upload `features.json`, `stages.json` and `projects.json` from `~/.local/state/agents/iteration-ledger/page/` as assets of the page.
3. `update` the `datasets/features`, `datasets/stages` and `datasets/projects` documents with the new `source.url` and `updated: {at, by}`.
4. Delete the replaced assets. Never republish the page itself, because republishing changes its link.
