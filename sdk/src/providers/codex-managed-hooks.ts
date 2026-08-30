import { createHash } from "node:crypto";
import {
  accessSync, closeSync, constants, fstatSync, lstatSync, openSync, readFileSync,
  realpathSync, statSync,
} from "node:fs";
import { basename, dirname, posix, relative, resolve, sep } from "node:path";
import { providerPreacceptError } from "./types";

export const CODEX_MANAGED_REQUIREMENTS = "/etc/codex/requirements.toml";
export const CODEX_MANAGED_HOOKS_DIR = "/etc/codex/hooks";
export const FIRN_SYSTEM_POLICY = "/etc/codex/hooks/firn-system-policy";
const MAX_REQUIREMENTS_BYTES = 128 * 1024;
const NORTH_ENFORCEMENT_ROOT = "/var/lib/north-enforcement";
const NIX_STORE_ROOT = "/nix/store";
const MAX_PROMOTION_RECORD_BYTES = 128 * 1024;
const MAX_PROMOTION_FILES = 1024;

type PromotedRepository = "nixos-config" | "north" | "beagle";

interface PromotedHookSource {
  repository: PromotedRepository;
  path: string;
}

const PROMOTED_HOOK_SOURCES: Readonly<Record<string, PromotedHookSource>> = {
  "agent-spawn-guard.sh": {
    repository: "north",
    path: "agent-runtime/hooks/agent-spawn-guard.sh",
  },
  "beagle-session-start.sh": {
    repository: "beagle",
    path: "integrations/north/hooks/beagle-session-start.sh",
  },
  "corpus-scan-guard.sh": {
    repository: "nixos-config",
    path: "dotfiles/agents/hooks/corpus-scan-guard.sh",
  },
  "firn-system-policy": {
    repository: "north",
    path: "agent-runtime/hooks/firn-system-policy.sh",
  },
  "concrete-model-identity-guard.sh": {
    repository: "nixos-config",
    path: "dotfiles/agents/hooks/concrete-model-identity-guard.sh",
  },
  "launch-critical-worktree-guard.sh": {
    repository: "nixos-config",
    path: "dotfiles/agents/hooks/launch-critical-worktree-guard.sh",
  },
  "logcompress-hook.py": {
    repository: "north",
    path: "agent-runtime/hooks/logcompress-hook.py",
  },
  "logcompress.py": {
    repository: "north",
    path: "agent-runtime/hooks/logcompress.py",
  },
  "resource-safe-search-guard.sh": {
    repository: "nixos-config",
    path: "dotfiles/agents/hooks/resource-safe-search-guard.sh",
  },
  "session-kill-guard.sh": {
    repository: "nixos-config",
    path: "dotfiles/agents/hooks/session-kill-guard.sh",
  },
  "tripwire-guard.sh": {
    repository: "nixos-config",
    path: "dotfiles/agents/hooks/tripwire-guard.sh",
  },
};

const PROMOTED_HOOK_DEPENDENCIES: Readonly<
  Record<string, readonly PromotedHookSource[]>
> = {
  "agent-spawn-guard.sh": [
    {
      repository: "north",
      path: "agent-runtime/hooks/agent-spawn-guard.js",
    },
    {
      repository: "north",
      path: "sdk/src/bridge/generated/beagle/core.js",
    },
    {
      repository: "north",
      path: "sdk/src/bridge/generated/beagle/exception-dispatch.js",
    },
  ],
  "logcompress-hook.py": [{
    repository: "north",
    path: "agent-runtime/hooks/logcompress.py",
  }],
};

interface PromotionRecord {
  id: string;
  nixosRevision: string;
  northRevision: string;
  beagleRevision: string;
  files: ReadonlyMap<string, string>;
}

interface CapturedPromotion {
  deploymentRoot: string;
  expectedOwnerUid: number;
  record: PromotionRecord;
}

export interface ManagedCodexHookInstallation {
  requirementsPath: string;
  managedDir: string;
  nixStoreRoot: string;
  enforcementRoot: string;
  expectedOwnerUid: number;
}

interface ManagedCommandHook {
  type: "command";
  command: string;
  timeout: number;
}

interface ManagedMatcher {
  matcher?: string;
  hooks: ManagedCommandHook[];
}

const managedBashPath = (managedDir: string): string => [
  resolve(managedDir, "runtime"),
  "/home/tom/.local/bin",
  "/run/current-system/sw/bin",
].join(":");

type ManagedHookLauncher = "bash" | "firn" | "python3";
const FIRN_SYSTEM_POLICY_ADAPTER = basename(FIRN_SYSTEM_POLICY);
const MANAGED_HOOK_LAUNCHERS = {
  "beagle-session-start.sh": "bash",
  "north-on-spawn-codex": "bash",
  "north-on-terminal-codex": "bash",
  [FIRN_SYSTEM_POLICY_ADAPTER]: "firn",
  "agent-spawn-guard.sh": "bash",
  "launch-critical-worktree-guard.sh": "bash",
  "concrete-model-identity-guard.sh": "bash",
  "tripwire-guard.sh": "bash",
  "corpus-scan-guard.sh": "bash",
  "resource-safe-search-guard.sh": "bash",
  "session-kill-guard.sh": "bash",
  "logcompress-hook.py": "python3",
  "north-on-tooluse-codex": "bash",
  "north-mark-delegated-codex": "bash",
  "north-on-stop-codex": "bash",
} as const satisfies Readonly<Record<string, ManagedHookLauncher>>;

function managedHookLauncher(name: string): ManagedHookLauncher {
  const launchers = MANAGED_HOOK_LAUNCHERS as Readonly<Record<string, ManagedHookLauncher>>;
  if (!Object.prototype.hasOwnProperty.call(launchers, name))
    throw new Error(`managed Codex hook identity ${name} is not allowed`);
  return launchers[name]!;
}

const command = (
  name: string,
  timeout = 10,
  managedDir = CODEX_MANAGED_HOOKS_DIR,
): ManagedCommandHook => {
  const launcher = managedHookLauncher(name);
  const interpreter = launcher === "python3" ? "python3" : "bash";
  return {
    type: "command",
    command: [
      resolve(managedDir, "runtime/env"),
      "-u", "BASH_ENV",
      "-u", "ENV",
      ...(launcher === "bash" ? [`PATH=${managedBashPath(managedDir)}`] : []),
      resolve(managedDir, `runtime/${interpreter}`),
      resolve(managedDir, name),
    ].join(" "),
    timeout,
  };
};

/**
 * Exact provider-native lifecycle/authoring/activity boundary for Codex.
 *
 * The `*-codex` lifecycle wrappers are identity-aware. With AGENT_ID present
 * (a managed North lane), they are graph/identity no-ops because the harness
 * owns registration, activity renewal, delegation settlement, and terminal
 * publication. Without AGENT_ID (a native Codex session), they delegate to the
 * pinned native lifecycle scripts with provider=openai. Reusing the native
 * scripts directly here would mint a duplicate session-* identity for one lane.
 *
 * launch-critical-worktree-guard.sh is wired on both mutation entrances on
 * purpose: apply_patch carries tool_input.file_path, Bash carries
 * tool_input.command, and enforcement on one entrance is not enforcement.
 * Firn's singular provider adapter runs once at the front of every hookable
 * PreToolUse entrance. Its core observes only its fixed operation vocabulary;
 * native code-mode exec/wait remain outside Codex's hook payload surface.
 */
export function expectedManagedCodexHooks(
  managedDir = CODEX_MANAGED_HOOKS_DIR,
): Record<
  | "SessionStart" | "SubagentStart" | "SubagentStop"
  | "PreToolUse" | "PostToolUse" | "Stop",
  ManagedMatcher[]
> {
  return {
    SessionStart: [{
      hooks: [
        command("beagle-session-start.sh", 30, managedDir),
        command("north-on-spawn-codex", 15, managedDir),
      ],
    }],
    SubagentStart: [{
      hooks: [command("north-on-spawn-codex", 15, managedDir)],
    }],
    SubagentStop: [{
      hooks: [command("north-on-terminal-codex", 3, managedDir)],
    }],
    PreToolUse: [
      {
        hooks: [command(FIRN_SYSTEM_POLICY_ADAPTER, 10, managedDir)],
      },
      {
        matcher: "^(Agent|Task|Workflow)$",
        hooks: [command("agent-spawn-guard.sh", 10, managedDir)],
      },
      {
        matcher: "^(Edit|Write|MultiEdit|apply_patch)$",
        hooks: [
          command("launch-critical-worktree-guard.sh", 10, managedDir),
          command("concrete-model-identity-guard.sh", 10, managedDir),
        ],
      },
      {
        matcher: "^Bash$",
        hooks: [
          command("agent-spawn-guard.sh", 10, managedDir),
          command("tripwire-guard.sh", 10, managedDir),
          command("launch-critical-worktree-guard.sh", 10, managedDir),
          command("corpus-scan-guard.sh", 10, managedDir),
          command("resource-safe-search-guard.sh", 10, managedDir),
          command("session-kill-guard.sh", 10, managedDir),
          command("concrete-model-identity-guard.sh", 10, managedDir),
        ],
      },
    ],
    PostToolUse: [
      {
        matcher: "^Bash$",
        hooks: [
          command("logcompress-hook.py", 10, managedDir),
          command("north-on-tooluse-codex", 10, managedDir),
        ],
      },
      {
        matcher: "^(Edit|Write|MultiEdit|apply_patch)$",
        hooks: [command("north-on-tooluse-codex", 10, managedDir)],
      },
      {
        matcher: "^(mcp__north__spawn|mcp__north__dispatch|Task|Agent)$",
        hooks: [command("north-mark-delegated-codex", 10, managedDir)],
      },
    ],
    Stop: [{
      hooks: [command("north-on-stop-codex", 10, managedDir)],
    }],
  };
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object")
    return Object.fromEntries(Object.keys(value as object).sort()
      .map((key) => [key, canonical((value as any)[key])]));
  return value;
}

function exact(value: unknown, expected: unknown, label: string): void {
  if (JSON.stringify(canonical(value)) !== JSON.stringify(canonical(expected)))
    throw new Error(`${label} does not match North's exact managed Codex contract`);
}

/**
 * Validate the requirements policy itself, not user/session config. Codex
 * intentionally ignores allow_managed_hooks_only outside requirements layers.
 */
export function validateManagedCodexRequirements(
  source: string,
  managedDir = CODEX_MANAGED_HOOKS_DIR,
): void {
  let parsed: any;
  try { parsed = Bun.TOML.parse(source); }
  catch (cause) { throw new Error("managed Codex requirements are invalid TOML", { cause }); }
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed))
    throw new Error("managed Codex requirements must be a TOML document");
  exact(
    Object.keys(parsed).sort(),
    [
      "allow_managed_hooks_only", "allow_remote_control", "features", "hooks",
      "managed_hook_failure_mode",
    ].sort(),
    "managed Codex requirements root surface",
  );
  if (parsed.allow_managed_hooks_only !== true)
    throw new Error("managed Codex requirements must enforce allow_managed_hooks_only=true");
  if (parsed.allow_remote_control !== false)
    throw new Error("managed Codex requirements must enforce allow_remote_control=false");
  if (parsed.managed_hook_failure_mode !== "block")
    throw new Error('managed Codex requirements must enforce managed_hook_failure_mode="block"');
  exact(parsed.features, { hooks: true }, "managed Codex feature requirements");
  if (parsed.hooks?.managed_dir !== managedDir)
    throw new Error(`managed Codex requirements must pin hooks.managed_dir=${managedDir}`);
  const expected = expectedManagedCodexHooks(managedDir);
  const expectedKeys = [...Object.keys(expected), "managed_dir"].sort();
  if (Object.keys(parsed.hooks ?? {}).sort().join(",") !== expectedKeys.join(","))
    throw new Error("managed Codex hook event surface is not exact");
  for (const [event, entries] of Object.entries(expected)) {
    const actual = parsed.hooks?.[event];
    if (Array.isArray(actual)) {
      for (const entry of actual) {
        if (!Array.isArray(entry?.hooks)) continue;
        for (const hook of entry.hooks) {
          if (typeof hook?.command === "string")
            managedCommandPaths(hook.command, managedDir);
        }
      }
    }
    exact(actual, entries, `managed Codex ${event}`);
  }
}

function assertNixManagedFile(
  path: string,
  executable = false,
  nixStoreRoot = NIX_STORE_ROOT,
): void {
  const info = statSync(path);
  if (!info.isFile()) throw new Error(`${path} is not a regular file`);
  const target = realpathSync(path);
  const canonicalStore = realpathSync(nixStoreRoot);
  if (!target.startsWith(`${canonicalStore}${sep}`))
    throw new Error(`${path} is not supplied by the verified Nix closure`);
  if (executable) accessSync(path, constants.X_OK);
}

function exactSealedBytes(
  path: string,
  label: string,
  expectedOwnerUid: number,
  maxBytes?: number,
): Buffer {
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const info = fstatSync(fd);
    if (!info.isFile()) throw new Error(`${label} is not a regular file`);
    const mode = info.mode & 0o777;
    if (info.uid !== expectedOwnerUid || mode !== 0o444 || info.nlink !== 1) {
      throw new Error(
        `${label} is not sealed (${info.uid}:${mode.toString(8)}:${info.nlink}, `
          + `want ${expectedOwnerUid}:444:1)`,
      );
    }
    if (maxBytes !== undefined && info.size > maxBytes)
      throw new Error(`${label} exceeds the bounded size`);
    const bytes = readFileSync(fd);
    if (maxBytes !== undefined && bytes.byteLength > maxBytes)
      throw new Error(`${label} exceeds the bounded size`);
    return bytes;
  } finally {
    closeSync(fd);
  }
}

function assertSealedDirectory(path: string, label: string, expectedOwnerUid: number): void {
  const info = statSync(path);
  const mode = info.mode & 0o777;
  if (!info.isDirectory() || info.uid !== expectedOwnerUid || mode !== 0o555)
    throw new Error(`${label} is not a sealed ${expectedOwnerUid}:555 directory`);
}

function assertOwnedSelector(path: string, label: string, expectedOwnerUid: number): void {
  const info = lstatSync(path);
  if (!info.isSymbolicLink() || info.uid !== expectedOwnerUid || info.nlink !== 1)
    throw new Error(`${label} is not an owned single-link selector`);
}

function parsePromotionRecord(source: string, deploymentName: string): PromotionRecord {
  if (!source || source.includes("\r") || source.includes("\0"))
    throw new Error("managed Codex promotion record encoding is invalid");
  const encoded = source.endsWith("\n") ? source.slice(0, -1) : source;
  const lines = encoded.split("\n");
  const exactHeader = (index: number, name: string, pattern: RegExp): string => {
    const line = lines[index] ?? "";
    const match = line.match(pattern);
    if (!match?.[1]) throw new Error(`managed Codex promotion record ${name} is invalid`);
    return match[1];
  };
  if (lines[0] !== "FORMAT north-enforcement-promote/v1")
    throw new Error("managed Codex promotion record format is invalid");
  const deploymentPattern = "nixos-[0-9a-f]{40}\\.north-[0-9a-f]{40}\\.beagle-[0-9a-f]{40}";
  const id = exactHeader(1, "ID", new RegExp(`^ID (${deploymentPattern})$`));
  const nixosRevision = exactHeader(2, "NIXOS_REV", /^NIXOS_REV ([0-9a-f]{40})$/);
  const northRevision = exactHeader(3, "NORTH_REV", /^NORTH_REV ([0-9a-f]{40})$/);
  const beagleRevision = exactHeader(4, "BEAGLE_REV", /^BEAGLE_REV ([0-9a-f]{40})$/);
  exactHeader(5, "PREVIOUS", new RegExp(`^PREVIOUS (${deploymentPattern})$`));
  exactHeader(6, "WHO", /^WHO ([A-Za-z_][A-Za-z0-9_.-]{0,127})$/);
  exactHeader(7, "WHEN", /^WHEN (\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z)$/);
  const why = exactHeader(8, "WHY", /^WHY (.{1,4096})$/);
  if (why.includes("\0")) throw new Error("managed Codex promotion record WHY is invalid");
  const expectedId = `nixos-${nixosRevision}.north-${northRevision}.beagle-${beagleRevision}`;
  if (id !== expectedId || id !== deploymentName)
    throw new Error("managed Codex promotion record revision mapping is invalid");

  const manifestLines = lines.slice(9);
  if (manifestLines.length < 1 || manifestLines.length > MAX_PROMOTION_FILES)
    throw new Error("managed Codex promotion manifest size is invalid");
  const files = new Map<string, string>();
  for (const line of manifestLines) {
    const match = line.match(/^FILE ([0-9a-f]{64})  ([A-Za-z0-9._/-]{1,512})$/);
    if (!match?.[1] || !match[2])
      throw new Error("managed Codex promotion manifest entry is invalid");
    const [, digest, path] = match;
    if ((path !== `nixos-config/${posix.normalize(path.slice("nixos-config/".length))}`
        && path !== `north/${posix.normalize(path.slice("north/".length))}`
        && path !== `beagle/${posix.normalize(path.slice("beagle/".length))}`)
        || path.includes("/../") || path.endsWith("/..") || path.includes("/./")
        || files.has(path)) {
      throw new Error("managed Codex promotion manifest path is invalid");
    }
    files.set(path, digest);
  }
  return { id, nixosRevision, northRevision, beagleRevision, files };
}

function captureActivePromotion(
  enforcementRoot: string,
  expectedOwnerUid: number,
): CapturedPromotion {
  const generationsRoot = realpathSync(resolve(enforcementRoot, "generations"));
  const deploymentsRoot = realpathSync(resolve(enforcementRoot, "deployments"));
  const activeSelector = resolve(enforcementRoot, "active");
  assertOwnedSelector(activeSelector, "managed Codex active promotion selector", expectedOwnerUid);
  // Resolve `active` exactly once. Every later read is rooted in this captured
  // generation, so a concurrent selector swap can only make live hooks differ.
  const generationRoot = realpathSync(activeSelector);
  if (dirname(generationRoot) !== generationsRoot)
    throw new Error("managed Codex active selector is outside the generation store");
  assertSealedDirectory(generationRoot, "managed Codex active generation", expectedOwnerUid);

  const currentSelector = resolve(generationRoot, "current");
  assertOwnedSelector(currentSelector, "managed Codex current promotion selector", expectedOwnerUid);
  const deploymentRoot = realpathSync(currentSelector);
  if (dirname(deploymentRoot) !== deploymentsRoot)
    throw new Error("managed Codex current selector is outside the deployment store");
  assertSealedDirectory(deploymentRoot, "managed Codex active deployment", expectedOwnerUid);

  const recordBytes = exactSealedBytes(
    resolve(generationRoot, "record"),
    "managed Codex promotion record",
    expectedOwnerUid,
    MAX_PROMOTION_RECORD_BYTES,
  );
  const recordSource = new TextDecoder("utf-8", { fatal: true }).decode(recordBytes);
  return {
    deploymentRoot,
    expectedOwnerUid,
    record: parsePromotionRecord(recordSource, basename(deploymentRoot)),
  };
}

function assertSealedPromotedSource(
  livePath: string,
  source: PromotedHookSource,
  promotion: CapturedPromotion,
): void {
  const manifestPath = `${source.repository}/${source.path}`;
  const revision = source.repository === "nixos-config"
    ? promotion.record.nixosRevision
    : source.repository === "north"
      ? promotion.record.northRevision
      : promotion.record.beagleRevision;
  const expectedPath = resolve(promotion.deploymentRoot, ...manifestPath.split("/"));
  if (dirname(expectedPath) === promotion.deploymentRoot
      || !expectedPath.startsWith(`${promotion.deploymentRoot}${sep}`)) {
    throw new Error(`${livePath} has an invalid sealed promotion path`);
  }
  const resolved = realpathSync(livePath);
  if (resolved !== expectedPath)
    throw new Error(`${livePath} does not resolve to the captured active deployment`);
  const recordedDigest = promotion.record.files.get(manifestPath);
  if (!recordedDigest)
    throw new Error(`${source.repository}@${revision}:${source.path} is absent from the promotion manifest`);
  const bytes = exactSealedBytes(
    resolved,
    `${source.repository}@${revision}:${source.path}`,
    promotion.expectedOwnerUid,
  );
  const digest = createHash("sha256").update(bytes).digest("hex");
  if (digest !== recordedDigest)
    throw new Error(`${source.repository}@${revision}:${source.path} differs from its promotion digest`);
}

function assertSealedPromotedHook(
  livePath: string,
  managedDir: string,
  promotion: CapturedPromotion,
): void {
  const hookPath = relative(resolve(managedDir), resolve(livePath));
  const source = PROMOTED_HOOK_SOURCES[hookPath];
  if (!source)
    throw new Error(`${livePath} has no allowed sealed promotion mapping`);
  assertSealedPromotedSource(livePath, source, promotion);
}

function managedCommandPaths(
  value: string,
  managedDir = CODEX_MANAGED_HOOKS_DIR,
): { env?: string; interpreter?: string; executable: string; script?: string } {
  const env = resolve(managedDir, "runtime/env");
  const prefix = `${env} -u BASH_ENV -u ENV `;
  if (!value.startsWith(prefix))
    throw new Error("managed Codex hook command does not scrub shell startup authority");
  const tokens = value.slice(prefix.length).split(" ");
  if (tokens.some((token) => !token))
    throw new Error("managed Codex hook command token sequence is not exact");
  const script = tokens.at(-1);
  const managedRoot = resolve(managedDir);
  if (!script || !script.startsWith(`${managedRoot}/`) || resolve(script) !== script)
    throw new Error("managed Codex hook command paths are outside the managed closure");
  const launcher = managedHookLauncher(relative(managedRoot, script));
  const bash = resolve(managedDir, "runtime/bash");
  const python = resolve(managedDir, "runtime/python3");
  const interpreter = launcher === "python3" ? python : bash;
  const expectedTokens = [
    ...(launcher === "bash" ? [`PATH=${managedBashPath(managedDir)}`] : []),
    interpreter,
    script,
  ];
  if (JSON.stringify(tokens) !== JSON.stringify(expectedTokens))
    throw new Error("managed Codex hook command token sequence is not exact");
  return { env, interpreter, executable: script, script };
}

function managedHookDependencies(
  script: string,
  managedDir: string,
): Array<{ hook: string; path: string; source: PromotedHookSource }> {
  const hook = relative(resolve(managedDir), resolve(script));
  const hookSource = PROMOTED_HOOK_SOURCES[hook];
  let repositoryRoot: string | undefined;
  return (PROMOTED_HOOK_DEPENDENCIES[hook] ?? []).map((source) => {
    const managedDependency = Object.entries(PROMOTED_HOOK_SOURCES)
      .find(([, candidate]) => candidate.repository === source.repository
        && candidate.path === source.path)?.[0];
    if (managedDependency) {
      return {
        hook: managedDependency,
        path: resolve(managedDir, managedDependency),
        source,
      };
    }
    if (!hookSource || hookSource.repository !== source.repository) {
      throw new Error(
        `managed Codex hook dependency ${source.repository}/${source.path} has no source root`,
      );
    }
    repositoryRoot ??= hookSource.path.split("/")
      .reduce((root) => dirname(root), realpathSync(script));
    return {
      hook: `${source.repository}/${source.path}`,
      path: resolve(repositoryRoot, ...source.path.split("/")),
      source,
    };
  });
}

/**
 * Pre-provider proof that Codex will load only the root-managed, exact hook
 * surface. The explicit installation argument exists for hermetic contract
 * tests; production always calls the fixed installed wrapper below.
 */
export function validateManagedCodexHookInstallation(
  installation: ManagedCodexHookInstallation,
): void {
  assertNixManagedFile(
    installation.requirementsPath,
    false,
    installation.nixStoreRoot,
  );
  const bytes = readFileSync(installation.requirementsPath);
  if (bytes.byteLength > MAX_REQUIREMENTS_BYTES)
    throw new Error("managed Codex requirements exceed the bounded size");
  const source = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  validateManagedCodexRequirements(source, installation.managedDir);
  const expected = expectedManagedCodexHooks(installation.managedDir);
  const commands = new Set(Object.values(expected)
    .flatMap((entries) => entries.flatMap((entry) =>
      entry.hooks.map((hook) => hook.command))));
  let promotion: CapturedPromotion | undefined;
  for (const commandLine of commands) {
    const paths = managedCommandPaths(commandLine, installation.managedDir);
    if (paths.env) assertNixManagedFile(paths.env, true, installation.nixStoreRoot);
    if (!paths.script) {
      assertNixManagedFile(paths.executable, true, installation.nixStoreRoot);
      continue;
    }
    assertNixManagedFile(paths.interpreter!, true, installation.nixStoreRoot);
    try {
      assertNixManagedFile(paths.script, false, installation.nixStoreRoot);
    } catch (cause) {
      promotion ??= captureActivePromotion(
        installation.enforcementRoot,
        installation.expectedOwnerUid,
      );
      try {
        assertSealedPromotedHook(paths.script, installation.managedDir, promotion);
      } catch (sealedCause) {
        throw new Error(`${paths.script} is neither Nix-supplied nor a proven sealed hook`, {
          cause: sealedCause,
        });
      }
    }
    for (const dependency of managedHookDependencies(paths.script, installation.managedDir)) {
      try {
        assertNixManagedFile(dependency.path, false, installation.nixStoreRoot);
      } catch (cause) {
        promotion ??= captureActivePromotion(
          installation.enforcementRoot,
          installation.expectedOwnerUid,
        );
        try {
          assertSealedPromotedSource(dependency.path, dependency.source, promotion);
        } catch (sealedCause) {
          throw new Error(`${dependency.path} is neither Nix-supplied nor a proven sealed hook dependency`, {
            cause: sealedCause,
          });
        }
      }
    }
  }
}

export interface ManagedCodexHookSupply {
  /** Managed-dir-relative name; the identity an operator reads in a failure. */
  hook: string;
  path: string;
  supply: "nix" | "sealed" | "unavailable";
  detail?: string;
}

export interface ManagedCodexHookReport {
  requirements: { path: string; ok: boolean; detail?: string };
  runtime: ManagedCodexHookSupply[];
  hooks: ManagedCodexHookSupply[];
}

function describe(error: unknown): string {
  if (!(error instanceof Error)) return String(error);
  const cause = (error as { cause?: unknown }).cause;
  return cause instanceof Error ? `${error.message}: ${cause.message}` : error.message;
}

/** The preflight's exact predicates, resolved per path so one bad hook cannot hide the rest. */
export function reportManagedCodexHookInstallation(
  installation: ManagedCodexHookInstallation,
): ManagedCodexHookReport {
  const requirements: ManagedCodexHookReport["requirements"] = (() => {
    try {
      assertNixManagedFile(installation.requirementsPath, false, installation.nixStoreRoot);
      const bytes = readFileSync(installation.requirementsPath);
      if (bytes.byteLength > MAX_REQUIREMENTS_BYTES)
        throw new Error("managed Codex requirements exceed the bounded size");
      validateManagedCodexRequirements(
        new TextDecoder("utf-8", { fatal: true }).decode(bytes), installation.managedDir,
      );
      return { path: installation.requirementsPath, ok: true };
    } catch (error) {
      return { path: installation.requirementsPath, ok: false, detail: describe(error) };
    }
  })();

  const runtime = new Map<string, ManagedCodexHookSupply>();
  const hooks = new Map<string, ManagedCodexHookSupply>();
  // Captured at most once: resolving `active` twice can straddle a promotion swap.
  let promotion: CapturedPromotion | undefined;
  const record = (
    into: Map<string, ManagedCodexHookSupply>,
    path: string,
    executable: boolean,
    hookName?: string,
    source?: PromotedHookSource,
  ) => {
    if (into.has(path)) return;
    const hook = hookName ?? (relative(resolve(installation.managedDir), resolve(path)) || path);
    try {
      assertNixManagedFile(path, executable, installation.nixStoreRoot);
      into.set(path, { hook, path, supply: "nix" });
      return;
    } catch (nixCause) {
      if (executable) {
        into.set(path, { hook, path, supply: "unavailable", detail: describe(nixCause) });
        return;
      }
      try {
        promotion ??= captureActivePromotion(
          installation.enforcementRoot, installation.expectedOwnerUid,
        );
        if (source) assertSealedPromotedSource(path, source, promotion);
        else assertSealedPromotedHook(path, installation.managedDir, promotion);
        into.set(path, { hook, path, supply: "sealed" });
      } catch (sealedCause) {
        into.set(path, {
          hook,
          path,
          supply: "unavailable",
          detail: `${describe(nixCause)}; ${describe(sealedCause)}`,
        });
      }
    }
  };

  const expected = expectedManagedCodexHooks(installation.managedDir);
  const commands = new Set(Object.values(expected)
    .flatMap((entries) => entries.flatMap((entry) => entry.hooks.map((hook) => hook.command))));
  for (const commandLine of commands) {
    let paths: ReturnType<typeof managedCommandPaths>;
    try {
      paths = managedCommandPaths(commandLine, installation.managedDir);
    } catch (error) {
      hooks.set(commandLine, {
        hook: commandLine, path: commandLine, supply: "unavailable", detail: describe(error),
      });
      continue;
    }
    if (paths.env) record(runtime, paths.env, true);
    if (paths.interpreter) record(runtime, paths.interpreter, true);
    record(hooks, paths.executable, paths.script === undefined);
    if (paths.script) {
      for (const dependency of managedHookDependencies(paths.script, installation.managedDir)) {
        record(hooks, dependency.path, false, dependency.hook, dependency.source);
      }
    }
  }
  const bySupply = (entries: Iterable<ManagedCodexHookSupply>) =>
    [...entries].sort((left, right) => left.hook.localeCompare(right.hook));
  return {
    requirements,
    runtime: bySupply(runtime.values()),
    hooks: bySupply(hooks.values()),
  };
}

/**
 * Repeated immediately before process spawn to close the admission / execution
 * filesystem race. Failure remains a proved-unsent provider preaccept error.
 */
export function assertInstalledManagedCodexHooks(): void {
  try {
    validateManagedCodexHookInstallation({
      requirementsPath: CODEX_MANAGED_REQUIREMENTS,
      managedDir: CODEX_MANAGED_HOOKS_DIR,
      nixStoreRoot: NIX_STORE_ROOT,
      enforcementRoot: NORTH_ENFORCEMENT_ROOT,
      expectedOwnerUid: 0,
    });
  } catch (cause) {
    throw providerPreacceptError("openai_managed_hooks_contract_unavailable", { cause });
  }
}
