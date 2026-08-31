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

The optional `minimum-sufficient-v2` sidecar derives a minimum capability
floor and reasoning level from decision ownership, seam scope, error exposure, oracle
strength, foundational impact, dependency shape, and reasoning shape. It is
not a ninth routing field.

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
counts, an episode identity, a minimum reasoning floor, and allowed efforts.
Agent Machinery enforces the period share, model eligibility, live inventory,
capability floor, effort distance, and deterministic assignment. An explicit
model pin disables exploration; explicit-only models such as Terra are never
automatic treatments. The plan records baseline, selected treatment, reason,
and propensity while leaving the portable request unchanged.

`summarizeSelectionEvidence` groups observations into daily or ISO-week periods
and route strata. It retains quality and process confidence intervals, exact
price and token-category coverage, expected price and duration per quality
pass, rework, intervention, and missing-measurement counts. The consumer owns
the recurrence clock and durable raw observations; it must feed
the resulting evidence back into this same resolver instead of maintaining a
second recommendation policy.

Schema identities are stable and versioned independently of package paths:
`urn:agent-machinery:schema:routing-request:v3` and
`urn:agent-machinery:schema:selection-assessment:v2`. Resolve their packaged
files through the contract and asset paths in `catalog.json`.
