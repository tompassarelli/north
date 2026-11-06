# Architectural drift: a measured loop, not only a periodic review

Decided 2026-11-06 under north#636, after Tom shared a community skill for untangling a codebase in one pass and a
model's rewrite of it, and asked how it should fold into north. The source is
[untangle-codebase](../references.md#untangle-codebase). North already had most of it: `code-health` measures structure, `code-health tick` starts a review when its
signal fires, and the review brief tells the worker what to do. North adds no untangle skill. Each idea goes to the
part of north that already owns its kind of decision.

## The question

Every structural decision answers one question: should these pieces change together? Pieces that must change together
belong together with the link visible in the code, pieces that change for different reasons are not tied, and
dependencies point at what changes less. Judgment answers it badly; git history and the import graph answer it with
data.

## Where each idea lives

- Measurement, `crates/north-guard/src/code_health`: `north code-health budget [REV]` prints each crate against its
  `[crate_lines]` budget, then three coupling signals over the `[[module]]` map:
  - hidden links: module pairs whose files changed together in at least three commits of the 90-day window with no
    `may_use` edge either way (`coupling.rs`, from the co-change pairs `history.rs` already computed);
  - volatile imports: a module importing one that changed at least three times as often in the window and in at least thirty
    commits (stable code that imports volatile code has to change whenever it does);
  - special cases per module: lines that swallow an error, sleep or retry, or disable a test.
  It closes with the change cost since the last review: the median hand files a feature commit touched and the share
  of feature commits later reverted (`change_cost.rs`). The rewrite's co-change script was the idea; the port is Rust,
  in the module that already walked the history.
- Budget: a landing that grows a crate already over its `[crate_lines]` cap is refused unless a commit in it records
  why with a `Code-health-allow: crate-lines <crate> — <reason>` trailer, the same escape the file, function and
  hotspot rules have. The report lists every allow as debt for the next review.
- When a review runs: the `[review]` table in `structure.toml` gains `hidden_links`, `volatile_imports` and
  `special_cases`, each the growth since the last review that fires it (0 turns it off). The tick runs
  `code-health signal --record`, which writes the change cost, the thresholds in force and the reasons that fired to
  `code-health/<repo>/loop.json` and a one-line score beside it. `harness/loops.toml` registers this as the closed
  `code-health-review` loop: the policy climber may move the thresholds in `structure.toml`, judged by files touched per
  feature commit and rework since the review, not by a fixed schedule.
- Procedure, `harness/code-health/review-brief.md`: freeze what is consumed outside the repo, take a baseline first,
  rank fixes by problems solved times churn over cost so untouched tangled code stays, find the one fix behind several
  problems and name it by the change it makes cheaper, land small behavior-preserving commits behind characterization
  tests, sort each special case as a missing concept, a real requirement or a workaround for something outside the
  code, log bugs instead of fixing them, staff fresh reviewers, and stop after a clean round, when fixes undo each
  other, or after three rounds.
- Doctrine, `agent-machinery/skills/program-craftsmanship/references/structure.md`: the change-together question and
  the three kinds of coupling it catches (hidden links, needless links, surplus).

## Left where it already lives

- Removing comments: north allows no code comments, and the hooks enforce it.
- Pruning tests: the verification skill, the mutation gate and the test-admission budget own it.
- The `/goal` plumbing and the status line: a fleet brief carries its Done when and the worker's final message is its
  report.

## Rejected

- A separate untangle skill run on demand: it would be a second, unmeasured path beside the review the tick already
  starts, and it would answer the change-together question by judgment where history can answer it.
- A fixed review schedule: a schedule spends reviews on quiet code and misses a burst; the signal fires on growth and
  the loop tunes the thresholds against what the reviews bought.
- Absolute coupling counts as triggers: existing coupling would fire every hour. Growth since the review fires once per
  new problem.
