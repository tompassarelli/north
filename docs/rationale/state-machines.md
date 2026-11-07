# State machines: one typed state, one record, every move logged

On 2026-11-07 north lost hours to state that moved through phases while nothing could say it was stuck: a snapshot pull marker whose mtime meant "running" after its pull was killed, a dispatch pause held by a stopped systemd timer that a deploy restarted, a policy generation held on every host for four hours, an API organization marked exhausted by a bare file, session reports lost when the reaper archived their sessions, and issues staffed twice because filing one did not claim it. Each is a state machine that north holds implicitly, in marker files, mtimes, unit state, status strings and boolean pairs. This note lists where each such machine lives today, ranked, and the one pattern every one of them moves to; each has a box on north#757.


## Inventory

Swept on 2026-11-07 across every crate, `bin/`, `lib/` and `harness/hooks/` by six read-only passes at 289fe31ea:
about 200 places where long-lived state moves through phases without one typed record. They group into the 36
machines below, each one box on north#757 sized for one worker. Rank is fix commits since 2026-10-24 whose message
names the machine (keyword-matched over `git log`, so a coarse count) times the cost of getting it wrong: 3 when it
loses or duplicates work or stops the whole fleet, 2 when it silently stops one flow or host, 1 when it wastes time.
The incident ledger lives on the ledger host and was not readable from the inventorying host, so commits stand in for
incidents. Shapes name the north#758 rule that matches each machine's code.

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
   move, so the two still race in the order they commit.
2. **Land ticket lifecycle** (330: 110 x 3). A ticket's phase is its queue directory (`Q/`, `outbox/`, `done/`,
   `withdrawn/`, `quarantine/`, remote `outbox/taken/`) plus the first word of its `verdict` string, read by value in
   fifteen places (`crates/north-landing/src/land/{bisect,records,incidents,reservation,replay,promote,sheriff}.rs`,
   `crates/north-lander-doctor/src/facts.rs:129`, `crates/north-fleet-supervisor/src/fleet/tickets.rs:60`). `settle` writes
   `done/` then removes the pending file (`land/queue.rs:175-186`), so a crash leaves both; `quarantine/` has no
   exit; `replace_older` deletes a ticket with no record (`land/enqueue.rs:148-165`). The landing intent
   (`Q/landing/intent.json`, `recovering.json`) keeps its tickets out of every candidate while recovery fails
   (`land/jobs.rs:93,119`), and `finish` removes the lane worktree before deciding its ticket (`land/after/mod.rs:136`).
   Shapes: phase-suffix (directory as phase), status-string.
3. **After-landing run, red-main episode and base-broken pause** (255: 85 x 3). The run's phase is spread over
   `Q/following/HEAD.json`, `Q/logs/post-land-SHA.{runner,status,outcome}` and process-group liveness
   (`land/watch.rs:26-186`, `land/post_land/runner.rs:25-99`); `misses` lives in memory, so a restart resets the
   alarm, and a reused process group passes for a live runner. The red episode marker `post-land-red-REPO` opens only
   if its alarm got through (`land/post_land/settle.rs:126`); `Q/base-broken` keeps `alerted=true` for the next
   breakage (`land/pause.rs:49`); strikes reset whenever main moves (`land/bisect.rs:49-118`). Shapes: presence-flag,
   pid-liveness.
4. **Issue claims** (195: 65 x 3). Four rules decide "held" (`crates/north-threads/src/threads/claims/ledger.rs:95`,
   `claims/verbs.rs:134-183`, `board/mod.rs:166`, `work_items.rs:126`); closing an issue never deletes its claim; a
   session-host close releases the claims of an idle-parked session that can still be revived
   (`crates/north-fleet/src/fleet/watch.rs:483`); a failed launch leaves claims on a name that no longer exists
   (`fleet/registry/launching.rs:73`); a refused re-claim is only logged (`fleet/claims.rs:47`). Filing an issue did
   not claim it until 22350314c. Shapes: status-string (`standing == "live"`).
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
   queue and the stopped-session filter stays a query.
7. **Threads spool and mirrored-client mode** (88: 44 x 2). `~/.config/threads/remote` presence switches a host to
   snapshot reads and spooled writes (`crates/north-threads/src/threads/client.rs:118-125`); a refused write is
   renamed `.failed` and never retried, and health skips `.failed` (`client.rs:454`,
   `crates/north-health/src/health/flows/facts.rs:37`). Shapes: presence-flag, phase-suffix.
8. **Gates** (84: 28 x 3). Open means `closed_ms IS NULL` in each host's `ledger.db`
   (`crates/north-providers/src/ledger/gates.rs:35-55`); other hosts see a deleted-and-reinserted `fleet_gates`
   snapshot (`crates/north-fleet/src/fleet/store/gates.rs:31`); the answer is delivered after the close with no retry;
   the default timer is a detached process that does not survive a reboot; a spooled answer is set aside as spoofed
   (north#747, which owns that bug). Shapes: status-string (NULL-encoded phase).
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
   decision strings.
10. **Lane retirement** (75: 25 x 3). Lane gc's activity from reflog and mtimes, `flagged_at` carried in
    `stale.json`, reap's age from a `.git` mtime that counts as old enough when unreadable
    (`crates/north-lanes/src/lanes/reap.rs:320`), rescue's partial end states, pin sidecars removed after their
    worktree (`lanes/pin_retire/consumed.rs:54`). Shapes: mtime-read, presence-flag.
11. **Job queue and job runs** (68: 34 x 2). A `taken` job has no lease or taker
    (`crates/north-threads/src/threads/jobs/queue.rs:178-224`); a crashed job's `job_runs.finished` stays NULL and
    JOB-START never meets a JOB-EXIT (`crates/north-ops/src/ops/job_run/records.rs`). Shapes: status-string.
12. **Offload slots** (66: 33 x 2). `SLOT.{lock,holder,owner,pgid}`, queue tickets held by flock, the warmer's
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
16. **API organization exhaustion** (57: 19 x 3). A bare file `api-exhausted/ORG` per host, never cleared, read for
    presence in five places (`crates/north-providers/src/providers/claude/api_pool.rs:73-119`, `accounts.rs:63-102`,
    `crates/north-pace/src/credit.rs:213`, `usage/credit_table.rs:19`, `src/cli/fleet/limit.rs:64`), overlapping the
    computed balance with nothing reporting a contradiction. Shapes: presence-flag.
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
    strings (`crates/north-dispatch/src/laptop/tick.rs:80,95`); draining stays forever when the job queue is
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
- Caches with a time to live (`gh_cache`, `github-mirror.db`, `usage-gate.json` as a reading, tune `cells.json`) and
  content-addressed verdicts (`verdict_cache`, `private-text-verdicts`): a key, not a phase.
- Already explicit: goal trend (`crates/north-goals/src/history.rs`), capacity `Mode` with its transition functions
  and `adjustments.jsonl`, the convo index's `files` table, flow episode alerts (`ops/flow/alerts.rs`, the closest
  existing template), and claims' `take`, a real transition in one transaction.
- Codex conduct state (`crates/north-guard/src/guard/codex_conduct/`): north builds nothing new for Codex
  (standing order of 2026-10-16).

## The pattern

Owner: north#757. The module is `north_base::lifecycle` (`crates/north-base/src/lifecycle.rs`).

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
