import { expect, test } from "bun:test";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { harnessOptions } from "../src/harness";

async function capturedLines(path: string, count: number): Promise<string[]> {
  for (let attempt = 0; attempt < 200; attempt++) {
    const lines = existsSync(path)
      ? readFileSync(path, "utf8").split("\n").filter(Boolean)
      : [];
    if (lines.length >= count) return lines;
    await Bun.sleep(5);
  }
  return existsSync(path) ? readFileSync(path, "utf8").split("\n").filter(Boolean) : [];
}

test("presence resolves its fake executable and NORTH_PORT after harness import for every call", async () => {
  const saved = Object.fromEntries(["PATH", "NORTH_PORT", "HARNESS_PRESENCE_LOG", "AGENT_LAWS", "AGENT_PRAXIS"]
    .map((key) => [key, process.env[key]]));
  const dir = mkdtempSync(join(tmpdir(), "north-harness-presence-"));
  const log = join(dir, "presence.log");
  const fakeBb = join(dir, "bb");
  writeFileSync(fakeBb, "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"$HARNESS_PRESENCE_LOG\"\n");
  chmodSync(fakeBb, 0o755);

  try {
    // harness.ts was imported above. These values intentionally arrive later,
    // reproducing Bun's cross-file module-cache ordering from the live leak.
    process.env.PATH = `${dir}:${saved.PATH ?? ""}`;
    process.env.HARNESS_PRESENCE_LOG = log;
    process.env.NORTH_PORT = "64123";
    process.env.AGENT_LAWS = "off";
    process.env.AGENT_PRAXIS = "off";
    const self = `test-late-presence-${process.pid}`;
    const options = harnessOptions({ self });
    const repoCwd = join(dir, "gaffer");
    const repoSelf = `${self}-repo`;
    const repoOptions = harnessOptions({ self: repoSelf, cwd: repoCwd });

    const registrations = await capturedLines(log, 2);
    expect(registrations).toHaveLength(2);
    for (const expected of [
      `${join(import.meta.dir, "../../cli/presence-cli.clj")} 64123 register ${self} ${process.cwd()} ${self}`,
      `${join(import.meta.dir, "../../cli/presence-cli.clj")} 64123 register ${repoSelf} ${repoCwd} ${repoSelf}`,
    ]) expect(registrations).toContain(expected);
    expect(repoOptions.cwd).toBe(repoCwd);
    expect(repoOptions.systemPrompt).toContain(`in "gaffer"`);

    process.env.NORTH_PORT = "64124";
    const renew = (options.hooks as any).PostToolUse[0].hooks[0];
    expect(await renew()).toEqual({ continue: true });
    expect(await capturedLines(log, 3)).toContain(
      `${join(import.meta.dir, "../../cli/presence-cli.clj")} 64124 renew ${self}`,
    );
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});

test("suppressed or injected registration never leaks a real PostToolUse renew", async () => {
  const saved = Object.fromEntries(["PATH", "NORTH_PORT", "HARNESS_PRESENCE_LOG", "AGENT_LAWS", "AGENT_PRAXIS"]
    .map((key) => [key, process.env[key]]));
  const dir = mkdtempSync(join(tmpdir(), "north-harness-presence-seam-"));
  const log = join(dir, "presence.log");
  const fakeBb = join(dir, "bb");
  writeFileSync(fakeBb, "#!/usr/bin/env bash\nprintf '%s\\n' \"$*\" >> \"$HARNESS_PRESENCE_LOG\"\n");
  chmodSync(fakeBb, 0o755);

  try {
    process.env.PATH = `${dir}:${saved.PATH ?? ""}`;
    process.env.HARNESS_PRESENCE_LOG = log;
    process.env.NORTH_PORT = "64125";
    process.env.AGENT_LAWS = "off";
    process.env.AGENT_PRAXIS = "off";
    const captured: string[] = [];
    const renewed: string[] = [];

    const suppressed = harnessOptions({ self: "presence-suppressed", presenceRegistrar: false });
    await (suppressed.hooks as any).PostToolUse[0].hooks[0]();

    const injected = harnessOptions({
      self: "presence-injected",
      presenceRegistrar: (self, cwd) => captured.push(`${self}|${cwd}`),
    });
    await (injected.hooks as any).PostToolUse[0].hooks[0]();

    const fullyInjected = harnessOptions({
      self: "presence-fully-injected",
      presenceRegistrar: (self, cwd) => captured.push(`${self}|${cwd}`),
      presenceRenewer: (self) => renewed.push(self),
    });
    await (fullyInjected.hooks as any).PostToolUse[0].hooks[0]();

    await Bun.sleep(50);
    expect(existsSync(log) ? readFileSync(log, "utf8") : "").toBe("");
    expect(captured.map((line) => line.split("|", 1)[0])).toEqual([
      "presence-injected", "presence-fully-injected",
    ]);
    expect(renewed).toEqual(["presence-fully-injected"]);
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});
