---
name: todo
description: >-
  Keep restart-grade records for cross-turn work, external waits, parked tasks, and handoffs, and recall past agent work, decisions, commands or session IDs across Claude Code and Codex transcripts with the indexed convo CLI instead of searching transcript storage.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Todo continuity

Use the flat `~/code/todo/` inventory only when work must survive the current
turn/process and Git plus conversation cannot safely recover its live state.
Ordinary plans, in-turn delegation, and recoverable worktrees need no record.

## Keep a resumable record

- One Markdown file with TOML front matter per independently resumable unit;
  no nested records or `main/`. `AGENTS.md` and `agent-coord.md` are singletons.
- Record the outcome, exact owner, current checkpoint, executable next actions,
  observed verification, and recovery/cleanup conditions. Include only needed
  dependencies, conversation IDs, coordination facts, and every live lane.
- Give independently required resumable execution one `[[attempt]]`.
  Forecast/staffing facts enter only when needed.
- Record a model only from concrete run/dispatch evidence. Unknown identity
  stays an evidence gap, never `self`, `parent`, `default`, or `auto`.
- Replace stale state at meaningful boundaries, including superseded holds.
  Distinguish operator restrictions from agent recommendations; notes do not
  grant or revoke authorization. Do not append a transcript.
- The product owner closes from terminal evidence; the run's host separately
  settles its process, delivery, driver, and child state.
- Delete only after owned changes, dependents, cleanup, continuity, and awaited
  acknowledgements are settled.

For exact fields, read `references/notes.md` and read its record
schema. Keep repository paths `repo:path` and runtime/worktree paths
`~`-anchored.

## Recall past work with convo

Find past agent work, decisions, commands or session IDs across Claude Code and
Codex transcripts with the indexed `north convo` CLI; the `corpus-scan-guard` hook
refuses recursive raw scans of transcript storage, which is too large to sweep.

- Search with `north convo <words>` (all words in one message; `-x` for exact strings,
  ids and error text) and narrow by role, tool, project, session, provider or
  time; flags are in `north convo --help`.
- Open a hit with `north convo show <id> -C 3` or `north convo session <id prefix>`, and read
  a raw transcript only at the path and line `show` printed, so no read sweeps
  the corpus.
- Treat exit 2 as inconclusive, never as absence, because the index may be busy
  or stale.
- Treat a hit as a recorded claim: check the speaker, the date and any later
  reversal, and verify consequential facts against current source, because
  transcripts record what was said, not what is true now.

Read [convo judging and internals](references/convo.md) for exit codes, what is
indexed, the corpus layout, maintenance (`index --full`, `compress`, `restore`)
and skill-usage counts.
