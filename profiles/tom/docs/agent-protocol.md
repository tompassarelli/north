# Agent protocol — driving agents via north

The live dispatch mode decides the spawn surface — read `north config dispatch`
at task intake. `native` pins the provider-native surface, `north` pins the
North-managed surface, and `auto` lets the system choose for each dispatch.
Under `auto`, the orthogonal `north config learning` axis selects deterministic
known-best assignment (`frozen`) or bounded experimental assignment
(`learning`); account allocation policy remains routing detail, not a dispatch
mode. Whatever the mode, North remains the coordination substrate: threads,
concerns, mail, and presence stay authoritative even when execution runs on the
provider-native surface. Enforcement is mechanical (`agent-spawn-guard.sh`
reads the same config); a denial is a routing instruction, never a wall.
Quick lookups → bash/grep/read inline. Real work → the protocol below.

## The stack

- **Work queue + coordination** = north threads + facts on `:7977`
  (`ready`/`next`/`leverage`; take work with `driver @agent`).
- **Managed spawn**: `mcp__north__dispatch` (thread-driven) / `mcp__north__spawn`
  (ad-hoc) — dormant until pinged.
- **Footprint**: declare before editing — `north:bin/concern declare|overlap|status`
  (`overlap <id>` marks likely-to-land work per line; alias: `shape`).
- **Reach a live agent**: it arms `north:bin/north listen <id>` (alias: `north-arm`); ping with
  `north-comms send <from> <role-alias> "<subject>" "<msg>"` — a message is
  the steer. Alias-first delivery resolves the current live recipient and fails
  loudly when none is reachable. Observe via `north watch`, `north agents`, or
  `north board`; the CLI/MCP surface is authoritative.
- **Concurrency is the engine's job** — fram owns write-serialization + OCC + the `lease`
  primitive (`acquire`/`release`/`fence`); apps express coordination as facts, never
  self-rolled locks. (`driver` = app intent; `lease` = DB mutual-exclusion — never conflate.)
- **Managed recursion** (when the mode selects North dispatch): children are
  created only through North admission — a fresh `part_of` thread, run,
  reservation, complete Orchestration route, resource envelope, and telemetry —
  and settle to their immediate parent. Jurisdiction law (worker vs
  orchestrator and reduction) is doctrine, not restated here.
  Scope overrun, new seams, budget pressure, or repeated no-progress →
  `north escalate needs-replan`; with no live parent, checkpoint and stop
  rather than silently broadening.

Org brain: PLAYBOOK = north thread `2026-06-22-232740` (consult first; append
learnings via `north tell 2026-06-22-232740 learning "…"`). How-to:
`north:AGENTS.md`.

Optional product references: `north:docs/operating-manual.md` explains the
thread and operator surfaces, and `north:docs/workflow-map.md` records pipeline
anatomy and historical failure modes. Neither document defines agent conduct
or overrides this profile.
