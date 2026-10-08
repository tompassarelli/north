import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  CATALOG_SCHEMA_ID,
  PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
  WORK_OWNERSHIP_SCHEMA_ID,
  loadExportCatalog,
  schemaPath,
} from "../index.mjs";

test("catalog assets and contracts bind stable versioned IDs to shipped schemas", () => {
  const catalog = loadExportCatalog();
  const expected = [
    CATALOG_SCHEMA_ID,
    PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
    WORK_OWNERSHIP_SCHEMA_ID,
  ];
  assert.deepEqual(Object.keys(catalog).sort(), ["$schema", "assets", "contracts", "package", "schema", "units"]);
  assert.deepEqual(
    [...new Set(catalog.assets.map(({ type }) => type))].sort(),
    ["catalog", "instructions"],
  );
  assert.deepEqual(
    catalog.units.filter(({ kind }) => kind === "module").map(({ id, members }) => [id, members]),
    [
      ["agent-machinery", ["delegation", "agent-practice"]],
      ["delegation", ["work-ownership"]],
      ["agent-practice", ["babashka-development", "build-vs-reuse", "ceremony-budget", "competitive-development-loop", "debugging", "effect-development", "external-code", "greenfield", "planning", "prior-art", "program-craftsmanship", "program-stewardship", "rust-development", "skill-maintenance", "terse", "verification"]],
    ],
  );
  for (const id of expected) {
    const schema = JSON.parse(readFileSync(schemaPath(id), "utf8"));
    assert.equal(schema.$id, id);
  }
});
