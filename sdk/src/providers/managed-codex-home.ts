import {
  chmodSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, realpathSync,
  rmSync, symlinkSync, writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { canonicalGlobalAgents } from "../harness";
import { scrubManagedNonclientReceiptEnvironment } from "./managed-nonclient-receipt";

const MANAGED_HOME_PREFIX = "north-managed-codex-";
const MAX_AUTH_BYTES = 1024 * 1024;
/** Preserved launch homes retained for review; older ones are pruned at prepare. */
const MAX_PRESERVED_HOMES = 50;
const activeHomes = new Set<string>();
let exitCleanupInstalled = false;

export interface PreparedManagedCodexHome {
  env: NodeJS.ProcessEnv;
  home: string;
  accountHome: string;
  dispose(): void;
}

function isMissing(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

/**
 * Managed launch homes live under the North state root, NOT `/tmp`. Two
 * defects made `mkdtemp(os.tmpdir())` the wrong root:
 *
 *  1. Rollouts are written to `$CODEX_HOME/sessions`, so `rm -rf`ing the home
 *     on dispose destroyed the only transcript of every managed Codex run —
 *     every run was forensically unreviewable by construction (regression
 *     traced to fb4c6cc).
 *  2. Codex refuses to materialize its PATH-alias helper binaries (apply_patch)
 *     under a temporary-directory CODEX_HOME: "Refusing to create helper
 *     binaries under temporary dir /tmp" (observed on every managed launch).
 *
 * The home is still per-launch, 0700, and carries no interactive account state.
 */
function managedHomeRoot(env: NodeJS.ProcessEnv): string {
  const explicit = env.NORTH_MANAGED_CODEX_HOME_ROOT?.trim();
  if (explicit) return resolve(explicit);
  const home = env.HOME?.trim() || process.env.HOME?.trim() || homedir();
  if (!home) throw new Error("managed Codex home root is unresolvable");
  return resolve(home, ".local/state/north/managed-codex");
}

function hasRollout(home: string): boolean {
  const sessions = join(home, "sessions");
  try { return readdirSync(sessions).length > 0; }
  catch { return false; }
}

/**
 * Settle one launch home. A home that captured a rollout is PRESERVED for
 * review with its volatile and credential-bearing parts removed (the auth
 * symlink, the private sqlite root, caches); a home that captured nothing is
 * removed whole. Preservation is the point: a lane that died mid-turn is
 * diagnosable only from its rollout.
 */
function settleHome(home: string, preserve: boolean): void {
  if (!activeHomes.delete(home)) return;
  if (!preserve || !hasRollout(home)) {
    rmSync(home, { recursive: true, force: true });
    return;
  }
  let entries: string[];
  try { entries = readdirSync(home); }
  catch { return; }
  for (const entry of entries) {
    if (entry === "sessions" || entry === "north-launch.json") continue;
    try { rmSync(join(home, entry), { recursive: true, force: true }); } catch {}
  }
}

function trackHome(home: string): void {
  activeHomes.add(home);
  if (exitCleanupInstalled) return;
  exitCleanupInstalled = true;
  process.once("exit", () => {
    for (const active of [...activeHomes]) {
      // Exit is the abnormal path (a killed lane): preserve whatever rollout
      // landed rather than deleting the evidence of the death.
      try { settleHome(active, true); } catch { /* process exit cannot recover cleanup */ }
    }
  });
}

/** Bound the preserved-rollout archive without touching a live launch home. */
function prunePreservedHomes(root: string): void {
  let entries: string[];
  try { entries = readdirSync(root).filter((name) => name.startsWith(MANAGED_HOME_PREFIX)); }
  catch { return; }
  if (entries.length <= MAX_PRESERVED_HOMES) return;
  const stale = entries.sort().slice(0, entries.length - MAX_PRESERVED_HOMES);
  for (const name of stale) {
    const path = join(root, name);
    if (activeHomes.has(path)) continue;
    try { rmSync(path, { recursive: true, force: true }); } catch {}
  }
}

/**
 * Attribution for a preserved rollout, written at dispose (never at launch:
 * the launch home stays exactly auth + canonical AGENTS + sqlite, and Codex
 * has already exited by the time this lands).
 */
function writeLaunchReceipt(home: string, env: NodeJS.ProcessEnv, accountHome: string): void {
  try {
    writeFileSync(join(home, "north-launch.json"), `${JSON.stringify({
      accountHome,
      agentId: env.AGENT_ID ?? null,
      runId: env.NORTH_RUN_ID ?? null,
      threadId: env.NORTH_THREAD_ID ?? null,
      settledAt: new Date().toISOString(),
    }, null, 2)}\n`, { mode: 0o600 });
  } catch { /* attribution is best-effort; the rollout is the evidence */ }
}

function exactPrivateAuthFile(accountHome: string): string {
  const auth = resolve(accountHome, "auth.json");
  let info;
  try { info = lstatSync(auth); }
  catch (cause) {
    if (isMissing(cause)) throw new Error("managed Codex account authentication is missing", { cause });
    throw new Error("managed Codex account authentication is uninspectable", { cause });
  }
  if (!info.isFile() || info.isSymbolicLink())
    throw new Error("managed Codex account authentication is not a regular file");
  if (info.size <= 0 || info.size > MAX_AUTH_BYTES)
    throw new Error("managed Codex account authentication has invalid size");
  if ((info.mode & 0o077) !== 0)
    throw new Error("managed Codex account authentication is not private");
  const canonical = realpathSync(auth);
  if (canonical !== auth)
    throw new Error("managed Codex account authentication escapes its account home");
  return canonical;
}

/**
 * Materialize the state boundary for one managed Codex launch. The selected
 * account remains the durable authentication store, but its mutable interactive
 * config, rules, hooks, plugins, skills, history, caches, and sqlite databases
 * are never a managed provider home. The launch sees only the exact auth file,
 * the canonical provider-neutral AGENTS source, and a new private sqlite root.
 */
export function prepareManagedCodexHome(
  accountEnv: NodeJS.ProcessEnv,
): PreparedManagedCodexHome {
  const accountHomeValue = accountEnv.CODEX_HOME?.trim();
  if (!accountHomeValue) throw new Error("managed Codex account home is missing");
  const accountHome = realpathSync(accountHomeValue);
  const auth = exactPrivateAuthFile(accountHome);
  const agents = canonicalGlobalAgents(accountEnv);
  if (!agents) throw new Error("managed Codex canonical AGENTS authority is disabled");

  const root = managedHomeRoot(accountEnv);
  mkdirSync(root, { recursive: true, mode: 0o700 });
  prunePreservedHomes(root);
  // Sortable stamp first: the archive prunes oldest-first by name, and a human
  // reading the directory sees launches in order.
  const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\..*$/, "");
  const home = mkdtempSync(join(root, `${MANAGED_HOME_PREFIX}${stamp}-`));
  trackHome(home);
  let disposed = false;
  const dispose = (): void => {
    if (disposed) return;
    disposed = true;
    if (hasRollout(home)) writeLaunchReceipt(home, accountEnv, accountHome);
    settleHome(home, true);
  };
  try {
    chmodSync(home, 0o700);
    mkdirSync(join(home, "sqlite"), { mode: 0o700 });
    chmodSync(join(home, "sqlite"), 0o700);
    symlinkSync(auth, join(home, "auth.json"));
    symlinkSync(agents.realpath, join(home, "AGENTS.md"));
    const env = {
      ...accountEnv,
      CODEX_HOME: home,
      CODEX_SQLITE_HOME: join(home, "sqlite"),
      CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED: "1",
    };
    scrubManagedNonclientReceiptEnvironment(env);
    return {
      env,
      home,
      accountHome,
      dispose,
    };
  } catch (error) {
    dispose();
    throw error;
  }
}
