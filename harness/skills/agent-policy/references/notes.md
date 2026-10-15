# Agent policy: full notes

Read this when you revise `SKILL.md`, choose a layer for a new rule, or need
catalog and activation detail.

## One owner, several surfaces

A source declaration owns a rule; a catalog identifies it; a generation
resolves its activity; provider surfaces project it. Copies and symlinks are
delivery mechanisms, not new authorities. Start with live evidence:

```text
agents status
agents inspect <id>
agents path <id>
```

Correlate the reported owner with its source catalog and operator overlay. Do
not hardcode a catalog path or repository topology from an older North
version, because the layout moves. The Firn client delegates resolution and
projection to North; it is not a second catalog or resolver.

## Extract the stable decision

Strong or repeated feedback shows a decision needs durable guidance, not that
every detail of the incident should become policy. Identify the trigger,
desired move, real scope, exceptions and the previous failure. Preserve
explicit user intent; do not generalize an example into unrelated work.

Prefer correcting an existing owner to creating another rule. If two sources
disagree, fix the authority split instead of telling models to reconcile it
forever, because every reader will reconcile it differently.

## Choose the loading layer

| Need | Owner |
| --- | --- |
| Boundary governing every matching action | nearest applicable AGENTS.md |
| Coherent reliably triggered workflow | one skill |
| Several registered units enabled together | module |
| Deterministic provider-event enforcement | hook |

Emotional force alone does not make a judgment mechanically enforceable; a
hook fits only a rule that a decoded payload can decide. Instruction and
template distributions attach to catalogued units; they are not extra unit
kinds. Modules compose existing identities, never hidden members.

## SKILL.md and references

Discovery descriptions state the trigger cheaply. `SKILL.md` contains the
complete normal decision path and essential boundaries. `references/` keep
reasoning, exact protocols, examples, counterexamples, rejected alternatives
and explicitly labeled open questions.

An adopted rule and an exploratory idea have different status; keep them
apart, and never hide a binding safety condition in optional notes or promote a
brainstorm into a mandatory checklist. Split a reference only where different
questions select different topics, with a short index of read conditions.

## Validation

Check a positive case and a nearby case that should not activate the rule:
would the instruction select the right source, action and stopping condition
without widening scope? Metadata and link checks prove structure; an existing
behavior fixture proves enforcement; text matching cannot prove judgment.

If a behavioral evaluation is justified and authorized, give the evaluator the
realistic request and raw inputs without leaking the desired answer. A useful
record names the trigger, scoped rule, compliant move, exceptions, observed
result and unresolved evidence; it need not become a new artifact.

## Registration and enforcement

Register the exact owner once; shape validation does not prove the rule
selects the right action. A hook's catalog support relations and provider
event wiring belong outside skill prose. A lifecycle wrapper keeps its actual
event and consumes resolved activity; it does not recompute module closure or
pretend to be PreToolUse. For executable enforcement changes read
[guards.md](guards.md).

## Permission, activity and activation

Stored permission says a unit may run; resolved activity also includes active
parents or supported claimants. North resolves provenance once, writes a
private generation and atomically advances the current selector.

After required owner and catalog commits land and clean source checkouts are
current, run `agents sync`. Change permission with `agents on <id>` only when
enablement is requested or required by the authorized workflow; editing a
reference does not authorize enabling it.

Inspect status, owner, activity, generation digest and provider activation
paths from the live client, and resolve them to landed authority, not a lane or
hand-edited projection. Stop on ambiguous identity or unlanded source.
Skill-only out-of-store activation needs no NixOS rebuild.
