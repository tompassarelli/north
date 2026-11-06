# Landing latency: where a worker's commit waits before main

Measured on Mak'gora, 2026-10-28, while about 15 workers waited on their landings during a live
playtest. The numbers for that day are in South (agents/decisions/makgora/landing-speed-2026-10-28.md);
this file keeps the method and what each fix removed.

## Method

One landing is a worker commit and the queue branch that carries it. Join four records by commit:

- Commit time: the author date of each commit on main (`git log origin/main --format='%at %ct %s'`),
  which rebases and the Autoland lane trailer keep. Commit to main is `%ct - %at`; leave out
  Autoland's own `Regenerate reports` commits.
- Queue branch pushed: `~/.local/state/safe-push/pushes.tsv` (time, repository, `claude/land-*`
  branch, tip).
- Autoland stages: the run titles `Autoland test SLOT BRANCH SHA` and `Autoland train`
  (`gh run list --workflow autoland.yml --json displayTitle,createdAt,updatedAt,conclusion`), and the
  `autoland` commit statuses on each tip (`queued`, `testing alone on`, `passed alone on`,
  `landed as`, or a failure with its reason).
- Worker side: `pre-push: NAME passed (N s)` lines and `DEFER_*` admissions in the session
  transcripts (`north fleet show NAME`).

Report the median and p75 of commit to main for the window, then split the tail by stage: commit
to queue push, push to test start, the test, and test end to main, plus how many tests handed the
lane back to its worker and why.

## What the 2026-10-28 profile showed

- A clean landing costs about 7 minutes: a minute to the test slot and a 6-minute suite, after
  which the test run lands the lane itself.
- The tail came from hand-backs, not from queueing: a quarter of Autoland's tests returned the lane
  to its worker (conflicts with main, a stale Classic effect record, a red main), and each one
  restarts the worker's rebase, pre-push and test.
- Tips that passed alone while main moved on their paths waited for a train, which reran the suite.
- Rerunning a stopped `safe-push --to main` repeated every check even when Autoland already held
  the commit.
- Workers launched with briefs that declare no `Surface:` stayed on the laptop, where the gaming
  hold defers heavy and native work for as long as Tom plays.
- North's own landings could not offload their land checks while Tom played: the offload needs a
  runtime newer than the one installed, and the runtime build is itself heavy work the hold defers.

## Fixes and their rule

- A pre-push check whose fix needs the local machine runs before the hand-off as well as in
  Autoland (makgora `localFix`), because Autoland can only hand such a lane back.
- `safe-push --to main` resumes the wait when origin already holds `claude/land-<lane>-<HEAD>` at
  HEAD, because the checks already ran for that exact commit.
- Placement treats a box with no `Surface:` as `Needs: native, private` while this host is in game
  reserve, so it goes to a machine that serves them instead of waiting out the game.

## North, 2026-10-28

North has no Autoland: `safe-push --to main` runs the land checks on the lane's host and pushes
straight to main. Over the last 24 h, 141 landings had a median commit to main of 34 min (p75
143). Queue push to main took under a minute and the last amend or rebase to push about 6 min, so
the tail sits between the first commit and the last amend. It was split from the session
transcripts:

- The 30 slowest landings (172 to 819 min) were mostly idle sessions: lanes parked as WIP
  commits, waits on a planner or lead that never answered, and gex45's disk at 95%.
- In the 15 to 60 min band that sets the median, land checks and their reruns were the biggest
  stage (254 of 564 attributed minutes, a median of 4 `safe-push --to main` runs per landing),
  then the review-gate round (191 min). Capacity and hold waits added nothing.
- One clean land check for a one-line `src/` change took 469 s: `cargo test` 142 s, the
  owner check 28 s, clippy 18 s, and 55 shell and Python tests run one after another 267 s.
  Any `src/` change selects every script that invokes north.

Fix: `north land-tests` runs the selected shell and Python tests side by side, up to the CPUs
the land check's capacity lease grants, and runs every test after a red one, so one run names
every failure instead of the first.

Second fix: `cargo test` ran its 65 test binaries one after another, 223 s of a 277 s run
on a cold build. `harness/scripts/cargo-tests.sh` builds them with `cargo test --no-run` and
hands the binaries to `north land-tests`, so they run side by side. On the same head and a
warm build, the land check went from 124 s to 41 s; the unit-test binary, 29 s, is now the
floor.

Third fix, 2026-10-29: both local hosts sat at load 47 to 51 while GitHub Actions used 5 of its 40
runner slots, and north's land checks filled gex45's disk. North's `.safe-push` now names
`land.workflow = land-check.yml`, so each portable check runs as one job of that workflow and only
the checks that need this host (the owner check, the machine-bound harness tests and the dev-shell
test, each `land.local-only` with its reason) run locally. The landing lock is released for every
check run, not only the first three, so it covers the fetch, scans and push alone.

## North verification, 2026-11-06

One cargo verification of a one-line north-tui change, run on gex45 at load 25 to 57 in an offload
slot whose target sat at an older commit: clippy on north-tui and north 69 s, their unit tests 27 s,
the whole tests/it suite 174 s (680 tests). Three causes, each measured:

- A job took the first free of the host's six slots, whatever commit that slot last built, so most
  checks rebuilt crates main had already built in another slot. A job now fetches its bundle first
  and tries the free slots in order of how many files differ between each slot's checkout and its
  head; a slot with no target comes last.
- cargo test runs one test binary at a time. The after-landing test check is one `cargo nextest
  run` over the affected packages' lib and bin tests and tests/it, each test its own process. On
  ccx33 (8 cores) tests/it took 142.7 s wall for 1,008 test-seconds; the slowest test took 38 s and
  the fleet_launch module alone 276 test-seconds. nextest selects exactly the tests the replaced `cargo test`
  commands list (3,668 for every module, 1,659 for one north-landing and north-runtime change).
- gex45 ran six slot jobs at once at load 45 to 57 on 20 threads, and a job waited up to 5 minutes
  for a slot. Under that load nextest's tests/it run took 200 s, two threads_snapshot tests took 72
  and 78 s, and two land_serve::precheck tests that compare sub-second check start and end stamps
  failed; on ccx33 at load 6 both passed.

Skipping a test binary whose crate and dependency sources match a recorded pass is not sound here:
27 crate test files read paths outside their crate through `CARGO_MANIFEST_DIR` (`../..`, other
crates' sources, `harness/`, `bin/safe-push`), so a crate's own source hash does not cover its
tests' inputs. The land-cache keeps its whole-check reuse keyed on every path the check names.

After those changes, clippy lints only the crates whose files changed (`SAFE_PUSH_LAND_CHANGED`), the
nextest check builds tests/it only when a module is selected, and the separate `cargo check --tests`
after line is gone. Timed the same morning through `north capacity offload run`, each run in the slot
nearest its head, the slot last built at main:

- One-crate change (a line in north-tui), on ccx33 at load 6 to 11: clippy on north-tui 4 to 5 s,
  nextest build and 248 tests 14 to 15 s (tests 7.4 s), 19 s in the slot, 23 s wall end to end. The
  same change on a slot that had not built main took 62 s (clippy 16 s, nextest 46 s).
- The same change on gex45 at load 37: clippy on north-tui and north 37 s, nextest 49 s, 86 s in the
  slot and 151 s wall, 60 s of it waiting for one of six busy slots.
- Hub change (a line in north-core), on ccx33: clippy on north-core 11 s, nextest 58 s (its build 35 s
  over north-core's seven dependents, 1,287 tests 18.8 s), 73 s wall. Under 20 s needs the build and the
  tests split: build on gex45's 20 threads instead of ccx33's 8, run the tests from one `cargo nextest
  archive` partitioned over gex45 and ccx33, and finish north-core's split (north#259) so a north-core
  edit stops rebuilding north-guard and north-ops (34,000 lines each) whose code does not use it.

gex45 is the binding constraint: through the morning it ran six slot jobs at once at load 37 to 57 on
20 threads, and a job waited 1 to 5 minutes for a slot, longer than the verification itself.

## Warm check slots (2026-11-06)

One after-landing run on ccx33 at 6:45 AM took 141 s from start to verdict. Its spans: the shared
`head-north --build` 12 s (10 s of it in line for a slot), nextest 20.5 s with a 0.1 s build because
the slot was already at main, eleven head-north checks of 0.1 to 5 s each, and `land-tests --select
changed --only portable` 125 s, the critical path. The run before it, held on the laptop, spent 56 s
building head-north and 55 s building tests. Four causes, three fixed together:

- gex45 deadlocked. Five of its six north slots stayed held for 60 to 80 minutes by worker jobs queued
  in the capacity gate at load 0.7: a land check that finds no host with room claims the host, the
  claim defers every heavy and moderate lease, and the deferred leases were the jobs holding the slots
  the land check waited for. A lease asked from inside an offload slot (`NORTH_OFFLOAD_SLOT`, set by the
  slot for its command) no longer yields to the claim.
- Slot targets were wiped before most jobs on ccx33: its 224 GB disk had 102 GB free against a
  100 GiB floor, and two slots held 22 GB against a 24 GiB cap. The floor is now 30 GiB, twice
  admission's 15 GiB build floor, and the cap 40 GiB.
- sccache never hit across slots: its base directory was `~/code`, so every key held the slot's
  absolute path; the cache held 1 GB of a 30 GiB budget. Each slot now names itself as the sccache base
  directory, on a port of its own, so a wiped or new slot rebuilds dependencies from the shared cache.
- The in-order slot queue (110c8759b) polled every 2 s and its waits printed no line the span reader
  knew, so a queued job's wait showed as sync, growing 2 s per job ahead of it (1.5 to 26 s across one
  run's 14 jobs). It polls every 0.2 s and prints `offload: waited Ns in line`, booked as host wait.

The remaining critical path is `bin/safe-push.test.sh`, which every Rust change selects because it
calls the north binary: 98 s mean over 138 runs (max 1,583 s), against 54 s for `bin/threads.test.sh`
and under 11 s for every other shell test. Its four parallel sections are unbalanced (section 3 holds
1,250 of 2,584 lines); the 30 s target needs that test split or ported, tracked on north#634.
