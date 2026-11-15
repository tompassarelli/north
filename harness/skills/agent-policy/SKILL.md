---
name: agent-policy
description: >-
  Author or change agent instructions, skills, hooks, guards or modules; turn lasting corrections into policy; import skills; audit the harness; review a followed source.
grounded: 2026-11-15
written: 2026-11-15
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
reliably. A lane that changes harness text (a skill, `CLAUDE.md` or an agent
definition) to fix an agent behavior adds a case under `harness/cases/` and
shows `north policy-replay case` failing on the base text and passing on the
lane's, because text a replay never saw fail may change nothing (the
[notes](references/notes.md#replay-proof) give the command, proof file and
judged-case bar). Once an enforcer holds the rule, delete the prose, because it
then only costs context; [enforcers.md](references/enforcers.md) lists the
rules a hook or gate already holds, one row per enforcer.

A policy or routing change, including every lesson turned into policy, lands
with its entry in `docs/rationale/experiments.toml`, because a rule nobody
reviews stays forever whether or not it paid off. On its review date,
`north check experiments judge` keeps it, reverts it or brings it to Tom with
the numbers.

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

1. Read the owner instructions and edit an owned worktree, never a projection
   or pin, because projections are regenerated. Follow
   [skill-creator](references/guides/anthropic-skill-creator.md) for
   frontmatter and resources.
2. Across repositories, land new source and confirm it runs live before the
   caller switch, registration or runtime selector that depends on it, and
   delete the old source last, because each reverse order broke landing or
   `agents sync` fleet-wide
   ([notes](references/notes.md#landing-order-across-repositories)).
3. Land, update clean main checkouts, run `agents sync`, and confirm that the
   active ID, owner revision and provider projection resolve to the landed
   source, because policy that is not active is not retained.

## Read when

- [notes.md](references/notes.md): choosing a layer, registration,
  validation, permission and activation detail.
- [guards.md](references/guards.md): implementing or changing a guard, its
  identity, activity gate, provider wiring or fixtures.
- [importing.md](references/importing.md): importing a third-party skill;
  licensing goes through `external-code` first.
- [watching.md](references/watching.md): a followed outside source is due
  for its monthly review, or a source joins the follow list.
- [guides.md](references/guides.md): harness redesign or audit. Run
  `north config check guides` first, because a guide over 90 days old is stale.
