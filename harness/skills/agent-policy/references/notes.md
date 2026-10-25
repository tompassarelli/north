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

Tom-specific facts that belong in a south reference rather than a north skill:
account letters and logins, hosts, plans, prices and credit, his installs and
devices, private repository names, and session, environment or artifact IDs.
A fact pushed to north stays readable in its history after it moves.

Size global policy, AGENTS.md files and skills by measurement, not line caps:
`agents sync` fails a change only when replay conformance drops or it adds
startup tokens with no measured gain against south:agents/operating-point.toml.
Put command detail in CLI help, `help TOPIC` or indexed docs.

## Validation

Check a matching case and a nearby nonmatching case with the standard
validator or an existing fixture. Does the rule activate on the matching case
only, and would the instruction select the right source, action and stopping condition
without widening scope? Metadata and link checks prove structure; an existing
behavior fixture proves enforcement; text matching cannot prove judgment.

If a behavioral evaluation is justified and authorized, give the evaluator the
realistic request and raw inputs without leaking the desired answer. A useful
record names the trigger, scoped rule, compliant move, exceptions, observed
result and unresolved evidence; it need not become a new artifact.

## Registration and enforcement

Register the exact owner once, preserve explicit module membership and hook
events, and add no aliases; shape validation does not prove the rule selects
the right action. A hook's catalog support relations and provider
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

## Landing order across repositories

Three separate rust-port incidents on 2026-10-15 set the order in `SKILL.md`
step 3. A safe-push caller switch to `north dep-gate` landed (north 9363e8a9)
while south pointed Firn system policy at unlanded Rust source (south 87f9f5e,
reverted in 05112fb), and every landing was refused for 13 minutes. A worker
hand-pointed a live runtime selector at an unlanded build, and a second worker
then backed out a good landing it took for a rollback. A guard port deleted
sources in north (de27467a) that south still registered, so `agents sync`
failed for every session until a 4-step order restored it: the new guard
beside the old files, promotion, removal of the registration, then deletion.
