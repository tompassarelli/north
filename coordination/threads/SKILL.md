---
name: threads
description: Read or update North's shared thread graph when the user or an existing workflow requires durable intentions, facts, dependencies, queue position, or outcomes. Do not create bookkeeping threads for ordinary local work.
---

# Coordination threads

The graph is authoritative; files under `threads/` are projections. Use the
smallest operation that expresses the requested change:

- `north show <id|slug|substring>` reads one thread and its facts.
- `north threads [--all]`, `north ready [--all]`, and `north next` read work
  projections.
- `north capture "<title>" [owner]` creates a committed thread when durable
  work tracking is actually requested.
- `north tell <thread> <predicate> <value>` asserts one fact.
- `north retract <thread> <predicate> <value>` retracts that exact fact.

Do not edit projection files as the canonical write path. Do not create a
thread merely to mirror an ephemeral checklist or report routine progress.
When a required graph write is unavailable, retain the intended predicate and
value in the final report so it can be applied without reconstruction.
