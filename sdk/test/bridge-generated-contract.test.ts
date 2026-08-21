import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const generated = join(import.meta.dir, "../src/bridge/generated");
const app = readFileSync(join(generated, "north/bridge/app.js"), "utf8");
const model = readFileSync(join(generated, "north/bridge/model.js"), "utf8");

test("generated bridge uses valid JavaScript member access", () => {
  expect(app).not.toContain("Math/max");
  expect(app).toContain("Math.max(");
});

test("generated bridge resolves only the vendored Beagle runtime", () => {
  for (const output of [app, model]) {
    expect(output).not.toMatch(/from ["']beagle\//);
    expect(output).toContain("from '../../beagle/core.js'");
  }
});
