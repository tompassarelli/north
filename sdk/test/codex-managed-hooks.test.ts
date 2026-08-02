import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmodSync, existsSync, linkSync, lstatSync, mkdirSync, mkdtempSync, readdirSync,
  readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import {
  assertInstalledManagedCodexHooks, expectedManagedCodexHooks,
  type ManagedCodexHookInstallation, validateManagedCodexHookInstallation,
  validateManagedCodexRequirements,
} from "../src/providers/codex-managed-hooks";

const roots: string[] = [];

function makeTreeWritable(path: string): void {
  if (!existsSync(path) || lstatSync(path).isSymbolicLink()) return;
  const info = lstatSync(path);
  if (!info.isDirectory()) return;
  chmodSync(path, 0o755);
  for (const entry of readdirSync(path)) makeTreeWritable(join(path, entry));
}

afterEach(() => {
  for (const root of roots.splice(0)) {
    makeTreeWritable(root);
    rmSync(root, { recursive: true, force: true });
  }
});

function requirements(
  mutate?: (document: any) => void,
  managedDir = "/etc/codex/hooks",
): string {
  const document: any = {
    allow_managed_hooks_only: true,
    allow_remote_control: false,
    managed_hook_failure_mode: "block",
    features: { hooks: true },
    hooks: {
      managed_dir: managedDir,
      ...expectedManagedCodexHooks(managedDir),
    },
  };
  mutate?.(document);
  const lines = [
    `allow_managed_hooks_only = ${JSON.stringify(document.allow_managed_hooks_only)}`,
    `allow_remote_control = ${JSON.stringify(document.allow_remote_control)}`,
    `managed_hook_failure_mode = ${JSON.stringify(document.managed_hook_failure_mode)}`,
    ...Object.entries(document)
      .filter(([key]) => ![
        "allow_managed_hooks_only", "allow_remote_control", "managed_hook_failure_mode",
        "features", "hooks",
      ].includes(key))
      .map(([key, value]) => `${key} = ${JSON.stringify(value)}`),
    "",
    "[features]",
    `hooks = ${document.features.hooks}`,
    ...Object.entries(document.features)
      .filter(([key]) => key !== "hooks")
      .map(([key, value]) => `${key} = ${JSON.stringify(value)}`),
    "",
    "[hooks]",
    `managed_dir = ${JSON.stringify(document.hooks.managed_dir)}`,
  ];
  const canonicalEvents = [
    "SessionStart", "SubagentStart", "PreToolUse", "PostToolUse", "Stop",
  ];
  const events = [
    ...canonicalEvents,
    ...Object.keys(document.hooks)
      .filter((event) => event !== "managed_dir" && !canonicalEvents.includes(event)),
  ];
  for (const event of events) {
    for (const group of document.hooks[event] ?? []) {
      lines.push("", `[[hooks.${event}]]`);
      if (group.matcher !== undefined) lines.push(`matcher = ${JSON.stringify(group.matcher)}`);
      for (const hook of group.hooks) {
        lines.push(
          "",
          `[[hooks.${event}.hooks]]`,
          `type = ${JSON.stringify(hook.type)}`,
          `command = ${JSON.stringify(hook.command)}`,
          `timeout = ${hook.timeout}`,
        );
      }
    }
  }
  return `${lines.join("\n")}\n`;
}

const northRevision = "1".repeat(40);
const beagleRevision = "2".repeat(40);
const deploymentId = `north-${northRevision}.beagle-${beagleRevision}`;
const promotedHooks = {
  "agent-spawn-guard.sh": "north/profiles/tom/hooks/agent-spawn-guard.sh",
  "beagle-session-start.sh": "beagle/integrations/north/hooks/beagle-session-start.sh",
  "launch-critical-worktree-guard.sh":
    "north/profiles/tom/hooks/launch-critical-worktree-guard.sh",
  "logcompress-hook.js": "north/profiles/tom/hooks/logcompress-hook.js",
  "racket-build-guard.sh": "beagle/integrations/north/hooks/racket-build-guard.sh",
  "tripwire-guard.sh": "north/profiles/tom/hooks/tripwire-guard.sh",
} as const;

interface HookFixture {
  root: string;
  managedDir: string;
  enforcementRoot: string;
  deploymentRoot: string;
  generationRoot: string;
  recordPath: string;
  promotedFiles: Record<string, string>;
  installation: ManagedCodexHookInstallation;
}

function write(path: string, contents: string, mode: number): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, contents, { mode });
  chmodSync(path, mode);
}

function digest(path: string): string {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function setupHookFixture(): HookFixture {
  const root = mkdtempSync(join(tmpdir(), "north-managed-hooks-"));
  roots.push(root);
  const managedDir = join(root, "etc", "codex", "hooks");
  const nixStoreRoot = join(root, "nix", "store");
  const nixPackage = join(nixStoreRoot, `${"a".repeat(32)}-managed-hooks-test`);
  const enforcementRoot = join(root, "north-enforcement");
  const deploymentRoot = join(enforcementRoot, "deployments", deploymentId);
  const generationRoot = join(enforcementRoot, "generations", "1000000000000000000-1-1");
  mkdirSync(join(managedDir, "runtime"), { recursive: true });
  mkdirSync(nixPackage, { recursive: true });
  mkdirSync(deploymentRoot, { recursive: true });
  mkdirSync(generationRoot, { recursive: true });

  for (const runtime of ["env", "bash", "node"]) {
    const target = join(nixPackage, "runtime", runtime);
    write(target, "#!/bin/sh\nexit 0\n", 0o555);
    symlinkSync(target, join(managedDir, "runtime", runtime));
  }

  const promotedFiles: Record<string, string> = {};
  for (const [hook, manifestPath] of Object.entries(promotedHooks)) {
    const target = join(deploymentRoot, ...manifestPath.split("/"));
    write(target, `${hook}\n`, 0o444);
    promotedFiles[hook] = target;
    symlinkSync(
      join(enforcementRoot, "active", "current", ...manifestPath.split("/")),
      join(managedDir, hook),
    );
  }

  const expected = expectedManagedCodexHooks(managedDir);
  const scripts = new Set(Object.values(expected)
    .flatMap((entries) => entries.flatMap((entry) => entry.hooks))
    .map(({ command }) => command.split(" ").at(-1)!));
  for (const script of scripts) {
    const name = script.slice(`${managedDir}/`.length);
    if (name in promotedHooks) continue;
    const target = join(nixPackage, "hooks", name);
    write(target, `${name}\n`, 0o444);
    symlinkSync(target, script);
  }

  const manifest = Object.entries(promotedHooks)
    .map(([hook, manifestPath]) =>
      `FILE ${digest(promotedFiles[hook]!)}  ${manifestPath}`)
    .sort();
  const recordPath = join(generationRoot, "record");
  write(recordPath, [
    "FORMAT north-enforcement-promote/v1",
    `ID ${deploymentId}`,
    `NORTH_REV ${northRevision}`,
    `BEAGLE_REV ${beagleRevision}`,
    `PREVIOUS ${deploymentId}`,
    "WHO test",
    "WHEN 2026-08-02T00:00:00Z",
    "WHY hermetic managed hook proof",
    ...manifest,
    "",
  ].join("\n"), 0o444);
  symlinkSync(`../../deployments/${deploymentId}`, join(generationRoot, "current"));
  chmodSync(deploymentRoot, 0o555);
  chmodSync(generationRoot, 0o555);
  symlinkSync(`generations/${generationRoot.split("/").at(-1)!}`, join(enforcementRoot, "active"));

  const requirementsPath = join(nixPackage, "requirements.toml");
  write(requirementsPath, requirements(undefined, managedDir), 0o444);
  const expectedOwnerUid = process.getuid?.() ?? 0;
  return {
    root,
    managedDir,
    enforcementRoot,
    deploymentRoot,
    generationRoot,
    recordPath,
    promotedFiles,
    installation: {
      requirementsPath,
      managedDir,
      nixStoreRoot,
      enforcementRoot,
      expectedOwnerUid,
    },
  };
}

test("managed Codex requirements admit the exact full lifecycle policy", () => {
  expect(() => validateManagedCodexRequirements(requirements())).not.toThrow();
});

test("North's managed hook contract admits Firn's source requirements exactly", () => {
  const path = resolve(
    process.env.NORTH_FIRN_ROOT ?? resolve(import.meta.dir, "..", "..", "..", "..", "nixos-config", "main"),
    "modules", "codex", "requirements.toml",
  );
  expect(existsSync(path)).toBe(true);
  expect(() => validateManagedCodexRequirements(readFileSync(path, "utf8")))
    .not.toThrow();
});

test("managed Codex requirements reject every authority-bearing drift", () => {
  const hostile: Array<(document: any) => void> = [
    (document) => { document.allow_managed_hooks_only = false; },
    (document) => { document.allow_remote_control = true; },
    (document) => { document.managed_hook_failure_mode = "continue"; },
    (document) => { delete document.managed_hook_failure_mode; },
    (document) => { document.features.hooks = false; },
    (document) => { document.features.remote_control = false; },
    (document) => { document.unreviewed_root_authority = true; },
    (document) => { document.hooks.managed_dir = "/tmp/hooks"; },
    (document) => { document.hooks.PreToolUse[1].matcher = "^apply_patch$"; },
    (document) => {
      document.hooks.PreToolUse[1].hooks[2].command = "/etc/codex/hooks/north-clock-guard.sh";
    },
    (document) => { document.hooks.PreToolUse[1].hooks.pop(); },
    (document) => {
      document.hooks.PostToolUse.push({
        matcher: ".*",
        hooks: [{ type: "command", command: "/etc/codex/hooks/ambient", timeout: 10 }],
      });
    },
    (document) => { document.hooks.Stop[0].hooks[0].command = "/bin/true"; },
    (document) => {
      document.hooks.UserPromptSubmit = [{
        hooks: [{
          type: "command",
          command: "/etc/codex/hooks/ambient-user-prompt",
          timeout: 10,
        }],
      }];
    },
  ];
  for (const mutate of hostile)
    expect(() => validateManagedCodexRequirements(requirements(mutate))).toThrow();
});

test("managed-only in a non-requirements-like location cannot substitute for the root field", () => {
  const source = requirements().replace(
    "allow_managed_hooks_only = true\n",
    "",
  ).replace(
    "[hooks]\n",
    "[hooks]\nallow_managed_hooks_only = true\n",
  );
  expect(() => validateManagedCodexRequirements(source))
    .toThrow();
});

test("remote-control denial is root-only, present, and type-exact", () => {
  const exact = requirements();
  for (const source of [
    exact.replace("allow_remote_control = false\n", ""),
    exact.replace("allow_remote_control = false", "allow_remote_control = true"),
    exact.replace("allow_remote_control = false", 'allow_remote_control = "false"'),
    exact.replace(
      "allow_remote_control = false\n",
      "",
    ).replace(
      "[hooks]\n",
      "[hooks]\nallow_remote_control = false\n",
    ),
  ]) {
    expect(() => validateManagedCodexRequirements(source)).toThrow();
  }
});

test("managed Codex hook installation accepts one exact captured sealed promotion", () => {
  const fixture = setupHookFixture();
  expect(() => validateManagedCodexHookInstallation(fixture.installation)).not.toThrow();
});

test("managed Codex hook installation rejects forged deployment paths and hashes", () => {
  const pathFixture = setupHookFixture();
  const livePath = join(pathFixture.managedDir, "agent-spawn-guard.sh");
  const unattested = join(pathFixture.enforcementRoot, "unattested", "agent-spawn-guard.sh");
  write(unattested, "agent-spawn-guard.sh\n", 0o444);
  unlinkSync(livePath);
  symlinkSync(unattested, livePath);
  expect(() => validateManagedCodexHookInstallation(pathFixture.installation)).toThrow();

  const hashFixture = setupHookFixture();
  chmodSync(hashFixture.recordPath, 0o644);
  const original = readFileSync(hashFixture.recordPath, "utf8");
  const actual = digest(hashFixture.promotedFiles["agent-spawn-guard.sh"]);
  writeFileSync(hashFixture.recordPath, original.replace(
    `FILE ${actual}  ${promotedHooks["agent-spawn-guard.sh"]}`,
    `FILE ${"f".repeat(64)}  ${promotedHooks["agent-spawn-guard.sh"]}`,
  ));
  chmodSync(hashFixture.recordPath, 0o444);
  expect(() => validateManagedCodexHookInstallation(hashFixture.installation)).toThrow();
});

test("managed Codex hook installation rejects forged owner, mode, and link metadata", () => {
  const ownerFixture = setupHookFixture();
  const actualOwner = ownerFixture.installation.expectedOwnerUid;
  expect(() => validateManagedCodexHookInstallation({
    ...ownerFixture.installation,
    expectedOwnerUid: actualOwner === 0 ? 1 : 0,
  })).toThrow();

  const modeFixture = setupHookFixture();
  chmodSync(modeFixture.promotedFiles["agent-spawn-guard.sh"], 0o644);
  expect(() => validateManagedCodexHookInstallation(modeFixture.installation)).toThrow();

  const linkFixture = setupHookFixture();
  linkSync(
    linkFixture.promotedFiles["agent-spawn-guard.sh"],
    join(linkFixture.root, "forged-hardlink"),
  );
  expect(() => validateManagedCodexHookInstallation(linkFixture.installation)).toThrow();
});

test("managed Codex hook installation rejects a hook reached through selector drift", () => {
  const fixture = setupHookFixture();
  const alternateId = `north-${"3".repeat(40)}.beagle-${"4".repeat(40)}`;
  const alternateDeployment = join(fixture.enforcementRoot, "deployments", alternateId);
  const manifestPath = promotedHooks["agent-spawn-guard.sh"];
  const alternateFile = join(alternateDeployment, ...manifestPath.split("/"));
  write(alternateFile, "agent-spawn-guard.sh\n", 0o444);
  chmodSync(alternateDeployment, 0o555);
  const alternateGeneration = join(
    fixture.enforcementRoot,
    "generations",
    "2000000000000000000-2-2",
  );
  mkdirSync(alternateGeneration);
  symlinkSync(`../../deployments/${alternateId}`, join(alternateGeneration, "current"));
  chmodSync(alternateGeneration, 0o555);
  symlinkSync("generations/2000000000000000000-2-2", join(fixture.enforcementRoot, "drifted"));
  const livePath = join(fixture.managedDir, "agent-spawn-guard.sh");
  unlinkSync(livePath);
  symlinkSync(
    join(fixture.enforcementRoot, "drifted", "current", ...manifestPath.split("/")),
    livePath,
  );
  expect(() => validateManagedCodexHookInstallation(fixture.installation)).toThrow();
});

test("installed managed Codex hook assertion completes before any thread or provider turn", () => {
  expect(() => assertInstalledManagedCodexHooks()).not.toThrow();
});
