# Docs audit, November 2026

Every tracked file under `docs/`, plus `README.md` and `AGENTS.md`, listed once with a verdict and the evidence checked, so that other workers can carry the verdicts out before north is shown more widely (north#1281, box "Docs audit"). The tree was read at commit c013d6124c on 2026-11-12. Nothing was rewritten in this change; this file is the only one it adds. Two changes that landed while it was written are folded into the rows: the logo became one `.jpeg` (commit 2edb3994b7) and `docs/rationale/operator-repository.md` was added (commit cda5987959).

Owner: north#1281

## How to read a row

Each row is one path, one verdict in bold, then the evidence and what to do.

- **keep**: the page stays where it is and its body stands. A row may carry `Fix:` items, each a correction of a sentence or less.
- **rewrite**: the body is wrong or badly shaped; the row says what is wrong.
- **merge**: the content moves into the page the row names, and the file goes.
- **delete**: the file goes; the row says why nothing is lost.

A worker who carries a row out appends `Done: COMMIT` to it, which is what the box "Docs carried out" of north#1281 checks.

## Counts

412 paths: the 411 that `git ls-files docs README.md AGENTS.md` prints once those two changes are counted, and this file.

- keep: 365
- rewrite: 29
- merge: 13
- delete: 5

By directory, for the 411: 276 files under `docs/features/` (265 records, 10 section titles, 1 header), 124 under `docs/rationale/` (91 notes, one of them added after the tree was read, their index, 2 registries, 30 files in five subdirectories), 9 other files under `docs/`, and the two root files.

## How it was checked

- **Commands.** The help text came from the installed runtime at d8b8c6bae2, 22 commits behind the tree read; between the two the command reference gained one line (`north land scan turn-scope`), which no page names. `north --help` lists 27 commands and `north --help-all` prints 339 lines. Every `north VERB` and `north VERB SUBVERB` a page writes in backticks was looked up in `north --help-all` and in `north VERB --help`. A verb the help missed was then looked up in the dispatcher (`src/cli/verbs/`, `src/cli/fleet/router.rs`, `crates/north-lander/src/land/commands.rs` and the owning crate's command table) to tell a verb that runs but is unlisted from one that is gone.
- **Bare commands.** Names such as `agents` and `threads` were compared with `ls bin` and with the files `git log --diff-filter=D -- bin` shows were deleted.
- **Paths and links.** Every backticked repository path was compared with `git ls-files`, and every relative link was resolved, a feature record's links from `docs/` because the index is written there.
- **Age.** `git log --follow` gave each file's first and last commit date.
- **Private detail.** Each file was counted for the owner's name, machine names, dollar figures and account labels. The counts are in the rows as input for the box "Privacy sweep"; nothing was judged private here.
- **Behaviour.** Existence was checked for every page. Whether a command behaves as its page says was read from source only where a row says so. No command with an effect was run, and nothing was built or tested.

`north check citations --root .` exits 0 on this tree. It reads the first word after `north` only, does not read `docs/rationale/`, and reads `docs/features/` for commands but not for paths or links, so its pass covers little of what the rows below report.

## What is wrong across the tree

1. **The commands a newcomer is told to type are not in the repository.** `bin/` held `agents`, `threads` and `safe-push` until 2026-11-06 (commit 4dd7f6aea0). Today `safe-push` is a link to the north binary that the runtime installer writes, and `agents` and `threads` resolve on the operator's machine through a repository outside this one. `README.md`, `docs/harness.md`, `docs/terms.md` and 39 feature records still write them bare (`threads` in 19 records, `agents` in 14). The help lists `north threads ARGV` and `north config agents`.
2. **The help lists a subset of what runs.** 45 feature records name 42 distinct verbs, and `docs/graph.md` one more, that neither `north --help-all` nor the verb's own `--help` lists. Each one traced is in the dispatcher: `north fleet need`, `north land profile`, `north land doctor`, `north runtime settle`, `north docs verbs-index` and `north proxy` are examples. `north runtime --help` lists 10 commands; the usage text the same verb prints on an error also names `settle`, `status`, `build`, `mirror`, `checkouts`, `follow` and `build-ahead`. The box "Docs carried out" asks that no page name a verb the help does not list, so most of these rows are closed by adding the help line, not by editing the page.
3. **The features index cannot be opened.** `README.md` links `docs/features/` as "what each command does". The index, `docs/features.md`, has been untracked since 2026-11-11 (commit 59f27fc3d7, `.gitignore` line 8), so a reader on GitHub or in a fresh clone finds ten directories of one-bullet files and no page.
4. **Nothing takes a newcomer from the README to a landed change.** "Try it" ends at opening the screen. Install, first landed change, running a fleet and safety have no page (see the reading order).
5. **Pages describe one operator's machines.** `docs/nexus-run.md` is about one machine and another project's assets. 48 feature records name the owner or his machines where a role would do, and `docs/harness.md` Install is written for his layout.
6. **Feature records have grown past their purpose.** `docs/rationale/docs-architecture.md` asks for one short paragraph per record. 22 records are over 2,000 bytes, the longest 9,413, each as a single bullet. 22 section and number pairs are shared by two or more records, and 55 file names carry an issue number.
7. **The rationale index does not sort.** It lists 90 notes alphabetically. For 23 the summary is only an owner line or a date, and plans, measurements, live mechanisms and retired designs look alike. Two notes open with "Nothing here is built yet" although parts are built.
8. **One check named by the docs does not exist.** `docs/rationale/docs-architecture.md` says `tests/readme_links.rs` keeps versions, dates and counts out of the README; no such file is tracked.

## Subjects told more than once

The page named first survives as the one a reader is sent to. The others keep their verdict in the rows; a dated rationale note is a record and stays.

- **Architecture.** Survivor: the root `ARCHITECTURE.md`. `docs/architecture.md` merges into it. `docs/rationale/architecture-layers.md` and `docs/rationale/crate-split.md` stay as the plan and the record.
- **Landing.** Survivor: a landing page cut from `docs/harness.md` "Landing a change". Also told in 66 files under `docs/features/04-landing-and-lanes/` and in `docs/rationale/landing-train.md`, `conflict-free-landing.md`, `landing-by-construction.md`, `one-landing-per-change.md`, `landing-refusals-shadow.md`, `landing-latency.md`, `landing-queue-p90.md`, `verdict-reuse.md`, `test-selection.md`, `coverage-test-selection.md`, `safe-push-composition.md`, `lander-doctor.md`, `lander-drill.md` and `replay-divergence.md`. Of the notes, `landing-train.md` is the long form.
- **Briefing, staffing and routing.** Survivor: a briefing page cut from `docs/harness.md` "Staffing a worker". Also told in `docs/features/02-staffing-and-routing/` and in `docs/rationale/tier-routing.md` (the survivor among the notes), `nearest-routing.md`, `learned-routing-and-goal-eta.md`, `estimation-audit-2026-11-05.md` and `repeat-runs-2026-11-06.md`.
- **Account usage.** Survivor: `docs/rationale/spend-rule.md`, which supersedes the usage parts of `docs/rationale/account-capacity.md`. `docs/rationale/usage-resets.md` and two feature records (`0009-usage-resets.md`, `0010-usage-read.md`) cover the readings.
- **Capacity and placement.** Survivor: `docs/rationale/placement.md`. Older: `docs/rationale/capacity-scheduler.md`, `dispatch-spread.md`, `pull-hand-over.md`, and `docs/features/08-machines-capacity-and-scheduled-jobs/`.
- **Build time.** Survivor: `docs/rationale/compile-reuse.md`, the current direction. Earlier: `rust-build-times.md`, `crate-split.md`, `build-cost.md`.
- **Runtime promotion.** Survivor: the feature record `0004-runtime-promotion.md`, once cut short. Also told in `docs/harness.md` Install steps 3 and 4, the records `0025-host-checkouts.md` and `0026-runtime-build-ahead.md`, and `docs/rationale/propagation.md`, `runtime-follow.md`, `runtime-install-lock.md` and `mayday-ladder.md`.
- **The fact graph.** Survivor: `docs/graph.md`. Four feature records merge into it. `docs/rationale/world-model.md`, `change-graph.md`, `change-graph-replay.md` and `graph-blame.md` are the decisions and measurements.
- **Learning from records.** Survivor: `docs/rationale/self-improvement-loop.md` for the account, `docs/feedback-loop.md` for the signal table. Also `learning-loops.md`, `measured-no-gain.md`, `policy-climber.md` (retired) and three feature records.
- **The coordinator.** Survivor: `docs/rationale/proxy-role.md`. Also `wake-paths.md`, `page-flood.md`, `lifecycle-deadlines.md`, `close-by-rule-share.md` and seven feature records on the proxy and its window.
- **Nix.** Survivor: `docs/rationale/nix-footguns.md`. Also `nix-current-practice.md` and `nix-leverage.md`.
- **The docs themselves.** Survivor: `docs/rationale/docs-architecture.md`. Also the README sections "Finding a fact" and "Keeping the docs true", three feature records (`0013-features-index.md`, `0021-docs-owners-scan.md`, `0024-doc-links-scan.md`) and this audit.
- **Safety.** No survivor exists. The subject is spread over `docs/rationale/worker-sandbox.md`, `agent-user.md`, `read-isolation.md`, `agent-push-identity.md`, `credentials-registry.md`, `private-text-judge.md`, `betterleaks.md`, `holdlist.md`, `guard-false-refusals.md` and `hooks-and-runtime.md`, and `docs/harness.md` "Where policy lives".

## Reading order for a newcomer

1. **What north is.** `README.md` (rewrite), then `docs/terms.md` (keep).
2. **Install.** Missing. `README.md` "Try it" builds the binary and opens the screen; `docs/harness.md` Install describes the operator's machines. The box "Onboarding" of north#1281 owns the page.
3. **First landed change.** Missing. The nearest material is the feature record `0001-product-verification.md`: `verify-north` walks a brief, a worker, a landed commit and a ledger row on a scratch repository.
4. **How work is briefed and landed.** `docs/harness.md`, sections "Staffing a worker" and "Landing a change" (rewrite, split into a briefing page and a landing page).
5. **How to run a fleet.** Missing as a page. The pieces are `docs/harness.md` "Messages between fleet sessions", the records under `docs/features/03-fleet-sessions/` and `north fleet --help`.
6. **How it is kept safe.** Missing as a page. The sources are the notes listed under "Safety" above.
7. **Reference.** `north --help` and each command's `--help`; the features index, once it can be opened; the root `ARCHITECTURE.md`; `docs/graph.md`; `docs/feedback-loop.md`; `docs/references.md`; `docs/rationale/README.md` for why.

Steps 1 to 6 should be one numbered list in the README and nowhere else. `AGENTS.md` stays the entry for agents and is not a step.

## The ten rewrites that matter most

1. `README.md`: every newcomer starts here, and it names commands the repository does not ship and says north does not install elsewhere.
2. `docs/harness.md`: the only page on briefing and landing; it becomes steps 2, 4 and part of 6.
3. `docs/features/_header.md` and the index it heads: the README's link to "what each command does" opens no page.
4. `docs/architecture.md`: merged into `ARCHITECTURE.md`, so there is one map.
5. `AGENTS.md`: the first file a newcomer's agents read; it tells them to run test files that are not there.
6. `docs/features/08-machines-capacity-and-scheduled-jobs/0004-runtime-promotion.md`: how an install updates itself, as one bullet of 9,413 bytes.
7. `docs/features/03-fleet-sessions/0011-dispatch.md`: how a fleet finds its own work, as one bullet of 5,788 bytes.
8. `docs/features/04-landing-and-lanes/0053-finish-verb.md`: how a worker hands in, as one bullet of 3,438 bytes.
9. `docs/features/03-fleet-sessions/0004-fleet-tick.md`: what runs a fleet between messages, as one bullet of 3,301 bytes.
10. `docs/rationale/README.md`: the index of 90 notes, changed in its generator.

## Feature records and structure records

**Feature records.** A newcomer should not read them as a path. Each is one bullet written by the change that added the behaviour, in landing order, and many assume the operator's setup. They are a lookup: given a command, what is it for. Keep one file per record, since that is what lets two changes add records without a conflict.

**Structure records.** The 234 files under `structure/` at the repository root hold one table each and the build writes `structure.toml` from them. They are outside `docs/`, so they have no row here. A newcomer never needs them; `ARCHITECTURE.md` is their readable form.

**State charts.** The 13 pages under `docs/rationale/state-machines/` are written by `north check machines --write`. A newcomer never needs them; `docs/rationale/state-machines.md` links them.

## What the indexes should do

The features index:

- be a page a reader can open from the README, in a fresh clone and on GitHub. Today it is untracked. `docs/rationale/README.md` shows the other way: tracked, regenerated by the lander, never in a worker's diff;
- be the Reference step of the reading order and nothing earlier;
- order its sections as `north --help` orders its commands, so a reader goes from a command to its record;
- show each record's name and first sentence, without issue numbers;
- give each record a number no other record in its section has.

The rationale index:

- say in its first line that nothing in it is needed to use north;
- group notes by kind: live mechanism, plan, measurement, decision record, review of an outside source, retired. The rows below give each note's kind;
- take its summary from the first sentence that is not an owner line or a date;
- leave out the registries (`experiments.toml`, `discovery-turns.toml`) and the generated charts, as it does today.

## Rows

### Front door, concept pages, references and assets

- `AGENTS.md`: **rewrite**. Agent entry; small fixes only. Wrong today: (1) tells agents to run `*.test.sh` or `*.test.py`, but `git ls-files '*.test.py'` prints nothing and three `*.test.*` files remain under `bin/`, `lib/` and `harness/`; (2) calls `bin/` "the agent commands", where `ls bin` now shows toolchain shims and three tools; (3) the router to `docs/architecture.md#owning-boundary` must follow that page's merge. Checked: all 24 backticked paths are tracked; `north dash`, `north concerns`, `north docs`, `north land`, `north check` are listed by `north --help`; `bin/verify-north` is tracked. Edits outside `## Routers` need a provenance record (docs/rationale/docs-architecture.md, box 2a). Last changed 2026-11-11. Private-detail scan: owner's name 1, machine names 0, dollar figures or account labels 0.
- `docs/architecture.md`: **merge**. Merge into the root `ARCHITECTURE.md` (2026-11-12, seven layers, every crate). This page predates it (first written 2026-08-01) and draws a four-directory picture. Unique content to carry over: the session scope paragraphs (`north-session-<uuid>.scope`, `north session-unit`). Then repoint the README link and the AGENTS.md router. Its 4 backticked paths are tracked. Last changed 2026-11-10.
- `docs/assets/north-logo.jpeg`: **keep**. Used by `README.md` line 1 since commit 2edb3994b7, which removed the `.png` and the `.webp`. 1,804,668 bytes, where the `.webp` it replaced was 27,626.
- `docs/feedback-loop.md`: **rewrite**. Reference, read by code: `crates/north-base/src/cites.rs` names it and `north health` reads its "File signals" and "Recorded readings" tables, so the file and those two headings stay. Rewrite the rest: the header dates its reader cells to a search of 2026-10-12; one cell cites `bin/capacity-watchdog:465`, which is not tracked (unbackticked, so the citation check misses it); five paragraphs under the table are dated measurements that belong in `docs/rationale/learned-routing-and-goal-eta.md`. Never on a newcomer's path. Last changed 2026-11-11. Private-detail scan: owner's name 3, machine names 0, dollar figures or account labels 0.
- `docs/graph.md`: **keep**. Reference for `north graph`; the survivor of the graph group. One fix: it names `north graph retain` and `north land follow`, which the help does not list. Last changed 2026-11-12. Private-detail scan: owner's name 3, machine names 1, dollar figures or account labels 2.
- `docs/harness.md`: **rewrite**. The end-to-end page, 217 lines, four subjects. Wrong today: (1) Install opens "is not a public install", which north#1281 reverses, and steps 2 and 6 say "Not verified here" and "not traced"; (2) it names `agents sync`, `agents plan`, `threads unowned`, `threads ready`, `threads show`, `threads queue follow` and `follows-release` as bare commands, and `bin/` ships none of them (the help lists `north config agents sync`, `north threads ARGV`, `north follows-release`); (3) Landing step 1 is one 190-word sentence. Split it: "Staffing a worker" and "Messages" become the briefing page, "Landing a change" the landing page, "Install" the onboarding page, "Where policy lives" the operator-repository page. Checked: 3 backticked paths tracked, links resolve, `north fleet tree|launch|send|ask`, `north land serve|enqueue`, `north runtime update|health|deploy`, `north ledger worker` are listed by help. Last changed 2026-11-12. Private-detail scan: owner's name 5, machine names 0, dollar figures or account labels 0.
- `docs/nexus-run.md`: **delete**. Move to the operator's private repository. Nothing is lost: the contract is the `north capacity nexus` line in `north capacity --help`; the rest describes one operator's machine (named 9 times), a private asset cache path and another project's commands. Remove the links in `README.md` and `docs/features/_header.md` in the same change. Last changed 2026-11-07. Private-detail scan: owner's name 0, machine names 9, dollar figures or account labels 0.
- `docs/references/2609.20804-harness-design.md`: **keep**. Digest of one paper, dated 2026-10-12, linked from `docs/references.md`. The only file in its directory. Last changed 2026-11-10.
- `docs/references.md`: **keep**. Outside sources, 784 lines; its anchors are link targets for rationale and watch notes, so headings keep their ids. Relative links resolve. Reference tier only. Last changed 2026-11-12. Private-detail scan: owner's name 4, machine names 0, dollar figures or account labels 5.
- `docs/terms.md`: **keep**. The four layer terms; accurate. One fix: the NorthThread section says items are managed "through the `threads` command"; `bin/` no longer ships it, the help lists `north threads ARGV`. Last changed 2026-11-10.
- `README.md`: **rewrite**. Front door. Wrong today: (1) says `bin/` holds `agents`, `threads` and `safe-push`; `ls bin` shows none of them (deleted 2026-11-06, commit 4dd7f6aea0; `safe-push` is now a link to the north binary that the runtime installer writes, and `agents` and `threads` resolve outside this repository); (2) links `docs/features/` as "what each command does", but the index `docs/features.md` is untracked since 59f27fc3d7 (`.gitignore` line 8), so the link opens ten directories of one-bullet fragments; (3) names Claude Code and Codex as the base harnesses, while `north fleet --help` takes `--provider claude|codex|opencode|pi`; (4) links `docs/architecture.md` and not the newer root `ARCHITECTURE.md`; (5) "Try it" stops at opening the screen and says the factory "does not install elsewhere", which north#1281 reverses; (6) lists `docs/nexus-run.md` and one rationale note among concept documents. `north check citations` exits 0 on it. Last changed 2026-11-12. Private-detail scan: owner's name 4, machine names 3, dollar figures or account labels 0.

### Feature records

- `docs/features/_header.md`: **rewrite**. Head of the generated features index. Says "The commands live in bin/", which `ls bin` contradicts, and links `nexus-run.md`. Rewrite as two sentences on what the index is and who owns flags and numbers. Last changed 2026-11-07. Private-detail scan: owner's name 0, machine names 4, dollar figures or account labels 0.

#### `docs/features/01-the-north-app/`

- `docs/features/01-the-north-app/0001-interactive-fleet-screen.md`: **keep**. 98 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/01-the-north-app/0002-work-items.md`: **keep**. 369 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/01-the-north-app/0003-where-work-state-lives.md`: **keep**. 222 bytes, last changed 2026-11-04. Names no `north` verb.
- `docs/features/01-the-north-app/0004-skills-and-hooks.md`: **keep**. 268 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/01-the-north-app/0005-verb-help.md`: **keep**. 967 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/01-the-north-app/0006-project-status-line.md`: **keep**. 509 bytes, last changed 2026-11-08. Names no `north` verb.
- `docs/features/01-the-north-app/0007-infra.md`: **keep**. 534 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/01-the-north-app/0008-operator-drill-downs.md`: **keep**. 1,742 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/01-the-north-app/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/02-staffing-and-routing/`

- `docs/features/02-staffing-and-routing/0001-claude-account-profiles.md`: **keep**. 1,079 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0002-interactive-claude-and-codex.md`: **keep**. 252 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0003-fleet-accounts.md`: **keep**. 722 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0004-account-owner-hosts.md`: **keep**. 1,325 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0005-routing.md`: **keep**. 551 bytes, last changed 2026-11-04. Names no `north` verb. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0006-spend-rule.md`: **keep**. 970 bytes, last changed 2026-11-12. Fix: names `north fleet need`, `north spend`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0007-resident-cap.md`: **keep**. 593 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0008-token-only-usage.md`: **keep**. 520 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0009-usage-resets.md`: **merge**. Merge into `0010-usage-read.md`: the body opens with how usage readings are published, not with resets; the reset decision is in `docs/rationale/usage-resets.md`. 1,060 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/02-staffing-and-routing/0010-usage-read.md`: **keep**. 816 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0011-worker-estimates.md`: **keep**. 510 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0012-follows-boxes.md`: **keep**. 318 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help. Fix: names the bare command `follows-release`, `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0013-project-quote.md`: **keep**. 358 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0014-fleet-idle-north-252.md`: **keep**. 799 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0015-context-arms-north-636.md`: **keep**. 1,176 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0017-goal-eta.md`: **keep**. 325 bytes, last changed 2026-11-05. Names no `north` verb. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0018-agents-plan-score-project-scores-a-passe.md`: **keep**. 456 bytes, last changed 2026-11-04. Names no `north` verb. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/02-staffing-and-routing/0020-effort-arms-north-112.md`: **rewrite**. Opens with behaviour "before the cell draw of north#939", so it records the past. State what runs now, or delete it if nothing does (docs/rationale/learning-loops.md, "The effort cell retired, 2026-11-10"). 654 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0020-review-items-by-kind-north-255.md`: **keep**. 683 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0021-tier-success-brier-north-255.md`: **keep**. 566 bytes, last changed 2026-11-07. Fix: names `north threads recommend`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/02-staffing-and-routing/0022-tier-per-category-north-966.md`: **keep**. 678 bytes, last changed 2026-11-09. Names no `north` verb.
- `docs/features/02-staffing-and-routing/0023-deepseek-overlay-arms-north-1020.md`: **keep**. 377 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/02-staffing-and-routing/0024-api-credit-readings-north-1043.md`: **rewrite**. One bullet of 2,474 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/02-staffing-and-routing/0025-subscription-floor-north-1049.md`: **keep**. 458 bytes, last changed 2026-11-11. Names no `north` verb.
- `docs/features/02-staffing-and-routing/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/03-fleet-sessions/`

- `docs/features/03-fleet-sessions/0001-fleet-sessions.md`: **keep**. 1,057 bytes, last changed 2026-11-09. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0002-fleet-store.md`: **keep**. 761 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0003-fleet-spool.md`: **keep**. 708 bytes, last changed 2026-11-05. Fix: names `north fleet spool`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/03-fleet-sessions/0004-fleet-tick.md`: **rewrite**. One bullet of 3,301 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Fix: names `north fleet spool`, `north fleet waits`, `north runtime checkouts`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/03-fleet-sessions/0005-fleet-control-north-433-box-1d-of-north.md`: **keep**. 1,017 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0006-placement-gate.md`: **keep**. 1,496 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0007-launch-tier-north-242.md`: **keep**. 860 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0008-learning-tick-north-112.md`: **keep**. Refile under section 05: it describes `north tune`, not a session. 1,527 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0008-learning-tick-north-255-budgets.md`: **merge**. Merge into `0008-learning-tick-north-112.md` (both describe `north tune`) and refile the result under section 05. 658 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0009-bet-dividends-north-112.md`: **keep**. Refile under section 09 (watches). 856 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0010-ops-check-north-112.md`: **keep**. 557 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 2 times; name the role instead.
- `docs/features/03-fleet-sessions/0011-dispatch.md`: **rewrite**. One bullet of 5,788 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north fleet need`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 3 times and the operator's machines 3 times; name the role instead.
- `docs/features/03-fleet-sessions/0011-paused-projects.md`: **keep**. 207 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0012-cloud-sessions.md`: **keep**. 649 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0013-working-slots.md`: **keep**. 1,302 bytes, last changed 2026-11-10. Fix: names `north fleet waits`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/03-fleet-sessions/0014-stranded-lanes-north-333.md`: **keep**. 689 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0015-live-claims.md`: **keep**. 971 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/03-fleet-sessions/0016-live-holders.md`: **keep**. 353 bytes, last changed 2026-11-04. Names no `north` verb. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/03-fleet-sessions/0018-placement-reach.md`: **keep**. 667 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 2 times; name the role instead.
- `docs/features/03-fleet-sessions/0019-fleet-gates.md`: **keep**. 1,424 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 2 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0019-fleet-tui-switch.md`: **keep**. 1,769 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0020-reboot-revival.md`: **keep**. 497 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0021-launch-claims.md`: **rewrite**. One bullet of 2,887 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0022-refused-land-branches.md`: **keep**. 482 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0023-cloud-handback-landing.md`: **keep**. 582 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0024-fleet-sender.md`: **keep**. 703 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0025-cloud-reap.md`: **keep**. 883 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0025-cloud-stall-wake.md`: **keep**. 468 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0025-loop-registry-north-112.md`: **keep**. Refile under section 05; `docs/rationale/learning-loops.md` is the long form. 982 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0026-fleet-role.md`: **keep**. 893 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0027-north-proxy.md`: **keep**. 899 bytes, last changed 2026-11-10. Fix: names `north proxy`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/03-fleet-sessions/0027-window-move.md`: **rewrite**. One bullet of 2,092 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north proxy`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/03-fleet-sessions/0028-fleet-forget-guard.md`: **keep**. 1,261 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0028-projection-scoring-north-255.md`: **keep**. 588 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0029-claim-holder-north-562.md`: **keep**. 890 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/03-fleet-sessions/0029-exited-claims-sweep-north-631.md`: **keep**. 1,444 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/03-fleet-sessions/0030-launch-hosts-north-573.md`: **keep**. 394 bytes, last changed 2026-11-06. Names no `north` verb. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/03-fleet-sessions/0031-dispatch-spread-north-259.md`: **keep**. 633 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0032-window-adopt-north-636.md`: **keep**. 1,143 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0033-proxy-name-follows-live-window-north-636.md`: **merge**. Merge into `0032-window-adopt-north-636.md`: both describe `north fleet adopt`. 1,021 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0034-fleet-reaper-north-636.md`: **rewrite**. One bullet of 2,270 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north fleet reap`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/03-fleet-sessions/0035-gate-relay-and-orphans.md`: **keep**. 611 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 3 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0036-proxy-take-over-window-north-733.md`: **merge**. Merge into `0027-north-proxy.md`: both say where the proxy runs. 1,695 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 2 times and the operator's machines 1 times; name the role instead.
- `docs/features/03-fleet-sessions/0037-lead-runs-on-a-trusted-host-north-765.md`: **keep**. 406 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0038-stop-waits-for-the-sessions-own-builds-north-772.md`: **keep**. 736 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0039-harness-stops-resume-by-themselves-north-790.md`: **keep**. 1,201 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0040-handover-brief-names-the-typed-lane-north-801.md`: **keep**. 1,676 bytes, last changed 2026-11-09. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0041-dispatch-pause-and-queued-launch-north-960.md`: **keep**. 1,407 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0042-fleet-state-follows-xdg-state-home-north-964.md`: **keep**. 491 bytes, last changed 2026-11-09. Names no `north` verb.
- `docs/features/03-fleet-sessions/0043-duplicate-stagings-north-888.md`: **keep**. 635 bytes, last changed 2026-11-09. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0044-launch-refuses-home-paths-on-another-host-north-1029.md`: **keep**. 456 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0045-pi-window-adopt-north-1048.md`: **merge**. Merge into `0032-window-adopt-north-636.md`: the Pi case of the same verb. 639 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0046-claude-interactive-onboarding-north-1048.md`: **keep**. 755 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0047-glance-north-1063.md`: **keep**. 525 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0047-session-host-socket-north-1069.md`: **keep**. 508 bytes, last changed 2026-11-10. Names no `north` verb.
- `docs/features/03-fleet-sessions/0047-spend-refused-launch-queues-by-priority-north-1073.md`: **keep**. 649 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0048-dispatch-skips-boxes-naming-unreadable-home-paths-north-1054.md`: **keep**. 367 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0049-page-flood-backstop-north-1048.md`: **rewrite**. One bullet of 2,654 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0050-proxy-break-glass-north-1106.md`: **keep**. 779 bytes, last changed 2026-11-11. Fix: names `north fleet break-glass`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/03-fleet-sessions/0051-queued-launch-withdrawn-or-dropped-north-1054.md`: **keep**. 587 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0052-hand-stops-north-1136.md`: **keep**. 995 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0053-drafts-north-1240.md`: **rewrite**. One bullet of 2,493 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/03-fleet-sessions/0053-small-first-north-1233.md`: **keep**. 1,739 bytes, last changed 2026-11-12. Fix: names `north finish`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/03-fleet-sessions/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/04-landing-and-lanes/`

- `docs/features/04-landing-and-lanes/0001-safe-push-to-main-lands-the-lane-as-one.md`: **keep**. 616 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0002-safe-push-to-main-reads-committed-safe-p.md`: **keep**. 1,664 bytes, last changed 2026-11-12. Fix: names `north depbox`, `north land profile`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0003-red-main-event.md`: **keep**. 1,096 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/04-landing-and-lanes/0004-landing-tempo-north-255-b12-and-b13.md`: **keep**. 620 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0005-north-capacity-offload-choose-run.md`: **rewrite**. One bullet of 2,243 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0006-fleet-capacity-rows.md`: **keep**. Refile under section 08 (capacity). 921 bytes, last changed 2026-11-07. Fix: names `north capacity actions-pull`, `north capacity pull`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0007-mutant-record-id-north-mutant-record-sav.md`: **keep**. 592 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0008-fails-first-gate.md`: **keep**. 591 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0009-provenance.md`: **keep**. 831 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0010-landing-flow.md`: **keep**. 1,157 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 2 times and the operator's machines 0 times; name the role instead.
- `docs/features/04-landing-and-lanes/0011-red-main-notice.md`: **keep**. 490 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help. Fix: names the bare command `ci-diagnose`, `red-main-notify`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/04-landing-and-lanes/0012-ci-diagnosis.md`: **keep**. 864 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help. Fix: names the bare command `ci-diagnose`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/04-landing-and-lanes/0014-lander-queue-stall.md`: **keep**. 1,315 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0015-box-trailers-and-the-landing-tick.md`: **keep**. 1,146 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/04-landing-and-lanes/0015-lander-promote.md`: **rewrite**. One bullet of 3,450 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north land doctor`, `north land replay`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0015-lander-watchdog.md`: **keep**. 989 bytes, last changed 2026-11-08. Fix: names `north land requeue`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0015-one-lander-per-repository.md`: **keep**. 842 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0016-clippy-first-land-check.md`: **keep**. 700 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0016-lander-after-landing-revert.md`: **keep**. 1,191 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0016-lander-check-local.md`: **keep**. 455 bytes, last changed 2026-11-06. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0017-remote-land.md`: **keep**. 536 bytes, last changed 2026-11-07. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0018-lander-route.md`: **keep**. 611 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0019-land-profile.md`: **keep**. 1,030 bytes, last changed 2026-11-06. Fix: names `north land profile`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0020-lander-lane-gates.md`: **keep**. 876 bytes, last changed 2026-11-07. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0021-docs-owners-scan.md`: **keep**. 820 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0022-coverage-test-selection.md`: **rewrite**. One bullet of 2,266 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0023-land-queue-arrival-order.md`: **keep**. 916 bytes, last changed 2026-11-09. Fix: names `north land urgent`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0024-doc-links-scan.md`: **keep**. 1,076 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0025-background-offload-slot.md`: **keep**. 438 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0026-lander-link-follows-a-red-main.md`: **keep**. 919 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0027-red-main-episode-message.md`: **keep**. 1,260 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0028-inferred-check-adds-nothing-new.md`: **keep**. 653 bytes, last changed 2026-11-06. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0028-lander-doctor.md`: **keep**. 1,733 bytes, last changed 2026-11-10. Fix: names `north land doctor`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0028-sheriff-reverts-any-red-main.md`: **keep**. 1,273 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0029-safe-push-link.md`: **keep**. 534 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0030-lander-drill.md`: **keep**. 1,109 bytes, last changed 2026-11-07. Fix: names `north land drill`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0031-gate-shadow-scan.md`: **keep**. 956 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0032-enqueue-conflict-refusal.md`: **keep**. 458 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0033-enqueue-after-checks.md`: **rewrite**. One bullet of 2,259 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0034-land-gate-stats.md`: **keep**. 886 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0035-landing-refusals-shadow.md`: **keep**. 1,044 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0036-lander-check-environment-fallback.md`: **keep**. 676 bytes, last changed 2026-11-07. Fix: names `north land doctor`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0036-test-wall-clock-scan.md`: **keep**. 780 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/04-landing-and-lanes/0037-detector-proof-scan.md`: **keep**. 1,418 bytes, last changed 2026-11-08. Fix: names `north policy-replay case`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names `harness/agents`, which is not tracked.
- `docs/features/04-landing-and-lanes/0037-land-first-batches.md`: **keep**. 1,391 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0038-lander-fix-lanes.md`: **keep**. 1,038 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0038-stale-git-locks.md`: **keep**. 1,600 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/04-landing-and-lanes/0039-lander-break-glass.md`: **keep**. 458 bytes, last changed 2026-11-08. Fix: names `north land break-glass`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/04-landing-and-lanes/0040-lane-remove.md`: **keep**. 1,980 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0041-lane-issue-and-archive.md`: **rewrite**. One bullet of 2,992 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0042-lander-ticket-status.md`: **keep**. 499 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0043-lifecycle-machines-checked-over-every-ordering.md`: **keep**. 1,244 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0044-advisory-land-checks.md`: **keep**. 834 bytes, last changed 2026-11-12. Names no `north` verb. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/04-landing-and-lanes/0045-lander-fast-forwards-the-served-checkout.md`: **keep**. 767 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/04-landing-and-lanes/0046-push-jobs-scan.md`: **keep**. 923 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0047-lane-sync.md`: **keep**. 770 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/04-landing-and-lanes/0048-root-commit-on-an-empty-origin.md`: **keep**. 481 bytes, last changed 2026-11-11. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0049-holdlist-guards-scan.md`: **keep**. 1,045 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help. Fix: names `harness/evals`, which is not tracked.
- `docs/features/04-landing-and-lanes/0050-hand-in-waits-for-the-verdict.md`: **keep**. 1,037 bytes, last changed 2026-11-12. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0051-hand-in-verb.md`: **keep**. 1,597 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0052-stale-host-token-is-no-red-main.md`: **keep**. 569 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0053-finish-verb.md`: **rewrite**. One bullet of 3,438 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north finish`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/0054-a-refusal-wakes-the-session-a-landing-wakes-nobody.md`: **keep**. 851 bytes, last changed 2026-11-12. Names no `north` verb.
- `docs/features/04-landing-and-lanes/0055-leftovers-scan.md`: **keep**. 885 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/04-landing-and-lanes/0056-buck2-test-reuse.md`: **keep**. 1,079 bytes, last changed 2026-11-12. Fix: names `north land reuse`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/04-landing-and-lanes/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/05-ledger-calibration-and-reports/`

- `docs/features/05-ledger-calibration-and-reports/0001-worker-delivery-costs.md`: **keep**. 1,123 bytes, last changed 2026-11-08. Fix: names `north threads ingest`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/05-ledger-calibration-and-reports/0002-split-outcome.md`: **keep**. 323 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0003-ledger-pull-north-394.md`: **keep**. 577 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 2 times; name the role instead.
- `docs/features/05-ledger-calibration-and-reports/0004-recurring-recalibration.md`: **keep**. 599 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/05-ledger-calibration-and-reports/0005-loop-report.md`: **keep**. 731 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0007-incidents-by-construction.md`: **keep**. 928 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0008-tool-mining.md`: **keep**. 777 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0009-landed-issues-north-255.md`: **keep**. 1,171 bytes, last changed 2026-11-10. Fix: names `north threads landed-runs`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/05-ledger-calibration-and-reports/0010-scoreboard-north-657.md`: **keep**. 1,062 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/05-ledger-calibration-and-reports/0011-shared-history-reads-north-562.md`: **keep**. 937 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/05-ledger-calibration-and-reports/0012-frontier-scoreboard-north-730.md`: **delete**. Records a retired benchmark, not a feature: `north test --help` lists no frontier verb. `docs/rationale/frontier.md` keeps the method and results. 374 bytes, last changed 2026-11-10. Names no `north` verb.
- `docs/features/05-ledger-calibration-and-reports/0013-peer-snapshot-reads-north-760.md`: **keep**. 599 bytes, last changed 2026-11-07. Names no `north` verb. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/05-ledger-calibration-and-reports/0014-dash-north-905.md`: **keep**. 1,528 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0015-deepseek-value-report-north-1020.md`: **keep**. 757 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0015-losses-north-903.md`: **keep**. 620 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0016-ledger-show-north-1072.md`: **keep**. 466 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/0017-handovers-per-week-north-1077.md`: **keep**. 452 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/05-ledger-calibration-and-reports/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/06-policy-measurement/`

- `docs/features/06-policy-measurement/0001-prompt-grounding.md`: **keep**. 1,281 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/06-policy-measurement/0002-context-audit.md`: **keep**. 811 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/06-policy-measurement/0003-hook-verdict-stats.md`: **keep**. 1,164 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/06-policy-measurement/0004-policy-regression-gate.md`: **keep**. 750 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/06-policy-measurement/0005-skill-prune.md`: **keep**. 619 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/06-policy-measurement/0006-policy-replay.md`: **keep**. 849 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/06-policy-measurement/0007-experiment-registry.md`: **keep**. 832 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/06-policy-measurement/0007-proxy-calls.md`: **keep**. 1,243 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 2 times and the operator's machines 0 times; name the role instead.
- `docs/features/06-policy-measurement/0008-goals-as-checks.md`: **keep**. 1,185 bytes, last changed 2026-11-11. Fix: names `north land profile`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/06-policy-measurement/0009-corrections-become-guards.md`: **keep**. 891 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 2 times and the operator's machines 0 times; name the role instead.
- `docs/features/06-policy-measurement/0010-recommendations-act.md`: **keep**. 855 bytes, last changed 2026-11-08. Fix: names `north hook recommend-acts`, `north hook stop`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 2 times and the operator's machines 0 times; name the role instead.
- `docs/features/06-policy-measurement/0011-deferred-checks.md`: **keep**. 1,561 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/06-policy-measurement/0012-correction-replay.md`: **keep**. 1,591 bytes, last changed 2026-11-08. Fix: names `north policy-replay case`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/06-policy-measurement/0013-command-handoff.md`: **keep**. 620 bytes, last changed 2026-11-08. Fix: names `north hook stop`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 2 times and the operator's machines 0 times; name the role instead.
- `docs/features/06-policy-measurement/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/07-issues-and-native-items/`

- `docs/features/07-issues-and-native-items/0001-native-items.md`: **keep**. 936 bytes, last changed 2026-11-10. Names no `north` verb. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/07-issues-and-native-items/0002-public-github-text.md`: **keep**. 547 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/0003-issue-box-lint.md`: **keep**. 357 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/07-issues-and-native-items/0004-github-as-the-north-fleet-app.md`: **keep**. 557 bytes, last changed 2026-11-10. Names no `north` verb. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/07-issues-and-native-items/0005-observation-boxes.md`: **keep**. 496 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/0006-digest.md`: **keep**. 810 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/07-issues-and-native-items/0007-done-when-boxes.md`: **keep**. 1,295 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/0008-cleanup-at-landing.md`: **keep**. 666 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/0009-oracle-close.md`: **keep**. 602 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/0010-credential-broker.md`: **keep**. 1,495 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/0011-cargo-box-rewrite.md`: **keep**. 827 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/07-issues-and-native-items/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/08-machines-capacity-and-scheduled-jobs/`

- `docs/features/08-machines-capacity-and-scheduled-jobs/0001-product-verification.md`: **keep**. Refile under section 10; `verify-north` is the command the onboarding page should end on. 1,492 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0002-fleet-landings.md`: **keep**. Refile under section 04. 675 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0003-fleet-receipts.md`: **keep**. Refile under section 03. 1,338 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0004-runtime-promotion.md`: **rewrite**. One bullet of 9,413 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north depbox`, `north runtime build`, `north runtime follow`, `north runtime settle`, `north runtime status`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the bare command `agents`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 0 times and the operator's machines 3 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0005-landing-watch.md`: **keep**. 1,548 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0006-orphan-lease-reaper.md`: **keep**. 474 bytes, last changed 2026-11-04. Fix: names `north capacity reap-orphans`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 0 times and the operator's machines 2 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0007-capacity-estimates.md`: **keep**. 590 bytes, last changed 2026-11-05. Names no `north` verb. Fix: names the bare command `machine-capacity`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0008-resource-model.md`: **keep**. 1,079 bytes, last changed 2026-11-05. Fix: names `north capacity calibration`, `north capacity model`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0009-nexus-ram-and-cpu-stalls.md`: **keep**. 1,265 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 6 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0010-rest-check.md`: **keep**. 676 bytes, last changed 2026-11-07. Names no `north` verb. Fix: names the bare command `capacity-watchdog`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0011-poll-loops.md`: **keep**. 359 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0012-farm-only-commands.md`: **keep**. 556 bytes, last changed 2026-11-05. Names no `north` verb. Fix: names the bare command `machine-capacity`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0013-scheduled-job-runs.md`: **keep**. 1,119 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0014-overdue-jobs.md`: **keep**. 512 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 0 times and the operator's machines 2 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0015-private-checks.md`: **keep**. 643 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0016-queued-laptop-jobs.md`: **keep**. 767 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0017-battery-shed.md`: **rewrite**. One bullet of 2,370 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0018-peer-forced-command.md`: **keep**. 1,092 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `job-run`, `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0019-agent-builds-offload-first.md`: **keep**. 1,718 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0019-github-mirror.md`: **keep**. 1,163 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the bare command `ci-diagnose`, `red-main-notify`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0020-fleet-ssh-scope.md`: **keep**. 715 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0021-credentials-registry.md`: **keep**. 1,066 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0021-pull-hand-over.md`: **keep**. 1,128 bytes, last changed 2026-11-06. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0022-records-mirror.md`: **keep**. 1,523 bytes, last changed 2026-11-11. Fix: names `north land profile`, `north runtime mirror`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0023-admission-ledger-rows.md`: **keep**. 697 bytes, last changed 2026-11-07. Fix: names `north threads run-ids`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0024-home-move.md`: **keep**. 1,175 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0025-host-checkouts.md`: **keep**. 807 bytes, last changed 2026-11-11. Fix: names `north runtime checkouts`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0026-runtime-build-ahead.md`: **keep**. 1,564 bytes, last changed 2026-11-12. Fix: names `north runtime build`, `north runtime build-ahead`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/08-machines-capacity-and-scheduled-jobs/0027-host-token-push.md`: **keep**. 613 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/08-machines-capacity-and-scheduled-jobs/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/09-watches-and-alerts/`

- `docs/features/09-watches-and-alerts/0004-cross-project-scan.md`: **keep**. 388 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help. Fix: names the bare command `threads`, which `bin/` no longer ships; write the `north` form the help lists.
- `docs/features/09-watches-and-alerts/0005-supply-alert.md`: **keep**. 240 bytes, last changed 2026-11-12. Names no `north` verb.
- `docs/features/09-watches-and-alerts/0006-supply-scan.md`: **keep**. 325 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help.
- `docs/features/09-watches-and-alerts/0007-self-detectors.md`: **rewrite**. One bullet of 2,649 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/09-watches-and-alerts/0008-promises.md`: **keep**. 650 bytes, last changed 2026-11-12. Names no `north` verb.
- `docs/features/09-watches-and-alerts/0009-alarm-fold.md`: **keep**. 1,766 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help (`north land watchdog:` there is a message prefix, not a verb).
- `docs/features/09-watches-and-alerts/_title.md`: **keep**. Section heading the index generator reads. 

#### `docs/features/10-development-tools/`

- `docs/features/10-development-tools/0001-dependency-sandbox.md`: **merge**. Merge into `0023-north-depbox.md`: `bin/depbox` runs `north depbox`, so the two records describe one sandbox. 1,678 bytes, last changed 2026-11-10. Fix: names `north depbox`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0002-retention.md`: **keep**. 735 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/10-development-tools/0003-disk-budget.md`: **keep**. 1,013 bytes, last changed 2026-11-10. Fix: names `north capacity disk-share`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0004-lane-target-seed.md`: **rewrite**. One bullet of 2,342 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Fix: names `north capacity disk`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/10-development-tools/0005-compiler-cache.md`: **keep**. 1,077 bytes, last changed 2026-11-05. Names no `north` verb. Fix: names the owner 0 times and the operator's machines 5 times; name the role instead.
- `docs/features/10-development-tools/0006-build-cache-gate.md`: **keep**. 651 bytes, last changed 2026-11-04. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/10-development-tools/0007-evidence-budget.md`: **keep**. 910 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0008-dedicated-automation-chrome.md`: **keep**. 554 bytes, last changed 2026-11-04. Names no `north` verb.
- `docs/features/10-development-tools/0009-effect-kit.md`: **keep**. 837 bytes, last changed 2026-11-08. Names no `north` verb.
- `docs/features/10-development-tools/0010-code-health.md`: **keep**. 1,516 bytes, last changed 2026-11-10. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0011-rust-build-times.md`: **keep**. 911 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help. Fix: names the owner 0 times and the operator's machines 1 times; name the role instead.
- `docs/features/10-development-tools/0012-rust-habits.md`: **rewrite**. One bullet of 3,375 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0013-features-index.md`: **rewrite**. One bullet of 3,248 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-11. Fix: names `north docs structure-index`, `north docs verbs-index`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0014-stub-exec-gate.md`: **keep**. 846 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0015-enqueue-clippy.md`: **merge**. Merge into `04-landing-and-lanes/0016-clippy-first-land-check.md`: one clippy land check, told twice. 568 bytes, last changed 2026-11-05. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0015-layer-gate.md`: **keep**. 638 bytes, last changed 2026-11-05. Fix: names `north code-health layers`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0015-split-move-module.md`: **keep**. 1,087 bytes, last changed 2026-11-07. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0016-north-graph.md`: **merge**. Merge into `docs/graph.md` (Kernel); leave one sentence and the link. 981 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0017-north-graph-ingest.md`: **merge**. Merge into `docs/graph.md` (Ingest); leave one sentence and the link. 1,051 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0018-graph-grounding.md`: **merge**. Merge into `docs/graph.md` (Grounding); leave one sentence and the link. 983 bytes, last changed 2026-11-07. Fix: names `north land follow`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0019-north-graph-ready.md`: **merge**. Merge into `docs/graph.md` (Ready set); leave one sentence and the link. 1,045 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help. Fix: names the owner 1 times and the operator's machines 0 times; name the role instead.
- `docs/features/10-development-tools/0020-offload-eviction.md`: **rewrite**. One bullet of 2,602 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0021-disk-ledger.md`: **keep**. 496 bytes, last changed 2026-11-07. Fix: names `north capacity disk`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0022-context-pack.md`: **keep**. 870 bytes, last changed 2026-11-08. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0023-north-depbox.md`: **rewrite**. One bullet of 2,025 bytes. Cut it to what the feature is for and the command that runs it; move flags to the command's help and reasons to a rationale note. Last changed 2026-11-12. Fix: names `north depbox`, which neither `north --help-all` nor the verb's own `--help` lists; add the help line where the dispatcher still runs it, else drop the name.
- `docs/features/10-development-tools/0024-buck-targets.md`: **keep**. 1,267 bytes, last changed 2026-11-11. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0025-check-oneshot.md`: **keep**. 346 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/0025-graph-split.md`: **keep**. 1,223 bytes, last changed 2026-11-12. Each `north` verb it names is listed by help.
- `docs/features/10-development-tools/_title.md`: **keep**. Section heading the index generator reads. 

### Rationale notes

- `docs/rationale/account-capacity.md`: **keep**. Plan, superseded: its lines 12 to 16 already say the limiter was deleted on 2026-11-11 and link `spend-rule.md`. 72,083 bytes, last changed 2026-11-12. Verbs the help does not list: `north fleet waits`, `north hook spawn-capacity`, `north hook spawn-gate`, `north threads calibrate`. Backticked paths no longer tracked: 3. Private-detail scan: owner's name 36, machine names 11, dollar figures or account labels 5.
- `docs/rationale/agent-era-security.md`: **keep**. Outside-source review. 18,382 bytes, last changed 2026-11-12.
- `docs/rationale/agent-push-identity.md`: **keep**. Live mechanism. 4,664 bytes, last changed 2026-11-11.
- `docs/rationale/agent-user.md`: **keep**. Live mechanism, with measurements. 5,409 bytes, last changed 2026-11-10. Verbs the help does not list: `north depbox`.
- `docs/rationale/architectural-drift.md`: **keep**. Decision record. 4,733 bytes, last changed 2026-11-10. Verbs the help does not list: `north code-health budget`.
- `docs/rationale/architecture-layers.md`: **keep**. Plan; the long form behind the root `ARCHITECTURE.md`. 65,103 bytes, last changed 2026-11-12. Verbs the help does not list: `north code-health layers`. Backticked paths no longer tracked: 1.
- `docs/rationale/attribution.md`: **keep**. Live mechanism. 19,543 bytes, last changed 2026-11-11. Verbs the help does not list: `north capacity deadline`, `north fleet need`. Private-detail scan: owner's name 7, machine names 3, dollar figures or account labels 0.
- `docs/rationale/backlog-ranking.md`: **keep**. Read by code: `crates/north-graph-ingest/src/ranking.rs` and `crates/north-dispatch/src/dispatch/issues.rs` name it. Its ranked list is of 2026-11-06. 8,753 bytes, last changed 2026-11-07. Private-detail scan: owner's name 4, machine names 6, dollar figures or account labels 0.
- `docs/rationale/betterleaks.md`: **keep**. Live mechanism; takes in `decisions/betterleaks-scan-times.md`. 2,750 bytes, last changed 2026-11-06. Private-detail scan: owner's name 1, machine names 1, dollar figures or account labels 0.
- `docs/rationale/build-cost.md`: **keep**. Plan. Its opening "Nothing here is built yet" needs a dated status line: line 72 already records one part superseded on 2026-11-11. 36,802 bytes, last changed 2026-11-11. Verbs the help does not list: `north runtime settle`. Backticked paths no longer tracked: 10. Private-detail scan: owner's name 2, machine names 42, dollar figures or account labels 0.
- `docs/rationale/capacity-scheduler.md`: **keep**. Plan of 2026-11-04; `placement.md` (2026-11-11) is the newer plan for the same subject and should be linked from its top. 65,078 bytes, last changed 2026-11-12. Verbs the help does not list: `north capacity actions-pull`, `north capacity calibration`, `north capacity deadline`, `north capacity model`, `north capacity predict`, `north capacity retune`. Backticked paths no longer tracked: 6. Private-detail scan: owner's name 18, machine names 94, dollar figures or account labels 0.
- `docs/rationale/change-graph.md`: **keep**. Decision record. 7,436 bytes, last changed 2026-11-12. Private-detail scan: owner's name 2, machine names 1, dollar figures or account labels 0.
- `docs/rationale/change-graph-replay.md`: **keep**. Measurement. 6,487 bytes, last changed 2026-11-08. Private-detail scan: owner's name 0, machine names 1, dollar figures or account labels 0.
- `docs/rationale/clean-up-scans.md`: **keep**. Live mechanism. 3,563 bytes, last changed 2026-11-12.
- `docs/rationale/close-by-rule-share.md`: **keep**. Measurement; verdict dropped. 2,851 bytes, last changed 2026-11-12.
- `docs/rationale/command-parsing.md`: **keep**. Measurement and choice. 11,443 bytes, last changed 2026-11-12. Verbs the help does not list: `north convo compress`, `north convo show`. Private-detail scan: owner's name 0, machine names 1, dollar figures or account labels 0.
- `docs/rationale/command-usage-2026-11.md`: **keep**. Measurement. 38,300 bytes, last changed 2026-11-11. Private-detail scan: owner's name 10, machine names 11, dollar figures or account labels 0.
- `docs/rationale/compile-reuse.md`: **keep**. Plan; the current direction for build time (decided 2026-11-11), so the survivor of the build-time group. 54,670 bytes, last changed 2026-11-12. Verbs the help does not list: `north build-time`, `north land reuse`. Backticked paths no longer tracked: 1. Private-detail scan: owner's name 0, machine names 32, dollar figures or account labels 0.
- `docs/rationale/concerns.md`: **keep**. Live mechanism. 33,615 bytes, last changed 2026-11-11. Verbs the help does not list: `north concerns ratchet`, `north concerns sample`, `north experiments`, `north land doctor`, `north threads ingest`. Backticked paths no longer tracked: 1. Private-detail scan: owner's name 7, machine names 1, dollar figures or account labels 0.
- `docs/rationale/conflict-free-landing.md`: **keep**. Plan. 39,201 bytes, last changed 2026-11-11. Backticked paths no longer tracked: 3. Private-detail scan: owner's name 1, machine names 6, dollar figures or account labels 0.
- `docs/rationale/context-economics.md`: **keep**. Measurement. 14,031 bytes, last changed 2026-11-08. Private-detail scan: owner's name 3, machine names 0, dollar figures or account labels 32.
- `docs/rationale/coverage-test-selection.md`: **keep**. Live mechanism, with measurements. 6,221 bytes, last changed 2026-11-10. Private-detail scan: owner's name 0, machine names 1, dollar figures or account labels 0.
- `docs/rationale/crate-split.md`: **keep**. Plan and record. 51,820 bytes, last changed 2026-11-11. Verbs the help does not list: `north land profile`. Backticked paths no longer tracked: 7. Private-detail scan: owner's name 0, machine names 9, dollar figures or account labels 0.
- `docs/rationale/credentials-registry.md`: **keep**. Live mechanism. 3,492 bytes, last changed 2026-11-08. Private-detail scan: owner's name 4, machine names 1, dollar figures or account labels 0.
- `docs/rationale/discovery-turns.toml`: **keep**. Read by code: `crates/north-land-scans/src/turn_scope.rs` names it. A registry, not a note. 1,584 bytes, last changed 2026-11-12.
- `docs/rationale/dispatch-spread.md`: **keep**. Live mechanism. 7,114 bytes, last changed 2026-11-12. Private-detail scan: owner's name 0, machine names 6, dollar figures or account labels 0.
- `docs/rationale/docs-architecture.md`: **keep**. The docs layout. Add a dated section, as its own rule asks: (1) it names `tests/readme_links.rs` as the check on the README, and no such file is tracked; (2) it treats the features index as a tracked file, which ended on 2026-11-11; (3) this audit's reading order replaces its "three rings" for people. 17,469 bytes, last changed 2026-11-11. Backticked paths no longer tracked: 9.
- `docs/rationale/docs-audit-2026-11.md`: **keep**. This audit. The workers who carry out a verdict mark its row done with the commit.
- `docs/rationale/estimation-audit-2026-11-05.md`: **keep**. Measurement. 16,944 bytes, last changed 2026-11-07. Verbs the help does not list: `north capacity calibration`.
- `docs/rationale/experiments.toml`: **keep**. Read by code: `north check experiments` judges its entries (`.safe-push` line 56). A registry, not a note. 32,188 bytes, last changed 2026-11-12. Private-detail scan: owner's name 7, machine names 10, dollar figures or account labels 26.
- `docs/rationale/failing-first.md`: **keep**. Reasoning behind a rule a skill loads. 1,946 bytes, last changed 2026-11-04.
- `docs/rationale/frontier.md`: **keep**. Retired design; holds the method and results. 16,193 bytes, last changed 2026-11-10. Verbs the help does not list: `north test frontier`. Private-detail scan: owner's name 2, machine names 1, dollar figures or account labels 0.
- `docs/rationale/graph-blame.md`: **keep**. Plan. 18,857 bytes, last changed 2026-11-11. Verbs the help does not list: `north land reds`. Private-detail scan: owner's name 2, machine names 4, dollar figures or account labels 0.
- `docs/rationale/guard-false-refusals.md`: **keep**. Measurement. 12,718 bytes, last changed 2026-11-10. Verbs the help does not list: `north hook dispatch`. Private-detail scan: owner's name 0, machine names 7, dollar figures or account labels 0.
- `docs/rationale/harness-losses-2026-11-08.md`: **keep**. Measurement. 9,866 bytes, last changed 2026-11-11. Verbs the help does not list: `north fleet need`. Private-detail scan: owner's name 0, machine names 13, dollar figures or account labels 0.
- `docs/rationale/harness-skill-roots.md`: **keep**. Decision record. 6,044 bytes, last changed 2026-11-10.
- `docs/rationale/holdlist.md`: **keep**. Live mechanism. 2,264 bytes, last changed 2026-11-11. Backticked paths no longer tracked: 1.
- `docs/rationale/hooks-and-runtime.md`: **keep**. Audit and plan. 19,665 bytes, last changed 2026-11-08. Verbs the help does not list: `north hook pre-bash`, `north hook session-kill`, `north hook spawn-capacity`, `north threads recommend`. Backticked paths no longer tracked: 4. Private-detail scan: owner's name 3, machine names 1, dollar figures or account labels 0.
- `docs/rationale/lander-doctor.md`: **keep**. Live mechanism. 8,934 bytes, last changed 2026-11-10. Verbs the help does not list: `north capacity disk`, `north land doctor`. Private-detail scan: owner's name 1, machine names 3, dollar figures or account labels 0.
- `docs/rationale/lander-drill.md`: **keep**. Live mechanism. 2,780 bytes, last changed 2026-11-08. Verbs the help does not list: `north capacity disk`, `north land drill`.
- `docs/rationale/landing-by-construction.md`: **keep**. Plan. 31,678 bytes, last changed 2026-11-10. Private-detail scan: owner's name 3, machine names 19, dollar figures or account labels 1.
- `docs/rationale/landing-latency.md`: **keep**. Measurement. 22,473 bytes, last changed 2026-11-08. Backticked paths no longer tracked: 4. Private-detail scan: owner's name 1, machine names 18, dollar figures or account labels 0.
- `docs/rationale/landing-queue-p90.md`: **keep**. Measurement. 7,166 bytes, last changed 2026-11-08. Private-detail scan: owner's name 0, machine names 0, dollar figures or account labels 1.
- `docs/rationale/landing-refusals-shadow.md`: **keep**. Live mechanism. 3,626 bytes, last changed 2026-11-07. Verbs the help does not list: `north runtime-floor-gate`.
- `docs/rationale/landing-train.md`: **keep**. Plan; at 138 kB and 108 commits the largest and most edited note, and the long form of the landing group. Its index line reads "Owner: north#1247". 138,094 bytes, last changed 2026-11-12. Verbs the help does not list: `north capacity deadline`, `north capacity gate`, `north land break-glass`, `north land profile`, `north land-red-wait`, `north land urgent`, `north lane lands`, `north runtime follow`. Backticked paths no longer tracked: 4. Private-detail scan: owner's name 23, machine names 64, dollar figures or account labels 1.
- `docs/rationale/leaner-north.md`: **keep**. Plan. 12,518 bytes, last changed 2026-11-10. Verbs the help does not list: `north land doctor`, `north land drill`, `north ops climb`, `north test frontier`.
- `docs/rationale/learned-routing-and-goal-eta.md`: **keep**. Measurement. 11,045 bytes, last changed 2026-11-07. Backticked paths no longer tracked: 2.
- `docs/rationale/learning-loops.md`: **keep**. Live mechanism. 19,362 bytes, last changed 2026-11-12. Backticked paths no longer tracked: 4. Private-detail scan: owner's name 2, machine names 4, dollar figures or account labels 0.
- `docs/rationale/lifecycle-deadlines.md`: **keep**. Plan. 23,306 bytes, last changed 2026-11-12. Verbs the help does not list: `north fleet turns`.
- `docs/rationale/mayday-ladder.md`: **keep**. Live mechanism. 5,215 bytes, last changed 2026-11-11.
- `docs/rationale/measured-no-gain.md`: **keep**. Measurement. 17,851 bytes, last changed 2026-11-08. Verbs the help does not list: `north advisor-report`, `north ledger planner-band`, `north threads replay-staffing`. Backticked paths no longer tracked: 28. Private-detail scan: owner's name 0, machine names 16, dollar figures or account labels 4.
- `docs/rationale/nearest-routing.md`: **keep**. Plan. 21,323 bytes, last changed 2026-11-10. Verbs the help does not list: `north fleet need`.
- `docs/rationale/nix-current-practice.md`: **keep**. Measurement. 14,525 bytes, last changed 2026-11-07. Backticked paths no longer tracked: 4. Private-detail scan: owner's name 3, machine names 10, dollar figures or account labels 0.
- `docs/rationale/nix-footguns.md`: **keep**. Plan; the survivor of the Nix group. Its opening "Nothing here is built yet" needs a dated status line: `harness/nix/cards.toml` is tracked. 75,510 bytes, last changed 2026-11-10. Verbs the help does not list: `north capacity carry`, `north capacity exec`, `north depbox`, `north dev-shell`, `north doctor`, `north experiments`, `north land-tests`, `north skill-review-queue`, `north target-seed`. Backticked paths no longer tracked: 13. Private-detail scan: owner's name 1, machine names 37, dollar figures or account labels 3.
- `docs/rationale/nix-leverage.md`: **keep**. Outside-source review. 14,213 bytes, last changed 2026-11-09. Verbs the help does not list: `north depbox`. Private-detail scan: owner's name 4, machine names 5, dollar figures or account labels 0.
- `docs/rationale/non-rust-inventory.md`: **keep**. Inventory. 6,735 bytes, last changed 2026-11-11. Verbs the help does not list: `north capacity gate`. Private-detail scan: owner's name 1, machine names 1, dollar figures or account labels 0.
- `docs/rationale/one-landing-per-change.md`: **keep**. Live mechanism. 2,369 bytes, last changed 2026-11-04.
- `docs/rationale/operator-repository.md`: **keep**. Plan of 2026-11-12 for the box "Operator repository, plan" of north#1281, added by commit cda5987959 after this audit read the tree. It names `north operator init`, `north operator check` and `north operator paths`, which it proposes and the help does not list yet; the box "Operator repository, built" adds them. 23,582 bytes.
- `docs/rationale/operator-views.md`: **keep**. Plan. 9,438 bytes, last changed 2026-11-11.
- `docs/rationale/outcome-watchdog.md`: **keep**. Live mechanism. 2,951 bytes, last changed 2026-11-10. Verbs the help does not list: `north health facts`. Private-detail scan: owner's name 0, machine names 1, dollar figures or account labels 0.
- `docs/rationale/page-flood.md`: **keep**. Live mechanism. 3,477 bytes, last changed 2026-11-12.
- `docs/rationale/per-file-scan-verdicts.md`: **keep**. Measurement; verdict dropped. 2,772 bytes, last changed 2026-11-12.
- `docs/rationale/placement.md`: **keep**. Plan; the survivor of the capacity and placement group. 22,044 bytes, last changed 2026-11-12. Private-detail scan: owner's name 4, machine names 27, dollar figures or account labels 0.
- `docs/rationale/policy-climber.md`: **keep**. Retired design; its first line says so. 15,779 bytes, last changed 2026-11-11. Verbs the help does not list: `north ops climb`. Backticked paths no longer tracked: 2.
- `docs/rationale/private-text-judge.md`: **keep**. Live mechanism. 6,685 bytes, last changed 2026-11-12.
- `docs/rationale/propagation.md`: **keep**. Plan. 18,779 bytes, last changed 2026-11-11. Verbs the help does not list: `north runtime pinned`, `north runtime promoted`, `north runtime status`. Private-detail scan: owner's name 2, machine names 17, dollar figures or account labels 0.
- `docs/rationale/provider-parity.md`: **keep**. Live mechanism. 2,381 bytes, last changed 2026-11-11.
- `docs/rationale/proxy-role.md`: **keep**. Plan; the survivor of the coordinator group. 48,519 bytes, last changed 2026-11-12. Verbs the help does not list: `north fleet break-glass`, `north proxy`. Backticked paths no longer tracked: 16. Private-detail scan: owner's name 60, machine names 8, dollar figures or account labels 0.
- `docs/rationale/pstack.md`: **keep**. Outside-source review. 5,688 bytes, last changed 2026-11-12.
- `docs/rationale/pull-hand-over.md`: **keep**. Live mechanism. 7,171 bytes, last changed 2026-11-10. Verbs the help does not list: `north capacity sandbox`. Private-detail scan: owner's name 2, machine names 19, dollar figures or account labels 0.
- `docs/rationale/read-isolation.md`: **keep**. Measurement. 3,300 bytes, last changed 2026-10-30.
- `docs/rationale/README.md`: **rewrite**. Generated by `north docs rationale-index`; change the generator, not the file. It lists 90 notes in one alphabetical run, and for 23 of them the summary is only an owner line or a decision, plan or measurement date, which says nothing about the note. Group by kind (see "What the indexes should do") and skip those lines. 23,085 bytes, last changed 2026-11-12. Verbs the help does not list: `north land drill`, `north ops climb`. Private-detail scan: owner's name 19, machine names 12, dollar figures or account labels 0.
- `docs/rationale/repeat-runs-2026-11-06.md`: **keep**. Measurement. 5,690 bytes, last changed 2026-11-07. Backticked paths no longer tracked: 1. Private-detail scan: owner's name 2, machine names 3, dollar figures or account labels 0.
- `docs/rationale/replay-divergence.md`: **keep**. Live mechanism. 7,747 bytes, last changed 2026-11-12. Private-detail scan: owner's name 0, machine names 1, dollar figures or account labels 0.
- `docs/rationale/review-triggers.md`: **keep**. Reasoning behind a rule a skill loads. The README lists it among concept documents; it is not one. 2,884 bytes, last changed 2026-11-07.
- `docs/rationale/runtime-follow.md`: **keep**. Live mechanism. 7,346 bytes, last changed 2026-11-12. Verbs the help does not list: `north fleet watchdog`, `north runtime settle`.
- `docs/rationale/runtime-install-lock.md`: **keep**. Live mechanism. 1,712 bytes, last changed 2026-11-08. Private-detail scan: owner's name 0, machine names 7, dollar figures or account labels 0.
- `docs/rationale/rust-build-times.md`: **keep**. Measurement and plan. 32,964 bytes, last changed 2026-11-11. Verbs the help does not list: `north land profile`. Backticked paths no longer tracked: 6. Private-detail scan: owner's name 3, machine names 20, dollar figures or account labels 0.
- `docs/rationale/safe-push-composition.md`: **keep**. Plan. 53,137 bytes, last changed 2026-11-11. Verbs the help does not list: `north code-health layers`, `north land restore`, `north land route`, `north remote-land`, `north safe-push-result`. Backticked paths no longer tracked: 8. Private-detail scan: owner's name 3, machine names 8, dollar figures or account labels 0.
- `docs/rationale/scoreboard.md`: **keep**. Live mechanism. 24,611 bytes, last changed 2026-11-11. Verbs the help does not list: `north land profile`.
- `docs/rationale/self-improvement-loop.md`: **keep**. Audit and plan; the survivor of the learning group, linked from the README. 20,067 bytes, last changed 2026-11-12.
- `docs/rationale/semantic-physics-review.md`: **keep**. Outside-source review. 15,133 bytes, last changed 2026-11-07.
- `docs/rationale/skill-provenance.md`: **keep**. Licence record of imported skills. Its title is "Provenance", the name of an unrelated landing gate; retitle it "Skill provenance". 7,733 bytes, last changed 2026-11-08. Backticked paths no longer tracked: 1.
- `docs/rationale/spend-rule.md`: **keep**. Live mechanism; supersedes the usage parts of `account-capacity.md`. 18,044 bytes, last changed 2026-11-12. Verbs the help does not list: `north hook spawn-gate`, `north spend`.
- `docs/rationale/state-machines.md`: **keep**. Live mechanism; the long form behind the generated charts in `state-machines/`. 41,174 bytes, last changed 2026-11-12. Verbs the help does not list: `north code-health implicit-state`, `north health facts`. Backticked paths no longer tracked: 4.
- `docs/rationale/strategy-kernel.md`: **keep**. Plan. 46,264 bytes, last changed 2026-11-11. Backticked paths no longer tracked: 2. Private-detail scan: owner's name 1, machine names 14, dollar figures or account labels 0.
- `docs/rationale/subsystem-ledger.md`: **keep**. Measurement. 12,810 bytes, last changed 2026-11-11.
- `docs/rationale/targets.md`: **keep**. Reasoning behind a rule a skill loads. 4,447 bytes, last changed 2026-11-11.
- `docs/rationale/test-selection.md`: **keep**. Plan. 40,451 bytes, last changed 2026-11-12. Backticked paths no longer tracked: 1. Private-detail scan: owner's name 1, machine names 10, dollar figures or account labels 0.
- `docs/rationale/tier-routing.md`: **keep**. Reasoning behind a rule a skill loads; the survivor of the routing group. 18,916 bytes, last changed 2026-11-12. Verbs the help does not list: `north ledger planner-band`. Backticked paths no longer tracked: 1. Private-detail scan: owner's name 4, machine names 2, dollar figures or account labels 27.
- `docs/rationale/usage-resets.md`: **keep**. Decision record. 3,146 bytes, last changed 2026-11-10.
- `docs/rationale/verdict-reuse.md`: **keep**. Plan. 32,924 bytes, last changed 2026-11-11. Verbs the help does not list: `north capacity carry`, `north land follow`, `north land reuse`. Backticked paths no longer tracked: 1. Private-detail scan: owner's name 1, machine names 2, dollar figures or account labels 0.
- `docs/rationale/wake-paths.md`: **keep**. Measurement. 11,618 bytes, last changed 2026-11-10. Backticked paths no longer tracked: 14.
- `docs/rationale/worker-sandbox.md`: **keep**. Live mechanism; the main source for the missing safety page. 14,500 bytes, last changed 2026-11-11. Verbs the help does not list: `north depbox`, `north hook pre-bash`. Private-detail scan: owner's name 1, machine names 1, dollar figures or account labels 0.
- `docs/rationale/world-model.md`: **keep**. Decision record. 10,877 bytes, last changed 2026-11-10. Verbs the help does not list: `north graph retain`, `north land follow`, `north land profile`. Private-detail scan: owner's name 2, machine names 4, dollar figures or account labels 0.

### Rationale subdirectories

- `docs/rationale/decisions/betterleaks-scan-times.md`: **merge**. Merge into `docs/rationale/betterleaks.md` under its "Measured" heading: 2.5 kB of scan times, the only file in `decisions/`, linked only from that note. 2,545 bytes, last changed 2026-11-06. Private-detail scan: owner's name 0, machine names 6, dollar figures or account labels 0.
- `docs/rationale/frontier/ab.jsonl`: **delete**. Data of the retired frontier benchmark; `north test --help` lists no frontier verb, no code reads the file, `frontier.md` holds the results and git history holds the file. Drop its mention in `frontier.md` and `watch/2026-11-08-t3code-gemini.md` in the same change.
- `docs/rationale/frontier/results.jsonl`: **delete**. As `ab.jsonl`: one result row of a benchmark nothing runs.
- `docs/rationale/frontier/tasks.json`: **delete**. As `ab.jsonl`: the task set of a benchmark nothing runs.
- `docs/rationale/history/fleet-name-2026-11-10.md`: **keep**. Record of the two retired names; `docs/terms.md` needs it to stay. 1,096 bytes, last changed 2026-11-11.
- `docs/rationale/state-machines/after-landing-run.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/base-broken.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/capacity-lease.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/check-environment.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/claim.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/fleet-session.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/gate.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/land-ticket.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/red-main.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/row-delivery.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/runtime-canary.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/runtime-promotion.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/state-machines/threads-spool.md`: **keep**. Generated by `north check machines --write`; the landing refuses a page that differs from what the check writes. Never edited by hand. Last changed 2026-11-10.
- `docs/rationale/watch/2026-10-27-pstack.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 9,840 bytes, last changed 2026-11-11.
- `docs/rationale/watch/2026-11-04-mattpocock-skills.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 23,154 bytes, last changed 2026-11-07.
- `docs/rationale/watch/2026-11-04-pstack.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 16,297 bytes, last changed 2026-11-07.
- `docs/rationale/watch/2026-11-06-claudemd-lines.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 18,169 bytes, last changed 2026-11-11.
- `docs/rationale/watch/2026-11-06-screen.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 9,705 bytes, last changed 2026-11-06.
- `docs/rationale/watch/2026-11-07-t3code.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 27,793 bytes, last changed 2026-11-08.
- `docs/rationale/watch/2026-11-08-gemini-moats.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 12,846 bytes, last changed 2026-11-08.
- `docs/rationale/watch/2026-11-08-t3code-gemini.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 18,166 bytes, last changed 2026-11-11.
- `docs/rationale/watch/2026-11-10-nixos-release-notes.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 8,901 bytes, last changed 2026-11-10.
- `docs/rationale/watch/2026-11-10-pstack-verification.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 24,065 bytes, last changed 2026-11-10.
- `docs/rationale/watch/2026-11-10-t3code.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 14,290 bytes, last changed 2026-11-11.
- `docs/rationale/watch/2026-11-12-pstack-lead-bot.md`: **keep**. Dated review of an outside source, reached from `docs/references.md`. 11,350 bytes, last changed 2026-11-12.
