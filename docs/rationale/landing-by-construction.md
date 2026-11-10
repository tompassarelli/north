# Landing by construction

On the morning of 2026-11-08 north's lander and fleet lost about two hours of landings, from 7:00
to 9:45 AM +07, to failures that a person noticed and fixed by hand. A lane that could not link
sank every batch it rode. A wall-clock budget refused probes on a loaded laptop. The lander ran a
build an hour old. Two good lanes were reverted for host faults. A new build host was sent checks
it could not run. Tom asked (2026-11-08) for each failure to be made impossible by how the system
is built, not patched case by case. This file names each class, the invariant that rules it out,
the mechanism that enforces the invariant, the replay of the day's incident that proves it, and
its status. north#892 tracks it, and land-lead's north#842 owns the throughput lanes it builds on.

## Five invariants

Most classes break one of five rules. A mechanism that enforces a rule rules out every class
under it, including ones that have not happened yet.

1. **Alone before together.** A lane joins a merged batch only after every verdict that judges the
   lane alone has passed: its diff gates, plus its build, link and selected tests on current main
   in the probes' environment. One bad lane then costs one solo check, never a batch. Bisection
   is left with interaction failures only, which are rare.
2. **No verdict is not a fail.** A check has three outcomes: pass, fail and no verdict. Only a fail
   that the subject itself produced, and that is differential, may refuse, hold or revert.
   Differential means it fails on the lane where the same check passes on the lane's parent on
   that host, or it fails again on a second host. A wrapper that could not start, an unreachable
   host, a missing input or a timed-out judge is no verdict. A no verdict reruns elsewhere and
   pages as a host fault.
3. **Measure the work, not the machine.** No check or test reads wall time, host load or how many
   jobs share the host. Checks bound CPU time, instructions or events, and tests order events
   through an injected clock or a waited-on event. Health gates judge breakage (the wrong code
   live, a service down, growth across a window), never a snapshot count that rises with
   legitimate work.
4. **Read the source of truth by revision.** The lander, every host's shared configuration and
   every placement decision read a named origin revision or a measured fact, with a bounded lag
   that is reported. The lander names its build and its lag on every round, and hosts read south
   from a fetched origin/main. Offload places a check only where its declared inputs and a
   reachable path exist. Messages resolve their addressee against the live fleet.
5. **Every gate runs before it can bite.** Any gate a repository runs after landing (land.after,
   after-landing.check, a push-to-main CI job) also runs before landing, or a
   `land.after-only = REASON :: COMMAND` line names why it cannot. A gate that only runs after the
   push can only revert. It can never refuse.

## Classes

Each class lists its incident, root cause, invariant, mechanism, replay and status. Status is one
of: enforced (the sha on origin/main), in flight (the lane or worker that carries it), waiting
(on whom), or rejected (with why). Statuses below were checked at 10:00 AM on 2026-11-08, and each
worker updates its own class's line when its lane lands.

### 1. One bad lane fails a batch

- Incident: n842-recorder failed to link `-lseccomp` in every probe it rode from 8:07 AM, so about
  15 minutes of 10- to 11-lane rounds were refused. Diff-gate failures sank whole rounds the same
  way: n706-cost-words broke 5 gate_rows tests, n876-t3-fixed-default failed privacy,
  n882-context failed a code-health layers rule, and n882-plan failed citations.
- Root cause: probes enforce every land.check line on the merged tree, and nothing judges a lane
  alone before it rides one. Enqueue ran only the `@enqueue` lines (tree policy, check owners),
  plus clippy, which does not link. n842-recorder's failure was not environment divergence:
  flake.nix carries no libseccomp on any ref, and the link failed alike on gex45 and in the
  strategy replay. `bin/head-north` turned every build failure into exit 75, so a lane that could
  not build read as a transient. 24744118dc pulls a lane after two failed probes, which catches
  the failure after it has cost two batches.
- Invariant: alone before together (1).
- Mechanism: (a) every gate that judges only the lane's diff runs once at enqueue and refuses
  there (n842-lanegates). (b) Enqueue runs the lane's own land.check lines (build, link, clippy,
  footprint-selected tests) with the `@enqueue` lines on the probe build hosts. A failure refuses
  the lane, and a pass records the lane's head as its alone verdict. The round planner puts a
  lane without an alone verdict only in a solo probe, and only vouched lanes form cumulative
  prefixes. `bin/head-north` passes cargo's exit status through (n892-solo). Checking at enqueue
  adds one check before a lane is queued and nothing to its landing, where a solo probe for
  every lane would add a round to each landing (probe p50 156 s, p90 419 s, round p50 239 s
  over the 3 h to 10:00 AM).
- Replay: `cargo nextest run -p north-lander -p north-land-probes`. n842-recorder's link failure,
  n706-cost-words' tests, n876-t3-fixed-default's privacy failure and n882-context's layers
  failure are each refused, or held in a solo probe, before any multi-lane probe carries them.
- Status: in flight. n842-lanegates (diff gates, lanes 9e003671b5 and 3ef579cf4d queued) and
  n892-solo (alone build, link and tests at enqueue). Caught today, not prevented: 24744118dc.

### 2. A check or test passes or fails on host load

- Incident: the north-replay `--budget` bound of 10 ms wall time per call refused probes at 37 to
  97 ms from 8:07 AM while the laptop ran at load 54 to 90. The ship slot-order test in
  north-runtime land_offload failed under load in the am-move probe. The threads_ledger test
  `the_ledger_host_over_ssh_spools_while_down_serves_a_stale_snapshot_and_flushes_in_order`
  panicked after 30 s in n882-context's probe.
- Root cause: checks and tests used wall time as a measure (budgets, p90-derived deadlines) or a
  sleep as an ordering assumption.
- Invariant: measure the work, not the machine (3).
- Mechanism: land checks bound CPU time or instructions (n842-loadaudit audits every land.check
  line and safe-push step in docs/rationale/landing-latency.md, and n716-replay-cpu moves the
  replay budget to CPU time). For tests, the Rust habits check's ambient-test-input rule, which
  already refuses ambient host reads in test code (ec1231f242), also refuses Instant::now,
  SystemTime::now and sleeps outside a named clock or event fixture (n892-testclock).
- Replay: the ship and threads_ledger tests pass 20 of 20 under `stress-ng --cpu 0` on a build
  host, where each failed before. The habits check refuses a planted wall-clock test.
- Status: in flight. n842-loadaudit, n716-replay-cpu and n892-testclock. The ship test's instance
  was fixed in dab9f6fcf4.

### 3. The lander runs an old build without saying so

- Incident: the lander ran runtime 3efa5a3d from 8:00 to 9:06 AM, until land-lead swapped the
  lander link by hand, so none of that morning's lander fixes were live. Each `north land promote`
  held because one of its 8 replayed rounds came back transient, although none diverged.
- Root cause: promotion treated a replay that reached no verdict as a reason to hold. Nothing told
  anyone the lander's build was behind, and the doctor's alarm reached nobody for an hour.
- Invariants: no verdict is not a fail (2), and read the source of truth by revision (4).
- Mechanism: promote moves the link on matches plus transients and holds only on a divergence or
  zero matches. Its replays run on build hosts, and the doctor pages the proxy when the link lags
  more than 3 landings or 15 minutes (n842-promote). Every lander round line names the build the
  lander runs, read from its own executable, and its count of lander-input commits behind
  origin/main, with the hold reason. Past 3 behind it pages the proxy once per build, which also
  catches a stale current (north-892-d0221).
- Replay: `cargo nextest run -p north-lander -p north-lander-doctor`. Matches plus one transient
  round move the link, and a divergence still holds it. The 8:00 to 9:06 AM window prints
  `3efa5a3d, N lander changes behind main, held: REASON` on every round.
- Status: in flight. n842-promote (b2f53919ed queued) and north-892-d0221 (n892-lander-build,
  2a616914fc queued).

### 4. A withdrawn lane keeps poisoning a round, and two culprits defeat bisection

- Incident: at 8:33 AM the lander named 8 suspects and probed eight 12-lane leave-one-out
  candidates. Two suspects were independently bad (n882-plan and n882-context), so all 8 failed
  after more than 11 minutes. Both tickets had been withdrawn after the round began.
- Root cause: leave-one-out isolates one culprit, not k. A ticket leaving the queue did not cancel
  the probes carrying it.
- Invariant: alone before together (1) makes this rare. Bisection must still be correct for k
  culprits.
- Mechanism: the suspect set splits in halves and recurses (group testing, about k·log2(n)
  probes). A ticket leaving the queue directory cancels every probe carrying it, and the round
  respawns those probes without it.
- Replay: `cargo nextest run -p north-land-probes -p north-lander`. Two bad lanes among 8 suspects
  are isolated within two rounds, and a ticket withdrawn mid-round cancels only its probes.
- Status: in flight. n842-bisect.

### 5. A gate runs only after landing

- Incident: wisp af6ed3fa was caught by the after-landing code-health gate, and firn 710344e4 by
  the after-landing shellcheck, after both had reached main.
- Root cause: repositories could add after-landing commands with no before-landing copy.
- Invariant: every gate runs before it can bite (5).
- Mechanism: the built-in after-landing gates other than the CI budget (code-health, crate-lines,
  instruction-limits, generated-rewrite) also run as lane gates. A new "after-only reason" lane
  gate refuses an after-landing command that is neither a land.check nor named by a
  `land.after-only` line (north-892-d0231). Push-to-main GitHub Actions jobs come under the same
  gate (n892-ciparity).
- Replay: `north land scan code-health acd3c88..af6ed3f` in wisp refuses (complexity 16 over 15),
  and nixos-config's `land-checks.sh scripts` on fdf5a936..710344e4 exits 1 on shellcheck SC2329.
  `cargo nextest run -p north-landing lane_gates` refuses a lane adding wisp's push-to-main
  code-health job with neither a check nor a reason.
- Status: in flight. Reasons landed in south b00e47cb, makgora 3300cefa, wisp c2121f87 and
  nixos-config dd13c9128d (9b38429b23 relanded after a host-fault revert, class 9). North's lane
  n892-after-parity is queued as 0266a74ef7, and CI jobs are with n892-ciparity.

### 6. A host runs on stale shared configuration

- Incident: gex45 ran agents sync on a south checkout 23 commits behind, and cpx62 lacked the
  account profiles its new orchestration.toml lines named.
- Root cause: every host read `~/code/south/main/agents`, a work tree nothing kept current.
- Invariant: read the source of truth by revision (4).
- Mechanism: north-shared-config fetches south's origin/main and materializes `agents/`'s
  top-level files at that revision under `~/.local/state/north/shared-config/current`, every five
  minutes from the runtime update tick. A host refuses to run on a snapshot whose last successful
  fetch is over an hour old.
- Replay: `cargo nextest run -p north-shared-config`. A host whose south checkout lags its origin
  reads the origin's revision, and a stamp past the hour bound refuses.
- Status: in flight. 631ec41340, queued from the laptop because cpx62 could not queue (class 11).

### 7. Dispatch staffs a box a live lane holds

- Incident: n882-tax's claim on north#879 matched no box, so dispatch staffed a second worker.
  Forgotten sessions north-870-d0106 and north-870-d0111 kept their claims.
- Mechanism: a running claim naming a box slug the issue lacks holds every box of the issue
  (210c4f1383), and forget releases every claim the ledger lists for the session (c2cba7ead6).
- Status: north#888 owns it. This class is done when north#888 closes. Three of its boxes are open.

### 8. An alarm goes to a retired name

- Incident: crates/north-base/src/platform/land_notice.rs sends every conflict notice to
  `lander-lead`, a retired session.
- Root cause: senders hard-code session names, and delivery does not check that the addressee
  exists.
- Invariant: read the source of truth by revision (4). The addressee is a fact read at send time.
- Mechanism: every service send resolves its addressee against the live fleet. A name that is not
  live or parked goes to the proxy, prefixed with the intended name, and the hard-coded
  `lander-lead` goes.
- Replay: `cargo nextest run -p north-fleet -p north-base`. A send to `lander-lead` and to a
  forgotten session reaches the proxy.
- Status: in flight. n892-addressee.

### 9. A host fault reads as a red main and is reverted

- Incident: south 6dd9582d2d was reverted as 47778f0 at 9:12 AM. Its post-land check got
  "Failed to start transient scope unit: Remote peer disconnected" from systemd-run on cpx62b,
  which capacity host run then reported as the subject's exit 1, and the check passes on that
  commit. nixos-config 9b38429b23, a `.safe-push`-only change, was reverted as 37aa6bfa at
  9:43 AM. Its post-land `nix flake check --offline --no-build` ran on cpx62b, where a
  base16-schemes drv that a concurrent check's eval cache named had never been written. The drv
  is valid on cpx62 and gex45, and the same change relanded green as dd13c9128d.
- Root cause: the wrapper chain conflates its own failure with the subject's exit code, and the
  sheriff reverts on a single run on a single host.
- Invariant: no verdict is not a fail (2).
- Mechanism: a subject-started witness, so a wrapper, ssh or dev-shell failure is no verdict, not
  an exit code. The sheriff reverts only on a differential fail and otherwise records a host
  fault, reruns elsewhere and pages. The private-text judge's timeout (north#842) becomes the same
  no verdict.
- Replay: `cargo nextest run -p north-landing -p north-capacity-host`. Both incidents revert before
  and reach no verdict after.
- Status: in flight. n892-hostfault2, which follows n892-hostfault (retired mid-work at about
  10:05 AM) from scratch/n892-hostfault-wip.

### 10. Offload places a check on a host that lacks its inputs

- Incident: at about 9:20 AM the after-landing graph-guards line of 78d8e9c920 ran on cpx62b,
  which had no `graph.db`, and read as red main. Makgora's map-build line ran on cpx62 and cpx62b
  without their private input store entries, and both landings were reverted.
- Root cause: placement chose hosts by free slots, not by what a check reads.
- Invariant: read the source of truth by revision (4). Placement is a function of declared inputs.
- Mechanism: each land.check and land.after line declares its host inputs. Offload skips a host
  lacking one, a missing input is a host fault, and an input missing everywhere runs the check on
  the lander host.
- Replay: `cargo nextest run -p north-runtime land_offload`. The graph-guards line is never placed
  on a host without graph.db, nor makgora's map-build line on one without its store.
- Status: in flight. n892-hostinput: makgora's declaration landed as 2eb84969cc, and north's lane
  149b044696 is queued. Not yet covered: a store that exists but lacks pinned entries (a
  content-level declaration).

### 11. A host cannot queue because a pre-queue check reaches for an unreachable host

- Incident: from 78d8e9c920 (the kill scan at enqueue) no north lane could be queued from cpx62.
  The scan offloads to `landing.remote-hosts = gex45`, which cpx62 cannot resolve, and safe-push
  exited 75, twice for n882-retain.
- Invariant: read the source of truth by revision (4). Placement uses measured reachability.
- Mechanism: the kill scan surveys only hosts this host reaches, and a host that reaches none
  runs the scan itself.
- Replay: `cargo nextest run -p north-land-scans test_admission` covers cpx62's case.
- Status: in flight. n892-hostinput, in the same lane 149b044696.

### 12. The promise checker runs prose

- Incident: the fleet tick ran ". It should be on origin/main by 9:36 AM:" from n842-scanreuse's
  report and reported a broken promise.
- Root cause: backtick pairs were matched across sentences, and any span was run.
- Invariant: read the source of truth (4). A promise is parsed, not guessed.
- Mechanism: one backtick span on the time claim's line, paired within the line, whose first word
  resolves to a program. Anything else is no checkable promise, logged and never reported broken.
- Replay: `cargo nextest run -p north-fleet promises` runs only the report's
  `git ... merge-base --is-ancestor` command.
- Status: in flight. n892-promise.

### 13. A health gate reads busyness as broken

- Incident: from about 9:30 AM north-runtime-follow failed 8 runs in a row on "whiterabbit: 197
  transient north processes (bound 100)", mostly idle offload wrappers. The laptop and lander
  could not take the day's fixes, including the fix to this bound. 05a48fd15f had exempted
  resident landers from the same bound an hour earlier.
- Root cause: a snapshot count stood in for health, and the running runtime's gate judged the
  candidate that fixes it.
- Invariant: measure the work, not the machine (3).
- Mechanism: settle and promotion judge only the wrong runtime live, a service down, or growth
  across every sample of a window. The candidate's own probe code judges its promotion.
- Replay: `cargo nextest run -p north-runtime`. The 9:30 AM readings settle, and a count that grows
  across its window still fails.
- Status: in flight. n842-transient-jobs (354da0f8a1, queued urgent: counts one process per
  offload job) and n892-healthgate (by design).

### 14. Placement reads leased load as no room for a session

- Incident: at about 10:00 AM six north#892 workers waited for placement because gex45 read
  load 56 on 20 threads and cpx62 22 on 16. On gex45, 20 runnable LTO threads of one leased build
  ran in a capacity scope capped at about 4.5 CPUs, while the live sessions there stalled for CPU
  0.14 to 0.35% of the time (cpu.pressure avg10 of each north-fleet-*.service). When cpx62b
  became a launch host, placement chose it before its account profiles existed, and each launch
  failed on a profile with no settings.json.
- Root cause: placement compared the load average with the host's threads. Capped leases' runnable
  threads count in the load average without taking a session's CPU. Placement also did not check
  that the host held the account's profile.
- Invariant: measure the work, not the machine (3). The gate measures what it protects: whether
  the work a new session runs beside stalls for CPU.
- Mechanism: `north capacity host backlog` adds the highest CPU stall of each agent session and
  unleased agent job in agent.slice. Placement refuses on a load over the host's threads only
  while that stall passes 10%, or when the host's runtime does not report it.
- Replay: `cargo nextest run -p north-capacity saturation`. gex45's 10:00 AM reading is admitted,
  and the same load with a 15% stall is still refused.
- Status: in flight. 0e4ef2f92f (construct-lead). Not yet covered: placement checking the
  account profile on the chosen host (an input declaration, as in class 10).

### 15. A lane lands without the tests that guard its code

- Incident: on 2026-11-10 ds-1033 (5f42fb5463) moved the worker sandbox's path policy into north-base. Its alone and
  after-landing selections ran 4 tests. GitHub Actions then failed a worker_sandbox confinement test on that commit,
  main stayed red, and the next landing's culprit search blamed the innocent 95afbfbca.
- Root cause: selection ran only the tests its graph knew, and the failing test was added after the graph's last
  recording, so nothing could select it. A graph answer also replaced the crate-graph module table, which would
  have selected the test, and blame trusted a base whose own check had not run the failed test.
- Invariants: alone before together (1) and no verdict is not a fail (2). A test the map cannot prove unaffected runs.
- Mechanism: [test-selection.md](test-selection.md). Every test in the changed crates' closure runs unless the
  coverage map proves it unaffected. Guard suites always run for the worker sandbox, the privacy gate and the landing
  machinery. A full-run failure that the culprit's selection missed is a `selection-miss` incident that teaches the
  edge, and blame probes the base with the exact failed tests.
- Replay: `cargo nextest run -p north-land-checks -E 'test(/a_moved_module_selects_the_tests_that_reach_it/)'`
  selects the failing test for 5f42fb5463 against its parent.
- Status: in flight. North#1039's lanes n1039-select, n1039-guards, n1039-blame, n1039-miss, n1039-map, n1039-proof and
  n1039-enforce.

## What stays only caught

These are detected after the fact, not prevented, until the lanes above land:

- A lane that fails alone still rides probes until n892-solo lands. 24744118dc pulls it after two
  failed probes.
- An interaction failure, where two lanes pass alone and break together, is caught by the merged
  probe by design. That is the probe's job, and bisection (class 4) isolates it.
- A host whose store holds an input's path but not its pinned content (makgora's map-build store)
  passes placement and fails the check. Class 9's no verdict keeps that from reverting a lane
  once n892-hostfault lands.
