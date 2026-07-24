#!/usr/bin/env node
// Compiles agents/*.md from the source blocks in docs/.
// Axes stay sharp at the source layer (one block per axis value); this
// script does the flattening the plugin format requires. Run after editing
// any block: node scripts/build-agents.mjs   (--check verifies, no writes)
import { readFileSync, writeFileSync, existsSync, mkdirSync, unlinkSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadStaffingCatalog } from "./staffing-catalog.mjs";
import { loadProviderCatalog, modelDeltaFor } from "./provider-catalog.mjs";
import { canonicalRoleId, containedLeaf } from "./role-id.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(resolve(ROOT, p), "utf8");
const agentPath = (role) => containedLeaf(resolve(ROOT, "agents"), `${canonicalRoleId(role)}.md`, `generated agent ${role}`);

// heading -> first fenced block after it (same extraction praxis consumers use)
function block(text, heading) {
  const lines = text.split("\n");
  const h = `## ${heading.toLowerCase()}`;
  let at = lines.findIndex((l) => l.trim().toLowerCase() === h);
  if (at === -1) throw new Error(`heading not found: ${heading}`);
  let open = -1;
  for (let i = at + 1; i < lines.length; i++) {
    const t = lines[i].trim();
    if (open === -1 && t.startsWith("## ")) break;
    if (open === -1 && t.startsWith("```")) { open = i + 1; continue; }
    if (open !== -1 && t.startsWith("```")) return lines.slice(open, i).join("\n");
  }
  throw new Error(`no fence under heading: ${heading}`);
}
const firstFence = (text) => {
  const m = text.match(/```\n([\s\S]*?)\n```/);
  if (!m) throw new Error("no fence in delta doc");
  return m[1];
};

const roles = read("docs/roles.md");
const taskGrades = read("docs/task-grades.md");
const postures = read("docs/postures.md");
const topologies = read("docs/topologies.md");
const comms = block(read("docs/comms.md"), "universal");
const anthropic = loadProviderCatalog("anthropic");

const staffing = loadStaffingCatalog();
export const SEMANTIC_TIERS = staffing.vocabulary.semanticTiers;
export const PRESETS = staffing.presets;
const COMPAT_ALIASES = staffing.aliases;
const CLAUDE_TOOLS = {
  "filesystem.read": ["Read"],
  "filesystem.search": ["Grep", "Glob"],
  "filesystem.write": ["Edit", "Write"],
  shell: ["Bash"],
  // Claude plugin-agent frontmatter cannot carry a hard sandbox policy.
  // A read-only shell therefore fails closed on this adapter: North may map
  // it to sandboxed Bash, but the generated native plugin withholds Bash.
  "shell.readonly": [],
  web: ["WebSearch", "WebFetch"],
  coordination: ["Agent"],
  // graph-authoring.fram is a North-managed sealed Fram MCP server injected by
  // the dispatch harness for managed lanes only. Native Claude plugin
  // frontmatter cannot carry that MCP, so this adapter fails closed with no
  // native tool, exactly like shell.readonly.
  "graph-authoring.fram": [],
};
if (JSON.stringify([...staffing.vocabulary.capabilities].sort()) !== JSON.stringify(Object.keys(CLAUDE_TOOLS).sort()))
  throw new Error("Claude tool adapter must map every canonical staffing capability exactly once");

function claudeTools(capabilities) {
  return [...new Set(capabilities.flatMap((capability) => CLAUDE_TOOLS[capability] ?? []))].join(", ");
}

// JSON string literals are valid YAML scalars. Serializing every free string
// this way prevents punctuation in descriptions (especially `: `) from
// changing the frontmatter's YAML structure.
const yamlString = (value) => JSON.stringify(String(value));

// Generated Claude Code agents are an adapter artifact. Resolve their concrete
// pins from the Anthropic catalog while keeping stock templates provider-neutral.
for (const preset of PRESETS) {
  const resolved = anthropic.tiers[preset.tier];
  if (!resolved) throw new Error(`Anthropic catalog does not resolve tier: ${preset.tier}`);
  if (!resolved.efforts?.includes(preset.deliberation))
    throw new Error(`${preset.name}: Claude adapter tier ${preset.tier} does not support deliberation ${preset.deliberation}`);
  preset.model = resolved.model;
  preset.effort = preset.deliberation;
}

function render(r) {
  const effectivePosture = r.posture;
  const delta = modelDeltaFor(anthropic, r.model);
  const routingPayload = JSON.stringify({
    role: r.routingRole || r.name,
    taskGrade: r.taskGrade,
    domainRequirements: [],
    topology: r.topology,
    tier: r.tier,
    reasoning: r.deliberation,
    posture: effectivePosture,
    composition: { kind: "preset", id: r.routingRole || r.name, overrides: [] },
  });
  const fm = [
    "---",
    `name: ${yamlString(r.name)}`,
    `description: ${yamlString(`${r.description} Task grade: ${r.taskGrade}.`)}`,
    `model: ${yamlString(r.model)}`,
    `effort: ${yamlString(r.effort)}`,
    `tools: ${yamlString(claudeTools(r.capabilities))}`,
    "---",
  ].join("\n");
  const parts = [
    fm,
    "",
    "<!-- GENERATED by scripts/build-agents.mjs — edit docs/ blocks or staffing/catalog.json, then rebuild. Do not edit by hand. -->",
    `<!-- GAFFER_ROUTING ${routingPayload} -->`,
    "",
    `You are the ${r.name}: ${r.tagline}.`,
    "",
    "## Role",
    block(roles, r.roleBlock || r.name),
    "",
    `## Task grade: ${r.taskGrade}`,
    block(taskGrades, r.taskGrade),
    "",
    `## Topology: ${r.topology}`,
    block(topologies, r.topology),
    "",
    `## Posture: ${effectivePosture}`,
    block(postures, effectivePosture),
  ];
  parts.push("", "## Output norms", comms);
  if (delta.kind === "calibrated")
    parts.push("", "## Delta protocol — tuned to this model's documented tendencies", firstFence(read(delta.path)));
  return parts.join("\n") + "\n";
}

function renderAlias(alias) {
  const target = PRESETS.find((r) => r.name === alias.target);
  return render({
    ...target,
    name: alias.name,
    routingRole: alias.target,
    roleBlock: alias.target,
    description: `Deprecated compatibility alias for gaffer:${alias.target}. ${target.description}`,
  });
}

// The north spawn-adapter's SPAWN SURFACES doctrine block — generated from the
// SAME PRESETS so the dials never drift from the agents. scripts/inject-doctrine.sh
// swaps this in for the native block when GAFFER_SPAWN_ADAPTER=north (or
// dispatch=north). Every role passes through North's open role string; the
// matching Gaffer block is loaded when present and bespoke contracts ride in
// the prompt.
function renderNorthAdapter() {
  const rows = PRESETS.map((r) => ({
    role: r.name, grade: r.taskGrade, tier: r.tier, reasoning: r.deliberation,
    topology: r.topology,
    posture: r.posture,
    capabilities: r.capabilities.join(","),
  }));
  const cols = [["gaffer role", "role"], ["task grade", "grade"], ["tier", "tier"],
    ["reasoning", "reasoning"], ["topology", "topology"], ["posture", "posture"],
    ["capabilities", "capabilities"]];
  const w = cols.map(([h, k]) => Math.max(h.length, ...rows.map((r) => String(r[k]).length)));
  const fmt = (cells) => ("  " + cells.map((c, i) => String(c).padEnd(w[i])).join("  ")).replace(/\s+$/, "");
  const table = [
    fmt(cols.map(([h]) => h)),
    fmt(w.map((n) => "-".repeat(n))),
    ...rows.map((r) => fmt(cols.map(([, k]) => r[k]))),
  ].join("\n");
  return `SPAWN SURFACES (adapter: north) — a squad member is the eight-field
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
Every canonical role passes North's open \`role\` string so its block is loaded
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
synchronization uses the separate \`north linear\` surface; other external
operations still require an authenticated execution surface established before
dispatch. North intersects the stock template's provider-neutral capabilities
with the selected adapter's concrete tool surface. Orchestrator topology is
admitted only when the composition explicitly carries coordination capability
and the adapter can enforce it; topology alone never loads the director role.
Capability enforcement is fail-closed. \`shell.readonly\` means a shell whose
working tree cannot be written, not merely a tool list without Edit/Write.
For managed Anthropic lanes North denies native Bash and exposes
\`mcp__north-readonly-shell__run\`: a bwrap-backed read-only host/checkout,
ephemeral \`/tmp\`, no network, and a cleared environment; unavailable
enforcement fails closed at preflight. For managed OpenAI lanes North launches
Codex with \`--sandbox read-only\` and marks North MCP required.
OpenAI orchestration is currently ineligible and fails pre-turn; with
\`provider=auto\`, North may select an eligible Anthropic target instead.
Claude plugin-agent frontmatter cannot encode a hard sandbox, so the generated
plugin adapter withholds Bash for \`shell.readonly\` stock templates rather
than claiming a boundary it cannot provide.
North presents composition provenance as \`gaffer:<preset>\`,
\`gaffer:<preset>+override\`, or \`gaffer:bespoke:<id>\`. A native session that
did not select Gaffer is \`gaffer:not-selected\`; only pre-contract records may
use \`gaffer:legacy-debt\`. Never collapse these states to \`gaffer:none\`.
Comparable successful bespoke recurrence is evidence for review, never
automatic promotion: responsibility, deliverable, capability/authority
boundary, done criteria, and report shape recur, and each use carries
done-criteria evidence.
For an unpinned request, North resolves tier+reasoning through the catalog's
canonical tier row and records both requested and concrete routes. An explicit
exact-model pin is a separate execution-envelope constraint: first resolve its
alias, then require the reasoning level in that model's provider-supported
Gaffer-vocabulary list AND in \`models[exact].routes[tier]\`. Never infer a
tier×reasoning cross-product from independent lists, and never filter an
alternate model through the tier's default model. Unknown models, missing or
empty support/routes, and unsupported exact shingles fail closed. This static
Gaffer result is necessary but never proves subscription entitlement or live
availability; North must independently establish an available authenticated
target for that exact provider/model before the turn. Both checks are required.
Routing defaults
(canonical stock templates — generated from the machine \`presets\` key, do not hand-edit):

${table}

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
squad pick or drop to an unrouted spawn.`;
}

function renderProviderMatrix() {
  const catalogs = ["anthropic", "openai"].map((provider) => loadProviderCatalog(provider));
  const aliasesFor = (catalog, model) => Object.entries(catalog.modelAliases)
    .filter(([, exact]) => exact === model)
    .map(([alias]) => `\`${alias}\``)
    .join(", ") || "—";
  const groupThousands = (value) => String(value).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const lines = [
    "# Provider resolution matrix",
    "",
    "<!-- GENERATED by scripts/build-agents.mjs from providers/*.json. Do not edit by hand. -->",
    "",
    "Concrete models and calibrated prompt deltas are provider-adapter facts. The",
    "semantic Gaffer request remains provider-neutral.",
    "",
    "## Freshness and official provenance",
    "",
    "Official sources establish only the listed model-family, availability,",
    "effort-support, context-window, and effective-date facts, with",
    "every exact catalog model covered for each fact",
    "category. Semantic tier placement, defaults, and omitted or",
    "dominated rungs are Gaffer's dated calibration judgments; the sources do not",
    "establish Gaffer's exact rung economics.",
    "Review-after dates are advisory freshness signals: overdue validation emits a",
    "warning but remains reproducible and nonfatal; malformed or reversed dates fail.",
    "",
    "| Provider | As of | Review after | Official sources and scope |",
    "|---|---|---|---|",
  ];
  for (const catalog of catalogs) {
    const sources = catalog.provenance.sources.map((source) => {
      const families = source.modelFamilies.map((model) => `\`${model}\``).join(", ");
      return `[${families}](${source.url}) — ${source.scopes.join(", ")}`;
    }).join("<br>");
    lines.push(`| ${catalog.provider} | ${catalog.provenance.asOf} | ${catalog.provenance.reviewAfter} | ${sources} |`);
  }
  lines.push(
    "",
    "## Semantic resolution",
    "",
    "Catalog aliases resolve to the exact model ID shown here before exact-model",
    "delta lookup; unversioned calibration is never inherited.",
    "",
    "| Provider | Tier | Exact model | Aliases | Deliberation | Default | Model delta |",
    "|---|---|---|---|---|---|---|",
  );
  const exactCompatibility = [];
  const contextWindows = [];
  const runtimeOnly = [];
  for (const catalog of catalogs) {
    const provider = catalog.provider;
    const tierModels = new Set();
    for (const tier of SEMANTIC_TIERS) {
      const entry = catalog.tiers[tier];
      tierModels.add(entry.model);
      const levels = entry.efforts ?? entry.reasoning;
      const defaultLevel = entry.defaultEffort ?? entry.defaultReasoning;
      const delta = modelDeltaFor(catalog, entry.model);
      const deltaLabel = delta.kind === "calibrated"
        ? `[calibrated](${delta.path.replace(/^docs\//, "")})`
        : `none — ${delta.reason}`;
      lines.push(`| ${provider} | ${tier} | \`${entry.model}\` | ${aliasesFor(catalog, entry.model)} | ${levels.join(", ")} | ${defaultLevel} | ${deltaLabel} |`);
    }
    for (const [model, descriptor] of Object.entries(catalog.models)) {
      const vocabulary = descriptor.efforts ? "effort" : "reasoning";
      const supported = descriptor.efforts ?? descriptor.reasoning;
      const routed = new Set(Object.values(descriptor.routes).flat());
      const unrouted = supported.filter((level) => !routed.has(level));
      const routes = Object.entries(descriptor.routes)
        .map(([tier, levels]) => `${tier}: ${levels.join(", ")}`)
        .join("<br>");
      exactCompatibility.push(`| ${provider} | \`${model}\` | ${aliasesFor(catalog, model)} | ${vocabulary} | ${supported.join(", ")} | ${routes} | ${unrouted.join(", ") || "—"} |`);
      const cw = descriptor.contextWindow;
      contextWindows.push(`| ${provider} | \`${model}\` | ${aliasesFor(catalog, model)} | ${groupThousands(cw.tokens)} | ${cw.effectiveFrom} |`);
    }
    for (const [model, delta] of Object.entries(catalog.modelDeltas)) {
      if (tierModels.has(model)) continue;
      const deltaLabel = delta.kind === "calibrated"
        ? `[calibrated](${delta.path.replace(/^docs\//, "")})`
        : `none — ${delta.reason}`;
      runtimeOnly.push(`| ${provider} | \`${model}\` | ${aliasesFor(catalog, model)} | ${deltaLabel} |`);
    }
  }
  lines.push(
    "",
    "## Exact-model pin compatibility",
    "",
    "The support column records provider-supported levels only within Gaffer's",
    "canonical deliberation vocabulary; it is not an exhaustive provider API",
    "enum. Routes are a separate Gaffer calibration: each row names exact",
    "model×tier×deliberation shingles, never a cross-product. Raw support does",
    "not make an omitted shingle routable. Supported-but-unrouted levels remain",
    "future calibration inputs, not dispatchable routes. Unpinned requests use the",
    "canonical semantic-resolution table above; this table is consulted only when",
    "the execution envelope explicitly pins an exact model or alias.",
    "",
    "Static compatibility is only one preflight. Account entitlement and current",
    "target availability are independent North facts; North must prove an",
    "available authenticated target for the exact provider/model before dispatch.",
    "No catalog support or route entry establishes either runtime fact.",
    "",
    "| Provider | Exact model | Aliases | Control | Provider-supported levels in Gaffer vocabulary | Calibrated exact routes | Supported but unrouted |",
    "|---|---|---|---|---|---|---|",
    ...exactCompatibility,
  );
  lines.push(
    "",
    "## Context window (provider limit)",
    "",
    "Each token count is the provider-published context-window ceiling for the",
    "exact model — the maximum the provider will accept. It is a model-level fact",
    "recorded only on the exact model; a local alias inherits it after resolution.",
    "The ceiling is not the usable harness budget: the runtime must reserve",
    "space for the system prompt, tool schemas, and generated output, so the",
    "dispatchable budget is always strictly smaller and is an independent runtime",
    "fact, never derived here. `effective from` dates the provider fact and is no",
    "later than the catalog `as of` snapshot; official context-window and",
    "effective-date provenance is listed in the freshness table above.",
    "",
    "| Provider | Exact model | Aliases | Provider limit (tokens) | Effective from |",
    "|---|---|---|---|---|",
    ...contextWindows,
  );
  if (runtimeOnly.length) lines.push(
    "",
    "## Runtime-only exact-model delta entries",
    "",
    "These models are not a canonical unpinned semantic-tier default. An explicit",
    "model pin must pass the exact compatibility table above; exact delta lookup",
    "then prevents calibration inheritance from the default tier model.",
    "",
    "| Provider | Exact model | Aliases | Model delta |",
    "|---|---|---|---|",
    ...runtimeOnly,
  );
  return [...lines, ""].join("\n");
}

const check = process.argv.includes("--check");
let dirty = 0;
for (const r of PRESETS) {
  const path = agentPath(r.name);
  const out = render(r);
  const cur = existsSync(path) ? readFileSync(path, "utf8") : "";
  if (cur === out) continue;
  if (check) { console.error(`STALE: agents/${r.name}.md`); dirty++; }
  else { writeFileSync(path, out); console.log(`wrote agents/${r.name}.md`); }
}
for (const alias of COMPAT_ALIASES) {
  const path = agentPath(alias.name);
  const out = renderAlias(alias);
  const cur = existsSync(path) ? readFileSync(path, "utf8") : "";
  if (cur === out) continue;
  if (check) { console.error(`STALE: agents/${alias.name}.md (compat alias)`); dirty++; }
  else { writeFileSync(path, out); console.log(`wrote agents/${alias.name}.md (compat alias)`); }
}
for (const pathName of ["agents/researcher.md"]) {
  const path = resolve(ROOT, pathName);
  if (!existsSync(path)) continue;
  if (check) { console.error(`STALE retired adapter: ${pathName}`); dirty++; }
  else { unlinkSync(path); console.log(`removed retired adapter ${pathName}`); }
}

// Generated spawn-adapter blocks (same drift-check contract as the agents).
const ADAPTERS = [
  { path: "docs/adapters/north.md", render: renderNorthAdapter },
  { path: "docs/provider-matrix.md", render: renderProviderMatrix },
];
for (const a of ADAPTERS) {
  const path = resolve(ROOT, a.path);
  const out = a.render().replace(/\n*$/, "\n");
  const cur = existsSync(path) ? readFileSync(path, "utf8") : "";
  if (cur === out) continue;
  if (check) { console.error(`STALE: ${a.path}`); dirty++; }
  else { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, out); console.log(`wrote ${a.path}`); }
}

if (check && dirty) process.exit(1);
console.log(check ? "check: all current" : "build: done");
