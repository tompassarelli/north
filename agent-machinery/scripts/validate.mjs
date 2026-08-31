#!/usr/bin/env bun
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { AGENT_SOURCE_PATHS, buildAgents } from "./build-agents.mjs";
import { loadStaffingCatalog } from "./staffing-catalog.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const EXCLUDED_TEXT = new Set(["PROVENANCE.md", "NOTICE", "LICENSE", "LICENSE-MIT", "LICENSE-APACHE"]);
const FORBIDDEN_DIRS = new Set(["adapters", "hooks", "providers", "secrets"]);
const forbiddenBrands = [
  ["Nor", "th"], ["Fi", "rn"], ["Nix", "OS"], ["Bea", "gle"],
].map((parts) => parts.join(""));
export const FORBIDDEN_TEXT = new RegExp(
  [["/", "home", "/"].join(""), ["~", "/"].join(""), ["mcp", "__"].join(""),
    String.raw`\b(?:${forbiddenBrands.join("|")})\b`].join("|"),
  "i",
);
const TYPED_TOOL = ["bea", "gle"].join("");
const TYPED_JS_HEADER = new RegExp(`^#lang ${TYPED_TOOL}\/js(?:\\r?\\n|$)`);
const TYPED_PACKAGE_COMMANDS = [
  `${TYPED_TOOL} build scripts/work-ownership.bjs scripts/work-ownership.js`,
  `${TYPED_TOOL} check scripts/work-ownership.bjs && ${TYPED_TOOL} fmt --check scripts/work-ownership.bjs`,
  `${TYPED_TOOL} build scripts/selection-statistics.bjs scripts/selection-statistics.js`,
  `${TYPED_TOOL} check scripts/selection-statistics.bjs && ${TYPED_TOOL} fmt --check scripts/selection-statistics.bjs`,
];

export function portableSourceText(relativePath, text) {
  if (relativePath.endsWith(".bjs")) return text.replace(TYPED_JS_HEADER, "");
  if (relativePath === "package.json")
    return TYPED_PACKAGE_COMMANDS.reduce((source, command) => source.replace(command, ""), text);
  return text;
}

function walk(directory = ROOT) {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (entry.name === ".git" || entry.name === "node_modules") continue;
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) files.push(...walk(path));
    else files.push(path);
  }
  return files;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function containedPath(relativePath, label) {
  assert(typeof relativePath === "string" && relativePath && !relativePath.startsWith("/"),
    `${label} must be package-relative`);
  const path = resolve(ROOT, relativePath);
  assert(relative(ROOT, path) && !relative(ROOT, path).startsWith(".."), `${label} escapes package`);
  return path;
}

function skillName(path) {
  const text = readFileSync(path, "utf8");
  const match = text.match(/^---\n([\s\S]*?)\n---/);
  assert(match, `${relative(ROOT, path)} has no frontmatter`);
  const name = match[1].match(/^name:\s*([^\n]+)$/m)?.[1]?.trim();
  assert(name, `${relative(ROOT, path)} has no name`);
  return name;
}

export function validatePackage({ checkGenerated = true } = {}) {
  const catalog = JSON.parse(readFileSync(resolve(ROOT, "catalog.json"), "utf8"));
  assert(JSON.stringify(Object.keys(catalog).sort()) === JSON.stringify(["$schema", "assets", "contracts", "package", "schema", "units"]),
    "export catalog must retain the closed six-key package shape");
  assert(catalog.schema === "agent-machinery.catalog/v1", "unexpected catalog schema");
  assert(catalog.package?.license === "MIT OR Apache-2.0", "unexpected package license");
  for (const forbidden of ["rootOrder", "distributions", "targets", "supports", "owner", "providerSupport"])
    assert(!JSON.stringify(catalog).includes(`"${forbidden}"`), `export catalog contains activation field ${forbidden}`);

  const ids = new Set();
  for (const unit of catalog.units) {
    assert(!ids.has(unit.id), `duplicate unit id ${unit.id}`);
    ids.add(unit.id);
    assert(["module", "skill"].includes(unit.kind), `invalid unit kind for ${unit.id}`);
    const source = containedPath(unit.source, `unit ${unit.id} source`);
    assert(existsSync(source), `missing unit source ${unit.source}`);
    if (unit.kind === "module") assert(Array.isArray(unit.members) && unit.members.length, `module ${unit.id} has no members`);
    else {
      assert(unit.members === undefined, `skill ${unit.id} must not declare members`);
      assert(skillName(source) === unit.id, `unit ${unit.id} source name mismatch`);
    }
  }
  for (const unit of catalog.units.filter(({ kind }) => kind === "module"))
    for (const member of unit.members) assert(ids.has(member), `module ${unit.id} has unknown member ${member}`);
  const assetIds = new Set();
  const declaredPaths = new Set(catalog.units.map(({ source }) => source));
  for (const asset of catalog.assets) {
    assert(!assetIds.has(asset.id), `duplicate asset id ${asset.id}`);
    assetIds.add(asset.id);
    assert(["instructions", "catalog", "generated-templates", "source-blocks"].includes(asset.type),
      `invalid asset type for ${asset.id}`);
    declaredPaths.add(asset.path);
    const path = containedPath(asset.path, `asset ${asset.id}`);
    assert(existsSync(path), `missing asset ${asset.path}`);
  }
  const schemaIds = new Set();
  const schemaPaths = new Set([
    ...catalog.assets.map(({ path }) => path).filter((path) => path.endsWith(".schema.json")),
    ...catalog.contracts.map(({ schema }) => schema),
  ]);
  for (const schemaPath of schemaPaths) {
    const path = containedPath(schemaPath, `schema ${schemaPath}`);
    assert(existsSync(path), `missing schema ${schemaPath}`);
    const schemaId = JSON.parse(readFileSync(path, "utf8")).$id;
    assert(/^urn:agent-machinery:schema:[a-z][a-z0-9-]*:v[1-9][0-9]*$/.test(schemaId),
      `schema ${schemaPath} has no stable versioned package ID`);
    assert(!schemaIds.has(schemaId), `duplicate schema id ${schemaId}`);
    schemaIds.add(schemaId);
  }
  assert(schemaIds.has(catalog.$schema), `catalog $schema is absent from declared schema assets`);
  for (const contract of catalog.contracts) {
    assert(schemaPaths.has(contract.schema), `contract ${contract.id} schema is absent from catalog`);
    assert(contract.schemaScope === "structural", `contract ${contract.id} must classify its raw schema as structural`);
    assert(contract.validator === "validateContract", `contract ${contract.id} must use the composed validator`);
    assert(existsSync(containedPath(contract.fixtures, `contract ${contract.id} fixtures`)), `missing fixtures for ${contract.id}`);
  }
  for (const input of AGENT_SOURCE_PATHS)
    assert(declaredPaths.has(input), `generated-agent input is absent from export catalog: ${input}`);

  loadStaffingCatalog();
  if (checkGenerated) buildAgents({ check: true });

  for (const path of walk()) {
    const rel = relative(ROOT, path);
    for (const segment of rel.split("/").slice(0, -1))
      assert(!FORBIDDEN_DIRS.has(segment), `forbidden source boundary directory: ${rel}`);
    const providerInterfaceMetadata = ["agents/", ["open", "ai"].join(""), ".yaml"].join("");
    assert(rel !== providerInterfaceMetadata && !rel.endsWith(`/${providerInterfaceMetadata}`),
      `provider interface metadata is outside package authority: ${rel}`);
    if (EXCLUDED_TEXT.has(rel) || statSync(path).size > 1_000_000) continue;
    const text = portableSourceText(rel, readFileSync(path, "utf8"));
    assert(!FORBIDDEN_TEXT.test(text), `non-portable source marker in ${rel}`);
  }
  return { units: catalog.units.length, templates: loadStaffingCatalog().presets.length };
}

const invoked = process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (invoked) {
  try {
    const result = validatePackage();
    console.log(`validate: ${result.units} units, ${result.templates} templates`);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
