import { setDefaultTimeout, afterAll, beforeAll, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

setDefaultTimeout(30_000);

import { parseConfig, thresholdsOf } from "./config";

const CLI = join(import.meta.dir, "cli.ts");
let root: string;
let repo: string;
const env: Record<string, string | undefined> = { ...process.env, GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t" };

function sh(args: string[], cwd = repo) {
  const r = Bun.spawnSync(args, { cwd, env, stdout: "pipe", stderr: "pipe" });
  return { code: r.exitCode, out: r.stdout.toString(), err: r.stderr.toString() };
}
function write(files: Record<string, string>) {
  for (const [p, text] of Object.entries(files)) {
    mkdirSync(dirname(join(repo, p)), { recursive: true });
    writeFileSync(join(repo, p), text);
  }
}
function commit(msg: string) {
  sh(["git", "add", "-A"]);
  sh(["git", "commit", "-qm", msg]);
  return sh(["git", "rev-parse", "HEAD"]).out.trim();
}
const fn = (name: string, n: number) => `export function ${name}() {\n${Array.from({ length: n }, (_, i) => `  const v${i} = ${i};`).join("\n")}\n}\n`;
const filler = (n: number) => Array.from({ length: n }, (_, i) => `export const c${i} = ${i};`).join("\n") + "\n";

const STRUCTURE = `[thresholds]
file_lines = 200
function_lines = 80
function_complexity = 15
hotspot_top = 1
[[module]]
name = "a"
paths = ["src/a/**"]
may_use = ["b"]
[[module]]
name = "b"
paths = ["src/b/**"]
[[module]]
name = "c"
paths = ["src/c/**"]
`;
const BASE_FILES = {
  "structure.toml": STRUCTURE,
  "src/a/main.ts": `import { y } from "../b/y";\nexport const m = y;\n`,
  "src/a/legacy.ts": fn("legacy", 100),
  "src/a/grow.ts": filler(190),
  "src/b/x.ts": `import { y } from "./y";\nexport const x = y;\n`,
  "src/b/y.ts": `export const y = 1;\n`,
  "src/c/z.ts": `export const z = 1;\n`,
};
const DEBT = {
  "src/b/y.ts": `import { x } from "./x";\nexport const y = 1;\nexport const yx = () => x;\n`,
  "src/b/x.ts": `import { y } from "./y";\nimport { z } from "../c/z";\nexport const x = y + z;\n`,
  "src/a/big.ts": fn("big", 100),
  "src/a/grow.ts": filler(230),
};
const LEGACY_TOUCHED = { "src/a/legacy.ts": fn("legacy", 110) };
let base: string, debt: string, touched: string;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "code-health-test-"));
  repo = join(root, "repo");
  mkdirSync(repo);
  env.XDG_STATE_HOME = join(root, "state");
  sh(["git", "init", "-q", "-b", "main"]);
  write(BASE_FILES);
  base = commit("base");
  write(DEBT);
  debt = commit("debt");
  sh(["git", "checkout", "-q", "-b", "legacy", base]);
  write(LEGACY_TOUCHED);
  touched = commit("touch legacy");
  sh(["git", "checkout", "-q", base]);
});
afterAll(() => rmSync(root, { recursive: true, force: true }));

const rulesOf = (out: string) => out.trim().split("\n").filter(Boolean).map((l) => l.split(" ").slice(0, 2).join(" ")).sort();
const EXPECTED = ["cycle src/b/y.ts", "file-size src/a/grow.ts", "function src/a/big.ts:1", "layer src/b/x.ts"];

test("gate names each new debt once and passes untouched legacy debt", () => {
  const r = sh(["bun", CLI, "gate", base, debt]);
  expect(r.err).toBe("");
  expect(r.code).toBe(1);
  expect(rulesOf(r.out)).toEqual(EXPECTED);
  const ok = sh(["bun", CLI, "gate", base, touched]);
  expect([ok.code, ok.out]).toEqual([0, ""]);
});

test("check names the same debt for dirty worktree files", () => {
  write(DEBT);
  try {
    const lines = Object.keys(DEBT).flatMap((p) => {
      const r = sh(["bun", CLI, "check", p]);
      expect(r.err).toBe("");
      return r.out.trim() ? r.out.trim().split("\n") : [];
    });
    expect(rulesOf(lines.join("\n"))).toEqual(EXPECTED);
    expect(sh(["bun", CLI, "check", "src/a/main.ts"]).out).toBe("");
  } finally {
    sh(["git", "checkout", "-q", "--", "."]);
    rmSync(join(repo, "src/a/big.ts"), { force: true });
  }
});

test("module limits override only named defaults in gate and edit feedback", () => {
  const text = STRUCTURE.replace('name = "a"', 'name = "a"\nthresholds = { file_lines = 300, function_lines = 120 }');
  const cfg = parseConfig(text);
  expect(thresholdsOf(cfg, "src/a/big.ts")).toEqual({ file_lines: 300, function_lines: 120, function_complexity: 15, hotspot_top: 1 });
  expect(thresholdsOf(cfg, "src/b/x.ts")).toEqual(cfg.thresholds);
  expect(thresholdsOf(cfg, "unknown.ts")).toEqual(cfg.thresholds);
  const map = join(root, "override.toml");
  writeFileSync(map, text);
  const r = sh(["bun", CLI, "gate", "--config", map, base, debt]);
  expect([r.code, r.err]).toEqual([1, ""]);
  expect(rulesOf(r.out)).toEqual(["cycle src/b/y.ts", "layer src/b/x.ts"]);
  write(DEBT);
  try {
    for (const path of ["src/a/big.ts", "src/a/grow.ts"]) {
      const checked = sh(["bun", CLI, "check", "--config", map, path]);
      expect([checked.code, checked.out, checked.err]).toEqual([0, "", ""]);
    }
  } finally {
    sh(["git", "checkout", "-q", "--", "."]);
    rmSync(join(repo, "src/a/big.ts"), { force: true });
  }
});
