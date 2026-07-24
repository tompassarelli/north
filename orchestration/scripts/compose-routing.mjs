#!/usr/bin/env node
import { loadStaffingCatalog } from "./staffing-catalog.mjs";
import { readFileSync } from "node:fs";
import { presetOverrides, validateRoutingRequest } from "./routing-request.mjs";
import { canonicalRoleId } from "./role-id.mjs";
import { assertAssessmentSelection, validateSelectionAssessment } from "./selection-assessment.mjs";

const usage = `usage: node scripts/compose-routing.mjs <role> [options]

Routing options:
  --task-grade <grade>      novice|junior|mid|senior|staff|principal|research-grade
  --domain <name[,name]>    repeatable domain requirement
  --topology <kind>         worker|orchestrator (bespoke compositions only)
  --tier <tier>             economy|standard|senior|frontier
  --deliberation <level>    low|medium|high|xhigh|max (alias: --reasoning; emitted as reasoning)
  --posture <posture>       explore|deliver|preserve|evaluate
  --nearest <template>      optional stock-template reference/defaults for a bespoke composition
  --rationale <reason>      required when <role> is not a stock template or alias
  --contract <JSON|@file>   bespoke authority/deliverable/done contract
  --assessment <JSON|@file> minimum-sufficient-v1 selection sidecar
  --promotion-candidate     nominate a bespoke composition for review
  --no-promotion-candidate  explicit false (the default; accepted for clarity)
  --override-reason <why>   required when changing an overrideable stock-template axis

Without --nearest, a bespoke composition must explicitly set --task-grade,
--topology, --tier, --deliberation/--reasoning, and --posture. Domain
requirements remain an explicit empty list when no --domain is supplied.

Prints one provider-neutral GAFFER_ROUTING JSON payload. Machine output retains
the v2 keys kind:"preset" and nearestPreset for compatibility.`;

function die(message) { console.error(message); console.error(usage); process.exit(1); }

function argumentsOf(argv) {
  if (!argv.length || argv.includes("--help") || argv.includes("-h")) {
    console.log(usage);
    process.exit(argv.length ? 0 : 1);
  }
  const role = argv[0];
  if (role.startsWith("-")) die("role must be the first argument");
  const values = { domains: [], promotionCandidate: false };
  const names = {
    "--taskGrade": "taskGrade", "--task-grade": "taskGrade", "--domain": "domain",
    "--topology": "topology", "--tier": "tier", "--deliberation": "deliberation", "--reasoning": "deliberation",
    "--posture": "posture", "--nearest": "nearest", "--rationale": "rationale",
    "--contract": "contract", "--assessment": "assessment", "--override-reason": "overrideReason",
    "--promotion-candidate": "promotionCandidate", "--no-promotion-candidate": "noPromotionCandidate",
  };
  for (let index = 1; index < argv.length; index++) {
    const [rawName, inline] = argv[index].split(/=(.*)/s, 2);
    const name = names[rawName];
    if (!name) die(`unknown option: ${rawName}`);
    if (name === "promotionCandidate" || name === "noPromotionCandidate") {
      if (values.promotionSpecified) die("choose exactly one promotion decision");
      values.promotionSpecified = true;
      values.promotionCandidate = name === "promotionCandidate";
      continue;
    }
    const value = inline ?? argv[++index];
    if (!value || value.startsWith("--")) die(`${rawName} requires a value`);
    if (name === "domain") values.domains.push(...value.split(",").map((part) => part.trim()).filter(Boolean));
    else values[name] = value;
  }
  return { role, ...values };
}

const catalog = loadStaffingCatalog();
const args = argumentsOf(process.argv.slice(2));
try { canonicalRoleId(args.role, "role"); }
catch (error) { die(error.message); }
const alias = catalog.aliases.find(({ name }) => name === args.role);
const canonicalRole = alias?.target ?? args.role;
const preset = catalog.presets.find(({ name }) => name === canonicalRole);
const nearest = args.nearest && catalog.presets.find(({ name }) => name === args.nearest);
if (args.nearest && !nearest) die(`unknown nearest stock template: ${args.nearest}`);
if (preset && (args.nearest || args.rationale || args.contract || args.promotionSpecified))
  die("--nearest, --rationale, --contract, and promotion decisions apply only to bespoke compositions");
if (args.topology !== undefined && !catalog.vocabulary.topologies.includes(args.topology))
  die(`invalid topology: ${args.topology}`);
if (preset && args.topology !== undefined)
  die("--topology applies only to bespoke compositions; stock-template topology is fixed");
if (!preset && !args.rationale?.trim()) die(`bespoke composition ${JSON.stringify(args.role)} requires --rationale`);
if (!preset && !args.contract) die(`bespoke composition ${JSON.stringify(args.role)} requires --contract JSON|@file`);
if (!preset && args.overrideReason) die("--override-reason applies only to stock-template axis overrides");
if (!preset && !nearest) {
  const required = [
    ["taskGrade", "--task-grade"], ["topology", "--topology"], ["tier", "--tier"],
    ["deliberation", "--deliberation/--reasoning"], ["posture", "--posture"],
  ];
  const assessedFields = args.assessment ? new Set(["tier", "deliberation"]) : new Set();
  const missing = required.filter(([field]) => args[field] === undefined && !assessedFields.has(field)).map(([, option]) => option);
  if (missing.length)
    die(`bespoke composition without --nearest must explicitly set: ${missing.join(", ")}`);
}

function parseContract(input) {
  const source = input.startsWith("@") ? readFileSync(input.slice(1), "utf8") : input;
  try { return JSON.parse(source); }
  catch (error) { die(`--contract must be valid JSON or @file: ${error.message}`); }
}

function parseAssessment(input) {
  const source = input.startsWith("@") ? readFileSync(input.slice(1), "utf8") : input;
  let value;
  try { value = JSON.parse(source); }
  catch (error) { die(`--assessment must be valid JSON or @file: ${error.message}`); }
  try { return validateSelectionAssessment(value); }
  catch (error) { die(error.message); }
}

const template = preset ?? nearest ?? catalog.defaults;
const assessment = args.assessment ? parseAssessment(args.assessment) : null;
if (assessment && args.tier !== undefined && args.tier !== assessment.selected.tier)
  die(`--tier ${args.tier} conflicts with assessment selection ${assessment.selected.tier}`);
if (assessment && args.deliberation !== undefined && args.deliberation !== assessment.selected.reasoning)
  die(`--deliberation/--reasoning ${args.deliberation} conflicts with assessment selection ${assessment.selected.reasoning}`);
const selected = {
  taskGrade: args.taskGrade ?? template.taskGrade,
  tier: args.tier ?? assessment?.selected.tier ?? template.tier,
  deliberation: args.deliberation ?? assessment?.selected.reasoning ?? template.deliberation,
  topology: preset ? preset.topology : (args.topology ?? template.topology),
  posture: args.posture ?? template.posture ?? catalog.defaults.posture,
};
for (const [field, axis] of [["taskGrade", "taskGrades"], ["tier", "semanticTiers"], ["deliberation", "deliberations"], ["topology", "topologies"], ["posture", "postures"]])
  if (!catalog.vocabulary[axis].includes(selected[field])) die(`invalid ${field}: ${selected[field]}`);

const payload = {
  role: canonicalRole,
  taskGrade: selected.taskGrade,
  domainRequirements: [...new Set(args.domains)],
  topology: selected.topology,
  tier: selected.tier,
  posture: selected.posture,
  reasoning: selected.deliberation,
  composition: preset
    ? { kind: "preset", id: canonicalRole, overrides: [] }
    : {
        kind: "bespoke", id: args.role,
        ...(nearest ? { nearestPreset: nearest.name } : {}),
        bespokeReason: args.rationale.trim(),
        promotionCandidate: args.promotionCandidate,
        contract: parseContract(args.contract),
      },
};

if (preset) {
  const overrides = presetOverrides(payload, preset, catalog);
  payload.composition.overrides = overrides;
  if (overrides.length && !args.overrideReason?.trim())
    die(`stock-template axis override requires --override-reason (changed: ${overrides.join(", ")})`);
  if (!overrides.length && args.overrideReason)
    die("unchanged stock template must not carry --override-reason");
  if (overrides.length) payload.composition.overrideReason = args.overrideReason.trim();
}

try { validateRoutingRequest(payload, catalog); }
catch (error) { die(error.message); }
if (assessment) {
  try { assertAssessmentSelection(assessment, payload.tier, payload.reasoning); }
  catch (error) { die(error.message); }
} else if (payload.reasoning === "max") {
  die("max reasoning requires --assessment with reasoningShape exceptional and exceptionalDeliberation");
}

console.log(JSON.stringify(payload));
