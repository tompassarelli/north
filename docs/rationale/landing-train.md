# Landing train: main never red, finished work never waits on someone else's break

Owner: north#1247

Tom asked on 2026-11-04 for the most ambitious correct redesign of north's landing, with maximum
robustness and the fastest landing the design allows. This file holds the baseline measured that day,
the design, the boxes that build it in waves that each land value alone, and what is Tom's. Revised the
same day after an adversarial review (table at the end). Built so far: `landing.exact = true` in north's
.safe-push (lead commit 5c12c46f3, lane landing-exact), which is Wave 0's delivered state.

## Baseline, 2026-11-04 (laptop clock)

Sources: `~/.local/state/safe-push/stages.tsv` (one row per safe-push stage; times in ms), the
`land-check-*.log` files beside it, `gh run list` for Harness, Cargo, Land check and Main is red,
`git log origin/main`, and issue north#521.

| Measure | Value | Source |
|---|---|---|
| Landings on main, 7 days | 503 first-parent commits, 72/day | `git log --since='7 days ago' --first-parent` |
| Laptop safe-push landings since 2026-10-30 17:06 | 137 lanes; first safe-push to landed p50 7 min, p75 18, p90 39 | stages.tsv |
| Re-runs since 2026-10-30 | 195 land checks for 137 landings; 34 lanes ran 2+ times | stages.tsv |
| Today 00:00-06:00 | 99 land checks, 59 lanes, 37 landed (p50 5 min, p90 19), 20 lanes ran 2-5 times | stages.tsv |
| Land check wall time today | p50 327 s, p75 403 s, p90 508 s; 30,326 s total under laptop leases | stages.tsv `land checks` |
| Lanes stranded at 06:00 | 12 finished lanes waiting 29-100 min | stages.tsv rows with no `push` |
| Shared unrelated failure | 24 of 24 local machine-bound land checks from 04:55 to 05:43 failed on `bin/threads.test.sh` (7 of 27 cases) | land-check logs |
| Red main, goal_eta.test.py | culprit 18ab04ed1 landed 04:52; first Harness verdict 05:09; green 05:29: 37 min red, 17 undetected | Harness runs |
| Red main, prompt-grounding.test.sh | culprit 566e9007d landed 05:38; still red at 06:00 | Harness run 41733477195, north#521 |
| Red main, threads.test.sh | broken 04:34 to at least 05:43 (69+ min); machine-bound, so no main workflow runs it | land-check logs |
| Machine-bound list | 11 entries in harness/machine-bound-tests; 6 name files that do not exist and are skipped silently (land-checks.sh `add_test`, `[ -f "$1" ] \|\| return 0`) | `git cat-file -e` per entry on origin/main |
| Main CI verdicts | Harness 31 main runs today: 16 cancelled, 2 failed, 13 green; Cargo 24: 18 cancelled, 5 green | `cancel-in-progress: true` on main |
| Sheriff | 34 Main is red runs today, 0 reverts ("previous main run was not green"); 4 reverts in 30 days | north#521, `git log --grep Sheriff-Reverts` |
| Rebase conflicts on re-land | 3 of 7 lanes re-landed after b4c690bc5: docs/features.md twice, hook.rs + structure.toml once; resolved by hand | lead report |
| Hotspot | docs/features.md changed by 115 of 503 landings in 7 days (23%); next src/cli/mod.rs 60, src/cli/tools.rs 43 | `git log --name-only` |
| Actions | north is public, so hosted minutes are free; 40 concurrent jobs across the account, shared with wisp and makgora, with no API for free slots. land-check.yml runs one job per portable command (about 5). Land check run p50 298 s, p90 336 s (97 today, 27 failed) | `gh repo view`, land-check.yml, runs |
| Laptop-only timing | threads.test.sh 203-406 s on the laptop's 24 cores; Nexus has 3 | land-check logs, Nexus probe |

Three structural causes, each enough on its own to put a break on main:

1. **Selection gaps.** b4c690bc5 widened the .safe-push path filters of both land-checks.sh checks to
   `crates/`, but land-checks.sh still enqueues the north binary's dependants only for `src/*`,
   `crates/north-core/src/threads/*`, `crates/north-providers/src/ledger/*` and Cargo files (line 44), so a
   change elsewhere in `crates/` (18ab04ed1, 566e9007d) runs the check and selects no shell test that runs
   north. check-owners also reads `$HOME/code/south/main` and `$HOME/code/nixos-config/main`, whose
   revisions no verdict records.
2. **Untested merge result.** Without `landing.exact`, safe-push keeps a verdict when main moved on
   other files (`keep_verdict_through_moves`, bin/safe-push ~1990), so the pushed tree was not the tested
   tree; 9e259eb0a landed 4 min after the threads fixture change 08cde981f and broke 3 of its cases.
   Closed for lanes that land through safe-push by `landing.exact = true` (5c12c46f3).
3. **No usable verdict on main.** The sheriff needs a green parent run; with cancel-in-progress it rarely
   has one, and for machine-bound tests it has none.

The block on unrelated lanes follows from all three: every lane's land check runs the broken test,
fails, and the worker ends with `Stop: landing`.

### What `landing.exact` already delivers, and what remains

safe-push's exact mode (bin/safe-push usage ~128-140; train loop ~1775-1830; verdict rule ~1990-2012):
lanes queue for the per-origin lock in arrival order; the holder builds origin/main plus each lane
cherry-picked, runs the hook once on that exact tree, pushes it with
`--force-with-lease=<main>:<base>`, re-runs halves on red, counts a lane already on main
(`git cherry`) as landed, returns a conflicting lane with its files, and lets a lane naming the open
"main is red" issue jump the queue.

| Lander feature | State |
|---|---|
| Exact tree tested and pushed by compare-and-swap | delivered |
| Train of queued lanes, bisect by halves | delivered |
| Already-landed lane counts as landed; red-fix lanes first, in the queue and for build-host offload slots: a land check whose candidate carries a red-fix ticket takes the first offload slot that frees, ahead of regular lanes and worker builds that waited longer (north#646, 2026-11-06) | delivered |
| Train candidate checked by the land checks, so a repository with land checks batches instead of every lane re-running its own (fixed 2026-11-04: land checks had sent such repositories around the train) | delivered |
| Land-check runner picked automatically: `land.runner = build-host` resolves through orchestration.toml's hosts to a build host with room and starts its portable checks there at once, local-only checks run here meanwhile; an Actions exit 75 (dispatch refused with 403, 429 or 5xx, run never appeared, run queued past 600 s, ref push rejected twice) re-runs the same checks on a build host with room, or here under the land-check capacity lease when none has room; only that code falls back, a check failure on any runner refuses the landing. The remote verdict gates, the local candidate is what is pushed (fixed 2026-11-04 after GitHub's dispatch limit refused every north landing for about an hour) | delivered |
| Queue owned by the system on an always-on host (today the lock holder is a worker's turn on whichever host) | remains: Wave 2 |
| Base-relative judge per test case | remains: Wave 1 |
| Full suite with inputs outside the tree pinned | remains: Waves 0-1 |
| Mechanical conflict resolution | remains: Waves 0-1 (merge driver, split), 4 (model resolver) |
| Telemetry | remains: Wave 1 |
| Main ruleset with a dedicated lander App | remains: Wave 3 |
| Speculation, result reuse, extra runners | remains: Wave 4 |

## Design

One lander owns main. Workers queue; the lander tests the exact tree it will push, judges each test
case against the base, lands by compare-and-swap, and tells the parent. Every piece that can is built
into the code safe-push's exact train already calls (land-checks.sh, `north land tests`, git merge
drivers), so it pays off before the lander exists; the lander is safe-push's train loop ported to Rust
and run as a service.

1. **Identity.** A dedicated GitHub App, "North Lander", separate from the North Fleet App that
   bin/gh-app-token mints gex45's tokens from (ruleset bypass is per App, so a shared App would let
   gex45 bypass too). Installed on every repository it serves, permissions `contents: write`, `workflows: write`,
   `metadata: read`, `actions: write` (dispatch land-check.yml), `statuses: write`. `workflows` lets it
   land `.github/workflows` lanes, which safe-push's autoland path refuses (`touches_workflows`,
   bin/safe-push ~1982). Its key lives only in the lander unit's credentials directory.
2. **Host.** Nexus (lead's call, recorded below). The lander runs as a systemd unit with
   `DynamicUser=yes`, `InaccessiblePaths=/home/tom /run/secrets` and the lander key by
   `LoadCredential`, the pattern of the gex45 plan's south-land unit. Its own clones of
   north, south and nixos-config live in `StateDirectory`.
3. **Queue.** `safe-push --to main` runs the cheap local gates (language gate, scans, denylist
   attestation, the selected checks as an advisory pre-check), pushes `claude/land-<lane>-<sha>`, prints
   `Done: queued <branch>` and exits 0. The queue is those refs, so a lander restart loses nothing.
   Oldest first; lanes naming the open main-is-red issue first (kept from exact mode).
4. **Candidate.** origin/main plus one cherry-picked commit per lane. Generated files (Cargo.lock, the
   features index) are regenerated on the candidate, never merged. The denylist and publication scans
   run on the candidate before any public push (`land-check/<sha>` refs are public on north), since
   resolutions and regenerated files are new text.
5. **Full suite with pinned inputs.** Every `land.check` command runs with no path selection
   (land-checks.sh `--all`): portable commands as land-check.yml jobs on the candidate's `land-check/`
   ref, machine-bound and local-only commands on the lander. Every result is keyed by
   (tree, command, env), env = hash of the south and nixos-config revisions the lander's clones are at,
   the dev-shell closure path, and the host (machine-bound) or runner image (`ImageOS`, `ImageVersion`).
   A red result is reused only under the same env, so a change outside the tree clears it.
   Dependency-derived selection is rejected below.
6. **Per-case, base-relative judge.** Cases are libtest `test <name> ... ok|FAILED` lines, the
   `PASS|FAIL <name>` lines 10 of north's 35 shell tests print (`rg -l "printf .(PASS|FAIL)"`), else
   the whole file as one case. New failures = candidate's failing cases minus the base's failing cases
   under the same env. Base results are almost always cached: with exact landing the base is the last
   landed candidate. On a miss, only the failing files rerun on the base tree.
   - A case failing on the base enters **quarantine** (case, reason, issue, opened_at): the lander
     files or updates "main is red" and judges lanes on all other cases. Exit: the case passes on a
     later base tree (a new tree, not a retry). TTL 24 h: after it, lanes whose land-checks selection
     includes that test file are held (not refused) and the issue is escalated to the lead.
   - A **flake** is a case seen both passing and failing on the same (tree, command, env) in the store.
     A flaky case's pass is never counted green; the case is quarantined with reason `flaky`, a flake
     issue is filed, and it exits only when a lane naming that issue lands. Nothing is retried to
     green (Tom's rule).
7. **Bisect and speculate.** A red candidate splits into halves tested side by side (delivered by
   exact mode). Beside the head candidate the lander tests base+1..k for depth
   D = floor((B - in-flight lander jobs) / jobs per suite), B = 16 of the account's 40 jobs (leaves 24
   for wisp, makgora and main CI), so D <= 3 at 5 jobs per suite. D is computed from the lander's own
   runs; no API reports free account slots.
8. **Land by compare-and-swap** (delivered by exact mode): push only if origin/main is still the base;
   else restack and retest.
9. **Mechanical conflicts.** (a) docs/features.md, 23% of landings, is split one file per feature under
   docs/features/ with a generated index, early, instead of `merge=union` (union keeps both sides of an
   edited line and drops deletes). (b) A git merge driver, `north merge-driver set`, resolves only
   files and keys on an allowlist (`harness/merge-sets.toml`) of order-insensitive unique-entry lists,
   for example `may_use` arrays in structure.toml and harness/machine-bound-tests: it applies both
   sides' added and removed entries to the base set, refuses when one side removes an entry the other
   kept, and fails on a duplicate entry. Because git calls it in every rebase and cherry-pick, it works in
   today's safe-push. (c) Ordered lists are never resolved mechanically: hook.rs guard arrays run in
   order, `evaluate` returns on the first Deny and passes rewritten input to later guards
   (crates/north-core/src/hook.rs ~213-215). (d) Later, a staffed worker-haiku resolver for other
   conflicts of at most 40 lines, whose result re-enters the queue as a new lane. (e) Otherwise the lane
   goes back to its author. Every resolution lands only through step 5.
10. **No review state in the train.** The Reviewed-trailer gate was retired on 2026-11-04 (deterministic
    gates own the paths it covered; adversarial review is triggered for plans, trust-boundary code and
    judged cross-repo lanes and is fixed in the lane before it queues), so the lander carries no review
    record and a rebase never re-opens a review.
11. **Sheriff through the lander.** main-red.yml stops pushing main (the ruleset refuses its
    `GITHUB_TOKEN`); it pushes `claude/land-revert-<sha>` naming the failing run and, as its green parent,
    the newest earlier green run that is an ancestor of the failing commit, so cancelled or red runs
    in between (cancel-in-progress leaves many) no longer stop it. The sheriff, in the
    lander: new failing cases on main = failing cases minus quarantined cases minus the parent landing's
    failing cases. It bisects the landings between the green parent and the failing commit by halves,
    running the failing cases at the middle landing, to find the first landing where they fail, and
    builds main minus that lane's commit. If the failing cases pass there, it lands
    that revert first in the queue and re-queues the lane to its author with `Follows:`. If they still
    fail, the lane is not the cause: no revert; the cases are quarantined as `environment`, and their
    env inputs are diffed against the last green record. Loop breaker: at most one revert per lane per
    red episode, never a revert of a revert (kept from main-red.ts), and after two failed verifications
    the sheriff stops and escalates on the issue.
    A red run whose log shows GitHub refusing a credential or a failed fetch of a sibling repository
    (`north_check_environment::credential`) is an infrastructure incident, not a lane's failure: neither the
    sheriff nor the after-landing culprit search reverts for it (north#818; the sheriff had reverted a lane for a
    `check-owners` red whose log showed the refusal).
12. **Main CI.** Keeps cancel-in-progress (a per-SHA run would double Actions load for trees the lander
    already tested). The lander posts a `land-train` commit status on each landed SHA linking the
    candidate's results. A scheduled main run every 6 h detects environment drift.
13. **Owned landing.** One fleet message to the parent: `landed <branch> as <sha>`, or the failing cases,
    log URL and base comparison; a refused lane is re-queued to its author as a Follows box.
14. **After landing.** After-landing gates, follows-release and mutants run on the lander per landed
    lane, off the landing path (removes the shared mutant lock from landing, north#390).
15. **Telemetry.** `landings` (lane, branch, tip, queued_at, candidate, tree, batch_size, bisect_depth,
    attempts, actor, landed_at, landed_sha, verdict) and `main_red`
    (sha, red_at, green_at, cause, cases, revert_sha) in the Nexus threads ledger, written first by
    safe-push's exact train and later by the lander; `north ledger flow` prints finished-to-landed p50/p90,
    re-lands per lane, red-main minutes and bisect depth.
    Built 2026-11-04: the tables are created by the first `north land train` write (not bin/threads), carry a `repo`
    column, and a third table `land_checks` records each check run so the share thrown away because main moved is
    measured; `main_red` is written by `north land train red`, which the Main-red marker work calls.
    Added 2026-11-05 (north#255, estimation audit): a land check that runs past its deadline writes a
    `land_checks` row with outcome `timeout`, its `command` and `deadline_s` (the runner, through
    `north land train check`); candidate rows keep an empty command and `north ledger flow` counts only them. A
    timeout on a lane whose base tree also ran past the deadline doubles that command's next deadline (up
    to the 2100 s ceiling) until twenty later runs age it out, and a refused candidate's `landings.detail`
    names the land check or gate that failed before its log path.
16. **Speed.** (a) Actions results are reused by tree: land-check.yml is dispatched at main's ref with
    the tree's SHA as input, so no lane judges itself with an edited workflow, and such a run counts
    whoever dispatched it; a lane alone on an unmoved main lands on its queue-time run. (b) Machine-bound
    results count only from the lander host, with a warm `CARGO_TARGET_DIR`. (c) gex45 runs checks on
    queued lanes and speculative candidates with a read-only token; its red moves that lane to the front
    for a solo test and its green never counts, so it lands nothing. (d) If B4 puts Nexus on the critical
    path, threads.test.sh's 27 cases are sharded across Actions jobs after B5's fixtures.

### Guarantees and how they are measured

| Guarantee | Target | Measured by |
|---|---|---|
| Every main commit's tree was a tested candidate | 100% | `north land train audit --since 1d`: main commits with no `landings` row whose tree equals the commit's tree: 0 |
| Only the lander and Tom's admin role advance main | GitHub rule | `gh api repos/tompassarelli/north/rulesets` shows one main ruleset naming the lander App and the admin role; a push from a fleet session is refused |
| Red-main minutes attributable to a landed change | 0/day | `main_red` rows with cause `landing` over 7 days |
| A break never blocks unrelated lanes | 0 lanes refused for a case failing on the base | `landings.verdict` joined to quarantine |
| Finished to landed | p50 <= 12 min, p90 <= 20 min (baseline p50 7, p90 39 over 4.5 days, plus 12 stranded); a target to measure, priced below | `landings.queued_at` to `landed_at` |

### Pricing

- Time per green lane, target: queue wait (<= half a suite with D >= 1) + one suite + push. Actions
  part: land check p50 298 s today for selected checks; the full portable suite is bounded by Cargo
  314-418 s and Harness 122-224 s as separate jobs, so about 7 min. Nexus part: unmeasured; laptop
  threads.test.sh alone is 203-406 s on 24 cores, and Nexus has 3. B4 replaces this line with measured
  numbers; if Nexus exceeds 7 min it sets p50, and 16(d) or a larger Nexus (Tom's, cost) follows.
- Actions: about 5 jobs per candidate. Today 97 land-check runs of about 4 jobs are about 390 jobs; 72
  landings a day at a mean batch of 2-3, plus bisection and speculation within B = 16, is about 30-50
  candidates, 150-250 jobs. No per-SHA main CI is added.
- Laptop: 0 landing leases instead of today's 99 (30,326 s).
- Rejected in one line each:
  - GitHub merge queue: needs pull requests and is offered only to organization-owned repositories.
  - Wisp's Autoland in Actions alone: cannot run machine-bound checks without a private-repo secret, and its workflow token cannot push `.github/workflows`.
  - Per-lane safe-push with wider selection only: still strands lanes on base breaks.
  - Dependency-derived selection: the proof needs a build-graph model, and most lanes touch crates the `north` binary links, so it selects the full suite anyway.
  - Laptop as lander: it sleeps and the gaming hold defers heavy work. Superseded by D4 (lander on the laptop, heavy checks on gex45 slots).
  - Shared fleet App as lander: rejected; the lander has its own App (Identity above).
  - Per-SHA uncancelled main CI: doubles Actions load for trees the lander already tested.
  - `merge=union` on docs/features.md: keeps both sides of an edited line and drops deletes.
  - Per-file judge: one red case on the base masks a lane's new red case in the same file.

## Boxes

Each box is at most 20 minutes. Waves run in order and each lands value on its own; boxes within a
wave run in parallel unless they say Follows. Every box: Category feature, Spec exact, Scope north,
Surface workflow, Verify local-test unless stated.

### Wave 0: delivered exact landing, independent fixes and measurements

Value alone: no silent machine-bound skips, crates/ changes select the north-invoking tests, no false
dropped-commit refusals, and the numbers every later wave is sized by.

- **B1 Dropped-commit check by tree.** Category bug; Spec exact; files bin/safe-push
  (`lane_commits_missing`), bin/safe-push.test.sh. A lane passes when the rebased tree equals
  `git merge-tree --write-tree origin/main <lane tip>` or `git cherry` marks every commit `-`. Done
  when: `bash bin/safe-push.test.sh` passes two new cases (a lane already on main; a lane whose diff
  changes only in rebase context) and the existing dropped-commit case still refuses.
- **B2 Listed machine-bound tests must exist.** Category bug; files harness/scripts/land-checks.sh,
  harness/machine-bound-tests, harness/scripts/land-checks.test.sh. Done when: land-checks.test.sh has a
  case where a listed missing file exits non-zero naming it, and `bash harness/scripts/land-checks.sh
  machine-bound` on origin/main lists 0 missing files after the 6 dead entries are removed.
- **B3 Selection fix and select-all mode.** Category bug; files harness/scripts/land-checks.sh,
  land-checks.test.sh. `crates/*` enqueues the north binary; `--all` selects every test of the mode.
  Done when: land-checks.test.sh shows a `crates/north-landing/` change selecting
  harness tests that run north, and `land-checks.sh portable --all` selecting every portable
  `*.test.sh`/`*.test.py` under the roots.
- **B4 Measure the full suite.** Category measurement; Verify measured; Surface Nexus and Actions.
  Follows B3. Done when: this file records wall time and CPU seconds of every land.check command with
  `--all` at one origin/main SHA on Nexus (cold and warm target dir) and on Actions, the 5 surviving
  machine-bound tests on Nexus, and the replaced p50 line in Pricing.
- **B5 Machine-bound tests under a clean HOME on Nexus.** Category research; Surface Nexus; Verify
  measured. Done when: a table here lists, for each surviving machine-bound test, its exit under
  `env -i HOME=$(mktemp -d)` on Nexus, what it reads (machine-capacity.test.sh: real
  `systemctl --user`, 24-core fixture; spawn-capacity-guard.test.sh: /etc/codex/hooks/runtime/python3;
  threads.test.sh: `~/.local/state/threads` under its scratch HOME), and the fixture that would replace
  it.
- **B6 Probe ruleset bypass actors.** Category research; read-only API. Done when: this file records
  the API responses for whether a user-owned repository's ruleset accepts an Integration (App), a
  DeployKey and the RepositoryRole admin as bypass actors, whether an App needs `workflows` to push
  `.github/workflows` under a ruleset, and north's deploy keys with their write flag (`gh api
  repos/tompassarelli/north/keys`).
- **B7 Split docs/features.md, part 1: index generator.** Category feature; files src/cli (new
  `north docs features-index`), tests. Done when: `north docs features-index --check` fails on a planted
  stale index and passes on a generated one in a fixture.

### Wave 1: per-case judge, telemetry and conflict tools inside today's safe-push

Value alone: safe-push's exact train lands unrelated lanes while main is red, a lane's new red case is
never hidden by a base red in the same file, conflicts on set files and features vanish, and
finished-to-landed is measured.

- **B8 Per-case outcomes.** Category feature; Spec exact; files
  crates/north-land-checks/src/landing/land_test_runner.rs, its tests. Done when: `north land tests --cases
  json` on fixtures prints one row per libtest case, per `PASS|FAIL` line, and one whole-file row for a
  test that prints neither, with names and outcomes matching the fixture.
- **B9 Results store.** Category feature; files crates/north-landing/src/landing/ (new results module).
  Key (tree, command, env) as design step 5. Done when: unit tests show a hit for the same key, a miss
  after the south revision changes, and a case recorded both ways on one key reported flaky.
- **B10 Judge.** Follows B8, B9. Category feature; Spec exact. Done when: unit tests show: base red
  case A plus lane red case B in the same file names only B new; a base-red case enters quarantine and
  exits on a passing later base tree; a flaky case's pass does not count; a quarantine older than 24 h
  holds a lane that selects its file.
- **B11 Judge in the land check.** Follows B10. Delivered in Rust inside `north land tests`
  (crates/north-landing-gates/src/landing/base_relative.rs), not in bin/safe-push, since the S6a runner
  replaced the bash land-check runner. Inside a candidate check (SAFE_PUSH_LAND_BASE and
  SAFE_PUSH_LAND_HEAD set) every case is recorded in the results store under the head tree, keyed per
  test file (cargo test binaries by name without their build hash) and per repository (its root
  commit), so the landed candidate leaves main's tree on record for the next one without a separate
  after-landing step. A base-red case is quarantined (case, tree first seen red, when) in
  `case-results/quarantine/<root>.tsv` and printed; only the lane's new red cases refuse it; a base
  with no recorded results refuses as before. tests/it/land_tests.rs shows each.
- **B12 Ledger tables and train records.** Category feature; files crates/north-core/src/threads
  (schema), bin/safe-push (`train_verdict` calls `north land train record`). Done when: a safe-push
  exact-mode test run leaves one `landings` row per lane with tree, batch_size, attempts and verdict.
- **B13 Telemetry in north ledger flow.** Follows B12. Files src/cli (flow). Done when: `north ledger flow` on a fixture ledger prints
  finished-to-landed p50 and p90, re-lands, red-main minutes today and the deepest bisect.
- **B14 Set merge driver.** Category feature; Spec exact; files src/cli (`north merge-driver set`),
  harness/merge-sets.toml, .gitattributes, bin/safe-push (sets `merge.set.driver` in the clone).
  Done when: a property test over random add/remove pairs applies both sides exactly and refuses a
  remove-versus-keep; a duplicate is refused; today's structure.toml conflict replays to the hand
  resolution; a hook.rs guard array conflict stays a conflict.
- **B15 Split docs/features.md, part 2.** Follows B7. Category program-craftsmanship. Done when:
  features live one per file under docs/features/, every in-repo reader of docs/features.md is updated,
  the index check is a land check, and a scripted two-branch test adding a feature each under one
  heading rebases with no conflict.

### Wave 2: the lander on Nexus, shadow then cutover

Value alone: the queue is the system's, a worker's turn ends at `Done: queued`, landing no longer
uses laptop leases or depends on the laptop being awake.

- **B16 Lander App credential.** Follows Tom's App creation (below). Files bin/gh-app-token (`--app
  lander`), south secrets. Done when: on Nexus, `gh-app-token --app lander` mints a token whose
  `gh api /installation/repositories` lists only north and whose permissions show contents and
  workflows write.
- **B17 Lander unit.** Category infra; Scope cross-repo (firn module, south hosts/nexus). Done when:
  `systemctl show north-land-train` on Nexus reports DynamicUser=yes and the two InaccessiblePaths, and
  a probe in the unit reading `/run/secrets` fails.
- **B18 Train loop in Rust.** Follows B10, B12. Files crates/north-landing/src/train/ (new). Port of
  safe-push's exact train (cherry-pick stack, CAS push, already-landed, conflict verdict, halves)
  reading `claude/land-*` refs oldest first. Done when: fixture tests of the existing exact-train
  cases from bin/safe-push.test.sh pass against `north land train serve --once` in a local bare repo.
- **B19 Suite runner.** Follows B3, B9, B18. Done when: one fixture candidate dispatches land-check.yml
  at main's ref with every portable command and the candidate SHA as input after a denylist pass, runs the
  machine-bound commands locally, stores results by key, and a second run on the same tree dispatches
  nothing.
- **B20 Shadow replay.** Follows B19; Surface Nexus; Verify measured. Pushing nothing, the lander
  rebuilds candidates on their real bases and must refuse: 18ab04ed1, 566e9007d, and 9e259eb0a on a base
  containing 08cde981f; a planted flaky case (not counted green); a planted red case in a file already
  red on the base (named new); and a check-owners run after a planted south change (red not reused).
  It must land 10 of today's clean landings. Done when: this file records the 16 verdicts, all as
  expected.
- **B21 Notification and client switch.** Follows B20. Files .safe-push (`landing.queue = train`),
  bin/safe-push, harness/skills/workers/references/notes.md § Landing mechanics. Done when: a worker's
  `safe-push --to main` prints `Done: queued <branch>` and exits 0, the lander lands it, and the parent
  receives one `landed <branch> as <sha>` fleet message.

### Wave 3: main protected, sheriff through the lander

Value alone: "only tested trees reach main" becomes a GitHub rule, and a red main is reverted to the
right lane only.

- **B22 Sheriff in the lander.** Follows B18, B12. Files harness/scripts/main-red.ts, .github/workflows/
  main-red.yml, crates/north-landing/src/train/. Done when: tests show: a two-lane range where only
  removing the older lane turns main green reverts only that lane; a failure that persists without the lane
  reverts nothing and records cause `environment`; a quarantined case triggers nothing; a second revert
  of one lane in one episode is refused; main-red.yml's job pushes only `claude/land-revert-*`.
- **B23** Category infra; Scope cross-repo (south); recorded in south.
- **B24 Main ruleset.** Follows B6, B16, B21, B22. Category infra; Surface GitHub settings. Done when: `gh api repos/tompassarelli/north/rulesets`
  shows the main ruleset with bypass actors the lander App and the admin role, a push to main with a
  North Fleet App token and one from a workflow `GITHUB_TOKEN` are both refused, and the lander lands the
  next queued lane.
- **B25 Removed.** The review gate it would have rebuilt was retired on 2026-11-04.

### Wave 4: speed

Value alone: each box cuts p50 or p90 measured by B13.

- **B26 Speculative candidates.** Follows B19. Files crates/north-landing/src/train/. Done when: with B = 16 and 5 jobs per suite, a fixture
  with 3 queued green lanes runs D = 3 candidates at once and lands all 3 within 1.2 suite lengths.
- **B27 Reuse by tree.** Follows B19. Files crates/north-landing/src/train/, bin/safe-push. Done when: a
  lane whose queue-time land-check run, dispatched at main's ref, tested the same tree the lander would build lands with 0 new Actions jobs.
- **B28 gex45 red-only runner.** Follows B19; Scope cross-repo (south hosts/gex45); files
  crates/north-landing/src/train/. Done when: a planted red lane run on
  gex45 moves to the queue front for a solo test, a gex45 green leaves the lane's verdict unchanged,
  and the gex45 runner's token lists only `contents: read`.
- **B29 Haiku conflict resolver.** Follows B18, B21. Files crates/north-landing/src/train/, worker brief
  template. Done when: a conflict of at most 40 non-set lines
  queues one worker-haiku job with both sides and the base, and its pushed resolution re-enters the
  queue as a lane.
- **B30 After-landing work on the lander.** Follows B21. Files crates/north-landing (after-landing), bin/safe-push. Done when: after-landing gates, follows-release
  and the mutant run fire once per landed lane, and no landing waits on the mutant lock.

### Wave 5: measure

- **B31 24-hour measurement.** Follows B24. Category measurement; Verify measured; files this one. Done when: this file records, for the
  first 24 hours on the protected train, the five guarantee measures against the baseline.

## safe-push in Rust

Tom asked on 2026-11-04 for hyperspeed landings and Rust-only code. bin/safe-push is north's largest
non-Rust file (2,452 lines of allowlisted bash) and it runs every landing. The lead set the target the
same morning after measuring the cost of every lane landing itself: about 25 lanes each ran their own
safe-push, pushed their own land-check ref and dispatched their own Actions run, every main move
re-ran all of them at once, and the burst tripped GitHub's secondary rate limit (403 on workflow
dispatch, "fatal error in commit_refs", a rejected push to main) while about 30 workers sat idle
waiting on their own landings. So the port does not make each lane's safe-push faster. It builds
**one always-on lander, `north land serve`**, ports safe-push's stages behind it, and deletes the parts
the lander makes unnecessary instead of porting them. Planned at origin/main 602d51c9b.

### Starting point (measured 2026-11-04)

- bin/safe-push is 2,452 lines. bin/safe-push.test.sh is 2,844 lines: 4 parallel sections, 80 fixture
  blocks (each named by `make_real_fixture`, `new_landing_origin` or `new_lane_container`; the case
  names below), 47 `PASS:` lines and 289 `fail` assertions. Two blocks eval bash function bodies out
  of the target (`sync_dependencies`, `tool_removal_scan`).
- Already Rust, in crates/north-landing/src/landing/ (914 lines): arguments, help text, destination
  snapshot, publication plan and one-branch-main (safe_push_plan.rs); repository state and remote
  snapshot (safe_push_state.rs); language gate, native-ref, betterleaks history and annotation scans,
  email scan, deny-list and instruction limits (safe_push_guards.rs); and the whole tag path
  (`north safe-push --tag`, safe_push_tag.rs). bash calls about 20 `north` subcommands: land actions,
  land red-check, land-red-wait, land clean, stale-revert, lane squash, dep-gate, denylist,
  language-gate, build-cache-gate, bin-references, review-gate, after-landing, follows-release,
  runtime follow, capacity, lane reap, land abandon, safe-push-result.
- Still bash: the driver (main flow ~2219-2452), the landing lock and ticket queue (~1549-1665), the
  train (~1667-1918), rebase and the verdict rule (~1920-2046), land checks (~1097-1300), Autoland
  (~2048-2178), lane cleanup and reap (~1302-1466), and the bash-only scans: tool removal, test
  integrity, generated rewrite, policy sweep, review, CI budget, code health, dependency sync.
- `safe-push --dry-run --to main` already runs every gate and every selected land check without
  taking the lock or pushing. That is how the first lander box gets today's checks without porting
  any of them.
- Rust integration tests already drive bin/safe-push from fixtures (tests/safe_push_land_branch.rs
  `Lane`, tests/safe_push_tag.rs).

### Target: five properties

1. **Workers enqueue and end their turn.** `safe-push --enqueue` (and, once a repository opts in,
   plain `safe-push --to main`) folds the lane to one commit (`north lane squash`, as today), writes a
   ticket naming the lane, its head and its author session, prints
   `Done: queued <worktree> <branch> <sha>` and exits 0. No worker waits on a landing.
2. **One lander owns landing.** `north land serve` runs as a user service on the laptop. It owns the
   queue, ordering and every GitHub write for landings: one main push per candidate, by
   compare-and-swap (`--force-with-lease=refs/heads/main:<base>`), with the north-lander App token
   once B16 lands. With warm-slot checks (S6b) it dispatches no Actions run per candidate.
3. **Speculative candidates.** While candidate N checks, N+1 (N plus the lanes that arrived since)
   starts. A red candidate checks each of its lanes alone at once (north#360), ejects only the culprit lane and messages that lane's
   author session; when every lane passes alone it checks the queue-order prefixes of the candidate together at once, lands the longest green one and refuses the lane after it as an interaction. N+1's verdict counts only if N landed and main is still N at its push.
4. **Warm build slots, selection by changed paths, Actions confirms.** Candidate checks run in
   persistent slots: on the laptop, the lander's stable worktrees (one per speculation depth), so
   `target/` survives and compiles are incremental; on gex45, the offload slots, whose clean keeps
   `/target` (crates/north-runtime/src/land_offload/ship/script.rs, `git clean -ffdxq -e /target`). A slot
   target past its 40 GiB cap is trimmed to 24 GiB, stale incremental state and test binaries first, by
   `north capacity offload trim-target`, and removed only when that cannot bring it under: on 2026-11-08
   whole targets of about 41 GiB were removed on hosts with 130 to 250 GiB free, so the next check
   compiled up to 235 crates cold (north#935). The
   `land.check` path filters select checks from base..candidate as today. The full suite runs on main
   after the push (Cargo and Harness workflows), and the sheriff reverts a confirmed culprit through a
   red-fix ticket, so Actions confirms rather than gates.
5. **Every guarantee safe-push gives today keeps a named test** (table below). Lock-specific cases
   whose mechanism disappears keep their guarantee as a lander test.

### Ticket and queue (fixed here so S1 and S2a run in parallel)

`$XDG_STATE_HOME/north/land-queue/<key>/<epoch µs>-<branch slug>.json`, with `<key>` the first 16 hex
characters of sha256(origin push URL), the key bash's lock already uses. Fields: `origin`, `root`
(lane worktree), `host`, `branch`, `head` (40 hex), `session` (`NORTH_FLEET_SESSION`), `queued_at`
(RFC 3339), `queued_micros` (epoch µs, the order key), and `red_fix` (issue number, `marker` for a `Main-red:` trailer, or null). Written to a
temporary file and renamed, so the lander never reads half a ticket. The lander moves a decided
ticket to `<key>/done/` with a `verdict` field: `landed <sha>`, `conflict <files>`,
`failed <log-or-url>`. Order: red-fix tickets first, then oldest by the `queued_micros` time enqueue records in the ticket, never by the file name, so renaming a ticket does not move it. A re-enqueued lane replaces its
older undecided ticket. Lane commits are fetched from `root` (same host) or `ssh://<host><root>`
(S11).

### Cut-over without two landing locks

The lander never takes bash's flock and keeps no lock of its own. Lanes are ordered by its single
queue, and it meets everything else that pushes main only at origin's compare-and-swap, which GitHub
enforces. So two implementations of the landing lock never exist at once:

- A repository moves from per-lane bash landing to the lander with one committed line,
  `landing.queue = lander` in its `.safe-push` (S4). bash reads it from fetched origin/main, as it
  reads `landing.queue = autoland` today (`autoland_queue`). A bin/safe-push older than S4 treats
  any unknown value as "not autoland" (`autoland_repo`, ~2048-2056) and keeps landing per lane. That
  is safe (it still lands by lease) but it is not property 1, so S4's Done when checks every landing
  host's `~/code/north/main` checkout. Until remote enqueue (S11) lands, the switch applies on the
  lander host only (`landing.lander-host`); other hosts keep landing per lane, by lease.
- A bash safe-push already past its checks when the lander pushes fails as it does today when a peer
  host lands under it: `revalidate_repository_state` refuses with "origin destination changed", or
  its lease push is rejected. Nothing is overwritten, but that lane fails and its worker re-runs
  `safe-push --to main`, which then enqueues. If the bash lane wins, the lander's lease fails and the
  lander rebuilds its candidate on the new main (`serve-cas-lost`).
- The bash lock, ticket queue and train stay in bin/safe-push for repositories that have not moved,
  and are deleted (not ported) in S19b, after the last repository moves (S18).
- S18 makes the lander the default: a main whose `.safe-push` sets no `landing.queue` is read as `lander`, so the committed line is only needed to name `lander-host` for one repository; `landing.queue = direct` (or `lock`, `autoland`) keeps the per-lane path.
- Rollback is reverting the `landing.queue = lander` line; the per-lane path never stopped working.
- North#482's rule holds: bin/safe-push calls `north land enqueue` only after probing
  `north land enqueue --help`, and fails with "run north runtime update" when the runtime lacks it.
  The S4 switch lands only when the lander host's runtime reports the subcommand.

### How the lander checks a candidate before the stages are ported

`safe-push --dry-run` is close to what the lander needs, but it skips things a real landing does: it
reads no red-fix state, so a failed Actions check can call `retry_after_red_main` and block in
`north land-red-wait` for up to 21,600 s (red_check.rs). It sets no `landing.capacity` wrapper and no
offload host (~2170-2177). It revalidates the destination against origin, which breaks speculation.
And every refusal exits 1, so a moved base or a transient failure looks the same as a culprit. So
S2a adds an explicit bash mode, `safe-push --check-candidate --base SHA`, and deletes `usage()` (97
lines that duplicate safe_push_usage.txt) in the same change, so the file still shrinks. The mode
runs what a real `--to main` landing runs between the lock and the push: the language gate, scans,
dependency gate, policy scans and land checks, under the repository's capacity wrapper and offload
choice. It differs in four ways:

- `SHA` is the destination, so the scan range and land-check selection are `SHA..HEAD`, and origin is
  not revalidated (the lander's lease does that).
- It never waits on red main.
- It neither pushes nor takes the lock.
- It ends with one line, `safe-push: verdict <class> <detail>`, where class is one of:
  - `green`;
  - `refused` (a gate or land check failed on the candidate);
  - `red-main` (`north land red-check` exit 3: the failure is main's);
  - `transient` (fetch, `bun install`, GitHub rejecting the land-check ref twice: exit 75);
  - `deferred` (capacity DEFER).

The lander searches for the culprit only on `refused`: it checks each lane of the refused candidate alone, at once, one worktree per lane (in waves of the free offload slots when there are fewer than lanes), refuses the lanes that fail alone with their own logs, and checks the rest together as one candidate. A lane that ends transient or deferred alone keeps its ticket; when every lane passes alone the refusal is an interaction, so it checks the prefixes of the candidate (its first 2, 3, ... lanes in queue order, widest first when slots run short) together at once, lands the longest green prefix as one candidate without checking it again, refuses the lane right after it with "interaction with" the prefix's lanes and that prefix's failing log, and leaves the later lanes queued; the whole candidate is already known red, so a clashing pair costs no further check. A prefix that ends transient or deferred holds the lane after the longest green prefix, with strikes kept per prefix. On `red-main` it holds the batch and lands red-fix tickets
first. On `transient` and `deferred` it retries the same candidate later. If main moved during the
run, it ignores the verdict and rebuilds.

**A lane-caused failure never waits on a host (north#710, 2026-11-07).** On 2026-11-07 the queue stood still about 40 minutes because a lane added crates the check host never fetched and its `--offline` check failed with "failed to download". Four rules now stop the lander blaming the host for what a lane caused. (1) A missing crate under `--offline` is an unfit host only when the check's own log shows the same land check failing on the base tree too (`safe-push` already reruns a failed land check on main on the same host and logs `and also fails on base tree`); when main passed the rerun the verdict stays `refused`, and the culprit search checks each lane alone. (2) Transient and deferred streaks are counted per lane, keyed by ticket file and head and cleared by any other verdict on that lane, so lanes arriving every round cannot reset a lane's count: a candidate holding a lane in 3 transient candidates skips the candidate check and checks each lane alone, and an alone check that is transient again after that refuses the lane as a persistent transient. (3) When the oldest waiting lane is over 20 minutes old and the lander's latest round waited on a transient verdict, the watchdog's stale-lane repair asks the lander (the `check-alone` file in the queue, at most once per 20 minutes) to check each lane of its next candidate alone. (4) `north land enqueue` runs `depbox fetch -- cargo fetch --locked` for a lane that changes `Cargo.lock` and refuses a lane whose crates cannot be fetched, so the lander's candidate check finds them; the lander also fetches an unrecorded `Cargo.lock` before its check and refuses the lane that changed it when the fetch fails.

**Per-lane pre-check (north#360, 2026-11-05).** The landings of 2026-11-05 showed 58 lanes whose checks took 3.2 min on average, while enqueue to check start took 56 min: the wait came from one candidate at a time, batches of up to 23 lanes and a refused batch halved for up to 5 rounds. A repository whose `.safe-push` sets `landing.slots = N` (north sets 3) is served by `land/precheck.rs` instead. Each ticket's lane is checked alone on current main in the first free build slot (the `lander-precheck-<i>` worktrees, each owning a slot) within one 250 ms tick of its enqueue; tickets wait in queue order when every slot is busy. A green lane lands in the tick its verdict arrives, without waiting for lanes queued ahead of it that are still checking (ready lanes go in queue order among themselves): on 2026-11-05 a lane whose offloaded check passed at 2:22 PM sat unlanded and silent for 40 minutes behind about 30 earlier lanes, one of which retried a broken-base check 16 times. A lane whose base is still main pushes its checked tree; one whose changed files meet the files main changed since its check started is checked again in full on the new main; any other runs only the repository's `[land] recheck` lines on the exact tree it would land (north runs `cargo check --tests` on the crates the lane affects, about 20 s on a warm build host, against a full check of about 3 minutes), falling back to the full check when the repository has none, because main never takes a tree no check ran on and with about 35 landings an hour nearly every lane's main moves during its check. File-disjoint lanes can still break each other: on 2026-11-05 one lane added a field to a Rust struct, another built that struct in a different file, and the old shortcut that skipped the recheck for disjoint files landed both and left main unable to compile (north#582). A refused lane is refused alone with its own log and holds no one. A transient, deferred or red-main verdict holds that lane for 30 s and strikes it like a culprit search does. The batch train above remains for a repository without `landing.slots` and for a lander whose build slots cannot be prepared.

**Shadow checks (north#360, 2026-11-05).** `north land shadow tick` (with no `--repo` it covers every repository the lander serves and stops after the first that ran a check), run every 2 minutes by the user timer `north-land-shadow.timer` under a lock file so only one runs at a time, takes the first queued lane (red-fix first, then oldest) that lacks a stored verdict for any check line of its candidate, builds that candidate in a scratch worktree, runs each missing line through `north land checks` under capacity admission on the host `north capacity offload choose --any` names, and records a pass or fail per (lane head, main sha, input hash) so the lander finds the verdict ready. It takes no lander lock and holds no build slot, and a round with no host that has room runs nothing.

The mode does not cover these, and the lander does them itself:

- `land.publish` refs (makgora only).
- The pre-push hook for repositories without `land.check`: that is north-only work in Wave L0; S18
  runs the hook on the candidate for the other repositories, as `check_candidate` does today.
- `run_queue_branch_hook` and `denylist_attest` (Autoland only).
- Per-lane stale-revert, and the already-landed check: S2a runs `north stale-revert --lane <lane head>
  <picked onto> HEAD` and `git cherry` at each pick, as `run_train` does.

In Wave L0, north's portable land checks still go to Actions. Each candidate run costs one
land-check ref push, one dispatch and one ref delete (land_actions.rs), plus the main push. That is
3-4 writes per candidate instead of 3 per lane on every main move. S6b takes the dispatch off the
gate path.

### Stages, modules and boxes

Rust modules live in crates/north-landing/src/land/ (new, beside landing/).

| Stage (bash) | Rust module | Box |
|---|---|---|
| Enqueue, lane fold, ticket write | queue.rs, enqueue.rs (lane_squash.rs) | S1 |
| Landing lock and ticket queue (~1549-1665) | replaced by queue.rs ordering plus origin CAS; bash deleted | S2a, S19b |
| Train collect, candidate build, run, bisect by halves, already-landed (~1667-1918) | serve.rs, candidate.rs, bisect.rs | S2a (build, check, CAS), S2b (bisect, verdict classes) |
| Speculation (B26) | speculate.rs | S5 |
| Red-main lookup and priority (`red_fix_issue`, Main-red marker, ~1572-1592) | queue.rs ordering, red.rs | S1 (marker), S2b (gh lookup) |
| Blocked-by-red retry (`red_main_check`, `retry_after_red_main`, ~1264-1278) | red.rs on red_check.rs; with quarantine (B10) a base-red case stops refusing lanes | S2b, S10 |
| Rebase, conflict report, merge-driver config (~1942-1968) | candidate.rs (cherry-pick on the lander's worktree) | S2a |
| Verdict rule, exact mode (`keep_verdict_through_moves`, ~2009-2046) | no counterpart: the lander pushes only the tree it checked | S2a, S19b |
| Pre-push hook on the candidate (~1970-2008) | checks.rs (hook run in the slot) | S18 (north has land.check, so Wave L0 needs no hook) |
| Land checks, local: selection, runner, deadline, frozen, orphans, logs, lease, publish ref (~1097-1300) | slots/runner.rs, slots/select.rs | S6a |
| Land checks on Actions (`north land actions`) and publication scan | dropped from the gate path by S6b; land_actions.rs kept for confirm runs and repositories not on slots | S6b |
| Land checks offloaded to gex45 (`north capacity offload`) | slots/runner.rs, placement rules in slots/select.rs | S6b |
| Candidate check mode (`safe-push --check-candidate --base SHA`) | checks.rs calls it; deleted in S19b when every stage is in-process | S2a, S19b |
| Language, build-cache and bin-reference gates (~1062-1096) | scans/language.rs on safe_push_guards::language_gate | S12 |
| betterleaks, deny-list, commit email, native-ref (~441-535, 2312-2330) | scans/secrets.rs on safe_push_guards, denylist.rs | S13 |
| Dependency gate (`north dep-gate`) | scans/dep_gate.rs on dep_gate.rs, in-process | S14 |
| Tool removal, test integrity, vendored trees (~536-606) | scans/tool_removal.rs, scans/test_integrity.rs | S14 |
| Policy sweep (`policy_sweep_scan`); the review gate and durable paths were retired in 58df15b56 (2026-11-04) | scans/policy.rs | S15 |
| Generated rewrite (~607-683) | scans/generated.rs | S15 |
| CI budget, code health, instruction limits, after-landing gates (~717-1023) | after/gates.rs; `north after-landing` calls it in-process, not `safe-push --after-landing` | S16 |
| Stale-revert and dropped-commit checks (~1024-1057) | candidate.rs, per lane as it is picked (stale-revert and already-landed in S2a; the bash functions in S17) | S2a, S17 |
| Dependency sync, bun (~1740-1764) | slots/deps.rs | S17 |
| Runtime follow: follows-release, `north runtime follow` (~1459-1466) | after/follow.rs, once per landed candidate | S2c |
| Lane cleanup, auto-reap, land clean (~1302-1466) | after/cleanup.rs, run by the lander after the landing | S21 |
| Stage telemetry: stages.tsv, pushes.tsv (one row per lane, the 7 columns fleet landings reads), landings table (B12) | telemetry.rs | S2c (tsv), S9 (ledger) |
| Terminal receipt (`safe-push-result`), NOT-landed check | enqueue receipt `Done: queued`; the lander's fleet message names the landed sha after `git cherry` shows every lane commit on main | S1, S2c |
| Autoland queue, gap, `--direct` (~2048-2178) | wisp and makgora move to the lander, or keep Autoland (lead's call in S18); deleted in S19b | S18, S19b |
| Publication path for non-default branches, scratch refs, races, lease (~220-435, 1467-1527, 2390-2452) | push.rs on safe_push_plan.rs and safe_push_state.rs | S19a |
| Tags, abandon | already Rust (safe_push_tag.rs, land_abandon.rs) | S19b (exec only) |

### Guarantees and the tests that keep them

"Lander test" means a named test in tests/it/land_serve.rs; "per-lane" means the case stays in the bash
suite until S20 ports it to `cargo test --test it land_`, under the same name.

| Guarantee | Today's cases | After the port |
|---|---|---|
| Nothing lands unless every blocking gate passes; every failure is named once | all-at-once, language-gate-boundary, language-<mode>, dep-gate, denylist-public-finding, email-to-main, native-ref, test-integrity, vendored-tree, mutant-record, tool-removal, policy-sweep, generated-rewrite | per-lane, plus lander test `serve-gate-refusal` (a candidate whose check refuses is not pushed, and the lane is told which gates refused) |
| Fail closed without north or betterleaks, or when origin can't be fetched | language-initial, train-language-<mode>, unreachable-fetch | per-lane, plus `serve-no-betterleaks` |
| The pushed object is the one scanned; any change to repository, branch, HEAD, worktree or destination during the scan refuses | race-branch, race-commit, race-destination, race-worktree, lease-boundary | per-lane; the lander pushes the candidate sha it checked, pinned by `serve-pushes-checked-tree` and `serve-candidate-head-unchanged` (the lander worktree's HEAD or gitdir moved during the check: nothing pushed) |
| No non-fast-forward, no silent publication of a non-main branch, symbolic destinations refused | non-fast-forward, no-upstream, inherited-upstream, symbolic-destination, normal, planning-outcomes, new-destination, explicit-main, dry-run-destination, argument-boundary | per-lane, then S19a's push.rs under the same names |
| Landings on one origin are ordered, red-fix lanes first | concurrent, queued, main-red-marker, red-fix, deferred-capacity | `serve-train`, `serve-red-fix-first`, `serve-reenqueue-replaces`, `serve-defer` (a capacity DEFER retries the same candidate and ejects no lane) |
| No landing is lost or doubled when the landing process dies or hangs | killed-holder, dead-check-orphans, hung-check-timeout, frozen-check-timeout, cancelled-admission | `serve-restart` (lander killed mid-check, restarted: every lane lands once, nothing pushed twice) plus runner tests ported in S6a under the same names |
| Checks never hold a resource others wait on; leases only around the local run | admitted-check-unlocked, lease-around-local-check, lease-released-for-actions-wait, land-check-boundary (descriptors) | S6a runner tests under the same names |
| Re-entry and scratch pushes never deadlock | reentry, farm-scratch | `serve-reentry` (a land check that runs `safe-push` to a scratch branch neither enqueues nor waits) and `serve-scratch-push-bypasses-queue` |
| The pushed tree is the checked tree | exact, kept-verdict-second-move-<kind>, land-check-retry | `serve-pushes-checked-tree`, `serve-cas-lost`, `speculate-discard-on-red` |
| Train: exact candidate checked once and pushed by lease; conflicts and failures returned per lane; halves; already-landed lanes count as landed | train, train-mixed, train-captured-candidate, train-deps, the train land-check cases of 617e2eeeb | `serve-train`, `serve-mixed`, `serve-captured-candidate`, `serve-deps` |
| No lane commit is dropped; no false drop refusal | rebase-drops-commit, relanded-lane-rewritten-main, lane-already-on-main, rebase-context-only | `serve-dropped-commit`, `serve-already-on-main`, `serve-context-only`, `serve-transient-not-culprit` (S2b: a transient or moved-base verdict ejects no lane) |
| A revert of newer main is refused, and the checks still report | stale-revert-before-land-checks | `serve-stale-revert` (S2a, per pick) |
| A conflict leaves the lane's worktree untouched | conflict, hook-fails | `serve-mixed` (the lane worktree's HEAD and status are unchanged; the lander rebases only its own worktree) |
| The pre-push hook passes on the exact commit that lands, and runs outside any lock | hooked, hooked-overlap, hook-fails | per-lane until S18, then `serve-hook-candidate` (hook repositories: the hook runs once on the candidate, a failing hook ejects by halves) |
| "Landed" is reported only when main carries every lane commit | push-without-movement, autoland (terminal sha) | `serve-landed-only-on-main` |
| Land-check logs kept on failure and pruned on success; refs it publishes are deleted | land-check-boundary, land-publish | S6a runner tests |
| A lane lands as one commit; published commits are never rewritten | squash-one, squash-published, squash-several, squash-message | per-lane (enqueue runs the same fold), plus `enqueue-squash` |
| Lane cleanup removes only a spent, vacant lane | clean, pid-namespace, keep, dirty, remote-merged, remote-unmerged | ported to the lander under the same names (S21) |
| Autoland lanes and claude/ attestations | autoland, denylist-attest | per-lane until S18 and S19b decide Autoland |
| After-landing gates and runtime follow run off the landing path | code-health-landed, autoland (follows-release through the runtime), the after-landing trace in language-gate-boundary | `serve-after-landing` (once per landed candidate, in the background) |
| CI budget | ci-budget, ci-unconfigured, ci-filtered | per-lane, then S16 |
| Dependency installs match the tree | fresh-north-worktree, the two sync_dependencies blocks, train-deps | `serve-deps`, S17 |
| Workers never wait | none today | `enqueue-returns` (exits 0 within 5 s with a ticket on disk, nothing pushed) |
| Lander worktrees survive reapers; lane-controlled check commands never see the push token | none today | `serve-worktree-not-reaped` (lane gc, lane reap and auto-reap skip `worktrees/lander*`), `serve-token-scoped` (check env has no `GH_TOKEN`/App token; only the push step does) |
| GitHub traffic per candidate is bounded | none today | `serve-github-writes` (the gh/git shim trace for a 3-lane candidate shows 1 main push plus, in Wave L0, 1 land-check ref push, 1 dispatch and 1 ref delete; 0 dispatches after S6b) |

### The protocol model

The stateright model of the landing protocol (`crates/north-landing/tests/lander_model.rs`) was deleted in the north-landing test cull (north#635, 2026-11-06): it checked a model rather than the lander, had already drifted from it (`4efb7995b`, then the batch landing of `152c6a303`), and its two seeded failures were variants of the model, not mutants of the code. The chaos and sheriff tests in `crates/north-lander/tests/it` replay the lander's real incidents instead.

### In-flight lanes land into this, not beside it

| In flight (lane) | Where it lands |
|---|---|
| Train with land checks (landing-train-checks, 617e2eeeb) | bash, as written. Its cases stay per-lane, and `serve-train` covers the same tree-once rule |
| Interim verdict reuse (exact-interim, 97c052083) | no change. The lander makes `landing.exact` moot for north: it pushes only checked trees |
| B8+B9 per-case results (case-results) | stays library code; S6a's runner keeps calling `north land tests` |
| B10-B11 judge | B11 targets the lander's checks.rs once S2a has landed, and bash `check_candidate` before then |
| B12+B13 telemetry (landing-telemetry) | lands in bash first; S9 writes the same `landings` rows from serve.rs |
| B14 merge driver (merge-driver) | lands in bash first; candidate.rs sets the same `merge.set.driver` config in the lander's worktree |
| B16 lander credential (lander-credential) | S8: serve pushes with `gh-app-token --app lander` |
| Land-check speed on Actions | edits land_actions.rs, which stays for confirm runs; independent |
| Shared dependency builds (flake.nix -Cmetadata) | helps S6b's warm slots; independent |

B17 (Nexus unit) gives way to S3 (laptop user unit) under D4. B18 is S2a-S2c. B19 and B27 are S6a-S6b. B26 is
S5. B22's revert becomes a red-fix ticket (S10).

### Boxes

Every box: Category feature, Spec exact, Scope north, Surface rust, Verify local-test, unless stated.
Each is 20-40 minutes. Boxes in one wave run in parallel unless they say Follows. Test runs go through
machine-capacity (moderate class).

**Wave L0: lands today, laptop lanes first.** Value alone: workers on the lander host end at
`Done: queued`, and each candidate costs one check and 3-4 GitHub writes, instead of one check per
lane on every main move. Order: S2a first (about 30 min); S1 runs beside it and rebases onto it;
then S2b and S2c in parallel; then S3 and S4.

- **S2a Lander loop, green path.** Files crates/north-landing/src/land/{mod,queue,serve,candidate,
  checks}.rs, src/cli/tools.rs (`land` with `serve` and `enqueue` arms; `enqueue` starts as a stub
  S1 fills), bin/safe-push (`--check-candidate --base SHA` as described above, and `usage()` deleted),
  tests/it/land_serve.rs. `north land serve [--once]` reads the queue and builds one candidate for an
  origin: red-fix tickets first, then oldest, re-enqueued tickets replacing older ones. The candidate
  is a detached HEAD in the stable worktree `~/code/<repo>/worktrees/lander`. At each pick it
  runs `git cherry`, so a lane already on main counts as landed, and `north stale-revert` against the
  lane's head. A conflict ejects that lane and records its files. The candidate is checked with
  `safe-push --check-candidate`, in an environment with `GH_TOKEN` and the App key removed. On green
  it pushes by lease. If the lease fails it rebuilds. Done when: (1) `cargo test --test it land_serve
  serve` passes `serve-train`, `serve-red-fix-first`, `serve-reenqueue-replaces`, `serve-cas-lost`,
  `serve-pushes-checked-tree`, `serve-candidate-head-unchanged`, `serve-gate-refusal`,
  `serve-stale-revert`, `serve-already-on-main`, `serve-token-scoped` and `serve-github-writes`;
  (2) `bash bin/safe-push.test.sh` prints PASS, with a new case `check-candidate` showing a red-main
  failure ending `verdict red-main` without calling `land-red-wait`; (3) `git diff --numstat
  origin/main -- bin/safe-push` shows more lines deleted than added.
- **S1 Enqueue.** Beside S2a; owns land/enqueue.rs and tests/it/land_enqueue.rs only, and writes the
  ticket format above. bin/safe-push gets an `--enqueue` arm of at most 3 lines that probes and execs
  `north land enqueue`. Done when: (1) `cargo test --test it land_enqueue` passes `enqueue-returns`,
  `enqueue-squash` (a 3-commit lane becomes one commit before its ticket is written),
  `enqueue-red-fix-marker` (a `Main-red:` trailer sets `red_fix`) and `enqueue-replaces`;
  (2) `safe-push --enqueue` with a runtime lacking `land` exits 1 naming `north runtime update`;
  (3) the bash suite prints PASS.
- **S2b Bisect and verdict classes.** Follows S2a. Files land/{bisect,red}.rs. The lander halves
  only on `refused`. On `red-main` it holds the batch and lands red-fix tickets first; on `transient`
  and `deferred` it retries the same candidate; a candidate whose base moved during its check is
  rebuilt. It messages the culprit's session with the log or run URL, and (north#360, 2026-11-04)
  the gate refusal, safe-push `✗` and `exited N` lines and each failing test's name and panic line
  from that lane's own check log, in log order, once each, within 1500 characters. Done when: `serve-mixed`
  (already-landed, conflict naming `shared.txt`, failure receipt with the run URL, the good lanes
  landed, the lane worktrees' HEAD and status unchanged), `serve-captured-candidate`,
  `serve-transient-not-culprit`, `serve-defer`, `serve-dropped-commit` and `serve-context-only`
  pass.
- **S2c After the landing.** Follows S2a. Files land/{telemetry}.rs, land/after/follow.rs. It
  writes one pushes.tsv row per lane (the 7 columns `record_push` writes, so fleet landings see the
  session) and stages.tsv rows. It sends `north fleet send <session> "landed <branch> as <sha>"`
  only after `git cherry` shows the lane's commit on main. In the background, once per candidate, it
  runs `north after-landing`, follows-release and `north runtime follow`. Done when: (1)
  `serve-landed-only-on-main`, `serve-after-landing`, `serve-restart` (the lander killed mid-check and
  restarted: every lane lands once and nothing is pushed twice) and `serve-reentry` pass; (2) one real
  north lane lands through `north land serve --once` on the laptop, its sha an ancestor of
  origin/main (`git merge-base --is-ancestor`), and its author session receives the landed message.
- **S3 Lander service.** Follows S2c. Scope cross-repo (nixos-config: a firn `north-land` user unit;
  until it lands, the lead starts `systemd-run --user --unit north-land`); Surface laptop. It runs
  the runtime's `north land serve`, restarts on exit and between candidates after a runtime follow.
  Since 2026-11-05 it retires on a runtime follow only when `git diff` of the two runtime SHAs
  touches crates/north-landing/, crates/north-base/, Cargo.lock, Cargo.toml or rust-toolchain.toml,
  or cannot be read; a follow that changes only docs or other crates keeps it serving and its
  speculative stack intact (north#360).
  lane gc, lane reap and auto-reap skip `worktrees/lander*`. Done when: (1) `systemctl --user is-active
  north-land` prints active; (2) after `kill -9` of its pid mid-check, a new pid lands the same
  candidate once within 10 s; (3) `serve-worktree-not-reaped` passes.
- **S4 North switches to the lander on the laptop.** Follows S1, S3. Files bin/safe-push
  (`landing.queue = lander` and `landing.lander-host` read from fetched origin/main, so `--to main`
  enqueues on the lander host), .safe-push, bin/safe-push.test.sh (case `lander-queue`),
  harness/skills/workers/references/notes.md (Landing mechanics: a landing ends at `Done: queued`).
  Done when: (1) `lander-queue` shows `--to main` on such a fixture writing a ticket, pushing nothing
  and exiting 0 on the lander host, and landing per lane on another host; (2) the laptop's runtime
  answers `north land enqueue --help` and `git -C ~/code/north/main merge-base --is-ancestor <S4 sha>
  HEAD` exits 0; (3) 30 minutes after the switch, `rg -c` over the new stages.tsv rows finds no
  `land checks` row from a laptop worker lane, and pushes.tsv shows at least one landing by the
  lander.

**Wave L1: the lander's own speed and identity.** Parallel, after S2b.

- **S5 Speculation (B26).** Files land/speculate.rs. Depth 2 by default, in a second stable
  worktree (`worktrees/lander-2`). Done when: (1) `speculate-two-batches`: with a stub check of 5 s,
  two batches arriving 1 s apart both land in under 12 s; (2) `speculate-discard-on-red`: a red N
  throws away N+1's verdict, and N+1 is rebuilt without the culprit and checked again; (3)
  `speculate-real-gates`: N lands while N+1's real `--check-candidate --base N` run is in flight, and
  N+1 still lands without re-running; (4) every test asserts that the pushed tree equals a checked
  tree. `--depth D` generalises it: D-1 spare worktrees (`worktrees/lander-2..lander-D`, each
  on a detached HEAD), each speculative candidate built on the head of the one beneath it and
  checked beside it, pushed in order only while everything beneath landed at the head it was built
  on. A red speculative candidate stops the candidates stacked on it at once; their lanes stay queued
  and are rebuilt on the new main. `speculate-depth-three` lands three lanes arriving during a 5 s
  check, in three stacked candidates, within 1.2 check lengths of the first check's start;
  `speculate-depth-three-red-middle`, `speculate-depth-one-waits` and
  `speculate-depth-two-stacks-once` cover the red middle and depths 1 and 2.
- **S6a Rust land-check runner, in place.** Files land/slots/runner.rs, bin/safe-push. It ports
  `run_land_checks`: selection from `land.check`, the local-only reasons, logs, the deadline under
  `north capacity deadline`, frozen-time accounting, orphan reaping, the capacity wrapper and the
  publish ref. bash's function body becomes one `north land checks --base B --head H` call, for every
  caller, so per-lane landings use it too and bash shrinks by about 90 lines. Done when: runner tests
  ported under their case names pass: land-check-boundary, land-publish, dead-check-orphans,
  hung-check-timeout, frozen-check-timeout, lease-around-local-check,
  lease-released-for-actions-wait, admitted-check-unlocked, land-check-retry; the bash suite prints
  PASS; `rg -n '^run_land_checks\(\)' bin/safe-push` shows a body of at most 3 lines.
  Hang verdict (2026-11-05): a check that runs past its deadline is judged against the base tree,
  never against the lane alone. The runner records each check's wall time per tree, command and host
  class (`safe-push/check-timings`); a deadline exit refuses the lane, naming the command, only when
  the base tree finished the same check within the deadline, and is otherwise an infrastructure
  verdict (exit 75) that keeps the ticket. The deadline, local or on an offload slot, prints the
  command and its process tree before it signals, and `land tests` prints `land check: started T`
  as each test begins, so a silent hang names its step. The first such hang, a 3-lane candidate
  carrying inferred land checks, was `bin/safe-push.test.sh`'s train-deps fixture: its inferred
  `bun test` reached a test stub that execs the dev shell's capacity-gate `bun` shim, and `north
  capacity gate` picked that stub as the real `bun`, an exec loop in one process that never printed.
  The gate now skips every program it already exec'd in the same process.
  Measured deadline (2026-11-05): a check's deadline is three times the p90 of its last 20 passing
  walls on that host class, at least 300 s and at most the former flat 2100 s, and 2100 s until 5
  passes are recorded (`check_timings::Deadline`); a full check takes about 150 s on an idle host,
  so a hang now costs minutes before the hang verdict instead of 35. Each check's start line names
  its deadline and its source, and `SAFE_PUSH_LAND_CHECK_TIMEOUT` still overrides it. The runner
  passes `north capacity deadline` the enclosing lease only when `AGENT_CAPACITY_LEASE` names one
  and `-` otherwise; an empty lease name is a named error, because the old `leases/.json` watched
  no lease and never paused for a freeze. Under `-` the clock pauses while one of the command's
  own processes is queued for capacity or holds a frozen lease.
  Slot deadline (2026-11-06): a job on a build host's offload slot runs under `north capacity
  deadline --wall`, whose clock counts queued and frozen time, because the slot stays held while
  its gate waits. At 7:34 AM four code-north-main slots on gex45 had been held for 6,300 to 7,000
  s by jobs with 900 to 1,800 s deadlines, all behind one gate ticket that never left the queue.
  A wall deadline that expires with any wait in it ends exit 75, a no-verdict that requeues the
  after-landing batch instead of blaming a lane; one that expires with none is still the hang
  judged against the base tree. The deadline renders the process tree before it signals and
  writes it after the group is gone, waiting at most 2 s for stderr, so a pipe nobody drains holds
  neither the kill nor the exit.
  Verdict rows (2026-11-06): an after-landing run that covers several landings writes one
  `safe-push/post-land.tsv` row and one `verdict` span per covered head, naming the run's head
  in an eighth column; a run with no verdict writes neither and reruns. `north land profile`
  prints each landing's verdict and how many landings have one, none or more than one.
  Run deadline (2026-11-06): the whole `north land checks --after` run gets the same measured
  deadline as a land check, three times the p90 wall of its repository's last 20 runs that reached
  a verdict (rows of `safe-push/post-land.tsv` with no covering head), at least 300 s and at most
  2,100 s, and 2,100 s until 5 runs are recorded; a run that expired doubles the next one's, up to
  the ceiling, until a run reaches a verdict. It replaced a flat 1,800 s that let one stuck run hold
  every later landing's verdict for half an hour while runs took p90 172 s. `landing.after-deadline-sec`
  in `.safe-push` still overrides it.
  Superseded run (2026-11-06): when a landing's `.safe-push` drops or rewrites a `land.after` line
  that the after-landing run in flight is running, the lander cancels that run: its process group
  is killed and it ends with no verdict, so it writes no `post-land.tsv` row, sends no red-main
  message and starts no culprit search or sheriff action. Its landings merge into the newest
  landing's run at once, with no 30 s retry delay and no missed-verdict count, so the current lines
  check all of them on the newest head. A run cancelled during its culprit search keeps its red
  verdict and blames no lane. On 2026-11-06 a 50-minute rerun of a firn check line that be08ad55
  had replaced built gcc from source on ccx33 and held firn's verdict.
  Failure verdict (2026-11-05): a check that exits nonzero inside a lander round reruns once on
  the base tree, on the same host and through the same wrapper, with `SAFE_PUSH_LAND_HEAD` naming
  the tree it runs on and `SAFE_PUSH_LAND_RUNTIME_HEAD` still naming the lane, so `bin/head-north`
  (citation-check, check-owners) picks the installed runtime or a build from the tree on the base
  as it did on the lane, while a check that judges the lane's diff (`code-health layers BASE HEAD`)
  sees an empty range there. When both named the lane, every such check failed the same way on
  the base, and from 1:00 PM on 2026-11-05 the lander deferred those lanes every 30 s instead of
  refusing them; an offloaded base rerun also failed `git bundle` on its empty base..base range,
  so its bundle now stops at the base's parent. The base result is kept per (base
  tree, command, host class) for the round (`safe-push/check-timings/TREE/*.result`), so one broken
  check costs one base rerun. When the base also fails, the verdict is transient with detail
  `main's check environment is broken: CHECK on HOST: FIRST ERROR LINE`; the lander keeps every
  ticket, takes no strike, raises one `supply-alert`, holds new candidates in `land-queue/KEY/base-broken`
  and retries every 5 min, clearing the hold on the first candidate that passes or is refused. A check
  the lane adds is not on the base and is never rerun there. From 3:42 to 4:05 AM on 2026-11-05
  every candidate whose diff forced the checks off the installed runtime (Cargo.lock, src/cli, the
  catalog or citation_check) failed `nix: command not found`; main passed on the runtime path at
  3:33 AM, and the lander refused experiments-registry for that environment fault until the service
  was stopped.
- **S6b Warm slots, Actions confirms.** Follows S6a. Files land/slots/{select,offload}.rs. With
  `--slots` (passed by `--check-candidate`), portable commands run in the lander's worktree or on a
  gex45 offload slot through `north capacity offload run`, never on Actions; local-only commands stay
  on the laptop. Done when: (1) `serve-github-writes` shows 1 main push and 0 dispatches per
  candidate; (2) on north, a second candidate's `cargo build` in the same slot takes under 30% of the
  first's wall time; (3) main's Cargo and Harness runs still start on every landed sha (`gh run list
  --commit <sha>`).
  Built 2026-11-05 (north#360), overflow half: with land.runner and land.workflow both set, north land
  checks asks `north capacity offload free --host H` once per run how many of the build host's slots
  for this checkout are unlocked (flock -n on each, holding none). With one free every check runs on
  the build host and Actions gets no dispatch; with none free the portable checks overflow to Actions
  and machine-bound ones stay on the host; a slot wait that runs out mid-run (offload exit 96) sends
  that portable check to Actions instead of failing the lane. The Actions verdict is recorded under
  host class `actions`, so a check that ran in a slot no longer records as `actions`. A runner whose
  `choose` keeps the checks on this host probes no slots and sends nothing to Actions. North itself
  sets no land.workflow, so its checks never overflow; the rule serves repositories that set both.
  Pruning each slot's target stays a separate box. The note's `--slots` flag and slots/offload.rs
  no longer fit the code: north land checks reads land.runner directly, the build host was chosen by
  6beeefd52 (laptop as check host), and the placement rules are a small type in slots/select.rs.
  B28's self-hosted Actions runner on gex45 is not needed for this: gex45 takes checks over ssh
  offload, and Actions serves only overflow. tests/it/land_warm_overflow.rs.
- **S6c Prechecks placed per slot across hosts (2026-11-05).** Each north land check leased 6 CPUs
  under the laptop's 20-core cap, so `landing.slots = 5` ran only 3 prechecks at once and one 20-min
  precheck at 10:48 AM waited 1038 s for admission. When `land.runner = build-host`, the lander now
  picks each precheck's host before it builds the candidate, through `north capacity offload choose
  --any --taken HOST...`: every running precheck counts its 6 cores against the host it was sent to,
  the host with the most free cores wins, and `SAFE_PUSH_LAND_RUNNER` pins the checker there, so its
  portable checks ship to that host's warm offload worktree. When no host has room, no slot starts
  and the lander asks again after 15 s, so the hosts with room set how many prechecks run and
  `landing.slots` is only the ceiling on the laptop's candidate worktrees. Machine-bound checks still
  run on the laptop under its own lease. `remote-hosts` adds ccx33. tests/it/land_serve.rs
  `prechecks_spread_over_hosts`.
- **S6d Unfit check hosts are infrastructure (2026-11-05, north#360).** Lane B (816e9fb68) was refused
  at 2:21 PM after its clippy ran on cx53, whose cargo registry lacked ahash v0.8.12 offline, and
  n360-s19a was refused when shipping the lander runtime to cx53 failed with exit 255; promotion then
  refused a candidate because it could not reproduce that failure. The lander now reads the check log
  for three signatures (a crate missing offline, `north` missing on PATH, an offload transport
  failure), records them as the existing transient verdict marked `infrastructure: the check host is
  unfit:`, and rechecks the lane at once with `north capacity offload choose --any --avoid HOST`;
  promote skips a recorded failure whose log carries one. The persistent-transient escalation still
  bounds a lane that is unfit on every host. tests/it/land_serve/unfit_host.rs,
  crates/north-lander/tests/it/promote.rs.
- **S7 Removed.** The scans bridge is not needed: S6a ports the land checks in place, and each Wave
  L2 box ports its scan in place, so `--check-candidate` keeps working throughout.
- **S8 Lander App token.** Follows B16. Done when: a landing by the lander shows the App as its push
  actor (`gh api repos/tompassarelli/north/events` push event `actor.login` is the App's bot), and
  `serve-token-scoped` still passes, with the token present only in the push step's environment.
  Built 2026-11-04 (north#360): a repository names its App with `landing.push-app` in its committed
  `.safe-push`, read from the candidate's base, and lands with the `agent` App, the one fleet sessions push with, when it names none (north names `lander`, whose installation covers only the repositories it was added to); the lander mints with `gh-app-token --app lander` after the
  check and before it records the push, and hands the token to the push alone through
  `NORTH_LAND_PUSH_TOKEN` and a one-shot credential helper that replaces every inherited helper, with
  the ambient `GH_TOKEN` removed from that push. A mint failure is a transient wait that keeps the
  ticket and falls back to nothing; a repository without the key keeps its inherited credential.
  tests/it/land_serve.rs: serve_push_actor_app, serve_app_mint_failure_keeps_the_ticket,
  serve_unconfigured_repository_keeps_its_credential.
- **S9 Landing records (B12).** Follows B12. Done when: one fixture run of serve leaves one
  `landings` row per lane with tree, batch_size, bisect_depth, attempts and verdict, and `north ledger flow`
  prints queued-to-landed p50 and p90 from those rows.
  Built 2026-11-04 (north#360): the lander spawns `north land train record` per lane it lands, and per
  lane it refuses with a conflict or a failed check; rows carry first_commit_at, check_started_at and
  check_finished_at. `north land enqueue` stamps the ticket's first-commit second from the lane
  branch's reflog before the squash, because a rebase or amend resets commit dates, and `north ledger flow`
  prints first-commit-to-landed p50 and p90 with the before-queue, queued and check split.
- **S10 Sheriff through the queue (B22).** Follows S2b, B10. Files harness/scripts/main-red.ts,
  land/red.rs. Done when: (1) a fixture where main's post-push suite fails on case X enqueues a
  red-fix revert ticket of only the lane whose removal turns X green, lands it ahead of the queue
  and messages the lane's author; (2) a failure that persists without the lane enqueues nothing.
  Built 2026-11-04 (north#360): main-red.ts no longer pushes a revert; on a confirmed red after a
  green parent it pushes `claude/land-revert-<head>`, a commit on the failing head whose message holds
  the failing log lines, `Green-parent:` and `Main-red-run:`. Each lander round starts with
  `land/sheriff.rs`: per request it reads main, and if main moved past the failing commit it reverts
  nothing (the 86f9a63ec rule); otherwise it reruns the failing cases on main without each landed
  lane, newest first, skipping reverts and lanes it already reverted, and queues a `Main-red:` red-fix
  ticket for the first lane whose removal passes them, which sorts first and is checked like any
  lane, and messages that lane's session. A failure that persists without every lane is recorded as
  `environment` under the queue's `sheriff/` and nothing is queued. The request ref is deleted once
  answered. Since 2026-11-09 (north#979) every sheriff
  rerun of a step passes `north land checks --after --fresh`, which never reuses a verdict-cache pass: the
  firn `land-checks.sh dotfiles` reverts of 27b05b0f and c78620bd rested on cached passes of main while the
  test failed on main since north#523. A failure that persists without every lane is reported to the proxy
  as a main breakage.
- **S11 Remote enqueue, pulled by the lander.** Files land/{enqueue,lander,serve,queue}.rs. Pull, not
  push: writing tickets to the laptop over ssh would need a new inbound command on the laptop, while
  the laptop already reaches gex45 over ssh for build offloads (redesigned 2026-11-04). On a host
  that is not the committed `landing.lander-host`, `north land enqueue` writes the same ticket under
  the same queue key into `outbox/` and prints `Done: queued (outbox) <root> <branch> <sha>`. The
  lander watches the outbox of each ssh host of orchestration.toml's `[hosts]` and each host in
  `landing.remote-hosts` (space-separated) from its own thread, independent of rounds: an ssh with
  `ConnectTimeout=5` waits on the host up to 10 s for a ticket, polling every 200 ms, and lists it;
  the lander then fetches each lane from
  `ssh://<host><root>` into `refs/land-remote/<host>/<branch>`, copies the ticket into its own queue
  with its `host` field kept, and moves the remote ticket to `outbox/taken/`. Landed messages are
  unchanged. A verdict goes back over the same link: an inotify watch on the lander's decided areas
  writes each decided ticket from a build host into that host's `outbox/decided/` as it is decided, so the
  hand-in waiting there reads a refusal within a second, not on the records mirror's next pass or never
  while the mirror is held (north#1185, 2026-11-12: six conflict refusals at 21 concurrent workers reached
  no worker). The build host cannot reach the laptop, so a hand-in that has heard nothing for 60 s asks
  through `outbox/asks/`, which ends the lander's 10 s outbox wait; the lander answers with the decided
  ticket or the ticket's place in `outbox/answers/`, and an ask with no answer in 15 s is printed as the
  lander not reading the outbox. A hand-in reads a verdict only from a ticket with its own name, head and
  branch, and reports landed only once origin holds the landed commit. An unreachable host gets a waiting line and is tried again 5 s later. A pulled ticket wakes the
  serve loop, so a build host's ticket enters a round within about a second of being queued
  (2026-11-08: the round-start pull left one waiting 19 s). A ticket written into the lander's own
  queue wakes it the same way, through an inotify watch on the queue directory, instead of waiting out the 2 s
  round pause (north#1138, 2026-11-11: a one-line change waited 0.7 to 1.8 s of a 2.0 s hand-in for pickup). The
  enqueue records the ticket as queued before it writes the file, so the woken lander never meets a ticket whose
  record the enqueue is still writing. The
  outbox key is the hash of the origin URL, so a remote host's origin must be spelled as the
  lander's. No host gains an inbound verb or key. Done when: `cargo test --test it land_serve remote`
  passes remote-outbox-lands, remote-outbox-unreachable and remote-enqueue-off-lander-host. The live
  switch (`landing.remote-hosts = gex45` and gex45's safe-push enqueueing off the lander host)
  follows it.
- **S21 Lane cleanup by the lander.** Files land/after/cleanup.rs. Done when: the lane cases clean,
  pid-namespace, keep, dirty, remote-merged and remote-unmerged pass against the lander's
  after-landing cleanup, under the same names. After each landing the lander removes a ticket's lane
  worktree and branch only when the lane still sits at the queued head, every commit is on the landed
  main, the tree is clean and no process has its working directory inside it (read from the host
  /proc, so a process in a PID namespace counts); the ticket's `keep_lane` (safe-push `--keep-lane`)
  keeps it. A kept lane is reported with its reason. The lane's origin branch is deleted, under a
  lease, only when its commits are on main. Removal uses no `--force`, and the branch goes by
  `update-ref -d` with the queued head as the expected value.

**Wave L2: stages ported in place.** Parallel, after S2a. Each box ports one group of bash functions
to Rust, available as `north land scan <name>`. It replaces each function body in bin/safe-push with
that call, so `--check-candidate` and per-lane landings both use it, and ports the named cases to
`cargo test --test it land_` under the same names. Each Done when also requires the bash suite to print
PASS, and `git diff --numstat` on bin/safe-push to show more lines deleted than added.

- **S12 Language, build-cache and bin-reference gates** (`run_language_gate`,
  `run_bin_reference_gate`). Cases language-gate-boundary, language-<mode>, language-initial,
  train-language-<mode>, train-captured-candidate.
- **S13 Secrets, deny-list, email, native-ref** (`denylist_scan`, `commit_email_scan`,
  `native_ref_scan`, `publication_scan`, the betterleaks call). Cases denylist-public-finding,
  email-to-main, race-* scan half, native-ref, all-at-once.
- **S14 Dependency gate, tool removal, test integrity** (`tool_removal_scan`, `vendored_trees`,
  `test_integrity_scan`). Cases dep-gate, tool-removal (including its eval'd block), vendored-tree,
  mutant-record, test-integrity.
- **S15 Policy sweep, generated rewrite** (`policy_sweep_scan`, `generated_rewrite_scan`, as
  scans/policy.rs and scans/generated.rs). Cases policy-sweep, generated-rewrite. The review gate,
  durable paths and codex_behavior.py's caller were retired in 58df15b56 (2026-11-04), so S15 ports
  none of them.
- **S16 CI budget, code health, instruction limits as after-landing gates** (`ci_budget_*`,
  `code_health_gate`, `run_after_landing_gates`); `north after-landing` stops calling
  `safe-push --after-landing`. Cases ci-budget, ci-unconfigured, ci-filtered, code-health-landed.
- **S17 Per-lane candidate checks: stale revert, dropped commits, dependency sync**
  (`lane_commits_missing`, `refuse_stale_revert`, `sync_dependencies`). Cases rebase-drops-commit,
  relanded-lane-rewritten-main, lane-already-on-main, rebase-context-only,
  stale-revert-before-land-checks, fresh-north-worktree, the two sync_dependencies blocks,
  train-deps.
- **Incident test** (north#112). `north land scan test-integrity` also runs `incident-test`, so
  every lander and per-lane check gets it with no bash change. A lane whose commit subject cites a
  north issue labelled `bug` (`north#N`, or `Fixes #N` on north) and changes no test (a path
  the landing gates share as a test file, `scans/test_file.rs`: under tests/ or test/, or named `tests.*`,
  `*.test.*`, `*_test.*` or `*_tests.*`; or an added `#[test]`) is refused naming the issue. Test
  admission counts that lane's one regression test per cited bug as the incident's, not as growth of a
  repository over its verification budget, so a bug fix never has to delete an unrelated test to land
  (north#681).
  A lane that changes only Markdown fixes no behaviour and passes without a label read (north#649).
  A fix that cannot carry a test (a skill, policy or configuration change) passes with an
  `Incident-test: none; REASON` commit trailer on any commit of the lane, also without a label read; a
  trailer with no reason does not count (north#675).
  Labels are read with gh only when no test changed or a lane adds tests over budget, cached per lander round for an hour under
  the state dir; a GitHub that does not answer passes the lane with one warning line.
  Case incident-fix-needs-a-test pins the citation formats and gh's label listing as a file-or-wire-format contract.

**Wave L3: every repository through the lander, then deletion.**

- **S18 Every repository on the lander.** Follows S4, S11. Scope cross-repo. `north land serve --all`
  runs one lander thread per repository, each with its own worktrees, queue and strikes; the queue
  directories map to checkouts by their tickets (`land/repositories.rs`: the newest local ticket whose
  root is a checkout pushing to that queue's key), and a queue with tickets and no checkout is logged,
  never served from a guess. `bin/safe-push` treats a main with no `landing.queue` as `lander`; the
  lander host comes from the repository's `.safe-push` or the machine's git config
  (`landing.lander-host`, `landing.remote-hosts`), and `landing.queue = direct`, `autoland` or `lock`
  opts out. `north land enqueue` refuses a repository the lander cannot serve. Wisp and makgora move
  off Autoland and `lock` to the lander. Done when: one real landing per repository (south,
  nixos-config, wisp, makgora and one with no `.safe-push`) through the lander, each sha an ancestor
  of its origin/main.
  Check environment (2026-11-05): the first proof lanes were refused by the environment, not by
  their trees, one cause per class. The rule for every served repository:
  (1) the build host runs a repository's checks in its main checkout's slot, `code/PROJECT/main`,
  whatever directory the lander's host keeps that repository's git directory in (wisp and makgora
  sent `code/wisp/worktrees/extract-20261005` and `code/makgora/worktrees/test-loop`, absent on
  gex45, and the bundle check exited 97);
  (2) a check runs inside the repository's flake dev shell when its flake provides one (evaluated, 6a48fa74d), else on
  the host's PATH (south's flake has no dev shell, and wrapping its checks in `nix develop` failed);
  a repository whose checks need a toolchain the build hosts keep only in dev shells, such as cargo,
  declares a dev shell (padbridge);
  (3) a shimmed toolchain the host lacks is infrastructure: the cargo, bun and nix shims and
  `north capacity gate` exit 75, not 127, when no real program stands behind them, so the inferred
  `command -v cargo || exit 75` guard, which a shim satisfies, no longer turns a missing toolchain
  into a refusal, and a gate that finds no real program from inside a project whose flake declares a
  dev shell it is not in runs the command in that dev shell (safe-push's pre-queue clippy ran bare
  `cargo` on gex45 and got 127);
  (4) a check starts from a clean slot, so an inferred bun check runs `bun install --frozen-lockfile`
  first and then the package's own `test` script when it declares one (wisp's is its test runner;
  raw `bun test` failed 5 of its 484 tests on gex45), and an explicit bun check line installs too.
  (5) a queue is keyed by the repository's GitHub identity, not the spelling of its origin URL: a host
  cloning over ssh and one cloning over https keyed one repository twice, and gex45's wisp and
  makgora tickets (ssh, `git@github.com:OWNER/REPO.git`) waited untaken in its outbox for 30 minutes; every ssh
  spelling now hashes as `https://github.com/OWNER/REPO.git`, which leaves every https key unchanged.
- **S19a Publication path in Rust.** Follows S13. Files land/push.rs (non-default branches, scratch
  refs, the race revalidation and the lease push, on safe_push_plan.rs and safe_push_state.rs), with
  `north land push` taking every non-default-branch push. Done when: the publication cases
  (argument-boundary, normal, planning-outcomes, lease-boundary, race-branch, race-commit,
  race-destination, race-worktree, no-upstream, inherited-upstream, dry-run-destination,
  new-destination, non-fast-forward, symbolic-destination, farm-scratch) pass against `north land
  push` under their names in `cargo test --test it land_`.
  Built 2026-11-05: `north land push` takes every push but one to origin's default branch, and a
  bare `safe-push` on the default branch now lands it as `--to` that branch, so the bash tracking
  and same-name publication code went. It does not rebase: an origin destination that is not an
  ancestor of HEAD is refused, because a scratch ref is per lane or per sha and rebasing the lane onto
  an older scratch tip rewrote the lane with that tip's content. It takes no lock, so a farm scratch
  push never waits on a landing.
- **S19b bin/safe-push becomes an exec.** Follows S12-S18, S19a. The lander calls every stage
  in-process, and the change deletes `--check-candidate`, the lock, ticket queue, train, rebase and
  verdict loop, Autoland and the driver. bin/safe-push becomes `exec -a safe-push "$runtime" land "$@"`
  plus bin/north's `CODEX_HOME` unset. The name stays because guards (worker_wait, tripwire), skills,
  south and nixos-config units and the `~/.local/bin` links use it. Done when: (1) `rg -c ''
  bin/safe-push` prints 5 or less; (2) `explicit-main` (now an enqueue) and every lander test pass;
  (3) one real landing in north and one in south go through the exec'd path, each sha an ancestor of
  its origin/main.
  Superseded 2026-11-06 by safe-push-composition.md (north#621): its P2a-P2c do the deletion, and
  `safe-push` becomes a symlink to the north binary instead of an exec shim.
- **S20 Shell suite retired.** Follows S19b. Done when: `git ls-files bin/safe-push.test.sh` prints
  nothing; .language-allowlist.json has no safe-push entry; `cargo test --test it -- --list land_` and
  `cargo test --test it -- --list land_serve` name every test in the guarantee table; both pass.
  Superseded 2026-11-06 by safe-push-composition.md P12a-P12c.

Rejected, one line each:
- A faster per-lane `north land` with today's lock ported byte for byte: keeps 25 lanes dispatching
  checks on every main move, the burst that hit the rate limit.
- Porting the lock and ticket queue: the lander's queue and origin's CAS make them unnecessary, and a
  port would put two lock implementations on one host during the cut-over.
- Porting the bash driver first and the scans later through the bash bridge, as the first draft of
  this section planned: it lands nothing for hours. The lander over `--dry-run` delivers today.
- Lander on Nexus (B17): it needs the Nexus unit, credentials and warm targets first. The laptop
  already has warm worktrees and the runtime (D4).
- Every candidate check on Actions: one dispatch per candidate still meets the burst limit under
  speculation and bisection, and Actions has no warm target/.
- Workers polling for their verdict: property 1. The verdict arrives as a fleet message.

Adversarial review of this section (a Sonnet reviewer, 2026-11-04), and the one revision:

| # | Finding | Change |
|---|---|---|
| 1 | Remote workers would enqueue to a queue no lander reads | S4 switches the lander host only (`landing.lander-host`); S11 lifts the limit |
| 2 | Dry-run still dispatches Actions: 3 writes per candidate, not 1 | Costs stated in the check-mode subsection and in `serve-github-writes`; S6b removes them |
| 3 | Dry-run has no red-fix state, so a red Actions check blocks for 6 h in `land-red-wait` | `--check-candidate` never waits and reports `verdict red-main` (case `check-candidate`) |
| 4 | Exit 1 conflates culprits with moved base, transient failures and DEFER | Verdict classes; bisect only on `refused`; `serve-transient-not-culprit`, `serve-defer` |
| 5 | Dry-run skips the capacity wrapper, offload, publish, hook and attest | Check mode applies capacity and offload; the rest are listed and assigned |
| 6 | Stale-revert and dropped-commit checks are weak until S17 | Per-pick stale-revert and `git cherry` moved into S2a |
| 7 | Speculation breaks on the destination revalidate | `--base SHA` without revalidation; S5 follows S2b; `speculate-real-gates` |
| 8 | An older bin/safe-push ignores `landing.queue = lander` | Cut-over text corrected; S4 checks the checkout |
| 9 | A bash lane in flight fails rather than retries | Cut-over text corrected |
| 10 | Table missed vendored-tree, mutant-record, review-coverage, the hook, re-entry and worktree races | Rows and tests added |
| 11 | S2, S6 and S19 are over 40 min; S4 waits an hour | S2a-S2c, S6a-S6b, S19a-S19b; S4 checks at 30 min |
| 12 | S1 and S2 edit the same files | S2a owns the shared files; S1 owns enqueue.rs and its own test file |
| 13 | Check commands come from the lane, in an environment with the push token | Scrubbed check environment; `serve-token-scoped`; token only in the push step |
| 14 | Reapers could remove the lander worktree | S3 exempts `worktrees/lander*`; `serve-worktree-not-reaped` |

## Decisions

Recorded, not reopened:

- **D1 (Tom, 2026-11-04).** Tom approved a GitHub App lander identity and a main ruleset letting only
  that App and his admin role push north main. Written here as a dedicated lander App, separate from
  the North Fleet App, which is the approved shape.
- **D2 (lead, 2026-11-04).** The lander and machine-bound checks run on Nexus; B4 measures its 3 cores.
- **D3 (lead, 2026-11-04).** `landing.exact = true` in north's .safe-push (5c12c46f3) as Wave 0.
- **D4 (lead, 2026-11-04 8:00 AM).** The lander is `north land serve`, a user service on the laptop, and
  workers end their turn at `Done: queued`. It is light, so the gaming hold defers only local-only
  checks while heavy checks run on gex45's warm slots. A sleeping laptop pauses the queue, but no
  worker waits on it. This supersedes D2 for the lander host; Nexus stays a later option (B17).

Only Tom can act (his GitHub session):

- Create the "North Lander" App with the permissions of design step 1 and install it on every repository it serves;
  the agent opens the creation page with every field filled, and Tom clicks Create. Blocks B16.

Decisions that are Tom's (credentials, cost):

- **Tom decides on a larger Nexus only if B4 shows its 3 cores set p50 above 12 min after 16(d).**
  Recommendation: none until B4. Alternative: keep 3 cores and accept B4's p50.

## Review findings and resolutions

| # | Finding (adversarial review, 2026-11-04) | Change, with lines in this file |
|---|---|---|
| 1 | Bypass is per App; Tom's PATs on Nexus; no `workflows`; sheriff pushes with `GITHUB_TOKEN` | Dedicated lander App, contents and workflows on north only (L83-88, L204); unit that cannot read Tom's secrets (L89-93); B16 (L293), B23 PATs off Nexus (L329), revocation is Tom's (L382); sheriff pushes only `claude/land-revert-*` (L147-148, B22 L324); B6 probes workflows and deploy keys (L244) |
| 2 | Per-file judge masks cases; flakes; stale reds | Per-case judge, quarantine TTL and exit, flake never green, never retried (L109-121, L207); env key clears reds (L102-108); B8-B11 (L259-272) |
| 3 | Full suite not implemented; south and nixos-config not keyed | Cause 1 notes b4c690bc5 and land-checks.sh line 44 (L37-42); `--all` and the crates/ fix (B3 L229); revisions in the env key (L102-108); B20 stale-south case (L308) |
| 4 | 6 of 11 machine-bound entries missing, skipped silently; host-bound tests | Baseline row (L27); B2 fails on a listed missing file (L225); B5 clean-HOME table on Nexus (L238) |
| 5 | Sheriff reverts the whole range; quarantine; environment loops | Newest recorded lane, verified by main without it, quarantined cases ignored, loop breaker (L147-156); B22 (L324) |
| 6 | Union merge loses edits; structure.toml modify/modify; ordered hook.rs arrays | features.md split early, no union (L129-131, L206); allowlisted set driver with duplicate check, ordered arrays refused (L131-140); B7 (L249), B14 (L278), B15 (L283) |
| 7 | `Rebased-patch-id:` author-writable; context drift | Moot: the trailer gate was retired on 2026-11-04 (step 10) |
| 8 | 40 jobs account-wide with no free-slot API; per-SHA main CI doubles load; Nexus 3 cores; public push before scan | D from own runs, B = 16 (L122-126); main CI keeps cancel, lander status plus 6-hourly run (L157-159, L205); p50 is a target priced from sources, B4 replaces it (L189-193); scan before any public push (L98-101) |
| 9 | Waves 1-4 deliver nothing before Wave 5; weak oracle | Exact mode as delivered Wave 0 (L53-73); one standalone-value line per wave (L217, L255, L290, L321, L343); judge and telemetry inside today's safe-push (Wave 1, L253); B20 oracle with flake, same-file, stale-south and the 08cde981f base (L308) |

## Wave 0 results

### B5 Machine-bound tests under a clean HOME on Nexus (2026-11-04)

Measured on Nexus (3 cores) at origin/main 489bc7cbd, cloned into a fresh `/tmp/b5-*` directory, each
test run as `env -i HOME=$(mktemp -d) PATH=/run/current-system/sw/bin:/home/tom/.nix-profile/bin bash
<test>`. Pass 2 added only `PATH=<dir with north>:...`, `NORTH_PACE_BIN` and `AGENT_CAPACITY_NORTH`
pointing at Nexus's installed runtime (`~/.local/state/north/current/north`, built 2026-11-03, not
489bc7cbd). Pass 3 also set `XDG_RUNTIME_DIR=/run/user/1000`. Four of the ten listed entries exist;
the other six are B2's dead entries.

| Test | Clean HOME | + north binary | + XDG_RUNTIME_DIR | Host state it reads | Fixture that makes it host-free |
|---|---|---|---|---|---|
| bin/threads.test.sh | 1 (25 of 27 fail) | 0 (27 of 27, 40 s) | 0 | `$repo/target/debug/north` (absent without a build); python3, sqlite3, jq; `~/.local/state/threads` and `~/.config/threads/ssh-config` under its scratch HOME with a fake ssh | None beyond a north build at the tested SHA (`NORTH_PACE_BIN`); already host-free otherwise, so it can move to Actions after `cargo build` |
| harness/lib/routing.test.sh | 1 (`north` not on PATH) | 0 | 0 | `north fleet pace` from PATH or `NORTH_PACE_BIN`; python3 | Same: a north build at the tested SHA through `NORTH_PACE_BIN` |
| bin/run-bounded.test.sh | 1 (`systemctl --user`: no `XDG_RUNTIME_DIR`/`DBUS_SESSION_BUS_ADDRESS`) | 1 | 1 (capacity-wrapped case); 0 once `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus` is also set (all six cases) | Real user systemd manager and session bus (`agent-bounded.slice`, `systemd-run --user --scope`); `/run/user/1000/systemd` and `agent-capacity-v1/mode`; `$HOME/.local/state/north/current/north` via the capacity helper | Already HOME-free given the bus address and `AGENT_CAPACITY_NORTH`; host-free only inside a container or VM running `systemd --user` |
| harness/skills/machine-capacity/scripts/machine-capacity.test.sh | 1 (helper spawns `$HOME/.local/state/north/current/north`: ENOENT) | 1 (`matchLevel` `family`, expected `family-repo`, first recorded case) | 1 (same) | `$HOME/.local/state/north/current/north` (or `AGENT_CAPACITY_NORTH`); real `systemctl --user` slices `agent-capacity-test$$`; `/sys/fs/cgroup` of those slices; a 24-core fixture (`AGENT_CAPACITY_CORES=24`); bun | Also exit 1 under Tom's real env on Nexus with the installed runtime, so the remaining failure is runtime skew (or a red at this SHA), not HOME; host-free needs a north build at the SHA plus a fake cgroup root (`AGENT_CAPACITY_CGROUP_ROOT`, already used by the recorded cases) in place of the real-slice cases |

Reading: two of four (threads, routing) are host-free once the lander builds north at the tested SHA
and passes it by env; they belong in the portable set. run-bounded and machine-capacity need a real
user systemd manager and stay machine-bound on Nexus, run with the lander's own user bus.

Result for machine-capacity.test.sh (2026-11-05): at e49c75748 in Nexus's `~/code/north/main` under
Tom's normal environment the installed runtime no longer skews (the `matchLevel` case passes) and
the test failed in capacity-unseen-pressure's first probe with `DEFER_MEMORY_HEADROOM`: the probe
read Nexus's real `/proc/meminfo` (8 GiB total, 4.1 GiB available) against the test's 24-core host.
The test now writes a 96343 MiB `AGENT_CAPACITY_MEMINFO` fixture for every helper call. Past
that, the 1 s deadline case failed: outside a dev shell (`IN_NIX_SHELL` unset, as over plain ssh)
a lease enters the cwd project's flake dev shell (capacity/dev_shell.rs), and `nix develop` alone
outlasts 1 s; agents on the laptop run inside the dev shell, so they never saw it. The test now runs
from its scratch directory, outside any project, so its fixture commands start bare. The
`family`-for-`family-repo` failure recorded above only appears outside a `~/code/<repo>` path: the
estimator takes the signature's repo from a cwd matching `~/code/<repo>/` (capacity/host/estimate.rs),
so a `/tmp` clone has no repo and cannot match at `family-repo`. Lander slots live under
`code/PROJECT/main`, so the checks see the repo.

Past those two host reads, three live cases still asserted the model before bbf0a8d1d (2026-11-04),
which moved heavy and moderate leases into the idle-weighted `agent-batch.slice` without CPUQuota,
paced heavy starts by a 30 s settle window and made moderate a frozen class; that commit updated the
fixture decisions but not the live cases, so the test was red on main wherever it ran outside a dev
shell. The four-heavy-jobs case now checks the batch parent (`cpu.idle` 1, `cpu.max` max), the
8 GiB MemoryHigh and the fourth job queued inside the third's settle window and started when it
ends, under `AGENT_CAPACITY_SETTLE_SECONDS=3` (at 30 s, jobs 1 and 2 hit their 30 s timeouts
first). The moderate session checks `north capacity deadline ... 1800` in place of
RuntimeMaxSec=30min. The native-waiting gpu probe runs away, since present the Tom-present GPU-busy
gate (capacity/gpu_admission.rs) defers the gpu lease first. The release measurement checks that the
record matches what the scope used (meanCores = cpuSeconds / wallSeconds, peak at most the two loops);
idle-weighted, two busy loops got 0.54 cores on a loaded Nexus, so a floor on meanCores read host
load. The test's cleanup also stops the batch subslice. With these, `machine-capacity.test.sh` exits
0 on Nexus from a `~/code/north` checkout under plain ssh.

### B6 Ruleset bypass actors and north's deploy keys (2026-11-04)

Already measured by B6: a user-owned repository's ruleset accepts an Integration (GitHub App) and the
RepositoryRole admin as bypass actors; north had no rulesets (`gh api repos/tompassarelli/north/rulesets
--jq length` returns 0); the North Lander App holds
`workflows: write`, so it can push `.github/workflows` changes under the ruleset.

`gh api repos/tompassarelli/north/keys --jq '.[]|{title,read_only}'` returns nothing (raw response
`[]`): north has no deploy keys, so no deploy key can write main around the ruleset.

## Incident, 2026-11-04: landing storm (7:30-8:15 AM, laptop clock)

Measured:

- About 25 north lanes ran their own `safe-push` in exact mode with the train bypassed. Each pushed a
  land-check ref and dispatched its own GitHub Actions run, and every landing restarted every other
  lane's checks.
- From 7:55 AM GitHub's secondary limit refused every workflow dispatch with HTTP 403 ("API rate limit
  exceeded") while the core quota stood at 1935/5000, and answered ref pushes, and one push to main,
  with "fatal error in commit_refs".
- Workers each retried on their own 4-5 minute timers, which kept the limit tripped; 30 workers sat
  idle waiting on their own landings.
- The lead ended it by holding every north landing, landing the train fix with its checks on a warm
  build host (the train fix's `SAFE_PUSH_LAND_RUNNER`, no GitHub dispatch), then landing every ready lane as one train.
- Two in-process subagents wrote the same file (`msg.txt`) in the lead's scratchpad, and the train
  fix's landing commit picked up the docs plan's subject.
- The installed `safe-push` ran from a main checkout stuck at 472d13847, so fixes that had landed never
  reached the lanes.

Rules adopted, each in its owner:

- Workers never retry a landing GitHub refuses; they report it queued and the train or lead lands it
  (`harness/claude/agents/worker*.md`).
- Workers write landing messages and landing scratch files in their lane's git dir (same files).
- A lead that sees one infrastructure cause fail landings across lanes holds landings, moves checks
  off the failing runner and lands one train (`harness/skills/workers/references/run-and-land.md`,
  Landing).
- A landing-machinery fix counts as working only once the installed `safe-push` and the promoted
  runtime contain it (`harness/skills/workers/SKILL.md`, Reports).
- Tom chose land-first: fast checks gate landing, the full suite runs on main afterwards; auto-revert
  follows (2026-11-05). Main's CI runs the full suite; `.safe-push` `after =` lines run the checks CI
  lacks on the landed sha after the push
  (`crates/north-landing/src/land/post_land.rs`); a failure is recorded in
  `safe-push/post-land.tsv` and alarms the proxy.
- A Rust precheck covers only the crates a lane changes and the workspace crates that depend on them
  (2026-11-05): the clippy line's `${SAFE_PUSH_LAND_PACKAGES:---workspace}` becomes `-p` flags from `cargo metadata` and the
  lane diff (`crates/north-land-checks/src/land/slots/affected.rs`), or `--workspace` when the lane changes
  a workspace manifest, `Cargo.lock`, the toolchain, an embedded input, or a crate every member uses.
  A narrowed check also leases fewer CPUs, two per crate with a floor of four, through
  `SAFE_PUSH_LAND_CPUS`, so more prechecks are admitted at once under the build host's cap. Main's CI
  still runs the whole workspace after landing. The number caps what admission charges, not what the
  check may use (2026-11-07): its scope keeps the landing ceiling, so a check alone on a host builds and
  tests on its free cores; a 400% quota at the slot share held the after-landing build near 77 s and
  nextest at 4 test threads (north#634).

Urgent lanes (2026-11-05): a fix that unblocks the queue itself waited behind 45 lanes after the morning fan-out. `north land urgent TICKET --reason TEXT` marks a queued ticket so the lander orders it with red fixes, ahead of older tickets, in both the batch queue and the per-lane prechecks. The proxy or a lead sets it; the reason is kept on the ticket.

## Land first, test after (2026-11-06)

Tom decided on 2026-11-05 and again on 2026-11-06 that landing takes 10 to 20 seconds, a batched
train at most 30, and that tests, CI and ceremony that do not need to block the push must not block
it. North#621 and north#633 record the work. This section supersedes the per-lane precheck (step 5
above in its precheck form) and makes the sheriff of step 11 act on the lander's own after-landing
run instead of waiting for main-red.yml.

### Measured before (24 h, 501 landings, proxy reading 2026-11-06 3:45 AM)

- Queue to main p50 712 s, p90 4685 s. A lane's precheck alone p50 71 s, p90 274 s.
- At 3:15 AM 17 north lanes sat queued behind pre-land cargo test and clippy runs; with 4 prechecks
  running, every further lane waited minutes for a 6-core slot (`no check host has room ... needs 6`).
- The same tree was checked four times: per-lane precheck, batch candidate, after-landing tests,
  GitHub CI. The sheriff had made 0 reverts on 34 red runs, because a red result only reached it as
  a main-red.yml branch whose rerun probe is rarely able to name a case.

### What runs before the push

Only what is fast and cannot be undone once main moves. The scans and gates run on the lander host;
since 2026-11-07 north leaves `land.check-local` unset, so its `land.check` lines (clippy, nextest,
gate-shadow) run on a build host through `land.runner`, because on the laptop they and the promotion
replays held the load at 50 to 90 on 24 threads and drained the battery while plugged in (north#634):

- the clean pick of every queued lane onto main (one candidate for all of them),
- the outgoing scans (secrets, email, deny-list, language, native references), the stale-revert
  gate, the dropped-commit gate and the review and trailer gates,
- the `land.check` lines: clippy and nextest of the changed crates, the whole-tree habit tests, the
  coverage-selected tests, and the checks that finish in seconds: the verb-table sort, the tests/it
  module list, code-health layers, agent-config tree-policy, package, features-index, rationale-index,
  doctor nix, test-admission kills, citation, nix-entry, safe-push-verbs, experiments and run-time-data (north#634, north#810).
  e1d1313a5 moved all but clippy and nextest after landing for speed (north#719); an after-landing
  line can only turn main red, and on 2026-11-07 a 6464-byte workers skill (98d30d226) and a layers
  breach (086197b4b) did, so every one of them a lane's own change can fail moved back (north#714).
  Each head-north check runs as `head-north --no-build` on the lander
  when its live runtime covers the checker code, never a compile there. When it does not (the lane
  or a main the runtime predates changed the checker), the lander runs the same check without
  `--no-build` on a build host, which builds north from the lane's tree; head-north refuses a
  `--no-build` check on an uncovering runtime instead of passing it unrun, so no lane skips one and
  none needs an after-landing backstop. A check holds a shared lease on the runtime or tree-built
  north it runs, and runtime collection and head-north's cache pruning skip a leased one. An entry over
  its cap is refused before the push, not found by a red main.

There is no per-lane precheck, no numbered build slot and no `landing.slots`. Every queued lane joins
the next candidate; the push is the only wait after the scans.

### What runs after the push

Every other check is a `land.after` line and runs once per landed batch on main, on the build host
(`landing.runner`) under capacity admission: owners, the graph validation, test-admission run, the
portable and machine-bound changed-tests selections and the agent-machinery tests. The affected-tests pre-land selection of north#360 moved here
with them. Each landed commit still has every check run on it, the checks above before its push; none was deleted or weakened, and the
two coverage waivers are the `Removes-check:` trailers on commit 9fe95ca5a.

### Red after landing: bisect inside the batch, revert the culprit

`north land serve` keeps a watch on every landed batch (speculative stacks included). When the after
run of the batch head is red, the watcher runs the same after set on the prefix that ends at lane
k for a binary search over the batch (`land/blame.rs`): about log2(n) extra runs, none for a single
lane, none when a probe is inconclusive (then the proxy is alarmed and nothing is reverted). The
first lane whose prefix is red is reverted with `git revert` of exactly its commits, each named in
a `Reverts:` trailer, and the revert is queued as a red-fix ticket that sorts ahead of the queue. The
lane's session is told to fix it and queue it again with `Follows:`. A revert is never itself
reverted: a red run on a red-fix lane alarms the proxy instead. A revert that no longer applies
cleanly alarms the proxy with the range. The retired main-red.yml request path stays for CI runs the
lander did not see.

### Why this is safe

- Main can be red for the minutes between the push and the verdict; the old design bought a green
  main with minutes of queue wait for every worker. Red main is bounded by the after run plus the
  bisection probes, and the revert lands first in the queue, so lanes that depended on the culprit
  see a single revert, not a stuck queue.
- A lane that breaks only in combination with a later lane in the batch is blamed on the first lane
  whose prefix is red, which is the lane that introduced the break in landing order.
- A lander exits as soon as its runtime link changes, without waiting for its in-flight
  after-landing runs, and promotion only swaps the link and never restarts the units itself. A
  stopped lander records no verdict for a run it was in: systemd's SIGTERM ends the lander before
  its killed run can report. The next start finds each unsettled batch in `<queue>/following/` and
  reruns it, whether the stop was a runtime swap, a `systemctl stop`, a crash or the watchdog
  (north#639). Before serving, it reaps the remote offload jobs the previous lander left on the
  build hosts (`north capacity offload reap --owner`), so a rerun never holds a slot beside an
  orphaned run of the same commit (north#649).
- A lander restart no longer loses the after-landing run in flight. On 2026-11-06 twelve runtime
  restarts between 8:00 PM and 12:25 AM each killed the run in flight and the restarted lander
  reran it from scratch, which made the 717 s and 413 s waits. Where the unit sets
  `NORTH_LAND_RUNNER_SLICE` (agent.slice on whiterabbit), the runner starts in its own transient
  systemd scope in that slice with `RuntimeMaxSec` at its deadline, so it outlives the lander's
  cgroup. A shell wrapper writes its exit status to `<queue>/logs/post-land-<head>.status`, and the
  lander records its process group, start, deadline, covered heads and host in `.runner` beside it.
  A restarted lander adopts each runner still alive or ended with a status: it waits for the status
  with the original deadline, cancels by process group, and reruns only batches without one. The
  runner's offload jobs carry the owner `<owner>-after`, which the restart reaps only when no runner
  is adoptable. Culprit-search prefixes, sheriff probes and candidate checks still die with the
  lander and rerun: a candidate's lanes are still queued, so nothing lands unchecked, and an adopted
  red run starts its culprit search again from the adopted verdict (north#696). On a runtime change
  the lander exits at once rather than waiting for culprit searches or a check in flight to end: on
  2026-11-08 that wait kept a lander from taking any candidate for 11 minutes while 12 tickets
  waited, and on 2026-11-10 the lander announced the change at 3:28:26 AM and waited 287 s for an
  alone check before exiting. A push already on its way to main gets at most 30 s to finish; a
  check or build is abandoned and rerun by the restarted lander (north#948).
- An alone check (a lane with no pass for its head) no longer holds the round. From 4:15 to 4:20 PM
  on 2026-11-10 one lane's alone check held 22 queued lanes with nothing landing, and another
  lane's checks took 7.7, 11.2 and 8.0 minutes each, because the round ran the check inline. The
  lander now builds the lane's candidate in its own `lander-alone` worktree and starts the check
  there as a process the round polls; lanes with a pass form and land their batch meanwhile, and
  one alone check is in flight at a time. The check runs under the same wrapper as the
  after-landing runner (its own scope in `NORTH_LAND_RUNNER_SLICE`, exit status written beside its
  log) with the offload owner `<owner>-alone`, and the lander records it in
  `<queue>/logs/alone-flight.json`, so a restarted lander adopts it, alive or ended with a status,
  instead of rerunning it, and reaps that owner only when nothing is adoptable (north#902).
- An after-landing run that only waited out busy build-host slots (exit 96) reached no verdict: it
  is recorded as transient and blames no lane, and a real failure in another check still wins over
  it (north#649).
- An after-landing run whose command exits 126 or 127 (not executable, not found) reached no
  verdict: it is recorded as transient and blames no lane. On 2026-11-06 the lander's check
  command named the laptop's runtime by its absolute path under `north-data/runtimes/`, which
  ccx33 never has, so the offloaded run exited 127 and was reported as red main. The offload slot
  script now runs the build host's own north when the command's program is an absolute path to a
  `north` the host cannot execute, and leaves a path the host has as given (north#662).

### Measured after

Runtime 157baa951 (precheck removed, after run leased and offloaded) was promoted to the live lander
at 4:14 AM on 2026-11-06 with `north land promote --shadow-minutes 1`; the lander's journal lines
`candidate X carries N lanes ... checking it` and `(branch): landed X` give the time from the lander
picking up the queue to the push having reached origin/main (the landed line is written after the
push and the record write). Eleven consecutive real north landings, one lane each:

- 4:15:03 to 4:15:10 (7 s), 4:15:22 to 4:15:28 (6 s), 4:16:12 to 4:16:19 (7 s), 4:16:34 to 4:16:40 (6 s),
  4:17:15 to 4:17:21 (6 s), 4:18:19 to 4:18:25 (6 s), 4:18:42 to 4:18:50 (8 s), 4:19:41 to 4:19:49 (8 s),
  4:20:58 to 4:21:04 (6 s), 4:23:02 to 4:23:08 (6 s), 4:23:50 to 4:23:57 (7 s).

Before the change the same journal showed `checks alone on main@...` prechecks of 10 s to several
minutes plus minutes of `no check host has room` waits per lane.

Finding from the first hour: with one lane the culprit is the lane, but main was already red from
an earlier commit (the test cull deleted fixtures a feature line cited), so the lander reverted
fast-verify-4 and fast-verify-5 twice over innocently. The fix, landed as 8f51ede07, probes the base
once before blaming the first landed lane and blames nobody when the base is red too; the lander
runs it from 4:25 AM.

Batch of three lanes: one candidate carried three documentation lanes queued within seconds of each
other, picked at 4:25:54 and landed at 4:26:03 (9 s), against the 30 s target. A one-lane landing
with a speculative second candidate stacked behind it took 7 s (4:26:12 to 4:26:19), and the
stacked one landed at 4:26:22.

### Land first again, without probe rounds (2026-11-08)

- Every queued lane has passed its own checks before queueing: the gates, and compile and clippy of its changed crates, which safe-push's checks before queueing run on the lane against current main. The lander treats that as the lane's alone pass, recorded on the ticket as the lane head that passed (`alone_green`) and the main it passed on (`alone_base`). The pass lasts `landing.alone-pass-landings` landings (5 when unset): once main is more first-parent commits past `alone_base` than that, or no longer descends from it, or the lane has no pass for its head, the lane stays out of the batch and, after the round's push, is checked alone on the new main with the lander's checker, one lane per round; a green check records the new pass and the lane joins the next batch, a red one refuses it with its log. Sheriff reverts carry no pass and are never held for one (north#936).
- Each round cherry-picks every queued lane onto main as one candidate, runs only the pick-time gates and scans, and pushes it with a lease on main. A lane whose gates passed before queueing, with nothing uncommitted or untracked in its checkout, carries the main commit they were judged from on its ticket (`gated_from`); when the pick lands on that same commit and leaves the lane's tree unchanged, the lander reuses that verdict instead of judging the gates again in a scratch worktree (north#1138). Nothing is checked on a merged tree before the push, and no push waits for an earlier batch's after-landing run; the lease is the only serialization.
- The after-landing run now runs the `land.check` lines as well as the `land.after` lines on the landed tree, since they no longer run on any merged tree before the push. A red run bisects inside the batch and reverts the culprit as above.
- The probe planner (`north-land-probes`), the probe fan-out, the suspect groups and the unbuilt-lane pull were deleted with nothing left calling them. The coverage agreement record lost its `landed` lines, which named the probe's check log.

Replays in crates/north-lander/tests/it/land_first.rs: three queued lanes land in one push with the checker never called; a batch carrying a bad lane is reverted to exactly that lane; a batch queued while the previous batch's after-landing run is held is pushed before that run ends.
A lane queued six landings ago is checked alone again after the round that lands a fresh lane directly, and lands in the next round without another check.

## Lander fixes never wait behind the lander (2026-11-08, north#930)

From 2026-11-04 to 2026-11-08 every lander improvement waited in the queue it was meant to fix. Land-first
(q902-deadcode), queued at 11:37 AM on 2026-11-08, was still 8th of the urgent tickets at 12:57 PM, behind
three Main-red fixes and four other urgent lanes, each waiting for probe rounds of about 10 minutes. Land-first
itself removed the probe rounds: the lander now pushes one candidate of every queued lane, which already passed
its own land checks on a build host before queueing, and checks the merged tree after landing. Two ways remained
for the lander to hold back its own repair, and a lander-fix lane closes both.

A lane marks itself with a `Lander-fix: REASON` trailer. `north land enqueue` accepts the mark only on a lane
that changes nothing but lander and landing code (`lander_fix::PATHS` in north-lander: the `crates/north-land*`
crates, north-safe-push, north-check-environment, the offload slot code, `tests/it/land*`, the landing docs and
the features index), so jumping the queue can never reach a product change, and records it on the ticket as
`lander_fix`.

- Order. The queue orders lander fixes ahead of every other ticket, Main-red fixes included, so a lander fix is
  the first commit of the push and no lane queued ahead of it can conflict it out of the candidate.
- Repairs only. While the sheriff answers a revert request or the base-broken pause holds, the lander builds
  only repairs; `lander_fix::repairs` puts the queued lander fixes there beside the Main-red fixes.

Safety is unchanged: the same land checks before queueing, the same scans, privacy and gates, the same lease
push, and the same after-landing run and sheriff revert follow the fix.

Replay: `cargo nextest run -p north-lander -E 'test(the_lander_fix_queued_at_12_57_pm)'` queues the 12:57 PM
queue (seven urgent tickets ahead, three of them Main-red fixes, and twenty lanes around q902-deadcode; the
lanes the ledger snapshot names keep their branch names and queue times, the rest are named by role). It shows
the whole queue landing in one push with q902-deadcode as its first commit. On land-first's main without the
lander-fix order, the first commit is red-fix-a.

Rejected: a separate pass that checks each lander fix alone off the lander before the batch. It was built
against the probe rounds. Land-first already lands every queued lane in the next push, so a second pass would only
add a check the lane passed before queueing.

### Break-glass, when even a lander fix cannot wait (2026-11-08, north#930)

Tom asked on 2026-11-08 for a sanctioned path past the lander after land-first waited over an hour; the
alternative in use was editing or bypassing a guard by hand, which leaves no record and skips the scans.
`north land break-glass TICKET --reason TEXT --approval GATE` lands one queued ticket only under Tom's recorded
approval. It runs the outgoing secret scan and the privacy judge on exactly the commits it pushes, records an
incident naming the reason, and hands the landing to the serving lander so the after-landing run and the
sheriff's revert still follow it. No guard changes to allow it.

Replay: `cargo nextest run -p north-lander -E 'test(break_glass_lands)'` refuses the command without
approval and pushes nothing, then lands the ticket once approval is recorded, with the secret scan run and the
incident recorded.

## After-landing run narrowed by the alone passes (2026-11-09, north#917)

- Land-first left the after-landing run checking the batch range's whole graph selection, though every lane had already run its own selection before queueing (`refuse_queue_checks` runs the lane's tests/it land check on the graph's answer for the lane), and north dash read 103 s for "checks after landing, typical" (n=56, records mirrored to gex45 up to 2026-11-08 01:57 UTC) against its 60 s bar.
- The lander now passes the after-landing run, for every landed lane, its range on main (`--lane NAME=PREV..TIP`, the ranges blame already used) and, for each lane whose alone pass is current on the batch's base (`landing.alone-pass-landings`), the range that pass checked (`--ran NAME=ALONE_BASE..LANE_HEAD`). `north land checks --after` hands both to `north graph ask affected`, which leaves out a test when exactly one landed lane's cone holds it, that lane's pass range holds it, and the landings between the pass's main and the batch base cannot affect it; a test two lanes share, a test a lane with no pass (a sheriff revert) can affect, and every test of a batch with an untraced lane still run. The land check's selection line names how many tests were carried.
- Known gap: the pass's cone is recomputed from today's graph, so a test the graph linked to the lane only after its pass ran is carried though it never ran. The miss check below measures this with every other miss.
- Each after-landing run records its graph selection in its outcome file (`selected BASE HEAD JSON`), and `north land missed --since DATE` lists each failing Cargo run on main since DATE, finds the recorded range holding its commit, and names every failed test the selection left out, exiting 1 on any.

### An aged alone pass no longer holds its lane out of the batch (2026-11-10, north#902)

Measured from the north lander's journal for 2:14 to 4:11 PM (+07) on 2026-11-10 (`journalctl --user -u north-land@north.service --since -2h`), at about 43 landings an hour in batches of up to 9 lanes:

- The serving loop spent 44.8 of the 117 minutes inside seven alone checks, one lane at a time, landing nothing else meanwhile: 1.9, 9.4, 7.7, 11.2, 8.0, 3.0 and 3.6 minutes. Five of the seven were lanes whose pass had only aged (`passed alone on ..., more landings ago than the alone pass lasts`); two had no pass for their head. Two of the checks (11.2 and 3.0 minutes) were thrown away by a runtime restart and one (7.7) reached no verdict on a broken base, so n1039-blame alone held rounds for about 27 minutes across three checks.
- At 4:12 PM all 18 queued tickets had a pass for their head, and every one of them was 7 to 40 first-parent commits past it, against `landing.alone-pass-landings` of 5. The window counted lane commits while one batch lands up to 9 of them, so each large batch made the whole queue stale at once and the lanes drained through the one-per-round alone check at 3 to 11 minutes each, which is the 22 to 35 minute wait lanes saw.
- Over 48 hours the lander rechecked 7 lanes for an aged pass; 5 passed and 2 failed.
- Two runtime restarts in the window cost 2.1 and 0.8 minutes before the next candidate, besides the two checks they killed; 94 restarts over 48 hours. The served checkout `/home/tom/code/north/main` being dirty cost no round time (it is logged in the second of the push).

Decision: a lane with a pass for its current head joins the candidate whatever main has landed since; only a lane with no pass for its head (a moved head, or a judge with no verdict at enqueue) is checked alone first. The policy is that a lane enters the queue only after passing alone on a recent main, which safe-push's checks before queueing still enforce; the lander recheck added nothing the after-landing run lacks, because that run runs a lane's whole affected selection whenever its pass is older than `landing.alone-pass-landings`, which now governs only that test carrying, and a red run is blamed and the culprit reverted. Replay in crates/north-lander/tests/it/land_first.rs: a lane whose pass is six landings old lands in the same push as a fresh lane with the checker never called.

## The private-kind verdict is asked beside the round, not in it (2026-11-12, north#1199)

- Since north#1153 a lane may be queued before its private-kind verdict, and the lander holds its push until the verdict passes. The serve loop asked for that verdict itself: each round asked every unjudged ticket in turn, one judge call or one ssh call to the ticket's host each, and when no ticket was ready it waited up to 10 s for all of the verdicts it had asked for.
- At 21 workers at once (load ladder, 2026-11-12) the scratch lander admitted 12 of the 21 first tickets this way. One round asked for 12 verdicts in a row and built its next candidate 9 s later, and the next round took another 10 s. Every lane already ready waited behind those asks: the queue wait was a median 11 s and a p95 23 s. The finished after-landing run of the landing before was reaped only when such a round ended. That is the 5.7 s inside it that no span covers: the lander had not looked yet. Pushes never waited for the after-landing run itself.
- Now each unjudged ticket's verdict is asked on its own thread, once per lane head, waiting until the lander's patience for that lane runs out. The serve loop only takes in the answers that have arrived, and counts the asks in flight as work in flight, so it looks again every 250 ms. A ticket with no passing verdict is still never handed to a batch, a refusal is decided as before, and a lane whose patience runs out still goes to its alone check.
- Replay in crates/north-lander/tests/it/land_first.rs: three lanes queued together before their verdicts; the judge holds one of them; the other two land while its single ask is still open, and it lands once its judge passes it.

## After landing, a change the graph reaches no test from runs no test (2026-11-12, north#1247)

- On 2026-11-12 the two commits that changed only a standalone eval crate ran all 1795 tests after landing, although `north graph ask affected` traced them and named no test. An answer with no test fell back to the crate-level table, and any path outside the crate DAG (`evals/`) made that table run the whole suite. With about twenty workers the build hosts sat at twice their thread count, and two after-landing runs passed their deadline waiting for a slot.
- After landing only, a traced answer with no test, for a change with no Rust source, manifest, selection table (`tests/it/crates.txt`, `tests/it/tree_wide_units.txt`, `harness/guard-suites.toml`, `structure.toml`, `.safe-push`) or guard-suite path, now runs no test: the Rust test check passes without a slot, its log names the reason and the outcome file's `selected` record carries it as `no_test`. A Rust change the graph reaches no test from (a crate in its own workspace) runs only the tree-wide modules and units, since those scans list every tracked Rust file and the read sets predate the file. An untraced change still runs the whole suite, and the checks before the push are unchanged.
- Shadow over ten landed commits of the day (posted on north#1247): no failure the whole suite found fell outside the selection; the eval-only Rust commit goes from 1795 tests (1205 test-seconds) to 11 (34). Documents-only commits still run about 825 tests, the 745 the graph knows with no read set: read sets were recorded once, on 2026-11-08, and the daily coverage record's recorder output is not ingested as read facts. That gap is closed by n1247 (see docs/rationale/test-selection.md, n1247): the record's reads reach the graph through the refresh.
- Backstop: GitHub Actions' Cargo workflow runs the whole suite in three shards on every push to main that touches a Rust input, off the landing path, at no cost on a public repository and no build-host time; the sheriff confirms and reverts from it as before, and `north land missed` judges the recorded selections against it.
