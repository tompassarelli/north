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
may map canonical capabilities to its execution substrate and supply live
provider inventory to the model-selection resolver, but it
must preserve the requested floor and fail closed when authority cannot be
enforced.

## Adding or maintaining a skill

A skill ships in two halves. The distilled `SKILL.md` is the complete
operating surface: an agent that loads it needs nothing else to act. Keep it
short — context spent here competes with the task it is meant to serve.

The long-form half is maintained separately and redistilled into the short
one; it is never merged back in. It preserves constraints, rationale,
examples, alternatives, and rejected options for the next re-distillation, and
is not a second set of routinely loaded instructions.

Give it its own `<name>-reference` unit, declared in
`agent-machinery:catalog.json` and resolved on demand through `agents path`,
when an agent may need to enter it mid-task; split that unit into
`references/*.md` topic files once it outgrows one page. A distilled-only
skill may instead link `references/*.md` directly from `SKILL.md`, with no
catalog entry. Never manufacture a second entrypoint for a skill that does not
need one.

Record the evidence a policy rests on in the long-form half, not in a commit
message. A rule whose observed incidents, superseded mechanism, and
deliberate omissions survive only in history gets deleted whole at the next
consolidation and re-derived from scratch — which has already happened once
in this corpus.

Add worked contrasts where the rule is a judgment made under ambiguity and
either direction is a plausible mistake; procedural or checkable rules do not
need them. Place them before the closing `agents path` pointer, which is the
file's last line.

Validate with `bun run check`.
