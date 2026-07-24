# Extending Gaffer — stock templates and bespoke compositions

Gaffer is a stdlib, not a framework: the templates are precompiled convenience
for hot paths, the blocks are parts, and only the LAWS bind everything.
This is the specification for building on it.

## The contract — what makes an agent "gaffer-valid"

Four requirements, everything else is free:

1. **Routing pinned** — semantic tier and deliberation are always explicit.
   Provider/account selection belongs to the harness execution envelope; North
   may leave provider `auto` or pin a target/model. The selected provider adapter
   must resolve model AND effort/reasoning rather than inheriting them from a
   session. An exact model pin must pass that model's explicit per-tier route;
   raw provider support alone is insufficient.
2. **Comms norms respected** — compact reports, provenance marks
   (observed/inferred/assumed), not-done lists. Embed `docs/comms.md` or
   restate equivalent norms.
3. **Bounded authority** — the agent states what it may decide, what it
   must escalate, and what "done" is. Borrow a role block or write your
   own; an agent with unstated authority is unreviewable.
4. **Executable access** — every side effect required by the deliverable is
   licensed by the declared capabilities. External systems and operations are
   named as domain requirements and matched to an authenticated adapter, CLI,
   or context before a model turn is purchased. Metadata never grants access;
   an unavailable surface is a preflight result, not a task for the worker to
   improvise.

A bespoke domain-specific composition that satisfies these is a full citizen —
the template library has no privileged status beyond being pre-built.

## Building a bespoke/custom composition

Selection ladder: use a stock template unchanged when its deliverable and
authority fit; use a justified stock-template override when task grade,
domains, tier, reasoning, or posture change but its fixed topology/capability
boundary still fits. A topology change is never a preset override. Any change
to responsibility, deliverable, capability/authority boundary, done criteria,
or report shape requires a bespoke composition. The v2 machine schema retains
`presets`, `kind: "preset"`, and `nearestPreset` for compatibility.

1. Name the axes: function/role, `taskGrade`, domain requirements, topology and
   dependency shape, leverage, quality floor, semantic tier, deliberation,
   posture, and authority. Do not derive one by renaming another.
2. Audit required side effects and external access. Each side effect must fit
   the declared canonical capabilities; each external system and operation
   must be a named domain requirement with an execution surface the harness
   can prove before dispatch. Read-only authority plus a write deliverable is
   an invalid composition.
3. Borrow blocks that fit: comms (almost always), task grade, topology,
   role/posture, and only the exact concrete model's delta when its provider
   catalog supplies a calibrated path.
4. Write the domain-specific remainder freely — that's the point.
5. Record a bespoke contract: an optional `nearestPreset` (when a stock
   template genuinely helps explain or seed the composition), why a stock
   template was not used, responsibility,
   deliverable, canonical `capabilities[]`, `mayDecide[]`, `mustEscalate[]`,
   `doneWhen[]`, report, and a stable composition name. Capabilities are always
   explicit; a nearest stock template does not silently donate its authority.
   Stable role/composition IDs are lowercase kebab case, start with a letter,
   and contain only lowercase letters, digits, and single hyphens. Retired IDs
   remain reserved; use `scout`, `analyst`, or `research-scientist` instead of
   the ambiguous former `researcher` role.
6. `promotionCandidate` defaults false; nominate explicitly when useful.
   Recurrence adds evidence for a later human/orchestrator review; it never
   promotes itself. Comparable successful recurrence means the same
   responsibility, deliverable, capability/authority boundary, done criteria,
   and report shape has been used more than once, with evidence for each use's
   done criteria. Shared domain wording alone is not enough.

## Promoting a recurring pattern into the library

- **New stock template**: add a role block to `docs/roles.md` (authority +
  deliverable + REPORT + REDIRECT), add a machine `preset` entry to
  `staffing/catalog.json`, update the reviewed stock-template set in
  `scripts/validate.mjs`, rebuild (`node scripts/build-agents.mjs`), and
  describe it in README. Agent files are generated — never hand-write one.
- **New posture**: add a block to `docs/postures.md` (collision order +
  licensed + forbidden + done-bar).
- **New model delta**: run the elicit skill — self-report (contamination
  guarded) → subtract what the model already does → compile the residue in
   its own vocabulary. Save to `docs/deltas/<model>.md`, then record its repo
   path under that exact model in `providers/<provider>.json`. Runtime models
   without a calibration use an explicit `none`; they never inherit one.
- **New provider model route**: add the model's provider-supported
  effort/reasoning levels *within Gaffer's canonical vocabulary* and official
  per-model `effort-support` provenance, then calibrate explicit disjoint
  `routes.<tier>` shingles. Do not cross-product independent tier and effort
  lists. Raw support may remain unrouted. A model used by a canonical tier row
  must give that tier an exactly equal, equally ordered route; runtime-only
  alternate models may expose only their independently calibrated subset.
  Account entitlement and current target availability never belong here.
- Promotion rule of thumb: review after comparable successful recurrence as
  defined above. Repetition is evidence, not authorization: promotion is always
  an explicit change to the stock-template library.

## What NOT to do

- Don't fork the laws per agent — they encode cost/ceiling economics, not
  preference. If a law seems wrong, change the law (doctrine.md), which
  changes it everywhere.
- Don't hand-edit `agents/*.md` — compiled artifacts; `--check` will call
  it out.
- Don't add stock templates speculatively — the library grows from recurring
  bespoke need, not from taxonomy completionism.

## Anatomy of an agent — the axes and where each is encoded

| Axis | Sets | Enforcement | In gaffer | At runtime |
|---|---|---|---|---|
| Routing | semantic tier + deliberation | hard at dispatch | stock template + routing request | semantic spawn opts |
| Substrate | model + effort/reasoning | hard (API params) | provider raw-support intersection + exact per-tier routes / compiled adapter | resolved spawn opts |
| Capability surface | provider-neutral capability labels | hard, fail-closed adapter intersection | stock-template `capabilities` | adapter tool + sandbox mapping |
| Role | authority / deliverable / report / redirect | advisory | `docs/roles.md` | — |
| Task grade | work scope / autonomy / novelty prior | advisory | stock template or bespoke contract | spawn metadata |
| Domain requirements | expertise + context the brief/adapter must supply | recorded; hard only when a harness has a real capability gate | stock template or bespoke contract | prompt / required capabilities |
| Topology | worker / orchestrator coordination authority (reviewer/verifier/judge are worker roles) | host-enforced | doctrine + request | spawn opts / host orchestration |
| Dependency shape | when topology pays for itself | planner input (advisory) | doctrine — derives topology | orchestration plan |
| Leverage | value of improved judgment / cost of plausible error | planner input (advisory) | doctrine — derives tier | selection reason |
| Quality floor | minimum responsible semantic tier | planner input (advisory) | doctrine — bounds tier | tier-selection bound |
| Deliberation | reasoning budget independent of model capability | hard at dispatch | routing request | resolved spawn opts |
| Allocation | preferential / balanced / reserved subscription use | host-enforced | NOT gaffer's — North policy | North resource pools |
| Posture | value-collision ordering | advisory | `docs/postures.md` | — |
| Calibration | exact-concrete-model compensations | advisory | provider `modelDeltas` → `docs/deltas/<model>.md` or explicit none | prompt |
| Comms | universal output norms | advisory | `docs/comms.md` | host may add layers (register, wire formats) |
| Coordination membership | peer coexistence (presence, claims) | host-specific | NOT gaffer's | host harness |
| Laws | house constitution | advisory | `doctrine.md` (routing laws only) | host config |
| Hierarchy | escalation target | thin | REDIRECT lines in role blocks | host orchestration |
| Task + identity | the brief, agent id, state | — | never — arrives at spawn | prompt body / host state |

The compiler (`scripts/build-agents.mjs`) flattens the source-layer prompt and
adapter axes into agent files where the adapter format requires it. Host
coordination, hierarchy, and task identity remain runtime concerns: gaffer
stays portable precisely by not encoding them as provider-specific doctrine.
Capability labels describe enforceable authority, not decorative tool hints.
`shell.readonly` requires a hard working-tree write denial; an adapter that
cannot provide one withholds shell access. Claude plugin-agent frontmatter has
no hard sandbox control, so generated non-authoring plugin agents omit Bash;
North adapters can expose shell probes only under their provider's read-only
sandbox. A tools allowlist without Edit/Write is not itself a write boundary.
Topology and capabilities are validated together: orchestrators require
`coordination` and forbid `filesystem.write` plus unrestricted `shell`; workers
forbid `coordination`; `shell` and `shell.readonly` are mutually exclusive.
Changing a stock template's topology is rejected rather than projecting a new
capability set behind the scenes; a different pairing is bespoke.

Resource policy follows the same boundary. A bespoke composition selects one
semantic tier and one reasoning level; it does not carry an ordered fallback
ladder. It must not embed account names, entitlement balances, reset dates, or
concrete model names. North resolves that request through provider catalogs,
owns the changing runtime alternatives, and records which pool it selected.
Automatic alternatives remain same-capability substitutions before side
effects; any degradation is explicit and evidenced.
An explicit exact-model pin is an execution-envelope constraint, not a ninth
Gaffer request field. Its static catalog check and North's live authenticated
target check are both required and independent.
