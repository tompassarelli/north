# Extending the template library

An agent-run-valid composition has an explicit semantic route, bounded
authority, executable canonical capabilities, done criteria, and compact
communication norms.

To add a stock template:

1. Add its behavior contract block to `agent-machinery:docs/roles.md`.
2. Add one entry to `agent-machinery:staffing/catalog.json`.
3. Declare every new generator input or documentation asset in
   `agent-machinery:catalog.json`.
4. Keep capability declarations transitively closed; shell authority includes
   its effective filesystem authority.
5. Rebuild with `bun run build`.
6. Validate with `bun run check`.

Role IDs use lowercase kebab case. Add templates only after a recurring
composition demonstrates the same responsibility, deliverable, topology,
capabilities, done criteria, and report shape more than once.

The package never adds concrete model routes or runtime mappings. A consumer
may map canonical capabilities to its execution substrate, but it must preserve the requested floor and fail closed when authority cannot be
enforced.

## Adding or maintaining a skill

One idea is one skill with a plain name, `skills/<name>/`, declared once in
`agent-machinery:catalog.json`. Its `SKILL.md` is the complete operating
surface: an agent that loads it needs nothing else to act. Keep it short —
context spent here competes with the task it is meant to serve.

Longer detail lives in the same skill's `references/` folder: `notes.md` as
the entry point, split into topic files once it outgrows one page. It
preserves constraints, rationale, examples, alternatives, and rejected
options for the next revision of `SKILL.md`, and is not a second set of
routinely loaded instructions. It is never a separate catalog unit; agents
read it only for a named unresolved question.

Record the evidence a policy rests on in `references/`, not in a commit
message. A rule whose observed incidents, superseded mechanism, and
deliberate omissions survive only in history gets deleted whole at the next
consolidation and re-derived from scratch — which has already happened once
in this corpus.

Add worked contrasts where the rule is a judgment made under ambiguity and
either direction is a plausible mistake; procedural or checkable rules do not
need them. Place them before the closing `references/` pointer, which is the
file's last line.

Validate with `bun run check`.
