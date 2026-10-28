import { setDefaultTimeout, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
setDefaultTimeout(30_000);
const CLI = join(import.meta.dir, "cli.ts"), MAP = { "structure.toml": "[thresholds]\nfile_lines = 500\n" };
function scratch(first: Record<string, string> = {}, review = "") {
  const repo = join(mkdtempSync(join(tmpdir(), "code-health-signal-")), "repo");
  const env = { ...process.env, XDG_STATE_HOME: join(repo, "../state"), GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t" };
  const sh = (args: string[]) => Bun.spawnSync(args, { cwd: repo, env, stdout: "pipe", stderr: "pipe" });
  const land = (files: Record<string, string>) => {
    for (const [path, text] of Object.entries(files)) { mkdirSync(dirname(join(repo, path)), { recursive: true }); writeFileSync(join(repo, path), text); }
    sh(["git", "add", "."]); sh(["git", "commit", "-qm", "land"]);
  };
  const signal = (...rev: string[]) => sh(["bun", CLI, "signal", "--repo", repo, ...rev]);
  Bun.spawnSync(["git", "init", "-q", repo]); land({ "a.ts": "export const a = 1;\n", ...first });
  if (review) sh(["bun", CLI, "review-done", "--repo", repo]);
  return { sh, land, signal, fire: () => signal().stdout.toString().trim() };
}
test("a repo without structure.toml is not enrolled and stays quiet", () => {
  const { land, signal } = scratch();
  const quiet = signal();
  expect(quiet.stdout.toString().trim()).toBe("quiet"); expect(quiet.exitCode).toBe(1);
  land(MAP);
  const fired = signal();
  expect(fired.stdout.toString()).toStartWith("FIRE no review recorded"); expect(fired.exitCode).toBe(0);
});
test("signal judges the named revision, not a stale checkout", () => {
  const { sh, land, signal, fire } = scratch();
  sh(["git", "checkout", "-qb", "ahead"]); land(MAP); sh(["git", "checkout", "-q", "-"]);
  expect(fire()).toBe("quiet");
  const fired = signal("--rev", "ahead");
  expect(fired.stdout.toString()).toStartWith("FIRE no review recorded"); expect(fired.exitCode).toBe(0);
});
test("the commit count fires at its limit, days before the 7-day signal could", () => {
  const { land, fire } = scratch({ "structure.toml": "[review]\ncommits = 3\nhand_lines_added = 0\ntest_lines_changed = 0\ndays = 7\n" }, "yes");
  land({ "b.ts": "export const b = 1;\n" }); land({ "c.ts": "export const c = 1;\n" });
  expect(fire()).toBe("quiet");
  land({ "d.ts": "export const d = 1;\n" });
  expect(fire()).toStartWith("FIRE 3 commits since review"); expect(fire()).not.toContain("days since review");
});
test("test-file churn counts lines added plus removed under test paths and fires at its limit", () => {
  const { land, fire } = scratch({ "structure.toml": "[review]\ncommits = 0\nhand_lines_added = 0\ntest_lines_changed = 10\ndays = 0\n", "tests/old.test.ts": "o1\no2\no3\no4\no5\n" }, "yes");
  land({ "tests/old.test.ts": "n1\nn2\nn3\n", "src.ts": "x\n".repeat(50) });
  expect(fire()).toBe("quiet");
  land({ "tests/new.test.ts": "1\n2\n3\n4\n" });
  expect(fire()).toBe("FIRE 12 test lines changed since review (limit 10)");
});
