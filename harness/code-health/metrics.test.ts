import { expect, setDefaultTimeout, test } from "bun:test";
import { measure } from "./functions";
import { importsOf } from "./imports";

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

test("use super inside an inline mod resolves to the file, and a super outside reaches the parent module", () => {
  const files = new Set(["Cargo.toml", "src/lib.rs", "src/link.rs", "src/util.rs"]);
  const resolver = { exists: (p: string) => files.has(p), crates: [{ name: "app", dir: "" }], all: () => files };
  const inlineOnly = "pub fn go() {}\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n";
  expect([...importsOf("src/link.rs", inlineOnly, resolver)]).toEqual([]);
  const reachesParent = `${inlineOnly}use super::util::helper;\n`;
  expect([...importsOf("src/link.rs", reachesParent, resolver)]).toEqual(["src/util.rs"]);
  const parentRoot = "use super::Shared;\nmod tests {\n    use super::super::Shared;\n}\n";
  expect([...importsOf("src/link.rs", parentRoot, resolver)]).toEqual(["src/lib.rs"]);
});
