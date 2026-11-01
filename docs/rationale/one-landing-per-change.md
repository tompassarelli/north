# One landing per change

A change lands on main as one commit. `safe-push --to main` folds the lane's unpublished commits
(those no origin ref reaches) into one commit before its land checks, so a lane's fixups, records
and re-recorded tests travel with the change they belong to. Commits already on origin are never
rewritten: the fold starts after the newest published commit the lane holds. The trailers of every
folded commit (`Test-kind`, `Reverts`, mutant names, `Provenance`, `Reviewed` and the rest) are kept
once each, in order of first appearance, because the after-landing gates read them from the landed
range. The message is `--message FILE` when the worker gives one, else the first commit's message
with the other subjects listed under it. `--several-commits REASON` lands the commits as they are
and logs the reason in `~/.local/state/safe-push/several-commit-landings.tsv`. The fold runs before
the Autoland queue push, so a `claude/land-*` branch carries the folded commit too.

The rule for agents follows from it: regenerated records, re-recorded tests or goldens, doc lines
and mutant names go into the lane before landing, not into a follow-up landing. A check that can
only write its record after landing folds the record into the next landing instead of committing
on its own.

## Why

Tom asked on 2026-11-01 for one landing per change, to cut noise in the commit logs without
rewriting published history. Squashing at landing time meets both: the lane's own history is
private until it lands, and nothing on origin moves.

## Measuring the follow-up share

A follow-up is a commit on main whose subject starts with `Record `, `Regenerate `, `Re-record `,
`Name the mutants` or `Pin `. The prefix match over-counts a little (some `Pin Wisp ...` and
`Record ...` commits carry a real change) and under-counts fixups named otherwise; it is meant for
comparing weeks, not for judging one commit. Bot commits from after-landing workflows
(`Regenerate reports for ...`) count, since they are the same noise. In each checkout:

```sh
git fetch -q origin && git log origin/main --since='7 days ago' --format=%s \
  | awk '{n++} /^(Record |Regenerate |Re-record |Name the mutants|Pin )/{f++} END{printf "%d commits, %.1f/day, %d follow-ups (%.1f%%)\n", n, n/7, f, 100*f/n}'
```

The baseline before the fold landed, and each re-measure, is in South
(agents/decisions/harness/one-landing-per-change.md).
