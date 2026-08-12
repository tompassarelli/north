import { createHash } from "node:crypto";
import {
  accessSync, closeSync, constants, fstatSync, lstatSync, openSync, readFileSync,
  realpathSync, statSync,
} from "node:fs";
import { basename, dirname, posix, relative, resolve, sep } from "node:path";
import { providerPreacceptError } from "./types";

export const CODEX_MANAGED_REQUIREMENTS = "/etc/codex/requirements.toml";
export const CODEX_MANAGED_HOOKS_DIR = "/etc/codex/hooks";
const MAX_REQUIREMENTS_BYTES = 128 * 1024;
const NORTH_ENFORCEMENT_ROOT = "/var/lib/north-enforcement";
const NIX_STORE_ROOT = "/nix/store";
const MAX_PROMOTION_RECORD_BYTES = 128 * 1024;
const MAX_PROMOTION_FILES = 1024;

type PromotedRepository = "north" | "beagle";

interface PromotedHookSource {
  repository: PromotedRepository;
  path: string;
}

const PROMOTED_HOOK_SOURCES: Readonly<Record<string, PromotedHookSource>> = {
  "agent-spawn-guard.sh": {
    repository: "north",
    path: "profiles/tom/hooks/agent-spawn-guard.sh",
  },
  "beagle-session-start.sh": {
    repository: "beagle",
    path: "integrations/north/hooks/beagle-session-start.sh",
  },
  "corpus-scan-guard.sh": {
    repository: "north",
    path: "profiles/tom/hooks/corpus-scan-guard.sh",
  },
  "launch-critical-worktree-guard.sh": {
    repository: "north",
    path: "profiles/tom/hooks/launch-critical-worktree-guard.sh",
  },
  "logcompress-hook.js": {
    repository: "north",
    path: "profiles/tom/hooks/logcompress-hook.js",
  },
  "tripwire-guard.sh": {
    repository: "north",
    path: "profiles/tom/hooks/tripwire-guard.sh",
  },
};

interface PromotionRecord {
  id: string;
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

const command = (
  name: string,
  timeout = 10,
  managedDir = CODEX_MANAGED_HOOKS_DIR,
  interpreter: "bash" | "node" = "bash",
): ManagedCommandHook => ({
  type: "command",
  command: [
    resolve(managedDir, "runtime/env"),
    "-u", "BASH_ENV",
    "-u", "ENV",
    resolve(managedDir, `runtime/${interpreter}`),
    resolve(managedDir, name),
  ].join(" "),
  timeout,
});

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
 * launch-critical-worktree-guard.sh is wired on BOTH authoring matchers on
 * purpose: apply_patch carries tool_input.file_path, Bash carries
 * tool_input.command, and enforcement on one entrance is not enforcement.
 */
export function expectedManagedCodexHooks(
  managedDir = CODEX_MANAGED_HOOKS_DIR,
): Record<
  "SessionStart" | "SubagentStart" | "PreToolUse" | "PostToolUse" | "Stop",
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
    PreToolUse: [
      {
        matcher: "^(Agent|Task|Workflow)$",
        hooks: [command("agent-spawn-guard.sh", 10, managedDir)],
      },
      {
        matcher: "^(Edit|Write|MultiEdit|apply_patch)$",
        hooks: [
          command("firn-guard.sh", 10, managedDir),
          command("launch-critical-worktree-guard.sh", 10, managedDir),
        ],
      },
      {
        matcher: "^Bash$",
        hooks: [
          command("agent-spawn-guard.sh", 10, managedDir),
          command("tripwire-guard.sh", 10, managedDir),
          command("firn-guard.sh", 10, managedDir),
          command("launch-critical-worktree-guard.sh", 10, managedDir),
          command("corpus-scan-guard.sh", 10, managedDir),
        ],
      },
    ],
    PostToolUse: [
      {
        matcher: "^Bash$",
        hooks: [
          command("logcompress-hook.js", 10, managedDir, "node"),
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
  for (const [event, entries] of Object.entries(expected))
    exact(parsed.hooks?.[event], entries, `managed Codex ${event}`);
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
    throw new Error("managed Codex promotion record framing is invalid");
  const framed = source.endsWith("\n") ? source.slice(0, -1) : source;
  const lines = framed.split("\n");
  const exactHeader = (index: number, name: string, pattern: RegExp): string => {
    const line = lines[index] ?? "";
    const match = line.match(pattern);
    if (!match?.[1]) throw new Error(`managed Codex promotion record ${name} is invalid`);
    return match[1];
  };
  if (lines[0] !== "FORMAT north-enforcement-promote/v1")
    throw new Error("managed Codex promotion record format is invalid");
  const id = exactHeader(1, "ID", /^ID (north-[0-9a-f]{40}\.beagle-[0-9a-f]{40})$/);
  const northRevision = exactHeader(2, "NORTH_REV", /^NORTH_REV ([0-9a-f]{40})$/);
  const beagleRevision = exactHeader(3, "BEAGLE_REV", /^BEAGLE_REV ([0-9a-f]{40})$/);
  exactHeader(4, "PREVIOUS", /^PREVIOUS (north-[0-9a-f]{40}\.beagle-[0-9a-f]{40})$/);
  exactHeader(5, "WHO", /^WHO ([A-Za-z_][A-Za-z0-9_.-]{0,127})$/);
  exactHeader(6, "WHEN", /^WHEN (\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z)$/);
  const why = exactHeader(7, "WHY", /^WHY (.{1,4096})$/);
  if (why.includes("\0")) throw new Error("managed Codex promotion record WHY is invalid");
  const expectedId = `north-${northRevision}.beagle-${beagleRevision}`;
  if (id !== expectedId || id !== deploymentName)
    throw new Error("managed Codex promotion record revision mapping is invalid");

  const manifestLines = lines.slice(8);
  if (manifestLines.length < 1 || manifestLines.length > MAX_PROMOTION_FILES)
    throw new Error("managed Codex promotion manifest size is invalid");
  const files = new Map<string, string>();
  for (const line of manifestLines) {
    const match = line.match(/^FILE ([0-9a-f]{64})  ([A-Za-z0-9._/-]{1,512})$/);
    if (!match?.[1] || !match[2])
      throw new Error("managed Codex promotion manifest entry is invalid");
    const [, digest, path] = match;
    if ((path !== `north/${posix.normalize(path.slice("north/".length))}`
        && path !== `beagle/${posix.normalize(path.slice("beagle/".length))}`)
        || path.includes("/../") || path.endsWith("/..") || path.includes("/./")
        || files.has(path)) {
      throw new Error("managed Codex promotion manifest path is invalid");
    }
    files.set(path, digest);
  }
  return { id, northRevision, beagleRevision, files };
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

function assertSealedPromotedHook(
  livePath: string,
  managedDir: string,
  promotion: CapturedPromotion,
): void {
  const hookPath = relative(resolve(managedDir), resolve(livePath));
  const source = PROMOTED_HOOK_SOURCES[hookPath];
  if (!source)
    throw new Error(`${livePath} has no allowed sealed promotion mapping`);
  const manifestPath = `${source.repository}/${source.path}`;
  const revision = source.repository === "north"
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

function managedCommandPaths(
  value: string,
  managedDir = CODEX_MANAGED_HOOKS_DIR,
): { env: string; interpreter: string; script: string } {
  const env = resolve(managedDir, "runtime/env");
  const prefix = `${env} -u BASH_ENV -u ENV `;
  if (!value.startsWith(prefix))
    throw new Error("managed Codex hook command does not scrub shell startup authority");
  const tokens = value.slice(prefix.length).split(" ");
  if (tokens.length !== 2 || tokens.some((token) => !token))
    throw new Error("managed Codex hook command token sequence is not exact");
  const [interpreter, script] = tokens as [string, string];
  const allowedInterpreters = new Set([
    resolve(managedDir, "runtime/bash"),
    resolve(managedDir, "runtime/node"),
  ]);
  if (!allowedInterpreters.has(interpreter)
      || !script.startsWith(`${resolve(managedDir)}/`)
      || resolve(script) !== script) {
    throw new Error("managed Codex hook command paths are outside the managed closure");
  }
  return { env, interpreter, script };
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
    assertNixManagedFile(paths.env, true, installation.nixStoreRoot);
    assertNixManagedFile(paths.interpreter, true, installation.nixStoreRoot);
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
  const record = (into: Map<string, ManagedCodexHookSupply>, path: string, executable: boolean) => {
    if (into.has(path)) return;
    const hook = relative(resolve(installation.managedDir), resolve(path)) || path;
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
        assertSealedPromotedHook(path, installation.managedDir, promotion);
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
    record(runtime, paths.env, true);
    record(runtime, paths.interpreter, true);
    record(hooks, paths.script, false);
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
