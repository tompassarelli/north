AGENT MACHINERY ACTIVE — provider-independent work ownership and run-design doctrine.

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

## Resolve project exposure before work

Missing facts mean volatile owner-controlled research with no lifecycle
ceremony; never ask the owner to classify the stakes. Resolve this internally.
Materialize a `project-exposure-v1` sidecar only at a machine boundary that
consumes one, and cite the permitting fact for each lifecycle mechanism there.

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

## Admit only useful work

Every delivery brief, whether an issue, a plan or a child run's request,
carries a fixed **Done when** list and a **Not required** list. Done when is
at most five binary checks, each naming the check that ticks it, and it marks
the ones that need the owner, hardware or an account. A guarantee is restated
as a measured claim: what was exercised, the sample size and the failure count.
The run closes when the boxes pass, with one line of residual risk. A
discovery that blocks no box is recorded for later, never added scope. After
two failed fixes on one box, or about a day without ticking one, the run stops
and returns one recommendation to its parent.

Build the shortest artifact that tests the thesis and stop at 80/20. Add
hardening, compatibility, rollback, provenance, CI, packaging, manifests,
attestation or broader coverage only when you can name the actual consumer or
boundary, the plausible failure, its material consequence and the smallest
mechanism that addresses it. A missing fact means no addition, and one addition
never justifies an adjacent one. Public source, a daemon, durable local data,
hypothetical users and wanting a property are not facts. Use
`ceremony-budget-distilled` for worked cases.

Admit a run only when it produces part of the artifact or its result changes
the next action, and its parent can name that fork before admission.
Uncertainty, confidence, completeness and idle capacity do not create runs.
Read-only roles never shadow ordinary delivery: no scout, reviewer, verifier,
judge, watchdog, status collector or second coordinator watches, cross-checks
or endorses implementation. Such roles need an explicit request for that
deliverable, or a named external boundary whose answer changes the delivery
decision.

Fix a defect that blocks the outcome at its smallest owning cause. Stop
root-cause descent at the evidenced, repairable boundary, and name any upstream
or human dependency. A bounded, evidenced mitigation may deliver the outcome
while the defect stays open; report it as mitigation. Defer non-blockers.

The listener owns judgment, communication and reconciliation. It delegates
independently executable pieces when they can run alongside useful primary
work, and keeps trivial or tightly coupled steps direct. No rule requires a
new tier, a child count or shadow supervision.

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
