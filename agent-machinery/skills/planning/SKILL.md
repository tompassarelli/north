---
name: planning
description: >-
  Plan a change when a concrete interface, durable state, dependency, or external boundary makes the decision costly to reverse. Skip routine changes with a clear method and check.
---

# Engineering planning

Use the global engineering-context rules. Architecture or uncertainty alone
does not require a planning artifact.

Define the requested outcome, scope, behavior to preserve, decisions, and
completion evidence. Choose the shortest implementation path and expose the
central risk with an early falsifying check.

Include migration, recovery, compatibility, or rollout only for an actual
affected boundary. Resolve reversible implementation choices locally; ask when
the choice changes the product or exceeds authority.

Use `prior-art` for a consequential unresolved design choice and
`verification` for its evidence. Stop planning when another paragraph
would change no decision or action.

Bad: writing a multi-section design doc for a one-file, easily reversible
change because the area "feels architectural." Good: a short paragraph
covering outcome, risk, and check is enough when nothing durable or
externally depended-on is at stake — expand only when an interface, durable
state, or external boundary makes the choice costly to reverse.

For a plan outline, use `references/notes.md`.
