import { setDefaultTimeout, expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

setDefaultTimeout(30_000);

const CLI = join(import.meta.dir, "cli.ts");

test("a repo without structure.toml is not enrolled and stays quiet", () => {
  const root = mkdtempSync(join(tmpdir(), "code-health-signal-"));
  const repo = join(root, "repo");
  const env = { ...process.env, XDG_STATE_HOME: join(root, "state"), GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t" };
  const sh = (args: string[]) => Bun.spawnSync(args, { cwd: repo, env, stdout: "pipe", stderr: "pipe" });
  Bun.spawnSync(["git", "init", "-q", repo]);
  writeFileSync(join(repo, "a.ts"), "export const a = 1;\n");
  sh(["git", "add", "a.ts"]);
  sh(["git", "commit", "-qm", "a"]);
  const quiet = sh(["bun", CLI, "signal", "--repo", repo]);
  expect(quiet.stdout.toString().trim()).toBe("quiet");
  expect(quiet.exitCode).toBe(1);
  writeFileSync(join(repo, "structure.toml"), "[thresholds]\nfile_lines = 500\n");
  sh(["git", "add", "structure.toml"]);
  sh(["git", "commit", "-qm", "map"]);
  const fired = sh(["bun", CLI, "signal", "--repo", repo]);
  expect(fired.stdout.toString()).toStartWith("FIRE no review recorded");
  expect(fired.exitCode).toBe(0);
});

test("signal judges the named revision, not a stale checkout", () => {
  const root = mkdtempSync(join(tmpdir(), "code-health-signal-rev-"));
  const repo = join(root, "repo");
  const env = { ...process.env, XDG_STATE_HOME: join(root, "state"), GIT_AUTHOR_NAME: "t", GIT_AUTHOR_EMAIL: "t@t", GIT_COMMITTER_NAME: "t", GIT_COMMITTER_EMAIL: "t@t" };
  const sh = (args: string[]) => Bun.spawnSync(args, { cwd: repo, env, stdout: "pipe", stderr: "pipe" });
  Bun.spawnSync(["git", "init", "-q", repo]);
  writeFileSync(join(repo, "a.ts"), "export const a = 1;\n");
  sh(["git", "add", "a.ts"]);
  sh(["git", "commit", "-qm", "a"]);
  sh(["git", "checkout", "-qb", "ahead"]);
  writeFileSync(join(repo, "structure.toml"), "[thresholds]\nfile_lines = 500\n");
  sh(["git", "add", "structure.toml"]);
  sh(["git", "commit", "-qm", "map"]);
  sh(["git", "checkout", "-q", "-"]);
  expect(sh(["bun", CLI, "signal", "--repo", repo]).stdout.toString().trim()).toBe("quiet");
  const fired = sh(["bun", CLI, "signal", "--repo", repo, "--rev", "ahead"]);
  expect(fired.stdout.toString()).toStartWith("FIRE no review recorded");
  expect(fired.exitCode).toBe(0);
});
