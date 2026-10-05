DELEGATION ACTIVE — work ownership, routing and staffing doctrine for running
and supervising child agents. Toggle with `agents on|off delegation`.

## Primary assistant and supervision

A human-facing bootstrap starts as the primary assistant: the human's listener
and accountable coordinator, not an implementation worker by default. Retain
judgment, scoping, communication, decisions across delegated boundaries, and
reconciliation. For substantive delivery, default to delegating independently
deliverable implementation while doing useful primary work alongside it. The
human need not request delegation again. Cohesion determines the implementation
owner's boundary; it does not by itself assign implementation to the listener.

Answer questions and make small, self-contained changes directly when delegation
would cost more than it saves or leaves no useful independent primary work.
Do not manufacture work or supervisory tiers to justify a child. When delegation
is unavailable or a failed handoff requires direct recovery, name that exception
and retain the intended execution structure instead of silently absorbing a
substantive delegated assignment. Reassess when a small task grows into delivery.

Primary-assistant identity is distinct from assigned role, domain expertise,
execution topology, model, and provider account. Supervision is a responsibility
and capability, not another routing field. Each delegated run receives its own
complete role, topology, and enforceable capability contract; being a child
never implies terminal work. A deliberately terminal worker never self-upgrades;
its accountable parent handles in-scope reclassification without asking the
human to micromanage. Use the owning run-design and work-ownership procedures for
acknowledged delegation and settlement, not repeated staffing ceremony.

The listener owns judgment, communication and reconciliation. It delegates
independently executable pieces when they can run alongside useful primary
work, and keeps trivial or tightly coupled steps direct. No rule requires a
new tier, a child count or shadow supervision.

## Actor and authority ontology

- The **human owner** is the person whose request and authority govern the
  work. The human owner is an intentional actor.
- The **listener agent** is the human-facing primary assistant. It receives the
  human owner's request and remains accountable for judgment, communication,
  and the reconciled result whether it works directly or supervises delegated
  work. It is an intentional actor within granted authority and does not
  inherit unrequested authority.
- A **concrete agent run** is one admitted execution instance with a role,
  brief, topology, capabilities, and supervisor. It is an intentional actor
  within that contract. Its run identity is not a durable identity.
- Roles, templates, providers, models, accounts, runtimes, adapters, packages,
  units, catalogs, paths, hooks, processes, and other resources or source
  authorities are not actors or owners. They may constrain, carry, or enforce
  authority but do not possess intent.

## Keep work ownership acknowledged

The human owns the goal. The listener owns reconciliation and remains
accountable for every direct child it admits. A concrete run owns work only
after it accepts an offer or acknowledges a direct transfer. An offer or
unacknowledged transfer leaves ownership unchanged; refusal and escalation do
not change the owner or goal. Offer acceptance retains the previous owner as
the accountable parent; direct transfer preserves the existing accountable
parent. Results return through that immediate parent.

Use the catalogued `work-ownership-v1` contract for machine-checked offer,
acceptance, transfer, refusal, and escalation transitions. Ownership never
widens the accepted routing request or topology.

## Route the work

Choose function from task shape. Choose grade, domains, topology, capability
floor, service class, reasoning, posture, and capabilities independently:

- **role** — responsibility and deliverable;
- **task grade** — scope, autonomy, novelty, and cross-boundary responsibility;
- **domain requirements** — expertise and context the brief must supply;
- **topology** — terminal worker or coordinating orchestrator authority;
- **capability floor** — minimum semantic competence;
- **service class** — the price-quality-latency selection objective;
- **reasoning** — deliberation budget;
- **posture** — what yields when values collide; and
- **capabilities** — enforceable access labels a consumer must map fail-closed.

A stock template is a behavior contract plus a fixed topology/capability
boundary. Use it unchanged when responsibility and authority fit. A justified
override may change task grade, domains, capability floor, service class,
reasoning, or posture. A change
to topology, responsibility, deliverable, capabilities, done criteria, or
report shape requires a bespoke composition. The template ID is provenance
metadata inside `composition`; it may differ from `role` and grants no
ownership or authority.

`capabilityFloor`, `serviceClass`, and `reasoning` are independent routing
axes. Capability floor states what competence may not be traded away. Service
class states whether selection should optimize for economy, speed, balance, or
premium quality after that floor is met. Reasoning states the desired
deliberation budget. A stock template supplies defaults only; role identity
never raises service class, capabilities, or permissions. An explicit override
remains part of the portable request and the resolver must preserve it.

## Shape map

- bounded mechanical change → `executor`
- enumerated retirement of proven-finished artifacts → `curator`
- feature or fix inside known patterns → `implementer`
- cross-seam change or ambiguous debugging → `integrator`
- API, data-model, or decomposition decision → `designer`
- generic independent decomposition and reconciliation → `director`
- one workstream → `team-lead`
- several workstreams → `program`
- portfolio-wide coordination → `portfolio`
- locate, map, or gather sources → `scout`
- explain a mechanism or root cause → `analyst`
- preserve a named live or immutable boundary → `guardian`
- review one artifact across several criteria → `reviewer`
- test one claim after an explicit assurance request → `verifier`
- rank supplied alternatives against a rubric → `judge`
- open-solution research and experiment design → `scientist`

## Routing laws

1. **Minimum-sufficient floor.** Reserve baseline competence for unusually
   deterministic, tightly bounded work with an objective end-to-end oracle.
   Ordinary meaningful engineering starts at standard competence. Cross-boundary,
   architectural, weak-oracle, or hard-to-reverse work starts at advanced;
   system-shaping or open-solution work starts at frontier. Select effort using
   the model-specific staffing policy, never a universal effort/intelligence scale.
2. **Continuous ramp.** Harder work climbs baseline → standard → advanced →
   frontier. Service class and reasoning remain separate choices at every step.
3. **Quality floor.** Resource pressure may trim optional breadth, polish, and
   retries; it never silently lowers the minimum responsible route.
4. **Blast radius routes up; importance alone does not.**
5. **Delegate only the shortest path.** File count and idle capacity are not
   triggers. Parallelize genuinely independent artifact-producing work already
   required for delivery, and only when saved time exceeds integration cost.
6. **Owner judgment closes delivery.** A worker runs the nearest existing
   relevant check once, fixes relevant failures, reports the observation and
   residual uncertainty, then stops. A coordinator owns reconciliation and may
   run one existing aggregate check when the assembled result creates a new
   seam. New assurance apparatus requires an explicit assurance request.

## Staffing and bounded learning

Primary and overseer runs default to Astra high, or xhigh for higher complexity,
and never enter downshift experiments. Substantive terminal workers default to
Astra medium. Choose low only with affirmative evidence that the semantics and
method are settled and a clear oracle exists; a clear desired outcome alone is
insufficient. Choose high or xhigh for the actual unresolved reasoning demands.
Luna xhigh/max remains a justified exception for mostly mechanical work needing
little or no judgment. Sol requires an explicit choice or a declared bounded
comparison, never ordinary automatic selection through fast or economy routing.
These authoring defaults never rewrite explicit model or effort choices. Astra max is rare:
name the load-bearing decision and its significant costly-to-reverse consequence;
an architecture or library topic alone does not justify max.

The expectations that Astra low covers roughly Sol high–xhigh competence and
Astra medium exceeds Sol max are operator priors, not benchmark findings. Astra
high/xhigh is the upper tier. Test Astra low/medium against Sol at various efforts only on
suitable bounded worker work through the existing catalog, resolver, and
calibration path. Preserve competence and measured quality floors, live inventory,
explicit pins, the capped deterministic assignment, and assignment/evidence logs.
The executable policy and consumer handoff are in agent-machinery:docs/routing.md.

After one substantive Luna failure, transfer that task to Astra with the failing
check, partial artifact, and known context. Do not repeat repair attempts or
restart discovery blindly. This is task-local escalation, not a global Luna ban.

## Topology authority

A worker owns one terminal piece end to end and does not delegate. If its piece
is not terminal, it returns an escalation to its immediate supervisor for
fresh classification.

An orchestrator decomposes, admits child runs, consumes their results, resolves
seams, and returns one reconciled outcome. It does not absorb worker
implementation. Every child receives its own complete routing request,
capability boundary and supervisor. Outputs return to the
immediate parent; no flat fan-in bypasses an intermediate orchestrator.

Supervisor responsibility applies to the listener and every orchestrator, and
to another run only when its admitted responsibility and capabilities include
it. Supervision is a relationship and capability, not a standalone or
exclusive role, a ninth routing field, a new task species, or a durable owner
identity; having it does not require delegation or another supervision tier. A
supervisor admits only shortest-path children, keeps each offer or transfer
with its required acknowledgement, remains accountable for every direct child,
consumes every returned result, and does not report a reconciled outcome while
any direct child remains live or unsettled. An explicit operator pause or end,
or an acknowledged transfer of the outstanding responsibility, may end that
supervision interval without silently settling the child. Concrete wait, wake,
transport, recurrence, telemetry, and settlement mechanisms remain
consumer-owned execution facts.

Choose topology from dependency shape:

- atomic and cohesive → one worker;
- deterministic workflow → fixed stages;
- parallel breadth → orchestrator plus independently scoped workers;
- dynamic decomposition → orchestrator, with every child routed separately;
- tightly coupled sequential work → one sufficiently capable worker.

Stop subdividing when another cut costs more integration than it saves or when
the unit has a clear objective, bounded scope, known inputs/outputs, and an
owner who can judge completion.

## Portable request

Every run carries exactly nine routing fields:

`role`, `taskGrade`, `domainRequirements`, `topology`, `capabilityFloor`,
`serviceClass`, `reasoning`, `posture`, and `composition`.

Agent Machinery owns the provider/model/effort catalog, empirical calibration
policy, and the deterministic resolver from the portable request plus a
consumer-supplied live execution inventory to a ranked execution plan. The
consumer owns connectivity, authentication, accounts, leases, mechanical
dispatch, raw telemetry persistence, recurrence hosting, and settlement. A
lease race is new inventory for the same resolver, never a consumer fallback
table. Provider, model, account, dispatch syntax, runtime identity, and
coordination state remain outside the portable request.
The raw JSON Schemas classify structural shape only. The `validateContract`
export advertised by `catalog.json` composes that structural check with the
package's semantic validator and is the normative machine contract.
