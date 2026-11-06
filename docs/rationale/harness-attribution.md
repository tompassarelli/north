# Harness attribution: every finished worker run has one cause, and harness fixes go in Pareto order

Decided 2026-11-07 under north#706. On 2026-11-06 and 07 most worker first-try failures came from the harness:
placement on a host that cannot do the box, landing refusals and false reverts, workers parked after compaction,
duplicate dispatch, and new gates refusing correct work. Fixes were picked by whichever alarm arrived next. north#705's
14-day replay put harness-caused runs at 31% of the window's API-equivalent usage, against 24% for worker-caused ones.

## One attribution, read by everything that judges a run

`north_threads::threads::calibration::attribution` gives each finished worker run one cause: kept, the worker's
(`missed`, `reworked`, `reasoning`, `leash`, `refused`) or the harness's, in one class of `placement`, `landing`,
`false-revert`, `gate-overturned`, `capacity`, `compaction`, `wait` and `duplicate`. A box waiting on a decision from
Tom is `decision`: not the worker's, so tier learning leaves it out, and not a class a harness lane removes, so the loss
share and the Pareto list leave it out too.

The order, first match wins:

1. A split re-scopes the box and has no cause; a kept run (Done, not re-run within 24 h, landed when a landing was
   expected) is kept.
2. Records, which a report cannot argue with:
   - **duplicate**: another worker session started the same item first and was still running when this one started;
   - **lander incident rows** on a lane whose branch names the box's number, detected while the run ran: an
     incident the proxy recorded under a harness class name (`north land withdraw --class false-revert`) is that class;
     a refusal whose same tip later landed was overturned, `false-revert` when its reason names a revert and
     `gate-overturned` otherwise; a hang, transient, deferral, conflict or hold is `landing`; a refusal nothing
     overturned is the lane's own failing checks, `refused`, the worker's.
3. The report's own `Stop:` word: `reasoning` and `leash` are the worker's; `landing`, `capacity`, `host` and `client`
   (`placement`), `wait` and `decision` are not.
4. Phrase rules on the report's opening paragraph only, the `Not done:` or `Blocked:` line, so a worker failure whose
   later text mentions the lander in passing is no longer excused.
5. A Done that was re-run is `reworked`; an unclear or handed-off end with an empty report after a compaction is
   `compaction`; any other unclear end is unattributed; every other end is `missed`.

`north dispatch route-replay` (north#705) reads its causes from this function, so the tier learning and the scoreboard
cannot disagree about a run. `nearest::cause` is gone.

## Where it shows

- `north scoreboard --ready` has the bar `first tries lost to the harness`: of the first attempts with a cause over the
  newest 24 h, the share the harness lost, at most 10% (`ready-harness-loss` in goals.toml).
- `north flow` prints `harness causes, 24 h:` with the top five classes, each with its runs and their API-equivalent
  dollars, and the total. The proxy staffs harness fixes from that line in order (harness/claude/agents/proxy.md).

## Known limits

An incident links to a run through the issue number in its lane's branch and the run's time span, because the lander
records the branch, not the session; a branch that names no number links nothing, and two boxes with the same number in
different repositories are told apart only when the incident row names its repository. A refused lane counts as
overturned only when the same tip later landed, so a gate fixed and the lane rebased reads as the worker's refusal
until the proxy records the withdrawal under `gate-overturned`.
