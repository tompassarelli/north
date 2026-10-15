import { expect, setDefaultTimeout, test } from "bun:test";
import { measure } from "./functions";

setDefaultTimeout(30_000);

test("boolean runs count once while mixed operators and separate expressions stay distinct", () => {
  const cases = [
    ...["&&", "||", "??"].flatMap((op) => Array.from({ length: 7 }, (_, n) => ({
      text: `export function run() { return ${Array.from({ length: n + 2 }, (_, i) => `v${i}`).join(` ${op} `)}; }`,
      complexity: 2,
    }))),
    { text: "function run() { return a && (b && c); }", complexity: 2 },
    { text: "function run() { return a && b || c && d; }", complexity: 4 },
    { text: "function run() { const x = a ?? b; return c ?? d; }", complexity: 3 },
    { text: "function run() { return a && (b || c) && d; }", complexity: 3 },
  ];
  const files = cases.map((c, i) => ({ path: `${i}.ts`, text: c.text }));
  const measured = measure({ h: files });
  for (const [i, c] of cases.entries()) expect(measured.get(`h/${i}.ts`)?.map((f) => f.complexity)).toEqual([c.complexity]);
});
