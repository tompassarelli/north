# Attribution: which side each worker outcome belongs to

A worker run can fail because of the worker or because of the harness that staffed, placed, landed or gated it. Counting both as the worker's failure hides which harness fixes would return the most work, and it misleads tier learning about which model fits which work (north#706). This note defines one attribution that every reader shares, so the ready bar, `north ledger flow` and tier learning count the same failures the same way.

## The rule

`north_threads::threads::attribution::attribute` reads a finished run's outcome, the cause a boundary recorded for it (below), its final report and its compaction count, and returns one cause:

- **worker**: every Done, `Stop: reasoning` or `Stop: leash` (the worker ran out of approach or budget, whatever else the report mentions), and any failure no rule below claims.
- **tom**: halted or paused by Tom, `Stop: decision`, or a `Blocked:` report no harness class claims (a `Blocked:` that ends `Stop: host NAME` is placement). Neither side failed.
- **proxy**: a proxy-role session stopped, forgot, relaunched or interrupted the worker mid-turn; only the fleet's record names it, and `north ledger flow` counts it apart from the harness classes, so its fixes aim at the proxy rather than at workers.
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
  - halted: the fleet stopped the session in the middle of a turn (a stop, forget, relaunch, take-over or retirement); only the fleet's record names it.
- **worker**, from the report itself when no class claims it: a `Split:`, or a report with no `Done:`, `Not done:`, `Split:` or `Blocked:` opener, which is the worker's own call or a report that broke the report protocol.

## Boundary records first

Each boundary of the harness that touches a session writes its own cause for that session with `north cause record --session NAME --source BOUNDARY --cause CAUSE --reason TEXT` (the `boundary_causes` table). The ingest records each run's fleet session name (`runs.fleet_name`), and `attribute` takes the latest cause recorded under that name since the run started for any run that did not finish Done; the phrase rules above apply only to a run no boundary names.

- lander: a failing check or a conflict in the lane's own lines is worker-caused; a deferral, a hang, a transient error or an origin that refused the push is a landing refusal.
- dispatch: a launch that failed to place (a host that would not take it) is placement.
- capacity: a fleet session's build that ends with no verdict (exit 75) is capacity.
- fleet: a turn that compacted and ended without a standard opener is compaction; a stop that cuts a turn short is halted (Tom when he opened the conversation in its interactive UI); a session parked on its account's usage limit or handed over to another account is capacity.

The worker's own `Stop:` line is the fallback: the ingest keeps each report's last `Stop:` line in `runs.stop`, because the ledger keeps only a report's first 600 characters and a long report's closing line fell outside them, and the rules read it after the kept head.

`north ledger flow` prints, beside the costliest harness classes, the share of the last 3 h of usage (the window the readiness bars are judged on) and of the last 24 h whose failed runs have neither a record nor a matching phrase rule ("unattributed"). A rule that reads the wrong side shows up as a class whose count moves the wrong way on `north ledger flow`, and the phrase rules are tuned against reports the ledger holds. Runs recorded before the ingest kept a fleet name stay on the phrase rules.

## Where it is read

- `north ledger scoreboard` has the bar "first tries lost to the harness": the share of worker first attempts over 24 h whose cause is a harness class, with the `ready-harness-loss` ceiling of 0.1% in goals.toml. A first try whose report stops on `Stop: landing` is pending, and left out, while the lander's queue on this host holds an undecided ticket from its session; the verdict wakes the session, whose next report replaces it. A `Stop: landing` with no such ticket counts as a landing refusal. A first try deferred to a dated check that has not missed, which first-try success leaves out, counts here as a waits loss: the box was staffed before it could finish.
- `north ledger flow` prints the five harness classes of the last 24 h that cost the most API-equivalent usage, with their run counts. The proxy staffs the next harness fix from the first class on that line (harness/claude/agents/proxy.md).
- `north graph ingest` charges each class's 24 h usage to the issue `harness_loss_owners` in harness/defaults.toml names as its owner, and `north graph ready` and dispatch rank open issues by that usage per dollar of one of their boxes (north#734, docs/graph.md).
- Tier learning (`north ledger worker --recommend`, ETA calibration and tier predictions, north#705) treats a harness- or Tom-caused outcome as unjudged, so only worker-caused outcomes count toward a tier's fitness for a shape.
- The nearest-box replay (`north dispatch route-replay`, north#705) reads the same cause through `nearest::cause`, which maps it onto kept, worker and harness and counts Tom's halts and decisions with the harness.
