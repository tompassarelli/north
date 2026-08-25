---
name: assignments
description: Bind concrete North threads to agents or people, inspect assignment state, and release ownership. Use after staffing resolves the role; assignment records who owns this particular work.
---

# Coordination assignments

Staffing resolves the requested behavior profile. Assignment records who owns
this thread now. Keep those decisions separate.

Prefer `north delegate "<task>" --thread <thread>` for managed work because the
dispatch lifecycle records and releases its driver claim. For an explicit
manual pickup, assert the exact driver fact with
`north tell <thread> driver <actor>` and release it with
`north retract <thread> driver <actor>`.

A `driver` fact is an assignment, not proof that its holder is currently live.
Inspect `north show <thread>`, `north threads`, or `north agents` before drawing
a liveness conclusion. Never overwrite another live owner; surface the
conflict. On an availability failure, do not guess that the assignment landed
and do not retry blindly—report the intended thread and actor exactly.
