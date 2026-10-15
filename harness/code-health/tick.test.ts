import { setDefaultTimeout, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { tickRepo, type TickEnv } from "./tick";

setDefaultTimeout(30_000);

function scratch(signal: string) {
  const root = mkdtempSync(join(tmpdir(), "code-health-tick-"));
  mkdirSync(join(root, "code/smashcraft/main"), { recursive: true });
  Bun.spawnSync(["git", "init", "-q", join(root, "code/smashcraft/main")]);
  const fake = join(root, "code-health");
  writeFileSync(fake, `#!/usr/bin/env bash\necho "$*" >> ${root}/calls\ncase $1 in\n  signal) ${signal} ;;\n  report) echo "hotspots: sim/physics.ts 412" ;;\nesac\n`);
  chmodSync(fake, 0o755);
  const env: TickEnv = {
    codeRoot: join(root, "code"), stateRoot: join(root, "state"), handoffs: join(root, "handoffs"),
    orgFile: join(root, "org.json"), codeHealth: [fake], agents: "agents",
    template: join(import.meta.dir, "review-brief.md"), dryRun: true, now: () => new Date(),
  };
  return { root, env };
}

test("FIRE starts exactly one review launch and a held lock starts none", () => {
  const { root, env } = scratch(`echo "FIRE commits 104 >= 100"`);
  const lines: string[] = [];
  const first = tickRepo("smashcraft", env, (line) => lines.push(line));
  expect(first.result).toBe("started");
  expect(lines).toHaveLength(1);
  expect(lines[0]).toStartWith("dry-run: agents lead start --provider claude --domain smashcraft --name code-health-review-smashcraft --role lead --budget 1 --brief ");
  const brief = readFileSync(join(env.handoffs, "code-health-review-smashcraft-brief.md"), "utf8");
  expect(brief).toStartWith("Delegation: role=lead budget=1\n");
  expect(brief).toContain("- FIRE commits 104 >= 100");
  expect(brief).not.toContain("{{");
  expect(existsSync(join(env.stateRoot, "smashcraft/review.lock"))).toBe(true);

  const second = tickRepo("smashcraft", env, (line) => lines.push(line));
  expect(second.result).toBe("held");
  expect(lines).toHaveLength(1);
  expect(readFileSync(join(root, "calls"), "utf8").match(/^signal /gm)).toHaveLength(1);
});

test("quiet releases the lock and starts nothing", () => {
  const { env } = scratch("echo quiet; exit 1");
  const lines: string[] = [];
  expect(tickRepo("smashcraft", env, (line) => lines.push(line)).result).toBe("quiet");
  expect(lines).toHaveLength(0);
  expect(existsSync(join(env.stateRoot, "smashcraft/review.lock"))).toBe(false);
});

test("a stale lock with no live review node is retaken", () => {
  const { env } = scratch(`echo "FIRE days 7"`);
  mkdirSync(join(env.stateRoot, "smashcraft"), { recursive: true });
  writeFileSync(join(env.stateRoot, "smashcraft/review.lock"), "{}\n");
  const later = { ...env, now: () => new Date(Date.now() + 2 * 60 * 60 * 1000) };
  expect(tickRepo("smashcraft", later, () => {}).result).toBe("started");
  writeFileSync(env.orgFile, JSON.stringify({ nodes: [{ id: "code-health-review-smashcraft" }] }));
  expect(tickRepo("smashcraft", later, () => {}).result).toBe("held");
});
