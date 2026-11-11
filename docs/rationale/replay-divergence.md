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
  verdict either, no commit between them caused it and holding the link gains nothing: the link
  follows and the proxy is told to file the flaky test (2026-11-07: holding on b96d3b9e for 39
  minutes after north-798 failed on the candidate and again on the held runtime, north#634). When
  the held runtime fails the round the same way but the candidate adds failures the held runtime
  lacks, those failures are the candidate's own: the link stays and the proxy is told. Otherwise
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

## A replay with no verdict is no divergence

A candidate replay that ends transient (the check environment broke, a host did not answer) has
said nothing about the candidate, so `north land promote` neither holds the link nor bisects on
it when it is the only evidence. When no round diverged and at least 2 rounds matched, the
promotion reports each transient round and moves the link: on 2026-11-08 all 19 promotions held on
one to three transient replays of 8 while 4 to 7 rounds matched, under slot contention, and the
lander ran 3efa5a3d for an hour after newer lander fixes landed (north#842). A replay that exits on an error (on 2026-11-08 a git index.lock another replay held in the one
shared `lander-replay` worktree) is no verdict too, and each replay now takes the first free of four
worktrees, `lander-replay` and `lander-replay-2` to `-4`, under a lock beside it, so two replays
never share an index. With fewer than 2
matched rounds it writes `~/.local/state/north/lander-unreached` with the candidate and the count of
promotions in a row that reached no verdict, exits 75, and the next promotion retries; a promotion
that settles clears the record. At about 4:10 PM on 2026-11-06 a replay of round
n656-adopted-capture failed because `mkdir -p` on gex45 did not finish within 20 s. The judge read
that as a divergence, bisected it to 9d67ceebd and fc091653d, queued a false revert and held the
link. A transient replay of a round recorded transient still matches, and the shadow pass still
counts a repeated transient as an error, so a candidate that breaks the check environment for
every lane is held there. The lander doctor retries a no-verdict promotion through
`north runtime promote` and pages the proxy at the third in a row
(docs/rationale/lander-doctor.md).

## The link moves first (2026-11-10, north#902)

`north land promote` used to replay and shadow before it moved the lander link, so the link waited out
8 recorded rounds plus the shadow on every promotion. Measured over one hour on 2026-11-10,
the link read 4 landings behind 5 times and then 14 landings and 27 minutes behind while 35 landings
an hour reached main; every round in `runtime-follow.log` lines 272204 to 272211 failed on both the
candidate and the held runtime ("the held runtime replays it the same way"), so that hour held nothing
back and cost two replays per round. Land-first applies to the link as it does to a lane: the link moves
to the runtime `current` names as soon as that runtime's canary passed, and the replay and shadow judge it
afterwards. A divergence the held runtime does not share moves the link back to the held runtime and names
the candidate in `lander-refused`, so promote leaves it off until current moves on; the doctor's rollback
writes the same record. A replay that reaches no verdict leaves the link on the candidate and exits 75 for
the next promotion, since it says nothing about it; a repeated shadow error rolls the link back and
refuses the candidate. The canary gate lives in `north runtime promote`: the settling host follows the
link from the spine walk once it passed, and a host that takes a shipped runtime follows at its swap,
since that runtime only arrives after its canary passed on the settling host.


## The replay runs in a unit of its own (2026-11-12, north#1183)

With the link already moved, nothing but the lander's own judgement waited on the replay, yet
`north land promote` still ran it inline. The ordered promotion of ab62f49061 on 2026-11-12 went live
on the settling host at 12:28:38 AM; its remote hosts had already ended within ten seconds of the
build, and their lines, the order's exit and the return of the promotion hold then waited on eight
sequential replays of nine to twelve minutes each, until `north runtime promote` killed the replay at
its 30-minute bound at 12:58:49 AM. The kill came before any verdict, so the lander served the new build
unjudged and the next promotion of that commit found the link already on it. Now `north land promote`
moves the link, starts `north-lander-replay.service` with `north land promote --judge CANDIDATE --held
RUNTIME`, and returns. The unit replays every recorded round, judges, rolls the link back on an
untolerated divergence and pages exactly as the inline replay did, logs its verdict to
`~/.local/state/north/lander-replay.log`, and is bounded by no caller, so the guard always reaches a
verdict. A newer promotion stops the unit before it moves the link, as it stopped an older inline replay.
When the unit cannot start, the promotion runs the replay itself, and `--dry-run` always does.
