import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  CATALOG_SCHEMA_ID,
  PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
  schemaPath,
} from "../index.mjs";

test("catalog assets and contracts bind stable versioned IDs to shipped schemas", () => {
  const expected = [
    CATALOG_SCHEMA_ID,
    PROJECT_EXPOSURE_PROFILE_SCHEMA_ID,
  ];
  for (const id of expected) {
    const schema = JSON.parse(readFileSync(schemaPath(id), "utf8"));
    assert.equal(schema.$id, id);
  }
});
