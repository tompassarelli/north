# Landing train: main never red, finished work never waits on someone else's break

Tom asked on 2026-11-04 for the most ambitious correct redesign of north's landing, with maximum
robustness and the fastest landing the design allows. This file holds the baseline measured that day,
the design, the boxes that build it in waves that each land value alone, and what is Tom's. Revised the
same day after an adversarial review (table at the end). Built so far: `landing.exact = true` in north's
.safe-push (lead commit 5c12c46f3, lane landing-exact), which is Wave 0's delivered state.

## Baseline, 2026-11-04 (laptop clock, +07)

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
   `crates/north-core/src/threads/*`, `crates/north-core/src/ledger/*` and Cargo files (line 44), so a
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
| Already-landed lane counts as landed; red-fix lanes first | delivered |
| Train candidate checked by the land checks, so a repository with land checks batches instead of every lane re-running its own (fixed 2026-11-04: land checks had sent such repositories around the train) | delivered |
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
into the code safe-push's exact train already calls (land-checks.sh, `north land-tests`, git merge
drivers), so it pays off before the lander exists; the lander is safe-push's train loop ported to Rust
and run as a service.

1. **Identity.** A dedicated GitHub App, "north lander", separate from the Socrates Agent App that
   bin/gh-app-token mints gex45's tokens from (ruleset bypass is per App, so a shared App would let
   gex45 bypass too). Installed on north only, permissions `contents: write`, `workflows: write`,
   `metadata: read`, `actions: write` (dispatch land-check.yml), `statuses: write`. `workflows` lets it
   land `.github/workflows` lanes, which safe-push's autoland path refuses (`touches_workflows`,
   bin/safe-push ~1982). Its key lives only in the lander unit's credentials directory.
2. **Host.** Nexus (lead's call, recorded below). The lander runs as a systemd unit with
   `DynamicUser=yes`, `InaccessiblePaths=/home/tom /run/secrets` and the lander key by
   `LoadCredential`, the pattern of the gex45 plan's south-land unit, so nothing else on Nexus (Tom's
   PATs, the Socrates App key) is reachable from it and it reaches nothing of Tom's. Its own clones of
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
    `GITHUB_TOKEN`); it pushes `claude/land-revert-<sha>` naming the failing run. The sheriff, in the
    lander: new failing cases on main = failing cases minus quarantined cases minus the parent landing's
    failing cases. It picks the newest lander-recorded lane whose landing record has those cases passing
    on its parent, and builds main minus that lane's commit. If the failing cases pass there, it lands
    that revert first in the queue and re-queues the lane to its author with `Follows:`. If they still
    fail, the lane is not the cause: no revert; the cases are quarantined as `environment`, and their
    env inputs are diffed against the last green record. Loop breaker: at most one revert per lane per
    red episode, never a revert of a revert (kept from main-red.ts), and after two failed verifications
    the sheriff stops and escalates on the issue.
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
    safe-push's exact train and later by the lander; `north flow` prints finished-to-landed p50/p90,
    re-lands per lane, red-main minutes and bisect depth.
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
| Every main commit's tree was a tested candidate | 100% | `north land-train audit --since 1d`: main commits with no `landings` row whose tree equals the commit's tree: 0 |
| Only the lander and Tom's admin role advance main | GitHub rule | `gh api repos/tompassarelli/north/rulesets` shows one main ruleset with bypass actors the lander App and the admin role; a push with a Socrates App token is refused |
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
  - Laptop as lander: it sleeps and the gaming hold defers heavy work.
  - Shared Socrates App as lander: ruleset bypass is per App, so gex45 tokens would bypass.
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
  crates/north-landing/src/landing/land_test_runner.rs, its tests. Done when: `north land-tests --cases
  json` on fixtures prints one row per libtest case, per `PASS|FAIL` line, and one whole-file row for a
  test that prints neither, with names and outcomes matching the fixture.
- **B9 Results store.** Category feature; files crates/north-landing/src/landing/ (new results module).
  Key (tree, command, env) as design step 5. Done when: unit tests show a hit for the same key, a miss
  after the south revision changes, and a case recorded both ways on one key reported flaky.
- **B10 Judge.** Follows B8, B9. Category feature; Spec exact. Done when: unit tests show: base red
  case A plus lane red case B in the same file names only B new; a base-red case enters quarantine and
  exits on a passing later base tree; a flaky case's pass does not count; a quarantine older than 24 h
  holds a lane that selects its file.
- **B11 Judge in the exact train.** Follows B10. Files harness/scripts/land-checks.sh, cargo-tests.sh,
  bin/safe-push (`check_candidate`). Done when: bin/safe-push.test.sh lands a lane in exact mode over a
  base with a planted red case and refuses a lane that adds a red case in the same test file.
- **B12 Ledger tables and train records.** Category feature; files crates/north-core/src/threads
  (schema), bin/safe-push (`train_verdict` calls `north land-train record`). Done when: a safe-push
  exact-mode test run leaves one `landings` row per lane with tree, batch_size, attempts and verdict.
- **B13 Telemetry in north flow.** Follows B12. Files src/cli (flow). Done when: `north flow` on a fixture ledger prints
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
  cases from bin/safe-push.test.sh pass against `north land-train serve --once` in a local bare repo.
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
- **B23 Tom's PATs off the lander's reach.** Category infra; Scope cross-repo (south). Done when: on
  Nexus each use of nexus-work and nexus-apply is replaced by a Socrates App token, no sops
  secret Nexus decrypts holds either, and the units that used them pass their own checks.
- **B24 Main ruleset.** Follows B6, B16, B21, B22. Category infra; Surface GitHub settings. Done when: `gh api repos/tompassarelli/north/rulesets`
  shows the main ruleset with bypass actors the lander App and the admin role, a push to main with a
  Socrates App token and one from a workflow `GITHUB_TOKEN` are both refused, and the lander lands the
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

## Decisions

Recorded, not reopened:

- **D1 (Tom, 2026-11-04).** Tom approved a GitHub App lander identity and a main ruleset letting only
  that App and his admin role push north main. Written here as a dedicated lander App, separate from
  the Socrates Agent App, which is the approved shape.
- **D2 (lead, 2026-11-04).** The lander and machine-bound checks run on Nexus; B4 measures its 3 cores.
- **D3 (lead, 2026-11-04).** `landing.exact = true` in north's .safe-push (5c12c46f3) as Wave 0.

Only Tom can act (his GitHub session):

- Create the "north lander" App with the permissions of design step 1 and install it on north only;
  the agent opens the creation page with every field filled, and Tom clicks Create. Blocks B16.

Decisions that are Tom's (credentials, cost):

- **Tom decides whether to revoke nexus-work and nexus-apply once B23 shows nothing on Nexus uses
  them.** Recommendation: revoke, because a token acting as Tom carries his admin role's ruleset bypass
  wherever it sits, and after B23 nothing needs them; the cost is his clicks in GitHub settings.
  Alternative: keep them as a fallback; nothing breaks, and two long-lived tokens acting as Tom stay on
  an always-on host.
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
| harness/lib/routing.test.sh | 1 (`north` not on PATH) | 0 | 0 | `north pace` from PATH or `NORTH_PACE_BIN`; python3 | Same: a north build at the tested SHA through `NORTH_PACE_BIN` |
| bin/run-bounded.test.sh | 1 (`systemctl --user`: no `XDG_RUNTIME_DIR`/`DBUS_SESSION_BUS_ADDRESS`) | 1 | 1 (capacity-wrapped case); 0 once `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus` is also set (all six cases) | Real user systemd manager and session bus (`agent-bounded.slice`, `systemd-run --user --scope`); `/run/user/1000/systemd` and `agent-capacity-v1/mode`; `$HOME/.local/state/north/current/north` via the capacity helper | Already HOME-free given the bus address and `AGENT_CAPACITY_NORTH`; host-free only inside a container or VM running `systemd --user` |
| harness/skills/machine-capacity/scripts/machine-capacity.test.sh | 1 (helper spawns `$HOME/.local/state/north/current/north`: ENOENT) | 1 (`matchLevel` `family`, expected `family-repo`, first recorded case) | 1 (same) | `$HOME/.local/state/north/current/north` (or `AGENT_CAPACITY_NORTH`); real `systemctl --user` slices `agent-capacity-test$$`; `/sys/fs/cgroup` of those slices; a 24-core fixture (`AGENT_CAPACITY_CORES=24`); bun | Also exit 1 under Tom's real env on Nexus with the installed runtime, so the remaining failure is runtime skew (or a red at this SHA), not HOME; host-free needs a north build at the SHA plus a fake cgroup root (`AGENT_CAPACITY_CGROUP_ROOT`, already used by the recorded cases) in place of the real-slice cases |

Reading: two of four (threads, routing) are host-free once the lander builds north at the tested SHA
and passes it by env; they belong in the portable set. run-bounded and machine-capacity need a real
user systemd manager and stay machine-bound on Nexus, run with the lander's own user bus.

### B6 Ruleset bypass actors and north's deploy keys (2026-11-04)

Already measured by B6: a user-owned repository's ruleset accepts an Integration (GitHub App) and the
RepositoryRole admin as bypass actors; north had no rulesets (`gh api repos/tompassarelli/north/rulesets
--jq length` returns 0); the north lander App (id 5603348, installation 178367738) holds
`workflows: write`, so it can push `.github/workflows` changes under the ruleset.

`gh api repos/tompassarelli/north/keys --jq '.[]|{title,read_only}'` returns nothing (raw response
`[]`): north has no deploy keys, so no deploy key can write main around the ruleset.

## Incident, 2026-11-04: landing storm (7:30-8:15 AM, laptop clock, +07)

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
