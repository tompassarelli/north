---
name: repo-safety
description: >-
  Edit and publish ~/code repos via worktrees and safe-push; protect main checkouts, pins and peer work; keep disc images and game files out of Git; bound file and process searches.
grounded: 2026-11-10
written: 2026-11-10
metadata:
  kind: playbook
---

# Repository safety

- Edit `~/code/<project>/worktrees/<slug>` and preserve main/pins, because main and pins carry Tom's and peers' live state; use [notes](references/notes.md) for creation, rescue and retirement.
- Keep build output inside its worktree, including Rust target directories, or one explicit temporary target, so builds never collide across lanes.
- Keep ISO/disc images and proprietary extracted game files outside Git trees, and never read values from them for project behaviour (`clean-room`), because they are not ours to publish or derive from.
- Stage named paths, finish commit hooks, publish separately through `safe-push --to main` and fast-forward clean main; chain a landing after its check with `&&`, never `;`, so a failed check never lands.
- Land each change once: `safe-push --to main` folds the lane's unpublished commits into one commit with their trailers (`--message FILE` sets its message; `--several-commits REASON`, logged, keeps them apart), so put regenerated records, re-recorded tests and goldens and doc lines into the lane before landing, never into a follow-up landing, and have a check that can only write its record after landing fold it into the next landing, because follow-up bookkeeping commits were 17% of makgora's main (north docs/rationale/one-landing-per-change.md).
- A repository with no `land.check` lines gets land checks inferred from its project kind; explicit `land.check` lines replace inference, and [notes](references/notes.md#inferred-land-checks) give the rules.
- Retire a worktree only as its owner/accountable parent after its work settles; preserve unknown ownership and live consumers, since a retired lane may hold someone else's unlanded work.
- Before starting an issue, check `north lane gc --unlanded` for an existing branch referencing it and continue that instead of starting over; an owner who abandons a lane deletes it in the same turn.
- The hourly `north lane gc` timer deletes landed, clean, idle lanes and archives long-idle unlanded ones; [notes](references/notes.md#lane-garbage-collection) give its rules.
- A worker removes its own finished lane with `north lane remove /absolute/path/to/worktrees/SLUG` rather than `git worktree remove`, which can leave an unregistered directory behind. The verb keeps a lane that is dirty, unlanded, locked, queued or in use. A reviewer that rejects a lane archives it with `north lane remove --rejected 'VERDICT' /absolute/path/to/worktrees/SLUG`.
- Signal owned processes by exact PID or unique scoped pattern, because a broad pattern kills peers' processes.
- Resolve guard denials through the sanctioned route and preserve secret/private-data/destructive/live-consumer boundaries, since a bypass removes the only check on them.

## Bounded search

The `resource-safe-search-guard` check in `north hook` refuses recursive searches of repository containers and `/proc`, `/sys`, `/dev`, `/run`; the `modern-search-guard` check refuses slow `grep -r`/`find` tree searches.

- Search one exact checkout, subtree, file or bounded data root, because a container holding main, worktrees and pins multiplies one search by every lane.
- Pass an absolute checkout or subtree operand, since launch context may differ from the tool's working directory and an implicit root may be the container.
- Name stdin explicitly when filtering a pipeline (`producer | rg PATTERN -`), so `rg` does not fall back to searching the working directory.
- Find a process through bounded native metadata (`pgrep`, `ps`, one `/proc/PID` field such as `readlink -e /proc/PID/cwd`), narrowed by PID, command, unit or user, and resolve its cwd before searching its files, because virtual trees expose device streams and links into arbitrary filesystems.
- Use `north convo` for transcript searches (see `todo`), because the transcript corpus is too large to scan.
- When a search shape proves unsafe, amend this rule and its guard together and test the offending shape plus the nearest legitimate bounded one, so the guard blocks only what it should.
