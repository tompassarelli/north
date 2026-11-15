# Rust refactor lessons: the failure modes a crate split meets

Owner: north#1487

The north-ops split and the crate splits of north#1276 and north#1435 moved about 20,000 lines between crates and retired a verb family. This note lists the failure modes those lanes met, each with the record that shows it, its cause, and its mitigations ranked the way north fixes a mistake: by construction first, then a check, then a test, and prose last. Read at commit a7d50adda1.

## 1. An emptied crate is still named where the lane did not look

**Evidence.** `5f2fd711a3` drops an unused `north_ops::ops` import at `src/main.rs:21` that turned main red on clippy's `-D warnings` from the moment `48d0e742e5` landed: the group 5 move left `north-ops` holding only `stale_issues`, which the root reaches through `north-issue-boxes`.

**Cause.** The lane checked the crates it moved, not the crate that named the emptied one. A move changes the dependency graph of every crate that imported the old path, and the root package is such a crate.

**Mitigation.**
- By construction: move with `north code-health split move-module` (recorded as `north split move-module` in `8d2a145292`), which rewrites every in-tree caller, and delete the emptied crate's surface in the same landing, so a leftover path is a compile error rather than an unused import.
- Check: run clippy at `-D warnings` over the crates that name the moved one, not only over the crates moved.
- Test: none; the compiler is the test.

## 2. An after-landing run judges another lane's break, and a revert races the fix

**Evidence.** `dd8fefd965` reverts the group 5 move because two post-land checks failed: `north check experiments` and workspace `cargo clippy`. `97e6f41bf9` records that `check experiments` had refused on main since `bb72d81f93`, another lane's landing. The clippy cause was fixed at 20:24:55 by `5f2fd711a3`; the revert landed at 20:31:49, seven minutes after the fix was on main; `97e6f41bf9` fixed the registry at 20:34:12; and `56979b20f7` relanded the move at 20:55:27 with the reason "the sheriff reverted an already-fixed commit". One move became five commits, and main spent the 52 minutes before the fix red on clippy.

**Cause.** After-landing checks judge main's whole tree, so a revert names the last commit that landed rather than the change that broke the check; and the revert landed after the move's own fix was already on main.

**Mitigation.**
- By construction: judge a lane's own change before the push (the lane gates of `521411db26`), so a red from another lane cannot accuse this one.
- By construction: revert from blame rather than recency, as the change graph answers it (north#1039), and never revert a commit whose repair is already on main.
- Check: a revert runs the same land checks as any lane, and a fix that only exists because of a landed move lands in that move's commit (see 3).

## 3. A follow-up that belongs to the move makes the move unrevertable

**Evidence.** `4367cb3d93` restores the `north_ops::ops` import that the revert of `48d0e742e5` needed, because the revert "brought back `crate::ops` uses in `src/cli/remote_serve.rs` and `src/cli/tools/subverbs.rs`, but left `5f2fd711a3`'s removal of the import that served them". Main failed to compile until that lane landed.

**Cause.** `5f2fd711a3` was a consequence of the move, landed as its own commit. Reverting the move alone therefore produced a tree neither commit had ever built.

**Mitigation.**
- By construction: land a move together with its consequences (the dropped import, the registry entry, the renamed path), which is also the one-landing-per-change rule in [one-landing-per-change.md](one-landing-per-change.md).
- Check: the reverted tree is compiled by the land check the revert itself runs, before it reaches main.
- Prose: a review of a refactor commit asks what else it makes true, and refuses a consequence landed apart.

## 4. The crate's identity is copied into ten hand-kept places

**Evidence.** The group 7 lane had to drop `north-ops` paths from Cargo, BUCK, `structure.toml`, `harness/merge-sets.toml`, `.gitattributes`, `.stub-exec-allowlist.json`, `harness/defaults.toml` and `tests/it/crates.txt` (issue comment 2026-11-14T14:43Z); `a735cb0326` still found "the stale `.gitattributes` merge-set entry for the deleted `crates/north-ops/src/ops/mod.rs`" while deleting the crate. A new crate meets the same list in reverse, and `dd8fefd965` shows the registry end of it: an entry missing from `docs/rationale/experiments.toml` refuses the landing after it.

**Cause.** A crate's identity is written by hand wherever the workspace enumerates crates, so a refactor that adds or removes one edits each copy and misses some.

**Mitigation.**
- By construction: one file per crate or module, and every registry generated from it; `59f27fc3d7` did this for the command and module registries (north#902), and `north docs structure-index` and `north buck targets` regenerate their tables from the tree.
- Check: run the generating commands with `--check` in every lane that adds or deletes a crate, beside `north check experiments`.
- Test: a fixture crate with no registry entry is refused, as the experiments check's own test does.

## 5. A retired verb keeps a caller outside the tree

**Evidence.** `41404189c4` retired the replay crate and the four `north ops strategy` verbs. `3958421e63` restores `north ops strategy log retain` because the hourly `north-strategy-log-retain` timer from nixos-config's `north-runtime-follow` module exited 2 from 04:00 UTC on 2026-11-15 until the restore at 05:27 UTC, while placement, admission and the lander still write the decision log, which grows about 0.5 MB an hour.

**Cause.** A verb's callers are the tree, the host units, south's routines, the skills and the goals' oracles. Only the tree fails to compile.

**Mitigation.**
- By construction: move the unit and the verb in one change, as retention now lives in `north-timers` with the other timer jobs.
- Check: the bin-references scan already reads south's units, hosts and bin and the goals' oracles for a verb a landing removes (`e590cde5e2`, north#703); a retirement lane runs it.
- Test: a fixture unit whose `ExecStart` names a removed verb is refused.

## 6. A large move lane waits, goes stale, and is dropped

**Evidence.** The `ops-split-g7` lane (head `73d670b3b1`) was refused with a conflict in the issue-box crate's stale-issue module after `3365b946f` changed that file (issue comment 2026-11-14T14:43Z). The 2026-11-15 lane sweep archived it as stale, its change not on main; `a735cb0326` redid the deletion the next day.

**Cause.** The deletion touched files active lanes owned, and it waited in the queue long enough for one of them to land first.

**Mitigation.**
- By construction: delete the emptied crate in the landing that moves its last module, so no shell exists for a later lane to conflict with and no second deletion is needed.
- By construction: keep each lane's file set disjoint from active lanes by splitting along module edges, as the map in [north-ops-split.md](north-ops-split.md) does.
- Check: the lane sweep already reports a lane whose change conflicts or is already on main; read it as a signal to hand in sooner, not later.

## 7. A re-export keeps the old path alive

**Evidence.** The map's rule is that "no re-export of the old `north_ops::ops::` paths survives" ([north-ops-split.md](north-ops-split.md)). `021ea25a9f` and `2b12a0ffcb` moved code but re-exported it from the old crate "so guard's `crate::lanes` paths are unchanged". `8d2a145292` shows the bill: dependents that reached the moved modules through `north-ops` kept it a hub, and a third dependent would have dropped its line cap from 25,000 to 10,000. North#1487's L49 box exists to remove the plain re-exports left in 34 crate roots.

**Cause.** A compatibility re-export hides every caller from the compiler and leaves the grab-bag's dependencies standing, so the move costs a crate and buys no rebuild.

**Mitigation.**
- By construction: no re-export; each dependent's manifest names the crate that owns the module, in the same lane as the move.
- Check: a code-health rule refusing `pub use OTHER_CRATE::*` in a crate root, which is what the L49 box's command measures.
- Test: a fixture crate root re-exporting another crate is refused.

## 8. The layer check cannot see a path written inline

**Evidence.** `e7540f1656` records that `imports_of` read imports from `use` lines alone, so a call written as `north_base::platform::ssh::x()` made no edge and the report printed 0 layer violations for references the module map forbids; the 2026-11-07 platform split review found four such calls by hand, among them `ssh_master.rs:128` to process.

**Cause.** The graph read one syntax for an edge. A move that leaves a qualified path in place therefore passes the DAG check while still reaching across it.

**Mitigation.**
- By construction: the checker reads inline qualified paths too, over text with strings and comments masked, and both callers share one scanner (done in `e7540f1656`).
- Check: every refactor lane runs `north code-health layers origin/main HEAD`.
- Test: the case that made it visible, `ssh_master.rs:128`, is asserted to resolve to process while a path inside a string does not.

## 9. A test reaches another crate's module and breaches the DAG after landing

**Evidence.** `62025a79c` landed and its after-landing layers check failed, because its test called the debt writer in `north-landing` from `north-graph-ingest`, which `structure.toml`'s crate DAG forbids; `7015d5c452` relanded it with the test rewritten inside `north-graph-ingest`'s own layer.

**Cause.** A dev-dependency or a test's reach can cross a layer without the crate's ordinary imports showing it, and only an after-landing check read the edge.

**Mitigation.**
- By construction: count test imports in the DAG, and make the layers check a lane gate, which north#1487's L03 and L05 boxes do.
- Check: `north code-health layers origin/main HEAD` before hand-in, over the tests as well as the crate sources.
- Test: a fixture lane that adds an upward manifest edge is refused by that check, the L03 box's own test.

## 10. A path-keyed count outlives the rename

**Evidence.** `06a98d48f7` moves four desktop modules out of `north-ops` and has to "follow a renamed test file's ambient-test-input count".

**Cause.** `ambient-test-inputs.tsv` keys a count by file path, so a move that renames a test file leaves a count that no longer reads that file, and the check that compares the two sees a stale row.

**Mitigation.**
- By construction: generate the table from the tree, as its reader already walks the test files, so a rename rewrites its row.
- Check: the ambient-input check in the lane, which is what caught this one.
- Test: rename a fixture test and assert its row follows.

## 11. A trust-boundary slice serialises the conversion

**Evidence.** North#1487's C3a box says its slice changes trust-boundary paths (`Cargo.toml`, `Cargo.lock`, the crate's `BUCK`, `third-party/BUCK`, `third-party/fixups/north/fixups.toml`), so two reviewer-role sessions that did not author it must accept its patch before the lane can be handed in; C3b to C3e then wait on C3a, each dated "not before 2026-11-16T12:00Z, a guess at when C3a's reviews land".

**Cause.** The dependency change and the first code conversion share a lane, so every later conversion waits for the slowest gate on the first.

**Mitigation.**
- By construction: land the manifest change (the parsing library, `BUCK`, the lock) alone as the first slice, so each conversion lane touches source only and passes the ordinary gates.
- Check: the golden test over each verb's `--help` and usage-error output, captured from the old code, run on the lane that converts it (C2's Done-when).
- Test: the same golden test, which is the test.

## Wisp

The only record of Wisp's Rust layout in this history is its commit `5bbe900d32` (2026-11-05), which added the crate-size and build-time gates to Wisp's `structure.toml` (north#259, "Split north-core, box 13"). Wisp therefore measures the two quantities every lesson above turns on: how many lines each crate holds, and how long a build takes. No record here shows Wisp running a crate split or meeting any failure mode above, so this note claims none for it and says nothing beyond those two measurements about what its gates would catch. When Wisp does split a crate, the order this note ranks is where to start: move where the compiler can see every caller, delete the emptied crate in the same landing, keep no re-export, and take both measurements before and after.
