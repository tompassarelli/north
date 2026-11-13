# Sub-issue cutover: ten parents, every live box a leaf, and the box code deleted last

Plan of 2026-11-12 for north#1137: how north's work moves from checkbox lines in issue bodies to sub-issues
closed by their own check, in which order, at what cost to GitHub's limits and to the build hosts, and where
the recorded decision should not be followed to the letter. It designs and edits no code. The decision it
plans was recorded on north#1137 on 2026-11-11 (work is tracked as sub-issues closed by their landing, one
cut, every caller in the same change, no compatibility layer) and extended on 2026-11-12 (the backlog comes
down by closing boxes, not by folding issues; the end state is ten parent issues by outcome with every live
box a leaf). All times are laptop time; all counts were read at about 6:10 PM on 2026-11-12 unless a line
says otherwise.

## What was measured

- Tracker: 22 open issues in north hold 679 open boxes and 172 ticked ones (645 open when the brief for this
  plan was written; boxes are still being added, 17 of them the operator-repository build boxes on
  north#1281). The largest body is 59,397 characters. No issue has a sub-issue and none is pinned.
- Box shape, over the 679: 92 are indented under another box; 349 carry ``Done when: `CMD` `` and 541 carry
  some Done when; 107 carry `Needs:`; 15 carry a `not before` date; 31 are marked observed, headless or live;
  8 are decisions for the owner. Median length is 398 characters, the 90th percentile 967; 504 are longer than
  GitHub's 256-character title limit.
- Titles: cutting each box at its first colon, full stop or bracket gives a lead clause of median 43
  characters, but 175 leads are shorter than 15 characters (labels such as a number or a letter code), 149 are
  longer than 100, 16 name a build host and 4 name the owner. About half the boxes have no usable title in
  their own text.
- Sorting so far (four triage comments, 205 boxes, 30% of the total): 3 ticked, 5 proposed for retirement, 12
  dated boxes overdue. On the two issues whose comments list them, 15 of 139 boxes are built with their check
  not yet run, and about 10 more are satisfied on main with no commit to name.
- The live runtime is d8b8c6bae2 and main is 45 commits ahead of it. Of the 14 commits the triage names as
  having built a box, 13 are ancestors of the live runtime and one (cf057a8e67) is on main only.
- Claims: the threads ledger held 17 live claims, every one of them on a box (`item@slug`), the oldest 178
  minutes old; the brief counts about 35 across hosts at busier hours.
- Churn: main took 357 landings in the last 24 hours, a median of 13 an hour and at most 27. The box
  interface is named in 177 tracked files; 94 of them changed in those 24 hours, 269 times.
- Callers: the three patterns north#1137's second box greps for (`--box`, `issue_boxes`, `SplitBox`) match 111
  lines in 61 files. The word "box" as a unit of work is on 2,092 lines in 401 Rust files and 1,215 lines in
  148 docs files.
- GitHub, read from the live schema and the live tracker: `createIssue` takes `parentIssueId` and
  `issueTypeId`, so a linked, typed child is one mutation; `addSubIssue` links an existing issue; an issue
  exposes `parent`, `subIssues`, `subIssuesSummary`, `issueType`, `blockedBy`. All five repositories have the
  types Task, Bug and Feature. A read of one parent with two levels of children, each with labels and
  assignees, costs 203 points whatever the parent holds; a flat read of a repository's open issues, each with
  its `parent`, type, labels, assignees and summary, costs 2 points per 100 issues; a one-level read of a
  parent's children without nested lists costs 1.
- Broker: every `gh` call on a host passes one broker, which answers identical reads from one call and
  spaces writes 1 s apart. A session's fair share of GitHub calls is 400 units an hour, enforced only when
  the allowance the sessions share runs low.
- Audience: north has 0 watchers, 5 stars and 1 fork. A wider showing is planned for the weekend of
  2026-11-14 (north#1281).
- Other trackers: south (private) has 30 open issues and 137 open boxes; wisp (public) 51 and 102, at most 7
  on one issue; makgora (public) 62 and 264, at most 55 on one issue. GitHub answers `smashcraft` with
  makgora, so the five names on north#1137 are four trackers. Battlecraft, which the box does not name, has
  one open issue with 2 boxes.

## Where this plan departs from the decision as written

Four departures, each with the evidence. The end state is the decided one in all four.

1. **A leaf is closed by its own check, which a landing passes for most leaves and not for all.** The
   decision says the landing closes the issue and nothing else marks work done. On the landing-speed issue
   13 boxes have their code on main and their check not passed, because the check is a reading on the live
   runtime; 31 open boxes are marked as observations a landing can never show; makgora's rules close a visual
   box on a native capture taken after it lands. Closing those at landing would hide exactly the state the
   sorting pass found (built, not verified). So: the lander closes a leaf when the lane's commit is on
   origin/main and the leaf's Done-when command passes there; a leaf whose command needs the live runtime, a
   native client or a date stays open with `waits:runtime` and is closed by the same command when it passes.
   No report closes anything, which is the goal the decision names.
2. **The leaf path arrives before the box path leaves.** The decision says one cut with no compatibility
   layer. A pilot on a real parent needs leaves to work while boxes still exist, 17 to 35 sessions hold box
   claims at any moment, and hosts on an older runtime keep running the old code until an order reaches
   them. So the leaf path lands first, additive, and every issue is in exactly one shape: it has checkboxes
   (old path) or it has none (leaf). Nothing translates one shape into the other and no flag selects a path,
   so there is no layer to remove later, only dead code once the last box is gone.
3. **The deletion is five interface-sized changes on one day, not one commit.** Each change removes one
   interface with every caller of it, which is what "every caller in the same change" protects. One lane over
   all five would touch about 60 non-test files in 25 crates while 94 of those files change every day, and its
   land check would be the whole workspace. Once no box exists the five are dead code and do not depend on
   each other's order beyond callers before callees.
4. **One read per repository, not one per parent.** The third box of north#1137 asks for a parent and its
   children in one query. With children of children that query costs 203 points; the flat read of every open
   issue with its `parent` costs 2 per 100 issues, covers any depth and gives dispatch the whole tree in six
   calls for 560 issues. Dispatch runs every few minutes, so the per-parent shape would spend the hour's
   budget on reads alone.

Two smaller points. The second box's grep covers `docs`, which would force rewriting dated rationale notes
that record what was true when written; the staged box scopes it to code, harness and feature pages. And the
word "box" stays as north's name for a unit of work in code and prose: renaming 3,300 lines is not part of
this, and the ledger's stored `box` column is left in place and no longer written, since north keeps no
migrations.

## The ten parents

The ten are existing issues, already titled as outcomes; nothing is folded. A fold copies box text between
bodies, and three fold passes ticked no box. Here an issue that serves another's outcome is linked under it
with `addSubIssue` (one call, body untouched) and becomes the middle level, so the shape is the one asked for
on north#1137 on 2026-11-11: an outcome, the issues it is made of, the units one worker lands. Open boxes
today are in brackets.

1. **Landing speed**: north#1239 [71].
2. **Tests and builds that never repeat**: north#1247 [67], with north#1039 (tests selected by reachability)
   [40] and north#722 (after-landing gate debt, which the landing machinery files into) [26] under it. 133.
3. **The Buck2 build**: north#1115 [23]. Closes by being finished.
4. **Code size and architecture**: north#1276 [89].
5. **Self-improvement and grounding**: north#1279 [12], with north#1147 (graph grounding in the brief) [9],
   north#1233 (small tasks on the small model) [11], north#709 (concerns) [32], north#1012 (skill review
   queue, machine-filed) [7] and north#1132 (cost alarm, machine-filed) [1] under it. 72. North#1147 and
   north#1233 are experiments and close by being finished.
6. **Fleet launch and session lifecycle**: north#1054 [30], with north#1235 (named states and deadlines)
   [38], north#1048 (the coordinator's window) [27] and north#1249 (unlanded lanes) [58] under it. 153.
   North#1249 closes by being finished: each of its boxes is a lane to continue or archive, and retirement
   will take most of them.
7. **Capacity and hosts**: north#1036 [54], with north#1282 (failing units on one host) [1] under it. 55.
8. **Guarantees in every provider's layer**: north#1041 [24].
9. **The runtime canary**: north#1022 [30].
10. **The public release**: north#1281 [23], with north#1137 (this change) [6] under it. 29. Both close by
    being finished.

The counts sum to 679. No parent has more than 100 direct children before sorting (the largest is north#1276
with 89), so GitHub's limit of 100 children forces no split; a box with boxes under it becomes a third-level
issue with its children as leaves, which gives a depth of four against GitHub's eight. A better ten would
re-sort north#1249's lanes by subject, but that is the hand redistribution the fold passes showed to cost
hours for no closure, so its boxes stay together and shrink.

## The order, with the expected count at each step

Nothing is converted that should have been closed, so conversion comes last.

1. **Sorting** (running, no build): each box is ticked only when its own check passes on main, proposed for
   retirement under a named rule, or left live. At the sampled rates (1.5% ticked, 2.4% proposed) it removes
   about 26 boxes, between 12 and 60. About 653 remain. The judged pass of 2026-11-12 on north#1022 and
   north#1249 took 62 open boxes to 58 by removing four (two superseded on each issue, each recorded word for
   word in the issue's comment headed "Judged pass 2026-11-12") and queued five ticks, so 53 remain.
2. **Verification of built boxes on the live runtime.** The sample puts built-and-unchecked boxes at 11% of
   the two issues that list them, 18% on one and 3% on the other, so about 70 across the tracker, between 25
   and 120. Five kinds of check, each with its runner:
   - a command that reads main: `north task check --done-when REPO#N`, by the sorting workers, no build;
   - a named test that main has but nothing has built: one build worker on a build host through
     `north capacity offload run`, all such tests in one build;
   - a run on the load ladder (four boxes on the landing-speed issue): one ladder pass by the session that
     owns the ladder, after the order below;
   - a reading of spans or outcomes from real landings (five boxes): a no-build worker reads the lander's
     records once 20 landings have passed on the ordered runtime, under two hours at 13 landings an hour;
   - a reading on the laptop: one laptop session.
   The order: 13 of the 14 named commits are already live, so their checks can run now; the rest wait for
   the next `north runtime order` whose commit contains them, tested per box with
   `git merge-base --is-ancestor SHA LIVE`. A box is ticked only by the checker's Evidence stamp. If half to
   four fifths pass, about 45 boxes are ticked, between 15 and 95. The boxes that are satisfied on main with
   no commit to name (about 45 by the sample) are ticked the same way, by their command alone; the rule that
   a tick names a commit cannot apply to a repository setting. About 565 remain, between 470 and 625.
3. **Retirement**: the coordinator accepts or refuses each proposal, and each overdue dated box (about 40)
   is read now and either ticked, retired with its recorded reading, or given a new date. If two thirds of
   those leave, about 540 boxes are live, between 440 and 610.
4. **Conversion of the live set only.** Of the live boxes, the 8 decisions for the owner become gates and are
   named in their parent's prose, about 20 boxes with children become third-level issues, and the rest become
   leaves: about 530 issues created, between 430 and 600.

The ranges are wide because 4 of 22 issues have reported and they are the landing and test issues; the other
18 triage comments narrow them, and the dry run of step 4 gives the exact number before anything is created.

## The leaf path, and what a leaf is

A leaf is an open issue with a parent, no checkbox and no open child. Its body is the box's text word for
word, then its ``Done when: `CMD` `` and `Needs:` lines, then one line naming the issue and box it came from,
which is also what makes a second run of the converter create nothing. Dependencies between leaves use
GitHub's blocked-by relation, which dispatch already reads. A checklist inside one landing is prose.

Arrival, each piece additive and landed alone (stage 2 of the boxes):

- the flat read of a repository's open issues with their parents, in dispatch's survey, through the caching
  GitHub broker every `gh` call already passes (`crates/north-ops/src/ops/gh_broker`);
- `north issue child`, a verb of the sandbox's credential broker (`crates/north-broker`): one mutation that
  creates a typed, labelled child under a parent and refuses a title that matches an open issue, a body
  with no runnable Done when, a parent in another repository, and a private word;
- the lander closing a leaf as departure 1 describes, and never closing an issue that has checkboxes or an
  open child;
- dispatch staffing open leaves and never a parent, and never a box of an issue that already has children
  (so a box whose claim died waits for the converter's sweep);
- the brief for a leaf: `Item:` alone, and a Done when that says the issue is closed by its check;
- the fact graph reading parent, child, state and Done when from the flat read;
- the ledger counting a closed leaf as a closed box, and a run that created children as a split, so the cost
  alarm and the weekly count do not read the conversion as a collapse in closures;
- the machine writers. `rolling_issue::section_box` and the trackers behind after-landing debt, the red-main
  record, health and tune alarms, the build-time notice and the skill review queue all append `- [ ]` lines
  to standing issues. Left alone they re-create boxes within hours of the conversion and north#1137's count
  never stays at 0. Each files a child under its standing issue instead. North#1137 does not name them today.
- the filing guard. Boxes exist to stop agents flooding the tracker. The guard keeps that: a worker may file
  only a child of the issue it holds, a bounded number per session, each with a runnable Done when; an issue
  with no parent stays the owner's to authorize.

Claims and gates need no new code for leaves: a claim without a box and a gate with `--issue` and no `--box`
already cover a whole issue. The pilot proves it.

## The cut, by interface and file

Stage 5 runs when no open box is left in the four trackers. Each item is one change with all its callers, in
this order where one names another's flag:

1. **Gates' `--box`**: `src/cli/gate.rs`, `src/cli/fleet/gated.rs`, `crates/north-fleet-store/src/gates.rs`
   and `gate_lifecycle.rs`, `crates/north-fleet-supervisor/src/fleet/registry/gate_rows.rs` and
   `gate_commands.rs`, `crates/north-dispatch/src/dispatch/survey.rs` (`gate_rows`, `gate_holds`) and
   `record.rs` (`BoxHolds`), `crates/north-providers/src/ledger/gates.rs`, the gate sentence in
   `crates/north-fleet/src/fleet/prompt.md`, `src/cli/reference/0168-gate.txt` and feature page 0019.
2. **Dispatch's per-box staffing and the brief's `Box:` line**: under `crates/north-dispatch/src/dispatch/`,
   `brief.rs`, `issues.rs` (the checkbox patterns, `takeable`, `needs_box`, `owned_by`), `effects.rs`,
   `queued.rs`, `mod.rs`, `drafting.rs`, `drafts.rs`, `host_needs.rs`, `hotspots.rs`, `screen.rs`,
   `manual.rs`, `cloud.rs`, `survey/github.rs` and `deepseek/`; `crates/north-guard/src/guard/brief_skeleton.rs`
   and `unrunnable_box.rs`; `harness/overlays/deepseek.md`; feature pages 0011 and 0041.
3. **The box claim**: `src/cli/fleet/launch.rs`, `claim_box.rs` and `claims.rs`;
   `crates/north-fleet/src/fleet/model.rs`, `rungs.rs` and `claims.rs`; under `crates/north-threads/src/threads/`,
   `claims/` (`box_slug`, `same_box`, `names_box`, the `@` key), `items/mod.rs`, `runs/record.rs`, `brief.rs`
   and `settle.rs`, and `issue_boxes.rs` with its table; feature pages 0021 and 0029.
4. **The lander's body editor and ticks**: under `crates/north-landing/src/`, `issue_body.rs` (the tick, land
   and untick edits; the outbox stays for comments and closes), `land/after/tick.rs`, `land/scans/boxes.rs`
   and `landing/after_landing.rs`; `crates/north-landing-gates/src/issue_box_lint.rs`;
   `crates/north-land-scans/src/detector_proof.rs` and `stranded_boxes.rs`;
   `crates/north-ops/src/ops/done_when_boxes/` (the check stays, run on a leaf);
   `crates/north-runtime/src/worker_sandbox/lane_grant.rs`; the `Box:` trailer in
   `crates/north-safe-push/src/shell/` and `src/cli/cloud_land.rs`; feature pages 0015, 0037 and 0007.
5. **The readers**: `crates/north-graph-ingest/src/issues.rs` and `ledger.rs`,
   `crates/north-graph-ground/src/issue.rs`, `docs/graph.md`; `src/cli/box_path/`;
   `crates/north-providers/src/ledger/closed.rs`; `crates/north-calibration/src/threads/calibration/boxes.rs`;
   the budget notice in `crates/north-code-health/src/build_time/issue.rs`.
6. **The rules**: `harness/skills/github/references/issues.md` and `SKILL.md`; `harness/skills/workers/SKILL.md`
   with `references/brief.md`, `run-and-land.md` and `cloud.md`; `harness/claude/agents/planner.md` and
   `worker.md`; `harness/gates.toml`; north's `AGENTS.md`; makgora's `AGENTS.md` (the line that sends a
   finding to a checkbox); the two lines of south's policy and the one of wisp's that say "box". It builds
   nothing and lands first of the six, on the day the last tracker is converted: rules that forbid boxes
   while most issues still hold them would have agents filing children the tracker is not ready for.

**What a session holding a box claim sees.** Nothing. The converter never touches a box with a live claim:
it converts the unclaimed boxes of a parent, leaves the claimed ones as lines in the body, and a later sweep
converts or removes them after their sessions land or end. A session that claimed a box finishes on the box
path it started on, and its landing ticks the line as it does today. By stage 5 no such session exists.

**What a quiet hour has to mean.** Under the letter of the decision it would mean all of this at once: dispatch
paused on every host, no claim with a box, no queued launch or follow that names a box, no open gate with a
box, an empty landing queue and an empty issue-body outbox, and every host on the cut's runtime. Reaching it
means waiting out the longest live session (178 minutes today) with the fleet idle, on hosts that are the
scarce thing, and it gives no way to stop halfway. This plan needs no fleet-wide drain. Conversion holds one
parent at a time and skips claimed boxes. Stage 5 starts when four counts are zero, each a command:
open boxes in the four trackers, `threads list --json` rows with a box, open gates with a box, and files in
the issue-body outbox. Its changes land like any other, and one `north runtime order` carries them to every
host, checked with `north runtime lag --json`. If the letter is wanted instead, the pause is
`north fleet dispatch pause --reason TEXT --by WHO --check COMMAND` with those counts as its check, and the
resume is the tick ending the pause when the check passes.

The pilot does need one order first: a host on a runtime without the leaf path skips a leaf as an issue with
no box and its lander would land a leaf's lane without closing it, so no leaf is created until
`north runtime lag --json` shows every dispatching and landing host on a runtime that contains stage 2.

## GitHub's limits and how long the conversion runs

- **Calls.** One query per repository for its id, label ids, type ids and the parents' node ids. One
  `createIssue` per leaf, carrying parent, type and labels. One `addSubIssue` per existing issue that moves
  under a parent, 12 in north. One body edit per parent, after its leaves exist. One flat read to confirm.
- **Limits.** GitHub documents secondary limits of 80 content-creating requests a minute and 500 an hour per
  actor (docs.github.com, "Rate limits for the REST API", secondary limits), and 2,000 GraphQL points a
  minute with a mutation counted as 5; a parent takes 100 children and nesting goes 8 deep. The hour limit
  binds: the converter creates at most 450 an hour (one every 8 s, above
  the broker's 1 s spacing), leaving the rest for the fleet's own comments, and stops for the stated wait on
  any 403 or 429.
- **The shared allowance.** The creations are a small part of an hour's GraphQL allowance; the fleet's
  reads are the larger part, which is why the flat read matters more than the pace of creation. The
  converter runs as one named session, which will pass its 400-unit fair share and be told so; it is refused
  only if the shared allowance falls under its reserve, and it then waits, since the fleet's calls matter
  more than its pace.
- **Duration.** North's 530 leaves take about 70 minutes of creating, split into two runs of at most 45
  minutes. South, wisp and makgora hold 503 open boxes before their own sorting; at north's rates about 400
  leaves, another 55 minutes, but wisp's issues are mostly already one unit of work (at most 7 boxes), so its
  conversion is mainly checkboxes becoming prose. The limits are per actor, so the four trackers go one after
  another, not side by side.
- **Afterwards.** Dispatch's survey is the flat read: six calls and 12 points for 560 open issues, against
  one body read per issue today.

## What an outsider sees

A few days before a wider showing the Issues tab will say about 560 open, newest first. No repository setting
for a default issue filter is known (if the pilot finds one it replaces the link below), so legibility comes
from four things:

- **Types.** The ten parents and the issues under them are Feature; leaves are Task, or Bug when the leaf
  names a defect with a repro. No new type and no new label: leaves take their parent's `area:` label and
  the `category:` label of their own verification class, and `waits:runtime` where departure 1 applies.
  Priority stays on the parent.
- **The parents view.** The README and the release page link to the list of open issues with no parent, which
  shows ten rows. The filter is `no:parent-issue`. On today's tracker `has:parent-issue` returns 0 where a
  made-up qualifier returns all 22, which suggests the pair is recognised, but with no child in the tracker
  that is not proof, so the pilot measures it.
- **Pins.** GitHub pins three issues at the top of the tab: the release parent, landing speed, and the
  self-improvement parent.
- **Titles.** A leaf's title is a sentence a stranger can read, at most 80 characters, with no letter code,
  build-host name, account label or the owner's name. Half the boxes have no such sentence in their own text,
  so the sorting workers write the titles into the dry run's manifest; the converter does not derive them.

Box text that should not become a public title: the 16 leads that name a host and the 4 that name the owner
(rewritten); the boxes on north#1041 that describe a way around a guard that is not closed yet (the title
states the guarantee, never the route; the body is already public and stays as written); the 8 decisions for
the owner (gates, not issues); and anything in south, whose leaves stay in south, since a public parent with a
private child would show that the child exists. A milestone names the dated push: the leaves under north#1281
carry one for the weekend.

Creating 530 issues notifies every watcher 530 times. North has no watchers today, so before the showing is
the cheap moment and after it is not.

## Pilot and reversal

**Pilot one: north#1147** (9 open boxes, no live claim, priority now, so its leaves are staffed at once). It
passes when, on the ordered runtime, all of these hold, each a command or a count:

- `north fleet dispatch --dry-run` lists a leaf of north#1147 and not north#1147;
- a launched session's `tasks` in `north fleet list --json` is the leaf, its claim in `threads list --json`
  has no box, and a second launch on the same leaf is refused;
- a gate opened with `--issue` naming a leaf holds that leaf's launch and no other;
- three leaves are closed by the lander within 60 s of their landing, each with a closing comment naming the
  commit, and `subIssuesSummary.completed` on north#1147 is 3;
- `north ledger closed` counts those three as closed boxes;
- the broker's call log shows the survey read as one call per 100 open issues;
- the list of open issues with no parent is 9 shorter than the list of all open issues;
- no session of the pilot is refused for a reason that names a box.

**Pilot two: north#1115** (23 boxes, two live claims), to show that claimed boxes are left in the body, their
sessions land and tick as before, and the sweep then clears the body.

**The dry run.** `north issue convert REPO#N --dry-run` writes a manifest with one row per open box: its
verdict (leaf, third-level issue, prose, gate, held by a claim), title, type, labels and body. Machine checks
refuse a manifest whose rows do not add up to the body's open boxes, a title over 80 characters or equal to
an open issue's, a leaf with no runnable Done when or declared class, and any row the landing's own privacy
scan refuses. The coordinator reads every flagged row and a random 30. The create run takes the manifest and
its hash and creates exactly those rows, so nothing unreviewed is created.

**Reversal.** Per parent: restore the body from the copy the converter saved (GitHub also keeps the edit
history), close the parent's leaves as not planned and unlink them, two calls a leaf. Before stage 5 that
returns the parent to the box path completely, which is why stage 5 is last. What cannot be undone: issue
numbers once used, notifications sent, the "sub-issue added" events on a parent, and the cross-reference
events a leaf's body leaves on every issue it mentions.

## The other trackers

North first, because the code and both pilots are there. Then:

1. **South** (private, 137 boxes on 30 issues): next, because nothing in it is public, so it tests the
   converter on a second repository at no exposure. Its leaves must stay private; its policy text says "box"
   in two rules about parking a decision, which become "issue".
2. **Wisp** (public, 102 boxes on 51 issues, at most 7 each): most issues are already one unit of work, so the
   converter's usual verdict is prose and few leaves are created. One rule cites a box by number and takes
   the leaf's number.
3. **Makgora** (public, 264 boxes on 62 issues, up to 55 on one; `smashcraft` is the same tracker): last and
   the most different. It already has a root and epics joined by text, which become real links. Its rules
   send a new finding to a checkbox, which becomes a child of the epic. Its visual and native boxes land
   before their capture and close on the capture, which is departure 1's rule and the reason the lander must
   not close on landing alone.

Each gets its own sorting pass first, since converting unsorted boxes is the mistake the order exists to
prevent. Battlecraft's two boxes become prose by hand.

## Stages, build cost and the two open choices

The boxes are on north#1137. Stage 1 (verification, two boxes) and stage 2 (the leaf path, ten boxes) run
side by side from now. Stage 3 (the converter's two boxes, then the two pilots) follows stage 2 and one
runtime order. Stage 4 (north in two runs, then south, wisp, makgora) follows a passed pilot and the end of
sorting, verification and retirement. Stage 5 (the rules, then five deletions) follows a count of zero.

Build cost: the pilots and stage 4 build nothing, and stage 1 takes one build for the tests nothing has built
yet. Stages 2, 3 and 5 are 17 lanes that build, each checking one to three crates. One measured lane's clippy
and tests took 21.8 s side by side on a warm build host and 52 to 228 s
when it waited for a lease, so the cost is lease waits, not compile time: at most three of these lanes at a
time on the build hosts keeps them off each other's leases, and the stage 2 lanes that touch the lander and
dispatch go first because the pilot waits on them.

Two choices are not this plan's to make. Each is given as what the plan assumes, why, and the alternative
with its cost.

**Direction: whether the leaf path arrives before the box path is deleted.** The plan assumes it does, as
departures 2 and 3 describe. That lets a real parent be piloted and reversed, never idles the fleet, and can
stop at any parent boundary if the weekend arrives first; the cost is some days in which north carries both
paths, with each issue in exactly one. The alternative is the letter of the decision, one change in a
fleet-wide quiet window. It is the cleanest history and leaves no period with two paths, and it costs a
drain as long as the longest session, no pilot outside a scratch repository, and a single lane against 269
daily changes to the files it rewrites.

**Publishing: whether north's leaves are created before the showing or after it.** The plan assumes before,
parent by parent as each dry run is reviewed, under the title rules above. The tracker is then evidence for
what north claims (closure read from a check), nobody is watching yet to be notified 530 times, and whatever
is not converted by the weekend stays as boxes under the same ten parents, which is no worse than today.
The alternative is to show north with today's 22 issues and convert the week after. The list stays short
for newcomers, and it costs showing bodies of up to 59,000 characters with 600 checkboxes, 530
notifications to each new watcher, and a tracker that changes shape just after people first read it.

## A lead for a parent: the session tree follows the issue tree (2026-11-13)

Plan of 2026-11-13 for the box on north#1137 under 'The session tree follows the issue tree, by rule'. It
designs and edits no code. The decision it plans: the session tree comes from the issue tree. A parent issue
with more work in flight than one session can follow gets a lead. Dispatch staffs that lead from a reference
in the workers skill, the same rule repeats down the issue tree, and a lead ends when its subtree is done.
The coordinator's limits on the decision govern it: every bound is a `harness/defaults.toml` key, a lead
counts toward the caps and holds that bind a worker, the plan raises no cap, existing rules are reused first,
and lead staffing lives in north-dispatch.

### Measured basis (fleet records of 2026-11-13, laptop time)

- Direct reports. At 9:27 AM the two hand-launched burn-down leads held 11 and 16 child sessions, with 6 and 7
  of them working and the rest idle or parked. The Buck2 stream's lead held 5, and the flow lead started 5
  lanes in its first five minutes. A lead kept 6 or 7 sessions working whether it held 11 or 16. The rest
  waited on its review or restaff.
- Messages. From 5 AM to 9:27 AM the fleet ledger addressed 331 messages to the coordinator (about 75 an
  hour). It addressed 149 to the first burn-down lead, 64 of them from the coordinator, and 101 to the second,
  36 of them from the coordinator in 1.8 hours. Most of the coordinator's messages were runtime orders and
  rulings relayed to leads.
- Unanswered wait. A message row holds `sent_at` but not the time its session took it, so how long a lead's
  messages waited cannot be read today. Slice S5 records that time.
- Context. Today's leads ran Opus at medium effort with compact windows of 200k to 400k tokens. The session
  host's checkpoint (`crates/north-fleet/src/fleet/checkpoint.rs`) already reads a session's context and asks
  at 250k tokens.

### Reused, and what each lacks for a lead

- Dispatch's survey and leaf staffing (boxes 2a, 2d). One query gives every open issue's parent, and dispatch
  staffs leaves and unconverted boxes at any depth, never a parent. What it lacks: its launches carry no
  `--parent`, so every session falls to the proxy (`src/cli/fleet/lineage.rs`).
- Fleet records' `role` and `parent`, the upward rule (`crates/north-fleet/src/fleet/upward.rs`) and
  `north fleet tree`. Reports already go to whatever parent a session names. Nothing is missing.
- Take-over (`north fleet launch --take-over --name NAME`). It moves the name and untaken messages. What it
  lacks: nothing starts it for a lead at its context limit.
- The reaper's `fleet_stale_hours` rule. It retires an idle or parked session after 2 quiet hours and keeps
  one that has a live child, so a lead whose subtree waits on a gate is retired 2 hours later. What it lacks:
  it ends a lead on silence, not on its subtree, and never ends a lead that keeps being woken. It stays as the
  backstop under S1a's end rule.
- The filing guard and `worker_children_per_session` (5). The session holding an issue may file 5 children
  of it, and a lead holds its parent's claim, so it can split work under that parent. What it lacks, to be
  read from S1a's records first: a lead that groups a flat parent may need more than 5. If the records show a
  lead refused, the remedy is a key of its own beside this one, never a number in the skill.
- The stalled-plan and unstaffed-box detectors (`crates/north-flow/src/detectors/plan.rs`). They page the
  coordinator and dispatch for ready, unstaffed work. What they lack: the page should reach the parent's lead
  when it has one. That is a later change. Until then dispatch staffs the unstaffed box anyway.
- The `proxy_role` table. It binds only the proxy (`crates/north-guard/src/guard/proxy_role.rs`). A lead gets
  no rows, because the table forbids reading source, and a lead's review reads diffs. The one lead-specific
  limit is S3's refusal of `--role lead` launched from a lead or worker. Rows for leads are revisited only if
  lead records show one tracing by hand instead of staffing.
- The 45-minute leash (`overtime_min`). Its only consumer, `crates/north-guard/src/guard/worker_lease.rs`,
  nudges Agent-tool subagents of a worker type, never a fleet session, so it does not treat a lead as an
  overtime worker. What does reach a lead: the silent-child note (`crates/north-fleet/src/fleet/silent.rs`)
  tells the parent when a child goes quiet for twice its brief's ETA, and a lead never sends a final report.
  S3 makes that note skip sessions whose role is lead. Until then the lead brief's ETA line reads `n/a` with
  the reason that a lead ends with its subtree.
- The caps: `dispatch_claude_sessions` (12), the spend rule's money ceiling, and the launch hold by the build
  hosts' measured wait (north#1036, slice A2). A lead launch passes all three in the same plan step as a
  worker launch, and nothing here raises them.

### The rule

- Load. Each dispatch run computes a load for every open parent: its own and its descendants' boxes and
  leaves that are ready or held by a live dispatched session, less those under a deeper parent that has its
  own lead.
- Staffing. A parent whose load exceeds `dispatch_lead_reports` gets one lead, at most `dispatch_leads`
  at once, largest load first. Dispatch launches `north fleet launch --role lead --task OWNER/REPO#N --name
  lead-REPO-N --parent L`. L is the lead of the nearest ancestor that has one, else the proxy. The lead claims
  its parent. Dispatch never staffs a parent as work, so that claim conflicts with nothing, and with the name
  it refuses a second lead. A live session with role lead holding the parent's claim, including one launched
  by hand, counts as that parent's lead. So does a hand-launched session with any role or none (the burn-down,
  flow and evals leads of 2026-11-13 hold no claim on a parent) when it is not the proxy and is the fleet
  parent of a live session holding an item in the subtree. Dispatch staffs no lead for that parent while such
  a session lives.
- Placement. A lead launch goes through the same placement step as a worker launch: `plan::place` with its
  candidates' refusals (best, reserve and hold in the operational basis, the API hold, the 98% subscription
  floor, the reserved account), never an API account, and the spend rule's `admits`.
- One staffer. Dispatch still staffs every box and leaf, and a launch under a parent with a lead carries that
  lead as `--parent`, so its reports reach the lead by the existing upward rule. The lead reviews, rules within
  its authority and files children under its parent to split work. A stop is restaffed by dispatch taking the
  item back on its next run. When a lead does restaff by hand, that launch meets dispatch's own bounds:
  `dispatch_claude_sessions` already counts every working worker session fleet-wide (`spend.working`), so a
  lead's hand launch counts against the cap in dispatch's next decision. What `north fleet launch` lacks is a
  check of the cap at launch time, so S3 adds that check for a launch from a session whose role is lead. The
  money ceiling, the floor and A2's hold already apply at that launch's placement.
- Recursion. The same rule applies at every level. A child that is itself a parent with load over the bound
  gets a lead parented to the lead above it, and its subtree leaves the upper lead's load.
- Route. The lead brief carries `Category: lead`, which the router maps to Opus at medium effort, as every
  lead of 2026-11-13 ran. That is one row in `crates/north-pace/src/router/brief.rs`, not a hand-picked model.

### The bounds, as harness keys

- `dispatch_lead_reports`, value 8: the load over which a parent gets a lead. Its unit is boxes and leaves
  ready or held in a parent's subtree. Its source is a prior: today's leads kept 6 or 7 working, and 8 is one
  build host's resident cap. Its fallback is none, since it is a stated bound. Its consumer is
  `crates/north-dispatch/src/dispatch/leads.rs`. It settles when 5 dispatched leads have ended with their
  peak direct reports and, after S5, their median message wait. A lead of a few hours carries hundreds of
  messages, so 5 ends give enough rows, where 20 ends at one lead at a time would take weeks. It is then
  re-set to the largest direct-report count at which that wait stays under the key S5 adds (prior 5 minutes).
- `dispatch_leads`, value 1 in S1a: leads dispatch itself launched that are live at once. Hand-launched leads
  do not count toward it, though they cover their parent as above. Its source is a prior, one lead until the
  first is seen to work, and it settles on the same 5 ends. S2 sets its own prior of 3 in the same key: a
  nested lead under a top lead, plus the second top lead the morning of 2026-11-13 ran at once.
- Both keys settle through one counter, `count_lead_runs`, over dispatch's records of lead launches and ends.
  The counter goes in `crates/north-ops/src/ops/defaults_measure/priors.rs`, in S1a.
- Context is not a bound. A lead at its checkpoint hands over (S4) and is not split.

### Splitting and ending, without a person

- Split. When a lead's direct reports (its live child sessions plus the ready items it would receive) exceed
  `dispatch_lead_reports`, dispatch staffs a lead for its child parent with the largest load of at least 2.
  New launches under that child go to the new lead. Live sessions keep their parent until they end, within one
  box length, so nothing moves mid-turn. When a parent's own leaves alone exceed the bound, no child can take
  them. Dispatch then holds that parent's direct work at the bound, and the lead reference tells the lead to
  group the leaves under child issues.
- End. Dispatch stops the lead in the second consecutive run that finds its parent closed, or its subtree with
  nothing ready and no live child (every item done, parked, gated or waiting on the runtime). Two runs keep it
  from stopping a lead that is about to file its next child or is waiting on a landing. The stop writes a
  dispatch record row and a fleet record naming the reason, so a reviewer can tell it from an unexplained stop.
  The lead writes each ruling as a comment on its parent when it makes it, because a successor has only the
  issue tree and the gates. Dispatch staffs another lead if the load passes the bound again.
- Hand-over. At the session host's 250k-token checkpoint, a session whose role is lead is taken over under its
  name (`--take-over`) instead of being asked for turns left. The successor gets the same brief plus its
  subtree from `north fleet tree` and the open gates under its parent.

### Who talks to whom

- The coordinator talks to top leads only, a worker reports to its lead, and a lead reports to its own parent.
  Upward messages stay by exception, as the skill says today. A report goes up for a closed subtree, a
  landing, a refusal or red main the lead cannot fix, a `Blocked:` naming a gate, or a ruling from the list
  below.
- Runtime orders. The decision on north#1022 removes them, and until then the coordinator sends each one to
  top leads only. The one relay left after that is "the fix you wait on is live". Slice S7 replaces it with a
  wait the session registers itself, `north fleet wait runtime:SHA`, which the tick settles when the live
  runtime of the host the waiting session runs on contains SHA. It reads that host's own runtime because hosts
  lag one another, and it never settles on a runtime commit it cannot read.
- Rulings a lead lacks authority for, a closed list. A lead escalates only these to its parent, and a top lead
  escalates them to the coordinator:
  1. A change to another parent's scope, order or priority, or moving work between top-level parents.
  2. A fleet-wide shared setting: a harness key, a host or placement rule, closing a build host, a landing
     hold or train.
  3. Retiring, parking or rewording a box, leaf or Done when that the owner or coordinator wrote.
  4. Policy text: a skill or AGENTS.md change, which goes through the agent-policy route.
  5. A model rung past the routing rules.
  6. A matter on the owner's ask-once list. The lead opens that gate itself with `--ask-once`, so no relay is
     needed.
  Everything else is the lead's own call: restaffing, splitting, order within its subtree, review, landing and
  filing children.

### What must not happen, and the check for each

- Leads staffing leads without bound. Only dispatch, or the proxy by hand, launches `--role lead`. S3's
  launch refusal covers a lead or worker trying it, and `dispatch_leads` covers dispatch. Check: S3's test and
  S1a's cap case.
- A tree deeper than the issue tree. A lead's parent is the lead of an ancestor issue and there is one lead
  per issue, so the session depth cannot exceed the issue depth. Check: S2's test, plus a dry-run line for any
  lead whose parent session holds no ancestor of its issue.
- Two leads for one parent. The parent's claim and the name `lead-REPO-N` each refuse a second, and a
  hand-launched lead covering the parent stops dispatch from staffing one. Check: S1a's second-run case, and
  its case of a hand-launched lead with role worker or none whose child holds a leaf of the parent.
- A lead's hand launch around the bounds. A lead launching work by hand would bypass dispatch's cap if
  nothing checked it at launch. Check: S3's case that a lead's launch is refused at the
  `dispatch_claude_sessions` cap.
- A lead outliving its subtree. S1a's end rule runs every dispatch run, and the reaper is the backstop. Check:
  S1a's closed-parent case, and live, no lead in `north fleet tree` whose issue closed more than one dispatch
  interval ago.
- Sessions past the money ceiling or the measured-wait hold. A lead launch passes the spend rule's `admits`,
  takes a slot under `dispatch_claude_sessions` and, once A2 lands, meets its hold, because a lead's only
  output is the work under it. Check: S1a's cases of an account at the ceiling, an account under the API hold
  and the reserved account.

### Precondition and price of the first slice

- Precondition. South's dispatch providers setting does not include Claude today, so S1a can be seen in the
  dry run today (`north fleet dispatch --dry-run` prints `lead OWNER/REPO#N: load L over 8`). It runs live once
  that setting admits Claude, which the coordinator switches when the measured-wait launch hold (north#1036
  A2) is live.
- Price, priced as the code-health reviews price. The cost is about 80 agent-minutes: S1a one Opus lane of
  about 45, S1b about 20, review about 15. The saving per use is about 10 coordinator-minutes, a first guess
  for one lead brief, its launch and the first relays, plus the stream's wait for a lead, which on
  2026-11-13 ran hours for the flow stream. At `dispatch_leads` 1, with a lead lasting hours, uses run about 3
  a day, so payback is about 3 days on the coordinator-minutes alone, at the bar. S2's prior of 3 brings uses
  back to the 4 a day of 2026-11-13 (burn-down twice, flow, evals) and payback to about 2 days.

### Slices

S1 is two boxes on north#1137, the coordinator's text:

- S1a. Dispatch staffs one lead for a parent whose load (its own and its descendants' boxes and leaves that
  are ready or held by a live dispatched session, less those under a deeper parent with its own lead) is over
  dispatch_lead_reports, at most dispatch_leads at once, largest load first; launches under that parent carry
  the lead as their parent; the lead is stopped in the run that finds its parent closed or its subtree with
  nothing ready and no live child; a lead takes a slot under dispatch_claude_sessions and passes the money
  ceiling like a worker; a second lead for one parent is refused.
  Files: `crates/north-dispatch/src/dispatch/leads.rs` (new), `plan.rs`, `effects.rs`, `brief.rs`,
  `report.rs` and `tests.rs` in that directory; `harness/defaults.toml`; the `lead` row in
  `crates/north-pace/src/router/brief.rs`; `count_lead_runs` in
  `crates/north-ops/src/ops/defaults_measure/priors.rs`. Test: `cargo nextest run -p north-dispatch -E
  'test(/a_parent_over_the_bound_gets_one_lead_and_loses_it_when_its_subtree_closes/)'`. Its cases, after the
  commit review: a parent with 9 ready items gets one lead and its launches carry `--parent`; a second run with
  the lead live staffs none; a hand-launched lead with role worker or none, whose child holds a leaf of the
  parent, stops dispatch from staffing one; an account at the money ceiling, under the API hold or reserved
  takes no lead; and a closed parent's lead is stopped on the second consecutive run, not the first. Size:
  one Opus lane, about 45 minutes. Reviewer: commit-reviewer.
- S1b. The workers skill carries a lead's standing rules in one reference (references/lead.md), with one
  pointer in the skill and the fleet reference's line on when a lead exists replaced; changed through the
  agent-policy route.
  The reference carries this section's rules from One staffer through the closed list and hand-over, with no
  number in it. In `harness/skills/workers/references/fleet.md`, the line 'A lead exists only for a
  multi-session workstream' gives way to dispatch staffing a lead for a parent over the bound. The clause in
  `harness/skills/workers/references/brief.md` saying an issue with many boxes needs no lead also goes, since
  it says the opposite. Why: a lead's brief must be policy, not a message the coordinator wrote that morning.
  Size: about 20 minutes. Reviewer: agent-policy.

After S1, each slice with its files, test, size and reviewer:

- S2, recursion and split. Nested leads, and a lead over the bound split along its children. `dispatch_leads`
  takes S2's prior of 3, then moves only on its settled reading. Files: `crates/north-dispatch/src/dispatch/leads.rs`, `tests.rs`.
  Test: `a_lead_over_the_bound_is_split_along_its_children_and_never_deeper_than_the_issue_tree`. Size: about
  30 minutes. Reviewer: commit-reviewer.
- S3, only dispatch or the proxy launches a lead, and the silent-child note skips a lead. Files:
  `src/cli/fleet/launch.rs`, `src/cli/fleet/lineage.rs`, `crates/north-fleet/src/fleet/silent.rs`. Tests:
  `a_lead_or_worker_cannot_launch_a_lead` and `a_quiet_lead_raises_no_silent_child_note`. After the commit
  review, a launch from a session whose role is lead also meets the `dispatch_claude_sessions` cap. That is a
  follow-up on S3's box if its first lane is already out. Size: about 30 minutes. Reviewer: commit-reviewer.
- S4, a lead's hand-over at its checkpoint. Files: `crates/north-fleet/src/fleet/checkpoint.rs` and the
  session host's checkpoint call. Test: `a_lead_at_its_checkpoint_is_taken_over_under_its_name`. Size: about
  45 minutes. Reviewer: commit-reviewer.
- S5, a message's taken time is recorded, so `dispatch_lead_reports` settles. Files: the message row in
  `crates/north-fleet-store` and its delivery in the fleet tick, plus a `lead_message_wait_min` key (prior 5).
  Test: `a_message_row_records_when_its_session_took_it`. Size: about 30 minutes. Reviewer: commit-reviewer.
- S6, the coordinator speaks to top leads only. A coordinator message to a session under a lead is refused,
  naming that session's top lead, unless it is sent urgent, and the urgent send is recorded. It is not quietly
  re-addressed, because a message written for a worker reads wrongly to its lead, and an urgent stop or
  interrupt must reach the worker without delay (coordinator ruling and commit review, 2026-11-13). Files: the
  send path and `crates/north-fleet-store/src/fold.rs`. Test:
  `a_proxy_message_to_a_worker_under_a_lead_names_its_top_lead`. Size: about 30 minutes. Reviewer:
  commit-reviewer.
- S7, a wait on the live runtime of the waiting session's own host, never settled on an unreadable runtime
  commit. Files: `crates/north-fleet-store/src/store/waits.rs` and the tick. Test:
  `a_runtime_wait_settles_when_the_live_runtime_contains_the_commit`. Size: about 30 minutes. Reviewer:
  commit-reviewer.

Measures read after S1 and S2: the coordinator's messages per hour to sessions other than top leads (goal
0); messages to the coordinator per hour (75 on the morning of 2026-11-13); and minutes from a parent's load
passing the bound to its lead's first turn (goal: the next dispatch run).
