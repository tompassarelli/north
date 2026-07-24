# gaffer

*Every squad needs a gaffer.*

**Gaffer is a provider-neutral routing doctrine for multi-agent orchestration**
— usable from any CLI or harness, not tied to one vendor. Orchestrating
agentic workflows is complicated. Which models, at what deliberation?
Template workers or purpose-built compositions? How should each worker be
prompted, and what should each report back? **Let the gaffer figure it out.**

Gaffer's portable core (routing laws, template library, payload method) is
adapter-agnostic; concrete delivery happens through adapters, of which two
ship today: a Claude Code plugin adapter and a [North multi-provider execution
adapter](docs/adapters/north.md). Gaffer chooses the semantic route; the
adapter resolves it to the provider, model, and reasoning/effort — whether
it's a single worker or a multi-stage workflow.

Install it and your sessions gain:

1. **A routing doctrine** (injected at session start): classify each
   delegated task by *shape* — execute / implement / integrate / design /
   direct / scout / analyze / review / verify / judge / research-science — and
   route it
   on one continuous semantic ramp. Function, task grade, domain requirements,
   topology, model capability, deliberation, and posture stay independent,
   governed by three routing laws:
   - **Minimum-sufficient floor** — repository layer never raises cost by
     itself. Foundational implementation-only work can remain economy/standard;
     owning invariant, cross-boundary, or system-shaping decisions raises the
     floor mechanically.
   - **Shingle law** — each semantic step exposes only useful
     model×deliberation rungs; the same concrete rung never masquerades as two
     tiers, and dominated overlaps are omitted. Harder ⇒ climb the semantic
     ramp, not effort alone against a low route.
   - **Quality floor** — resource pressure can trim optional breadth, polish,
     and retries, but never silently route consequential work below the
     minimum responsible capability.
2. **A template library** — agents with a semantic route, exact adapter
   model, deliberation, and capability surface pinned, plus a composed role
   payload:

   | Agent | Semantic route | Shape it plays |
   |---|---|---|
   | `gaffer:executor` | economy / low | bounded mechanical changes |
   | `gaffer:implementer` | standard / medium | one feature/fix inside known patterns |
   | `gaffer:integrator` | senior / high | cross-seam work, ambiguous debugging, behavior-at-stake refactors |
   | `gaffer:designer` | frontier / xhigh | choosing shapes: APIs, data models, decomposition (decision-only, read-only tools) |
   | `gaffer:director` | frontier / xhigh | decompose, independently staff, consume evidence, and reconcile multi-agent work |
   | `gaffer:scout` | economy / low | locate, map, gather sources (breadth, fan-out) |
   | `gaffer:analyst` | senior / high | deep-dive: how/why it works, root-cause, design-grounding (read-only) |
   | `gaffer:reviewer` | senior / high | one supplied artifact/change reviewed across multiple criteria, with findings and disposition |
   | `gaffer:verifier` | senior / high | adversarial verification of one claim (justified overrides may move up or down; quality floor binds) |
   | `gaffer:judge` | frontier / xhigh | rubric-backed ranking of multiple supplied alternatives |
   | `gaffer:research-scientist` | frontier / xhigh | hypothesis/experiment design plus existing non-mutating evidence probes; new apparatus is handed off |

   Exact versioned model pins are generated from the dated provider catalogs;
   see [`docs/provider-matrix.md`](docs/provider-matrix.md). Every exact catalog
   model must have official model-family, availability, and effort-support
   provenance; provider-wide union coverage cannot substitute for a model's
   missing scope. Model entries separately record provider-supported levels
   within Gaffer's deliberation vocabulary and calibrated exact per-tier route
   shingles. Raw support never implies a tier cross-product or a live account.

   The template library also staffs **workflow stages**. The doctrine tells the
   session which member plays each stage and stops workers from silently
   inheriting a top-tier session's model; concrete invocation syntax belongs
   to the selected adapter's fenced example.

   Verification remains evidence-backed: workers report evidence, the
   coordinator drives the assembled result end-to-end and independently
   spot-checks materially load-bearing seams, and an independently staffed
   verifier returns a verdict with the probe and observed result where the
   outcome calls for one. Current lanes share one OS uid, so Gaffer does not
   treat that staffing separation as security-grade attestation; `attested` or
   `verified` status is reserved for a future protected trust boundary.

3. **Skills**:
   - `compose` — assemble a bespoke (custom) composition for spawns the
     templates do not cover (Workflow calls, unusual pairings).
   - `elicit` — calibrate a payload for a model gaffer doesn't know yet,
     using the method below.

## The payload method (the interesting part)

The agent payloads are **not** generic best-practice prompts. Each is built
by **elicit → subtract → compile**: have the model write an introspective
self-report of how it actually works (contamination-guarded), *subtract
everything it already does natively* — restating it wastes budget and
dilutes attention — and compile only the remainder, phrased in the model's
own vocabulary: its named limits become procedure, its named tells become
triggers, its stale self-models get corrected.

The shipped self-reports in `docs/self-reports/` illustrate the method: a
model's confidence tell becomes a concrete uncertainty procedure, a stale
self-model about tool access becomes a run-don't-predict correction, and
already-native engineering habits are removed so only useful residue remains.
Unversioned self-reports remain method evidence, not calibration for a newer
exact model: generated agents include a delta only when that exact model's
catalog entry says `calibrated`.
Concrete model names and their delta paths live exclusively in provider
catalogs and the generated provider matrix.

Full rationale: [`docs/method.md`](docs/method.md).

## Install

### Claude Code plugin

```
/plugin marketplace add tompassarelli/gaffer
/plugin install gaffer@gaffer
```

Start a new session — you'll see `GAFFER ACTIVE` in the session context.
Then delegate normally: the session routes by shape, or spawn a template
worker directly via the Agent tool (`subagent_type: "gaffer:implementer"`).

### North multi-provider harness

North consumes Gaffer's `staffing/catalog.json` and `providers/*.json` directly,
accepts the portable eight-field routing request, then selects an authenticated
subscription account and concrete provider runtime. The generated
[North adapter contract](docs/adapters/north.md) documents the spawn surface
and fail-closed capability mapping. Exact-model pins additionally require both
Gaffer's static exact-route acceptance and North's independent proof of an
available authenticated target; install and bootstrap North itself rather
than applying the Claude plugin commands above.

## Architecture note

`agents/*.md` are **generated** — compiled from the source blocks in
`docs/` (roles, task grades, topologies, postures, comms, deltas) by
`scripts/build-agents.mjs`.
The axes stay sharp at the source layer; the script does the flattening the
plugin format requires. Edit blocks, rebuild (`node
scripts/build-agents.mjs`), never hand-edit agent files (`--check` verifies
freshness).
Template capabilities are provider-neutral catalog labels; the generator
maps them to Claude tools, while other harnesses map the same labels through
their own adapters.

Provider resolution lives in `providers/*.json`; `docs/routing.md` defines the
portable request and fallback contract. The concrete Claude pins in generated
agents are
compiled compatibility output, not the shared vocabulary.

Terminology and selection are deliberately simple: a **template** is a
reusable, named starting composition; a **bespoke** (custom) composition is an
explicit one-off contract. Start with a template. Use it unchanged when its
deliverable and authority fit; use a justified override when task grade,
domains, tier, reasoning, or posture change but the fixed topology/capability
boundary still fits. Topology is never a template override. Use a bespoke
composition when topology, responsibility, deliverable, capability/authority
boundary, done criteria, or report shape differs. The v2 machine schema
retains `presets`, `kind: "preset"`, and `nearestPreset` for compatibility.

Each template is a transparent starting composition, not an identity
constraint:
`role` names the deliverable, `taskGrade` ranges from `novice` through
`research-grade`, domain requirements state expertise/context the brief or
adapter must supply (metadata alone loads nothing), topology grants
coordination authority, semantic tier sets a model capability floor, and
deliberation sets reasoning depth; posture sets what yields under collision.
Template overrides record exactly which overrideable axes changed and why.
Bespoke compositions carry a complete authority/deliverable/done contract and
an explicit promotion decision.

The axes are conceptually independent, but the shipped templates bind
topology to a fixed, enforceable capability set: director carries
orchestrator/coordination authority and the other stock templates carry worker
authority. A topology change requires a bespoke composition; it never
manufactures capabilities on a stock template.

Worker authority is terminal. Orchestrator authority may recursively staff a
worker or child orchestrator when that child's local dependency shape warrants
it, but every child crosses a fresh North admission, routing, resource, metering,
and settlement boundary. The immediate parent owns reduction. Explicit budgets,
cycle/no-progress controls, and settlement gates stop recursion; Gaffer does not
impose a global depth cap. Provider-native opaque fanout is not equivalent and
remains unavailable under North until the same per-child boundary is enforceable.

Gaffer also separates task economics from provider state. The seven
minimum-sufficient signals capture where better judgment changes outcomes and
how work composes; dependency shape argues for one worker, a fixed workflow, a
director with parallel workers, or a strong sequential worker plus a verifier
when verdict leverage warrants one. These are **planner inputs** in a sidecar,
not fields on the request. The routing request carries
only role, task grade, domain requirements, topology (`worker`/`orchestrator`),
tier, reasoning, posture, and composition; the composer validates the semantic
pair and North resolves its provider/account/model. Allocation strategy,
candidate waterfalls, resolved model, account identity, and resource pressure
are North's runtime facts, never Gaffer request fields. Same-tier substitution
may happen automatically before side effects; lowering capability or
verification is explicit degradation.

New selections can carry a separate
[`minimum-sufficient-v1`](contracts/selection-assessment.schema.json) assessment.
It records decision ownership, seam scope, error exposure, oracle strength,
foundational impact, dependency shape, and reasoning shape; its derived minimum
and rule codes are recomputed rather than trusted. A selected route below the
minimum is rejected, one above it requires a coded detailed exception, and
`max` requires a concrete exceptional-deliberation justification. The sidecar
does not alter the exact eight-field routing request, so existing consumers
remain compatible.

## Tuning

- Different top tier available? The semantic ramp extends naturally — run
  `elicit` to calibrate a delta for that exact concrete model, or record an
  explicit `none`; never inherit a neighboring model's delta.
- The templates are a **standard library, not a roster limit**: bespoke
  compositions are first-class — the laws still bind them, the
  blocks are borrowable parts, and recurring bespoke patterns are surfaced for
  review; promotion requires an explicit library change. Full contract:
  [`docs/extending.md`](docs/extending.md).
- Template capabilities are enforceable authority. Non-authoring templates
  request
  `shell.readonly`; an adapter must provide a hard write-denying sandbox or
  withhold shell access. Generated Claude plugin agents take the latter path
  because plugin frontmatter cannot encode a hard sandbox.

## Related work

- [Anthropic's multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)
  and [OpenAI Agents SDK orchestration](https://openai.github.io/openai-agents-python/multi_agent/)
  document orchestrator/manager-worker patterns, specialization, parallel
  independent work, delegation contracts, and the added cost of coordination.
  [LangChain](https://docs.langchain.com/oss/python/langchain/multi-agent),
  [AutoGen](https://microsoft.github.io/autogen/stable/user-guide/agentchat-user-guide/tutorial/teams.html),
  and [Google ADK](https://adk.dev/workflows/patterns/) likewise emphasize
  choosing multi-agent patterns only when coordination pays. Gaffer borrows
  those patterns; its own layer is the independent routing axes, enforceable
  template/bespoke contracts, and the semantic-tier/deliberation boundary with
  runtime subscription allocation.
- [tzachbon/claude-model-router-hook](https://github.com/tzachbon/claude-model-router-hook) —
  same delivery mechanism (SessionStart-injected routing rules), keyword-based
  three-model tiers. Gaffer adds the shape taxonomy, effort as a first-class
  dial, and the layer-floor / shingle laws.
- [wshobson/agents](https://github.com/wshobson/agents) — model pinning at
  scale (194 domain agents). No effort pinning; domain taxonomy rather than
  task-shape taxonomy.
- [JuliusBrussee/caveman](https://github.com/JuliusBrussee/caveman)'s
  cavecrew agents — the scope-refusal, redirect-protocol, and compact
  report-contract patterns, which gaffer's agents adopt. Cavecrew optimizes
  coordinator context; gaffer optimizes routing + calibration.
- Anthropic's per-model prompting guides — prior art for per-model
  behavioral calibration as a concept. Gaffer's contribution is the
  mechanism: elicit a contamination-guarded self-report from the model,
  subtract what it already does natively, compile only the residue into the
  agent's system prompt. We did not find model-and-effort pinning per agent or
  this subtraction method in the systems cited above; that is a scoped search
  result, not a claim of exhaustive novelty.

## License

Gaffer is dual-licensed under your choice of the [MIT License](LICENSE-MIT) or
the [Apache License, Version 2.0](LICENSE-APACHE)
(`MIT OR Apache-2.0`).
