# North — threads and concurrent writes

## Thread files (fact-native)

A thread file is `@<id>` + `predicate  object` triple lines + `---` + prose
body; refs are `@id`, literals EDN. Lifecycle is DERIVED from facts
(committed/outcome/abandoned/driver/depends_on) — no state enum; a fresh
capture is committed. Relatedness is `relates_to @<thread>` (no string tags —
former tags are `@topic-*` threads). ids: `2026-06-15-150040`. Time: `north clock`
(fact-native sessions; Clockify is an on-demand projection via `clock sync`).
For the product's file format and data model, see
`north:docs/fact-native-redesign.md` as non-authoritative reference material.
Agent conduct remains owned by the profile that linked this document.

## Writing safely under concurrent agents

North threads are backed by the North coordination graph — recursive Triples with
assertion history (engine `~/code/fram`). North's rows are performative, so
calling them facts is honest; the stored unit is the Triple plus its occurrence. Work
and telemetry have separate configured origins; their paths and writer
lifecycle belong to the deployment, not to an agent session. Do not infer a
log path or start, restart, import, or export a live corpus as routine setup.

- Create threads with `north capture "<title>"`.
- Change facts through `north tell <id> <pred> <value>` and `north retract <id>
  <pred> <value>` (`untell` is an alias). These writes are serialized and
  rule-checked by the coordinator.
- Do not use `north set` during concurrent work; it is an offline/single-writer
  primitive.
- Treat thread-file editing and `north import`/`north export` as explicit
  operator migration or recovery operations, never ordinary coordination.
- If the substrate is unavailable or slow, report the exact command and
  observed failure. Availability is not permission to mutate service state.

## Session state lives on threads — no markdown dumps (dogfood protocol)

The graph is the working memory, not your context window.

1. **Substantive work runs on a thread.** Find or capture the owning thread,
   then record meaningful state changes there.
2. **State = facts, not dumps.** Milestones/findings → `tell <id> progress
   "..."`; durable lessons → `tell <id> learning "..."`; finish → `outcome`.
   Writing a `SESSION-DUMP-*.md` is a protocol violation — the thread IS the
   handoff; the next session reads `north show <id>`, not a file.
3. **Agent briefs are thread refs.** When spawning an agent for thread work,
   the brief is "read `north show <id>`, write `progress` back" plus only the
   delta the thread doesn't hold — not a restatement of everything you know.
4. **Findings about the substrate go IN the substrate.** Found a bug mid-work?
   `capture` it, keep moving. Discovery-by-inhabiting is the point.
