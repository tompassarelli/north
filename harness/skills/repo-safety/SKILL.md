---
name: repo-safety
description: >-
  Edit and publish ~/code repositories through owned worktrees, enumerated staging, and safe-push; preserve main checkouts, pins, and peer work; keep disc images and extracted game files out of Git trees. Also select bounded filesystem roots before searching repository containers or virtual filesystems, and use native process metadata for process discovery.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Repository safety

- Edit `~/code/<project>/worktrees/<slug>` and preserve main/pins, because main and pins carry Tom's and peers' live state; use [notes](references/notes.md) for creation, rescue and retirement.
- Keep build output inside its worktree, including Rust target directories, or one explicit temporary target, so builds never collide across lanes.
- Keep ISO/disc images and proprietary extracted game files outside Git trees, and never read values from them for project behaviour (`game-ip`), because they are not ours to publish or derive from.
- Stage named paths, finish commit hooks, publish separately through `safe-push --to main` and fast-forward clean main; chain a landing after its check with `&&`, never `;`, so a failed check never lands.
- Retire a worktree only as its owner/accountable parent after its work settles; preserve unknown ownership and live consumers, since a retired lane may hold someone else's unlanded work.
- Before starting an issue, check `lane-gc --unlanded` for an existing branch referencing it and continue that instead of starting over, so work is not duplicated; an owner who abandons a lane deletes it in the same turn. The hourly `lane-gc` timer retires landed, clean, idle lanes.
- lane-gc flags unlanded lanes and branches idle 24 h and, at 72 h, archives clean ones as archive/<container>/<lane>/<tip> tags and retires them; continue or land work before then. It retires landed-elsewhere, superseded (every named issue closed) and stale-dirty lanes the same way, archive tag first.
- Signal owned processes by exact PID or unique scoped pattern, because a broad pattern kills peers' processes.
- Resolve guard denials through the sanctioned route and preserve secret/private-data/destructive/live-consumer boundaries, since a bypass removes the only check on them.

## Bounded search

The `resource-safe-search-guard` hook (Codex only so far) refuses recursive searches of repository containers and `/proc`, `/sys`, `/dev`, `/run`; `modern-search-guard` refuses slow `grep -r`/`find` tree searches.

- Search one exact checkout, subtree, file or bounded data root, because a container holding main, worktrees and pins multiplies one search by every lane.
- Pass an absolute checkout or subtree operand, since launch context may differ from the tool's working directory and an implicit root may be the container.
- Name stdin explicitly when filtering a pipeline (`producer | rg PATTERN -`), so `rg` does not fall back to searching the working directory.
- Find a process through bounded native metadata (`pgrep`, `ps`, one `/proc/PID` field such as `readlink -e /proc/PID/cwd`), narrowed by PID, command, unit or user, and resolve its cwd before searching its files, because virtual trees expose device streams and links into arbitrary filesystems.
- Use `north convo` for transcript searches (see `todo`), because the transcript corpus is too large to scan.
- When a search shape proves unsafe, amend this rule and its guard together and test the offending shape plus the nearest legitimate bounded one, so the guard blocks only what it should.
