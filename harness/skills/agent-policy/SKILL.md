---
name: agent-policy
description: >-
  Author, locate, register, or activate source-owned agent instructions, skills, hooks, and modules;
  turn explicit lasting instructions, repeated corrections or demonstrated recurring lessons into scoped policy;
  import third-party skills into the shared catalog; implement or change a command/write guard and its
  identity, activity gate, provider wiring, and deny/pass fixtures; audit or redesign the harness against the
  authoring guides.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Agent policy

Keep one source for each rule, because two sources drift and readers reconcile
them differently. Resolve the live owner with `agents status`,
`agents inspect <id>` and `agents path <id>`; never infer it from projection
paths or remembered layouts, because both move.

## Turn a correction into policy

An explicit lasting instruction or a demonstrated recurring lesson authorizes
a scoped policy update. Extract its trigger, action, scope and exceptions into
the existing owner rather than a new rule, because duplicates conflict.

Fix a repeated correction at the highest level that works: architecture, then
types, then lint, then test, then prose, because each lower level is enforced
less reliably. Prove the fix catches a real past mistake by replaying that
mistake against it, because an enforcer that never fired on a real case may
not fire at all. Once an enforcer holds the rule, delete the prose rule,
because the prose then only costs context. Check
[enforcers.md](references/enforcers.md) for the rules a hook or gate already
holds, and add a row when you add an enforcer.

Place universal boundaries in the nearest AGENTS.md, a triggered workflow in
one skill, a group of units in a module, and mechanically decidable rules in
hook code. Permission and resolved activity are distinct.

## SKILL.md and references

One skill per idea, with a plain name. Keep the description short and naming
every trigger, and `SKILL.md` complete for ordinary work, because agents load
only that. `references/` keep constraints, rationale, examples, alternatives
and labeled exploratory ideas; link each from `SKILL.md` with when to read it.
Update both layers when an adopted rule changes.

Keep a north skill's method public and put Tom-specific facts in a south
reference the skill points to with `south:<path>`: account letters and logins,
hosts, plans, prices and credit, his installs and devices, private repository
names, and session, environment or artifact IDs. North is a public repository,
and a fact pushed there stays readable in its history after it moves.

## Change and activate

1. Read owner instructions; edit an owned worktree, never a projection or pin,
   because projections are regenerated.
2. Follow `skill-creator` for frontmatter and resources. Register each identity
   once, preserve explicit module membership and hook events, and add no
   aliases.
3. Check a matching and a nonmatching case with the standard validator or an
   existing fixture, because shape checks cannot prove the rule selects the
   right action.
4. Land the owner and catalog commits, update clean main checkouts, then run
   `agents sync`. Change permission only when requested or needed. Local skill
   edits need no NixOS rebuild.
5. Verify the active ID, owner revision and provider projection resolve to that
   published source; unlanded or ambiguous authority blocks activation, so
   policy is not retained until it is active.

Size global policy, AGENTS.md files and skills by measurement, not line caps:
`agents sync` fails a change only when replay conformance drops or it adds
startup tokens with no measured gain against south:agents/operating-point.toml.
Put command detail in CLI help, `help TOPIC` or indexed docs.

## Read when

- [notes.md](references/notes.md): choosing a layer, validation, catalog,
  permission and activation detail.
- [guards.md](references/guards.md): implementing or changing a guard, its
  identity, activity gate, provider wiring or fixtures.
- [importing.md](references/importing.md): importing a third-party skill;
  licensing goes through `external-code` first.
- [guides.md](references/guides.md): harness redesign or audit. Run
  `bun run check:guides` in `agent-machinery/` first, because guides older than
  90 days are stale.
