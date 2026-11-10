# Leaner north: a measured plan to cut the code toward half

Tom asked on 2026-11-09 for a leaner north, possibly half its lines, and north#970 asks for the cuts to be
planned by measurement before any lane starts. This note measures the tree as it stands, sizes each cut it
can find with a line count and a risk, and shows which part of the issue's 25% bar no cleanup can pay for.

Owner: north#970

## The tree and the bar

`git ls-files '*.rs' | xargs cat | wc -l` reads 290,565 lines on 2026-11-09 across 2,283 files. A file counts
as test when it sits under `tests/`, ends in `_tests.rs`, `tests.rs` or `_test.rs`, or holds an inline
`#[cfg(test)] mod`, which splits the tree into 240,820 code lines and 49,745 test lines, 17.1% of the total.
The issue measured 282,548 lines, so main has grown 8,017 since; its box, at most 211,911 lines, now asks for
78,654 lines gone, 27.1% of today's tree.

## What measurement found first

Cleanup cannot pay for that bar, and the plan must say so before it writes a box that cannot land.

- No Rust source file is unreachable. Every file is declared from a `lib.rs`, `main.rs`, test or example root,
  including the odd mounts: `crates/north-core/src/agent_catalog.rs` mounts the whole `crates/north-core/src/catalog/`
  directory with `#[path = "catalog/..."]`, `crates/north-lander/src/land/mod.rs` mounts
  `crates/north-lander/src/land/commands.rs` with `include!`, and `crates/north-calibration/src/lib.rs`
  declares `threads` inline so its `crates/north-calibration/src/threads/calibration/` directory is live.
  A plan that starts by deleting "dead files" deletes working code.
- Unreferenced functions are small. Counting each function name's occurrences across the tree leaves 46 names
  that occur once, 390 lines in all, and that is an upper bound: it still counts trait impls and helpers that
  only a test calls. A confirmed sweep is worth a few hundred lines, not thousands.
- Duplicate bodies are smaller. Normalizing comments and whitespace leaves 8 groups whose bodies are byte
  identical, 104 removable lines: seven `evaluate` guards at 6 lines each and two at 10, `parse` in
  `north-lander` and `north-landing` at 13, `read_request` in `north-base` and `north-ops` at 11, three `repo`
  helpers at 6, `human` at 8, `visit_seq` at 7 and `text_of` at 7.
- Tests are the one large pool. `#[test]` marks 1,344 functions over 23,146 lines, and only 26 of them, 246
  lines, assert nothing. Deleting the entire test suite removes 49,745 lines, still 28,909 short of the bar,
  and takes every guarantee north checks after a landing with it.

So the 25% box is a capability decision, not a code-health one. A cut that only moves code counts for
nothing under north#949, which rules out the file merges and crate folds that a line count would otherwise
invite.

## Cuts, ranked by lines removed per risk

Measured lines are what the tree holds now for that cut's files; where a box sets a bar below the pool, the
box says so.

| Cut | Measured lines | Risk | What decides it |
| --- | --- | --- | --- |
| 1. Unreferenced functions and duplicate bodies | 494 lines | Low | Each of the 46 names is checked and the 104 duplicate lines are byte identical |
| 2. Tests that assert nothing | 246 lines | Low | 26 `#[test]` functions with no assertion in their body |
| 3. Guard rule boilerplate | 1,661 lines | Medium | 46 `north-guard` rule files of at most 60 lines each (662 test lines and 996 lines across 25 distinct rules); one decision shape and shared search-option table, bar 213 lines (`21,700` to `21,487`) |
| 4. Test-guarantee audit | 12,000 lines of a 49,745-line pool | Medium | Each deleted test names the journey test that already pins its guarantee; the test-admission rule stays intact |
| 5. `north-convo` | 7,665 lines | High | `north convo`, `north incident-mine`, `north tool-mine` and the `incident-retro` timer read transcripts |
| 6. `north-tui` | 3,156 lines | High | `north fleet` is Tom's screen; nothing else draws it |
| 7. `north-frontier` | 2,800 lines | High | `north test frontier run\|grade\|report` is the frontier scoreboard |
| 8. `north-lander-doctor` | 2,385 lines | Medium | `north land drill`, `north land doctor` and `north land gate-stats` diagnose the lander |
| 9. `north-tune` | 1,856 lines | High | `north tune` is the daily learning tick and the writer `harness/loops.toml` names |
| 10. `north-replay` | 1,069 lines | Medium | `north ops strategy replay` and `north ops strategy refusals` re-decide logged cases |
| 11. `north-climb` | 915 lines | Medium | `north ops climb run` proposes the week's policy candidate |
| 12. `north-context-pack` | 267 lines | Medium | `north work brief pack` and its settled experiment arm |
| 13. Command-surface audit | 308 commands | Medium | `src/cli/reference.txt` lists 308 command lines; 39 command entries have no literal mention elsewhere, most reached through a timer enum, so the box counts them from 30 days of transcripts and unit Exec lines instead |
| 14. Capability retirements if the bar is kept | 5,123 to 21,727 lines each | High | `north-code-health` 5,123 lines, `north-diff-gates` 4,117, the change graph (`north-graph` plus `graph-ingest`, `graph-change` and `graph-ground`) 9,394, `north-pacing` 3,973, `north-guard` 21,727; each removes a check or a routing input Tom relies on |

Cuts 1 to 4 are safe and land without a decision. Cuts 5 to 12 retire a capability; each is one box and each
waits for Tom's answer, because deleting a feature is his call, not a worker's. Cut 13 measures usage before
any command is retired, and cut 14 exists only so the arithmetic below is honest about what the issue's bar
would actually take.

## How the counts were taken

Every number in the table comes from the tree at this commit, not from an estimate. `git ls-files '*.rs' |
xargs cat | wc -l` gives the 290,565 total, and the same count per crate gives each feature's lines. A file is
a test file when it sits under `tests/`, ends in `_tests.rs`, `tests.rs` or `_test.rs`, or holds an inline test
module whose `mod` follows `#[cfg(test)]`; those lines are the 49,745 test lines. A function body runs from
`fn NAME` to the brace that closes it. Duplicate bodies are equal bodies in two files after comments are
dropped and whitespace is collapsed; unreferenced functions have one occurrence of their name across the tree.
`#[test]` marks the 1,344 test functions. A lane re-runs each count on its own base before filing a box,
because a landing moves the total and the box's bar must match the tree it lands on.

## The arithmetic

The safe cuts remove 494 + 246 + 213 + 12,000 = 12,953 lines, 16.5% of the bar. Adding all eight small
features in cuts 5 to 12, 20,113 lines together, reaches 33,066 lines and still leaves 45,588. Reaching
78,654 requires retiring capabilities on the scale of cut 14, or most of the test suite plus cut 14; either
way it is a project-scope choice.

Recommendation: land cuts 1 to 4 first, then let Tom name the features in cuts 5 to 14 he wants gone and set
the line bar from that list. A 25% bar with no named features can only be met by deleting working checks and
tests, which is the opposite of a leaner north.

## Why each cut is safe to size this way

- Cuts 1 and 2 have a mechanical oracle: the function name or the duplicate body either remains or not.
- Cut 3 keeps every guard's policy and its tests; it replaces repeated plumbing, not decisions. A guard whose
  rule needs its own shape stays as it is and is named in the box's comment.
- Cut 4 deletes a test only when an existing journey test pins the same guarantee, and names that test in the
  commit message, so a reviewer can see the guarantee still has one. The verification budget (`test_admission`)
  counts tests added, not deleted, so the cut does not weaken it.
- Cuts 5 to 12 each name their command and callers, so the box can update the deployed unit, the timer or the
  docs in the same landing; `north land scan publication` and `north bin-references` refuse a lane that leaves
  a caller behind.
- Cut 13 measures before it deletes: the ledger host holds the 30 days of transcripts and the timers' unit
  lines, and the box files one measured count per command before any retirement.

## Boxes

Paste these onto north#970; the issue's own text is edited by whoever holds the GitHub credential.

- [ ] Cut 1, dead and duplicate helpers: delete the 46 functions with one occurrence that the cut confirms unreferenced, and the 8 byte-identical bodies. Done when: the cut's commit removes at least 400 lines by `git show --numstat` and clippy for every touched crate is green.
- [ ] Cut 2, tests that assert nothing: delete the 26 `#[test]` functions with no assertion, or give each the assertion its name claims. Done when: the listed functions are gone or assert, and `cargo nextest run --workspace` and the land checks are green.
- [ ] Cut 3, guard rule boilerplate: one decision shape for the 54 `north-guard` rule modules and one shared search-option table, removing 213 lines (`21,700` to `21,487`). Done when: `test $(git -C /home/tom/code/north/main ls-files "crates/north-guard/*.rs" | xargs -d "\n" cat | wc -l) -le 21487` exits 0 and every guard test passes.
- [ ] Cut 4, test-guarantee audit: delete 12,000 lines of tests whose guarantee an existing journey test already pins, naming that test per deletion. Done when: the cut's commits remove at least 12,000 test lines and `north land scan test-admission` and `north land scan test-integrity` are green.
- [ ] Cut 5, `north-convo`: retire transcript search, incident mining and their timer callers, 7,665 lines. Needs Tom's decision.
- [ ] Cut 6, `north-tui`: retire the fleet screen, 3,156 lines. Needs Tom's decision.
- [ ] Cut 7, `north-frontier`: retire the frontier scoreboard, 2,800 lines. Needs Tom's decision.
- [ ] Cut 8, `north-lander-doctor`: retire the lander drill and doctor, 2,385 lines. Needs Tom's decision.
- [ ] Cut 9, `north-tune`: retire the daily learning tick, 1,856 lines. Needs Tom's decision.
- [ ] Cut 10, `north-replay`: retire strategy replay, 1,069 lines. Needs Tom's decision.
- [ ] Cut 11, `north-climb`: retire the weekly policy candidate, 915 lines. Needs Tom's decision.
- [ ] Cut 12, `north-context-pack`: retire the brief pack and its experiment arm, 267 lines. Needs Tom's decision.
- [ ] Cut 13, command-surface audit: count every `north` command in 30 days of transcripts and in every unit Exec line and hook, and file one measured count per command. Done when: the count table is on this issue and every command reading 0 has a retirement box.
- [ ] Cut 14, the bar: with Tom's named capabilities from cuts 5 to 12, reach at most 211,911 Rust lines. Done when: `git -C /home/tom/code/north/main ls-files '*.rs' | xargs cat | wc -l` is at most 211,911 with Cargo and Harness green.

## Retired mechanisms

Rust lines by `git ls-files "*.rs" | xargs -d "\n" cat | wc -l`: 312,286 on origin/main before the first retirement below (2026-11-10), 309,216 after the last (bb36adcae); the 8-command retirement landed earlier and is counted in neither figure.

| mechanism | measured gain from the ledger | commit | state |
|---|---|---|---|
| north-climb, the policy climber (922 lines, `ops climb`) | subsystem ledger 2026-11-07: 9 fix commits, 0 decisions, 0 harness-loss rows in 14 days; its run rejected every call without --dry-run | bce9c6040 | retired |
| north-frontier, the frontier scoreboard (2,770 lines, `north test frontier`, `north dash --frontier`) | subsystem ledger 2026-11-07: 13 fix commits, 0 decisions in 14 days | bb36adcae | retired |
| 8 commands with no use (`check provenance audit`, `check provenance inventory`, `config agents usage`, `graph ground hook`, `graph ground import`, `ledger incident import`, `runtime agy`, `test frontier oracle`) | no subsystem-ledger row; command-usage-2026-11.md counts 0 uses of each in 30 days | bef1590fb | retired |

Still open: a third ledger-ranked crate. north-lander-doctor and north-replay each feed a live lander gate or health loop, and north-context-pack has no ledger row, so none retires without a call on its gate.

## Clean-up of this plan

This note ships no test, fixture or script: the counts come from the tree and the commands in it, so there is
nothing to turn down. When cuts 1 to 4 land, each box deletes its own scaffolding with it; no TODO or
half-removed path is left in this note. The `north docs rationale-index --check` land check keeps the index
beside it current.
