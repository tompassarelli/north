import { setDefaultTimeout, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { reviewName, tickRepo, type TickEnv } from "./tick";

setDefaultTimeout(30_000);

function scratch(signal: string) {
  const root = mkdtempSync(join(tmpdir(), "code-health-tick-")), start = new Date();
  mkdirSync(join(root, "code/smashcraft/main"), { recursive: true });
  Bun.spawnSync(["git", "init", "-q", join(root, "code/smashcraft/main")]); Bun.spawnSync(["git", "init", "-q", "--bare", join(root, "origin.git")]);
  Bun.spawnSync(["git", "-C", join(root, "code/smashcraft/main"), "remote", "add", "origin", join(root, "origin.git")]);
  const fake = join(root, "code-health");
  writeFileSync(fake, `#!/usr/bin/env bash\necho "$*" >> ${root}/calls\ncase $1 in\n  signal) ${signal} ;;\n  report) echo "hotspots: sim/physics.ts 412" ;;\nesac\n`); chmodSync(fake, 0o755);
  const fleet = join(root, "north"), fleetSessions = join(root, "fleet.json"); writeFileSync(fleetSessions, "[]\n"); writeFileSync(fleet, `#!/usr/bin/env bash\necho "$*" >> ${root}/north-calls\ncase "$1 $2" in\n  "fleet list") cat ${fleetSessions} ;;\n  "fleet accounts") printf "claude\\tb\\nclaude\\ta\\n" ;;\n  "fleet launch") name=$(echo "$*" | sed "s/.*--name \\([^ ]*\\).*/\\1/"); grep -q "\"$name\"" ${fleetSessions} && { echo "error: a session named $name already exists" >&2; exit 1; }; [[ " $* " == *" --account b "* ]] && { echo "claude:b is out; placement picks claude:c; launch with --account c, or keep claude:b with --pin" >&2; exit 1; }; echo ok ;;\n  *) exit 2 ;;\nesac\n`); chmodSync(fleet, 0o755);
  const env: TickEnv = {
    codeRoot: join(root, "code"), stateRoot: join(root, "state"), handoffs: join(root, "handoffs"),
    codeHealth: [fake], north: fleet,
    template: join(import.meta.dir, "review-brief.md"), dryRun: true, now: () => start,
  };
  return { root, env };
}

test("FIRE starts exactly one review launch and a held lock starts none", () => {
  const { root, env } = scratch(`echo "FIRE commits 104 >= 100"`);
  const lines: string[] = [];
  expect(tickRepo("smashcraft", env, (line) => lines.push(line)).result).toBe("started"); expect(lines).toHaveLength(1);
  expect(lines[0]).toStartWith("dry-run: "); expect(lines[0]).toContain(`fleet launch --provider claude --name ${reviewName("smashcraft", env.now())} `); expect(lines[0]).toContain("--system-prompt "); expect(lines[0]).toContain("--account b");
  const brief = readFileSync(join(env.handoffs, `${reviewName("smashcraft", env.now())}-brief.md`), "utf8");
  expect(brief).toStartWith("# Code health review: smashcraft"); expect(brief.split("\n\n")[0]).toContain("Surface: ts\nNeeds: local-state");
  expect(brief).toContain("- FIRE commits 104 >= 100"); expect(brief).not.toContain("{{");
  expect(existsSync(join(env.stateRoot, "smashcraft/review.lock"))).toBe(true);
  expect(tickRepo("smashcraft", env, (line) => lines.push(line)).result).toBe("held"); expect(lines).toHaveLength(1);
  expect(readFileSync(join(root, "calls"), "utf8").match(/^signal /gm)).toHaveLength(1);
});

test("quiet releases the lock and starts nothing", () => {
  const { env } = scratch("echo quiet; exit 1");
  const lines: string[] = [];
  expect(tickRepo("smashcraft", env, (line) => lines.push(line)).result).toBe("quiet"); expect(lines).toHaveLength(0);
  expect(existsSync(join(env.stateRoot, "smashcraft/review.lock"))).toBe(false);
});

test("a stale lock with no live review node is retaken", () => {
  const { env } = scratch(`echo "FIRE days 7"`);
  mkdirSync(join(env.stateRoot, "smashcraft"), { recursive: true });
  writeFileSync(join(env.stateRoot, "smashcraft/review.lock"), "{}\n");
  const later = { ...env, now: () => new Date(env.now().getTime() + 2 * 60 * 60 * 1000) };
  expect(tickRepo("smashcraft", later, () => {}).result).toBe("started");
  writeFileSync(join(env.north, "../fleet.json"), JSON.stringify([{ name: reviewName("smashcraft", later.now()) }]));
  expect(tickRepo("smashcraft", { ...later, now: () => new Date(later.now().getTime() + 600_000) }, () => {}).result).toBe("held");
});

test("a launch the first account cannot take is retried once on the account placement names", () => {
  const { root, env } = scratch(`echo "FIRE days 7"`); writeFileSync(join(root, "fleet.json"), JSON.stringify([{ name: "code-health-review-smashcraft" }, { name: reviewName("smashcraft", new Date(env.now().getTime() - 3_600_000)) }]));
  expect(tickRepo("smashcraft", { ...env, dryRun: false }, () => {}).result).toBe("started");
  const launches = readFileSync(join(root, "north-calls"), "utf8").split("\n").filter((line) => line.startsWith("fleet launch"));
  expect(launches).toHaveLength(2); expect(launches[0]).toContain("--account b"); expect(launches[1]).toContain("--account c");
});

test("a launch placement refuses on every account is an error that releases the lock", () => {
  const { root, env } = scratch(`echo "FIRE days 7"`);
  writeFileSync(join(root, "north"), "#!/usr/bin/env bash\nexit 1\n");
  expect(tickRepo("smashcraft", { ...env, dryRun: false }, () => {}).result).toBe("error"); expect(existsSync(join(env.stateRoot, "smashcraft/review.lock"))).toBe(false);
});

test("a failed fetch is an error that releases the lock, not quiet", () => {
  const { root, env } = scratch("echo quiet; exit 1");
  Bun.spawnSync(["git", "-C", join(root, "code/smashcraft/main"), "remote", "set-url", "origin", join(root, "missing.git")]);
  expect(tickRepo("smashcraft", env, () => {}).result).toBe("error");
  expect(existsSync(join(env.stateRoot, "smashcraft/review.lock"))).toBe(false);
});
