# Replay divergence: a declared change promotes, an undeclared one is reverted

The lander runs from its own runtime link and `north land promote` moves it only when the candidate
runtime replays the latest recorded rounds to the verdicts the lander recorded
(docs/features/04-landing-and-lanes/0015-lander-promote.md). From 10:58 AM to about 2:00 PM on
2026-11-06 that rule held the link on ef71d1f40: cc7710501 made a lane need a cached clippy verdict
on purpose, so every later candidate replayed credentials-clippy and n636-decide-msg as failed where
the held runtime had landed them. Nothing let the change say it was intended, and nothing named the
commit, so every lander fix of that afternoon sat undeployed (north#634).

A mismatch is now a decision with a bound, made in the same promotion run:

- Declared. A commit between the held and the candidate runtime carries
  `Replay-change: land.<kind> <reason>`, where kind is the verdict the candidate now replays
  (landed, failed, conflict, undecided, deferred, transient, red-main, error). When every
  mismatching round replays a declared kind, the link follows, each divergence is logged with the
  commit that declared it, and the proxy is told. `north land enqueue` refuses a malformed trailer,
  so a typo cannot silently fail to declare.
- Undeclared. The judge replays the first undeclared round with the built runtimes in
  `~/.local/state/north/runtimes` whose commits lie on main's first-parent line between the held
  and the candidate runtime, held first. If the held runtime does not reproduce the recorded
  verdict either, no commit between them caused it: the link stays and the proxy is told. Otherwise
  a binary search finds the first runtime that diverges; a runtime that cannot replay is skipped,
  so the search always ends. The commits between the last matching and the first diverging runtime
  that touch the lander's crates (all of them when none do) are reverted through the sheriff's
  queue path, ahead of the queue, with `Sheriff-Reverts:` and `Replay-divergence:` trailers, and
  the proxy is told. A record under the queue's `sheriff/replay-SHA` keeps a later promotion from
  queueing the same revert twice; once the revert lands, the next runtime replays the round as
  recorded and the link follows.

Why bisect over built runtimes rather than build one per commit: a runtime build takes up to 25
minutes under a heavy lease, longer than the 15-minute hold bound; every landed batch already
leaves a runtime behind, so the search costs one replay per probe, about log2 of the runtimes kept.
The cost is precision: when a batch landed several lander commits as one runtime, all of them are
reverted, and each owner re-queues with `Follows:` and the declaration.
