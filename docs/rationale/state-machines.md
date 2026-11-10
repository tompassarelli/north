# State machines: one typed state, one record, every move logged

On 2026-11-07 north lost hours to state that moved through phases while nothing could say it was stuck: a snapshot pull marker whose mtime meant "running" after its pull was killed, a dispatch pause held by a stopped systemd timer that a deploy restarted, a policy generation held on every host for four hours, an API organization marked exhausted by a bare file, session reports lost when the reaper archived their sessions, and issues staffed twice because filing one did not claim it. Each is a state machine that north holds implicitly, in marker files, mtimes, unit state, status strings and boolean pairs. This note lists where each such machine lives today, ranked, and the one pattern every one of them moves to; each has a box on north#757.

Owner: north#1042

The machine-fix and machine-refusal detectors file one box per machine and week on the owner: for a machine whose
code an incident fix changed, and for a move the lifecycle store refused. Every machine that has moved to the pattern
is checked over every ordering of a stated scope at the landing of each change to it
([Checking every ordering](#checking-every-ordering-north1042)).

## Inventory

Swept on 2026-11-07 across every crate, `bin/`, `lib/` and `harness/hooks/` by six read-only passes at 289fe31ea:
about 200 places where long-lived state moves through phases without one typed record. They group into the 36
machines below, each one box on north#757 sized for one worker. Rank is fix commits since 2026-10-24 whose message
names the machine (keyword-matched over `git log`, so a coarse count) times the cost of getting it wrong: 3 when it
loses or duplicates work or stops the whole fleet, 2 when it silently stops one flow or host, 1 when it wastes time.
The incident ledger lives on the ledger host and was not readable from the inventorying host, so commits stand in for
incidents. Shapes name the north#758 rule that matches each machine's code.

Reviewed on 2026-11-10 (north#1042, [watch record](watch/2026-11-10-pstack-verification.md)): the rank is a keyword
count from one sweep and is not kept current. The list of machines that have moved to the pattern is the registry in
`src/cli/machines.rs`, which the landing holds complete, and `north check machines --replay` recounts the incident
fixes in each machine's code from history.

1. **Fleet session lifecycle and retirement** (score 348: 116 x 3). `fleet/NAME/session.json` spreads the phase over
   `stopped`, `ended`, `host_pid` and `adopted_ms` (`crates/north-fleet/src/fleet/model.rs:231-301`); status is never
   stored but re-derived on each read from pid liveness, the stream's last turn signal and `unaccepted.jsonl`
   (`fleet/registry/status.rs:24-99`), and a stream read error reads as Exited. Seven writers, four without the start
   lock (`adopting.rs:51`, `senders.rs:60`, `follower.rs:95`, `watch.rs:321`), so a concurrent halt's `stopped` can be
   overwritten and the session revived. The reaper writes `retired.json` with the first 4000 characters of the last
   reply (`reaper.rs:22,194`), so a long report loses its `Stop:` line, and archives a parentless session a minute
   after its report, before dispatch reads it (0c54814b5, 952e8132c). `host.pid` is never removed and liveness has no
   start time. Shapes: lifecycle-bools, pid-liveness, status-string (`fleet_sessions.status TEXT`,
   `fleet/store/schema.rs:23`).
   Converted (`crates/north-session-host/src/session_lifecycle.rs`): `fleet-session` keeps one record per launch
   (`NAME@CREATED_MS`; live, parked, idle-exited, host-ended, stopped, adopted, retired) in the host's `ledger.db` and
   replaces `stopped` and `ended`. Live is held by the session host's process with its start time, so a dead host
   orphans to parked and a reused pid never reads as live; a stop or a new host preempts a live hold; retired is
   final and its cause carries the report's `Stop:` line. Status is derived from the record, and from the stream only
   for a live session. Not converted: `host.pid`, which stays the handle `Host::terminate` signals, and
   `fleet_sessions.status`, the cross-host projection of derived status; a revival that follows a stop is a legal
   move, so the two still race in the order they commit. See the
   [fleet-session chart](state-machines/fleet-session.md).
2. **Land ticket lifecycle** (330: 110 x 3). A ticket's phase is its queue directory (`Q/`, `outbox/`, `done/`,
   `withdrawn/`, `quarantine/`, remote `outbox/taken/`) plus the first word of its `verdict` string, read by value in
   fifteen places (`crates/north-landing/src/land/{bisect,records,incidents,reservation,replay,promote,sheriff}.rs`,
   `crates/north-lander-doctor/src/facts.rs:129`, `crates/north-fleet-supervisor/src/fleet/tickets.rs:60`). `settle` writes
   `done/` then removes the pending file (`land/queue.rs:175-186`), so a crash leaves both; `quarantine/` has no
   exit; `replace_older` deletes a ticket with no record (`land/enqueue.rs:148-165`). The landing intent
   (`Q/landing/intent.json`, `recovering.json`) keeps its tickets out of every candidate while recovery fails
   (`land/jobs.rs:93,119`), and `finish` removes the lane worktree before deciding its ticket (`land/after/mod.rs:136`).
   Shapes: phase-suffix (directory as phase), status-string. Now the machine `land-ticket`
   (`crates/north-base/src/land_ticket.rs`, `0b3ae0170`), one record per ticket on the lander host; see the
   [land-ticket chart](state-machines/land-ticket.md).
3. **After-landing run, red-main episode and base-broken pause** (255: 85 x 3). The run's phase is spread over
   `Q/following/HEAD.json`, `Q/logs/post-land-SHA.{runner,status,outcome}` and process-group liveness
   (`land/watch.rs:26-186`, `land/post_land/runner.rs:25-99`); `misses` lives in memory, so a restart resets the
   alarm, and a reused process group passes for a live runner. The red episode marker `post-land-red-REPO` opens only
   if its alarm got through (`land/post_land/settle.rs:126`); `Q/base-broken` keeps `alerted=true` for the next
   breakage (`land/pause.rs:49`); strikes reset whenever main moves (`land/bisect.rs:49-118`). Shapes: presence-flag,
   pid-liveness. Now the machines `after-landing-run`, `red-main` and `base-broken`
   (`crates/north-landing/src/land/after_landing_lifecycle.rs`); see the
   [after-landing-run chart](state-machines/after-landing-run.md), the [red-main chart](state-machines/red-main.md)
   and the [base-broken chart](state-machines/base-broken.md).
4. **Issue claims** (195: 65 x 3). Four rules decide "held" (`crates/north-threads/src/threads/claims/ledger.rs:95`,
   `claims/verbs.rs:134-183`, `board/mod.rs:166`, `work_items.rs:126`); closing an issue never deletes its claim; a
   session-host close releases the claims of an idle-parked session that can still be revived
   (`crates/north-fleet/src/fleet/watch.rs:483`); a failed launch leaves claims on a name that no longer exists
   (`fleet/registry/launching.rs:73`); a refused re-claim is only logged (`fleet/claims.rs:47`). Filing an issue did
   not claim it until 22350314c. Shapes: status-string (`standing == "live"`). Now the machine `claim`
   (`crates/north-threads/src/threads/claims/lifecycle.rs`); see the [claim chart](state-machines/claim.md).
5. **Runtime promotion** (141: 47 x 3). `current` points at a new runtime before its health check and `agents sync`
   pass (`crates/north-runtime/src/runtime/update/sync.rs:17-58`), and on a first install a failed check leaves it
   live while saying "rolled back"; `runtime-previous` is not updated on rollback; the one-hour `runtime-failed` hold
   lapses and re-promotes the same failing main every hour (`update/hold.rs:55-93`); `runtime-settle-judged` is
   written before its alarm, so a failed reopen is never retried (`settle.rs:63-66`); the swap log turns a rolled-back
   runtime's commits live (`live.rs:64-75`). Shapes: link-read, mtime-read, phase-suffix (`SHA.staging`).
   Converted (`crates/north-runtime/src/runtime/lifecycle.rs`): `runtime-promotion` keeps one record per runtime
   (building, installing, live, superseded, build-failed, install-failed, rolled-back, unfinished) and replaces
   `runtime-failed`, `runtime-failed-causes` and `runtime-previous`; `runtime-canary` keeps one per host and runtime
   and replaces `runtime-canary`; `runtime-landing-judgement` replaces `runtime-settle-judged`, set only after its
   alarm is delivered. The swap log now notes a runtime only once it is live, and a failed first install removes
   `current`. Not converted: the `SHA.staging` copies shipped to other hosts (written over ssh by `deploy.rs`) and
   the `agents-sync-unpublished` marker, which belongs to policy publication.
6. **Fleet message delivery** (111: 37 x 3). Spool file, ledger event, `delivered/ID`, a `.handing` marker and a
   flock (`crates/north-fleet-supervisor/src/fleet/registry/hand_in.rs:16-121`, `fleet/store/spool.rs`); a crash leaves
   `.handing`, resolved only by searching the stream; rows to a stopped session wait while a local send revives it
   (`fleet/store/exchange.rs:74`). Shapes: presence-flag.
   **Converted (north#757):** the machine `row-delivery`, one instance per `SESSION/ROW` in the host `ledger.db`
   (`crates/north-session-host/src/row_delivery.rs`, writers in `fleet/registry/hand_in.rs` and `delivery.rs`): `handing-in`
   (held, 10 min), `delivered`, `interrupted` (a dead holder's `handing-in`, 15 min), `failed` (retried each tick, 1 h). The
   journal directory and its `.handing` markers are adopted once at `Fleet::open` and removed; the spool file stays a
   queue and the stopped-session filter stays a query. See the [row-delivery chart](state-machines/row-delivery.md).
7. **Threads spool and mirrored-client mode** (88: 44 x 2). `~/.config/threads/remote` presence switches a host to
   snapshot reads and spooled writes (`crates/north-threads/src/threads/client.rs:118-125`). **Converted (north#757):**
   the machine `threads-spool`, one record per spool file in the client's `ledger.db`: queued, sending, sent and
   refused, the flush's pid holding a sending write. A write the ledger host refuses stays in `refused`, its cause
   naming the verb, and `north health` reports one finding per refused verb, not one per file (north#957). A write the
   host withheld (a verb off its allowlist, or an exit 126 or 127) is asked again by sending its verb's oldest write an
   hour after that verb's last refusal, and once a write of the verb is delivered every withheld one goes back to
   `queued` (north#872). The mirrored-client mode itself stays a file (north#757). Shapes: presence-flag. See the
   [threads-spool chart](state-machines/threads-spool.md).
8. **Gates** (84: 28 x 3). Open means `closed_ms IS NULL` in each host's `ledger.db`
   (`crates/north-providers/src/ledger/gates.rs:35-55`); other hosts see a deleted-and-reinserted `fleet_gates`
   snapshot (`crates/north-fleet/src/fleet/store/gates.rs:31`); the answer is delivered after the close with no retry;
   the default timer is a detached process that does not survive a reboot; a spooled answer is set aside as spoofed
   (north#747, which owns that bug). Shapes: status-string (NULL-encoded phase). Now the machine `gate`
   (`crates/north-fleet-store/src/gate_lifecycle.rs`); see the [gate chart](state-machines/gate.md).
9. **Capacity leases** (82: 41 x 2). Queue ticket, lease JSON, scope unit and three freezing layers
   (`crates/north-capacity-host/src/capacity/host/{state,decision,run}.rs`,
   `crates/north-capacity/src/capacity/{lease_deadline,agent_load}.rs`); `scope_live` treats an unknown systemd
   answer as live (`host/systemd.rs:76-82`), and a lease with `frozenAt` never expires if the thaw tick stops.
   Decision strings RUN, RESERVED, DEFER are compared by value. Shapes: unit-intent, status-string.
   **Converted (north#757):** the machine `capacity-lease` (`crates/north-capacity/src/capacity/lease_lifecycle.rs`),
   one instance per lease id in `lifecycle.db` beside the lease files in the capacity runtime directory (both vanish
   at reboot, so a record never outlives its file; `north health facts` reads that store as well as `ledger.db`).
   States: reserved, running, frozen (dwell 6 h, the thaw tick that stopped), overstaying (expired, scope would not
   die, 10 min), settling (held by the wrapper, orphan abandoned, 5 min), abandoned, released, reclaimed. `frozenAt`
   is gone from the lease JSON; `scope_live` still answers "live" when systemd cannot be asked, but the lease then
   moves to overstaying with that cause instead of silently staying. A lease file with no record is adopted once from
   its `kind` and `frozenAt`; a record whose file is gone is reclaimed. Not converted: the queue tickets, the
   `gaming-frozen.json` unit list and the batch slice `cgroup.freeze` (the other two freezing layers) and the
   decision strings. See the [capacity-lease chart](state-machines/capacity-lease.md).
10. **Lane retirement** (75: 25 x 3). Lane gc's activity from reflog and mtimes, `flagged_at` carried in
    `stale.json`, reap's age from a `.git` mtime that counts as old enough when unreadable
    (`crates/north-lanes/src/lanes/reap.rs:320`), rescue's partial end states, pin sidecars removed after their
    worktree (`lanes/pin_retire/consumed.rs:54`). Shapes: mtime-read, presence-flag.
11. **Job queue and job runs** (68: 34 x 2). A `taken` job has no lease or taker
    (`crates/north-threads/src/threads/jobs/queue.rs:178-224`); a crashed job's `job_runs.finished` stays NULL and
    JOB-START never meets a JOB-EXIT (`crates/north-ops/src/ops/job_run/records.rs`). Shapes: status-string.
12. **Offload slots** (66: 33 x 2). `SLOT.{lock,busy,holder,owner,pgid}` (a holder keeps `.lock` exclusive and `.busy` shared, and probes test only `.busy`), queue tickets held by flock, the warmer's
    `warm.lock` and `.warmed`, build-offload claims whose liveness is `/proc/PID`
    (`crates/north-runtime/src/land_offload/ship/script.rs:97-155`, `build_offload/claims.rs:11-55`); a SIGKILL leaves
    `.owner` and `.pgid`, and a reused pid whose cmdline says "offload" can be killed. Shapes: lock-flag,
    pid-liveness.
13. **Window adoption and interactive hand-off** (66: 22 x 3). An adopted name is `adopted_ms` plus `stopped` plus
    `host.sock` created as a directory (`crates/north-fleet-supervisor/src/fleet/registry/adopting.rs:17-159`); two detectors
    disagree about the window (`window_inbox.rs:18-48`, `interactive.rs:58-66`), so a live window reads exited and
    the name moves to another window; an interactive hand-off is on disk identical to a stop. Shapes: lifecycle-bools,
    pid-liveness.
14. **Dispatch staffing holds** (60: 20 x 3). Holds recomputed from every host's fleet list, a laptop mirror judged by
    mtime, `launches.jsonl` per dispatching host and `revived.json` never pruned
    (`crates/north-dispatch/src/dispatch/{record,revive,effects,survey,fleets}.rs`); a retired session is filed as
    forgotten, so the two-tries hold never fires. Shapes: status-string (`WORKING`, `HOLDING`), mtime-read.
15. **Policy publication** (57: 19 x 3; every host held four hours on 2026-11-07). `agents-sync-unpublished`
    presence and mtime (`crates/north-runtime/src/runtime/update/sync.rs:34-148`, cause parsed from log lines),
    `gen-*` directories and the `current` link (`crates/north-core/src/catalog/generation.rs:28-77`); a failed
    profile projection still publishes and then reports "not published" (`catalog/sync_gate.rs:17-46`).
    Shapes: presence-flag, link-read.
16. **API organization exhaustion** (57: 19 x 3). A file `api-exhausted/ORG` per host keeps the provider's refusal
    body and is written only from a refusal that names low credit; a passing one-token probe (`north fleet accounts
    --probe`) clears it and leaves a tombstone of the cleared text so a peer's host sync cannot restore that mark.
    Five readers still read presence (`crates/north-providers/src/providers/claude/api_pool.rs`, `accounts.rs`,
    `crates/north-pace/src/credit.rs`, `usage/credit_table.rs`, `src/cli/fleet/limit.rs`), and a reading can now
    contradict the computed balance. Shapes: presence-flag, status-string.
17. **Capacity mode, game reserve and host mode** (52: 26 x 2). `capacity-mode/state.json` read-modify-written
    without a lock by the tick and the quiet phrase (`crates/north-guard/src/guard/quiet_phrase.rs:107`); a quota
    applied with `set-property --runtime` is cached past a reboot; `gaming.json` has two writers; the game reserve is
    written non-atomically; `K/mode` is a second "away" that resets at boot. Shapes: lifecycle-bools, unit-intent.
18. **Test-case quarantine** (36: 12 x 3). A flaky entry leaves only through `landed_naming`
    (`crates/north-landing-gates/src/landing/judge.rs:79`), which nothing calls, so after 24 h every lane selecting that test
    file is held (`landing/base_relative.rs:108`). Shapes: status-string.
19. **Host disk episode and watchdog caps** (32: 16 x 2). `host-disk.episode` written only if its page succeeded
    (`crates/north-runtime/src/offload_cache/mod.rs:307-325`); overload caps recorded even when `systemctl` failed and
    forgotten when `overload.json` is lost (`crates/north-capacity-host/src/capacity/watchdog/act.rs:202`);
    `disk-reclaim.stamp` written before the reclaim. Shapes: presence-flag, marker-stamp.
20. **Spend breaker, tripwire and usage gate** (30: 15 x 2). An unknown breaker label becomes Within and un-trips
    (`crates/north-pace/src/usage/breaker/state.rs:66`); trips lapse with age, so a stopped tick fails open
    (`spend/tripwire.rs`); each reader of `usage-gate.json` judges its staleness alone. Shapes: status-string,
    mtime-read.
21. **Worker run lifecycle** (27: 9 x 3). `runs.outcome` mixes lifecycle with result; a spawn-hook worker that goes
    quiet keeps a `running` row with `ended=''` forever and never releases its claim
    (`crates/north-threads/src/threads/runs/record.rs:126-146,367`,
    `crates/north-providers/src/providers/claude/runs/row.rs:143-156`). Shapes: status-string.
22. **Hook activation and retirement** (22: 11 x 2). A missing `current` link counts as a first sync and turns every
    permission on (`crates/north-core/src/agent_catalog.rs:169`); two readers of `activation.json`, one skipping the
    environment override (`crates/north-guard/src/guard/desktop_action.rs:33`). Shapes: presence-flag.
23. **Registered waits and parks** (20: 10 x 2). One notion in two stores, `.waits/SESSION.json` and `fleet_waits`,
    plus `.all-out` (`crates/north-fleet/src/fleet/{waits.rs,store/waits.rs,handover.rs,registry/kept.rs}`); Parked
    has no record. Shapes: presence-flag.
24. **GitHub broker and tokens** (16: 8 x 2). A leftover `resident.wanted` makes on-demand brokers flap; a killed
    broker leaves the user's gh config pointing at a dead socket
    (`crates/north-ops/src/ops/gh_broker/mod.rs:90-189`); a stale pushed `gh-write-token` disables minting for good
    (`crates/north-base/src/platform/github_app.rs:289`). Shapes: presence-flag, mtime-read.
25. **Snapshot pull** (12: 6 x 2; pulls stopped 95 minutes on 2026-11-07). One `.tried` file carries three
    meanings, its mtime, its flock and its text (`crates/north-threads/src/threads/snapshot.rs:51-91`); a forced
    caller re-stamps it before its lock attempt; health judges age at two different thresholds
    (`crates/north-health/src/health.rs:111`, `health/flows.rs:18`). Shapes: marker-stamp, lock-flag.
26. **Health episodes and goal announcements** (no fix commits counted). `health.json` is saved before its alerts
    go out, so a failed alert is lost for the episode (`crates/north-health/src/health.rs:471`); a killed landing probe
    leaves a worktree and branch in `~/code/north/main` (`health.rs:292-329`); `goals/tick.json` is saved before its
    lines are delivered (`crates/north-goals/src/tick.rs:74`). Shapes: presence-flag.
27. **Main-red episode on GitHub** (none counted). The sheriff's state is markers in an issue body; a failed rerun
    request leaves the attempt-1 marker, so attempt 2 never runs (`crates/north-ops/src/ops/ci_watch/sheriff.rs:362`);
    a private check killed after posting "pending" leaves it pending forever (`ops/private_check/judge.rs:161`).
    Shapes: reviewer-only (state in issue text).
28. **Dispatch on and off** (2 incidents on 2026-11-07). The pause eba976a4f added is itself a presence-and-mtime
    file (`crates/north-base/src/dispatch_pause.rs`, `crates/north-dispatch/src/dispatch/pause.rs`) beside the timer
    it stops; health compares the two. Shapes: presence-flag, unit-intent.
29. **Private-text judge** (none counted). No in-flight record: absent means not judged, judging or dead
    (`crates/north-base/src/platform/private_text.rs:41`, `crates/north-runtime/src/privacy/judge.rs:29-132`); a
    failed judge keeps answering "judging". Shapes: presence-flag.
30. **Review and distill locks** (none counted). `code-health/REPO/review.lock` is released by the launched agent's
    `rm` and a dry run keeps a real lock (`crates/north-code-health/src/code_health/tick.rs:95-173`); `distill.lock.d` is
    stolen after 10 minutes from a run that still holds it (`crates/north-core/src/grounding/distill.rs:799`).
    Shapes: mtime-read, presence-flag.
31. **Private desktop run** (none counted). A launcher killed with SIGKILL leaves its compositor running and only the
    next start archives the directory (`crates/north-ops/src/ops/private_desktop/{session,run_state,reap}.rs`).
    Shapes: presence-flag, pid-liveness.
32. **Experiments and deferred checks** (none counted). An extension is lost when the process dies between the body
    edit and the key file (`crates/north-flow/src/experiments/file.rs:127`); a deferred check written to an unlanded
    lane leaves its issue closed with a waiting box (`experiments/deferred.rs:240`). Shapes: reviewer-only.
33. **Issue bodies as shared state** (none counted). Every box writer replaces the whole body with no
    compare-and-swap (`crates/north-ops/src/ops/observation_boxes/mod.rs:43-109` reads no fresh copy), so concurrent
    writers lose each other's ticks. The fix is a guarded edit, not a record. Shapes: reviewer-only.
34. **Timers that read a session name as running** (none counted). incident-retro and watch-review count a fleet row
    named with their prefix and not `exited` as running (`crates/north-runtime/src/timers/incident_retro/mod.rs:71`,
    `timers/watch_review/mod.rs:144`); a session that dies leaves the week unclassified. Shapes: status-string.
35. **Laptop availability** (none counted). Automatic versus manual is decided by comparing `by` and `reason`
    strings (`crates/north-pacing/src/laptop/tick.rs:80,95`); draining stays forever when the job queue is
    unreadable. Shapes: status-string.
36. **Rename-into-place leftovers** (none counted). Staging names nothing reaps after a crash: `.stage.*` and
    `.current.PID` (`crates/north-ops/src/ops/vendor_runtime/mod.rs:100-125`), `.incoming/PID`
    (`ops/retention/archive.rs:141`), `.zst.tmpPID` (`ops/convo/archive.rs:246`), `target.seeding-PID`
    (`crates/north-lanes/src/lanes/target_seed.rs:44`). Shapes: phase-suffix.

### Not converted

- Mutual-exclusion flocks with no phase (`runtime-update.lock`, `runtime-settle.lock`, `records-mirror.lock`,
  `ingest.lock`): the kernel releases them with their holder.
- Once-guards and dedupe memories (`file-skills/SESSION/HASH`, `nix-cards`, `recommend-acts`, `seen.json`,
  `dispatch/alert.json`, `landing/notified`, `landing/followed`): a lost or stale entry repeats or skips one notice.
- Caches with a time to live (`gh_cache`, `github-mirror.db`, `usage-gate.json` as a reading, tune's cells in the graph) and
  content-addressed verdicts (`verdict_cache`, `private-text-verdicts`): a key, not a phase.
- Already explicit: goal trend (`crates/north-goals/src/history.rs`), capacity `Mode` with its transition functions
  and `adjustments.jsonl`, the convo index's `files` table, flow episode alerts (`ops/flow/alerts.rs`, the closest
  existing template), and claims' `take`, a real transition in one transaction.
- Codex conduct state (`crates/north-guard/src/guard/codex_conduct/`): north builds nothing new for Codex
  (standing order of 2026-10-16).

## The pattern

The module is `north_base::lifecycle` (`crates/north-base/src/lifecycle.rs`), built under north#757.

- **One typed state per machine.** A machine implements `Lifecycle`: its `NAME`, a `State` enum whose every
  variant is a phase a person would name, `legal(from, to)` (the whole transition table in one match), `dwell(state)`
  (the longest a healthy instance stays in a state; `None` for a resting state) and `orphan(state)` (where an
  instance goes when the process holding the state dies; `None` for a state no process holds). A combination the old
  representation allowed but no phase names cannot be written, because the record holds one variant, not a set of
  flags.
- **One record per instance.** `lifecycle_states(machine, instance, state, since_ms, due_ms, owner, cause, seq)` in
  the SQLite file that owns the machine's domain: `~/.local/state/north/ledger.db` for a host-local machine (snapshot
  pull, dispatch intent, policy publication, runtime promotion), the ledger host's `threads.db` for a fleet-wide one
  (sessions, claims, gates), through the same `Store`. No new store and no new file.
- **One writer.** `Store::transition::<L>(instance, to, cause, at)` is the only way a record changes. It runs in one
  `BEGIN IMMEDIATE` transaction, refuses a move `legal` does not list (`Refusal::Illegal`, logged), refuses to move a
  state a live process holds (`Refusal::Held`, not logged: two takers racing is expected), and records the mover's
  process as the holder of a state `orphan` names. The holder is host, pid, process start time and boot time, so a
  reused pid or a reboot never passes for the holder.
- **Every move logged with its cause.** `lifecycle_moves` keeps from, to, time, mover and cause for 30 days, refused
  moves included. When the holder of a state is gone, the next transition first logs the move to the orphan state with
  "the holder, pid N on HOST, is gone", so the log says what happened instead of the old marker silently aging out.
- **Stuck judged without types.** Each record carries its own staleness contract (`due_ms` from the dwell, `owner`
  for a held state), so `Store::faults(now)` finds every overdue state, every state whose holder died and every illegal
  move of the last hour, for every machine, without knowing any machine's type. `north health facts` on each host
  returns them and the laptop's `north health` shows them as the `state-machines` flow of the outcome watchdog
  (docs/rationale/outcome-watchdog.md), which pages the proxy after two failing ticks like every other flow.
- **A move and the effects it starts commit together.** `Store::transition_with_effects::<L>(instance, to, cause,
  at, effects)` writes the move and its side effects (start a watchdog, start an after-landing run, announce an
  offload offer) into `lifecycle_effects` in the move's one `BEGIN IMMEDIATE` transaction; a refused move queues none.
  Each effect has a kind, an idempotency key unique across the store (a second effect under a key already queued or
  delivered is dropped) and a delivery: **replay-safe** effects are requeued when their lessee dies or their lease
  runs out and their receiver dedupes on the key, **process-bound** effects belong to the process that committed the
  move, only it leases them, and they are retired once it is gone. `Store::lease(kind, at, hold)` takes the oldest
  deliverable effect of a kind after that recovery, `Store::delivered`, `Store::undelivered` (retried after a minute,
  at most five leases) and `Store::retire` settle it, and `Store::deliver` drains a kind. Before north#862 each move
  and its effect were separate writes, so a restart between them lost or repeated the effect: four fixes in 72 h
  (163a23c725, 97e4aa6827, 22571229b6, 363349d345), each in its own module. The mechanism follows T3 Code's
  `EventSink`, `EffectWorker` and `EffectOutbox` (pingdotgg/t3code at 32a0531, MIT, read as data, no code copied).
  On it: the fleet watchdog start (`fleet-watchdog`, committed with a session's move to Live and started by the launcher
  or the next fleet tick), each queue's after-landing runs (`after-landing-run QUEUE`, committed with a head's move to
  Journaled and leased by the queue's lander until the verdict) and an offload offer's start here (`offload-start`,
  process-bound, committed with the `offload-offer` machine's move to Started in the capacity runtime directory's
  `lifecycle.db`).

### Why this shape

- The 2026-11-07 failures were all a phase held by something that cannot say it is stale: a marker whose mtime meant
  "running" after its writer died, a systemd timer whose state stood for an intent a deploy overwrote, a file whose
  presence meant "exhausted" with nobody to clear it, a session status re-derived from processes differently by two
  readers. A typed record with a holder and a dwell makes "stuck" a query instead of an investigation.
- SQLite rather than files: a transition must read the current state and write the next one atomically, and two
  processes (a timer and a hand-run command) do race on most of these machines. `ledger.db` and `threads.db` already
  run in WAL mode with busy timeouts; a marker file cannot compare-and-swap.
- The machine trait sits in `north-base` and knows no store schema beyond its own two tables, so every crate can
  define a machine without depending on health, and health judges every machine without depending on any of them
  (dependency inversion: the record carries the judgement's inputs).
- A refusing move is not a landing gate, so it needs no shadow period; what it refuses is a bug in north's own code.

### Converting a machine

1. **Map before you change it.** Write the machine's state vector (every field, file, marker, timer or flag that
   jointly governs it), its named states, the combinations the current representation allows that no state names,
   the transition table as the code behaves (from, trigger at FILE:LINE, guard, to, fields changed), every site that
   changes one field without its coupled siblings, and every (state, event) pair that silently does nothing. The
   machine's box on north#757 names where its state lives today; the conversion extends that map.
2. **Classify before you switch writers.** Write `classify(old state vector) -> Result<State, Impossible>` and run it
   over every host's live files (`north health facts` reaches them all) before changing a writer. A combination it
   calls impossible that turns out legitimate means the model is wrong; one that is a real bug is a cheap catch.
   This is the out-of-memory form of asserting the invariant before collapsing flags into an enum.
3. **Switch every writer to `Store::transition`** and every reader to `Store::current`, in one lane, deleting the
   marker, flag or unit-state read. The classifier becomes the adoption step: an instance with no record is adopted
   from its old representation once, with the cause naming what it was adopted from, and the old path is removed.
4. **Test the two guarantees a person would notice:** an illegal move is refused and leaves the record unchanged, and
   the stuck state of the incident that motivated the machine (a holder killed, a dwell outlived) appears in
   `Store::faults`.
5. **Register it.** Add the machine to the registry in `src/cli/machines.rs` with its table file, the code that
   drives it, its ends and the states a writer may announce again, run `north check machines --write --root .` and
   commit its chart. The landing refuses a machine the registry does not name.

### Rules that keep it from coming back (north#758)

North#758 adds an after-landing code-health rule, `implicit-state` (`north code-health implicit-state` lists the
debt), that never refuses a landing and files each new occurrence as gate debt. The shapes it files:

- `marker-stamp`: `set_modified`, `set_times` or `touch` stamping a file with the current time, read later as state.
- `presence-flag`: a path the repository writes or removes whose existence, or `read_to_string().ok()`, decides
  behaviour, the empty marker file included.
- `unit-intent`: `systemctl stop|disable|mask` of a unit, used to hold an intent no record keeps.
- `pid-liveness`: `/proc/PID` existence or `kill(pid, 0)` taken as a holder's liveness, with no start-time check.
- `phase-suffix`: a `.staging`, `.old`, `.partial`, `.done` or `.tried` name whose presence is read as a phase.
- `status-string`: a field named status, state, phase or stage compared with a word literal outside parsing, a SQL
  `status TEXT` column without a `CHECK(status IN (...))`, and `WHERE status = '...'`.
- `lifecycle-bools`: a long-lived struct with two or more lifecycle booleans.

Listed but not filed, because legitimate code reads them too: `mtime-read` (the outcome watchdog reads write times as
outcomes), `unit-read`, `link-read` (a symlink's target or mtime as what is live and since when), `lock-flag` (an flock
on a marker file meaning "running") and `state-entropy` (a lifecycle boolean beside four or more booleans and
`Option`s).

Reviewer-only (no call pattern finds them; the code-health review asks about them): which configuration table an
entry sits in standing for a phase, a log or journal line parsed back as current state, two writers of one state in
different crates, a process list or window list read as a session's state, and a cached value whose source can change
underneath it (a `set-property --runtime` value cached past a reboot).

Code that reads and writes its state through `north_base::lifecycle::Store` is exempt; an exception is a
`[[implicit_state.allow]]` entry in structure.toml naming the shape, path, item and reason, or a
`Code-health-allow: implicit-state PATH — REASON` trailer; the files under `[implicit_state] owners` are not scanned.

## Checking every ordering (north#1042)

A machine on the pattern is a table the code already holds, so north checks the table itself and keeps no second
model of it. `north check machines --root .` drives each machine's own `Lifecycle` impl through the real store over
every ordering of a stated scope (one instance; three processes, two on one host and one on another, each trying
every move for every holder and settling the instance; the death of the process holding the state; its dwell running
out) and exits 1 with each broken promise and the shortest ordering that shows it. The comparison with pstack's
formal-verification skills, TLA+, stateright and Kani, and the measured results, are in
[watch/2026-11-10-pstack-verification.md](watch/2026-11-10-pstack-verification.md).

- **Promises.** Every accepted move is logged and the log alone rebuilds the record; a live holder is never declared
  gone, and a held state is taken from a live holder only by a move the machine lets replace one; an end has no way
  out, no dwell and no holder; a state is accepted twice exactly where the machine promises it; every other situation
  can be left once nothing more dies; every state is reached.
- **Registry.** `src/cli/machines.rs` names every `Lifecycle` machine in the tree, modelled or not. The landing runs
  the search when a lane changes the kernel, a table, the explorer, the registry or a chart, and on every code lane
  refuses an `impl Lifecycle for` the registry does not name.
- **Charts.** `north check machines --write` writes one state chart per modelled machine from the moves the store
  accepted during the search, and the check fails on a page that differs or that no machine owns. Besides the charts
  linked from the inventory, the [check-environment chart](state-machines/check-environment.md) is of a machine the
  inventory does not list (`crates/north-check-environment/src/lib.rs`).
- **Not modelled yet.** `runtime-landing-judgement` and `offload-offer` are registered with no model, because their
  types are private to their crates.
- **Reviews.** The machine-fix detector files "Model NAME" for an unmodelled machine whose code an incident fix
  changed and "Extend the NAME model to the fixes its check let through" for a modelled one, counting fixes newer
  than its chart. The machine-refusal detector files "Settle NAME's refused move to STATE" when a host's lifecycle
  stores (each host's ledger and the capacity runtime's `lifecycle.db`) refused a move in the last hour. Each is one
  box per machine and week on the owner above.
- **Two instances.** `fleet-session` and `land-ticket` also search a second instance: the first through every
  ordering of the scope above, the second attempting to start at each of the first's situations, and two launches
  racing from no record. The machine's real admission path decides whether a start is possible — the name claim
  (`Session::claim`) for a session, the withdrawal check (`withdraw::refuse_requeue`) for a ticket — and the
  `together` promise names the situations in which no second instance may start: a live session name, a withdrawn
  head. The recorded ordering fixes `97e4aa682` (two launches of one name) and `f1ff89dde` (a withdrawn head queued
  again) are of that kind (north#1042).
- **Replay at a recorded fix parent.** Each of the two highest-ranked machines asserts its model inside an existing
  nextest test, so the command replays every ordering of its stated scope and fails with the guard of its recorded
  ordering fix put back as the parent had it:
  `cargo nextest run -p north-fleet-supervisor -E 'test(a_retired_session_refuses_a_revival_and_keeps_its_retired_record_with_the_reports_stop_line)'`
  fails with `fleet-session: together: two launches started together and both hold the name, the second as live ...`
  when `Session::claim` is removed, because the launch at `97e4aa682`'s parent (`dcb3a077b`) only checked
  `session_dir.exists()` and two racers both passed it, and
  `cargo nextest run -p north-lander -E 'test(a_withdrawn_lane_is_refused_with_the_withdrawal_reason_when_its_session_queues_the_same_head_until_the_withdrawer_clears_it_or_the_head_changes)'`
  fails with `land-ticket: together: a second instance started while the first was withdrawn ...` when
  `refuse_requeue` is not consulted, as enqueue did not at `f1ff89dde`'s parent (`249dbce78`). Both pass on
  origin/main, where the claim and the withdrawal check gate admission, and `north check machines --root .` runs the
  same searches at every change.
