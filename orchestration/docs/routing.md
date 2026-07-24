# Provider-neutral routing

Gaffer chooses semantics; a provider adapter chooses a concrete runtime. Three
layers sit above that choice, and keeping them apart is the whole contract:

- **Planner inputs** are how a caller REASONS ITS WAY to a request. Task shape
  proposes a role; seven versioned minimum-sufficient signals derive the
  capability/deliberation floor. They live in a validated sidecar, not on the
  wire, and DERIVE the routing fields below.
- **The routing request** is the provider-neutral payload Gaffer emits and a
  harness consumes. Hard controls such as tier/reasoning/topology affect
  dispatch; descriptive fields such as grade/domains/contracts are validated
  and recorded, then matter only when a prompt or adapter actually consumes
  them. Metadata is never described as magic execution.
- **Runtime allocation** — which account, which concrete model, resource
  pressure, fallback — is owned entirely by the harness. Gaffer never names an
  account or a model and does not define allocation's schema.

## Planner inputs (upstream — they derive the request, they are not it)

These inform how the routing axes are chosen. Feeding one as a request field
is an error, not a silently-ignored no-op — the composer rejects unknown options.

| Planner input | What it captures | Derives |
|---|---|---|
| task shape | execute / implement / integrate / design / direct / scout / analyze / review / verify / judge / research-science | role + stock-template defaults |
| decision ownership | none, bounded, cross-boundary, system-shaping, open-solution-class | tier floor |
| seam scope | none, established, consequential, system-wide | tier floor |
| error exposure | contained-reversible, material-recoverable, high-or-hard-to-reverse | tier floor |
| oracle strength | not-applicable, objective-local, objective-end-to-end, partial, judgment-only | tier + deliberation floor |
| foundational impact | none, implementation-only, invariant-decision-owned | no automatic layer promotion; invariant ownership raises the floor |
| dependency shape | atomic-cohesive, deterministic-workflow, parallel-breadth, dynamic-decomposition, tightly-coupled-sequential | topology + route floor |
| reasoning shape | deterministic, bounded-branching, multi-hypothesis, system-synthesis, exceptional | tier + deliberation floor |

Topology follows LOCAL dependency shape at every admitted node; a frontier
director is warranted by dynamic decomposition, not by importance or by its
distance from the root. Worker jurisdiction is terminal. Orchestrator
jurisdiction may recursively staff workers or child orchestrators through
North, with each child freshly classified, admitted, routed, resource-bounded,
metered, and settled. The immediate parent owns reduction. Budgets, cycle and
no-progress controls, and settlement gates stop recursion; there is no global
hard depth cap. North records `topology` but does not synthesize a director
graph from this metadata alone.

The canonical sidecar schema and examples are
[`contracts/selection-assessment.schema.json`](../contracts/selection-assessment.schema.json)
and [`contracts/selection-assessment.fixtures.json`](../contracts/selection-assessment.fixtures.json).
`scripts/selection-assessment.mjs` recomputes the derived minimum and rule codes,
rejects a selected tier or reasoning level below either minimum, and requires a
coded detailed exception above either minimum. `max` additionally requires
`reasoningShape=exceptional` and a concrete `exceptionalDeliberation`. That
field is rejected below max. The assessment is not part of `RoutingRequest`.

## The routing request (Gaffer's contract)

```json
{
  "role": "integrator",
  "taskGrade": "senior",
  "domainRequirements": [],
  "topology": "worker",
  "tier": "senior",
  "reasoning": "high",
  "posture": "deliver",
  "composition": { "kind": "preset", "id": "integrator", "overrides": [] }
}
```

```ts
type OverrideField =
  | "taskGrade" | "domainRequirements" | "tier" | "reasoning" | "posture";

type RoutingRequest = {
  role: string;                 // function / deliverable; stock-template or bespoke name
  taskGrade: "novice" | "junior" | "mid" | "senior" | "staff" | "principal" | "research-grade";
  domainRequirements: string[];
  topology: "worker" | "orchestrator";                       // coordination authority; reviewer/verifier/judge are worker ROLES
  tier: "economy" | "standard" | "senior" | "frontier";      // model capability floor
  reasoning: "low" | "medium" | "high" | "xhigh" | "max";    // deliberation
  posture: "explore" | "deliver" | "preserve" | "evaluate";
  composition:
    | { kind: "preset"; id: string; overrides: OverrideField[]; overrideReason?: string }
    | {
        kind: "bespoke"; id: string; nearestPreset?: string;
        bespokeReason: string; promotionCandidate: boolean;
        contract: {
          responsibility: string; deliverable: string;
          capabilities: string[];
          mayDecide: string[]; mustEscalate: string[]; doneWhen: string[];
          report: string;
        };
      };
};
```

`role`/function describes the deliverable; `taskGrade` describes the scope and
judgment expected of the work; `tier` is the model capability floor; `reasoning`
is deliberation; `domainRequirements` names context/expertise the brief or
adapter must supply, including named external-access prerequisites when the
deliverable depends on another system (recording a requirement alone grants
nothing); `topology` describes coordination authority; and `posture` names the
priority order when values collide. An adapter must not infer one
solely from another. A stock template may propose all of them, but the recorded
request keeps them distinct. A changed overrideable stock-template axis is
listed in `overrides[]` and requires `overrideReason`; an unchanged stock
template uses `overrides: []` and must not carry a reason. Stock topology is
fixed and never appears in `overrides[]`; changing it requires a bespoke
composition with explicit capabilities. The canonical JSON Schema and
cross-harness examples are
[`contracts/routing-request.schema.json`](../contracts/routing-request.schema.json)
and [`contracts/routing-request.fixtures.json`](../contracts/routing-request.fixtures.json).
The request therefore remains exactly eight fields even when its selection was
grounded by a sidecar. Free-text `composition.overrideReason` remains human
provenance for a changed stock template; mechanically derived `ruleCodes` do
not replace or populate it.

Role and composition IDs use one lowercase kebab-case namespace
(`^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$`). Composer-only compatibility aliases are
normalized to their canonical stock template before emission; an alias is not a valid
wire-level `role`. Retired IDs such as `researcher` remain invalid rather than
silently returning as bespoke roles.

`tier` and `reasoning` are independent axes, but their COMBINATION must resolve
through a provider catalog (see below). `reviewer`, `verifier`, and `judge` are
functions on worker topology, not topologies: `topology` is only `worker` or
`orchestrator`.

## Shape routing

| Shape | Default role | Tier | Topology | Posture |
|---|---|---|---|---|
| execute | executor | economy | worker | deliver |
| implement | implementer | standard | worker | deliver |
| integrate | integrator | senior | worker | deliver |
| design | designer | frontier | worker | explore |
| direct | director | frontier | orchestrator | deliver |
| scout | scout | economy | worker | explore |
| analyze | analyst | senior | worker | explore |
| review | reviewer | senior | worker | evaluate |
| verify | verifier | senior | worker | evaluate |
| judge | judge | frontier | worker | evaluate |
| research-science | research-scientist | frontier | worker | explore |

These are stock templates, not coupled identities. `taskGrade`, domain
requirements, topology, semantic tier, deliberation, and posture are
conceptually independent. Current stock templates nevertheless ship fixed,
enforceable topology/capability pairings. A stock template's topology cannot
be overridden. Choose a compatible stock template or a bespoke composition
with explicit capabilities for a different topology. Foundational
implementation-only work may remain `economy` or `standard`; owning a
foundational invariant decision raises the minimum to `senior`. System-shaping
ownership or system synthesis raises it to `frontier`. A repository or path
never raises the route by itself, and importance alone does not.

Reviewer is the multi-criterion evaluation of one supplied artifact or change:
prioritized evidence-backed findings plus `accept`, `changes-required`, or
`cannot-assess`. Analyst explains a mechanism, verifier decides one claim,
judge ranks multiple supplied alternatives, designer chooses or redesigns a
shape, and integrator applies a change. Verifier's senior/high default may be
justifiably overridden up or down, but the task's quality floor remains
binding.

Orchestrator topology grants coordination and reconciliation authority, not
worker implementation authority. A coordinator consumes worker evidence,
drives the assembled result end-to-end, and performs bounded independent
non-authoring verification probes at the materially load-bearing child seams.
A probe may execute a narrow test/build and create disposable cache or
temporary state needed to observe the assertion; it may not edit, implement, or
repair the deliverable or inherit a worker's full local-probe burden.
Self-contained units add a verifier sibling when verdict leverage warrants one.
An emergent aggregate always receives an independently staffed,
context-carrying whole-outcome verifier report. These obligations affect how
the topology is staffed and prompted; they do not add a ninth routing field.
An orchestrator may create a child orchestrator only through the same North
execution boundary used for any child: a fresh eight-field Gaffer request plus
independent admission, provider/account resolution, resource envelope,
telemetry, and settlement. No route or budget is inherited by tree position,
and each immediate parent reduces its direct children. Provider-native opaque
fanout is distinct from this contract and remains disallowed under North until
the harness can enforce equivalent per-child boundaries.

The canonical machine-readable stock-template definitions live in
`staffing/catalog.json` (`staffing/catalog.schema.json` documents the format).
Catalog v2 has exactly `$schema`, `version`, `vocabulary`, `defaults`,
`presets`, and `aliases` at its top level; `version` is `2` and `presets` is
the compatibility key for the named stock-template library. Compose a portable
payload without knowing a
provider model name:

```sh
node scripts/compose-routing.mjs implementer --domain Nix \
  --assessment @/absolute/path/to/selection-assessment.json
node scripts/compose-routing.mjs migration-forensics --nearest analyst \
  --rationale "needs provenance tracing plus schema recovery" \
  --contract @/absolute/path/to/migration-contract.json \
  --assessment @/absolute/path/to/selection-assessment.json \
  --no-promotion-candidate
```

The command prints the JSON that follows a `GAFFER_ROUTING` marker.
Stock-template values are defaults only: every changed overrideable axis
replaces only itself and is auditable. Their topology is fixed. Unknown roles
are valid bespoke compositions only with a reason, promotion status,
structured authority / deliverable / done contract, and an optional
`nearestPreset` reference when a stock template genuinely helps explain or
seed the composition. Without `--nearest`, the composer requires explicit task
grade, topology, tier, deliberation, and posture; an assessment may supply the
selected tier and deliberation but never the other axes. It never fills an unknown role
from generic defaults. Domain requirements may explicitly be an empty list.

Selection ladder: use a stock template unchanged when its deliverable and
authority fit; use a justified stock-template override when task grade,
domains, tier, reasoning, or posture change but its fixed topology/capability
boundary still fits. A topology change is never a preset override. Any change
to responsibility, deliverable, capability/authority boundary, done criteria,
or report shape requires a bespoke/custom composition. Machine payloads retain
the v2 `presets`, `kind: "preset"`, and `nearestPreset` names for compatibility.

## Tier × deliberation resolution

`tier` and `reasoning` are chosen independently, but the pair is only
dispatchable if a provider catalog resolves it. Each `providers/<provider>.json`
maps the semantic ramp (economy → standard → senior → frontier) onto that
provider's useful model×deliberation rungs and OMITS dominated combinations —
the shingle law. One exact model×deliberation rung belongs to only one tier; a
provider's strongest model may span adjacent tiers only through disjoint
deliberation levels. A `(tier, reasoning)` pair is provider-neutral and
dispatchable iff some catalog offers it; the composer rejects any pair no
catalog resolves — before dispatch, never by silently substituting a level.
Overriding `tier` alone onto a stock template whose `reasoning` the new tier does not
offer is therefore rejected: set both axes, or set a `reasoning` the tier
supports. This rejects unsupported and dominated routes without collapsing the
two axes into one.

The current concrete matrix is generated directly from the catalogs in
[`docs/provider-matrix.md`](provider-matrix.md); it is never duplicated here.
Runtime model promotion or provider fallback performs an exact concrete-model
delta lookup. It must not inherit the original tier model's calibration: every
runtime model declares either a calibrated repo path or explicit `none` in its
provider catalog.

Provider catalogs keep two model-level facts separate. `models.<exact>.efforts`
or `.reasoning` records provider-supported levels intersected with Gaffer's
canonical deliberation vocabulary; it is not an exhaustive provider API enum.
`models.<exact>.routes.<tier>` is the smaller calibrated set of exact
model×tier×deliberation shingles. Route lists are explicit and disjoint: raw
support never implies a tier cross-product. Missing models, raw support, route
maps, tier entries, or deliberation entries fail closed.

The portable eight-field composer remains unpinned. It resolves only through
the canonical `tiers.<tier>` row, whose default model route must match that row
exactly; alternate exact-model entries do not silently broaden it. When a
harness execution envelope explicitly pins a model or alias, the adapter
instead validates the request's tier+reasoning against that selected model's
raw-support intersection and exact route. It never filters an alternate model
through the canonical tier model's effort list.

## Target resolution — owned by the harness, not by Gaffer

Gaffer stops at the semantic request. Choosing an account, a concrete model, a
transport, and an allocation strategy — and reporting what actually ran — is the
harness's job. That resolution is North's contract, not Gaffer's: Gaffer does
not enumerate its fields or name any account, pool, or model.

A conforming harness accepts the eight-field Gaffer request inside its own
execution envelope. That envelope may additionally pin a provider, account, or
exact model,
but those are North inputs rather than Gaffer fields. The harness:

1. Honors an explicit provider/account/model pin from its execution envelope, else
   selects freely among compatible accounts.
2. For an explicit model pin, requires the catalog's static raw-support and
   exact-route checks; for an unpinned request, uses the canonical tier row.
3. Independently requires an available authenticated target for the resolved
   provider/model. Static catalog compatibility establishes neither account
   entitlement nor current target availability.
4. Removes providers lacking required capabilities, authentication, or capacity.
5. Rejects candidates below the mechanically derived minimum when a selection
   assessment is attached, missing
   required capabilities, or unable to prove a named external-access
   prerequisite before the model turn.
6. Applies its own subscription-allocation policy and resource pressure; pressure
   trims optional breadth, polish, and retries before capability.
7. Resolves the semantic `tier` + `reasoning` through `providers/<provider>.json`
   to a concrete model and effort/reasoning control.
8. Records the requested route beside the resolved one for audit.

Stock-template capabilities are provider-neutral requirements. An adapter may expose
only the intersection it can enforce, and must fail closed when a required
boundary is unavailable. In particular, `shell.readonly` means an OS-enforced
working-tree write denial; removing Edit/Write while leaving an unrestricted
shell does not satisfy it. An adapter without a hard read-only shell omits the
shell rather than silently widening authority. This is also why capabilities
are stock-template facts rather than a ninth routing field: named stock templates supply them,
while a bespoke contract states the authority its adapter must realize.

Automatic fallback is SUBSTITUTION only: it preserves the semantic tier and
required capabilities and is safe only before side effects. Lowering capability,
deliberation, scope, or verification is DEGRADATION — an explicit, recorded
decision, never a disguised fallback. Allocation strategy (preferential /
balanced / reserved), account and pool identity, resource pressure, and resolved
model names are runtime facts the harness owns and records; they are never
Gaffer request fields and carry no API-credit meaning.

`auto` is a North execution-envelope value, not a Gaffer field or model name.
Gaffer intentionally does not claim that model aliases or effort labels are
equivalent between providers.

## Bespoke compositions

Composition provenance has five deliberately distinct presentation states:

- `gaffer:<preset>` — an unchanged stock template.
- `gaffer:<preset>+override` — a stock template with recorded axis changes and
  an `overrideReason`.
- `gaffer:bespoke:<id>` — an improvised, structured composition with its own
  capabilities and authority contract.
- `gaffer:not-selected` — a native session that did not select Gaffer at all.
- `gaffer:legacy-debt` — a pre-contract record whose provenance cannot yet be
  reconstructed; this is migration debt, not a stock template.

`gaffer:none` is not a valid display state because it collapses intentional
non-selection, bespoke composition, and missing legacy data into one ambiguous
label. A refreshed legacy record should be reclassified into one of the first
four states; it is never guessed into a stock template.

Every bespoke composition supplies `composition.kind = "bespoke"`, a stable
`id`, an optional `nearestPreset` stock-template reference, a one-line
`bespokeReason`, a boolean
`promotionCandidate` (false by default; nomination is explicit), and a structured contract: responsibility, deliverable,
canonical `capabilities[]`, `mayDecide[]`, `mustEscalate[]`, `doneWhen[]`, and
report. The capabilities are explicit even when `nearestPreset` is present:
the nearest stock template can seed composition defaults but never grants authority by
implication. A harness records the
requested composition beside the resolved route and evidence-backed outcome. Repeated
successful fingerprints are visible for review regardless of nomination; runtime observations never
rewrite Gaffer's stock-template library automatically. Comparable successful
recurrence means the same responsibility, deliverable, capability/authority
boundary, done criteria, and report shape has been used more than once and each
use has evidence against its done criteria. Domain wording alone does not make
two compositions comparable, and recurrence triggers review rather than
promotion.

## Current consumption boundary

`scripts/compose-routing.mjs` validates semantic tier and deliberation
resolvability; current North chooses the provider/account and resolves the pair.
Together they validate and record role, task grade, domain requirements,
topology, tier, deliberation, posture, and composition as independent metadata.
Recorded metadata is useful for audit and empirical routing reports, but does
not by itself load domain expertise, change a role contract by grade, or spawn
an orchestrator graph. Planner inputs are deliberately NOT accepted as request
fields. The seven minimum-sufficient signals are accepted only inside the
versioned `--assessment` sidecar; feeding one as a top-level composer option or
routing field fails rather than being accepted as if it worked.

## Compatibility

The harness must advertise whether it accepts this contract. North accepts the
eight semantic fields plus North-owned execution-envelope controls such as
`provider` and `target`; a legacy North accepts `model` and `effort`. Callers
detecting a legacy surface resolve the chosen tier through the provider catalog
before calling it, and must not send unknown fields hoping they are ignored.

Claude Code agent frontmatter still contains Anthropic `model` and `effort`
pins. Those are compiled adapter artifacts. The semantic `tier` is the source
decision and provider catalogs perform resolution. Existing Claude plugin
behavior therefore remains unchanged while North and other harnesses use the
portable fields.
