#!/usr/bin/env node
// Assembles a stock-template lane's behavioral payload (the block stack
// build-agents.mjs compiles into plugin agents) for custom dispatch surfaces:
// compose-payload.mjs <role> --provider <name> [--model|--reasoning|--tier ...]
// [--steering light|moderate|strong]
// [--conformance advisory|preferred|required] [--task <file|->] [--no-family].
// Payload → stdout, resolution → stderr.
import { readFileSync, existsSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { loadStaffingCatalog } from "./staffing-catalog.mjs";
import { loadProviderCatalog, resolveModelAlias, resolvePinnedModelRoute, modelDeltaFor, PROVIDER_NAMES } from "./provider-catalog.mjs";
import { canonicalRoleId } from "./role-id.mjs";
import { block, firstFence } from "./blocks.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(resolve(ROOT, p), "utf8");

const FAMILY_BLOCKS = { openai: "docs/deltas/openai-common.md" };
const STRONG_VERIFICATION_BLOCK = "docs/artifacts/verification-loop-strong.md";

export const CONFORMANCE_HEADERS = {
  advisory: "CONFORMANCE: advisory — the blocks below are calibrated defaults distilled from observed failures; weigh them with your own judgment, and note each deviation in one logged line.",
  preferred: "CONFORMANCE: preferred — the blocks below are the operating defaults; deviate only with a logged one-line reason.",
  required: "CONFORMANCE: required — the blocks below are binding requirements; follow them exactly.",
};

function parseArgs(argv) {
  const args = { flags: {} };
  const positional = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--no-family") { args.flags.noFamily = true; continue; }
    if (a === "--steering") {
      const value = argv[++i];
      if (!["light", "moderate", "strong"].includes(value))
        throw new Error("--steering must be light, moderate, or strong");
      args.flags.steering = value;
      continue;
    }
    if (a === "--conformance") {
      const value = argv[++i];
      if (!["advisory", "preferred", "required"].includes(value))
        throw new Error("--conformance must be advisory, preferred, or required");
      args.flags.conformance = value;
      continue;
    }
    if (a.startsWith("--")) {
      const key = a.slice(2);
      const value = argv[++i];
      if (value === undefined) throw new Error(`missing value for --${key}`);
      args.flags[key] = value;
      continue;
    }
    positional.push(a);
  }
  if (positional.length !== 1) throw new Error("usage: compose-payload.mjs <role> --provider <name> [options]");
  args.role = canonicalRoleId(positional[0]);
  return args;
}

function main() {
  const { role, flags } = parseArgs(process.argv.slice(2));
  const provider = flags.provider;
  if (!PROVIDER_NAMES.includes(provider))
    throw new Error(`--provider must be one of: ${PROVIDER_NAMES.join(", ")}`);

  const staffing = loadStaffingCatalog();
  const preset = staffing.presets.find((p) => p.name === role);
  if (!preset)
    throw new Error(`role '${role}' is not a stock template; bespoke compositions use the compose skill`);

  const tier = flags.tier ?? preset.tier;
  if (!staffing.vocabulary.semanticTiers.includes(tier))
    throw new Error(`unknown tier: ${tier}`);
  const catalog = loadProviderCatalog(provider);

  let resolved;
  if (flags.model) {
    const reasoning = flags.reasoning ?? preset.deliberation;
    resolved = resolvePinnedModelRoute(catalog, { model: flags.model, tier, reasoning });
  } else {
    const row = catalog.tiers[tier];
    const levels = row.efforts ?? row.reasoning;
    const reasoning = flags.reasoning ?? row.defaultEffort ?? row.defaultReasoning;
    if (!levels.includes(reasoning))
      throw new Error(`${provider}/${tier} does not support deliberation ${reasoning} (supported: ${levels.join(", ")})`);
    resolved = { provider, model: resolveModelAlias(catalog, row.model), tier, reasoning };
  }

  const overrides = [];
  if (flags.tier && flags.tier !== preset.tier) overrides.push("tier");
  if (flags.reasoning && flags.reasoning !== preset.deliberation) overrides.push("reasoning");
  const routing = {
    role, taskGrade: preset.taskGrade, domainRequirements: [],
    topology: preset.topology, tier, reasoning: resolved.reasoning,
    posture: preset.posture,
    composition: { kind: "preset", id: role, overrides },
  };

  const parts = [
    `<!-- ORCHESTRATION_ROUTING ${JSON.stringify(routing)} -->`,
    `<!-- RESOLVED provider=${resolved.provider} model=${resolved.model} reasoning=${resolved.reasoning} — composed by scripts/compose-payload.mjs -->`,
    "",
    `You are the ${preset.name}: ${preset.tagline}.`,
    "",
    "## Role",
    block(read("docs/roles.md"), role),
    "",
    `## Task grade: ${preset.taskGrade}`,
    block(read("docs/task-grades.md"), preset.taskGrade),
    "",
    `## Topology: ${preset.topology}`,
    block(read("docs/topologies.md"), preset.topology),
    "",
    `## Posture: ${preset.posture}`,
    block(read("docs/postures.md"), preset.posture),
    "",
    "## Output norms",
    block(read("docs/comms.md"), "universal"),
  ];

  const steering = flags.steering ?? "moderate";

  // Conformance modulates binding register only; content is identical. The
  // 2026-07-31 A/B held all three targeted safety behaviors under advisory
  // and shortened output — see docs/openai-steering.md, retest results.
  const conformance = flags.conformance ?? "required";
  const conformanceHeader = CONFORMANCE_HEADERS[conformance];
  parts.push("", conformanceHeader);

  const familyPath = FAMILY_BLOCKS[provider];
  if (familyPath && !flags.noFamily) {
    if (!existsSync(resolve(ROOT, familyPath)))
      throw new Error(`provider family block missing: ${familyPath}`);
    parts.push("", `## Provider family protocol (${provider})`, firstFence(read(familyPath)));
  }

  if (steering !== "light") {
    const delta = modelDeltaFor(catalog, resolved.model);
    if (delta.kind === "calibrated")
      parts.push("", "## Delta protocol — tuned to this model's documented tendencies", firstFence(read(delta.path)));
    else
      parts.push("", `<!-- model delta: explicit none for ${resolved.model} — ${delta.reason} -->`);
  }

  if (steering === "strong")
    parts.push("", "## Strong verification-loop protocol", firstFence(read(STRONG_VERIFICATION_BLOCK)));

  if (flags.task) {
    const task = flags.task === "-" ? readFileSync(0, "utf8") : readFileSync(resolve(process.cwd(), flags.task), "utf8");
    parts.push("", "## TASK", "", task.trimEnd());
  }

  const payload = parts.join("\n") + "\n";
  process.stdout.write(payload);

  const caps = preset.capabilities;
  const sandbox = caps.includes("filesystem.write") || caps.includes("shell") ? "workspace-write" : "read-only";
  const behavioral = payload.split("\n").length - (flags.task ? flags.task.length : 0);
  process.stderr.write(
    `resolved: ${resolved.provider}/${resolved.model} tier=${tier} reasoning=${resolved.reasoning} steering=${steering} ` +
    `topology=${preset.topology} sandbox=${sandbox}\n` +
    (provider === "openai"
      ? `invoke: codex exec -m ${resolved.model} -c model_reasoning_effort='"${resolved.reasoning}"' -s ${sandbox} --ephemeral -C <workdir> -o <result-file> - < <payload-file>\n`
      : "") +
    `payload lines: ${payload.split("\n").length}\n`,
  );
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
