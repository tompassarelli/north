SPAWN SURFACES (adapter: north) — a squad member is the eight-field
Gaffer request (role, taskGrade, domainRequirements, topology, tier, reasoning,
posture, composition), delivered on the North substrate. Provider, account,
and an optional exact-model pin are North execution-envelope controls. Native Agent/Task/Workflow are DENIED
here (dispatch=north) — the harness still advertises gaffer:* + native agent
types, IGNORE that and go STRAIGHT to north; never let the advertised list bait a
native call (that is the recurring misfire).
- contract-v2 job → mcp__north__spawn {prompt, provider, model, tier, role, posture,
  taskGrade, domainRequirements, topology, reasoning, composition}
- fan-out → one mcp__north__spawn per lane in the SAME turn; observe at web :8088
- thread-driven → capture the thread, then mcp__north__dispatch (posture from claims)
Every canonical role passes North's open `role` string so its block is loaded
and the choice is observable. Bespoke role names are also allowed; their
authority/deliverable contract and explicit canonical capabilities ride in the
prompt. A nearest stock template may seed defaults but never grants capabilities.
Stock-template overrides may change task grade, domains, tier, reasoning, or
posture with one justification. Stock topology is fixed; a topology,
responsibility, deliverable, capability/authority boundary, done-criteria, or
report-shape change requires a bespoke composition.
Pin task grade+tier+posture.
Use provider=auto unless policy or the caller explicitly overrides it. These
fields form North's v2 staffing contract: North assembles the selected role,
task-grade, topology, posture, communication, and exact-model calibration
blocks; North gates each named domain requirement on explicit brief context,
relevant loaded repo docs/skills/capability, or escalation — metadata alone
never confers expertise. A domain requirement is a context/prompt gate:
it is not proof of arbitrary external-service authority. Deterministic Linear
synchronization uses the separate `north linear` surface; other external
operations still require an authenticated execution surface established before
dispatch. North intersects the stock template's provider-neutral capabilities
with the selected adapter's concrete tool surface. Orchestrator topology is
admitted only when the composition explicitly carries coordination capability
and the adapter can enforce it; topology alone never loads the director role.
Capability enforcement is fail-closed. `shell.readonly` means a shell whose
working tree cannot be written, not merely a tool list without Edit/Write.
For managed Anthropic lanes North denies native Bash and exposes
`mcp__north-readonly-shell__run`: a bwrap-backed read-only host/checkout,
ephemeral `/tmp`, no network, and a cleared environment; unavailable
enforcement fails closed at preflight. For managed OpenAI lanes North launches
Codex with `--sandbox read-only` and marks North MCP required.
OpenAI orchestration is currently ineligible and fails pre-turn; with
`provider=auto`, North may select an eligible Anthropic target instead.
Claude plugin-agent frontmatter cannot encode a hard sandbox, so the generated
plugin adapter withholds Bash for `shell.readonly` stock templates rather
than claiming a boundary it cannot provide.
North presents composition provenance as `gaffer:<preset>`,
`gaffer:<preset>+override`, or `gaffer:bespoke:<id>`. A native session that
did not select Gaffer is `gaffer:not-selected`; only pre-contract records may
use `gaffer:legacy-debt`. Never collapse these states to `gaffer:none`.
Comparable successful bespoke recurrence is evidence for review, never
automatic promotion: responsibility, deliverable, capability/authority
boundary, done criteria, and report shape recur, and each use carries
done-criteria evidence.
For an unpinned request, North resolves tier+reasoning through the catalog's
canonical tier row and records both requested and concrete routes. An explicit
exact-model pin is a separate execution-envelope constraint: first resolve its
alias, then require the reasoning level in that model's provider-supported
Gaffer-vocabulary list AND in `models[exact].routes[tier]`. Never infer a
tier×reasoning cross-product from independent lists, and never filter an
alternate model through the tier's default model. Unknown models, missing or
empty support/routes, and unsupported exact shingles fail closed. This static
Gaffer result is necessary but never proves subscription entitlement or live
availability; North must independently establish an available authenticated
target for that exact provider/model before the turn. Both checks are required.
Routing defaults
(canonical stock templates — generated from the machine `presets` key, do not hand-edit):

  gaffer role         task grade      tier      reasoning  topology      posture   capabilities
  ------------------  --------------  --------  ---------  ------------  --------  -----------------------------------------------------------------
  executor            novice          economy   low        worker        deliver   filesystem.read,filesystem.search,filesystem.write,shell
  implementer         mid             standard  medium     worker        deliver   filesystem.read,filesystem.search,filesystem.write,shell
  integrator          senior          senior    high       worker        deliver   filesystem.read,filesystem.search,filesystem.write,shell
  designer            staff           frontier  xhigh      worker        explore   filesystem.read,filesystem.search,shell.readonly
  director            staff           frontier  xhigh      orchestrator  deliver   filesystem.read,filesystem.search,shell.readonly,web,coordination
  scout               junior          economy   low        worker        explore   filesystem.read,filesystem.search,shell.readonly,web
  analyst             senior          senior    high       worker        explore   filesystem.read,filesystem.search,shell.readonly,web
  reviewer            senior          senior    high       worker        evaluate  filesystem.read,filesystem.search,shell.readonly
  verifier            senior          senior    high       worker        evaluate  filesystem.read,filesystem.search,shell.readonly
  judge               staff           frontier  xhigh      worker        evaluate  filesystem.read,filesystem.search,shell.readonly
  research-scientist  research-grade  frontier  xhigh      worker        explore   filesystem.read,filesystem.search,shell.readonly,web

ORCHESTRATION (role-jurisdiction law, see doctrine.md): a WORKER owns one
terminal piece and MUST NOT delegate. An ORCHESTRATOR coordinates rather than
executing terminal work. It classifies every direct child from that child's
LOCAL dependency shape: atomic or tightly coupled → worker; independently
decomposable → child orchestrator. Every child is a fresh mcp__north__spawn or
dispatch with its own complete Gaffer request, North admission, provider/account
resolution, resource envelope, telemetry, and settlement; no route or budget is
inherited by nesting. Verification is a sibling lane owned by the immediate
orchestrator. The parent consumes worker evidence or a child orchestrator's
reconciled outcome, runs bounded independent non-authoring probes at materially
load-bearing direct-child seams, and OWNS REDUCTION of those direct children. A
probe may create disposable test/build/cache state needed for observation, but
never edits, implements, or repairs the deliverable or absorbs a worker's full
local-probe burden. Never bypass a child orchestrator with flat fan-in.
STOP-RULE: subdivide only while it buys more independence, certainty, or
verifiability than integration cost. A clear objective with bounded scope,
known I/O, and a verification path is terminal. Recursion stops through this
local rule, explicit budgets, cycle detection, bounded no-progress/retry
controls, and settlement gates — never a global depth cap. Deliverables return
UP to the immediate parent, never sideways. Provider-native opaque fanout is
distinct and disallowed under North until equivalent per-child admission,
authority, metering, and settlement can be enforced.

If a native call slips through, the agent-spawn-guard hook denies with the exact
mcp__north__spawn call pre-resolved for that role and tier — one-paste recovery. A native
denial is a routing instruction, never a wall: translate, never abandon the
squad pick or drop to an unrouted spawn.
