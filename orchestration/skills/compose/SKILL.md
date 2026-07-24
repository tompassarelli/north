---
name: compose
description: Assemble a provider-neutral bespoke (custom) composition across independent role, task-grade, domain, topology, posture, semantic-tier, deliberation, comms, and model-delta axes when no template fits.
---

# Compose a bespoke (custom) composition

Templates cover the common cells. Use one unchanged when deliverable and
authority fit; use a justified template override when task grade,
domains, tier, reasoning, or posture change but its fixed topology/capability
boundary still fits. A topology change is never a template override. Any change
to responsibility, deliverable, capability/authority boundary, done criteria,
or report shape requires a bespoke composition. Machine payloads retain v2
`kind: "preset"` and `nearestPreset` keys.
For a bespoke composition, assemble the payload from this plugin's blocks and
pass it through the selected adapter's documented spawn surface.

## Procedure

1. **Classify the independent axes** before selecting blocks:
   - Role/function — responsibility and deliverable: executor, implementer,
     integrator, designer, director, scout, analyst, reviewer, verifier, judge,
     or research-scientist.
   - `taskGrade` — novice, junior, mid, senior, staff, principal, or
     research-grade.
   - Domain requirements — expertise, context, and external-access
     prerequisites the brief or runtime must actually provide. A requirement
     records a gate; it never grants access by itself.
   - Topology — worker or orchestrator (coordination authority); reviewer,
     verifier, and judge are worker-topology roles, never a topology; never
     inferred from grade. Stock templates carry fixed topology/capability
     pairings, so use a bespoke composition rather than attempting a topology
     override.
   - Semantic tier — economy, standard, senior, or frontier capability floor.
   - Deliberation — low, medium, high, xhigh, or max reasoning budget where
     supported by the selected provider tier.
   - Posture — explore, deliver, preserve, or evaluate collision ordering. It
     never expands role, topology, or capabilities.
   Done: each axis is stated explicitly; no role name doubles as a grade,
   model, or manager permission.
2. **Audit authority and access before buying a model turn.** List every
   side effect required by the deliverable. Each must fit the contract's
   canonical capabilities. Name every required external system and operation
   in `domainRequirements`, together with the authenticated adapter, CLI, or
   context that will satisfy it. If the harness cannot prove that access at
   admission, split to an appropriately authorized composition or stop
   preflight; never ask a read-only worker to improvise a write.
   Done: every required side effect has both authority and an available
   execution surface; unmet access is a preflight result, not a model task.
3. **Pick the prompt blocks** (all under this skill's plugin root):
   - Role — `docs/roles.md`: executor · implementer · integrator ·
     designer · director · scout · analyst · reviewer · verifier · judge ·
     research-scientist. Sets authority, deliverable, report format, redirects.
   - Task grade — `docs/task-grades.md`: novice through research-grade. Sets
     the work-contract prior without impersonating a model tier.
   - Topology — `docs/topologies.md`: worker · orchestrator. Sets spawn and
     reduction authority without contaminating posture.
   - Posture — `docs/postures.md`: explore · deliver · preserve · evaluate. Sets the
     collision priority order (what yields when values conflict).
   - Comms — `docs/comms.md` (universal block): output norms every report
     follows regardless of role.
   - Model delta — `docs/deltas/<model>.md`: include only when the selected
     provider adapter ships a calibrated delta for the concrete model. Absence
     is explicit, not permission to borrow a different provider's delta.
   Done: role, grade, topology, posture, and comms paths are named; the exact
   concrete model's delta path is named or explicitly `none` for this adapter.
4. **Derive and pin provider-neutral routing**: classify decision ownership,
   seam scope, error exposure, oracle strength, foundational impact, dependency
   shape, and reasoning shape in a `minimum-sufficient-v1` assessment. Recompute
   its minimum tier/reasoning and rule codes; never trust caller-supplied
   derived values. Foundational implementation-only work may remain
   economy/standard; invariant-decision ownership routes at least senior, and
   system synthesis routes frontier. A route below the derived minimum is
   invalid; one above it needs a coded detailed exception. `max` additionally
   requires exceptional reasoning shape and concrete `exceptionalDeliberation`.
   Semantic tier and deliberation are explicit. Provider/account selection is a separate harness-envelope concern;
   North defaults that envelope to `provider: auto`. The selected adapter
   resolves the concrete model and effort/reasoning. Pass the assessment to
   `scripts/compose-routing.mjs --assessment @<absolute-path>`; it remains a
   sidecar and the emitted request retains exactly eight fields.
   Done: tier + deliberation are literal Gaffer request values; provider is a
   literal North envelope value (normally `auto`), never a ninth Gaffer field.
5. **Paste** the blocks above the task text. Trim the delta before trimming
   role/posture.
   Done: `wc -l` of the assembled payload ≤ 60, every block above the task.
6. If no block fits, write a bespoke role contract and record: an optional
   `nearestPreset` stock-template reference when useful, why a stock template
   was not used,
   responsibility, deliverable, canonical `capabilities[]`, `mayDecide[]`, `mustEscalate[]`,
   `doneWhen[]`, report contract, and a stable composition name. Record either
   `promotionCandidate` status (false by default; `--promotion-candidate`
   nominates explicitly);
   recurrence is logged evidence and never auto-promotes the role.
   Without `nearestPreset`, explicitly set task grade, topology, tier,
   deliberation, and posture; domain requirements may be an explicit empty
   list. Done: the bespoke reason and contract are present, with promotion
   status and no implicit routing defaults.

## Adapter example (North execution envelope)

The following fenced syntax belongs to the North adapter; it is not part of
the portable Gaffer request contract.

```js
const prompt = roleBlock + gradeBlock + topologyBlock + postureBlock +
  commsBlock + (deltaBlock ?? '');
await spawn(`${prompt}\n\nTASK: ${task}`, {
  provider: 'auto', tier: 'standard', reasoning: 'medium',
  role: 'implementer', taskGrade: 'mid', topology: 'worker',
  posture: 'deliver',
  domainRequirements: ['repository conventions'],
  composition: {kind: 'preset', id: 'implementer', overrides: []}
})
```
