# The Phase 2 cut plan: buckets 1 to 3 loose, bucket 4 out of the workspace

This page orders the Phase 2 cuts of north#1650, starting from the five-step order of the issue's "Inventory review and decisions" section, and checks each edge, `.safe-push` line, blocker and fleet call against the code. Written 2026-11-16 at main 16192c759: the proven-core inventory, [proven-core.md](proven-core.md), on top of e87d0a1601, the archive commit that keeps bucket 4 in history. Where the code disagrees with the review or the inventory, the code wins and the step says so.

Owner: north#1650.

## What Phase 2 must reach

Buckets 1 to 3 build alone, each crate depending only on crates in its own bucket or a lower one. Landing knows nothing of issues, reviews, dispatch or the fleet. Bucket 4 (28 crates, 122,005 lines by the inventory, plus the root verbs that call them) leaves the workspace. Bucket 5 (`north-guard`, `north-agent-policy`, `north-code-health`, `north-concerns`, `north-implicit-state`, `north-mutants`) stays in the workspace behind `north policy`, so no core crate may depend on it. The issue's Done when: each core crate builds and passes its tests alone, and a lane lands from the laptop with every build host switched off.

Every lane below lands through the lander on the land checks `.safe-push` already selects. No lane adds a gate, a rule or a process.

## Facts every lane works under

- Every lane touches a `Cargo.toml` or `Cargo.lock` (or `.safe-push`, `bin/` or `.github/`). All of them are in `trust_boundary_paths` (harness/defaults.toml:1986), so under the enforced review-accept land check (.safe-push:37) each lane lands only with a reviewer's accept of its patch. That is the main fixed cost of Phase 2. The one-shot security read (partly built in crates/north-lander/src/land/review_request.rs) would lower it, but it is a separate box of north#1650 and this plan does not wait for it.
- Lanes that touch `landing_path_paths` (harness/defaults.toml:1994: safe-push, the lander and landing crates, the offload's land paths, `crates/north-runtime/src/worker_sandbox/`, `crates/north-runtime/src/runtime/`, depbox) are landing-machinery lanes. They run their own tests before hand-in, per AGENTS.md.
- The land checks of a candidate come from the `.safe-push` committed in that candidate (docs/landing.md:9). So a lane that drops a check line is not judged by that line. The `land scan safe-push-verbs` check (.safe-push:62) refuses a `.safe-push` line whose verb is missing from `src/cli/verbs.rs`. That is why the lines that call a verb must go before the verb does.
- Removing a verb means removing its entries under `docs/features/`, `src/cli/verbs/`, `src/cli/reference/` and `src/cli/help/` in the same lane (AGENTS.md routers). Removing a crate means removing its `Cargo.toml` member and dependency lines, its `structure/*-crate-dag-crate-NAME.toml` entry, its lines in tests/it/crates.txt and its `BUCK` targets (`north buck targets` writes them; .safe-push:27-29 checks them).
- Sizes are Rust lines at 16192c759, counted with `wc -l`. "Moved" lines are file moves that git shows as renames.

## Verified edges

Each core crate's `[dependencies]` path lines were read from its manifest. Each use was counted as the files under the crate's `src/` that name `CRATE::`.

Core edges into bucket 4 or 5, as the code has them:

- `north-landing` → `north-github-mirror` (crates/north-landing/Cargo.toml:18) and → `north-mutants` (crates/north-landing/Cargo.toml:20): 0 files use either one. These are manifest-only edges.
- `north-guard` (bucket 5) → `north-threads` (crates/north-guard/Cargo.toml:21) and → `north-github-mirror` (crates/north-guard/Cargo.toml:22): 0 files. Manifest-only.
- `north-lander` → `north-graph-ground` (crates/north-lander/Cargo.toml:33): 0 Rust uses. The one hit, crates/north-lander/src/land/enqueue_tests.rs:263, is a string written into a fixture file. Manifest-only.
- `north-lander` → `north-concerns`: 1 file. crates/north-lander/src/land/enqueue.rs:218 `pays_debt` puts a lane first when it pays down a concerns ceiling.
- `north-landing` → `north-concerns`: 2 files, the after-landing concern attribution in crates/north-landing/src/land/after/concerns.rs (255 lines, plus 90 lines of tests) and its call at crates/north-landing/src/land/after/mod.rs:117.
- `north-landing-gates` → `north-mutants`: 3 files. crates/north-landing-gates/src/landing/fails_first.rs and crates/north-landing-gates/src/landing/flake.rs use the mutants crate's git, env and tree helpers. The fails-first runner is what test admission replays (.safe-push:72), and test admission stays. Only crates/north-landing-gates/src/landing/mutant_runs.rs belongs to mutation runs.
- `north-landing-gates` → `north-graph-loops`: 1 file, crates/north-landing-gates/src/landing/check_timings.rs:11 (the check-deadline helpers, crates/north-graph-loops/src/check_deadline.rs, 89 lines).
- `north-code-health` (bucket 5) → `north-graph-loops`: 1 file, crates/north-code-health/src/code_health/signal.rs:8 (crates/north-graph-loops/src/code_health.rs, 74 lines).
- `north-ci` → `north-github-mirror`: 3 files, all in `ci_watch` (the sheriff, 1,204 lines). `private_check` names no bucket 4 crate.
- `north-land-scans` → `north-github-mirror`: 4 files. These are crates/north-land-scans/src/docs_owners.rs, crates/north-land-scans/src/stranded_boxes.rs, crates/north-land-scans/src/lib.rs:112 and crates/north-land-scans/src/tool_removal.rs:129. The tool-removal land check (.safe-push:32) runs the stranded-boxes scan through the mirror.
- `north-lanes` → `north-github-mirror`: 4 files, all in `gc`: crates/north-lanes/src/lanes/gc/audit.rs, crates/north-lanes/src/lanes/gc/evidence.rs, crates/north-lanes/src/lanes/gc/liveness.rs and crates/north-lanes/src/lanes/gc/remote.rs (1,107 lines between them).
- `north-offload` → `north-capacity-host`: 14 files. Most of these name modules that `north-capacity-host` only re-exports from `north-capacity` (crates/north-capacity-host/src/capacity/mod.rs:1 is `pub use north_capacity::capacity::*`). Only `host::{land_check_cpus, cpu_limit, urgency, job_refusal, lease_waiting}` belong to `north-capacity-host` itself, the last two being lease waits.
- `north-offload` → `north-pacing`: 4 files. They read the host table: `Machine`, `machines`, `ssh_destination` and `job_source` at crates/north-pacing/src/pacing/placement/gate.rs:64-420, and `config` at crates/north-pacing/src/pacing/placement/inputs.rs:10.
- `north-offload` → `north-dispatch`: 1 file. crates/north-offload/src/land_offload/room.rs:4 reads a host's memory through crates/north-dispatch/src/dispatch/measure.rs (78 lines).
- `north-runtime` → bucket 4. Outside `worker_sandbox`: the canary (calibration, providers), crates/north-runtime/src/runtime/dispatch_record.rs (dispatch, pacing), crates/north-runtime/src/runtime/trip_command.rs (unblocks), and `laptop_runner` (pacing placement, calibration's `shell_quote`). It also uses `north-runtime-records` in 49 files, 44 of them for its 24-line error type in crates/north-runtime-records/src/failure.rs.
- `north-agent-policy` and `north-guard`'s always-on public-text scan → `north-pace`, for crates/north-pace/src/usage/profiles.rs (193 lines) only. The guard's other bucket 4 edges (fleet-store, pace, calibration, unblocks, graph-ground) are in 29 coordinator and fleet guard modules (3,856 lines, plus 657 lines of tests).
- Bucket 1 to 3 crates the review did not list: `north-graph-ingest` → threads (9 uses), fleet (5) and mirror (3); `north-reclaim` → fleet (2) and threads (1); `north-remote-serve` → fleet-supervisor (1) and calibration (1).
- `north-strategy-types`, `north-shared-config`: no edge up. The first depends on nothing; the second only on `north-trusted-mirror`.

Where the code disagrees with the review or the inventory:

- `north-runtime-follow` is not a shallow leaf to cut. The review and the first inventory put it in bucket 4, but the code makes it a 518-line primitive that depends only on `north-base`. Its runtime tree layout and collection carry the remote checks (crates/north-offload/src/land_offload/ship/pinned.rs, crates/north-offload/src/land_offload/ship/script.rs). Its runtime link is what the broker, the GitHub broker, the session host and the worker sandbox follow (crates/north-runtime/src/worker_sandbox/broker.rs). Decision 9 keeps deploy. This plan therefore keeps the crate in bucket 1, as the inventory now records, and cuts only what promotion uses: `leaving`, `LANDER_ROLLBACK`, and crates/north-lander/src/land/build_lag.rs and crates/north-lander/src/land/link_hold.rs, in step 5.
- `north-runtime-records` is only partly governance, and the inventory now records it as split. `failure` (24 lines) is `north-runtime`'s error type and moves into it. `hold_ledger` (306 lines) carries review accepts through the sandbox (crates/north-runtime/src/worker_sandbox/hold_request.rs) and moves with the sandbox. Only `after_landing` (118 lines) leaves.
- `north-mutants` is bucket 5, not a leaf of the core. The core needs its helpers, so they move down into `north-landing-gates`, and the edge turns around: mutants then depends on landing-gates.
- `north-strategy-types` does not shrink to "envelope and land" alone, as the inventory now records. `admission` is the record type of the kept local compile admission (crates/north-capacity/src/capacity/admission_record.rs). `github::Quota` is read by crates/north-base/src/platform/github_quota.rs. `placement`, `credit`, `tokens` and `brief` lose their users only when bucket 4 leaves, so they are dropped in the eviction lanes, not in step 3.
- `capacity host run` (.safe-push:2) is not a mothballed verb. It is how the kept local compile admission is entered: crates/north-capacity-host/src/capacity/gate.rs:99 calls `host::run`. The worker sandbox also wraps confined builds in it (crates/north-runtime/src/worker_sandbox/tests.rs:342-371). It stays, and only its lease bookkeeping goes, in step 4.
- The capacity slice is entangled. depbox (crates/north-capacity-host/src/capacity/depbox.rs:19) uses `gate`, and `gate` calls the lease runner `host::run`. So depbox cannot move to bucket 1 or 3 without the gate and a lease-free runner.
- The warm spares (crates/north-fleet/src/fleet/spares.rs, 385 lines) have no caller outside the fleet launch path. They are reached only from crates/north-fleet-supervisor/src/fleet/registry/spares.rs and src/cli/fleet/spares.rs. They read north-fleet's launch model, remote, runtime and store types. Keeping them, as the inventory decides, means taking those types along (step 4).
- 3 of the 22 worker_sandbox integration tests exercise fleet calls: `a_worker_send_to_a_session_on_its_host_or_another_exits_0_with_no_error_and_leaves_the_row_for_the_fleet_tick_north_1219` and `a_confined_seat_shows_another_sessions_transcript_and_sends_it_a_row_with_the_ledger_a_schema_behind_north_1485` (fleet-supervisor spool), and `a_sandboxed_worker_opens_a_gate_and_records_a_box_check` (gates, threads). They leave in the lane that removes those fleet calls. The other 19, both tests of tests/it/depbox_broker.rs and the test of tests/it/agent_build_sandbox.rs pass on every lane.

## The sandbox's fleet calls

Read from crates/north-runtime/src/worker_sandbox/ (9,198 lines; with `agent_contain`, 9,571):

- Fleet-only request kinds: planner (`north-pace`'s model family), judge, eval-unit (eval start, build, units), review-call (with its verdict store, review request and lane fetch in north-landing) and review-fetch. These are 1,491 lines with their clients and tests.
- Accept carrying: hold (crates/north-runtime/src/worker_sandbox/hold_request.rs) records review accepts through `north-runtime-records`' hold ledger and crates/north-runtime/src/runtime/review_accept.rs. The review-accept land check still reads those accepts, so the hold kind stays until the one-shot read replaces that check.
- Fleet reads inside core kinds: `finish` reads `LANES_HANDED` from the fleet session lifecycle (crates/north-runtime/src/worker_sandbox/finish_request.rs). crates/north-runtime/src/worker_sandbox/brokered_reads.rs reads fleet records, crates/north-runtime/src/worker_sandbox/capability.rs reads the fleet `Role`, crates/north-runtime/src/worker_sandbox/readable.rs and crates/north-runtime/src/worker_sandbox/request_env.rs read provider runtimes and keys, and crates/north-runtime/src/worker_sandbox/host_op_request.rs reads `north-unblocks`. That is 419 lines across four files, plus the host-op part.
- Edges inside the crate: the sandbox uses `runtime::review_accept`, `runtime::update::pull_held` and, in tests, `runtime::scratch_project_probes`. Going the other way, only the canary (crates/north-runtime/src/runtime/canary/probes.rs:180-222) and `laptop_runner` (`agent_contain`) reach into the sandbox.

## The offline blocker

bin/head-north:17-21: on a host with no `/etc/buildbarn/store-generation` that is not GitHub Actions, it runs only the tree's build-ahead artifact, fetching it through `runtime build-ahead --fetch`. It exits 75 when none can be had. bin/head-north:22 bootstraps a host with no live runtime by a Buck2 `--remote-only` build, which also needs a server. crates/north-runtime/src/runtime/tree_build.rs:4-5 states the same rule. `north runtime tree-build` already builds with Cargo on a host whose `buck2_tree_build` is unset, so the fallback reuses it.

## `.safe-push` lines that call mothballed verbs

Each verb was traced to the code that runs it:

- .safe-push:30 and its `local-only` twin .safe-push:97: `land scan gate-shadow`, a self-policing scan (inventory, rows the review added).
- .safe-push:33 and .safe-push:34: `check machines`, which is src/cli/machines.rs on `north-machine-model` (mothballed, decision 5). Its file list names `north-threads`, `north-fleet-store` and capacity lease files.
- .safe-push:35: `land scan holdlist-guards-resolve`, self-policing.
- .safe-push:36: `land scan turn-scope`. It guards the mechanism-discovery loop through docs/rationale/discovery-turns.toml and docs/rationale/experiments.toml, and is hold H13 in harness/holdlist.toml:149-157.
- .safe-push:40 and .safe-push:101: `land scan alarm-definitions`, which reads crates/north-fleet-store/src/fold.rs.
- .safe-push:60, .safe-push:79 and .safe-push:108: `check citations`, which is `north-claims` (bucket 4, depends on `north-fleet-supervisor`).
- .safe-push:63 and .safe-push:111: `check experiments`, which is `north-flow` (bucket 4).

Lines that stay, with the reason from the code:

- .safe-push:2: the admission entry (see above).
- .safe-push:37-38: review-accept and safe-push-keys, the ring-0 review until the one-shot read replaces it.
- .safe-push:42: mayday, the north-free bottom of the degradation ladder.
- .safe-push:48-50: `code-health` layers, gate and habits. The layers check is the crate DAG check that shows the cuts hold.
- .safe-push:66-68: operator config.
- .safe-push:70: agent owners, after-only, bucket 5.

.safe-push:44 and .safe-push:45 name `north-threads`, `north-capacity-host` and `north-fleet-store` packages. They change in the lanes that remove those packages. The `[mutant]` table (.safe-push:10-16) stays with `north-mutants` in bucket 5.

`.github/workflows/sheriff.yml` builds north and runs `north main-red` (`north-ci`'s `ci_watch`). No other workflow calls a north verb.

## The cut order

Five steps, as the review ordered them. The code shows only these real orderings: step 3's sheriff lane needs step 1; step 4's spares and step 5's sandbox lanes come before the evictions that remove their old homes; and each eviction lane needs every lane that cut a core edge into the crates it removes. Steps 1 and 2 can land side by side.

### Step 1: unblock the landing gates

Before it: nothing. Green after it: each lane's own land checks, which here are nix-entry (.safe-push:61), safe-push-verbs (.safe-push:62), safe-push-keys and review-accept, plus the harness tests selected for `bin/`.

- Lane 1a, the gate lines (about 75 lines removed).
  - Drop .safe-push:30, 33, 34, 35, 36, 40, 60, 63, 79, 97, 101, 108 and 111.
  - Delete `.github/workflows/sheriff.yml` and rewrite the "Main is red" sentence at docs/landing.md:17.
  - Remove hold H13 from harness/holdlist.toml, with a `Holdlist:` trailer naming north#1650.
  - The test crates/north-lander/src/land/enqueue_tests.rs:269 reads the real `.safe-push` and expects `check experiments`, so drop that entry from its list in the same lane.
- Lane 1b, the offline fallback (about 12 lines changed). In bin/head-north:17-21, when no build-ahead artifact can be fetched and a live runtime exists, run `"$live" runtime tree-build` (Cargo) instead of exiting 75. With no live runtime, bootstrap with `cargo build` through `bin/land-shell --dev` instead of the remote-only Buck2 build at bin/head-north:22. Update crates/north-runtime/src/runtime/tree_build.rs:4-5 to match. This is a landing-machinery lane, so it runs tests/it/land_checks.rs before hand-in.

### Step 2: cut the shallow leaf edges

Before it: nothing. Green after it: clippy and the tests of each changed crate, the crate DAG check (.safe-push:48), Buck target checks (.safe-push:27-29) and the tests/it modules that crates.txt selects.

- Lane 2a, manifest-only edges (about 15 lines removed): crates/north-landing/Cargo.toml:18 and :20, crates/north-guard/Cargo.toml:21-22, crates/north-lander/Cargo.toml:33, their `may_use` entries under `structure/`, `Cargo.lock` and `BUCK`.
- Lane 2b, concerns (about 360 lines removed): crates/north-landing/src/land/after/concerns.rs, crates/north-landing/src/land/after/concerns_tests.rs, the call at crates/north-landing/src/land/after/mod.rs:117, `pays_debt` and its use at crates/north-lander/src/land/enqueue.rs:218-228, and the manifest lines crates/north-landing/Cargo.toml:19 and crates/north-lander/Cargo.toml:17.
- Lane 2c, mutants (about 350 lines moved, 40 changed): move crates/north-mutants/src/env.rs, crates/north-mutants/src/trees.rs and the `process_env` and `config` helpers into `north-landing-gates`, then point crates/north-landing-gates/src/landing/fails_first.rs and crates/north-landing-gates/src/landing/flake.rs at them. Move crates/north-landing-gates/src/landing/mutant_runs.rs into `north-mutants`, which then depends on `north-landing-gates`, and point src/cli/tools/subverbs.rs at it. tests/it/mutant_worker_lane.rs stays green.
- Lane 2d, graph-loops (about 165 lines moved): crates/north-graph-loops/src/check_deadline.rs goes into `north-landing-gates` and crates/north-graph-loops/src/code_health.rs into `north-code-health`.

`north-runtime-follow` stays (see "Where the code disagrees").

### Step 3: cut the issue and ledger edges, settle the base types

Before it: lane 1a for lane 3a. Green after it: as in step 2. Lanes 3d to 3f are landing-machinery lanes and run the landing crates' tests before hand-in.

- Lane 3a, `north-ci` (about 1,250 lines removed): delete `crates/north-ci/src/ci_watch/` and the `main-red` verb (src/cli/tools.rs:90) with its docs, verbs, reference and help entries. `north-ci` keeps `private_check`, which depends only on `north-base` and `north-foundation`.
- Lane 3b, land-scans (about 300 lines removed): delete crates/north-land-scans/src/docs_owners.rs, crates/north-land-scans/src/stranded_boxes.rs and crates/north-land-scans/src/stranded_boxes_tests.rs, the `docs-owners` arm at crates/north-land-scans/src/lib.rs:112 and the stranded-boxes call at crates/north-land-scans/src/tool_removal.rs:129, plus the mirror line in its manifest.
- Lane 3c, lane GC (about 1,100 lines touched, 600 net removed): lane GC judges a lane by git state alone (landed, merged or gone), not by its issue. Edit the four `gc` files above and drop the mirror line in the manifest.
- Lane 3d, landing knows no issues (about 750 lines removed): delete crates/north-landing/src/box_tick.rs, crates/north-landing/src/issue_body.rs and crates/north-landing/src/issue_body_tests.rs, and their callers in crates/north-landing/src/landing/after_landing.rs, crates/north-landing/src/land/lane_gates.rs and crates/north-landing/src/land/scans/boxes.rs.
- Lane 3e, red-issue matching (about 350 lines removed): the `gh issue list` lookup and `red_fix` at crates/north-lander/src/land/enqueue.rs:120-160, crates/north-lander/src/land/serve/alone.rs, crates/north-lander/src/land/enqueue/alone.rs, and the `RedFix` ordering in crates/north-landing/src/land/queue.rs.
- Lane 3f1, the sheriff (about 1,320 lines removed): crates/north-landing/src/land/sheriff.rs, `crates/north-landing/src/land/sheriff/`, crates/north-landing/src/land/sheriff_tests.rs and crates/north-landing/src/land/red_episode.rs.
- Lane 3f2, culprit search (about 570 lines removed): crates/north-landing/src/land/blame.rs, crates/north-landing/src/land/blame_tests.rs, crates/north-landing/src/land/bisect.rs and crates/north-landing/src/land/incidents.rs. The after-landing run and its revert of the landed lane (.safe-push:89) stay.
- Lane 3g, ledger edges of buckets 1 to 3 (about 400 lines):
  - `north-graph-ingest` keeps its code loaders and drops the issue and ledger loaders.
  - `north-reclaim` drops its fleet and threads reads.
  - `north-remote-serve` drops its fleet-supervisor and calibration reads.
  - `laptop_runner` stops using calibration's `shell_quote`.
  - src/main.rs stops writing the `NativeSolo` evidence row to the providers ledger.

Base types: the code shows no upward edge in `north-strategy-types`, `north-shared-config` or `north-ci`'s `private_check`. Their unused types go in the eviction lanes, once nothing uses them.

### Step 4: cut placement, take the capacity slice and the warm spares

Before it: nothing for 4a, 4b and 4c. 4a2 needs 4a1. Green after it: as in step 2, plus tests/it/depbox_broker.rs, tests/it/agent_build_sandbox.rs, tests/it/carry_matrix.rs and tests/it/capacity_agent_compile.rs, which crates.txt selects for these crates. 4a1, 4a2 and 4b are landing-machinery lanes.

- Lane 4a1, the capacity slice moves (about 1,700 lines moved, 80 changed). Move `crates/north-capacity-host/src/capacity/` depbox, depbox_broker, depbox_check, depbox_mode, gate, seat_stall and `crates/north-capacity-host/src/run_bounded/` into `north-capacity`, with `host::run` and `host::parse_window_seconds` reached through the capacity-host side for now. Point every user that named them through `north_capacity_host::capacity` at `north_capacity::capacity`: north-offload, north-runtime, tests/it/depbox_broker.rs:37 and the crates.txt lines. No behavior changes.
- Lane 4a2, the lease cut (about 800 lines). `capacity host run` keeps its verb and its admission through `crates/north-capacity/src/capacity/admission/`, and drops the lease database (crates/north-capacity-host/src/capacity/host/database.rs and its use in `crates/north-capacity-host/src/capacity/host/run/`). Its runner moves into `north-capacity`. `.safe-push`:45's `-p north-capacity-host` changes with it.
- Lane 4b, offload placement (about 600 lines). Move the host table (the `Machine`, `machines`, `ssh_destination` and `job_source` part of crates/north-pacing/src/pacing/placement/gate.rs, plus crates/north-pacing/src/pacing/placement/inputs.rs:10) and crates/north-dispatch/src/dispatch/measure.rs into `north-capacity`. Drop the offload's lease waits in crates/north-offload/src/land_offload/warm.rs. Delete crates/north-runtime/src/runtime/dispatch_record.rs and its call. Point `laptop_runner` at the moved table. `north-offload` then depends on base, foundation, build-system, capacity, lanes and runtime-follow alone.
- Lane 4c, the warm spares (about 1,200 lines moved). Move crates/north-fleet/src/fleet/spares.rs, the fill and take of crates/north-fleet-supervisor/src/fleet/registry/spares.rs, and the launch-spec, host-spec and store-path types they read into a new bucket 1 crate beside `north-ssh-spare`, with its tests from crates/north-fleet/src/fleet/link_tests.rs. The cheaper alternative evicts the spares with the fleet and restores them from e87d0a1601 when parallel warm workers return in Phase 4. This plan follows the inventory's keep.

### Step 5: split north-runtime, trim the guard and the CLI, evict bucket 4

Before it: steps 1 to 4 for the lanes noted. Green after it: as in step 2, with every worker_sandbox, depbox_broker and agent_build_sandbox test selected and run on each lane that touches the sandbox, and `verify-north` before each runtime lane's hand-in, per AGENTS.md.

- Lane 5a1, the fleet-only request kinds out (about 1,100 lines removed): planner, judge and eval-unit in `crates/north-runtime/src/worker_sandbox/`, and their entries in crates/north-runtime/src/worker_sandbox/typed_requests.rs.
- Lane 5a2, the fleet reads out (about 500 lines):
  - Edit crates/north-runtime/src/worker_sandbox/brokered_reads.rs, crates/north-runtime/src/worker_sandbox/capability.rs, crates/north-runtime/src/worker_sandbox/readable.rs, crates/north-runtime/src/worker_sandbox/request_env.rs, crates/north-runtime/src/worker_sandbox/finish_request.rs and crates/north-runtime/src/worker_sandbox/host_op_request.rs so that no sandbox file names `north-fleet-supervisor`, `north-providers`, `north-unblocks` or `north-pace`.
  - Delete the 3 fleet-call worker_sandbox tests named above, with the helper at tests/it/worker_sandbox.rs:564 and the fleet uses at tests/it/worker_sandbox.rs:293 and :487-494.
- Lane 5a3, review-call and review-fetch out (about 400 lines), together with the review-call verdict store, review request and lane fetch in north-landing. Before it: the lander makes the one-shot security read of a held lane itself (crates/north-landing/src/land/security_read.rs, north#1650), so no review-routing caller is left. The hold kind and crates/north-runtime/src/runtime/review_accept.rs stay with review-accept.
- Lane 5b, the sandbox crate (about 9,700 lines moved, about 400 changed): the riskiest lane.
  - `git mv` `crates/north-runtime/src/worker_sandbox/` and `crates/north-runtime/src/agent_contain/` into a fresh bucket 3 crate.
  - Take crates/north-runtime/src/runtime/review_accept.rs, `update::pull_held` and the hold ledger along, so the new crate depends on nothing in `north-runtime`. `north-runtime` then depends on it (canary probes, `laptop_runner`), never the reverse.
  - Move crates/north-runtime-records/src/failure.rs into `north-runtime`.
  - Update in the same commit: src/main.rs:28; the `worker_sandbox` paths in `trust_boundary_paths` and `landing_path_paths` (harness/defaults.toml:1986-1997), so the moved sandbox stays ring-0 for review-accept; tests/it/crates.txt; `structure/`; `BUCK`.
- Lane 5c1, the canary out (about 2,330 lines of whole files removed, about 50 edited): `crates/north-runtime/src/runtime/canary/` and crates/north-runtime/src/runtime/canary.rs, its calibration and providers edges, and hold H4 in harness/holdlist.toml.
- Lane 5c2, promotion out (about 2,700 lines of whole files removed, about 150 edited), with holds H2 and H3 in harness/holdlist.toml:
  - In `north-runtime`: `crates/north-runtime/src/runtime/watch/`, crates/north-runtime/src/runtime/watch.rs, `crates/north-runtime/src/runtime/lag/`, crates/north-runtime/src/runtime/lag.rs and the unblocks use in crates/north-runtime/src/runtime/trip_command.rs.
  - In `north-lander`: `crates/north-lander/src/land/promote/`, crates/north-lander/src/land/build_lag.rs, crates/north-lander/src/land/link_hold.rs and `crates/north-lander/src/land/shadow/`.
  - `north-runtime-follow`'s `leaving` and `LANDER_ROLLBACK` go too.
  - Covers, tree-build and deploy stay (decision 9).
- Lanes 5d1 to 5d3, the guard trim (about 4,500 lines removed in three lanes of about 1,500). Delete the 29 coordinator and fleet guard modules named in "Verified edges", their tests and their rows in crates/north-guard/src/hook.rs. Move crates/north-pace/src/usage/profiles.rs into `north-base`, so the always-on public-text scan and `north-agent-policy` stop needing `north-pace`.
- Lanes E1 to E7, the evictions, from the top of the crate DAG down. Each lane removes the root verbs and their files under `src/cli/` that call the crates it evicts. Of the 216 files in `src/`, 145 (15,556 lines) name a bucket 4 crate or a `src/main.rs` alias of one. Each lane also removes the bucket 4 crates' directories and their tests/it modules, rows of `src/main.rs` (including the `RUN_TIME_READERS` validators of the crates it removes), docs entries, `structure/` entries and `.safe-push`:44-45 package names, and the strategy-types and shared-config items left with no user.
  - E1: `north-tui` with its monitor, `north-desktop`, `north-land-watch`, `north-lander-doctor`, `north-machine-model`, `north-claims` and `north-context-pack` (16,353 crate lines).
  - E2: `north-flow`, `north-health`, `north-tune`, `north-expectations`, `north-timers` and `north-issue-boxes` (25,004).
  - E3: `north-core` and `north-convo` (14,441).
  - E4: `north-dispatch` and `north-pacing` (17,112), after 4b.
  - E5: `north-providers` and `north-calibration` (16,688), after 3g, 5a2 and 5c1.
  - E6: `north-fleet-supervisor`, `north-fleet-store` and the rest of `north-fleet` (about 25,700), after 4c, 5a2 and 5d.
  - E7: `north-threads`, `north-pace`, `north-control-plane`, `north-unblocks`, `north-github-mirror`, `north-graph-loops`, `north-graph-ground` and what is left of `north-runtime-records` (about 20,200), after steps 2 and 3 and lanes 5b and 5d.

## Lane size and the 2,000-line ceiling

Every lane above except the evictions edits at most about 2,000 lines of files that stay; 5c1 and 5c2 also delete whole files past that, counted as below. Lane 5b moves about 9,700 lines, but as renames, with about 400 lines changed. The evictions delete whole crates (16,000 to 26,000 lines per lane). This plan holds each eviction lane to about 2,000 edited lines in surviving files and does not count whole-file deletions toward the ceiling. A deleted crate is checked by the build failing if anything still names it, and that check does not grow with the crate's size. Counting deletions toward the ceiling would split E1 to E7 into about 70 lanes, each needing an accept, with no added safety. If an eviction lane's edits to surviving files (mostly `src/cli/`) pass about 2,000 lines, it splits by verb family.

That makes 34 lanes: 2 in step 1, 4 in step 2, 8 in step 3, 4 in step 4, and 16 in step 5 (5a1 to 5a3, 5b, 5c1, 5c2, 5d1 to 5d3 and E1 to E7). Each needs a review accept.

## The riskiest lane and how it is de-risked

Lane 5b is the riskiest. `north-runtime` had 853 commits and 33 reverts in the 14 days to 2026-11-16 (inventory), the most of any crate. The sandbox is ring-0, and moving it also moves the paths that make review-accept guard it. It is de-risked by order and shape:

- It lands after 5a1 to 5a3 and before 5c1 and 5c2. Its only edges into `north-runtime` (review accept and fetch, `pull_held`, the hold ledger, the test-only `scratch_project_probes`) are then the four the lane moves along.
- It is one commit of renames and path rewrites with no behavior change. `git diff -M --stat` shows renames, and the reviewer's accept is on that small diff.
- `trust_boundary_paths`, `landing_path_paths`, tests/it/crates.txt and `structure/` change in the same commit. The moved sandbox stays ring-0, and crates.txt still selects every worker_sandbox, depbox_broker and agent_build_sandbox test for the lane.
- It runs those tests and `verify-north` before hand-in, as a landing-machinery lane. It lands with no other `north-runtime` lane queued, so a red after-landing run reverts it alone.

The second riskiest is lane 4a2. It changes how every heavy build and every depbox run is admitted. It is de-risked the same way, by landing after the pure move of 4a1, with tests/it/carry_matrix.rs and tests/it/capacity_agent_compile.rs on the lane.

## Phase 2 done

After E7, the workspace members are buckets 1 to 3 and 5. The crate DAG check (.safe-push:48) passes on the reduced `structure/` tables. Each core crate's tests pass in the after-landing workspace run (.safe-push:89). A lane then lands from the laptop with gex45 and cpx62b switched off, which lane 1b makes possible. Those are existing checks, read in order, not new ones.
