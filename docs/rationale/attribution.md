# Attribution: which side each worker outcome belongs to

A worker run can fail because of the worker or because of the harness that staffed, placed, landed or gated it. Counting both as the worker's failure hides which harness fixes would return the most work, and it misleads tier learning about which model fits which work (north#706). This note defines one attribution that every reader shares, so the ready bar, `north ledger flow` and tier learning count the same failures the same way.

## The rule

`north_threads::threads::attribution::attribute` reads a finished run's outcome, the cause a boundary recorded for it (below), its final report and its compaction count, and returns one cause:

- **worker**: every Done, `Stop: reasoning` or `Stop: leash` (the worker ran out of approach or budget, whatever else the report mentions), and any failure no rule below claims.
- **tom**: halted or paused by Tom, `Stop: decision` or `Blocked:`. Neither side failed.
- **harness**, the first class whose rule the report matches, in this order:
  - placement: `Stop: host`, or the report says this host or machine cannot run, push, land or reach what the box needs.
  - access: a GitHub, ssh, token or credential call failed, expired or could not decrypt.
  - capacity: `Stop: capacity|client|budget`, a usage or rate limit, or capacity admission deferred the run.
  - waits: `Stop: wait until`, or the box was staffed while blocked on an unlanded issue or waiting on a date, a clock or a full day of readings.
  - false revert: the lander, the after-landing gate or red-main reverted the worker's landing.
  - duplicate: the work was already on main, already claimed, owned or overtaken by another session.
  - landing refusal: `Stop: landing`, the lander rejected the lane or safe-push refused it.
  - gate refusal: a gate, guard, hook or judge refused or blocked the work.
  - compaction: the report says the run compacted or hit its context limit, or the run compacted and its last message carries no `Done:`, `Not done:`, `Split:` or `Blocked:` opener, so it was parked rather than finished.

## Boundary records first

Each boundary of the harness that touches a session writes its own cause for that session with `north cause record --session NAME --source BOUNDARY --cause CAUSE --reason TEXT` (the `boundary_causes` table). The ingest records each run's fleet session name (`runs.fleet_name`), and `attribute` takes the latest cause recorded under that name for any run that did not finish Done; the phrase rules above apply only to a run no boundary names.

- lander: a failing check or a conflict in the lane's own lines is worker-caused; a deferral, a hang, a transient error or an origin that refused the push is a landing refusal.
- dispatch: a launch that failed to place (a host that would not take it) is placement. The launch governor's staffing refusal is a gate refusal, and a launch forced beside a holder of its item is a duplicate.
- capacity: a fleet session's build that ends with no verdict (exit 75) is capacity.
- fleet: a turn that compacted and ended without a standard opener is compaction.

`north ledger flow` prints, beside the costliest harness classes, the share of the last 24 h of usage whose failed runs have neither a record nor a matching phrase rule ("unattributed"). A rule that reads the wrong side shows up as a class whose count moves the wrong way on `north ledger flow`, and the phrase rules are tuned against reports the ledger holds. Runs recorded before the ingest kept a fleet name stay on the phrase rules.

## Where it is read

- `north ledger scoreboard` has the bar "first tries lost to the harness": the share of worker first attempts over 24 h whose cause is a harness class, with the `ready-harness-loss` ceiling of 1% in goals.toml.
- `north ledger flow` prints the five harness classes of the last 24 h that cost the most API-equivalent usage, with their run counts. The proxy staffs the next harness fix from the first class on that line (harness/claude/agents/proxy.md).
- Tier learning (`north ledger worker --recommend`, ETA calibration and tier predictions, north#705) treats a harness- or Tom-caused outcome as unjudged, so only worker-caused outcomes count toward a tier's fitness for a shape.
- The nearest-box replay (`north dispatch route-replay`, north#705) reads the same cause through `nearest::cause`, which maps it onto kept, worker and harness and counts Tom's halts and decisions with the harness.
