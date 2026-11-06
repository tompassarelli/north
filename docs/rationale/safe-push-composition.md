# Safe-push composition: one Rust crate, one concern per module, no bash

Tom asked on 2026-11-06 that safe-push be composed of separate files with separated
concerns, and then that it become pure Rust with no bash at all: `safe-push` on PATH is a symlink to the
north binary, which picks the safe-push entry point from argv[0], and each concern keeps its decisions in
pure functions over typed inputs, with git, GitHub, file and process effects in a thin typed shell. This
note is the plan for that work. It settles the module tree, each module's interface, the port order and
the boxes. It implements nothing. Owner: north#621.

## What safe-push does today (observed 2026-11-06 at origin/main bc3aa3bde, revised at dc20aeb3f)

- `bin/safe-push` is 1,727 lines of bash (the shell limit is 80; `.language-allowlist.json` exempts it,
  pending the port that north#76 named). `bin/safe-push.test.sh` is 2,583 lines and 54 `PASS:` scenarios.
  Eleven files in `tests/it` also run `CARGO_MANIFEST_DIR/bin/safe-push`.
- Most of its behaviour is already Rust, reached by `exec` or a subprocess: argument grammar
  (`landing/safe_push_plan.rs`), every push to a non-default branch (`north land push`, `land/push.rs`),
  `--restore` (`north land restore`, north#618), `--tag` (`north safe-push`, `landing/safe_push_tag.rs`),
  ledger-client south lands (`src/cli/remote_land.rs`), `--enqueue` and the lander route
  (`north land enqueue`, `north land route`), every scan (`north land scan ...` on
  `landing/safe_push_guards.rs`), land checks (`north land checks`), the receipt (`north safe-push-result`).
- What only bash does:
  1. The **per-lane landing** of origin's default branch: the landing lock and ticket queue, the landing
     train, rebase onto the moved destination with the keep-verdict rule, the pre-push hook loop,
     Autoland, the final-scan and land-check interleave under the lock, the push, and the aftermath
     (lane cleanup, auto-reap, follows-release, after-landing gates). About 1,100 lines.
  2. The **dry-run landing**, `safe-push --dry-run --to main`: every gate and the land checks, no push.
     `north health` runs it every 30 minutes (`crates/north-core/src/health.rs`, the
     `north-health-probe-*` lanes in `~/.local/state/safe-push/events.jsonl`).
  3. The **candidate verdict**, `safe-push --check-candidate --base SHA`: the lander's checker
     (`land/serve.rs` `checker: safe-push`), classes green, refused, transient, deferred, red-main.
  4. **Process plumbing**: argv0 mode dispatch, output-log claiming, stage timing (`stages.tsv`),
     events (`events.jsonl`, plus `north watch --once` on a refusal), the terminal receipt, signal
     cancellation and child processes, and runtime-skew probes (`north X --help` before each call).

Why it is that way: safe-push predates the Rust tree and was ported mode by mode, each mode moving behind
an `exec` when its Rust half landed (landing-train.md S19a for branch publication; the tag, restore and
south-route modes each the same way). The lander (landing-train.md, north#360) replaced the per-lane path
for every repository whose `.safe-push` reaches it, and S18 (65021e000, 2026-11-05 4:34 AM) made the
lander the default for every repository.

Measured on the two hosts that land:

- whiterabbit, the lander host: `events.jsonl` has no per-lane default-branch landing since 11:16 AM on
  2026-11-05. There is no `landing lock`, `autoland` or `push` stage for a real lane, and the only events
  from the per-lane code are the health probe's dry runs.
- gex45: the last per-lane landing was 12:01 PM on 2026-11-05 (makgora, lp-habits-makgora). There have
  been none in the 12 hours since, while its events kept flowing.

No checked origin main (north, south, wisp, makgora, firn) sets `landing.queue` to `lock`, `autoland` or
`direct`. Only north sets `landing.exact`, so the train is unreachable: north's committed
`landing.lander-host` sends each north lane to the lander or refuses it. Other repositories reach the
lander only because both hosts' `~/.config/git/config` names `landing.lander-host = whiterabbit` and
`landing.remote-hosts = gex45` (`land/route.rs` `decide`). On a host or sandbox `HOME` without that config,
a repository with no committed host still lands per lane today. That includes the test fixtures, Nexus
(which placement already forbids landing from) and any new machine. `--direct` was last used on
2026-10-22 (`direct-landings.tsv`).

## Decisions

1. **Delete the per-lane landing; do not port it.** landing-train.md already decided this as S19b
   ("deleted (not ported) in S19b, after the last repository moves (S18)"), and S18 has moved them. After
   the deletion, `north land route` has two answers for a default-branch landing that is not a dry run:
   the lander (local or remote enqueue), or a refusal that names the setting to add
   (`landing.remote-hosts`). `landing.queue = lock | autoland | direct` and `--direct` go, along with the
   train, the lock and the Autoland path. This plan replaces S19b's last step (bin/safe-push as a
   ten-line exec) with the argv[0] link. The deletion is boxes P2a-P2e, in north#360's S19b slot. The
   refusal reaches exactly the hosts that lack the machine config above. It names the two lines to add,
   and `enqueue.rs`'s "set landing.queue = direct" advice changes to match.
2. **A new crate, `crates/north-safe-push`**, that depends on `north-landing` and `north-base` and that
   only the root `north` crate depends on. north-landing has 23,181 non-test lines against a 25,000 cap
   (`north code-health`, 2026-11-06), so the port cannot grow it. Moving the safe-push modes out of
   north-landing also shrinks it. A safe-push edit then rebuilds only this crate and the root crate
   (crate-split.md).
3. **The gates stay in north-landing.** The lander's candidate build, enqueue and shadow checks call
   the same scans, stale-revert, lane squash and red-check code (`land/candidate.rs`, `land/enqueue.rs`,
   `land/sheriff.rs`), so safe-push calls them in process through their public entries. It no longer
   runs `north land scan ...` subprocesses. That removes runtime skew by construction: safe-push and
   its gates are one binary.
4. **Land checks stay a child process** (`current_exe land checks ... --outcome FILE`). They run for
   minutes, own their process group, are killed on cancel, and already talk through an outcome file.
   Running them in process would need cancellation threaded through north-landing's runner for no gain.
   Every other gate runs in process.
5. **argv[0] dispatch in the root `src/main.rs`**, from a pure `north_base::platform::entry_points`.
   Its `ENTRY_POINTS` list (today `["safe-push"]`) is the only list of names: main dispatches on it, the
   runtime installer links it, and tool-removal accepts it. `north safe-push ARGS` stays an exact alias.
   Two rules come with it:
   - The safe-push entry does what `bin/north` does before it runs: `unset CODEX_HOME`. The link
     bypasses that launcher, and landing-train S19b required the unset.
   - No caller runs safe-push by its canonical path, because canonicalizing the link yields `north`
     and loses argv[0]. Today `land/slots/runner.rs` runs `Command::new(<canonical safe-push>)
     --publication-scan`, fed by `land/after/follow.rs` `executable` and `after/mod.rs`'s `--safe-push`.
     Each of those calls becomes `entry_points::command(SafePush)`, which is `current_exe()` with
     `.arg0("safe-push")`, or, for a program found on PATH, the PATH name uncanonicalized. P13 switches
     them, before P14 makes the canonical path `north`.
6. **The runtime installer creates the link.** `north-runtime` `install()` (`runtime/update.rs`), which
   every built or pulled runtime passes through, writes `tree/bin/<name> -> ../../north` for each
   `ENTRY_POINTS` name, idempotently, and only when the exported tree has no tracked file at that path.
   The existing chain then resolves unchanged: `~/.local/bin/safe-push` (local-bin-links.sh) to
   `~/.local/share/north/bin/safe-push` (home-manager, out-of-store) to
   `~/.local/state/north/current/tree/bin/safe-push` to `runtimes/<sha>/north`. The kernel passes the
   caller's argv[0] through every symlink. On 2026-11-06, a binary reached through two hops was named
   `safe-push` when run from PATH and by the link's path when run by absolute path, and its
   `/proc/PID/cmdline` contained `safe-push` both times, which `north watch`'s liveness match reads. The
   after-landing gates find north as the canonical safe-push's directory joined with `north`
   (`land/after/gates.rs` `tools`). After the swap, that is `runtimes/<sha>/north` itself. nixos-config
   needs no change. local-bin-links.sh links symlinks (`[ -x "$src" ]` follows them) and drops a
   managed link only when it dangles, which the ordering in P14 prevents.
7. **One push function.** `shell::origin` is the only code that runs `git push`, for every publication,
   tag push, tag delete and restore. It sets `SAFE_PUSH_ACTIVE=1` (the tripwire's marker) and
   `SAFE_PUSH_OUTPUT_CLAIMED`, which lets a nested safe-push in a pre-push hook skip the log lock
   (`land/push.rs:166`, `safe_push_tag.rs:159`). It never clears the environment and never sets a
   `credential.*` option. So north#619's per-session credential helper (`GIT_CONFIG_COUNT`, the Socrates
   Agent app) applies to every safe-push write without safe-push knowing about it. One consequence is
   unchanged by the port: GitHub refuses a non-`v*` tag created from an agent session (no App bypass on
   that ruleset, `agent-push-identity.md`; observed in b081e327c). `--tag` therefore succeeds only from
   Tom's own login, while `--delete-tag` (the ruleset governs creation) and `north release` (the App has
   the `v*` bypass) work under both identities.
8. **One release-tag predicate.** `tags::name::release_tag` (today `starts_with('v')`, from north#360's
   release verb). `--tag` refuses it, `--delete-tag` refuses it, and `north release` requires it. The
   in-flight `--delete-tag` (north#620) duplicates the rule inline, and P5 folds it into the predicate.
9. **Every port box is a covered harness change.** south `agents/roles.toml` puts `north:bin/` under the
   `harness-editor` role, with tiers Opus medium, high and xhigh and a review that is fresh, independent,
   natively verified and created after the reviewed commit. The code these boxes move out of
   `bin/safe-push` is the same harness code wherever it lives, so P4 adds `north:crates/north-safe-push/`
   to the role's paths in south, and the role follows the code into the crate. Each box therefore names
   its review in a line of its own, and the rule is stated once above the boxes. It lands with no
   `Reviewed:` trailer. north retired that landing gate on 2026-11-04 (58df15b56, review-triggers.md),
   and nothing reads a trailer now.

Rejected, one line each:
- Port the per-lane path to Rust: about 1,100 bash lines rebuilt for a path with no landing in a day,
  which landing-train.md rejected already ("the lander's queue and origin's CAS make them unnecessary").
- A committed `bin/safe-push -> ../../north` symlink: correct inside a runtime tree, dangling in every
  checkout, and tests could not use it.
- A 10-line bash shim (`exec -a safe-push`): superseded by Tom's 2026-11-06 direction.
- Keep safe-push inside north-landing: no room under its crate cap, and it mixes the lander's gates
  with one client's command line.

## Module tree

`crates/north-safe-push/src/`. Each module states its concern, its interface and its `may_use` edges. A
module's decisions are pure functions over typed facts. Effects reach it only as `&dyn` shell traits
passed in, so tests are properties over the pure functions plus recorded scenarios against
`shell::recorded`, with real-git end-to-end tests at the argv[0] boundary only.

- `lib.rs`: `pub mod` declarations only. structure.toml's `crate-roots` owns every `crates/*/src/lib.rs`,
  so logic or `pub use` there would land outside the safe-push modules' edges.
- `entry.rs`: the entry. `pub fn run(arguments: &[String], shell: &Shell, host: &HostFacts) -> u8`
  parses the request, picks the mode and calls the mode's module. The argv[0] dispatch in
  `src/main.rs` calls `north_safe_push::entry::run`, and the `north release` verb calls
  `north_safe_push::tags::release::run`.
- `request.rs` (+ `request/usage.txt`): argv to `Request`. Pure, with no effects at all.
  `parse(&[String]) -> Result<Request, Refusal>`. The variants are `Help`, `Abandon(args)`,
  `Enqueue(args)`, `Restore { commit, branch, dry_run }`, `PublicationScan { base, head }`,
  `CheckCandidate { base }`, `Tag { name, dry_run }`, `DeleteTag { name, dry_run }` and
  `Push { to, dry_run, keep_lane, fold }`, where `fold` is `Lane | Message(path) | SeveralCommits(reason)`.
  `Refusal` is `Usage | Destructive`, with exit codes 2 and 1 as today. It uses nothing.
- `mode.rs`: `Request` plus `RepoFacts { current_branch, default_branch, origin_name, ledger_client }`
  to `Mode`: `Publish { to }`, `Preflight { to }` (a dry run to the default branch), `Land` (to the
  default branch), `SouthLedger`, or one of the direct modes. A bare push on the default branch is
  `Land`. Pure. It uses `request`.
- `shell/`: the thin typed effect layer. It is the only code here that runs processes or touches files.
  - `mod.rs`: `Shell { git, origin, github, files, processes, settings }` and `Shell::live()`.
  - `git.rs`: the `Git` trait. Local reads (rev-parse, symbolic-ref, `config --blob`, diff names,
    cherry, rev-list counts, worktree root and git dirs) and local writes (none after P2, except tag
    creation for release).
  - `origin.rs`: the `Origin` trait. `snapshot(ref)` (ls-remote), `fetch(ref)`, `push(&Publication)`,
    `delete(ref, lease)`. A pure `push_command(&Publication) -> CommandSpec` keeps decision 7 testable.
  - `github.rs`: the `GitHub` trait. Repository visibility, release create and view.
  - `files.rs`: the `StateFiles` trait. Appends to the state directory (`events.jsonl`, `stages.tsv`,
    `pushes.tsv`) and flock on an output log.
  - `processes.rs`: the `Processes` trait. Run a child with the outcome-file protocol and kill it on
    cancel, spawn detached (`setsid`; the watch nudge), and `current_exe`.
  - `settings.rs`: `Settings::from_env()`, read once at entry: `XDG_STATE_HOME`, `HOME`,
    `SAFE_PUSH_SOUTH`, `SAFE_PUSH_DENYLIST`, `SAFE_PUSH_OUTPUT_CLAIMED`, `SAFE_PUSH_LAND_OFFLOAD`,
    `NORTH_FLEET_SESSION`, `HOSTNAME`. No other module reads the environment.
  - `recorded.rs` (`#[cfg(test)]`): a scripted double of every trait. It answers from a scenario and
    records each call.
  - It uses only north-base `platform`.
- `report/`: what safe-push tells people and ledgers. `event.rs` holds `event_line(&Event) -> String`,
  pure and byte-identical to bash's `safe_push_event_line`, and `emit` (append, then the watch nudge on
  refused or failed). `stage.rs` holds the stage line and the `stages.tsv` row. `terminal.rs` holds
  `Terminal = Published(publication) | Queued(line) | Clean(dry_run_line) | Refused(reason) | Cancelled` with
  `receipt(&Terminal) -> (String, u8)`. It is moved from `src/cli/safe_push_result.rs`. It uses `shell`.
- `session/`: process-level guarantees. `output_log.rs` holds `claim(&Shell) -> Result<Claims, Refused>`
  (one safe-push per log file). `cancel.rs` installs the INT, TERM and HUP handler, kills the running
  child and exits 130. `identity.rs` holds `Identity` (root, git dir, origin URL, branch ref, head) and
  a pure `changed(&Identity, &Identity) -> Option<StateChange>`, the "repository state changed during
  scan" rule. It uses `shell` and `report`.
- `gates/`: which pre-publication gates apply, running them, and the one collected verdict.
  - `plan.rs`: `plan(&GateFacts) -> Vec<Gate>` from land checks present, destination, dry run,
    betterleaks on PATH and origin visibility. Pure.
  - `verdict.rs`: `verdict(&[(Gate, Outcome)]) -> Result<(), Refusal>` gives the "N blocking check(s)
    failed: a; b" line. `reuse(&Scanned, &Now) -> bool` reuses the background scans only when head,
    destination and range are unchanged. Pure.
  - `run.rs`: runs the in-process scans through north-landing's public entries (language, native-ref,
    secrets, deny-list, commit-email, dep-gate, tool-removal, test-integrity, policy-sweep,
    stale-revert, dropped-commits), and the land checks as a child, with the capacity command from
    `.safe-push` `landing.capacity` and `landing.offloaded-capacity`. Final scans run on a scoped
    thread beside the land checks.
  - It uses `shell`, `report` and `session`, plus the north-landing scans and `land checks`.
- `publication/`: pushing HEAD to a non-default branch (today's `north land push`). `plan.rs` holds the
  pure snapshot parse, outgoing range, lease and fast-forward rule (from `safe_push_plan.rs`).
  `run.rs` captures identity, runs the gates, revalidates and pushes through `Origin`. It uses
  `request`, `gates`, `session`, `report` and `shell`.
- `landing/`: the default branch.
  - `route.rs`: `decide(&LanderSettings, host, &Request) -> Route { Lander, SouthLedger, Refused }`.
    Pure, moved from `land/route.rs`; `LanderSettings::read` stays in north-landing.
  - `south_ledger.rs`: the pure mbox build and limits, plus sending to the ledger host's
    `south land`. Moved from `src/cli/remote_land.rs`; `MAX_COMMITS` and `MAX_PATCH_BYTES` move down
    to north-base, shared with north-ops' server half.
  - `preflight.rs`: the dry run. The same gates as a landing, then "✓ clean (--dry-run) — would push".
  - `candidate.rs`: `--check-candidate`. A pure
    `classify(&CandidateOutcome) -> Verdict { Green, Refused, Transient, Deferred, RedMain }`, then
    the "safe-push: verdict CLASS DETAIL" line the lander parses.
  - Enqueue is an in-process call to north-landing `land::enqueue::enqueue`.
  - It uses `request`, `gates`, `publication` (the "would push" text), `session`, `report` and `shell`.
- `tags/`: `name.rs` (`release_tag`, ref-format check; pure), `publish.rs` (`--tag`), `delete.rs`
  (`--delete-tag`, north#620, with a pure `custody(&TagFacts) -> Option<Custody>`), `release.rs`
  (`north release`). It uses `request`, `gates`, `session`, `report` and `shell`.
- `restore.rs`: `--restore` (north#618). A pure `source(&RestoreFacts) -> Result<Source, Refusal>`
  (the default branch or the lane-gc mirror; never over an existing branch), then `Origin::push`. It uses
  `request`, `session`, `report` and `shell`.

The edges form a DAG: `shell` at the bottom; then `request` and `report`; then `session` and `mode`;
then `gates`; then `publication`, `tags` and `restore`; then `landing`; then `entry`.

structure.toml records this tree now, ahead of the code, directly under the existing `landing` module. It
has one `[[module]]` per concern, named `landing.safe_push.<concern>` (`shell`, `request`, `report`,
`session`, `mode`, `gates`, `publication`, `tags`, `restore`, `landing`, `entry`), each with the
`may_use` edges above plus `foundation`. `shell` may also use north-base's `platform`. `gates`, `landing`
and `entry` may use the existing `landing` module, which holds north-landing's scans, enqueue and land
checks. `cli` may use it for the argv[0] dispatch and `north release`, and `tests` may use every one of
them. A `crate_dag` entry `north-safe-push` has `may_use = ["north-landing", "north-base"]`. Every box
that adds a file keeps it inside its module's paths, and `north code-health layers origin/main HEAD` is
the check.

What moves inside north-landing, so its scans have names of their own: `safe_push_state.rs`'s generic
`Refused`, `Step` and `git_read` become `landing/step.rs`. `safe_push_guards.rs` becomes
`land/scans/outgoing/` (`language.rs`, `native_ref.rs`, `secrets.rs`, `email.rs`, `denylist.rs`), beside
the `land/scans/outgoing.rs` that already wraps it. `stale_revert.rs`, `lane_squash.rs`, `red_check.rs`,
`fails_first.rs`, `after_landing.rs`, `bin_references.rs` and `land_actions.rs` stay where they are:
the lander and the after-landing gates own them, and safe-push only calls them.

## Port order and boxes

Every box keeps `bash bin/safe-push.test.sh` (while it exists) and `cargo test` green on its own. Each
box lands alone through safe-push. Scenario numbers are the `PASS:` lines of `bin/safe-push.test.sh` in
file order. A Done when holds checks a worker runs before landing. "After landing" lines are observations
the box's owner records once the landing reaches the host's runtime. They are not landing gates.

The review rule, which applies to every box (decision 9; workers skill § Review):
- The author is a `harness-editor` tier, which on the Claude side means the Opus `worker`.
- Before safe-push, a fresh reviewer of a different tier (`worker-sonnet`) reads the box's final diff at
  its sha and runs its Done when itself. The reviewer must be a session created after that commit.
- Each code finding carries the command it ran and the output it got. The lead reruns the command, and
  every finding that reproduces is fixed in the lane before landing.
- The box's report names the reviewer session and its `Accepted flaws: N`.

Kept scenarios that land today with a non-dry `--to main` and no lander config: 1, 2, 5, 8-11, 13, 17,
21-23 and 31-38. The per-lane deletion would turn every one of them red, so P2a converts them first, while
the per-lane code still exists.

- **P1 argv[0] entry (the early falsifying check).** Covered harness change: review per the rule above.
  Files: `crates/north-base/src/platform/entry_points.rs`, `src/main.rs`, `tests/it/safe_push_entry.rs`.
  Main dispatches a `safe-push` argv[0] to today's `north safe-push` function, after `unset CODEX_HOME`.
  Done when:
  1. A test links a temp `safe-push` to `CARGO_BIN_EXE_north` through two symlink hops, and
     `safe-push --help` exits 0 printing the usage that `north safe-push --help` prints.
  2. The same test shows `CODEX_HOME` unset in a child the entry runs.
  3. A property holds: any argv[0] whose basename is not in `ENTRY_POINTS` runs north.
- **P2a Tests stop depending on per-lane landing.** Covered harness change: review per the rule above.
  Test-only. Files: `bin/safe-push.test.sh`, `bin/follows-release.test.sh`, and the `tests/it` cases that
  run `bin/safe-push --to main` per lane (`land_offload.rs:111-120`, and the per-lane cases in
  `land_actions.rs`, `land_publication.rs`, `safe_push_land_branch.rs` and `land_route.rs:253-262`).
  - Each kept scenario above switches to `--dry-run --to main`, `--check-candidate` or a branch
    publication, together with its assertion. Scenario 2's "publishes" becomes branch publication.
    Scenario 17 asserts the enqueue receipt. Scenario 22's `--direct` half is deleted.
  - Each `tests/it` per-lane case either becomes a lander-config enqueue case or is deleted, with the
    test named in the commit.
  - Done when: (1) the bash suite and `cargo test --test it land_ safe_push` pass against today's bash;
    (2) `rg -n -- "--to main" bin/safe-push.test.sh bin/follows-release.test.sh` lists only
    `--dry-run`, `--check-candidate` and the scenarios P2d deletes.
- **P2b Route refuses instead of landing per lane.** Covered harness change: review per the rule above.
  Files: `crates/north-landing/src/land/route.rs` and its tests, `land/enqueue.rs` (its "set
  landing.queue = direct" message), `harness/loops.toml` (`route.rs::PER_LANE`). `Route::PerLane` goes. A
  repository or host the lander cannot take is refused, naming `landing.lander-host` and
  `landing.remote-hosts`. A `landing.queue` other than `lander` is refused, naming the lander. Done when:
  (1) `cargo test -p north-landing route` passes, with cases for an uncommitted repository on an unnamed
  host and for `landing.queue = lock`; (2) the bash suite passes.
- **P2c Delete the lock, ticket queue and train.** Covered harness change: review per the rule above.
  Files: `bin/safe-push`, `bin/safe-push.test.sh`, `crates/north-landing/src/watch/facts.rs` (it reads
  the tickets). Delete scenarios 24-30 and 44-48. Done when: (1)
  `rg -c 'acquire_push_lock|run_train|collect_train|take_train_result' bin/safe-push` prints nothing; (2)
  the bash suite and `cargo test --test it watch` pass.
- **P2d Delete Autoland, sync and the verdict loop, the hook loop and the aftermath.** Covered harness
  change: review per the rule above. Files: `bin/safe-push`, `bin/safe-push.test.sh`. Delete the `--to`
  landing block, `--direct`, the NOT-landed terminal check, and scenarios 3, 6, 14-16, 18-20, 39-43 and
  50-53. Name each in the commit body. Done when: (1) this prints nothing:
  `rg -c 'land_through_autoland|keep_verdict_through_moves|rebase_onto_destination|cleanup_landed_lane|auto_reap|release_follows' bin/safe-push`;
  (2) the bash suite and `cargo test --test it serve` pass.
- **P2e Delete the verbs only that bash called.** Covered harness change: review per the rule above. For
  each of `autoland-wait`, `land-red-wait`, `land-clean` and `land-train`, find its callers with `rg`
  over crates/, src/, tests/, bin/ and harness/. Delete only a verb that no Rust caller uses
  (`land-train` has threads and tempo callers, so it stays). Done when: `cargo test --workspace` passes,
  and the commit body lists each verb with its caller count.
- **P3 Name north-landing's shared pieces.** Covered harness change: review per the rule above.
  Mechanical `git mv`. `safe_push_state.rs` splits into `landing/step.rs` (`Refused`, `Step`, `git_read`,
  `git_quiet`, `git_status`) and the rest, which stays in place until P4. `safe_push_guards.rs` becomes
  `land/scans/outgoing/{language,native_ref,secrets,email,denylist}.rs`. Done when:
  `cargo test --workspace --lib` passes and `north code-health layers origin/main HEAD` is clean.
- **P4 Crate, request, publication, route.** Covered harness change: review per the rule above. Create
  `crates/north-safe-push`: Cargo.toml, lib.rs, entry.rs and the `tests/it/crates.txt` lines. Every file
  sits inside the `landing.safe_push.*` paths that structure.toml already records.
  - In south, add `north:crates/north-safe-push/` to `agents/roles.toml` `harness-editor` paths, landed
    through south safe-push alongside this box (decision 9).
  - `git mv` `safe_push_plan.rs` to `request.rs` and `publication/plan.rs`, `land/push.rs` to
    `publication/run.rs`, `land/route.rs`'s `decide` to `landing/route.rs`, and the rest of
    `safe_push_state.rs` to `session/identity.rs` and `session/cancel.rs`.
  - Add `crates/north-safe-push/` to `citation_check/config_keys.rs` `SAFE_PUSH_READERS` and to
    `land_model/features.rs` `LANDER_PATHS`.
  - Point `north land push` and `north land route` in `src/cli/tools.rs` at the crate.
  - Changed on 2026-11-06 while landing this box: `land/restore.rs`, `safe_push_tag.rs`, `safe_push_tag_delete.rs` and `release.rs` move here too, as `restore.rs`, `tags/publish.rs`, `tags/delete.rs` and `tags/release.rs`, because they use the identity, plan and cancel code that moves, and leaving them in north-landing would make that crate depend on the crate that depends on it. The cancel flag, `cancelled`, `refuse` and `request_cancel` go to `landing/step.rs` in north-landing, which the scans there also use, and `session/cancel.rs` only installs the signal handler. `north land push`, `route` and `restore` are dispatched from `src/cli/tools.rs`, and `land/commands.rs` no longer lists them. All of `land/route.rs` moves, not only `decide`: `Settings::read` and `run` use the moved arguments type, and the lander-side reads they call are now `pub` in north-landing. The modules that now call north-landing's scans and step helpers (session, publication, tags, restore) gain `landing` in their `may_use` in structure.toml until P6, P7 and P9 route those calls through `shell` and `gates`; `default_branch` and the ls-remote snapshot parse sit in `shell/git.rs` and `shell/origin.rs` for the same reason, and `claim_output_log` is `session/output_log.rs`.
  - Done when: (1) `cargo test --workspace` passes; (2) code-health layers is clean; (3)
    `git -C ~/code/south/main show origin/main:agents/roles.toml | rg -c north:crates/north-safe-push/`
    prints 1; (4) `north code-health` shows north-landing's non-test line count lower than before.
- **P5 Tags, restore, south ledger, receipt.** Covered harness change: review per the rule above.
  The tag and restore files already moved in P4. Remaining:
  - `tags/name.rs` holds the one `release_tag` predicate, used by `tags/publish.rs` and `tags/release.rs`.
  - `src/cli/remote_land.rs` to `landing/south_ledger.rs`. Its limits move to north-base, and root
    passes `ledger_client` in as a `HostFacts` field.
  - `src/cli/safe_push_result.rs` to `report/terminal.rs`.
  - Done when: (1) `cargo test --test it safe_push_tag release safe_push_remote_land` and `cargo test -p
    north-safe-push` pass; (2) `rg -n "starts_with\('v'\)" crates/north-safe-push/src` prints only
    `tags/name.rs`.
- **P6 The typed shell and publication on it.** Covered harness change: review per the rule above. Files:
  `shell/*`, `publication/run.rs`. Done when:
  1. `rg -n 'Command::new' crates/north-safe-push/src/publication` prints nothing.
  2. The property `push_command_env` passes: the only variables set are `SAFE_PUSH_ACTIVE` and
     `SAFE_PUSH_OUTPUT_CLAIMED`, with no `env_clear`, no `env_remove` and no `-c credential.*`.
  3. `cargo test --test it land_publication` passes.
- **P7 Restore and tags on the shell.** Covered harness change: review per the rule above. Files:
  `restore.rs`, `tags/*`. The decisions (`source`, `custody`, `release_tag`) are pure, with properties.
  Done when: (1) `rg -n 'Command::new' crates/north-safe-push/src/{restore.rs,tags}` prints nothing; (2)
  the tag, release and restore tests pass.
- **P8 Report and session.** Covered harness change: review per the rule above. Files: `report/*`,
  `session/output_log.rs`, `session/cancel.rs`. `Terminal` has no `Landed` or `TimedOut` variant, since
  only the deleted paths produced them. Done when: (1) a golden test shows `event_line` equal to a line
  that bash's `safe_push_event_line` printed for the same fields (the fixture is committed); (2) scenario
  35, which now means SIGINT during a dry-run's land check exits 130 and leaves no land-check process
  alive, passes against the argv[0] link.
- **P9a Gates: plan, verdict, in-process scans.** Covered harness change: review per the rule above.
  Files: `gates/plan.rs`, `gates/verdict.rs`, `gates/run.rs`, and `--publication-scan` in `entry.rs`.
  bash's `--publication-scan` execs `north safe-push`. Done when: (1) the properties for `plan` and
  `verdict` pass; (2) scenarios 9 and 12 pass in the bash suite.
- **P9b Gates: land checks child, background scans, capacity.** Covered harness change: review per the
  rule above. Files: `gates/run.rs`, `shell/processes.rs`. Scenarios 36-38 now mean that a dead, hung or
  frozen land check ends the dry run refused within its deadline and leaves no child behind (the lock
  half went in P2c). Done when: (1) the `reuse` property passes; (2) scenarios 10, 11 and 36-38 pass in
  the bash suite.
- **P10 Preflight and candidate in Rust.** Covered harness change: review per the rule above. Files:
  `landing/preflight.rs`, `landing/candidate.rs`. bash execs `north safe-push` for
  `--dry-run --to DEFAULT` and for `--check-candidate`. Done when: (1) scenarios 1, 2, 7, 8, 13, 21 and
  49 pass; (2) `cargo test --test it serve` passes. After landing: `north health` on whiterabbit reports
  the land probe passing.
- **P11 Rust takes every mode.** Covered harness change: review per the rule above. Files: `entry.rs`,
  `bin/safe-push`. `entry::run` dispatches every `Request`, with abandon and enqueue as in-process calls.
  bin/safe-push becomes `exec north safe-push "$@"` for the time between this box and P14. Done when: (1)
  `rg -c '' bin/safe-push` prints 2; (2) the bash suite and `cargo test --test it` pass. After landing:
  one real landing each in north and south goes through it, each sha an ancestor of its origin/main.
- **P12a-c Scenarios to cargo.** Covered harness change: review per the rule above. Files:
  `tests/it/safe_push/` (`publication.rs`, `gates.rs`, `preflight.rs`, `candidate.rs`, `session.rs`) and
  a `safe_push()` helper that links `CARGO_BIN_EXE_north` as `safe-push` in a temp dir.
  - Each box moves about eight of the kept scenarios (1, 2, 4, 5, 7-13, 17, 21-23, 31-38, 49) and
    deletes them from the bash suite.
  - The runtime-skew halves of 4, 5 and 54 are deleted, not ported: safe-push and north are one binary.
  - P12c retires the suite. It points the `harness-tests` canary (`harness/canaries/canaries.conf`
    and `harness-tests.patch`) at another shell suite and drops the allowlist entry.
  - Done when (P12c): (1) `git ls-files bin/safe-push.test.sh` prints nothing; (2) `cargo test --test it
    -- --list safe_push` names each kept scenario.
- **P13 The installer links entry points, and no caller canonicalizes safe-push.** Covered harness
  change: review per the rule above. Files: `crates/north-runtime/src/runtime/update.rs` (`install`),
  `land/scans/tool_removal.rs`, `land/slots/runner.rs`, `land/after/{follow,gates,mod}.rs`. Done when:
  1. A runtime test installs a tree with no `bin/safe-push` and finds `tree/bin/safe-push` resolving to
     the runtime's `north`, and a second install changes nothing.
  2. A tool-removal test passes the removal of `bin/safe-push` and still refuses the removal of
     `bin/agents`.
  3. A runner test runs the land-check publication scan through a `safe-push` link whose canonical path
     is `north` and gets the scan's verdict.
- **P14 The swap.** Covered harness change: review per the rule above. Precondition, checked by the
  worker before landing: `readlink ~/.local/state/north/current` on whiterabbit and on gex45
  (`ssh gex45`) names a runtime that contains P13. Delete `bin/safe-push` and its allowlist entry. Point
  the `tests/it` callers at the helper. Update AGENTS.md, `src/cli/help.txt`, docs/features and
  `runtime_floor_gate_tests.rs`. Done when: (1) `git ls-files bin/safe-push` prints nothing; (2)
  `cargo test --workspace` passes. After landing, on whiterabbit and gex45:
  - after `north runtime update`, `readlink -f ~/.local/bin/safe-push` ends in `/north` under
    `runtimes/`, and `safe-push --help` exits 0;
  - one real landing each in north and south goes through the link, each sha an ancestor of its
    origin/main;
  - `north health` reports the land probe passing.
- **P15 Boundaries.** Covered harness change: review per the rule above. Delete the verbs that only bash
  called: `north land push`, `north land route`, `north land restore`, `north remote-land` and
  `north safe-push-result`. Delete the test-only environment hooks, and settle structure.toml. Done when:
  1. `north code-health layers origin/main HEAD` is clean.
  2. `rg -n 'Command::new' crates/north-safe-push/src --glob '!shell/**'` prints nothing.
  3. `rg -n 'SAFE_PUSH_(TEST_REAL_NORTH|WATCH|SELF)' crates src tests` prints nothing.
  4. `rg -n 'bin/safe-push' crates src tests harness bin --glob '!**/fixtures/**'` prints only
     historical records (incident and staffing fixtures stay as recorded).

Why this order:
- P1 is the early falsifying check. If argv[0] did not survive the symlink chain, decision 5 would fall
  back to a `north safe-push` wrapper, and only P14 would change.
- P2 comes first among the real work because it deletes about two thirds of what would otherwise be
  ported. Its tests move first, so the deletion is never red.
- P3-P5 are moves (`git mv`, no logic), so each carries its own equivalence argument.
- P6-P10 change structure behind tests that do not change. P11 is the cut-over, and P12 moves the
  scenarios.
- P13 has to be current on every host before P14 lands. Each host's old runtime binary installs the
  next runtime, and P14 is the commit that makes the canonical path `north`.

## How the four in-flight changes fit

All four are on origin/main as of this revision.

- `--restore` (north#618, eb3fddf5f): `land/restore.rs` moves to `restore.rs` (P4) and goes onto
  `Origin::push` (P7). Its scenarios already live in cargo.
- `--delete-tag` (north#620, a1c853cf2): `safe_push_tag_delete.rs` moves to `tags/delete.rs` (P4). Its
  inline `starts_with('v')` gives way to `tags::name::release_tag`, and its push goes through
  `Origin::delete` (P7).
- The non-admin push identity (north#619, bc3aa3bde and b081e327c): nothing in safe-push changes.
  Decision 7 and P6's property keep every safe-push write under the session's credential helper.
- `v*` refusal outside `north release` (d8e911988): `release.rs` moves to `tags/release.rs` (P4) and its
  `version_tag` to `tags/name.rs` as `release_tag` (P5). `--tag` and `--delete-tag` refuse through the
  same function.

## Review findings and answers

On 2026-11-06 a fresh Sonnet 5.5 reviewer (a different tier from the Opus author) read the first draft
against the source and tried to break it. Findings and answers:

1. BLOCKER: the per-lane deletion would turn red the bash scenarios 1, 2, 5, 8-11, 22 and 23, which
   land with a non-dry `--to main` and no lander config, and seven `tests/it` callers, before any box
   converted them. Answered: P2a is now a test-only box that converts or deletes each of them while the
   per-lane code still exists. The kept-but-landing list is stated above the boxes.
2. BLOCKER: canonicalizing the safe-push path (`follow.rs` `executable`, `after/gates.rs`) yields
   `north` once the link exists, so `slots/runner.rs`'s `--publication-scan` call would reach north's
   own argument parser. Answered: decision 5 forbids running safe-push by its canonical path, and P13
   moves those callers to `entry_points::command(SafePush)` with a test through a canonicalized link,
   before P14.
3. MAJOR: pushes also set `SAFE_PUSH_OUTPUT_CLAIMED`, which the property "SAFE_PUSH_ACTIVE and nothing
   else" would strip. Answered: decision 7 and P6's property allow exactly these two variables.
4. MAJOR: the deletion premise was measured on whiterabbit alone. gex45 landed per lane until 12:01 PM
   on 2026-11-05, and the refusal reaches any host without the machine git config. Answered: the
   evidence now covers both hosts (none since that time). Decision 1 names who the refusal reaches and
   what it says, and P2b rewrites `enqueue.rs`'s "landing.queue = direct" advice.
5. MAJOR: `rg 'bin/safe-push'` cannot print nothing (historical fixtures), and several registries were
   unplanned: `citation_check` `SAFE_PUSH_READERS`, `loops.toml` `PER_LANE`, `tests/it/crates.txt`,
   `LANDER_PATHS`, `follows-release.test.sh`, `runtime_floor_gate_tests.rs`. Answered: P15's check is
   scoped and leaves fixtures, and each registry is assigned to P2a, P2b, P4 or P14.
6. MAJOR: the link bypasses `bin/north`'s `unset CODEX_HOME`. Answered: the safe-push entry unsets it,
   tested in P1.
7. MINOR: once the per-lane path is gone, scenarios 35-38 would prove nothing. Answered: `Terminal`
   loses `Landed` and `TimedOut`, and P8 and P9b state what 35-38 now prove (no orphaned land-check
   process, a refusal within the deadline).
8. MINOR: P2c was oversized with an unfalsifiable check, and some checks needed live hosts. Answered:
   P2 is now five boxes, each with a grep-able or test check. Live-host checks are "After landing"
   observations, not landing gates.
9. MINOR: the in-flight commits were cited by their pre-rebase shas as not on main, and the agent
   identity's effect on `--tag` was unstated. Answered: the shas are corrected, and decision 7 states
   which identity can create which tag.

The reviewer found no cycle in the module DAG and confirmed that the P13-before-P14 ordering keeps
`~/.local/bin/safe-push` from dangling, that the scenario partition covers 1-54, and that the lander's
`checker: safe-push` and absolute-path invocations survive the link.

Later on 2026-11-06 the lead asked that every box state its harness-editor coverage, and that
structure.toml record the module tree under the landing module. Both are in this revision: decision 9 and
the review rule above the boxes, and eleven `landing.safe_push.*` modules plus the `north-safe-push`
crate entry in structure.toml. Recording the tree moved the entry point out of `lib.rs` into `entry.rs`,
because `crate-roots` owns every crate root. The request also named a `Reviewed:` trailer. The plan
keeps none, because that gate was retired on 2026-11-04 and the live review rule records no trailer.
