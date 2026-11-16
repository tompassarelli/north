# North-ops split: every verb family moves to the crate that owns its domain

Owner: north#1435

An outside review of the workspace layout on 2026-11-14 named `north-ops` the one escape valve in an otherwise partitioned workspace: thirty modules under `crates/north-ops/src/ops/`, about 20,000 lines with tests, of which one names another. The owner asked for a proper fix before the public launch (2026-11-14). This note maps every family to its destination and states what the check that keeps a grab-bag from forming again must tell apart; the moves landed as the second box of north#1435 on 2026-11-14 and 2026-11-15.

Read at commit 15fc1ebdb0. Line counts include each family's tests.

## Rules the map follows

- A family goes to the existing crate whose one-line purpose already names its domain, else to a new crate named for that one domain. No destination is described as a list of unrelated things.
- Dependencies point down the layers of [architecture-layers.md](architecture-layers.md): a verb is a thin imperative shell in a crate that depends on the cores it drives, and no core gains a dependency on a shell. Where a lower crate needs a shell's behaviour today it already takes it as a function argument (the offload cache's `reclaim(before_disk_defer_here)`), and that stays.
- Every in-repo caller changes in the same lane; no re-export of the old `north_ops::ops::` paths survives.
- `north --help-all` prints the same verbs before and after each lane. No verb leaves the public workspace (see the last section but one).

## The map

### New crate `north-github-broker` (layer 1, platform)

The GitHub call broker: one process that holds the hourly quota, batches calls, serves `gh` and git credential requests through it and hands short-lived push tokens to hosts.

- `gh_broker` (2,021 lines): `north gh-broker serve|release|quota`, `north ops github budget|push-token`. It names only north-base, so it sits in the platform layer beside `north-broker`. It stays a crate apart from `north-broker`, whose domain is credentials (a policy ceiling per user and scoped tokens); this one meters calls.

### New crate `north-ci` (layer 6, operations)

The continuous integration north runs and watches for its repositories: checks that cannot run on GitHub, posted as commit statuses, and the main-is-red sheriff.

- `ci_watch` (1,355 lines): `north main-red`, `north ci-diagnose`, `north red-main-notify`. Reads the GitHub mirror (layer 2).
- `private_check` (540 lines): `north private-check`, and the run-time validator of `north-data/private-checks.toml` in `src/main.rs`. It runs a check's steps and posts the commit status, the commit-status domain `ci_watch` reads.
- Not `north-land-watch`: that crate watches the lander's own lanes and their checks, while these follow main's CI and private repositories that never pass through the lander.

### New crate `north-issue-boxes` (layer 6, operations)

An issue's checkboxes as work: convert unticked boxes into child issues, and tick a box whose Done-when check or observation passes.

- `issue_convert` (1,191 lines) and `issue_child` (149 lines): `north issue convert`, `north issue child`.
- `done_when_boxes` (824 lines), `observation_boxes` (138 lines) and `cargo_boxes` (599 lines): `north task check`, which ticks a box whose Done-when or `Observe:` command passes, and `north task rewrite-cargo`, which rewrites the cargo commands in open boxes onto Buck2. The root keeps passing the sandboxed shell in, so the crate does not depend on north-runtime.

### New crate `north-claims` (layer 6, operations)

Claims a written text makes about the tree, checked against it: a doc or skill citing a subcommand, flag, path, config key or metric source, and the fact items of a design review.

- `citation_check` (698 lines): `north check citations`, and the citation section of `north health`.
- `review_items` (403 lines): `north work review PLAN`. Its fact items run through `citation_check`, the one module of north-ops that names another, so the two move together.

### New crate `north-desktop` (layer 6, operations)

The desktops agents drive: an isolated private desktop, screenshots that leave the owner's clipboard alone, and the game client a playtest opens on them.

- `private_desktop` (1,666 lines): `north desktop private start|control|type|capture|clipboard|reap`, and the `private-desktop` re-entry in `src/main.rs`.
- `agent_capture` (103 lines): `north desktop capture`.
- `warcraft_close` (202 lines): `north desktop warcraft close`.
- `wc3_login_field` (238 lines): `north desktop warcraft login-field`, which types into a private desktop through `private_desktop`.

### New crate `north-reclaim` (layer 6, operations)

Disk room on the fleet's hosts: what may be deleted or archived, under which budget, and when.

- `reclaim` (1,263 lines): `north lane reclaim`, `north evidence start`, and the disk-defer hook the offload cache calls through its function argument.
- `retention` (847 lines): `north retain`, which archives evidence past its age to another host and deletes the source copy, the same delete-under-a-rule domain as `reclaim`.

### New crate `north-credentials` (layer 6, operations)

Every credential the fleet uses, with its kind, expiry and renewal path, and the renewals themselves.

- `credentials` (1,081 lines): `north ops credentials check|renew|import-cookies`, and the declared-credentials schema `north config operator check` reads.

### New crate `north-remote-serve` (layer 6, operations)

The command a trusted ssh peer runs on this host: its plan, guards, audit and log.

- `remote_serve` (1,074 lines): `north remote-serve` and `north remote-serve audit`. The command wiring stays in `src/cli/remote_serve.rs`.

### New crate `north-expectations` (layer 6, operations)

Declared expectations judged on a timer, each alerting once per breach and once per clear.

- `check` (366 lines): `north ops check`, the core expectations of `north-data/expectations.toml`, and their run-time validator.
- `watch` (793 lines): the work probes `north ops watch` runs after the landing and GitHub probes of `north-land-watch`.
- `dividend` (657 lines): `north ops dividend`, which judges each bet issue's dividend and messages the coordinator when it decays.

### Into existing crates

- `incident_mine` (319 lines) to `north-convo`, transcript search: `north incident-mine` mines transcripts through convo.
- `defaults_measure` (1,272 lines) to `north-tune`, tuned values: `north defaults-measure` sweeps and proposes measured defaults. Its one reach into `ops::report` is `wilson`, a re-export of `platform::pystats::wilson`, which it names directly.
- `strategy_log` (214 lines) to `north-replay`, strategy replay: `north ops strategy log check|retain` reads the decision log that `north ops strategy replay|refusals` already replays from that crate.
- `vendor_runtime` (528 lines) to `north-runtime`, runtime update: `north runtime claude|agy|pi|opencode` install the agent runtimes `north runtime agents` updates.
- `run_bounded` (157 lines) and `nexus_ram` (580 lines) to `north-capacity-host`, host leases and room: `north capacity bounded`, and `north nexus-ram record|report`, which judges one host's memory headroom beside `north capacity nexus` and the watchdog's samples.
- `job_run` (403 lines) to `north-timers`, the timers: `north job-run` writes the scheduled-job records that `job_watch` in the same crate reads.
- `report` (275 lines) to the root package, beside `src/cli/views/work.rs`: its only caller is the `north work` view, and its `run` (the retired `north fleet report`) has no caller and goes.
- `command` (12 lines) to the root package, folded into the `north ops` table in `src/cli/tools/subverbs.rs`.
- The three integration tests under `crates/north-ops/tests/` test other crates through north-ops' re-exports: `dispatch_pause_deploy.rs` and `health_filing.rs` move to `north-health`, `lane_gc_reaping.rs` to `north-lanes`.

Then `crates/north-ops/` is deleted with its crate-dag part, its workspace member, its Buck target and its place in the `ops` module entry of `structure.toml`, whose `paths` name the new crates instead.

### Drift with north#1276

Drift: docs/rationale/architecture-layers.md:440 sends north-core's `nix_gc` and `doctor_nix` to north-ops (L26); north-ops is gone after this split, so Nix upkeep (`north nix`, `north check nix`, about 600 lines) goes to a new crate `north-nix` (layer 6), named for its one domain.

## Verbs that serve only the owner's machines

Each of these hard-codes or defaults to one of the owner's hosts, accounts or repositories:

- `north desktop warcraft close` and `north desktop warcraft login-field` drive the owner's Steam launch and stored game accounts: `north-desktop`.
- `north nexus-ram record|report` measures the host named Nexus: `north-capacity-host`.
- `north retain` defaults to gex45 and its game evidence: `north-reclaim`.
- `north remote-serve`'s `NEXUS_PEER` and its south land policy serve the owner's operator repository: `north-remote-serve`.
- `north ops credentials import-cookies` reads the owner's Chrome profile: `north-credentials`.
- `north private-check` runs the checks `north-data/private-checks.toml` lists for the owner's private game repository: `north-ci`.

All of them stay in the public workspace, in the crate of their domain. The private values they use (accounts, profiles, checks) already live in the operator repository or `north-data`, so the code holds nothing private, and the operator repository has no Rust workspace to receive them: moving about 3,000 lines there would mean building and pinning a second workspace against north's crates, a change of direction the launch does not need. `north --help-all` therefore lists the same verbs after the split.

## What the fitness check must tell apart

Box 3 added a code-health rule that refuses a crate spanning unrelated domains. A text scan at this commit, counting the islands of each crate's top-level modules joined by `crate::` paths, shows that island count alone cannot be that rule: north-land-scans has 11 islands, north-graph-ingest 8 and north-docs 6, yet each is a family of independent peers serving one domain (the scans a landing runs, the loaders into the graph, the generated indexes). What sets the old north-ops apart is that its islands also reach disjoint parts of the workspace: `gh_broker` names only north-base, `ci_watch` the GitHub mirror, `private_desktop` capacity and pacing, `defaults_measure` the router. So the rule joins two modules when one names the other or both name the same workspace crate beyond north-base (`north code-health domains` joins on the crate a module names, not on the item it names), counts the islands left, and refuses above `max_islands` = 10, the count north-land-scans measures on main; north-fleet (14 islands on 2026-11-15) carries a dated allowance until its split is filed under north#1435. A fixture crate built like the old north-ops, modules that name nothing of each other and reach disjoint crates, must fail, and north-land-scans must pass.

## Result

`crates/north-ops/` is gone: `test ! -d crates/north-ops` exits 0 on main, and the persisted check `n1435-north-ops-stays-gone` in `docs/rationale/experiments.toml` runs the same test until 2026-12-15.

The second box's Done when has two parts, read on main on 2026-11-15:

- `north --help` lists the same top-level verbs before and after the split: the verb lines of `src/cli/help.txt` at the design's base commit `15fc1ebdb0` and on main both number 27, and a `diff` of the two sorted lists exits 0. No verb moved out of the public workspace.
- `north code-health domains --repo .` exits 0 with `domains: no crate spans more unrelated domains than its bound`. The two dated allowances in `structure/0235-domains.toml` are north-land-scans (under north#1487) and north-fleet (until its split is filed).

The group boxes of the split, read on origin/main at `7192b7d29` on 2026-11-16, each by its own filter: `git ls-tree --name-only origin/main crates/north-ops/src/ops/ | grep -E` with that group's names printed nothing (exit 1) for every group, and `north-ops` has no tree on main (`git ls-tree -d --name-only origin/main crates/north-ops` printed nothing):

- Group 1, GitHub and CI: `gh_broker` is in `north-github-broker`, `ci_watch` and `private_check` in `north-ci`, first landed in `ea982628f` (north#1487).
- Group 2, issue boxes and claims: `issue_convert`, `issue_child.rs`, `done_when_boxes`, `observation_boxes`, `cargo_boxes` are in `north-issue-boxes`, and `citation_check` and `review_items` in `north-claims`, first landed in `d730f20f8` (north#1487).
- Group 4, disk and host room: `reclaim` and `retention` are in `north-reclaim`, `run_bounded` and `nexus_ram` in `north-capacity-host`, `job_run` in `north-timers`, first landed in `269be1eb0` (north#1487).
- Group 7, expectations and removal: `check`, `watch` and `dividend` are in `north-expectations`, `report` and `command` in the root package, the three integration tests in `north-health` and `north-lanes`, and `crates/north-ops/` is deleted; the moves began in `cce923a3b`.

