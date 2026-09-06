# Routing contract

Routing follows a resolved `project-exposure-v1` sidecar. That sidecar binds the
scoped engineering context and lifecycle budget without adding a routing field.
An omitted sidecar resolves to volatile owner-controlled research with exact
bounded-claim correctness and an empty lifecycle budget; omission never becomes
a form requirement or evidence of higher stakes. Consumers use
`resolveProjectExposureProfile` and
`validateRoutingAdmission` at the execution boundary; raw routing validation
alone does not admit work.

The portable routing request has exactly nine fields:

| Field | Meaning |
| --- | --- |
| `role` | Stable lowercase kebab-case responsibility ID, independent of template identity |
| `taskGrade` | Work scope and autonomy prior |
| `domainRequirements` | Context or expertise the brief must supply |
| `topology` | `worker` or `orchestrator` authority |
| `capabilityFloor` | Minimum semantic competence (`baseline` through `frontier`) |
| `serviceClass` | Price-quality-latency objective (`economy`, `fast`, `balanced`, or `premium`) |
| `reasoning` | Deliberation level |
| `posture` | Value-collision ordering |
| `composition` | Stock template or complete bespoke contract, including composition provenance |

Capability floor, service class, and reasoning are orthogonal.
`capabilityFloor` is a non-negotiable competence floor; `serviceClass` chooses
the selection objective after that floor; `reasoning` is the deliberation
budget. No value on one axis normalizes another. Consumers preserve the
explicit triple and execute the Agent Machinery plan rather than replacing it
with a local template or model default.

Stock templates have fixed topology and capabilities. `composition.id` names
the template and may differ from `role`; that identity is nested provenance
metadata, not ownership, authority, or a ninth field. Overrides may change task
grade, domains, capability floor, service class, reasoning, or posture and
must record the exact changed
fields plus one reason. A bespoke composition supplies responsibility,
deliverable, capabilities, decision and escalation bounds, done criteria, and
report shape.

Capability lists are transitively closed declarations of effective authority:

- `filesystem.search` implies `filesystem.read`;
- `shell.readonly` implies filesystem read and search authority, and is valid
  only when filesystem writes from shell execution are denied; and
- `shell` implies filesystem read, search, and write authority.

`shell` and `shell.readonly` are mutually exclusive. A consumer must enforce
the effective closure, not only the literal labels. If it cannot enforce the
declared filesystem boundary, it must fail closed and not run the agent.

The optional `minimum-sufficient-v3` sidecar derives a minimum capability
floor from decision ownership, seam scope, error exposure, oracle
strength, foundational impact, dependency shape, and reasoning shape. It is
not a tenth routing field. It validates the selected competence against that
floor, not against a universal effort ladder. Effort eligibility and named-decision
restrictions belong to the model-specific resolver, including when authoring with
`--assessment`. An advanced/low or baseline/max request alone is not admission;
the resolved model-effort pair must satisfy the catalog's competence floor.

Consumers map canonical capabilities to concrete tools and sandboxes. Missing
or unenforceable capability mappings fail closed. Agent Machinery resolves a
ranked provider/model/effort plan from this request, its selection catalog,
empirical observations, and consumer-supplied live inventory. The consumer
owns accounts, leases, connectivity, dispatch, raw telemetry, recurrence, live
coordination, and settlement. A changed inventory is resolved again through
the same function; it does not authorize a second fallback table.

The selection catalog defines a hard quality and success floor before any
price, latency, token, rework, or intervention objective is considered. Arms
remain on catalog priors until observations meet the configured confidence
threshold; a measured failure may exclude an automatic arm, but cost pressure
never lowers `capabilityFloor`. Economy and fast service classes compare
expected price or latency per quality-passing result only among eligible arms.

Bounded exploration is an optional sidecar to `resolveExecutionPlan`, never a
tenth routing field. The consumer supplies the period's eligible and treatment
counts, an episode identity, a same-model minimum reasoning floor, and allowed efforts.
Agent Machinery enforces the period share, model eligibility, live inventory,
capability floor, model-by-effort eligibility, and deterministic assignment.
Same-model comparisons retain the configured effort-distance limit; declared
cross-model arms use competence eligibility instead, allowing Astra low/medium
to compete with Sol high/xhigh without treating effort labels as intelligence.
Only catalog-declared experiment arms participate; Astra max is never an experiment,
while ordinary Luna xhigh/max may be compared on mechanical work.
An explicit model or effort pin disables exploration; explicit-only models such as Terra are never
automatic treatments. The plan records baseline, selected treatment, reason,
and propensity while leaving the portable request unchanged.

The selection catalog owns staffing priors as well as model availability rules.
Its model-specific effort policies restrict automatic and experimental arms and
competence at each effort. Substantive terminal workers default to Astra medium.
Low requires settled semantics and method plus a clear oracle, not merely a clear
desired outcome. High/xhigh follow actual unresolved reasoning demands.
The staffing catalog supplies authoring effort defaults; the selection catalog's
worker policy restricts ordinary unpinned selection to Astra, with Luna xhigh/max
as the justified mechanical exception at baseline competence. Sol and other models
remain available through explicit model selection or an explicit different provider;
catalog-declared experimental
arms remain eligible for bounded comparisons. Fast/economy objectives rank only
eligible models and never override this worker policy. Astra medium is frontier-eligible.
Astra high/xhigh is the upper tier. Astra low around Sol high–xhigh and Astra
medium above Sol max are explicitly unmeasured operator priors.
Confidence-qualified outcomes can change worker rankings inside these bounds.

`resolveExecutionPlan` accepts execution `context` separately from the nine-field
portable request. Consumers set `context.supervisory: true` for a primary or
overseer, regardless of its role or topology; orchestrators are always supervisory.
These runs default to the catalog's Astra high/xhigh policy and never explore;
the named consequential-decision exception may admit Astra max.
The consumer composes high by default and xhigh for higher complexity. A conflicting
pin or unavailable required model fails closed; no effort is silently rewritten.
For rare Astra max work, `context.loadBearingDecision` names the load-bearing
decision and significant costly-to-reverse consequence. Architecture alone is
insufficient. Named decisions are also excluded from exploration.

Luna max is ordinary mechanical effort, not an exceptional consequential decision.
After one substantive Luna failure the consumer hands that task's failing check,
partial artifact, and known context to Astra, using the existing `constraints.model`
pin for the resumed task. The outcome still enters calibration; one failure does
not establish a global Luna prohibition.

Normal selection preserves `request.reasoning` exactly. An explicitly pinned
effort additionally sets `constraints.effort` to that same value; a mismatch is
rejected. Only a consumer's opt-in bounded experiment may select another effort.
The consumer must persist the plan's policy revision, context, episode/period,
baseline, selected action and propensity beside the execution outcome, supply
serialized period counters, and feed existing route-stratified evidence back to
the resolver. This package returns assignments; it does not create a scheduler,
durable logger, or live experiment population by itself.

`summarizeSelectionEvidence` groups observations into daily or ISO-week periods
and route strata. It retains quality and process confidence intervals, exact
price and token-category coverage, expected price and duration per quality
pass, rework, intervention, and missing-measurement counts. The consumer owns
the recurrence clock and durable raw observations; it must feed
the resulting evidence back into this same resolver instead of maintaining a
second recommendation policy.

External charts and anecdotes remain priors, and seeded assignment simulations
prove routing behavior only. Compare accepted completed work, including context
loading, retries, repairs, human intervention, and full wall time. Decode time
excludes first-token delay and other overhead. API prices, token counts, and coarse
subscription percentages are distinct measurements, not exact account-quota cost.

Schema identities are stable and versioned independently of package paths:
`urn:agent-machinery:schema:routing-request:v3` and
`urn:agent-machinery:schema:selection-assessment:v3`. Resolve their packaged
files through the contract and asset paths in `catalog.json`.
