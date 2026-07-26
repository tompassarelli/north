import { execFileSync } from "node:child_process";
import { accessSync, constants, realpathSync } from "node:fs";
import { resolve } from "node:path";

export class TrustedGitOracleError extends Error {
  constructor(readonly code: "execution_failed" | "unexpected_result", options?: ErrorOptions) {
    super(`trusted Git oracle ${code.replace("_", " ")}`, options);
    this.name = "TrustedGitOracleError";
  }
}

function trustedStoreExecutable(
  candidates: readonly (string | undefined)[],
  pattern: RegExp,
  label: string,
): string {
  for (const candidate of candidates) {
    if (!candidate) continue;
    try {
      const target = realpathSync(candidate);
      if (!pattern.test(target)) continue;
      accessSync(target, constants.X_OK);
      return target;
    } catch {
      // A candidate is evidence only after canonical-path and executable proof.
    }
  }
  throw new Error(`trusted Nix-store ${label} executable unavailable`);
}

/** A username safe to interpolate into a fixed system-profile path. */
function safeProfileUser(user: string | undefined): string | undefined {
  return user && /^[a-z_][a-z0-9_-]*$/.test(user) ? user : undefined;
}

/**
 * Fixed, non-arbitrary entry points to a NixOS-immutable Git, most trusted
 * first: the wrapper's explicit injection, the root-managed system profile, the
 * root-managed home-manager per-user profile, then the caller's own Nix
 * profile. These are ENTRY hints only — trust is never granted by the location.
 * Every candidate must still pass the canonical `/nix/store` + executable proof
 * below, so a repointed profile symlink or a shim can only ever resolve to the
 * immutable store or be rejected. Ambient `$PATH` is deliberately absent.
 */
function defaultTrustedGitPointers(): readonly (string | undefined)[] {
  const home = process.env.HOME;
  const user = safeProfileUser(process.env.USER);
  return [
    process.env.NORTH_GIT_BIN,
    "/run/current-system/sw/bin/git",
    user ? `/etc/profiles/per-user/${user}/bin/git` : undefined,
    home ? `${home}/.nix-profile/bin/git` : undefined,
  ];
}

/**
 * Resolve a Git whose real canonical executable lives in the immutable Nix
 * store. Managed spawns do not always inherit the wrapper's NORTH_GIT_BIN, so
 * the default candidates also include the real NixOS current-system/profile
 * layout — but only as entry hints. Ambient `$PATH` and writable shim locations
 * are never candidates, and every accepted path is proven to canonicalize into
 * `/nix/store` and be executable. Absent that proof, resolution fails closed.
 */
export function trustedGitExecutable(
  candidates: readonly (string | undefined)[] = defaultTrustedGitPointers(),
): string {
  return trustedStoreExecutable(
    candidates,
    /^\/nix\/store\/[0-9a-z]{32}-git(?:-[^/]+)?\/bin\/git$/,
    "Git",
  );
}

/**
 * Fixed, non-arbitrary entry points to a NixOS-immutable Babashka, most trusted
 * first: the wrapper's explicit peer/MCP/CLI injections, then the same
 * root-managed system-profile, home-manager per-user profile, and per-user Nix
 * profile layout as Git. Managed children do not always inherit the wrapper's
 * NORTH_PEER_BB/NORTH_MCP_BB, so the `bb` powering the durable North mail feed
 * must be discoverable from these immutable pointers. As with Git these are ENTRY
 * hints only — every candidate still passes the canonical `/nix/store` +
 * executable proof below, so a repointed profile symlink or writable shim can
 * only resolve into the immutable store or be rejected. Ambient `$PATH` is
 * deliberately absent, and absence of proof stays fail-closed.
 */
function defaultTrustedBabashkaPointers(): readonly (string | undefined)[] {
  const home = process.env.HOME;
  const user = safeProfileUser(process.env.USER);
  return [
    process.env.NORTH_PEER_BB,
    process.env.NORTH_MCP_BB,
    process.env.NORTH_BB,
    "/run/current-system/sw/bin/bb",
    user ? `/etc/profiles/per-user/${user}/bin/bb` : undefined,
    home ? `${home}/.nix-profile/bin/bb` : undefined,
  ];
}

/**
 * Resolve the Babashka that runs North's own coordinator/live-feed scripts, whose
 * real canonical executable lives in the immutable Nix store. Unlike the provider
 * CLI, `bb` only interprets North's version-controlled `.clj` and its exact
 * store hash is behaviorally irrelevant to trust, so any canonical
 * `/nix/store/*-babashka/bin/bb` that is executable is safely discoverable from
 * the entry hints above. Ambient `$PATH` and writable shim locations are never
 * candidates; absent the `/nix/store` + X_OK proof, resolution fails closed.
 */
export function trustedNorthBabashkaExecutable(
  candidates: readonly (string | undefined)[] = defaultTrustedBabashkaPointers(),
): string {
  return trustedStoreExecutable(
    candidates,
    /^\/nix\/store\/[0-9a-z]{32}-babashka(?:-[^/]+)?\/bin\/bb$/,
    "Babashka",
  );
}

/**
 * Fixed, non-arbitrary entry points to the NixOS-immutable managed Codex, most
 * trusted first: the wrapper's explicit NORTH_MANAGED_CODEX_BIN injection, then
 * the same root-managed system-profile, home-manager per-user profile, and
 * per-user Nix profile layout as Git/Babashka. Managed children spawned outside
 * the wrapper (e.g. a checkout-driven `bin/north`) do not inherit
 * NORTH_MANAGED_CODEX_BIN, so preflight needs these immutable pointers too. As
 * with Git/Babashka these are ENTRY hints only — trust is never granted by the
 * location. Every candidate still passes the canonical `/nix/store` + executable
 * proof below, so a repointed profile symlink or writable shim can only ever
 * resolve into the immutable store or be rejected. Ambient `$PATH` is
 * deliberately absent.
 */
function defaultTrustedCodexPointers(): readonly (string | undefined)[] {
  const home = process.env.HOME;
  const user = safeProfileUser(process.env.USER);
  return [
    process.env.NORTH_MANAGED_CODEX_BIN,
    "/run/current-system/sw/bin/codex",
    user ? `/etc/profiles/per-user/${user}/bin/codex` : undefined,
    home ? `${home}/.nix-profile/bin/codex` : undefined,
  ];
}

/**
 * Managed provider execution never consults NORTH_CODEX_BIN or ambient PATH:
 * the managed Codex IS the billed provider, so its exact build must still prove
 * canonical residence in the immutable Nix store. The wrapper's
 * NORTH_MANAGED_CODEX_BIN injection remains the highest-priority entry hint,
 * but managed children that never inherit it (a checkout-driven dispatch) fall
 * back to the same root-managed system/profile pointers Git and Babashka use.
 * Every candidate — wrapper-injected or profile-pointed — is proven by the
 * canonical `/nix/store` + X_OK check below before it is trusted, so a
 * repointed profile symlink or forged shim can only resolve into the immutable
 * store or be rejected; substituting a non-store `codex` is never possible.
 */
export function trustedManagedCodexExecutable(
  candidates: readonly (string | undefined)[] = defaultTrustedCodexPointers(),
): string {
  return trustedStoreExecutable(
    candidates,
    /^\/nix\/store\/[0-9a-z]{32}-[^/]*codex[^/]*\/bin\/codex$/,
    "Codex",
  );
}

/**
 * Git root/branch discovery is an authority oracle. Give it a closed
 * environment so GIT_DIR, GIT_WORK_TREE, config include paths, ceiling
 * directories, and repository-replacement variables cannot redirect it.
 */
export function gitOracleEnvironment(): NodeJS.ProcessEnv {
  return {
    HOME: "/homeless-shelter",
    PATH: "",
    LC_ALL: "C",
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_CONFIG_GLOBAL: "/dev/null",
    GIT_CONFIG_SYSTEM: "/dev/null",
    // Pin discovery's upper boundary so Git 2.54 emits the same exact
    // non-repository diagnostic on mounted and ordinary filesystems.
    GIT_CEILING_DIRECTORIES: "/",
  };
}

export function trustedGitProjectRoot(
  cwd: string,
  gitExecutable = trustedGitExecutable(),
): string {
  const canonicalCwd = realpathSync(cwd);
  try {
    const root = execFileSync(
      gitExecutable,
      ["-C", canonicalCwd, "rev-parse", "--show-toplevel"],
      {
        encoding: "utf8",
        env: gitOracleEnvironment(),
        timeout: 2_000,
        maxBuffer: 16 * 1024,
        stdio: ["ignore", "pipe", "pipe"],
      },
    ).trim();
    if (!root) throw new TrustedGitOracleError("unexpected_result");
    return realpathSync(root);
  } catch (cause) {
    if (cause instanceof TrustedGitOracleError) throw cause;
    const error = cause as NodeJS.ErrnoException & {
      status?: number | null;
      stderr?: Buffer | string;
    };
    const stderr = String(error.stderr ?? "").trim();
    // A real, canonical cwd need not be a Git checkout. This one exact
    // C-locale result is absence, not oracle failure. Every execution/config/
    // ownership error remains fatal.
    if (error.status === 128
        && /^fatal: not a git repository \(or any of the parent directories\): \.git$/.test(stderr)) {
      return canonicalCwd;
    }
    throw new TrustedGitOracleError("execution_failed", { cause });
  }
}

/**
 * The exact Git metadata roots a checkout needs in order to COMMIT: its own
 * `--git-dir` (index/HEAD/logs) and its `--git-common-dir` (objects/refs, which
 * a linked worktree shares with the main checkout). Both are returned canonical
 * and deduplicated; an ordinary repository yields one path.
 *
 * Codex's workspace-write sandbox makes the workspace writable but keeps the
 * Git metadata directory read-only unless it is named as a writable root, so a
 * managed lane cannot commit without this. Observed 2026-07-26 through the
 * managed app-server `command/exec` seam: `git commit` under
 * `writableRoots: []` fails `.git/index.lock: Read-only file system`; the same
 * command with the git dir as a writable root exits 0.
 */
export function trustedGitMetadataRoots(
  cwd: string,
  gitExecutable = trustedGitExecutable(),
): string[] {
  const canonicalCwd = realpathSync(cwd);
  const read = (flag: "--git-dir" | "--git-common-dir"): string | undefined => {
    let value: string;
    try {
      value = execFileSync(gitExecutable, ["-C", canonicalCwd, "rev-parse", flag], {
        encoding: "utf8",
        env: gitOracleEnvironment(),
        timeout: 2_000,
        maxBuffer: 16 * 1024,
        stdio: ["ignore", "pipe", "ignore"],
      }).trim();
    } catch { return undefined; }
    if (!value) return undefined;
    try { return realpathSync(resolve(canonicalCwd, value)); }
    catch { return undefined; }
  };
  const roots: string[] = [];
  for (const flag of ["--git-dir", "--git-common-dir"] as const) {
    const root = read(flag);
    if (root && !roots.includes(root)) roots.push(root);
  }
  return roots.sort();
}

export function trustedGitBranchName(
  projectRoot: string,
  gitExecutable = trustedGitExecutable(),
): string {
  return execFileSync(
    gitExecutable,
    ["-C", projectRoot, "branch", "--show-current"],
    {
      encoding: "utf8",
      env: gitOracleEnvironment(),
      timeout: 2_000,
      maxBuffer: 16 * 1024,
      stdio: ["ignore", "pipe", "ignore"],
    },
  ).trim();
}
