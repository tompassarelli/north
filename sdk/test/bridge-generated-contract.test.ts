import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const generated = join(import.meta.dir, "../src/bridge/generated");
const app = readFileSync(join(generated, "north/bridge/app.js"), "utf8");
const model = readFileSync(join(generated, "north/bridge/model.js"), "utf8");
const modelDeclarations = readFileSync(
  join(generated, "north/bridge/model.d.ts"), "utf8",
);
const referentActions = readFileSync(
  join(generated, "north/bridge/referent-actions.js"), "utf8",
);
const referentActionDeclarations = readFileSync(
  join(generated, "north/bridge/referent-actions.d.ts"), "utf8",
);

test("generated bridge uses valid JavaScript member access", () => {
  expect(app).not.toContain("Math/max");
  expect(app).toContain("Math.max(");
});

test("generated bridge resolves only the vendored Beagle runtime", () => {
  for (const output of [app, model, referentActions]) {
    expect(output).not.toMatch(/from ["']beagle\//);
    expect(output).toContain("from '../../beagle/core.js'");
  }
});

test("canonical generation owns semantic model and action declarations", () => {
  expect(modelDeclarations).toContain('as "->ExecutionItem"');
  expect(modelDeclarations).toContain('as "->TrackedThing"');
  expect(modelDeclarations).not.toContain("WorkItem");
  expect(model).not.toContain("WorkItem");
  expect(modelDeclarations).toContain('as "replace-catalog"');
  expect(referentActionDeclarations).toContain('as "referent-action-argv!"');
  expect(referentActionDeclarations).toContain('as "validate-semantic-catalog!"');
  expect(referentActions).toContain("north.semantic-catalog");
});
