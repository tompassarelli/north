---
name: agent-policy
description: >-
  Author, locate, register, or activate source-owned agent instructions, skills, hooks, and modules;
  turn explicit lasting instructions, repeated corrections or demonstrated recurring lessons into scoped policy;
  import third-party skills into the shared catalog; implement or change a command/write guard and its
  identity, activity gate, provider wiring, and deny/pass fixtures; audit or redesign the harness against the
  authoring guides; review a followed outside source on its monthly cycle.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Agent policy

Keep one source for each rule, because two sources drift and readers reconcile
them differently. Resolve the live owner with `agents status`,
`agents inspect <id>` and `agents path <id>`, never from projection paths or
remembered layouts, because both move.

## Turn a correction into policy

An explicit lasting instruction or a demonstrated recurring lesson authorizes
a scoped policy update. Extract its trigger, action, scope and exceptions into
the existing owner rather than a new rule, because duplicates conflict.

Fix a repeated correction at the highest level that works: architecture, then
types, lint, test and prose last, because each lower level is enforced less
reliably. Prove the fix by replaying a real past mistake against it, because an
enforcer that never fired on a real case may not fire at all. Once an enforcer
holds the rule, delete the prose, because it then only costs context.
[enforcers.md](references/enforcers.md) lists the rules a hook or gate already
holds; add a row when you add an enforcer.

## SKILL.md and references

One skill per idea, with a plain name. Keep the description short and naming
every trigger, and `SKILL.md` complete for ordinary work, because agents load
only that. `references/` keep constraints, rationale, examples, alternatives
and labeled exploratory ideas; link each from `SKILL.md` with when to read it.
Update both layers when an adopted rule changes. Keep a north skill's method
public and put Tom-specific facts behind a `south:<path>` reference, because
north is public and a pushed fact stays in its history; the notes list which
facts count.

## Change and activate

1. Read owner instructions; edit an owned worktree, never a projection or pin,
   because projections are regenerated.
2. Follow `skill-creator` for frontmatter and resources, and register and
   validate as [notes.md](references/notes.md) describes.
3. Across repositories, land new source and confirm it runs live before landing
   the caller switch or registration that depends on it, and delete the old
   source last. Never point the south repository, a registration or a runtime
   selector at source that is not landed and live, because each reverse order
   broke landing or `agents sync` fleet-wide ([notes](references/notes.md#landing-order-across-repositories)).
4. Land the owner and catalog commits, update clean main checkouts, run
   `agents sync`, and verify the active ID, owner revision and provider
   projection resolve to that landed source, because policy that is not active
   is not retained; unlanded or ambiguous authority blocks activation.

## Read when

- [notes.md](references/notes.md): choosing a layer, validation, catalog,
  permission, activation and sizing detail.
- [guards.md](references/guards.md): implementing or changing a guard, its
  identity, activity gate, provider wiring or fixtures.
- [importing.md](references/importing.md): importing a third-party skill;
  licensing goes through `external-code` first.
- [watching.md](references/watching.md): a followed outside source is due
  for its monthly review, or a source joins the follow list.
- [guides.md](references/guides.md): harness redesign or audit. Run
  `bun run check:guides` in `agent-machinery/` first, because guides older than
  90 days are stale.
