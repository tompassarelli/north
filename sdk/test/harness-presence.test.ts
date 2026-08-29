import { expect, spyOn, test } from "bun:test";
import {
  chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { harnessOptions } from "../src/harness";
import { beagleStoreSelection } from "../src/beagle-store";
import {
  canonicalPresenceFence, parsePresenceFence, presenceFencePath,
} from "../src/presence-fence";

const forbiddenOnlineToken = /(^|[^A-Za-z0-9_])presence([^A-Za-z0-9_]|$)/i;

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
  const saved = Object.fromEntries(
    ["PATH", "NORTH_PORT", "NORTH_AGENT_LOGS_DIR", "HARNESS_PRESENCE_LOG", "AGENT_LAWS", "AGENT_PRAXIS", "NORTH_PEER_BB"]
      .map((key) => [key, process.env[key]]),
  );
  const dir = mkdtempSync(join(tmpdir(), "north-harness-presence-"));
  const log = join(dir, "presence.log");
  const fakeBb = join(dir, "bb");
  writeFileSync(fakeBb, `#!/usr/bin/env bash
printf '%s\\n' "$*" >> "$HARNESS_PRESENCE_LOG"
if [ "\${1-}" = "-cp" ]; then shift 2; fi
verb="\${3-}"
handle="\${4-}"
if [ "$verb" = "register" ]; then
  printf '{"resource":"session:%s","holder":"%s","epoch":7}\\n' "$handle" "$handle"
elif [ "$verb" = "renew" ]; then
  printf '{"resource":"session:%s","holder":"%s","epoch":8}\\n' "$handle" "$handle"
fi
`);
  chmodSync(fakeBb, 0o755);

  try {
    // harness.ts was imported above. These values intentionally arrive later,
    // reproducing Bun's cross-file module-cache ordering from the live leak.
    process.env.PATH = `${dir}:${saved.PATH ?? ""}`;
    process.env.HARNESS_PRESENCE_LOG = log;
    process.env.NORTH_PORT = "64123";
    process.env.NORTH_AGENT_LOGS_DIR = join(dir, "agents");
    process.env.AGENT_LAWS = "off";
    process.env.AGENT_PRAXIS = "off";
    // presenceBb() honors NORTH_PEER_BB ahead of a PATH lookup (src/harness.ts:68).
    // An ambient managed-lane NORTH_PEER_BB (real babashka) would shadow this
    // test's fake `bb` and try to register against a real coordinator on the
    // fake port, failing with exit 1. Delete it so PATH resolution is exercised
    // as intended, matching the same NORTH_PEER_BB scrub other harness tests
    // apply (topology-authority.test.ts, spawn-boundary.test.ts).
    delete process.env.NORTH_PEER_BB;
    const self = `test-late-presence-${process.pid}`;
    const options = harnessOptions({ self });
    const repoCwd = join(dir, "orchestration");
    const repoSelf = `${self}-repo`;
    const repoOptions = harnessOptions({ self: repoSelf, cwd: repoCwd });
    const fencePath = presenceFencePath(self);

    const registrations = await capturedLines(log, 2);
    expect(registrations).toHaveLength(2);
    const storePrefix = `-cp ${beagleStoreSelection().out} `;
    for (const expected of [
      `${storePrefix}${join(import.meta.dir, "../../cli/presence-cli.clj")} 64123 register ${self} ${process.cwd()} ${self}`,
      `${storePrefix}${join(import.meta.dir, "../../cli/presence-cli.clj")} 64123 register ${repoSelf} ${repoCwd} ${repoSelf}`,
    ]) expect(registrations).toContain(expected);
    expect(repoOptions.cwd).toBe(repoCwd);
    expect(repoOptions.systemPrompt).toContain(`in "orchestration"`);
    expect(readFileSync(fencePath, "utf8")).toBe(
      `{"resource":"session:${self}","holder":"${self}","epoch":7}\n`,
    );
    expect(statSync(fencePath).mode & 0o777).toBe(0o600);

    process.env.NORTH_PORT = "64124";
    const renew = (options.hooks as any).PostToolUse[0].hooks[0];
    expect(await renew()).toEqual({ continue: true });
    expect(await capturedLines(log, 3)).toContain(
      `${storePrefix}${join(import.meta.dir, "../../cli/presence-cli.clj")} 64124 renew ${self} `
      + `{"resource":"session:${self}","holder":"${self}","epoch":7}`,
    );
    expect(readFileSync(fencePath, "utf8")).toBe(
      `{"resource":"session:${self}","holder":"${self}","epoch":8}\n`,
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

test("liveness fence paths and rendered validation errors use current vocabulary", () => {
  expect(presenceFencePath("liveness-path-probe", {
    NORTH_AGENT_LOGS_DIR: "/tmp/north-liveness-path-probe",
  })).toEndWith("/liveness-path-probe.liveness-fence.json");
  const rendered: string[] = [];
  for (const invoke of [
    () => canonicalPresenceFence(null),
    () => parsePresenceFence("not-json", "liveness-path-probe"),
    () => canonicalPresenceFence(
      { resource: "session:other", holder: "other", epoch: 1 },
      "liveness-path-probe",
    ),
  ]) {
    try {
      invoke();
    } catch (error) {
      rendered.push(String(error));
    }
  }
  expect(rendered).toHaveLength(3);
  expect(rendered.join("\n")).not.toMatch(forbiddenOnlineToken);
});

test("failed renewal renders a sanitized liveness diagnostic", async () => {
  const keys = [
    "PATH", "NORTH_PORT", "NORTH_AGENT_LOGS_DIR", "HARNESS_PRESENCE_LOG",
    "AGENT_LAWS", "AGENT_PRAXIS", "NORTH_PEER_BB",
  ];
  const saved = Object.fromEntries(keys.map((key) => [key, process.env[key]]));
  const dir = mkdtempSync(join(tmpdir(), "north-harness-liveness-error-"));
  const fakeBb = join(dir, "bb");
  writeFileSync(fakeBb, `#!/usr/bin/env bash
if [ "\${1-}" = "-cp" ]; then shift 2; fi
verb="\${3-}"
handle="\${4-}"
if [ "$verb" = "register" ]; then
  printf '{"resource":"session:%s","holder":"%s","epoch":7}\\n' "$handle" "$handle"
  exit 0
fi
printf 'north lease-internal: liveness renewal rejected\\n' >&2
exit 2
`);
  chmodSync(fakeBb, 0o755);
  const rendered: string[] = [];
  const errorSpy = spyOn(console, "error").mockImplementation((...args) => {
    rendered.push(args.map(String).join(" "));
  });
  try {
    process.env.PATH = `${dir}:${saved.PATH ?? ""}`;
    process.env.NORTH_PORT = "64126";
    process.env.NORTH_AGENT_LOGS_DIR = join(dir, "agents");
    process.env.AGENT_LAWS = "off";
    process.env.AGENT_PRAXIS = "off";
    delete process.env.NORTH_PEER_BB;
    const options = harnessOptions({ self: "liveness-failure-probe" });
    const renew = (options.hooks as any).PostToolUse[0].hooks[0];
    expect(await renew()).toEqual({ continue: true });
    for (let attempt = 0; attempt < 200 && rendered.length === 0; attempt++)
      await Bun.sleep(5);
    expect(rendered.join("\n")).toContain("[liveness]");
    expect(rendered.join("\n")).toContain("north lease-internal: liveness renewal rejected");
    expect(rendered.join("\n")).not.toMatch(forbiddenOnlineToken);
  } finally {
    errorSpy.mockRestore();
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    rmSync(dir, { recursive: true, force: true });
  }
});
