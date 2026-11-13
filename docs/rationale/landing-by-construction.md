# Landing by construction

On the morning of 2026-11-08 north's lander and fleet lost about two hours of landings, from 00:00
to 02:45 UTC, to failures that a person noticed and fixed by hand. A lane that could not link
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
   is left with interaction failures only, which are rare. A repository whose landing base names
   its test line in a `land.after-only` line (north alone, north#1036) keeps the diff gates, the
   build and link of the rebased tree, clippy, the scans and the manifest check before landing,
   and its tests judge the landed commit, where a red run reverts the lane (invariant 5).
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
   push can only revert. It can never refuse. A reason may name a land.check line too, and that
   line then leaves the checks before queueing: north's test line does so, read from the landing
   base so a lane cannot grant itself the exception, while `tests_after_landing` in
   harness/defaults.toml is on and the lane changes no path in `trust_boundary_paths` (class 5).

## Classes

Each class lists its incident, root cause, invariant, mechanism, replay and status. Status is one
of: enforced (the sha on origin/main), in flight (the lane or worker that carries it), waiting
(on whom), or rejected (with why). Statuses below were rechecked against origin/main bd4d7feae on
2026-11-10, each named sha passing `git merge-base --is-ancestor SHA origin/main`, and each worker
updates its own class's line when its lane lands.

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
- Mechanism: every check of this class runs once before the ticket is written, and refuses there
  or, under shadow, logs its would-be refusal: the lane-local `land.check` lines marked
  `@enqueue` (citations, code-health
  layers, experiments, test admission, gate-shadow, the tests/it module list, the nix lock and
  nix-entry, safe-push-verbs and run-time-data, 5b770fd9d7); the lane's clippy, which builds and
  links it, and its land test selection (`refuse_queue_checks`); the privacy scans (secrets,
  deny-list, commit email, native refs), where a judge that reaches no verdict withholds the pass
  and the lander judges it again alone before it lands (263e4e51ca); and the built-in after-landing
  gates (code-health, crate-lines, instruction-limits, generated-rewrite), which now run as lane
  gates at enqueue and at the pick (521411db26); beside them the lane gates run test admission (the
  bug needs its test), test integrity (the wall-clock scan), the incident test, tool removal and
  the policy sweep. A pass records `alone_green` and `alone_base` on
  the ticket; a lander round batches only a pass whose head is the lane's and whose base is main or
  at most `landing.alone-pass-landings` first-parent commits behind it, and a lane without a
  current pass is checked alone on current main before it joins a batch (f5fd775d71,
  `land/serve/alone.rs`). The `@enqueue` land.check lines refuse in either mode; a lane gate
  outside them follows `landing_refusals`, so in shadow it logs its would-be refusal at enqueue and
  cannot fail a batch later. Privacy judges each commit's added prose as its own segment, so one
  lane's text never fails another's pass (4127fb389a). One ticket is batched without a pass: the
  sheriff's revert, whose tree is the main its culprit replaced and so the side of a red main that
  already passed.
- Replay: `cargo nextest run -p north-lander the_lane_local_land_checks_that_refused_n882_context_in_every_probe_run_before_its_ticket_is_written`
  reads the check set the enqueue runs and pins n882-context's layers, experiments and test
  admission, n882-plan's citations, the habits wall-clock scan, and the clippy and land test
  selection that carry n842-recorder's link and n706-cost-words' tests; the same test file's
  `@enqueue` refusal fixture refuses a lane whose check fails. `cargo nextest run -p north-landing an_alone_pass_vouches_for_its_lane_until_main_gains_more_landings_than_the_setting_allows_or_stops_descending_from_the_main_it_was_checked_on`
  pins that only a current pass batches, and `cargo nextest run -p north-landing-gates two_lanes_adding_lines_to_one_doc_are_judged_as_separate_segments`
  replays n876-t3-fixed-default's privacy failure.
- Status: enforced. 5b770fd9d7, 521411db26, 263e4e51ca, 4127fb389a and f5fd775d71 (which replaced
  the probe rounds and the enqueue-time full land.check run with the alone pass this class now
  rests on), with the replay above added by this lane.

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
  host, where each failed before. The habits check refuses a planted wall-clock test. The
  threads_ledger failure replays red on its own trees: with the flush holding the spool lock 3 s
  after seeing the host down, 05a48fd15f fails 3 of 3 at threads_ledger.rs:184 after 34 s and
  origin/main fb8466f217 passes 3 of 3 in 7 s, because both failing trees predate dea8714681, which
  lets a detached flush wait its turn rather than exit Busy. The scan's replay is
  `cargo nextest run -p north-land-checks -E 'test(/no_land_check_or_test_reads_wall_time_or_host_load/)'`.
- Status: in flight. The three races are enforced: the ship ordering test in dab9f6fcf4, the
  threads_ledger spool flush in dea8714681, and the replay budget, timed by thread CPU time since
  be5886ab3 (north#716). The test-code scan landed advisory in 2c86fcbe9 and refuses once north#716
  records a week with no false hit. The land-check half, a scan refusing a land check that reads
  wall time or host load, is in flight in session north-892-d1203-no-land-check-or-test-can-pass;
  its replay test is not yet on origin/main.

### 3. The lander runs an old build without saying so

- Incident: the lander ran runtime 3efa5a3d from 8:00 to 9:06 AM, until land-lead swapped the
  lander link by hand, so none of that morning's lander fixes were live. Each `north land promote`
  held because one of its 8 replayed rounds came back transient, although none diverged.
- Root cause: promotion treated a replay that reached no verdict as a reason to hold. Nothing told
  anyone the lander's build was behind, and the doctor's alarm reached nobody for an hour.
- Invariants: no verdict is not a fail (2), and read the source of truth by revision (4).
- Mechanism: promote moves the link on matches plus transients and holds only on a divergence or
  zero matches. Its replays run on build hosts, and the doctor pages the proxy when the link lags
  more than 3 landings or 15 minutes (n842-promote). The lander names the build it runs, read from
  its own executable, and the lander-input commits behind origin/main it lacks whenever that note
  changes at the start of a serve round, over the limit once the first of them landed more than 15
  minutes ago, since promote-on-landing passes several changes within minutes (north-892-d0221,
  north#1048); the doctor's link-behind bound is the one page for a lagging lander, since the lander follows its link within
  seconds (north#1081).
- Replay: `cargo nextest run -p north-lander -p north-lander-doctor`. Matches plus one transient
  round move the link, and a divergence still holds it. The build_lag replay names a build six
  lander changes behind main as within the limit after 2 minutes and over it after 17:
  `a_lander_missing_lander_code_main_landed_over_fifteen_minutes_ago_names_its_build`; a lander
  checking a lane moves to its promoted link within 10 s: `a_busy_lander_moves_to_a_promoted_link_within_its_bound`.
- Status: enforced. n842-promote landed as cd2922adad and north-892-d0221 as
  1bc7787e31af64a04159f42190d62fc111fafb82, both ancestors of origin/main.

### 4. A withdrawn lane keeps poisoning a round, and two culprits defeat bisection

- Incident: at 8:33 AM the lander named 8 suspects and probed eight 12-lane leave-one-out
  candidates. Two suspects were independently bad (n882-plan and n882-context), so all 8 failed
  after more than 11 minutes. Both tickets had been withdrawn after the round began.
- Root cause: leave-one-out isolates one culprit, not k. A ticket leaving the queue did not cancel
  the probes carrying it.
- Invariant: alone before together (1) makes this rare. Bisection must still be correct for k
  culprits.
- Mechanism: after `north graph ask blame` narrows the suspects, one binary search per failing
  check finds the lane whose prefix first reaches it, and every prefix a search needs is checked
  once, so k independently bad lanes are isolated in at most k·log2(n) prefix checks with no
  revert between the searches. A red run with one failing check keeps the old prefix bisection,
  and a bad lane whose check already failed earlier is found by the next run after the first
  lane's revert lands.
- Replay: `cargo nextest run -p north-lander --test it -E 'test(land_first::a_red_after_landing_run)'`
  reverts exactly the two bad lanes of an eight-lane batch, the first and last, in at most 6 prefix
  checks, and `cargo nextest run -p north-landing --lib -E 'test(land::blame)'` pins the two
  searches for two bad lanes among eight at no more than 2·log2(8) prefix runs.
- Status: enforced. The k-culprit bisection landed as f2560cb0e. The withdrawn-lane half is
  rejected: land-first (7e285c4f35) deleted the probe rounds, so no round outlives a withdrawn
  ticket. North#892's box for this class names a replay test of its own
  (`two_bad_lanes_among_eight_are_isolated_in_at_most_six_bisect_runs` in north-landing), which is
  in flight in session north-892-d1203-a-red-after-landing-run-isolates.

### 5. A gate runs only after landing

- Incident: wisp af6ed3fa was caught by the after-landing code-health gate, and firn 710344e4 by
  the after-landing shellcheck, after both had reached main.
- Root cause: repositories could add after-landing commands with no before-landing copy.
- Invariant: every gate runs before it can bite (5).
- Mechanism: the built-in after-landing gates other than the CI budget (code-health, crate-lines,
  instruction-limits, generated-rewrite) also run as lane gates. A new "after-only reason" lane
  gate refuses an after-landing command that is neither a land.check nor named by a
  `land.after-only` line (north-892-d0231). It also refuses the push-to-main GitHub Actions job a
  lane adds or changes unless every run step is a land.check or a `land.after-only` line names
  the job's workflow file (`.github/workflows/canary.yml`) with its key appended as `:JOB`
  (n892-ciparity); a job already on main before the lane is the repository's inherited gate and
  is not re-judged, so a repository can record reasons for its existing jobs without stopping
  every lane.
- Replay: `north land scan code-health acd3c88..af6ed3f` in wisp refuses (complexity 16 over 15),
  and firn 710344e4's `push.sh` change was caught after landing by its `tests.yml` shellcheck job.
  `cargo nextest run -p north-landing lane_gates` refuses a lane adding wisp's push-to-main
  code-health job and a lane adding firn's push-to-main shellcheck job with neither a check nor a
  reason, passes a lane that changes no push-to-main job, and passes each job with a reason or
  with every run step listed as a land.check.
- Status: enforced (north-892-d0231 and n892-ciparity). Reasons landed in south b00e47cb, makgora 3300cefa,
  wisp c2121f87, firn ac135e3da10e and nixos-config dd13c9128d (9b38429b23 relanded after a
  host-fault revert, class 9). North's own after-only reasons landed with 521411db26.
- Revision (north#1036, 2026-11-13): a `land.after-only` reason on the landing base may also name a
  land.check line, which then leaves the checks before queueing, by the owner's direction that a
  change making landing faster goes live with its revert ready and is proven by backtest. North names
  its test line so (83 to 112 s of a one-line core change), and its after-landing run reverts a lane
  that turns it red. The diff gates, the rebased build and link, clippy, the scans and the manifest
  check stay before landing. A lane on a `trust_boundary_paths` path keeps the line before landing,
  and `tests_after_landing = "off"` in harness/defaults.toml restores it for every lane. No other
  repository names such a line. A ticket that left its test line records it, so the after-landing
  selection never counts that lane's tests as already run.

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
- Status: in flight. The first lane landed as 566d24dd8f and was reverted as c6c5fc0c6b after its
  after-landing nextest failed; its branch scratch/n892-shared-config is gone. North#892's
  shared-configuration box carries the relanding, and dispatch staffs it as a lane of its own;
  its replay is `cargo nextest run -p north-ops -E 'test(/a_host_refuses_configuration_older_than_main_by_more_than_the_bound/)'`.

### 7. Dispatch staffs a box a live lane holds

- Incident: n882-tax's claim on north#879 matched no box, so dispatch staffed north-879-d0116
  beside it on 2026-11-08, and north-873-d0051 ran beside n882-brief whose brief named its arm
  box. Forgotten sessions north-870-d0106 and north-870-d0111 kept the boxes they had claimed.
- Root cause: the per-box check read a claim whose slug matched no box as holding nothing and
  could not see a box another issue's brief named; forget released only the launch record's claim
  keys, never the threads ledger's.
- Invariant: one session per box, decided before a session starts on it (4).
- Mechanism: a running claim naming a box slug the issue lacks was held against every box of the
  issue (210c4f1383) and since north#1054 holds none, dispatch listing it as stale; a launch whose brief names another issue's box claims that box too
  (cc9f745bff86); forget releases every threads claim the ledger lists for the session, even
  when its host left placement (c2cba7ead6, 1e19a696c1); a launch whose box claim the ledger has
  not confirmed does not start, and a turn handed a new box takes that box's claim first. The
  fleet tick counts duplicate stagings per day.
- Replay: `cargo test -p north-fleet-supervisor claim` names the 2026-11-08 pairs and passes on
  main e8ba82581d67; `cargo test -p north-dispatch a_claim_on_a_slug` refuses a second staff of
  north#879, and `cargo test -p north-fleet claims` refuses north-873-d0051 while n882-brief runs.
- Status: enforced. north#888 closed on 2026-11-09 with every box ticked: 210c4f1383,
  cc9f745bff86, c2cba7ead6 and 1e19a696c1 are on origin/main, and the supervisor replay above
  passed on e8ba82581d67.

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
- Status: enforced. 89ab826dba sends the lander's alarms to the live lander owner, else to the
  proxy marked with the intended name, and a40793dcd bounds every service's pages to the proxy in
  the fleet store's send path.

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
- Status: enforced. n892-hostfault2 landed as 6a13c3fff2; its replay is
  `cargo nextest run -p north-landing -p north-capacity-host -E 'test(a_lease_whose_scope_never_started_its_subject) | test(a_check_that_fails_only_on_one_host)'`.

### 10. Offload places a check on a host that lacks its inputs

- Incident: at about 9:20 AM the after-landing graph-guards line of 78d8e9c920 ran on cpx62b,
  which had no `graph.db`, and read as red main. Makgora's map-build line ran on cpx62 and cpx62b
  without their private input store entries, and both landings were reverted.
- Root cause: placement chose hosts by free slots, not by what a check reads.
- Invariant: read the source of truth by revision (4). Placement is a function of declared inputs.
- Mechanism: each land.check and land.after line declares its host inputs. Offload skips a host
  lacking one, a missing input is a host fault, and an input missing everywhere runs the check on
  the lander host. A `STORE@PINS` input names a content-addressed store and the directory at the
  check's head that holds one pin file per family, so the declaration resolves to the exact
  `STORE/FAMILY/ENTRY` the build reads rather than to the store path alone.
- Replay: `cargo nextest run -p north-runtime land_offload` places no check on a host without
  graph.db, nor makgora's map-build line on one without its store or with a store that lacks its
  pinned entry; `cargo nextest run -p north-lander -E 'test(the_lane_local_land_checks_that_refused_n882_context_in_every_probe_run_before_its_ticket_is_written)'`
  resolves a store declaration to the head's pinned entries.
- Status: enforced. The path declarations and placement landed as 1338880a64 and the pinned-entry
  declaration as d1a08f50db37; makgora's declaration landed as 2eb84969cc. A store that exists
  but lacks a pinned entry takes no check that reads it (north#892).

### 11. A host cannot queue because a pre-queue check reaches for an unreachable host

- Incident: from 78d8e9c920 (the kill scan at enqueue) no north lane could be queued from cpx62.
  The scan offloads to `landing.remote-hosts = gex45`, which cpx62 cannot resolve, and safe-push
  exited 75, twice for n882-retain.
- Invariant: read the source of truth by revision (4). Placement uses measured reachability.
- Mechanism: the kill scan surveys only hosts this host reaches, and a host that reaches none
  runs the scan itself.
- Replay: `cargo nextest run -p north-land-scans test_admission` covers cpx62's case.
- Status: enforced. 1338880a64; the replay is
  `cargo nextest run -p north-land-scans -E 'test(a_mutant_kill_inside_an_offload_slot_runs_on_the_slot_host_and_tries_only_hosts_it_reaches)'`.

### 12. The promise checker runs prose

- Incident: the fleet tick ran ". It should be on origin/main by 9:36 AM:" from n842-scanreuse's
  report and reported a broken promise.
- Root cause: backtick pairs were matched across sentences, and any span was run.
- Invariant: read the source of truth (4). A promise is parsed, not guessed.
- Mechanism: the last backtick span on the time claim's line whose first word resolves to a
  program, with runs paired within the line the way Markdown pairs them: a run closes only
  against a run of its own length, and a run with no partner stays literal text rather than
  re-pairing the spans after it (north#1041). Anything else is no checkable promise, logged and
  never reported broken.
- Replay: `cargo nextest run -p north-fleet promises` runs only the report's
  `git ... merge-base --is-ancestor` command, and n842-reuse's two report lines yield only their
  promised `git ... log origin/main ... | rg -q` command, never the prose after `n842-reuse-plan`.
- Status: enforced. The Markdown pairing landed as 9f8fb36be and the replay of n842-reuse's real
  report lines as be5e0f66b (north#1041). North#892's box for this class names its replay in
  north-core (`promise_checker_runs_only_the_command_the_report_names`), not yet on origin/main,
  so that box stays open until the named replay exists.

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
- Status: enforced. 56d733b8ff judges transient north processes by growth across the window, not
  a snapshot count, and settles with the runtime it judges, so the gate never blocks the update
  that carries its fix; its replay is `cargo nextest run -p north-runtime growth`.

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
- Status: enforced. The stall gate landed as 340dd8bcf8, and `north capacity host backlog` prints
  the unleased CPU stall as its fourth field. The missing profiles are enforced at their cause:
  agents sync creates every declared Claude profile on its host (28440aad91, pinned by d2332dc34b,
  north#900), so a launch host no longer lacks one.

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
  n1039-enforce; n1039-map and n1039-miss are running now.

### 16. A build that offload declines runs on the laptop under a playtest hold

- Incident: at 10:12 AM on 2026-11-08 offload declined n892-testclock's
  `cargo build --locked -p north --bin north` because the build writes files the caller runs, and
  `capacity host run` then built it on the laptop during the playtest hold, while laptop load
  reached 80.
- Root cause: a declined offload fell through to a local run, and the playtest hold judged only
  the work it was asked to place, not the fallback.
- Invariant: measure the work, not the machine (3), read the other way: the hold is a fact about
  the host, so no fallback path may spend it.
- Mechanism: under a playtest hold, `capacity host run` queues or refuses a build it cannot
  offload, and never runs it on the laptop.
- Replay: `cargo nextest run -p north-capacity-host a_build_offload_declines_as_an_artifact`.
- Status: enforced. 6adefaae54.

### 17. Agents take the laptop while Tom works at it

- Incident: on 2026-11-10 the laptop ran at load 76 to 110 from 3:05 to 3:36 PM and 208 at
  4:46 PM with agents at 2200% while Tom worked at it. On 2026-11-08 the same quota held the laptop
  at load 54 to 90 through the morning's probes (class 2).
- Root cause: the desk mode kept the away quota of 22 of 24 cores and kept the laptop a build host.
- Invariant: measure the work, not the machine (3). The desk's bound is set by who is at it.
- Mechanism: the desktop row of `capacity_mode_table` caps agents at 12 CPUs and is no longer a
  build host, so heavy land checks go to the servers; away keeps 22 of 24. `north capacity mode
  relieve` and the playtest hold work on top of it. Decided on 2026-11-10 by gate 213.
- Replay: `cargo nextest run -p north-capacity -p north-pacing`, 53 of 53 passed on the rule.
- Status: enforced. 69093c66c4. North#892's box for it stays open on its live check,
  `north capacity mode status | rg -q 'quota.*12' && awk '{exit !($3<24)}' /proc/loadavg`, which
  passes once the proxy's temporary hold and relief lapse.

## What stays only caught

These are detected after the fact, not prevented:

- An interaction failure, where two lanes pass alone and break together, is caught by the
  after-landing run of the batch by design, since land-first (7e285c4f35) lands a batch whose lanes
  each passed alone and checks the merged tree after the push. Bisection (class 4) then reverts
  only the culprits.
