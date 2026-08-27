import { afterEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmodSync, existsSync, linkSync, lstatSync, mkdirSync, mkdtempSync, readdirSync,
  readFileSync, rmSync, symlinkSync, unlinkSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import {
  expectedManagedCodexHooks, CODEX_MANAGED_HOOKS_DIR, FIRN_SYSTEM_POLICY,
  type ManagedCodexHookInstallation, reportManagedCodexHookInstallation,
  validateManagedCodexHookInstallation, validateManagedCodexRequirements,
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
  systemPolicyPath = FIRN_SYSTEM_POLICY,
): string {
  const document: any = {
    allow_managed_hooks_only: true,
    allow_remote_control: false,
    managed_hook_failure_mode: "block",
    features: { hooks: true },
    hooks: {
      managed_dir: managedDir,
      ...expectedManagedCodexHooks(managedDir, systemPolicyPath),
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
    "SessionStart", "SubagentStart", "SubagentStop",
    "PreToolUse", "PostToolUse", "Stop",
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

const nixosRevision = "0".repeat(40);
const northRevision = "1".repeat(40);
const beagleRevision = "2".repeat(40);
const deploymentId = `nixos-${nixosRevision}.north-${northRevision}.beagle-${beagleRevision}`;
const promotedHooks = {
  "agent-spawn-guard.sh": "north/agent-runtime/hooks/agent-spawn-guard.sh",
  "beagle-session-start.sh": "beagle/integrations/north/hooks/beagle-session-start.sh",
  "corpus-scan-guard.sh": "nixos-config/dotfiles/agents/hooks/corpus-scan-guard.sh",
  "concrete-model-identity-guard.sh":
    "nixos-config/dotfiles/agents/hooks/concrete-model-identity-guard.sh",
  "launch-critical-worktree-guard.sh":
    "nixos-config/dotfiles/agents/hooks/launch-critical-worktree-guard.sh",
  "logcompress-hook.py": "north/agent-runtime/hooks/logcompress-hook.py",
  "logcompress.py": "north/agent-runtime/hooks/logcompress.py",
  "session-kill-guard.sh": "nixos-config/dotfiles/agents/hooks/session-kill-guard.sh",
  "tripwire-guard.sh": "nixos-config/dotfiles/agents/hooks/tripwire-guard.sh",
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
  const systemPolicyPath = join(nixPackage, "bin", "firn-system-policy");
  const enforcementRoot = join(root, "north-enforcement");
  const deploymentRoot = join(enforcementRoot, "deployments", deploymentId);
  const generationRoot = join(enforcementRoot, "generations", "1000000000000000000-1-1");
  mkdirSync(join(managedDir, "runtime"), { recursive: true });
  mkdirSync(nixPackage, { recursive: true });
  mkdirSync(deploymentRoot, { recursive: true });
  mkdirSync(generationRoot, { recursive: true });

  for (const runtime of ["env", "bash", "python3"]) {
    const target = join(nixPackage, "runtime", runtime);
    write(target, "#!/bin/sh\nexit 0\n", 0o555);
    symlinkSync(target, join(managedDir, "runtime", runtime));
  }
  write(systemPolicyPath, "#!/bin/sh\nexit 0\n", 0o555);

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

  const expected = expectedManagedCodexHooks(managedDir, systemPolicyPath);
  const scripts = new Set(Object.values(expected)
    .flatMap((entries) => entries.flatMap((entry) => entry.hooks))
    .map(({ command }) => command.split(" ").at(-1)!));
  for (const script of scripts) {
    if (script === systemPolicyPath) continue;
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
    `NIXOS_REV ${nixosRevision}`,
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
  write(requirementsPath, requirements(undefined, managedDir, systemPolicyPath), 0o444);
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
      systemPolicyPath,
    },
  };
}

test("managed Codex requirements admit the exact full lifecycle policy", () => {
  expect(() => validateManagedCodexRequirements(requirements())).not.toThrow();
});

test("managed Codex authoring entrances invoke the native Firn system policy", () => {
  expect(FIRN_SYSTEM_POLICY)
    .toBe("/home/tom/.local/lib/firn/policy/current/bin/firn-system-policy");
  const expected = expectedManagedCodexHooks();
  const preToolUse = expected.PreToolUse;
  for (const matcher of ["^(Edit|Write|MultiEdit)$", "^Bash$"]) {
    const commands = preToolUse.find((entry) => entry.matcher === matcher)!.hooks
      .map((hook) => hook.command);
    expect(commands).toContain(FIRN_SYSTEM_POLICY);
  }
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
      document.hooks.PreToolUse[2].hooks[0].command = "/etc/codex/hooks/unsealed-authoring-guard";
    },
    (document) => { document.hooks.PreToolUse[1].hooks.pop(); },
    (document) => {
      document.hooks.PostToolUse.push({
        matcher: ".*",
        hooks: [{ type: "command", command: "/etc/codex/hooks/ambient", timeout: 10 }],
      });
    },
    (document) => { document.hooks.Stop[0].hooks[0].command = "/bin/true"; },
    (document) => { document.hooks.SubagentStop[0].hooks[0].timeout = 10; },
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

test("managed Codex hook installation requires the logcompress companion module", () => {
  const fixture = setupHookFixture();
  unlinkSync(join(fixture.managedDir, "logcompress.py"));
  expect(() => validateManagedCodexHookInstallation(fixture.installation)).toThrow();
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
  const alternateId = `nixos-${"5".repeat(40)}.north-${"3".repeat(40)}.beagle-${"4".repeat(40)}`;
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

test("managed Codex hook report resolves every path the preflight verifies", () => {
  const fixture = setupHookFixture();
  const report = reportManagedCodexHookInstallation(fixture.installation);
  expect(report.requirements.ok).toBe(true);
  expect(report.runtime.map(({ hook }) => hook).sort())
    .toEqual(["runtime/bash", "runtime/env", "runtime/python3"]);
  expect(report.hooks.every(({ supply }) => supply === "nix" || supply === "sealed")).toBe(true);
  expect(report.hooks.some(({ hook }) => hook === "beagle-session-start.sh")).toBe(true);
});

// The Aug-3 failure class: one hook outside the closure kills every OpenAI lane
// at preflight, and the preflight's short-circuit names only the first one.
test("managed Codex hook report names every hook outside the closure, not just the first", () => {
  const fixture = setupHookFixture();
  for (const hook of ["beagle-session-start.sh", "tripwire-guard.sh"])
    unlinkSync(join(fixture.managedDir, hook));
  const report = reportManagedCodexHookInstallation(fixture.installation);
  const unavailable = report.hooks
    .filter(({ supply }) => supply === "unavailable").map(({ hook }) => hook);
  expect(unavailable.sort()).toEqual(["beagle-session-start.sh", "tripwire-guard.sh"]);
  const detail = report.hooks.find(({ hook }) => hook === "beagle-session-start.sh")?.detail;
  expect(detail).toContain("beagle-session-start.sh");
  expect(() => validateManagedCodexHookInstallation(fixture.installation)).toThrow();
});

test("managed Codex hook report survives invalid requirements and still checks hooks", () => {
  const fixture = setupHookFixture();
  const { requirementsPath } = fixture.installation;
  chmodSync(requirementsPath, 0o644);
  writeFileSync(requirementsPath, requirements((document) => {
    document.allow_managed_hooks_only = false;
  }, fixture.managedDir));
  const report = reportManagedCodexHookInstallation(fixture.installation);
  expect(report.requirements.ok).toBe(false);
  expect(report.hooks.every(({ supply }) => supply !== "unavailable")).toBe(true);
});
