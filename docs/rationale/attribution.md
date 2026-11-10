# Attribution: which side each worker outcome belongs to

A worker run can fail because of the worker or because of the harness that staffed, placed, landed or gated it. Counting both as the worker's failure hides which harness fixes would return the most work, and it misleads tier learning about which model fits which work (north#706). This note defines one attribution that every reader shares, so the ready bar, `north ledger flow` and tier learning count the same failures the same way.

## Mechanical sources only

A bar, an attribution class or a routing score reads only mechanical sources: git ancestry of origin/main, lander verdicts and queue state, exit codes, capacity admission records, registered waits, placement refusals and fleet lifecycle records. Text an agent wrote (a `Stop:` line, a `Done:` or `Not done:` word, the prose of a report, a comment or an issue body) is a hint: it may be shown, searched and quoted, and it never decides a verdict, so a worker's wording cannot move a harness number (north#791; on 2026-11-07 waits and capacity were read from `Stop:` text, host needs were guessed from issue prose, and first tries were scored on what the worker said instead of what the harness recorded).

The phrase rules and report readers below are the fallback that predates this rule. `harness/agent-text-reads.toml` lists every file that still reads agent text to decide a verdict, with the count of lines that do; each box of north#791 replaces one kind with its mechanical source and lowers its entry. `tests/it/agent_text_reads.rs` scans the workspace sources and refuses any line that reads a report beyond that list, so a new read cannot land while the old ones are replaced; the same test plants a read in a new file and in a listed file and checks that both are refused.

## Inventory of 2026-11-07

Measured at 06:14 UTC against the ledger and graph (north#791, the inventory comment n-physics-review posted there); each input is tagged mechanical, structured GitHub, mixed or agent text, and the files that read agent text are the entries of `harness/agent-text-reads.toml`.

- Scoreboard, 11 bars: 5 mechanical, 1 structured GitHub, 2 mixed, 2 agent text (first try, harness loss), 1 model-judged. The first-try bar read the run outcome from the report opener (`src/cli/scoreboard/workers.rs`, `outcome.rs`).
- Attribution (`attribution.rs`), over the 181 worker runs that finished from 23:00 to 06:14 UTC, when every run carries a fleet name and boundary records exist: 19 (10.5%) decided by a boundary record, the rest (89.5%) by the report opener, a phrase rule or the default. Of the 93 not Done, 20% were recorded and 80% read from the report. Of those 93, 25 ended `Stop: wait`, 24 `Stop: landing`, 22 `Stop: host`, 11 `Stop: capacity` or `client`. (A trailing 24 h window reads 4.1% mechanical only because half of it predates the fleet-name column.)
- Routing, 18 inputs: 10 agent text (holds after `Stop: decision`, `Blocked:`, `Stop: wait until` and `Stop: host`, the climb after `Stop: reasoning`, box waits, `Category:`, backlog rank, dated checks, measured worth), 3 structured GitHub, 2 mixed, 3 mechanical (placement refusals, saturation refusals, the climb after two lander refusals).
- The landing join: of the 76 Done runs from 23:00 to 06:14 UTC that expected a landing, 24 showed `landed = 0` on their row, and for 22 of them the lander host's push log names a landing by the same fleet session. A session on another host writes its row there, and only the lander host holds the landing, so the row never saw it. The ledger host now emits each push of its log as a `push_join` row, and ingest gives the session's newest earlier run the sha and time.
- Done: the first-try bar counted 136 of 339 first tries as Done by opener on the 24 h ledger, and 112 by landed sha; a Done run that expected a landing and has no landed sha is now not done in the first-try and harness-loss readings.

Replay of the closing `Stop:` lines of the runs that did not finish Done from 23:00 to 06:14 UTC (66 of the 93 end on one), against the records a mechanical reader would use (north#791):

- `Stop: landing`, 6 runs: the lane's ticket record (`north_base::land_ticket::lanes_of`, read by the scorer's pending check) replaces the words; 3 of the 6 also had a boundary cause.
- `Stop: capacity`, 10 runs: none has a boundary record, so the worker's line was the only evidence. The fleet records a capacity cause only for a usage-limit park or an exit 75, and neither was written for these sessions.
- `Stop: wait until`, 24 runs: 2 have a boundary cause and none has a row in `fleet_waits`, which holds only live leases and drops a wait at its end, so the registered wait is gone when the run is scored.
- `Stop: host`, 20 runs: 4 had a boundary cause; the host a worker named was kept only by dispatch's `host-needs.jsonl`, which that report line fed; a worker now records it with `north fleet need HOST REASON`, which writes the row (north#791), and dispatch's HOST regex is gone.

Capacity, waits and host therefore need a record written at the boundary and kept before their phrase rules are removed; until then the allowlist keeps their reads.

Order of replacement: done from the landed sha with the push-log join, then each `Stop:` kind from its registered or recorded source.

## The rule

`north_threads::threads::attribution::attribute` reads a finished run's outcome, the cause a boundary recorded for it (below), its final report and its compaction count, and returns one cause:

- **worker**: every Done, `Stop: reasoning` or `Stop: leash` (the worker ran out of approach or budget, whatever else the report mentions), and any failure no rule below claims.
- **tom**: halted or paused by Tom, `Stop: decision`, or a `Blocked:` report no harness class claims. Neither side failed.
- **proxy**: a proxy-role session stopped, forgot, relaunched or interrupted the worker mid-turn; only the fleet's record names it, and `north ledger flow` counts it apart from the harness classes, so its fixes aim at the proxy rather than at workers.
- **harness**, the first class whose rule the report matches, in this order:
  - placement: a session that recorded a host need with `north fleet need` inside the run (a `host-need` cause row, north#791), or the report says this host or machine cannot run, push, land or reach what the box needs.
  - access: a GitHub, ssh, token or credential call failed, expired or could not decrypt.
  - capacity: a usage or rate limit, or capacity admission deferred the run; `Stop: capacity|client|budget` no longer decides it.
  - waits: only a recorded wait (below); the report's `Stop: wait until` and its prose no longer decide it.
  - false revert: the lander, the after-landing gate or red-main reverted the worker's landing.
  - duplicate: the work was already on main, already claimed, owned or overtaken by another session.
  - landing refusal: `Stop: landing`, the lander rejected the lane or safe-push refused it.
  - gate refusal: a gate, guard, hook or judge refused or blocked the work.
  - compaction: the report says the run compacted or hit its context limit, or the run compacted and its last message carries no `Done:`, `Not done:`, `Split:` or `Blocked:` opener, so it was parked rather than finished.
  - halted: the fleet stopped the session in the middle of a turn (a stop, forget, relaunch, take-over or retirement); only the fleet's record names it.
  - provider error: a provider's stream ended a turn on its own error, when it is not an access refusal or an account limit; only the provider that read the stream records it, with the error it read.
  - safety refusal: a provider's safety classifier flagged a turn and refused it or moved it to a fallback model; only the provider that read the frame records it, naming the category and the models, and dispatch never climbs the box on one.
  - launch refusal: the launch gate refused to start a session, so there is no run of it to carry the loss; only the gate records it, against the name the launch would have had.
  - lease timeout: the deadline a capacity lease set expired under the command the lease held, rather than the command waiting for a slot; only the lease's guard records it, against its session.
- **worker**, from the report itself when no class claims it: a `Split:`, or a report with no `Done:`, `Not done:`, `Split:` or `Blocked:` opener, which is the worker's own call or a report that broke the report protocol.

## Boundary records first

Each boundary of the harness that touches a session writes its own cause for that session with `north cause record --session NAME --source BOUNDARY --cause CAUSE --reason TEXT` (the `boundary_causes` table). The ingest records each run's fleet session name (`runs.fleet_name`), and `attribute` takes the latest cause recorded under that name since the run started for any run that did not finish Done; the phrase rules above apply only to a run no boundary names.

- lander: a failing check or a conflict in the lane's own lines is worker-caused; a deferral, a hang, a transient error or an origin that refused the push is a landing refusal.
- dispatch: a launch that failed to place (a host that would not take it) is placement.
- capacity: a fleet session's build that ends with no verdict (exit 75) is capacity.
- waits: a failed run whose session registered a `farm` or `branch` wait (`fleet_waits`, or `fleet_wait_history` once the wait settled, which a trigger on `fleet_waits` fills) within the run's window plus ten minutes is waits, read by `recorded_column`; a `job` or `capture` wait is the session's own background work, so the run stays open (pending, left out of the bar) while its lease holds and the session's wake judges it, never a waits loss; dispatch records a waits cause for a session when it first learns the session's dated stop (`wait-stops.jsonl`), a wait on a date the session started nothing to cover; a box deferred with `north check experiments defer` writes a `deferral` cause row (waits) for the calling session, and a first try holding that row, or a registered wait, and no missed check is left out of first-try success and counts as waits; the report words `dated check` and `Stop: wait until` decide nothing, so the same words from a session that recorded neither are the worker's.
- capacity queue: a failed run whose session queued for admission at least `QUEUED_BOUND_SECONDS` (300) inside the run's window, read from the host's `capacity_priority` rows (owner `gate:SESSION`) by `north_capacity_host::capacity::host::queued_behind_capacity`, is capacity; the scorer reads it when no boundary row names the run, and `Stop: capacity`, `Stop: client` and `Stop: budget` no longer decide it (north#791).
- fleet: a turn that compacted and ended without a standard opener is compaction; a stop that cuts a turn short is halted (Tom when he opened the conversation in its interactive UI); a session parked on its account's usage limit or handed over to another account is capacity.
- provider: the provider's stream ended the turn on its own error (`is_error` on the Claude result frame, or the failed turn's error on Codex and OpenCode), unless the same frame carried the account's limit or a credential failure, which the limit watch and the access path own; the row names the session the transcript belongs to and quotes the provider's message. A `system/model_refusal_fallback` or `system/model_refusal_no_fallback` frame is a safety refusal instead, naming its category and models, and dispatch reads it as a boundary row rather than a reasoning failure.
- launch: the launch gate refused the launch, so no run exists; the row is written against the name the session would have had, with the refusal text, by `placement::placed` in `src/cli/fleet/placement.rs`.
- lease: a capacity lease's deadline expired under the command the lease held (no slot wait), written by `north capacity deadline` for the session in `$NORTH_FLEET_SESSION`; a command that expired while waiting for a slot stays capacity.

The worker's own `Stop:` line is the fallback: the ingest keeps each report's last `Stop:` line in `runs.stop`, because the ledger keeps only a report's first 600 characters and a long report's closing line fell outside them, and the rules read it after the kept head.

`north ledger flow` prints, beside the costliest harness classes, the share of the last 3 h of usage and of the last 24 h whose failed runs have neither a record nor a matching phrase rule ("unattributed"). A rule that reads the wrong side shows up as a class whose count moves the wrong way on `north ledger flow`, and the phrase rules are tuned against reports the ledger holds. Runs recorded before the ingest kept a fleet name stay on the phrase rules.

## Where it is read

- `north dash` has the bar "first tries lost to the harness": the share of worker first attempts over 24 h whose cause is a harness class, with the `ready-harness-loss` ceiling of 0.1% in goals.toml. A first try whose report stops on `Stop: landing` is pending, and left out, while the lander's queue on this host holds an undecided ticket from its session; the verdict wakes the session, whose next report replaces it. A `Stop: landing` with no such ticket counts as a landing refusal. A first try the harness resumed by itself (a `resume` cause row, north#790) is judged by the run its chain ends in, and a chain not yet ended is pending and left out; a message from Tom or the proxy ends the chain, so the stopped run stays lost. A `Stop: host` run is judged by the run that relaunched it with `Follows:` naming its session, and is pending until that relaunch for up to 24 h, after which its own end judges it (north-750-d1226 handed the laptop half of its box on and was counted lost to placement 18 minutes later, north#751). Any unfinished first try is pending in the same way while its session has a build queued or running in this host's capacity admission (`north_fleet::fleet::builds::held`, the reading `north fleet stop` refuses on), and while its session holds an unexpired wait lease in the fleet ledger (`fleet_waits`: a `job-run --notify` job, farm, branch or capture), since that wait's end wakes the session to report again; once the lease runs out unrenewed, the report counts as written. A first try whose session recorded a deferral or registered a wait and whose check has not missed, which first-try success leaves out, counts here as a waits loss: the box was staffed before it could finish.
- `north ledger flow` prints the five harness classes of the last 24 h that cost the most API-equivalent usage, with their run counts. The proxy staffs the next harness fix from the first class on that line (harness/claude/agents/proxy.md).
- `north graph ingest` charges each class's 24 h usage to the issue `harness_loss_owners` in harness/defaults.toml names as its owner, and `north graph ready` and dispatch rank open issues by that usage per dollar of one of their boxes (north#734, docs/graph.md).
- Tier learning (`north ledger worker --recommend`, ETA calibration and tier predictions, north#705) treats a harness- or Tom-caused outcome as unjudged, so only worker-caused outcomes count toward a tier's fitness for a shape.
- The nearest-box replay (`north fleet dispatch route-replay`, north#705) reads the same cause through `nearest::cause`, which maps it onto kept, worker and harness and counts Tom's halts and decisions with the harness.

## Replay of 2026-11-06 23:00 to 2026-11-07 06:14 UTC against records (north#791)

The ledger snapshot holds 84 worker runs in the window that ended not done, escalated, split or unclear. 65 closed with a `Stop:` line and 19 with none. Reading boundary causes (`boundary_causes`, apart from `resume`) and registered waits (`fleet_waits`, `fleet_wait_history`) alone:

| closing `Stop:` line | runs | a record names the run | no record |
|---|---|---|---|
| wait until | 24 | 2 | 22 |
| host | 19 | 4 | 15 |
| capacity | 11 | 1 | 10 |
| reasoning | 4 | 3 | 1 |
| landing | 4 | 2 | 2 |
| decision | 3 | 2 | 1 |

14 of the 65 have a record and 51 have none, so mechanical sources alone leave those 51 to the worker by default, where the Stop: line alone classed them; the records written since (host needs, deferral rows, `fleet_wait_history`, capacity queue time) cover the later runs but cannot be written back to these. 13 of the 14 that have a record disagree with the Stop: line:

- Stop: landing, record says capacity or worker: n-takeable (lander worker, fleet capacity), north-722-d0300 (fleet capacity).
- Stop: host, record says worker or capacity: n-gate-relay, n-visible-handover (lander worker), north-725-d0306, n-credit-handover (fleet capacity).
- Stop: wait until, record says worker or capacity: n-shadow-gates (fleet proxy, lander worker, fleet capacity), n-launch-relay (lander worker).
- Stop: reasoning, record says halted or capacity: north-746-d0351 (fleet halted, capacity, lander worker), north-745-d0351 (fleet halted, capacity).
- Stop: capacity, record says worker: north-746-d0541 (lander worker).
- Stop: decision, record says worker: cpx62-wait-learn, n-nexus-load (lander worker, fleet capacity).

The one agreement is a `Stop: capacity` run that the capacity boundary also recorded. The capacity queue rows (`capacity_priority`) live on the capacity host and are not in the snapshot, so the 10 capacity runs without a record may still have one there. The remaining text reads in harness/agent-text-reads.toml stay until a record covers their kind for runs from the window forward.
