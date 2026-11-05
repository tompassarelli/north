# Docs architecture

Tom asked on 2026-11-04 for north docs that resist rot, disclose detail progressively and help
agents find the right fact without reading the whole tree. This note is the target layout and the
boxes that reach it. It builds on two lanes in flight that day: the features index (one file per
feature under `docs/features/`, with `docs/features.md` generated from them and checked at landing)
and the landing-train merge driver (`north merge-driver set`, docs/rationale/landing-train.md
step 9b).

## What was measured

- `docs/features.md` held 92 entries on 143 lines; 27 entries were over 1,000 characters and 14 over
  2,000, the longest 6,622. Every feature lane edited the same lines, and rebase conflicts there
  refused several landings on 2026-11-04. It changed in 15 commits from 2026-11-01 to 2026-11-04,
  `docs/feedback-loop.md` in 13.
- `docs/harness.md` restates flags, timeouts and thresholds already owned by `--help` and
  `harness/defaults.toml` (for example "6 free cores", "48 h", "every 2 minutes"), in numbered steps
  of up to about 2,000 characters.
- The grounding index (`crates/north-core/src/grounding/sources/repos.rs`) splits each `.md` file into
  lines, keys a line by the file-path suffixes it names (never a bare directory prefix), drops a
  line with fewer than four words of three or more letters, cuts a long line to a 200-character
  window around the mention and labels it with the file's last two path parts. A short single-topic
  file therefore retrieves under a meaningful label (`03-fleet-sessions/0003-fleet-spool.md`); a
  4,000-character bullet retrieves as a 200-character fragment of something else, and a terse
  "path: doc" router line is not indexed at all.
- `crates/north-core/src/health/loops.rs` reads `docs/feedback-loop.md` and checks its "File
  signals" and "Still unread" sections, so that file and those headings must stay.
- `north citation-check` already fails on a doc, skill or hook citing a `north` subcommand the CLI
  does not define, but it ran only inside `cargo test` and `north health`. `.safe-push` runs the cargo
  tests only when code paths change, so a docs-only landing was never checked, and a doc naming a
  deleted file or subcommand failed the next unrelated code landing instead.
- A probe of backticked repository paths and relative links found living docs naming deleted files
  (`harness/scripts/routing-backtest.py` in feedback-loop.md, `harness/scripts/codex-route-propose.py`
  in python-port-order.md). Rationale notes name many removed paths, correctly: they record a past
  state.

## Reading order

An agent reads in three rings and stops at the first that answers.

1. Always: `AGENTS.md`, the agent's entry. It holds the rules for working in the repository and an
   area router: one line per code area (a path prefix) naming the doc to read before touching it.
   `README.md` is the human front door: what north is, how to try it, and the same map in prose.
2. When touching an area: the doc the router names. Concept docs (`architecture.md`, `harness.md`,
   `terms.md`, `feedback-loop.md`) explain how parts fit and link to owners; they never restate a
   flag, default or threshold. The feature entry under `docs/features/` says what one command is for.
3. On demand: the owner of the exact fact. `north --help` and `north SUB --help` for commands and
   flags, `harness/defaults.toml` for every tuned number with its unit, source and consumers,
   `.safe-push` for land checks, `structure.toml` for module boundaries, a skill's `SKILL.md` for a
   procedure, `docs/rationale/` for why, `docs/references.md` for outside sources.

## Where each kind of fact lives

| Fact | Home | Docs may |
| --- | --- | --- |
| Commands and flags | the command's usage text (`src/cli/help.txt`, each module's usage string) | name a command or flag; checked to exist |
| Tuned numbers and thresholds | `harness/defaults.toml` (`value`, `unit`, `source`, `consumers`) | name the key, never the value |
| Land checks | `.safe-push` | name the check; `safe-push --help` is the contract |
| Module boundaries | `structure.toml`, then `architecture.md` for the picture | link the path |
| What a feature is for | one file per feature under `docs/features/<NN-section>/` | be one short paragraph; detail goes to `--help` |
| Feature index | `docs/features.md`, generated | never be edited by hand |
| How parts fit | concept docs under `docs/` | link owners; no numbers |
| Procedures | skills under `harness/skills/` and `agent-machinery/skills/` | link the skill |
| Why, with dates and measurements | `docs/rationale/<topic>.md` | quote numbers as measured on a date |
| Outside sources | `docs/references.md` | cite |
| Tom-specific facts | south `agents/decisions` | not appear in north |

Rationale notes are dated records: they keep the paths and numbers of their day, are exempt from
the existence checks, and are superseded by a new dated section, not rewritten. Code comments are
not a docs home in this repository (Tom's code rule allows only one-line format or determinism
constraints), so "next to the code" means usage text and `defaults.toml` entries.

## Generated and checked

| Fact | Generated by | Checked by |
| --- | --- | --- |
| Feature index | `north docs features-index` from `docs/features/` | `north docs features-index --check` (land check) |
| North subcommands named in docs, skills and hooks | | `north citation-check` (land check, this box) |
| Repository paths named in docs | | `north citation-check` (land check, this box) |
| Flags named with a `north` subcommand | | `north citation-check` (box 4) |
| `harness/defaults.toml` and `.safe-push` keys named in docs | | `north citation-check` (box 5) |
| Every living doc reachable from `AGENTS.md` or `README.md` | | `north citation-check` (box 2) |
| Rationale index | `north docs rationale-index` from each note's title and first sentence | `--check` (box 6) |
| No versions, dates, counts or model ids in the README | | `tests/readme_links.rs` |

Rejected, one line each:

- A generated `docs/cli.md` copy of `north --help`: a second copy of the usage text that adds
  nothing the router's pointer to `north --help` does not give.
- A generated config reference from `defaults.toml`: the file already carries value, unit, source
  and consumers per key and reads well as is.
- Rust module docs (`//!`) as the "next to the code" home: Tom's code rule forbids them.
- A separate docs linter binary or script: `north citation-check` already scans the right roots and
  runs in `cargo test` and `north health`; one checker keeps one list of roots and exemptions.
- Checking a flag against the exact subcommand's parser: north subcommands parse arguments by hand
  with no shared registry, so box 4 checks that the flag exists as a string literal in the code,
  which catches removal and renames.

## The land check

`north citation-check --root "$PWD"` runs as a `land.check` on every landing (`**`), so a docs-only
landing is checked and a code landing that deletes a named file or subcommand is refused in its own
lane rather than in the next unrelated one. `harness/scripts/citation-check.sh` runs it
with the held `north` runtime when the lane leaves `src/cli`, the checker and `Cargo.lock` as that
runtime has them (the pattern of `harness/scripts/check-owners.sh`), so a docs landing builds
nothing; otherwise it builds the lane's `north`. It is `land.local-only` because the held runtime
exists only on a host.
The same scan runs in `cargo test` (`src/cli/tests.rs`) and `north health`. It fails, naming
`file:line`, on:

- a `north SUB` in a code span or fence where the CLI does not define `SUB` (existing);
- a relative Markdown link in any scanned `.md` file whose target is not a tracked file or
  directory; a link between skills resolves in either skill root, because `agents sync` publishes
  `harness/skills/` and `agent-machinery/skills/` side by side;
- a backticked repository path (one starting with a tracked top-level directory such as `bin/`,
  `crates/`, `docs/` or `harness/`) in `README.md`, `AGENTS.md` or a living doc under `docs/` that
  is tracked neither from the repository root nor from the doc's directory. Placeholders (an
  upper-case segment such as `NAME`, `<...>`, `*`, `{`) are skipped.

Existence is judged against `git ls-files`, not the disk, so an untracked local file never passes
here and fails on another host. Skills are scanned for links but not backticked paths, because a
skill names files in other repositories (makgora, wisp) that look like north paths.
`docs/rationale/` is not scanned; `docs/references/` is scanned for links only; `docs/features/`
is not scanned because its links are written relative to the generated `docs/features.md`, which
is.

## Boxes

Each box is one lane landed through `safe-push --to main`. Code boxes that extend
`crates/north-core/src/ops/citation_check/` run in sequence (4, then 5, then 2b) because they edit the
same module; content boxes run in parallel with them and with each other. Box 3 waits for the
features-index lane, box 6 for box 3d and the merge-driver lane.

| Box | Files | Runs after |
| --- | --- | --- |
| 1 | citation_check, `src/cli/tests.rs`, `.safe-push`, `harness/scripts/citation-check.sh`, README, two living docs | landed with this note |
| 2a | `AGENTS.md` `## Routers` only | 1 |
| 2b, 4, 5 | citation_check | 1, in the order 4, 5, 2b |
| 3a, 3b, 3c | `docs/features/` sections | features-index lane |
| 3d | features_index | 3a, 3b, 3c |
| 6 | `north docs`, merge driver, `.gitattributes` | 3d, merge-driver lane |
| 7 | `docs/harness.md`, `docs/feedback-loop.md`, `tests/readme_links.rs`, a new rationale note | 1 |

1. **Front door and land check (landed with this note).** Extend `north citation-check` to links and
   backticked paths as above, add its land check, fix the rot it finds in living docs, and make
   `README.md` the human front door with the three reading rings. Done when: `north citation-check
   --root .` exits 0 on main; the unit test
   `the_command_exits_one_on_a_missing_path_and_zero_once_it_exists` passes (a tracked `docs/x.md`
   naming `bin/gone` exits 1, and 0 once `bin/gone` is tracked); `git config --file .safe-push
   --get-all land.check` lists the `**` citation-check line.
2a. **Agent router (20 min).** Rewrite only the `## Routers` section of `AGENTS.md` (the provenance
   gate, `crates/north-landing/src/provenance.rs`, exempts that section alone) as one sentence per
   area that names a concrete file, so the grounding index keys it: for example "Before changing
   `bin/safe-push` or `crates/north-landing/src/landing/mod.rs`, read docs/harness.md#landing-a-change".
   Cover landing, fleet and TUI, capacity, hooks and skills, policy, grounding, the ledger. Done
   when: `north citation-check --root .` exits 0; `AGENTS.md` is under 45 lines (`wc -l`); every
   router line has at least four words of three or more letters besides its paths.
2b. **Reachability and anchors (30 min).** Make `citation-check` fail on a `docs/*.md` outside
   rationale that neither `README.md` nor `AGENTS.md` links, and on a link `file.md#anchor` whose
   file has no heading with that GitHub slug. Done when: `north citation-check --root .` exits 0;
   unit tests show an unlinked `docs/orphan.md` and a link to `terms.md#no-such-heading` each exit 1
   naming the file.
3a, 3b, 3c. **Short feature entries (30 to 40 min each, in parallel).** Cut each entry file over
   1,200 characters in the box's sections to one paragraph on what the feature is for and which
   command runs it, moving flag lists into the command's usage text, leaving thresholds to their
   `harness/defaults.toml` key, or splitting one entry into several files. 3a: section 03 (10 oversize
   entries); 3b: sections 02 and 08 (7); 3c: the rest (6). Never add text to `docs/harness.md` (box 7
   owns it). Done when: `find docs/features/<sections> -name '[0-9]*.md' -size +1200c` prints nothing;
   `north docs features-index --check` and `north citation-check --root .` exit 0.
3d. **Entry cap (20 min).** `north docs features-index --check` fails on an entry file over 1,200
   characters, with no exemption list. Done when: it exits 0 on main and exits 1 naming the file on
   a scratch copy with one 1,300-character entry.
4. **Flags exist (25 min).** A cited `north SUB ... --flag` must occur as the string literal
   `"--flag"` somewhere in `src/` or `crates/`. The review probe found one miss in forty cited flags
   (`--durable-harness-paths`); fix the doc. Done when: `north citation-check --root .` exits 0; a
   unit test citing `north health --no-such-flag` exits 1 naming that flag.
5. **Config keys exist (30 min).** First measure: list backticked `[table]`, `[table] key` and
   `table.key` names in living docs by owning file (`harness/defaults.toml`, `.safe-push`,
   `structure.toml`, host configuration). Check only the owners north holds: a named
   `harness/defaults.toml` table, or a `.safe-push` section and key, must exist. Done when:
   `north citation-check --root .` exits 0; a unit test naming `[no_such_default]` exits 1.
6. **Regenerated indexes never conflict (35 min).** Add a `regenerate` kind to the merge driver
   (`harness/merge-sets.toml`, `merge_set.rs`) that resolves a conflicted generated file by running
   its generator on the merged sources, register `docs/features.md` with it in `.gitattributes`, and
   add `north docs rationale-index` writing `docs/rationale/README.md` (one line per note: title, link,
   first sentence) with `--check` as a land check, registered the same way. Done when: in a scratch
   repository, rebasing a lane that adds a feature file onto a main that added the neighbouring one
   completes with no conflict and `north docs features-index --check` exits 0;
   `north docs rationale-index --check` exits 0 on main.
   Landed design: git runs a merge driver before it writes the merged tree, so the driver cannot read the
   merged sources; it leaves a `regenerate` file conflicted and `north merge-driver continue` (run by
   safe-push, its train and the lander after a stopped rebase or cherry-pick) regenerates it from the
   merged worktree, stages it and continues, one commit at a time. Lanes that change sources without
   touching the generated file never conflict, so the landing candidate also reruns the generator after
   each such pick and folds the result into that lane (`merge_continue::refresh`); a batch of lanes that
   each add a rationale note then lands with a fresh index whatever their order.
   The rationale-index land check landed one step later, once the lander ran a runtime with refresh:
   the batch that brought refresh was still picked by a lander without it, and four batch-mates adding
   notes went stale against the check on 2026-11-05.
7. **Concept docs carry no dates or versions (35 min).** Cut `docs/harness.md` to the flow with links
   to owners, replacing restated flags, timeouts and thresholds with the `harness/defaults.toml` key,
   the command's `--help` or a link to its feature entry; move the dated "Learned routing backtest"
   and "Goal ETA" sections of `docs/feedback-loop.md` to a new `docs/rationale/` note, keeping "File
   signals" and "Still unread" in place for `health/loops.rs` and fixing the links into the moved
   sections (`docs/references.md` links them). Then apply the date, version, percentage and model-id
   rules of `tests/readme_links.rs` (not the count rule: list numerals and quantities are prose) to
   `docs/harness.md`, `docs/architecture.md` and `docs/terms.md`. Done when: `cargo test --test
   readme_links` passes with those files covered; `awk 'BEGIN{RS=""} length > 900'
   docs/harness.md` prints no paragraph; `north citation-check --root .` exits 0.

Not required: a docs website, a prose style linter, rewriting rationale notes, moving
`docs/python-port-order.md` (it is a living plan until the port ends), or moving south's docs.

## Review

A fresh reviewer on a different tier tried to break the first draft on 2026-11-04. Accepted and
folded in above: the merge driver had no kind that can regenerate a file (box 6 now adds one and
waits for that lane); three boxes edited the same checker module (now serial); the count rule
would fail every concept doc (box 7 drops it); `health/loops.rs` reads `docs/feedback-loop.md`
sections (kept); anchors were unchecked (box 2b); grounding keys file suffixes, not directory
prefixes, and drops short lines (router lines are now sentences naming files); the trigger list
missed `lib/`, `tests/` and other roots (now `**`); the check would build north on every landing
(now the held runtime, as check-owners does); box 3's split was unbalanced and its exemption list
could grow (rebalanced, cap added after with no list); the line-length measure was gameable
(paragraph measure); `AGENTS.md` edits outside `## Routers` need provenance (box 2a stays inside).
