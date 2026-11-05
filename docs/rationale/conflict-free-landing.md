# Conflict-free landing: no lane starves, no author round-trip for a mechanical conflict

On 2026-11-05 Tom asked for a fix by construction after the lane that puts every repository on the
lander (s18-integrated) lost the race to main four times in 20 minutes while its author rebased by
hand. This note owns the lander's conflict semantics, queue fairness, generated files and
staffing-time overlap. [landing-train.md](landing-train.md) owns the rest of the lander, and its step 9
(mechanical conflicts) points here once box C9 lands. Planned at origin/main 09cb73335 and revised at
f1745de2e against two reviews (disposition at the end).

## Evidence, 2026-11-05, 2:39 to 3:22 AM (laptop clock, +07)

Sources: every `north land:` line that `journalctl --user -u north-land` printed on the laptop from
2:30 to 3:30 AM (78 lines; the lead copied them to gex45), and a replay on gex45 of each conflicted
ticket's head (the lane branches are on gex45; s18-integrated's heads are on the laptop and were
not replayed). The replay checked out the round's main, ran the lander's own
`git cherry-pick --allow-empty main..head` with no other lane picked first, and classified each
conflict block with `merge.conflictStyle diff3`: a block whose base side is empty is insert/insert,
anything else is modify.

What the lander did:

- 2:53 AM: candidate d64aab1 (refusal-reason, s8-app-push) landed; the runtime changed and the lander restarted.
- 2:54 AM: it pulled 18 tickets from gex45's outbox and sent 12 back with `conflict` before it built
  one candidate: etxtbsy-helper, s6b-warm-slots, s21-cleanup and land-check-hang on
  tests/it/land_serve.rs; s14-deps, s13-scan, s12-language and several-commits on
  crates/north-landing/src/land/mod.rs; b26-depth on land/lander.rs and land/serve.rs; docs-3a on
  docs/features.md; docs-7 on docs/harness.md; docs-8 on citation_check/tests.rs.
- 3:02 to 3:21 AM: 15 more lanes landed in four candidates and 13 more send-backs followed, each
  within seconds of a restart that pulled the outbox.
- In all: 25 conflict verdicts on 15 lanes and 17 lanes landed in 28 minutes. several-commits went
  back three times, s18-integrated four.
- Conflicted files across the 25 verdicts:
  - crates/north-landing/src/land/mod.rs: 11
  - tests/it/land_serve.rs: 5
  - land/serve.rs: 4
  - docs/harness.md: 3
  - docs/features.md and land_offload/ship/tests.rs: 2 each
  - land/enqueue.rs, land/lander.rs, slots/runner.rs, tests/it/land_scan.rs, tests/it/land_enqueue.rs
    and citation_check/tests.rs: 1 each
  - "unreadable" (a missing head): 1

s18-integrated sent back four times, each against the lane that had just landed on its files or was
being picked with it:

- 3:02:35 AM: ship/tests.rs, land/mod.rs, land/serve.rs, onto main cf363a7. The author's rebase took 10 minutes.
- 3:13:00 AM: land/mod.rs, land/serve.rs, tests/it/land_serve.rs, in the candidate that became
  63d9e77, after docs-7, b26-depth and land-check-hang were picked onto b563103. The rebase took
  4.5 minutes.
- 3:17:34 AM: land/mod.rs, land/serve.rs, onto 63d9e77, whose b26-depth commit c21abdf98 changed both
  files 9 seconds before. The rebase took 4 minutes.
- 3:21:31 AM: land/enqueue.rs, tests/it/land_enqueue.rs, onto 09cb733, which changed both files
  9 seconds before.

The brief lists a fifth send-back at 2:54 AM. The journal has no s18-integrated line at 2:54.

### s18-integrated replayed on the laptop (box C1)

Each head was picked with `git cherry-pick --allow-empty MAIN..HEAD` onto that round's main alone, in a
scratch worktree off the laptop's object store, with `merge.conflictStyle diff3`. No path in its
conflicts has a `merge` attribute today (only structure.toml and harness/machine-bound-tests do).

- **13107824d, 3:02:35 AM, main cf363a7.** Alone it conflicts only on land_offload/ship/tests.rs: one
  insert/insert block at the end of the file, both sides whole `#[test]` functions and helpers, against
  offload-no-auto-maintenance (94980cb8c, landed at 3:02:22). Its land/mod.rs and land/serve.rs
  conflicts were batch-mate conflicts with s10-sheriff-queue and s14-deps, picked before it into the
  candidate that became b563103. No driver or split merges the ship/tests.rs block: no rule covers that
  file, and this plan adopts no Rust item union. One genuine author turn.
- **572a28fa9, 3:13:00 AM, main b563103.** Alone it picks cleanly: b563103 is its parent. The whole
  verdict was a batch-mate conflict with b26-depth and land-check-hang, which were in the same
  candidate and had not landed.
- **af10b142d, 3:17:34 AM, main 63d9e77, picked first onto bare main.** Three modify blocks against
  b26-depth (c21abdf98): the one-line `USAGE` in land/mod.rs, the one-line `USAGE` in land/serve.rs
  (b26 made `--depth 1|2` into `--depth D`, s18 added `--all`), and serve.rs's `run` (b26 rewrote its
  49 lines, s18 replaced it with a 2-line call to `serve_repository`). The land/mod.rs split (C5)
  removes the first block, since the help would print serve.rs's own `USAGE`. The two serve.rs blocks
  stay genuine.
- **7d5a3e04e, 3:21:31 AM, main 09cb733, picked first onto bare main.** Against s13-scan: an
  insert/insert block of `use` lines in land/enqueue.rs (s13 added `use super::scans::outgoing;`, s18
  added `repositories` and `setting`), and a modify block in tests/it/land_enqueue.rs, where s13 added
  two fields to the fixture's `Environment` and s18 wrapped the `enqueue` call in a
  `landing.lander-host` setting. Neither merges by a driver or the split: enqueue.rs is not a
  declarations-only file, and the `lines` entry pattern takes only `mod` lines. Genuine.

Positions, from the queued microseconds of each lane's oldest ticket in its run of done/ tickets:

- b26-depth 2:39:35 AM, s13-scan 2:42:41, several-commits 2:43:47, land-check-hang 2:52:57.
- s18-integrated 2:54:28 (13107824d), 91 seconds behind land-check-hang, so it sorts behind all four.
  It also sorts behind s10-sheriff-queue (12:50:30 AM) and s14-deps (2:33:11), and ahead of
  s14-deps's second run, which began at 3:17:20 after its first run landed.
- The same session's earlier branch, s18-lander-per-repo (queued 12:46:20 AM, sent back), is a
  different (host, root, branch) lane and gives s18-integrated no earlier position.

The kept position (G4) and the reservation (G5) therefore save s18-integrated nothing on their own,
as the arch review's 2e disposition expected. Part 2 would have kept it out of all four verdicts:

- **3:02:35, held.** b26-depth, s13-scan, several-commits and land-check-hang were each sent back at
  2:54 on files s18-integrated changes (land/mod.rs, land/serve.rs, land/enqueue.rs,
  tests/it/land_serve.rs), so their reservations were live until about 3:14. The pick was also onto a
  stack, which part 1 alone defers.
- **3:13:00, held.** b26-depth and land-check-hang were queued in the same candidate, and s13-scan and
  several-commits were sent back at 3:07, live until about 3:27. Part 1 alone defers it too: the pick
  was clean on main and conflicted only on the stack.
- **3:17:34, held.** s13-scan's reservation was live, and several-commits had a queued ticket
  (1e47420e1, 3:15:50) on land/mod.rs. Without the Held rule, the queued-ahead clause of Defer still
  defers it.
- **3:21:31, held.** s13-scan had landed, but several-commits, sent back at 3:17:36 and queued again
  at 3:29:25 and sent back at 3:34:07, held land/enqueue.rs, tests/it/land_enqueue.rs and land/mod.rs
  until 3:54 at the latest (G5).

Under part 2, then, s18-integrated is judged once, onto a main that holds b26-depth, land-check-hang
and s13-scan, and its author rebases once. Its first head picked onto f1745de2e (main after s14-deps,
3:33 AM) conflicts on seven files: ship/tests.rs, land/enqueue.rs, land/mod.rs (3 blocks),
land/serve.rs (3), tests/it/land_enqueue.rs, tests/it/land_serve.rs and tests/it/main.rs. The land/mod.rs and tests/it/main.rs blocks
are in files C5 puts under the `lines` rule. That one send-back replaces four, and its one rebase
replaces the 18.5 minutes the first three took.

Replay, 19 of the 25 send-backs (s18-integrated's four heads and the 1e47420 head of several-commits
are not on gex45, and s6b's 3:02 "conflict unreadable" is a missing head, not a conflict):

- **Batch-mate only, 5:** docs-3a, docs-8 and docs-7 at 2:54, docs-7 at 3:02 and default-checks at 3:12
  pick cleanly onto that round's main alone. Each conflicted only with a lane picked before it into the
  same candidate, and that lane had not landed. `candidate::pick` reports any failed pick as
  `conflict`, and `serve.rs` decides the ticket. Replayed again onto the mains after their batch-mates
  landed:
  - docs-8's batch-mate, the citation-check commit d94e608b8, never landed. docs-8 still picks cleanly
    onto cf363a7 and b563103, so it was sent back for nothing.
  - docs-3a conflicts only on docs/features.md once docs-3b, docs-3c and cloud-handback-land are on
    main.
  - docs-7 conflicts on docs/harness.md once s10-sheriff-queue (60c6d07b6) is on main.
  - default-checks conflicts on slots/runner.rs once land-check-hang is on main.
- **Whole Rust items appended at one point, 3:** s6b, s21-cleanup and land-check-hang each conflict in
  one tests/it/land_serve.rs block. s6b's 2:54 verdict also names land_offload/ship/tests.rs; that part
  picks cleanly onto main alone, so it was a batch-mate conflict. The base side of the block is
  empty, and both sides are complete
  items (`const`, `impl Fixture`, `mod cleanup`; each side parses as a Rust file under `rustfmt`).
- **land/mod.rs, 7:** s14-deps, s13-scan, s12-language and several-commits at 2:54, and s13-scan,
  b26-depth and several-commits at 3:07. Every one has a modify block on the single-line `USAGE`
  string that joins all four subcommands, so each lane that changes any subcommand's flags edits it.
  Four also have a `pub mod` insert/insert block (whole items), and s13-scan at 3:07 has an
  inserted match arm (not an item).
- **Generated aggregate, 1:** experiments-registry on docs/features.md, which docs-6-regenerate
  regenerates.
- **Genuine overlap, 3:** etxtbsy-helper (a sweep that renames `executable` to
  `shim::write_executable` across land_serve.rs, 10 modify blocks), b26-depth at 2:54 (it and
  s8-app-push both rewrote `--depth` parsing and `Lander::new` in serve.rs), and s14-deps at 3:17
  (docs/harness.md prose that docs-7, which landed 9 seconds before, had rewritten).

Hotspots since 2026-11-03 (`git log --since=2026-11-03 -- FILE`): docs/features.md 36 commits,
land/serve.rs 12, land/mod.rs 10, tests/it/land_serve.rs 10 (1,444 lines, over the 1,200 ceiling
structure.toml sets for tests), docs/harness.md 7.

Two more facts the design depends on:

- The lander's worktree resolves `merge=set` paths only because bash safe-push once wrote
  `merge.set.driver` into the shared `.git/config` (`bin/safe-push` ~1798). `candidate::ensure` on main
  sets no driver. When git finds no definition for a driver that `.gitattributes` names, it falls back
  to its built-in text merge without saying so, so the driver would stop working the day S19b deletes
  the bash path.
- rerere would have resolved none of the 19. It replays a recorded resolution only when the same
  conflict hunk recurs. The lanes that came back (several-commits three times, s13, b26 and docs-7
  twice each) conflicted with a different main hunk each time, and the hand resolutions were recorded
  in the authors' worktrees on gex45, not in the lander's on the laptop.

Also observed, and outside this plan's scope: every landing of a north lane restarts the lander
on the new runtime (five restarts in 28 minutes). The 3:12:43 restart discarded speculative
candidate a77357a while its check was running.

## Design

Terms:

- A ticket's **files** are the union, over the lane's current run of tickets (below), of
  `git diff --name-only --no-renames $(git merge-base main head) head`. This covers files a rebase
  newly touches. A head the lander cannot read contributes no files.
- A path is **driver-resolved** when its `merge` attribute names a harness/merge-sets.toml rule
  (`lines`, `toml-array` or `regenerate`). Two lanes on such a path never count as overlapping.
- A lane is (host, root, branch), as `queue::collect` and `speculate::lane` already key it.
- A lane's **run** is its decided tickets in queue/done/, newest first, taken while their verdict is a
  `conflict` or "replaced by a newer ticket for this lane". The run stops at the first other verdict,
  and `landed` ends it.
- A lane's **position** is the queued microseconds (the done/ file-name prefix) of the oldest ticket
  in its run, provided the run's last send-back is less than 24 hours old. Otherwise it is the live
  ticket's own queued microseconds. Red fixes still sort first.
- A lane's **reservation** is live while its run is non-empty and either it has a queued ticket or
  its last send-back was less than B minutes ago. It covers the lane's files minus the
  driver-resolved paths.

Position and reservation are derived from queue/ and queue/done/ in each round, which hold every
ticket with its time, head and verdict. `sheriff::lane_ticket` already reads them that way. There is
no claims store, no writer and no delete site. If done/ is pruned, the reservation and position
disappear, which only loosens holds.

`land::reservation::judge(ticket, stack, earlier) -> Pick` is one pure function, called from
`candidate::build` in position order. Its rules, first match wins:

- **Red fix:** a red-fix or sheriff-revert ticket ignores every hold. Main red outranks fairness.
- **Held:** a live reservation of another lane with an earlier position overlaps the ticket's
  files. The ticket is not picked and stays queued.
- **Include:** the pick onto the stack is clean.
- **Defer:** the pick conflicts, and either the stack is not bare main, or a queued ticket of another
  lane with an earlier position overlaps the ticket's files. The ticket stays queued and is not decided.
- **Send back:** the pick conflicts on bare main, and nothing ahead of it on its files is in flight.
  The verdict names the files, and the lane's run grows by one, which makes its reservation live.
- Unreadable head, dropped commits and stale revert: **send back**, as today.

Guarantees, each with what enforces it:

- **G1, a conflict is judged against main alone, and no ticket is sent back for a batch-mate.**
  Enforced by the rule order of `judge`, its truth-table unit tests, and the fixture
  `land_conflict::batch_mate_defers`.
- **G2, tickets that overlap are never in one candidate.** Overlap means git's own test: a pick that
  conflicts on the stack is deferred out of the candidate. Same-file lanes whose picks merge stay
  together. Enforced by `judge` and `land_conflict::overlapping_pick_leaves_candidate`.
- **G3, no starvation.** Holds and deferrals point only at lanes with earlier positions, so they cannot
  cycle. A position never moves later, and new lanes sort after it, so only the finite set of lanes
  ahead of a ticket can delay it. Each of those leaves the in-flight set by landing, by a decision that
  is not a conflict, or B minutes after its last send-back without a return. The earliest lane on a
  set of files is never held or deferred for anyone. The bound: a lane is sent back at most once more
  for each earlier-positioned lane that lands on its files after its first send-back, plus red fixes.
  Enforced by `land_conflict::oldest_is_judged_on_main` and `land_conflict::held_lane_bounded`.
- **G4, a sent-back lane keeps its position.** Enforced by `reservation::position`, used by
  `queue::pending`, and `land_conflict::sent_back_keeps_position`.
- **G5, a bounded reservation from the first send-back (N = 1).** It holds later lanes off the lane's
  files for at most B = 20 minutes past its last send-back, or until it lands. Enforced by
  `land_conflict::reservation_holds_later_lanes` (injected clock),
  `land_conflict::reservation_expires`, and `land_conflict::red_fix_ignores_reservation`.
- **G6, no author round-trip for a conflict a driver resolves.** Every git call the lander makes
  carries every driver as `-c merge.NAME.driver=<running north> merge-driver NAME %O %A %B %P`, so
  the definition cannot be missing and the binary is the lander's own. The candidate check checks the
  merged result. Enforced by `candidate::output` and the gate test `every_merge_attribute_has_a_driver`.

Why N = 1, against N = 2 in the first draft: once drivers take the mechanical conflicts and deferral
takes the batch-mate ones, every send-back is a genuine conflict that its author fixes next. A second
send-back is exactly the turn a reservation exists to save, and holding later lanes off those files
for at most 20 minutes costs them less than one check cycle (3 to 9 minutes tonight) per wasted turn.
The 20 minutes is twice tonight's longest author rebase (10 minutes). Both values are
harness/defaults.toml keys: `land.reserve_after_sendbacks` and `land.reservation_minutes`.

### Part 1: the deferral rule (G1, G2)

This is one new arm in `candidate::build`: `Pick::Deferred`, the ticket untouched. It needs no extra
pick. A deferred ticket comes back in the next round in position order, so once everything ahead
of it on its files has been decided it is picked onto bare main and judged. Speculation already keeps
only `Pick::Included` and decides nothing, so `speculate::Taken` is unchanged.

Rejected: file-overlap selection, which kept two lanes on any shared path out of one candidate. Its
test is coarser than git's. Every .rs path counts as shared, and so do bin/safe-push and
src/cli/help.txt, with 19 and 17 commits since 2026-11-03 and no conflict verdict tonight. It would
have split three of tonight's four landed multi-lane candidates:

- d64aab1: refusal-reason and s8-app-push share land/mod.rs, serve.rs and land_serve.rs.
- b563103: s10 and s14 share land/mod.rs.
- 63d9e77: b26 and land-check-hang share land_serve.rs.

That is three more serial checks for no gain.

### Part 2: no send-back while lanes ahead are still landing (G3, G5)

The **Held** rule, together with the queued-ahead clause of **Defer**, is what saves author turns.
A lane whose earlier-positioned neighbours on its files are queued or out with their authors is judged
only once they have landed. Its author then rebases once, onto a main that already holds them. A
lane that is sent back holds later lanes off its files, so its rebased head picks onto a main whose
versions of those files moved only by earlier lanes.

The send-back message, through refusal-inline-clippy's refusal text, says three things:

- the lane's queue position is kept;
- its files are reserved until T;
- the lanes ahead of it on those files have landed.

### Part 3: the lander resolves mechanically (G6)

- **Drivers on each call.** `candidate::output` passes `-c` for every entry of `merge_set::DRIVERS`,
  using the absolute `current_exe()`. Nothing is written to the shared `.git/config`, so authors'
  merges on the laptop do not change. It does not race with bash safe-push's `git config` writes, and
  it survives S19b. The gate test is pure: every `merge=NAME` in `.gitattributes` names an entry of
  `DRIVERS`.
- **Unknown kinds.** `merge_set::read_rules` skips a kind it does not know and reports it once. A
  driver that finds no rule for a path then returns git's own text merge and its exit status, so an
  older north on another host merges as git would.
- **Generated files.** These are docs-6-regenerate's `regenerate` kind, in flight, and its
  `north merge-driver continue`.
- **Generator changes.** A `regenerate` rule gains `generator = [paths]`. The regenerator is the
  running lander binary, not the candidate tree, so a ticket that changes a rule's generator defers
  every later pick in the same candidate that touches that rule's sources or aggregate. Otherwise
  D's regenerated aggregate would come from the pre-G generator, fail `--check`, and cost a culprit
  search.
- **Module lists.** Use the existing `lines` kind with a new `entry` pattern, for files that hold only
  declarations: crates/north-landing/src/land/mod.rs after C5, tests/it/main.rs (95 `mod x;` lines,
  7 commits since 2026-11-03), and tests/it/land_serve/mod.rs after C6. The pattern is
  `^(pub )?mod [a-z0-9_]+;$`. A line that does not match makes the driver leave the file conflicted,
  so a function moved back into mod.rs is a conflict, not a set merge. Sorted sides stay sorted
  (`merge_set` keeps order when all three sides are sorted), which matches rustfmt. The
  .gitattributes line and the rule land in the same lane.
- **Not adopted:**
  - A `rust-items` union driver on `*.rs`. It would union same-named items (`use std::path::Path;`
    twice: E0252), it would union statements such as `assert!(..);` that syn parses as items, and it
    would route every two-lane .rs merge through north. After the splits it would cover none of the
    replayed blocks.
  - A `rust-slice` command-list kind: it resolved 0 of the 19 replayed blocks.
  - rerere: no conflict hunk recurred tonight.
  - Generic `merge=union`: landing-train.md step 9a.
  - The worker-haiku resolver, landing-train.md B29, stays a later option outside this guarantee.

### Part 4: hotspot splits

- **crates/north-landing/src/land/mod.rs** keeps only `pub mod` lines.
  - `run` and the help move to land/command.rs. Its help prints each subcommand's own module
    `USAGE` (slots::runner, serve, enqueue, scans, after::follow), so `north land --help` gains the
    `--portable` line and the five further scan lines it omits today. That is a deliberate change,
    and the box updates any test that pins the text.
  - `follow` stays unlisted in the help, as now.
  - A lane that changes one subcommand's flags then edits only that module. A lane that adds a
    subcommand edits command.rs's match: a genuine overlap only when two lanes add commands at once.
    Tonight that happened once (s13-scan and s14-deps both added `scan`).
- **tests/it/land_serve.rs** splits into tests/it/land_serve/: `mod.rs` (declarations only, under the
  `lines` rule), `fixture.rs`, `train.rs`, `culprit.rs`, `credential.rs`, `after.rs`, `remote.rs`,
  `speculate.rs`, `records.rs`, `checks.rs` and `sheriff.rs` (today's land_sheriff.rs).
  - A split is a sweep, and it conflicts modify/delete with every lane open on the file.
  - The box is staffed only when staffing-overlap reports no in-flight lane and no queued ticket on
    the file, and it is a pure move.
  - This plan's own fixtures go in a new tests/it/land_conflict.rs, so they need neither split.
  - structure.toml gives tests/it/land_serve/** a 400-line ceiling.

### Part 5: staffing-time overlap

staffing-overlap is in flight (it warns, and when two or more in-flight lanes changed a file, queues
the box behind the earliest). After it and C4:

- A live lander reservation counts as hot on its own. A box that names a reserved file is queued
  until the reservation ends, through a hold that dispatch re-evaluates on each tick against the
  derived reservation, not a `Follows:` line. So when the reserving lane's session dies, the box
  starts at T rather than waiting for an issue to close.
- "In flight" counts only live sessions and queued tickets.
- The exempt paths are the driver-resolved set. The caller computes it from `git check-attr merge`
  and harness/merge-sets.toml and passes it into the pure function in
  crates/north-core/src/fleet/overlap.rs, so fleet stays independent of north-landing.

### Expected effect on tonight's 19 replayed send-backs

- **9 merged by a driver or a split:**
  - docs-3a and experiments-registry by `regenerate`;
  - s14-deps, s12-language and several-commits at 2:54, and b26-depth at 3:07, by the land/mod.rs
    split and the `lines` rule (their only other block was the USAGE line);
  - s6b, s21-cleanup and land-check-hang by the land_serve.rs split, because each appends a new
    concern's const, `mod` or `impl`, and those land in different concern files.
- **1 deferred, then landed without its author:** docs-8.
- **3 never issued as separate send-backs:**
  - s13-scan at 2:54 and docs-7 at 2:54, deferred until the lane ahead of each landed;
  - several-commits at 3:07, which would have landed at 2:54.
- **6 genuine, one author turn each:** etxtbsy-helper, b26-depth at 2:54, s13-scan, docs-7,
  default-checks and s14-deps at 3:17.

s18-integrated (measured by C1, above): 4 send-backs become 1 genuine one, held by part 2 until the lanes ahead on its files land; 1 of the 4 was a pure batch-mate conflict.

## In-flight lanes: what each part changes, and the landing order

- **staffing-overlap** (fleet only): C7 extends it.
- **land-check-deadline** (slots/runner.rs, capacity): C5 makes runner.rs's `USAGE` public, so C5
  follows it.
- **s18-integrated** (land/mod.rs, serve.rs, enqueue.rs, ship/tests.rs, land_serve.rs): the starved
  lane. Until C4 exists, the lead holds new lander tickets on its files until it lands.
- **lander-culprit-parallel** (committed 49d4b547d; serve.rs, mod.rs, land_serve.rs): unchanged.
  Deferred tickets never reach its culprit search.
- **refusal-inline-clippy** (enqueue.rs, refusal.rs, mod.rs, land_serve.rs): unchanged. C4's message
  goes through its refusal text.
- **docs-6-regenerate** (candidate.rs, merge_set.rs, merge_continue.rs, .gitattributes): delivers
  `regenerate`. C3 replaces its `git config` writes in `ensure` with per-call `-c`.
- **lander-outbox-each** (the remote-outbox pull, on the laptop): C4 reads positions from done/, so
  it follows this lane's pull path.

Order:

1. staffing-overlap and land-check-deadline, any time.
2. s18-integrated, with the hold above.
3. lander-culprit-parallel, then refusal-inline-clippy, then docs-6-regenerate, then
   lander-outbox-each. Each later lane's author rebases after the one before it lands, because they
   share land/mod.rs, serve.rs, land_serve.rs and candidate.rs.
4. C1 to C9 below, in order. The deferral and driver boxes come before the two sweeps.

## Boxes, in landing order

Each box is about one lane.

- **C1, replay s18-integrated on the laptop (20 min).**
  - Follows: nothing; the lead or a laptop worker runs it.
  - Files: this note.
  - Done when:
    1. This note's s18 section lists, for each of the four heads from the laptop lander's object
       store: the conflict files and block shapes alone on that round's main; whether a driver or the
       land/mod.rs split would merge them; and its first ticket's queued time against b26-depth,
       s13-scan, several-commits and land-check-hang.
    2. It states whether part 2 would have held or deferred it each time.
- **C2, the deferral rule and positions (40 min).**
  - Follows: docs-6-regenerate, lander-culprit-parallel.
  - Files: new crates/north-landing/src/land/reservation.rs (`judge`, `position`, pure) with
    reservation_tests.rs; candidate.rs (`Pick::Deferred`); serve.rs; queue.rs (order by position);
    new tests/it/land_conflict.rs; tests/it/main.rs.
  - Done when:
    1. Fixture `batch_mate_defers`: A and B both change line 1 of shared.txt, A's check fails, and C
       changes other.txt. The journal prints `deferred: B`, A is refused, C lands, and then B lands
       with no conflict verdict.
    2. When A passes, B gets exactly one conflict verdict, naming shared.txt, on the main that holds A.
    3. `sent_back_keeps_position`: S is sent back, X and Y queue, and S re-queues. S is the next
       candidate's first pick.
    4. A run 25 hours old gives no kept position.
    5. The truth table of `judge` passes as unit cases.
- **C3, drivers on every lander git call (25 min).**
  - Follows: docs-6-regenerate.
  - Files: candidate.rs (`output`, `ensure`), crates/north-landing/src/landing/merge_set.rs
    (`DRIVERS`, skipping unknown kinds), harness/merge-sets.toml (`generator`), merge_continue.rs.
  - Done when:
    1. Fixture `set_driver_without_shared_config`: in a repository whose .git/config has no `merge.*`
       keys, two lanes that each add a structure.toml `may_use` entry land in one candidate.
    2. `every_merge_attribute_has_a_driver` fails on a planted `merge=nosuch`.
    3. A merge-sets.toml with an unknown kind still merges structure.toml as a set.
    4. A ticket that changes a `regenerate` rule's generator path defers a later ticket that touches
       that rule's sources.
- **C4, reservations (40 min).**
  - Follows: C2, refusal-inline-clippy, lander-outbox-each.
  - Files: reservation.rs (live reservation, Held rule, red-fix exemption), refusal.rs (message),
    harness/defaults.toml (`land.reserve_after_sendbacks = 1`, `land.reservation_minutes = 20`),
    land_conflict.rs.
  - Done when:
    1. Fixture `reservation_holds_later_lanes`, with an injected clock: S is sent back. Later lane X
       on one of S's files is held with "reserved by S until T", disjoint Z lands, S re-queues and
       lands, and then X lands.
    2. `reservation_expires`: S never returns, and X lands once the clock passes T.
    3. `red_fix_ignores_reservation`: a red-fix ticket on S's file lands while S's reservation is live.
    4. An older lane on S's file is not held.
    5. S's send-back message names the kept position, the files and T.
- **C5, land/mod.rs to declarations only (30 min).**
  - Follows: s18-integrated, lander-culprit-parallel, refusal-inline-clippy, land-check-deadline, C3.
    Staffed when staffing-overlap shows no in-flight lane on land/mod.rs.
  - Files: land/mod.rs, new land/command.rs, the subcommand modules' `USAGE` visibility, the callers
    of `land::run`, .gitattributes, harness/merge-sets.toml (`entry` on `lines`), merge_set.rs.
  - Done when:
    1. Fixture: two lanes that each add a `pub mod` line to land/mod.rs land in one candidate.
    2. A lane that adds a `fn` to land/mod.rs conflicts.
    3. Two lanes that each add a `mod` line to tests/it/main.rs land in one candidate.
    4. `north land --help` prints each module's usage, the diff against before shows only added lines,
       and the help test is updated.
- **C6, split tests/it/land_serve.rs (30 min).**
  - Follows: C5 and staffing-overlap. Staffed only when staffing-overlap reports no in-flight lane and
    no queued ticket on the file.
  - Files: tests/it/land_serve.rs and land_sheriff.rs into tests/it/land_serve/, tests/it/main.rs,
    .gitattributes, harness/merge-sets.toml, structure.toml.
  - Done when:
    1. The staffing check printed no lane on the file at launch.
    2. `cargo test --test it land_serve -- --list` prints the same names before and after.
    3. No file under tests/it/land_serve/ exceeds 400 lines, and land_serve/mod.rs is under the
       `lines` rule.
- **C7, staffing reads reservations and driver-resolved paths (30 min).**
  - Follows: staffing-overlap, C4, C5.
  - Files: crates/north-core/src/fleet/overlap.rs, src/cli/fleet/launch.rs,
    crates/north-ops/src/ops/dispatch/.
  - Done when:
    1. Replaying staffing-overlap's fixture of 2026-11-05, 12:30 to 1:30 AM, queues the same launches
       as before, except boxes whose only overlap is a driver-resolved path, which start.
    2. A box naming a file under a live reservation prints `queued:`, and with the reserving session
       gone it starts at T on the next dispatch tick.
    3. The exempt set comes from .gitattributes and harness/merge-sets.toml, with no path literal in
       the source.
- **C8, measure the built lander against tonight (30 min).**
  - Follows: C2 to C7.
  - Files: this note.
  - Done when:
    1. This note records a one-time replay on the laptop of tonight's 23 readable heads (19 here plus
       s18-integrated's four) through the built lander, with each verdict's outcome against the
       expected effect above, and every difference explained.
    2. No standing test asserts these counts: the per-guarantee fixtures are the standing tests.
- **C9, link from landing-train.md (10 min).**
  - Follows: C8.
  - Files: docs/rationale/landing-train.md.
  - Done when:
    1. Step 9 (e) of landing-train.md links this note.
    2. Its "In-flight lanes" section names this note as the owner of conflict semantics.

## Review disposition (2026-11-05)

Two reviews of the first draft (1885e3e30): review-conflict-scn, which ran scenarios against the
code, and review-conflict-arch, which looked for the cheapest design and for drift.

Accepted:

- **Scenario M1:** item-union makes duplicates. `rust-items` is dropped (arch 3).
- **Scenario M2:** reservations held red fixes. Red-fix and sheriff-revert tickets now ignore holds
  (C4 check 3).
- **Scenario M3:** staffing holds never expired. Holds are re-evaluated against the derived
  reservation, and count live sessions only (C7 check 2).
- **Scenario M4:** the help text has to change. C5 says so and updates the test.
- **Scenario M5:** one kind per path, and `*.rs` routed through north. There is no Rust kind now,
  `lines` rules use exact paths, and each lands with its .gitattributes line.
- **Scenario M6, in part:** this plan's fixtures go in a new land_conflict.rs. The land_serve.rs split
  waits for an observed zero-lane window and is a pure move. The false "disjoint" claim about
  land-check-deadline is fixed.
- **Scenario S1:** the landing order is named, and the deferral and driver boxes come before the
  sweeps.
- **Scenario S2:** holds point only at earlier positions.
- **Scenario S3:** files are the union over the run, each head against its own merge base.
- **Scenario S4:** there is no pending grant state.
- **Scenario S5:** file-overlap selection is dropped.
- **Scenario S6:** there is no probe.
- **Scenario S7:** the `generator` paths defer picks (C3 check 4).
- **Scenario S8:** drivers are passed with `-c` and the absolute exe, and unknown kinds are skipped.
- **Scenario S9:** an unreadable head contributes no files.
- **Scenario S10:** no `select`, so nothing to re-run.
- **Scenario S11:** lanes are keyed by the (host, root, branch) triple.
- **Scenario S13:** 15 lanes; several-commits at 3:07 is counted once; s6b's ship/tests.rs part was a
  batch-mate conflict.
- **Arch 1:** deferral replaces selection.
- **Arch 2a to 2d:** one reservation, ordered by position, derived from done/, named `reservation`.
- **Arch 3:** `lines` with an entry pattern replaces `rust-items` and `rust-slice`.
- **Arch 4:** s18 is replayed first (C1), and the tonight replay is a one-time measurement (C8).
- **Arch 5:** drivers on each call.

Accepted with a different reason:

- **Arch 2e (N = 1).** The reason given was that s18-integrated's 3:12:55 ticket would then have
  picked onto unchanged files. But s10 and s14, which changed land/mod.rs, were already in the
  candidate under check at 3:02, and the lanes that beat s18 later had run positions from 2:54, so
  a hold on later lanes alone might not have stopped any of its send-backs. N = 1 stands for the
  reason in G5. What helps s18 is the queued-ahead deferral and the Held rule (part 2), and C1
  measures it.

Rejected:

- **Scenario S12 (syn features).** `cargo tree -e features -i syn@2.0.119` shows `full` already
  enabled in north's build. The point is moot anyway, since no syn dependency remains.
