# The proven core: every part of north sorted into five buckets

Phase 1 of north#1650 (agreed with the owner on 2026-11-16): each crate under `crates/`, each hook north registers, each skill under `harness/skills/`, each systemd unit and timer the hosts run for north, and each top-level `north` verb, sorted into one of five buckets with a call of keep, simplify or mothball and the recorded evidence behind it. In bucket 5, mothball means off by default behind `north policy`. Where nothing recorded a catch, the row says "no recorded catch".

Owner: north#1650. It follows docs/rationale/leaner-north.md (north#970), which sized cuts by line count; this page sorts by what each part caught and cost.

## Summary for marking

Buckets: 1 execution primitives (keep), 2 check and land (keep; the public-repo secret and private-text scans sit here and stay on always), 3 sandbox and broker (simplify), 4 fleet governance (mothball), 5 policy (opt-in).

Crates (80; Rust lines are every `.rs` file under the crate, tests included, at e87d0a1601):

- Bucket 1: 16 crates, 58,406 lines; 8 keep, 8 simplify
- Bucket 2: 24 crates, 90,540 lines; 12 keep, 10 simplify, 2 mothball
- Bucket 3: 7 crates, 30,468 lines; 3 keep, 4 simplify
- Bucket 4: 27 crates, 121,487 lines; all mothball (north-runtime-records split: its failure type moves into north-runtime)
- Bucket 5: 6 crates, 41,158 lines; 5 mothball, 1 simplify
- Total 342,059 lines under `crates/`. Buckets 1 to 3 hold 179,414 lines (52%); buckets 4 and 5 hold 162,645 (48%). The root binary adds 21,851 lines in `src/` and 6,913 in `tests/it` (see the added rows).

Hooks (6 registered units, 45 decider rows): bucket 3 has 6 (3 keep, 3 simplify), bucket 4 has 18 (all mothball), bucket 5 has 27 (1 simplify, 26 mothball). No session hook stays on; the secret and private-text scans run at push and landing.

Skills (22): bucket 4 has 3, bucket 5 has 19; all mothball.

Systemd units and timers (59, defined in nixos-config modules): bucket 1 has 11 (8 keep, 3 simplify), bucket 2 has 11 (3 keep, 2 simplify, 6 mothball), bucket 3 has 7 (2 keep, 4 simplify, 1 mothball), bucket 4 has 26 (all mothball), bucket 5 has 4 (all mothball).

Top-level verbs (108, plus bare `north`): bucket 1 has 13 (8 keep, 3 simplify, 2 mothball), bucket 2 has 24 (14 keep, 3 simplify, 7 mothball), bucket 3 has 7 (4 keep, 3 simplify), bucket 4 has 56 (55 mothball, 1 simplify), bucket 5 has 9 (1 keep, 8 mothball).

Rows the review added (12): bucket 1 has 2 (1 keep, 1 simplify), bucket 2 has 2 (both simplify), bucket 3 has 2 (both keep), bucket 4 has 5 (4 mothball, 1 simplify), bucket 5 has 1 (keep).

The first draft's ten least certain calls went to an outside review on 2026-11-16; the decisions below replace them.

## Inventory review and decisions (2026-11-16)

The owner had an outside model review this inventory and the coordinator's ten proposed calls; the coordinator adopted the review with one change, to decision 2 (north#1650). Each decision is applied to the rows below, and each line count or dependency it cites was checked against the code at e87d0a1601; where the code differed, the row gives what the code says.

1. Capacity: the kept slice (the `seat-probe` unit, bounded offload runs in `north-capacity-host/src/run_bounded/`, local compile admission in `north-capacity/src/capacity/admission/`) moves to bucket 1 as simplify. Leases, modes, `nexus_ram` and the `capacity-watchdog` unit are mothballed: the watchdog paged "host overload" 78 times and no record shows a stall it prevented.
2. The public-repo scans (`north denylist`, `north private-check`, the `denylist-attest` unit, `north-shared-config`, `north-ci`'s `private_check`) move to bucket 2 and stay on always. The coordinator's change: `north policy off` keeps the deterministic secret and private-text scan on the owner's own push, since it takes about a second and refuses only a real hit; everything else in policy goes off. The `disclosure-judge` and `privacy-judge` session hooks are mothballed (disclosure-judge 6 of 30 sampled refusals false, 20%; 226 pending waits between them, 203 and 23).
3. `north-lander`, `north-lanes` and `north-session-host` change from keep to simplify. Their manifests confirm it: the lander names `north-runtime-follow`, `north-concerns` and `north-graph-ground`; `north-lanes` names `north-github-mirror`; `north-session-host` names `north-runtime-follow`.
4. `north-diff-gates` shrinks to the secrets and language gates; the dep, build-cache and stub-exec gates are mothballed, having refused in shadow only (479, 1 and 348 would-be refusals in 14 days) with no enforced catch.
5. The `north-tempo` unit, `north-machine-model` and the `verify-north` daily timer are mothballed.
6. `north-threads` and `north-providers` are mothballed; Phase 3 reads timings and cost from the land tickets and the session transcripts.
7. `launch-critical` stays as an opt-in policy hook; the worker sandbox enforces the same boundary for confined workers without a hook (north#1650).
8. `north-github-mirror` is mothballed once its core callers are cut. The review counts about 10; 10 manifests name it, and the core ones are the root `Cargo.toml`, `north-lanes`, `north-landing`, `north-land-scans` and `north-ci` (the other five, `north-guard`, `north-graph-ingest`, `north-dispatch`, `north-unblocks` and `north-issue-boxes`, leave with buckets 4 and 5).
9. Automatic runtime promotion is off. `bin/head-north` gets a local build fallback so the laptop checks with every server down; `north runtime` covers, tree-build and deploy stay.
10. The lander doctor and the `north-land-shadow` and `north-land-drill` units are mothballed. `north-strategy-types` shrinks to its envelope and land types in bucket 1. `north-buck` and `north-test-cases` stay while the after-only Buck2 workflow stays, and laptop safe-push never needs a remote Buck2 build. The language gate stays at safe-push (bucket 2); the `language-guard` hook is mothballed.

The review also fixes the Phase 2 cut order and the Phase 3 not-ready thresholds; north#1650 holds them, and the Phase 2 cut plan (docs/rationale/core-cut-plan.md, its own box) verifies each edge against the code.

## Evidence sources

Each figure was read on 2026-11-16 from one of these sources; the 14-day windows end then.

- Hooks: `north hook stats --since 14d` (fires, refusals, rewrites, sessions that repeated a refusal, sampled false share, and the prune list of rules with no fire in 30 days).
- Landing gates: `north land gate-stats --since 14d`, read from whiterabbit's records mirror (refusals, worker-hours from a refusal to the lane's next ticket, lanes that later landed unchanged, after-landing failures, enqueue refusals, shadow would-be refusals).
- Fleet: `north work --since 14d` (sessions, open gates, 511 messages held from waking their recipient, watchdog and disk alarms).
- Churn: `git log --since='14 days ago' e87d0a1601` (5,073 commits); a crate's count is the commits that touched it, and reverts are those whose subject starts with "Revert".
- Dependencies: `[dependencies]` and `[dev-dependencies]` lines in each `crates/*/Cargo.toml` and the root `Cargo.toml`; "manifests name it" counts those files, the root one included.
- Experiments: `docs/rationale/experiments.toml`. The ones that touch these calls (context-pack, grounding-g2, fresh-diff-reviewer, enqueue-gates, lander-default, flake-gate, failing-first) all have review dates from 2026-11-18 on, so none has a verdict yet.
- Disk: `graph.db` is 2.9 GB on gex45 (`ls -la ~/.local/state/north`); north#1650 gives 18 GB on the laptop.
- North#1650 itself, for the incidents of 2026-11-16 and the review decisions: dispatch rounds started again after their timer was stopped, and an accept pinned to a patch was voided twice when a gate refused the lean lander afterwards.
- Not read: the incidents ledger, which this session could not reach, so no row below cites an incident class count.

## Rows the review added

Parts that are not a crate, hook, skill, unit or verb, and the governance that still lives inside the core crates.

| Item | Bucket | Call | Evidence |
|---|---|---|---|
| the other 28 entries of `bin/` (shims, their `.test.sh` files and `fixtures/`; 259 lines of top-level files) | 1 Execution primitives | keep | toolchain and client shims (cargo, rustc, nix, gh, ssh, bun and others) the hooks and sandbox route through; not sorted one by one here |
| root binary: `src/` (21,851 lines), `Cargo.toml` (270 lines), `tests/it` (6,913 lines) | 1 Execution primitives | simplify | every verb's entry point; Phase 2 trims `src/cli` to the core verbs and drops the integration tests of mothballed verbs |
| `bin/head-north` (114 lines) | 2 Check and land | simplify | the offline blocker: with no build-ahead artifact and no remote Buck2 build it exits 75 (bin/head-north:19, bin/head-north:22), so the laptop cannot check with every server down; gets a local build fallback (decision 9) |
| `.safe-push` (119 lines) | 2 Check and land | simplify | the landing's check list; lines that call mothballed verbs are dropped first in Phase 2 (remote hosts gex45 and cpx62b today) |
| one-shot security read | 3 Sandbox and broker | keep | partly built (crates/north-lander/src/land/review_request.rs); the standing reviewer session retires once the one-shot read covers every protected lane (north#1650) |
| ring-0 write denial | 3 Sandbox and broker | keep | a bucket 3 decision of north#1650 |
| `.github/workflows/sheriff.yml` (38 lines) | 4 Fleet governance | mothball | "Main is red" workflow that hands a red main run to the culprit revert; retired in Phase 2 step 1 |
| governance inside the core: issue-driven lane GC in `north-lanes` | 4 Fleet governance | mothball | gc reads the GitHub mirror to judge a lane's issue (src/lanes/gc/audit.rs, remote.rs, evidence.rs) |
| governance inside the core: patch-pinned review-accept and red-issue matching in `north-lander` | 4 Fleet governance | mothball | review_request.rs holds a ticket under the patch id an accept is keyed by (an accept voided twice on 2026-11-16 when a later gate changed the patch, north#1650); enqueue.rs:138 matches the issues a lane names against the open "main is red" issues through `gh`, and serve/alone.rs reorders the queue while main is red |
| governance inside the core: self-policing scans in `north-land-scans` and `north-landing-gates` | 4 Fleet governance | mothball | gate-shadow, holdlist, docs-owners, alarm definitions, ci-budget, issue-box lint, follows-release |
| governance inside the core: fleet request kinds in `worker_sandbox` | 4 Fleet governance | simplify | 12 request kinds in crates/north-runtime/src/worker_sandbox/*_request.rs; finish, check and offload serve the core, the planner, judge, review, hold and eval-unit kinds serve the fleet |
| `bin/north` (22 lines) | 5 Policy | keep | the policy-off hook bypass: with `policy-off/record` present, `north hook` returns before any decider runs (bin/north:5), and `fleet` or `policy` `off`/`on` go to `north-switch` (bin/north:8) |

## Crates

| Crate | Bucket | Call | Rust lines | Commits/reverts, 14 d | Evidence |
|---|---|---|---:|---:|---|
| `north-claude-stream` | 1 Execution primitives | keep | 258 | 4/0 | parses the Claude client stream the warm clients use |
| `north-foundation` | 1 Execution primitives | keep | 1729 | 64/5 | error and test-input base; 40 crates depend on it |
| `north-git-locks` | 1 Execution primitives | keep | 252 | 10/0 | the one stale-lock removal every lane path uses |
| `north-publish` | 1 Execution primitives | keep | 28 | 3/0 | whole-file create; threads and fleet-store depend on it |
| `north-resume` | 1 Execution primitives | keep | 116 | 6/0 | session resume record |
| `north-runtime-follow` | 1 Execution primitives | keep | 518 | 16/1 | the runtime layout library: it depends only on north-base, and its tree layout and collection carry the remote checks (crates/north-offload/src/land_offload/ship/pinned.rs:3), while its runtime link is what the broker, GitHub broker, session host and worker sandbox follow (crates/north-runtime/src/worker_sandbox/broker.rs); only `leaving` and `LANDER_ROLLBACK` serve promotion, which stays off (decision 9; docs/rationale/core-cut-plan.md) |
| `north-ssh-spare` | 1 Execution primitives | keep | 327 | 10/1 | spare ssh channels opened ahead: the single-call ssh of north#1650 |
| `north-switch` | 1 Execution primitives | keep | 1160 | 2/0 | off switches that answer with the runtime broken (Phase 0, north#1648) |
| `north-base` | 1 Execution primitives | simplify | 12883 | 470/23 | shared base holding dispatch_pause, autoland_status, agents_sync beside platform code; 470 commits and 23 reverts in 14 d, the most of any base crate |
| `north-capacity` | 1 Execution primitives | simplify | 6623 | 144/10 | kept slice is local compile admission (src/capacity/admission/, 670 lines); leases, modes and the watchdog are mothballed: the watchdog paged "host overload" 78 times with no recorded prevented stall (decision 1) |
| `north-capacity-host` | 1 Execution primitives | simplify | 11814 | 199/11 | kept slice is the bounded offload runs (src/run_bounded/, 157 lines); nexus_ram (580 lines) and leases are mothballed (decision 1); 199 commits, 11 reverts in 14 d |
| `north-fleet` | 1 Execution primitives | simplify | 9157 | 383/22 | holds the warm spares of 6a8b10f237 (src/fleet/spares.rs); the rest is fleet session governance; 383 commits, 22 reverts in 14 d |
| `north-lanes` | 1 Execution primitives | simplify | 9277 | 104/4 | lane worktrees, gc, rescue, squash; its gc reads the GitHub mirror (src/lanes/gc/audit.rs, remote.rs, evidence.rs), the issue-driven lane GC (decision 3) |
| `north-reclaim` | 1 Execution primitives | simplify | 2061 | 11/1 | disk reclaim and retention; keep reclaim, drop retention tied to fleet state |
| `north-session-host` | 1 Execution primitives | simplify | 1353 | 36/3 | holds a session's process and delivers its rows; names north-runtime-follow in its manifest (decision 3) |
| `north-strategy-types` | 1 Execution primitives | simplify | 850 | 53/4 | keeps its envelope, land and admission types and `github::Quota`: the kept local compile admission records `admission` cases (crates/north-capacity/src/capacity/admission_record.rs) and crates/north-base/src/platform/github_quota.rs reads the quota; placement, credit, tokens and brief go once bucket 4 leaves; 11 manifests name it, the lander and landing among them (decision 10) |
| `north-buck` | 2 Check and land | keep | 1743 | 17/0 | Buck2 target writer; stays while the after-only Buck2 workflow stays, and laptop safe-push never needs a remote Buck2 build (decision 10) |
| `north-build-system` | 2 Check and land | keep | 2125 | 60/1 | the one build/test/clippy interface; 13 crates depend on it |
| `north-check-environment` | 2 Check and land | keep | 133 | 8/0 | where a land check runs: runner, build host or here |
| `north-cli-reference` | 2 Check and land | keep | 59 | 4/0 | verb list the reference and the scans share |
| `north-coverage` | 2 Check and land | keep | 1725 | 39/1 | test-to-function map behind pre-push test selection; coverage test refused 8 lanes in 14 d, all later landed |
| `north-docs` | 2 Check and land | keep | 1474 | 9/0 | derived docs indexes; docs features-index refused 22 lanes before 62f0f64a3f regenerated them |
| `north-graph-change` | 2 Check and land | keep | 2718 | 23/0 | affected, overlap and blame queries the lander asks: the code knowledge to salvage |
| `north-land-checks` | 2 Check and land | keep | 10121 | 222/10 | land check slots and offload placement; nextest refused 27 lanes in 14 d, 24 later landed |
| `north-land-sandbox` | 2 Check and land | keep | 1398 | 16/0 | confines what a land check may read and write |
| `north-registries` | 2 Check and land | keep | 88 | 4/0 | derived registries regenerated at landing (62f0f64a3f, north#1537) |
| `north-safe-push` | 2 Check and land | keep | 4795 | 69/5 | the landing entry; 69 commits, 5 reverts in 14 d |
| `north-test-cases` | 2 Check and land | keep | 248 | 6/0 | runs libtest cases one per process for the Buck2 target; stays with the Buck2 workflow (decision 10) |
| `north-lander-doctor` | 2 Check and land | mothball | 2754 | 70/1 | auto-remedy loop for stuck lander states; a lean lander has few states to heal; no recorded catch in the reports read |
| `north-machine-model` | 2 Check and land | mothball | 874 | 9/1 | lifecycle machine checker; no recorded catch (decision 5) |
| `north-ci` | 2 Check and land | simplify | 1689 | 5/0 | private_check is the private-text scan, on always (decision 2); ci_watch is mothballed |
| `north-diff-gates` | 2 Check and land | simplify | 3662 | 29/1 | keeps the secrets and language gates; dep-gate (479), stub-exec (348) and build-cache (1) would-be refusals were shadow only, so those three are mothballed (decision 4) |
| `north-graph` | 2 Check and land | simplify | 4279 | 31/0 | triple kernel; graph.db is 2.9 GB on gex45 (ls, 2026-11-16) and 18 GB on the laptop (north#1650); rebuild as a small library |
| `north-graph-ingest` | 2 Check and land | simplify | 2049 | 41/1 | loads the GitHub mirror, ledger and push log; keep the code half, drop the issue half |
| `north-lander` | 2 Check and land | simplify | 12563 | 371/23 | the lean lander (10d6a73af8, north#1553), but it still names north-runtime-follow, north-concerns and north-graph-ground in its manifest and holds patch-pinned review-accept and red-issue matching (decision 3) |
| `north-landing` | 2 Check and land | simplify | 12158 | 671/28 | landing core, but box_tick and issue_body make landing know issues, which Phase 2 forbids; 671 commits, 28 reverts in 14 d, the most churned landing crate |
| `north-landing-gates` | 2 Check and land | simplify | 8445 | 94/7 | gates plus follows_release, issue_box_lint and land_abandon, the self-policing governance inside the core |
| `north-land-scans` | 2 Check and land | simplify | 8577 | 131/6 | 30-odd scans; "lander scan" refused 76 lanes (37.6 worker-h, 13 landed unchanged); its self-policing scans (gate-shadow, holdlist, docs-owners, alarm definitions, ci-budget) are governance inside the core |
| `north-offload` | 2 Check and land | simplify | 6365 | 49/4 | remote checks with the laptop fallback; depends on north-dispatch and north-pacing, which Phase 2 must cut |
| `north-shared-config` | 2 Check and land | simplify | 498 | 18/1 | reads south agents/ config; keeps only what the always-on public-repo scans and the landing read (decision 2) |
| `north-agent-starts` | 3 Sandbox and broker | keep | 304 | 5/0 | the one door from fleet to an agent-user session; ring-0 |
| `north-credentials` | 3 Sandbox and broker | keep | 1083 | 5/1 | credential store behind the broker; ring-0 |
| `north-trusted-mirror` | 3 Sandbox and broker | keep | 222 | 7/0 | trusted bare mirror outside every worktree (south#128); ring-0 path |
| `north-broker` | 3 Sandbox and broker | simplify | 2126 | 45/3 | agent broker; move to one permission manifest read at start (north#1650) |
| `north-github-broker` | 3 Sandbox and broker | simplify | 2224 | 10/0 | every gh call through one broker; keep the token gate, drop read pacing and budgets |
| `north-remote-serve` | 3 Sandbox and broker | simplify | 1088 | 6/1 | forced-command peer server; ring-0 |
| `north-runtime` | 3 Sandbox and broker | simplify | 23421 | 853/33 | worker_sandbox and agent_contain (9,571 lines) become a fresh bucket 3 crate in Phase 2; the sandbox carries 12 fleet request kinds (finish, check, hold, host-op, offload, planner, review-call, review-fetch, judge, withdraw, eval-unit, child); 853 commits, 33 reverts in 14 d, the most of any crate |
| `north-calibration` | 4 Fleet governance | mothball | 3489 | 21/0 | ETA and budget calibration |
| `north-claims` | 4 Fleet governance | mothball | 1104 | 2/0 | citation check and review items |
| `north-context-pack` | 4 Fleet governance | mothball | 327 | 7/0 | brief context pack; experiment context-pack has no verdict before 2026-12-11 |
| `north-control-plane` | 4 Fleet governance | mothball | 92 | 1/0 | lease naming the control-plane host |
| `north-convo` | 4 Fleet governance | mothball | 7898 | 31/2 | transcript search, incident mining, policy replay |
| `north-core` | 4 Fleet governance | mothball | 6543 | 485/6 | grounding, coordinator, contract probe, nix doctor; 485 commits, 6 reverts in 14 d |
| `north-desktop` | 4 Fleet governance | mothball | 1996 | 6/0 | private desktop, Warcraft capture; product tooling, not core |
| `north-dispatch` | 4 Fleet governance | mothball | 11230 | 391/11 | dispatch rounds restarted on nexus twice after its timer was masked (north#1650, 2026-11-16); 391 commits, 11 reverts in 14 d |
| `north-expectations` | 4 Fleet governance | mothball | 1892 | 5/0 | expectation watch and dividend |
| `north-fleet-store` | 4 Fleet governance | mothball | 5023 | 76/5 | session store, alarms, coordinator records; 511 messages held from waking their recipient (north work --since 14d) |
| `north-fleet-supervisor` | 4 Fleet governance | mothball | 11913 | 300/18 | fleet tick, registry, leads; 300 commits, 18 reverts in 14 d |
| `north-flow` | 4 Fleet governance | mothball | 11231 | 95/1 | flow detectors, tempo, experiments judge, tool mining |
| `north-github-mirror` | 4 Fleet governance | mothball | 844 | 22/1 | the graph's issue mirror, mothballed once its core callers are cut: 10 manifests name it, the core ones being the root Cargo.toml, north-lanes, north-landing, north-land-scans and north-ci (decision 8) |
| `north-graph-ground` | 4 Fleet governance | mothball | 965 | 19/0 | prompt grounding from the graph; experiment grounding-g2 has no verdict before 2026-11-28 |
| `north-graph-loops` | 4 Fleet governance | mothball | 1026 | 16/0 | learning-loop facts in the graph |
| `north-health` | 4 Fleet governance | mothball | 1769 | 46/0 | readiness bars and rolling unit-failure issues |
| `north-issue-boxes` | 4 Fleet governance | mothball | 2721 | 15/1 | issue box conversion and Done-when boxes |
| `north-land-watch` | 4 Fleet governance | mothball | 1194 | 23/1 | raises landing hiccups to the proxy and the lane's lead |
| `north-pace` | 4 Fleet governance | mothball | 7667 | 146/2 | spend, credit and the model router |
| `north-pacing` | 4 Fleet governance | mothball | 5882 | 102/4 | placement and laptop pacing |
| `north-providers` | 4 Fleet governance | mothball | 13199 | 293/9 | provider launchers and the run ledger; Phase 3 uses plain Claude Code or Pi sessions and reads cost from transcripts (decision 6) |
| `north-runtime-records` | 4 Fleet governance | mothball, split | 451 | 3/0 | crates/north-runtime-records/src/failure.rs (24 lines) is north-runtime's error type, used in 44 of its files, so it moves into north-runtime as core; crates/north-runtime-records/src/hold_ledger.rs (306 lines) carries review accepts through the sandbox and moves with it; crates/north-runtime-records/src/after_landing.rs (118 lines, culprit-search records) is mothballed (docs/rationale/core-cut-plan.md) |
| `north-threads` | 4 Fleet governance | mothball | 9136 | 220/6 | ledger of runs, claims and gates; Phase 3 reads timings and cost from land tickets and transcripts (decision 6) |
| `north-timers` | 4 Fleet governance | mothball | 4114 | 16/0 | job-run, job-watch, outside-watch, usage web reads |
| `north-tui` | 4 Fleet governance | mothball | 6143 | 82/3 | the fleet screen |
| `north-tune` | 4 Fleet governance | mothball | 3277 | 36/0 | learning-loop tuner (harness/loops.toml) |
| `north-unblocks` | 4 Fleet governance | mothball | 361 | 3/1 | coordinator's manual unblocks (north#1577) |
| `north-agent-policy` | 5 Policy | mothball | 6886 | 2/0 | agent config check and policy contract; behind north policy |
| `north-code-health` | 5 Policy | mothball | 7648 | 56/1 | debt ceiling refused 46 lanes (31.8 worker-h, 21 landed unchanged); after-landing code-health 733 failures in 14 d |
| `north-concerns` | 5 Policy | mothball | 1017 | 8/0 | concerns.toml checks; opt-in |
| `north-implicit-state` | 5 Policy | mothball | 822 | 5/0 | code-health rule |
| `north-mutants` | 5 Policy | mothball | 1120 | 9/1 | mutation sweeps; test admission kills scan |
| `north-guard` | 5 Policy | simplify | 23665 | 277/5 | the hook deciders (one row each below); keep public-text and the secret paths on, the rest opt-in; 23665 lines, the largest crate |

The workspace also lists `crates/north-tui/monitor`, the fleet monitor inside `north-tui`: bucket 4, mothball with it.

## Hooks

North registers six hook units (`harness/policy-owners.toml`); the `north-hook` dispatcher runs the deciders of `crates/north-guard/src/guard/`. The deciders are listed by the rule names `north hook stats` reports, so the counts below are theirs. "False" is the sampled false share the hook statistics record.

| Item | Bucket | Call | Evidence |
|---|---|---|---|
| `sops-decrypt` | 3 Sandbox and broker | keep | 29 refusals, 0/8 false |
| `tripwire` | 3 Sandbox and broker | keep | 186 refusals, 1/17 false; .push 132 (0/19 false), .delete 118 (0/9 false) |
| unit: `read-reach (pre-read)` | 3 Sandbox and broker | keep | PreToolUse Read/Grep/Glob reach check (file_read_reach) |
| `clipboard` | 3 Sandbox and broker | simplify | 6 refusals, 0/1 false |
| `corpus-scan` | 3 Sandbox and broker | simplify | 716 refusals, 8/30 false (27%); fold into read reach |
| `worktree-remove` | 3 Sandbox and broker | simplify | 631 refusals, 0/30 false; becomes a sandbox rule |
| `agent-cwd`, `coordinator-role`, `frozen-topics`, `native-launch`, `generated-policy`, `model-identity` | 4 Fleet governance | mothball | no fire in 30 days (north hook stats prune list) |
| `handoff-record` | 4 Fleet governance | mothball | 23 advisories |
| `issue-cleanup` | 4 Fleet governance | mothball | 118 refusals, 1/20 false |
| `issue-filing` | 4 Fleet governance | mothball | 590 refusals, 0/7 false |
| `issue-labels` | 4 Fleet governance | mothball | 395 refusals, 0/30 false |
| `issue-shrink` | 4 Fleet governance | mothball | 3 refusals, 1/2 false |
| `lead-timer` | 4 Fleet governance | mothball | 10 refusals, 0/2 false |
| `native-evidence` | 4 Fleet governance | mothball | 2 refusals, 1/3 false |
| `proxy-role` | 4 Fleet governance | mothball | 48 refusals, 6/10 false |
| `question-box` | 4 Fleet governance | mothball | 4 refusals |
| `reviewer-cli` | 4 Fleet governance | mothball | 4 refusals |
| `spawn-capacity` | 4 Fleet governance | mothball | 20 advisories |
| unit: `codex-conduct` | 4 Fleet governance | mothball | Codex-only deciders; no Codex subscription |
| `unrunnable-box` | 4 Fleet governance | mothball | 4 refusals, 12/16 false (75%) |
| unit: `prompt-grounding` | 4 Fleet governance | mothball | grounds Tom's interactive prompt from the graph |
| `worker-handoff` | 4 Fleet governance | mothball | 58 advisories |
| `worker-tier` | 4 Fleet governance | mothball | missing-facet 168, facet-value 36, routing 30, payback 23, item 14 refusals |
| `worker-wait` | 4 Fleet governance | mothball | 662 refusals, 24/30 false (80%) |
| `ambient-test-input` | 5 Policy | mothball | 62 advisories |
| `blind-stage` | 5 Policy | mothball | 2376 refusals, 0/30 false, 223 sessions repeated it |
| `claude-sleep` | 5 Policy | mothball | 3911 refusals, 0/30 false, 475 sessions repeated it |
| `code-health` | 5 Policy | mothball | 607 advisories |
| `desktop-action` | 5 Policy | mothball | 15 refusals, 0/5 false |
| `disclosure-judge` | 5 Policy | mothball | 6 refusals, 6/30 false (20%); 23 pending waits; session hook mothballed (decision 2) |
| `file-skill` | 5 Policy | mothball | 107 advisories |
| `git-stash` | 5 Policy | mothball | 320 refusals, 0/30 false |
| `heavy-command` | 5 Policy | mothball | 8207 fires, 8159 rewritten into capacity runs, 5/19 false; .local-write 2209 refusals; .farm 35 |
| `language` | 5 Policy | mothball | 13 refusals, 1/1 false |
| `launch-critical` | 5 Policy | mothball | 1994 refusals, 0/18 false, 249 sessions repeated it; stays as an opt-in policy hook (decision 7) |
| `masked-pipeline` | 5 Policy | mothball | 27 refusals, 0/14 false |
| `modern-search` | 5 Policy | mothball | 5660 fires, 5330 rewritten, 8/30 false |
| `nix-activity` | 5 Policy | mothball | 3709 advisories; .deny 13 refusals, 4/7 false |
| `node-modules-write` | 5 Policy | mothball | 1 refusal |
| `opencode-cli` | 5 Policy | mothball | 7 refusals, unmeasured |
| `other` | 5 Policy | mothball | 4584 fires, 1937 refusals under no named rule; unclassified |
| `poll-loop` | 5 Policy | mothball | 1401 refusals, 0/2 false |
| `privacy-judge` | 5 Policy | mothball | 53 refusals, false share unmeasured; 203 pending waits; session hook mothballed (decision 2) |
| `public-text` | 5 Policy | mothball | 53 refusals in 14 d, 0/1 false; a session hook, so it goes off with the rest of policy while the push-time private-text scan stays on (decision 2) |
| `resource-safe-search` | 5 Policy | mothball | 463 refusals, 20/30 false (67%) |
| `session-kill` | 5 Policy | mothball | 935 refusals, 13/30 false (43%); .self-match 905, 19/30 false (63%) |
| `shared-mutability` | 5 Policy | mothball | 3 advisories |
| `single-impl-trait` | 5 Policy | mothball | 17 advisories |
| unit: `firn-system-policy` | 5 Policy | mothball | refuses edits to firn system policy paths |
| unit: `language-guard` | 5 Policy | mothball | same rule the landing's language-gate enforces (45 lane refusals) |
| unit: `north-hook (dispatch)` | 5 Policy | simplify | the dispatcher of every decider below; stays only to run the keep-on rules |

## Skills

Every skill is off by default in the core. Several back a hook whose refusals sessions repeated anyway; `north hook stats` names the skill line that should have prevented each one.

| Skill | Bucket | Call | Lines | Evidence |
|---|---|---|---:|---|
| `machine-capacity` | 4 Fleet governance | mothball | 336 | capacity runs; backs heavy-command.local-write, which 597 sessions repeated |
| `planning` | 4 Fleet governance | mothball | 477 | issues, boxes and briefs for dispatch |
| `workers` | 4 Fleet governance | mothball | 311 | fleet worker conduct; backs claude-sleep (475 repeats) and worker-wait (197 repeats) |
| `agent-policy` | 5 Policy | mothball | 3074 | policy publishing and hooks |
| `astro` | 5 Policy | mothball | 106 | product skill |
| `clean-room` | 5 Policy | mothball | 90 | product skill for commercial-game work |
| `debugging` | 5 Policy | mothball | 141 | diagnosis procedure |
| `effect-development` | 5 Policy | mothball | 359 | product skill |
| `external-code` | 5 Policy | mothball | 59 | license check before outside code |
| `game-design-prototyping` | 5 Policy | mothball | 60 | product skill |
| `github` | 5 Policy | mothball | 166 | gh, issues, Actions |
| `makgora` | 5 Policy | mothball | 577 | product skill |
| `nix-development` | 5 Policy | mothball | 149 | Nix practice |
| `performance-design` | 5 Policy | mothball | 199 | hot-path practice |
| `private-desktop-development` | 5 Policy | mothball | 219 | product skill |
| `program-craftsmanship` | 5 Policy | mothball | 243 | refactor and structure practice |
| `repo-safety` | 5 Policy | mothball | 229 | backs launch-critical and blind-stage, whose refusals 249 and 223 sessions repeated anyway |
| `rust-development` | 5 Policy | mothball | 554 | compile-time and Rust practice |
| `threejs` | 5 Policy | mothball | 5278 | product skill |
| `todo` | 5 Policy | mothball | 319 | restart records and convo recall |
| `verification` | 5 Policy | mothball | 786 | test retention and landing tiers; the landing gates enforce the same rules |
| `warcraft-modding` | 5 Policy | mothball | 1507 | product skill |

## Systemd units and timers

North's units are defined in nixos-config (`/home/tom/code/nixos-config/main/modules/`, one module per unit or timer family), not in this repository; each row is one module. The modules that only mention north in shell or client configuration (bash, claude, codex, opencode, pi, niri) define no north unit and are left out.

| Item | Bucket | Call | Evidence |
|---|---|---|---|
| `agent-slice` | 1 Execution primitives | keep | agent lanes in a yielding compute slice |
| `agent-user` | 1 Execution primitives | keep | unix user the agent sessions run as |
| `disk-reclaim` | 1 Execution primitives | keep | hourly cache and scratch trim; gex45 root disk crossed its 87% watermark 19+28 times (north work --since 14d) |
| `nexus-backup` | 1 Execution primitives | keep | daily encrypted backup of nexus |
| `north-profile` | 1 Execution primitives | keep | puts north on the host's path |
| `north-switch` | 1 Execution primitives | keep | hosts `north fleet off` reaches (Phase 0) |
| `ssh-masters` | 1 Execution primitives | keep | ssh-master probe recycles a master slower than 3 s |
| `state-backup-push` | 1 Execution primitives | keep | daily encrypted agent-state backup to the vault host |
| `job-run` | 1 Execution primitives | simplify | scratch tmpfs handover plus the scheduled-job registry; keep the tmpfs part |
| `lane-gc` | 1 Execution primitives | simplify | hourly scratch and landed-worktree reclaim |
| `seat-probe` | 1 Execution primitives | simplify | stall cutoff: stops the fleet's laptop units when a synced write stalls; part of the kept capacity slice (decision 1) |
| `coverage-map-refresh` | 2 Check and land | keep | nightly coverage map for pre-push test selection |
| `denylist-attest` | 2 Check and land | keep | GitHub App attestation for the deny-list scan; public-repo scans stay on always (decision 2) |
| `north-land` | 2 Check and land | keep | the lander |
| `build-time` | 2 Check and land | mothball | daily build-time measurement |
| `delivery-liveness` | 2 Check and land | mothball | Firn delivery liveness on main advance |
| `nexus-checks` | 2 Check and land | mothball | nexus standing checks and alerts |
| `north-land-drill` | 2 Check and land | mothball | nightly stuck-state drill against a scratch lander |
| `north-land-shadow` | 2 Check and land | mothball | every 2 min runs the lander doctor and pre-checks the next candidate |
| `verify-north` | 2 Check and land | mothball | daily end-to-end spine run; mothballed (decision 5) |
| `capacity-taker` | 2 Check and land | simplify | polls reachable hosts for queued heavy runs; keep as the remote-check runner |
| `north-graph-retain` | 2 Check and land | simplify | daily graph history delete; ends with the small library |
| `bubblewrap` | 3 Sandbox and broker | keep | sandbox runtime |
| `github-pat` | 3 Sandbox and broker | keep | GitHub token configuration |
| `codex-runtime` | 3 Sandbox and broker | mothball | roots Codex runtimes; no Codex subscription |
| `agent-brokers` | 3 Sandbox and broker | simplify | the broker serving both sockets |
| `agent-claude-managed` | 3 Sandbox and broker | simplify | managed-settings.json with permissions and the north guard: becomes the one permission manifest read at start |
| `gh-broker` | 3 Sandbox and broker | simplify | GitHub call broker |
| `north-remote-serve` | 3 Sandbox and broker | simplify | peers reaching this host only through remote-serve |
| `agent-runtime-update` | 4 Fleet governance | mothball | twice-daily agent runtime updates |
| `agents-flow-stall` | 4 Fleet governance | mothball | 5-min landing stall report and experiments judge |
| `capacity-watchdog` | 4 Fleet governance | mothball | load and PSI sampling; paged "host overload" 78 times (north work --since 14d) with no recorded prevented stall (decision 1) |
| `convo-retain` | 4 Fleet governance | mothball | transcript compression and corpus append |
| `cross-project-scan` | 4 Fleet governance | mothball | daily overlap scan for Plato |
| `eta-calibration` | 4 Fleet governance | mothball | weekly ETA post to north#255 |
| `fleet-tick` | 4 Fleet governance | mothball | 30 s fleet spool flush and row delivery |
| `harness-health` | 4 Fleet governance | mothball | 2-min harness invariant checks |
| `incident-retro` | 4 Fleet governance | mothball | weekly Haiku retro |
| `job-watch` | 4 Fleet governance | mothball | overdue-job alarm |
| `laptop-runner` | 4 Fleet governance | mothball | ledger-queued laptop jobs every 30 s |
| `laptop-window` | 4 Fleet governance | mothball | minutely laptop window |
| `makgora-goals-perf` | 4 Fleet governance | mothball | Mak'gora frame run; product job, not north's |
| `makgora-retain` | 4 Fleet governance | mothball | Mak'gora evidence archive; product job, not north's |
| `nexus-ram` | 4 Fleet governance | mothball | minutely nexus memory row |
| `north-runtime-follow` | 4 Fleet governance | mothball | promotes origin/main to the live runtime every 20 s |
| `north-stable-hold` | 4 Fleet governance | mothball | holds promotion at a pin |
| `north-tempo` | 4 Fleet governance | mothball | hourly landing p50/p90; mothballed (decision 5), Phase 3 reads timings from land tickets |
| `north-watch` | 4 Fleet governance | mothball | 30 s landing hiccups to the proxy and lead |
| `ops-check` | 4 Fleet governance | mothball | 5-min ops check |
| `outside-watch` | 4 Fleet governance | mothball | 10-min GitHub spam minimizing |
| `pace-enforcement` | 4 Fleet governance | mothball | minutely provider usage sample for the spend rule |
| `tool-mine` | 4 Fleet governance | mothball | weekly tool mining |
| `usage-sample` | 4 Fleet governance | mothball | minutely Claude usage sample |
| `watch-review` | 4 Fleet governance | mothball | daily followed-source review sessions |
| `worker-ledger` | 4 Fleet governance | mothball | 15-min worker run recording |
| `cargo-deny` | 5 Policy | mothball | cargo-deny check |
| `mutant-sweep` | 5 Policy | mothball | monthly mutation sweep on a build host |
| `skill-prune` | 5 Policy | mothball | weekly skill prune |
| `supply-chain-scan` | 5 Policy | mothball | daily osv-scanner pass; no recorded catch in the reports read |

## Top-level verbs

One row per first word of the verb registry (`src/cli/verbs/`), 108 in all, plus bare `north`. The subverbs go with their verb, except where a row names the ones it keeps.

| Item | Bucket | Call | Evidence |
|---|---|---|---|
| `help` | 1 Execution primitives | keep | help |
| `lane` | 1 Execution primitives | keep | collect, reap, rescue, seed, squash, audit lanes |
| `session-attach` | 1 Execution primitives | keep | attach to a hosted session |
| `session-host` | 1 Execution primitives | keep | hosts a session process |
| `session-link` | 1 Execution primitives | keep | link a remote session unit |
| `session-unit` | 1 Execution primitives | keep | systemd-run in the session's scope |
| `ssh-argv` | 1 Execution primitives | keep | attributed ssh argv |
| `ssh-session` | 1 Execution primitives | keep | refuses ssh from confined workers, points at offload |
| `home` | 1 Execution primitives | mothball | one-off home move plan |
| `retain` | 1 Execution primitives | mothball | Mak'gora evidence archive; product tooling |
| `capacity` | 1 Execution primitives | simplify | keeps seat-probe, bounded offload runs and local compile admission; leases, modes and the watchdog go (decision 1) |
| `holds` | 1 Execution primitives | simplify | lists hand-set stops; keep the stable-pin and lock part |
| `runtime` | 1 Execution primitives | simplify | keeps covers, tree-build and deploy; automatic promotion is off (decision 9) |
| `bin-references` | 2 Check and land | keep | refuses deleting a bin entry callers name |
| `buck` | 2 Check and land | keep | Buck2 targets for the after-only CI workflow, which stays (decision 10) |
| `denylist` | 2 Check and land | keep | secret and deny-listed term scan, on always: 215 enqueue refusals in 14 d (decision 2) |
| `docs` | 2 Check and land | keep | regenerate derived docs (62f0f64a3f) |
| `finish` | 2 Check and land | keep | worker's one-call finish |
| `hand-in` | 2 Check and land | keep | queue a passing step |
| `landed-tree` | 2 Check and land | keep | already-landed test the lander uses |
| `language-gate` | 2 Check and land | keep | stays at safe-push: refused 45 lanes (5.4 worker-h, 6 landed unchanged) (decision 10) |
| `merge-driver` | 2 Check and land | keep | order-insensitive list merges for the derived lists |
| `nix` | 2 Check and land | keep | explain a Nix footgun, root and collect the store |
| `private-check` | 2 Check and land | keep | private-value checks, on always, including on the owner's own push with policy off (decision 2) |
| `safe-push` | 2 Check and land | keep | check and land a lane |
| `stale-revert` | 2 Check and land | keep | "this landing undoes recent commits": 34 refusals in 14 d, 33 landed later, 5 unchanged |
| `test` | 2 Check and land | keep | coverage map and mutation sweeps; keep coverage |
| `after-landing` | 2 Check and land | mothball | after-landing checks: code-health 733, crate-lines 270, ci-budget 39 failures in 14 d, none reverted |
| `autoland-status` | 2 Check and land | mothball | reads a lane's Autoland result |
| `build-cache-gate` | 2 Check and land | mothball | 1 shadow would-be refusal in 14 d (decision 4) |
| `dep-gate` | 2 Check and land | mothball | 479 would-be refusals in shadow, none enforced (decision 4) |
| `follows-release` | 2 Check and land | mothball | starts queued boxes after predecessors land |
| `hardcoded-repo-path-check` | 2 Check and land | mothball | path count check |
| `main-red` | 2 Check and land | mothball | after-landing culprit revert on red CI |
| `check` | 2 Check and land | simplify | citations, concerns, boxes, nix, provenance, privacy, harness; keep privacy and the landing checks |
| `graph` | 2 Check and land | simplify | keep ask affected/overlap; drop tell, retract, ground |
| `land` | 2 Check and land | simplify | 13 subverbs; keep serve, status, withdraw, abandon; promote, machinery-replay, gate-stats go with promotion and governance |
| `agent-contain` | 3 Sandbox and broker | keep | moves agent children out of Tom's slices |
| `depbox` | 3 Sandbox and broker | keep | dependency sandbox for builds |
| `gh-session` | 3 Sandbox and broker | keep | confined gh call through the broker |
| `git-credential` | 3 Sandbox and broker | keep | git credential helper through the broker |
| `broker` | 3 Sandbox and broker | simplify | agent credential broker |
| `gh-broker` | 3 Sandbox and broker | simplify | GitHub call broker |
| `remote-serve` | 3 Sandbox and broker | simplify | forced-command peer server |
| `accounts` | 4 Fleet governance | mothball | account windows |
| `alarm` | 4 Fleet governance | mothball | alarm stats |
| `box` | 4 Fleet governance | mothball | box state path |
| `cause` | 4 Fleet governance | mothball | no reference line; retired view |
| `ci-diagnose` | 4 Fleet governance | mothball | Haiku and Opus diagnosis on red-main issues |
| `cloud` | 4 Fleet governance | mothball | cloud sessions; no cloud session in the 14-day window read |
| `codex-runtime-gcroots` | 4 Fleet governance | mothball | Codex runtime roots; no Codex subscription (standing order 2026-11-14) |
| `contract-probe` | 4 Fleet governance | mothball | contract probe |
| `control-plane` | 4 Fleet governance | mothball | control-plane lease |
| `convo` | 4 Fleet governance | mothball | transcript search |
| `coordinator` | 4 Fleet governance | mothball | coordinator config and move |
| `cross-project-scan` | 4 Fleet governance | mothball | overlap scan for Plato |
| `dash` | 4 Fleet governance | mothball | board |
| `defaults-measure` | 4 Fleet governance | mothball | defaults measurement |
| `desktop` | 4 Fleet governance | mothball | private desktop, screenshots |
| `desktop-idle-state` | 4 Fleet governance | mothball | desktop idle state |
| `eta-calibration` | 4 Fleet governance | mothball | weekly ETA post |
| `evidence` | 4 Fleet governance | mothball | evidence folder per run |
| `fetch-fleet` | 4 Fleet governance | mothball | list wrapper |
| `fleet-for` | 4 Fleet governance | mothball | list wrapper |
| `fleet-issue` | 4 Fleet governance | mothball | list wrapper |
| `gate` | 4 Fleet governance | mothball | decision gates; 10 open (north work --since 14d) |
| `github-mirror` | 4 Fleet governance | mothball | issue mirror sync |
| `git-maintenance-nightly` | 4 Fleet governance | mothball | git maintenance under a capacity lease |
| `glance` | 4 Fleet governance | mothball | one-line status |
| `health` | 4 Fleet governance | mothball | readiness bars |
| `incident-mine` | 4 Fleet governance | mothball | transcript incident mining |
| `incident-retro` | 4 Fleet governance | mothball | weekly sampled retro |
| `infra` | 4 Fleet governance | mothball | paid resources |
| `issue` | 4 Fleet governance | mothball | issue convert |
| `job-run` | 4 Fleet governance | mothball | scheduled job runner |
| `job-watch` | 4 Fleet governance | mothball | overdue job alarm |
| `laptop-runner` | 4 Fleet governance | mothball | ledger-queued laptop jobs |
| `ledger` | 4 Fleet governance | mothball | runs, cost, incidents |
| `loops` | 4 Fleet governance | mothball | learning-loop check |
| `nexus-ram` | 4 Fleet governance | mothball | nexus memory rows |
| (none: `north` alone) | 4 Fleet governance | mothball | opens the fleet screen |
| `opencode-host` | 4 Fleet governance | mothball | OpenCode host |
| `ops` | 4 Fleet governance | mothball | watch, expectations, credentials, strategy |
| `outside-watch` | 4 Fleet governance | mothball | spam minimizing on GitHub |
| `pi-host` | 4 Fleet governance | mothball | Pi host |
| `pin-retire` | 4 Fleet governance | mothball | content-addressed pin retirement |
| `proof-run` | 4 Fleet governance | mothball | proof run printer |
| `provider-capacity` | 4 Fleet governance | mothball | provider capacity |
| `red-main-notify` | 4 Fleet governance | mothball | tells culprit sessions about red main |
| `resident-cap` | 4 Fleet governance | mothball | resident session cap |
| `spend` | 4 Fleet governance | mothball | API-equivalent spend |
| `standin-host` | 4 Fleet governance | mothball | no reference line |
| `task` | 4 Fleet governance | mothball | verdicts and box ticks |
| `threads` | 4 Fleet governance | mothball | run ledger and claims |
| `tool-mine` | 4 Fleet governance | mothball | weekly tool mining |
| `tune` | 4 Fleet governance | mothball | learning-loop tuner |
| `usage-web-read` | 4 Fleet governance | mothball | usage page reader |
| `watch-review` | 4 Fleet governance | mothball | daily followed-source review |
| `work` | 4 Fleet governance | mothball | fleet work view and work items |
| `fleet` | 4 Fleet governance | simplify | 21 subverbs; keep off and on (north#1648), mothball the rest |
| `policy` | 5 Policy | keep | the switch for bucket 5 |
| `agent-policy-contract` | 5 Policy | mothball | policy contract check |
| `code-health` | 5 Policy | mothball | debt ceiling 46 refusals, 21 landed unchanged |
| `config` | 5 Policy | mothball | skills, hooks, agent plan |
| `guard` | 5 Policy | mothball | guard decider entry |
| `hook` | 5 Policy | mothball | hook dispatcher entry and hook stats; stays only as the dispatcher of the keep-on rules |
| `policy-replay` | 5 Policy | mothball | replays policy over transcripts |
| `skill-prune` | 5 Policy | mothball | weekly skill prune |
| `supply-chain-scan` | 5 Policy | mothball | daily osv-scanner pass; no recorded catch in the reports read |
