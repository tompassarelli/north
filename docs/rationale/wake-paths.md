# Wake paths: what wakes an agent turn, and what to cut

Measured 2026-11-04 over the 7 days 2026-10-28 to 2026-11-04 (UTC). Fleet sessions went quiet for hours while
Claude Code subagents rarely did, and the suspicion was too many overlapping wake paths, some of which depend
on the agent's prose rather than a process. This page lists every path, counts what each one woke, and gives
the cut list. The target is two paths: a process or message event, and one silence check that escalates a
session quiet past twice its ETA to its parent.

## Method

- Wakes: a jq scan of every transcript under `~/code/north-data/profiles` modified in the last 8 days, taking
  each non-tool-result user message dated 2026-10-28 or later, classified by its text, deduplicated on kind and
  timestamp (8,811 wakes). `north convo -x` cross-checks the fleet strings: 41 old-phrase prose-wait wakes
  ("your turn ended on a wait") and 51 run-finished wakes, which match the counts in the brief.
- No-op: the woken turn ended without a `Done:`, `Split:` or `Stop:` opener and either made at most one tool
  call or made at most six and ended on waiting text ("still", "not yet", "nothing landed", "will wake",
  "blocked on the run", "registered a north fleet wait", "this tick"). This is a text heuristic; it
  undercounts turns that ran a status check with seven or more calls.
- Lead timer wakes: a user message whose text equals a `prompt` the same transcript passed to
  `ScheduleWakeup` or `CronCreate`.
- Session runs: `runs` in `~/.local/state/threads/snapshot.db`; uuid-shaped agent ids are sessions, 17-character
  ids starting with `a` are subagents; wall time is `ended - started`, bucketed by the UTC day of `started`.

## Inventory

| # | path | owning code | 7-day wakes | no-op | a process event already covers it | verdict |
|---|---|---|---|---|---|---|
| 1 | Claude Code background-task notification (`run_in_background` shell exits) | Claude Code built-in; teardown guard `crates/north-guard/src/guard/subagent_teardown.rs:77` | 3,713 | 1,578 (42%) | is the process event | keep |
| 2 | Claude Code Monitor notification (event, expiry, timeout) | Claude Code built-in | 2,016 | 1,530 (76%) | is the process event | keep |
| 3 | Claude Code subagent completion and `SendMessage` resume | Claude Code built-in | 358 | 208 (58%) | is the message event | keep |
| 4 | Lead timers: `ScheduleWakeup`, `/loop`, `CronCreate`, Codex `/wake` | Claude Code built-in; `harness/codex/prompts/wake.md:6` | 322 (97, 46, 83, 88, 8 on 10-28 to 11-01) | 156 (48%); 48 lead replies said "this tick" with nothing to do | yes: child reports (11), waits (7, 8, 10) and flow events (17) arrive as messages | cut |
| 5 | Fleet tick prose-wait poke: a box worker whose turn ended on waiting prose with no registered wait is woken after a minute, three times | `crates/north-core/src/fleet/invariant.rs:55` (`judge`), `:106` (`wake_prompt`), `:14` (`WAKES`), `:18` (`CLI_WORK_HELD`) | 63 (2, 17, 43, 1 on 10-29 to 11-01) | 32 (51%) | partly: a registered wait or detached run covers the case where a process exists; the rest is silence | cut |
| 6 | Poke stall to parent: after three pokes the parent is told | `crates/north-core/src/fleet/invariant.rs:110`, `:183` | 12 | 12 (100%) | no; the silence check (12) replaces it | cut |
| 7 | Registered `north fleet wait` on job, farm, branch or capture | `crates/north-core/src/fleet/waits.rs:181` (`settle`), `:199` (message); `src/cli/fleet/waits.rs:53` | 2 of the 64 run-over wakes | counted with 8 | is the process event | keep |
| 8 | pid waits (`pid:PID@START`), registered for detached runs | `crates/north-core/src/fleet/waits.rs:29`; `crates/north-core/src/fleet/watch.rs:233` (`register_detached`) | 49 of the 64 run-over wakes (7 on 11-02, 1 on 11-03) | 50 of 64 (78%) | is the process event for runs outside the CLI | keep |
| 9 | Detached-run hold: a session with a live detached process is held unparked and unwoken for up to 2 h before its pid waits are registered | `crates/north-core/src/fleet/idle.rs:9` (`DETACHED_HOLD`), `:59`; `crates/north-core/src/fleet/detached.rs:86` | 0 (it suppresses wakes) | n/a | yes: the pid wait (8) delivers the same end, and holds no slot | cut |
| 10 | Wait expiry: a wait unresolved after 24 h is dropped with a message | `crates/north-core/src/fleet/waits.rs:20` (`EXPIRY_SECS`), `:192` | 0 | n/a | no; the silence check (12) reaches the parent long before 24 h | cut |
| 11 | Parent report delivery: a child's `Done:`, `Not done:`, `Split:`, `Stop:` or `Blocked:` turn, landing or failure reaches its parent | `crates/north-core/src/fleet/receipts.rs:66` (`deliver_report`); `crates/north-core/src/fleet/registry/parents.rs:45` (`tell`) | 736 | 517 (70%, the parent only acknowledged) | is the message event | keep |
| 12 | Silent notice: a child quiet twice its brief's ETA (45 min when none), parked or exited with no final report, or a `Done:` with an unlanded lane, is reported to its parent | `crates/north-core/src/fleet/silent.rs` (`quiet_after`, `notice`); `crates/north-core/src/fleet/watch.rs:99` | 227 | 198 (87%) | no; it is the only silence check | keep, retimed to twice the ETA |
| 13 | Idle park and revive-on-message: an idle session parks at 10 min (30 s with a registered wait) and its next fleet message revives it | `crates/north-core/src/fleet/idle.rs:47` (`step`), `:6`; `crates/north-core/src/fleet/session_host.rs:340`; `crates/north-core/src/fleet/registry/senders.rs:49` | carrier of rows 7, 8, 11, 14 to 18 | n/a | is how a message event reaches a parked session | keep |
| 14 | Other fleet messages (`north fleet send` between leads, workers and the proxy) | `crates/north-core/src/fleet/registry/senders.rs:38` | 1,001 | 602 (60%) | is the message event | keep |
| 15 | `job-run --notify`: the detached job's exit spooled to its session | `crates/north-core/src/ops/job_run/mod.rs`; lease `crates/north-core/src/fleet/jobs.rs:1` | 78 | 31 (40%) | is the process event | keep |
| 16 | `threads queue follow`: a Follows box is sent to its parent once its predecessor lands | `crates/north-core/src/threads/jobs/queue.rs:82`; `crates/north-landing/src/follows_release.rs:114` | 1 | 0 | is the message event (a landing) | keep |
| 17 | `north flow` stall, stranded and refused alerts to the repository's Fleet parent, repeated every 15 min while they last | `crates/north-ops/src/ops/flow/alerts.rs:136`, `:15` (`REPEAT_SECONDS`), `:93` to `:106` | 219 | 192 (88%) | the first alert is the event; each repeat restates it | cut the repeat; the first alert stays a message event |
| 18 | Host reboot resume | `crates/north-core/src/fleet/rebooted.rs:18`, `:59` | 1 | 0 | is the process event | keep |
| 19 | Context handoff nudge at the brief's arm threshold | `crates/north-core/src/fleet/handoff.rs:19`; `crates/north-core/src/fleet/watch.rs:223` | 0 | n/a | is a message event (a context reading) | keep |

19 paths: 12 keep as process or message events, 1 keeps as the silence check (12, retimed), and 6 are cut
(4, 5, 6, 9, 10, and 17's repeat, which leaves its first alert as a message event). Rows 1, 2 and 3 are the reason subagents rarely go quiet:
every wait they have ends in a process event the CLI itself delivers.

## Session runs over 2 h of wall time

| UTC day | session runs | over 2 h | share | subagent runs | over 2 h |
|---|---|---|---|---|---|
| 2026-10-25 | 379 | 100 | 26.4% | 83 | 0 |
| 2026-10-26 | 642 | 144 | 22.4% | 67 | 1 |
| 2026-10-27 | 295 | 78 | 26.4% | 118 | 2 |
| 2026-10-28 | 186 | 38 | 20.4% | 115 | 2 |
| 2026-10-29 | 360 | 49 | 13.6% | 155 | 0 |
| 2026-10-30 | 453 | 51 | 11.3% | 79 | 1 |
| 2026-10-31 | 360 | 121 | 33.6% | 141 | 1 |
| 2026-11-01 | 77 | 13 | 16.9% | 23 | 0 |
| 2026-11-02 | 19 | 1 | 5.3% | 4 | 1 |
| 2026-11-03 | 28 | 1 | 3.6% | 81 | 0 |

The detached-run hold and pid waits (d0b8ef32, north#424) landed 2026-10-31 16:30 UTC. `north-runtime-follow`
first reported a live runtime containing it, f4771806, at 2026-10-31 17:05 UTC (2026-11-01 00:05 laptop
time); the runtime before it was 08b8ab8a. So the last 7 hours of 10-31 and all of 11-01 to 11-03 ran it. The
share fell to 5.3% and 3.6% on 11-02 and 11-03, but those days had 19 and 28 session runs against 186 to 642
before, so the drop rests on 2 long runs and does not by itself separate the fix from the lower load.

## Cut list

- [x] Retime the silent notice to the one silence check: a session whose last output is older than twice its
  brief's ETA (45 min when the brief has none) and that has no live registered wait is reported to its parent
  once per quiet stretch, replacing the 4-minute idle trigger. Done when: `cargo test -p north-core fleet::silent`
  passes with a test that a child quiet 1.9x its ETA is not reported and one quiet 2.1x its ETA is reported once.
- [x] Remove the prose-wait poke and its stall-to-parent message (`invariant.rs` `judge`, `wake_prompt`,
  `stalled`, `WAKES`, `GRACE_TICKS`, `CLI_WORK_HELD`, `WAKES_PER_TICK`) and the workers skill sentence that
  describes it. Done when: `rg -n "wake_prompt|WAKES_PER_TICK|ended on a wait · woken" crates src harness`
  prints nothing and `cargo test -p north-core fleet::` passes.
- [x] Remove the detached-run hold: a session with a live detached process registers its pid waits and parks at
  the registered-wait park time. Done when: `rg -n DETACHED_HOLD crates` prints nothing and an idle.rs test shows
  a session with a detached run parks at 30 s with its pid wait registered.
- [x] Remove the 24-hour wait expiry message; the silence check reaches the parent first. Done when:
  `rg -n "never ended" crates/north-core/src/fleet/waits.rs` prints nothing and `cargo test -p north-core fleet::waits` passes.
- [x] Send each `north flow` stall, stranded or refused alert once per episode instead of every 15 minutes, and
  update the workers skill line that promises the repeat. Done when: an alerts.rs test shows an unchanged stall
  across two runs 15 minutes apart sends one message, and a cleared then renewed stall sends a second.
- [ ] Stop leads from waking on timers: the workers skill tells a lead to end its turn and wake on child reports,
  waits and flow alerts, and never to call `ScheduleWakeup`, `/loop` or `CronCreate` for fleet work, with a
  guard that refuses those calls in a Fleet lead session. Done when: a guard test refuses `ScheduleWakeup` with
  `NORTH_FLEET_SESSION` set, and a 7-day rescan with this page's method counts 0 lead timer wakes.
  Guard half done 2026-11-04: `crates/north-guard/src/guard/lead_timer.rs` refuses both calls in a Fleet session that
  has child records or a `lead` name segment; `/loop` reaches the hook only as those two calls. The skill sentence
  and the rescan remain.

## Messages to an in-process subagent

An Agent subagent reads a `SendMessage` only at its next tool round, so a message to one inside a long build, test or
wait sits until that call returns, while a fleet session takes native delivery within seconds (north#523, raised
2026-11-03). The runtime gives no way to interrupt a subagent's tool call, so the sender is told instead:
`crates/north-guard/src/guard/subagent_message.rs` adds that delay to the sender's context on every `SendMessage` whose
target is not a live fleet session record, and stays silent for fleet sessions.
