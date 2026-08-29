// Programmatic parity for the interactive PreToolUse authoring guards.
// ============================================================================
// THE SMOKING GUN: harness.ts builds worker Options with a programmatic `hooks`
// object and settingSources:[], so the SDK never loads ~/.claude/settings.json —
// every north-dispatched worker ran with ZERO authoring guards.
//
// We deliberately keep settingSources empty (enabling one would drag the user-settings surface —
// permissions, MCP, statusline, plugins — into workers). Instead we RE-EXECUTE the
// same guard scripts the interactive matchers run, in-process, and translate their
// CLI-hook-protocol output into the SDK's HookJSONOutput. One source of truth for
// the guard LOGIC (the .sh scripts), two callers (Claude Code CLI + this harness).
//
// Guard-script result protocol:
//   - stdout JSON with hookSpecificOutput.permissionDecision === "deny"
//       -> DENY, reason = permissionDecisionReason
//       (firn-system-policy)
//   - process exit code 2 -> DENY, reason = stderr
//       (tripwire-guard)
//   - unavailable guards remain advisory; their explicit denials still win.
// ============================================================================
import {
  spawn, type ChildProcess,
} from "node:child_process";
import {
  closeSync, constants, existsSync, mkdtempSync, openSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import {
  parseInvocationObservationReceipt,
  type InvocationObservationReceipt,
} from "./invocation-observation";
import { parseStrictJson } from "./strict-json";

// Provider workers consume the current immutable North generation. The exact
// override exists for hermetic tests and alternate state roots, not as another
// hook inventory.
export function authoringHooksDir(env: NodeJS.ProcessEnv = process.env): string {
  const override = env.NORTH_AGENT_PROVIDER_HOOKS?.trim();
  if (override) return resolve(override);
  const stateRoot = env.NORTH_AGENT_STATE_ROOT?.trim()
    || resolve(env.HOME ?? "", ".local", "state", "north", "agents");
  return resolve(stateRoot, "current", "provider-hooks");
}

export const HOOKS_DIR = authoringHooksDir();

export type GuardDecision =
  | { decision: "deny"; reason: string; observation?: InvocationObservationReceipt }
  | { decision: "allow"; observation?: InvocationObservationReceipt }
  | { decision: "unavailable"; reason: string };

const ALLOW: GuardDecision = { decision: "allow" };
const GUARD_OUTPUT_MAX_BYTES = 64 * 1024;
const GUARD_TERM_GRACE_MS = 100;
const GUARD_KILL_GRACE_MS = 100;
const GUARD_POSIX_PROCESS_GROUP = process.platform !== "win32";

function signalGuardProcessTree(
  child: ChildProcess,
  signal: NodeJS.Signals,
): void {
  if (GUARD_POSIX_PROCESS_GROUP && child.pid !== undefined) {
    try {
      process.kill(-child.pid, signal);
      return;
    } catch {
      // The process group may already be gone; fall back to the direct child.
    }
  }
  try { child.kill(signal); } catch { /* already gone */ }
}

function preparedGuardInput(hookInput: unknown): { fd: number; dispose(): void } {
  const directory = mkdtempSync(join(tmpdir(), "north-guard-input-"));
  const path = join(directory, "hook.json");
  let fd: number | undefined;
  try {
    const serialized = JSON.stringify(hookInput);
    writeFileSync(path, serialized === undefined ? "" : serialized, {
      encoding: "utf8",
      flag: "wx",
      mode: 0o600,
    });
    fd = openSync(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
    return {
      fd,
      dispose() {
        if (fd !== undefined) {
          try { closeSync(fd); } catch {}
          fd = undefined;
        }
        try { rmSync(directory, { recursive: true, force: true }); } catch {}
      },
    };
  } catch (error) {
    if (fd !== undefined) try { closeSync(fd); } catch {}
    try { rmSync(directory, { recursive: true, force: true }); } catch {}
    throw error;
  }
}

// Resolve a generated provider adapter by name to its absolute path IF it exists.
export function resolveGuard(name: string): string | null {
  const p = resolve(HOOKS_DIR, name);
  return existsSync(p) ? p : null;
}

export function resolveManagedGuardChain(
  names: readonly string[],
  hooksDir = HOOKS_DIR,
): string[] {
  return names.flatMap((name) => {
    const path = resolve(hooksDir, name);
    return existsSync(path) ? [path] : [];
  });
}

// Run one guard script: exec it, feed the hook input as JSON on stdin exactly as the
// CLI hook protocol delivers it ({tool_name, tool_input:{file_path|command,...},
// cwd, session_id, ...}), and interpret the result per the protocol above.
// Inherits the parent process env (default spawn behavior — NOT overridden) so the
// guards see BEAGLE_STORE_LOG, AGENT_NO_AUTHORING_HOOKS, and the rest of the killswitch env.
export function runGuardScript(
  scriptPath: string,
  hookInput: unknown,
  timeoutMs = 10000,
  env?: NodeJS.ProcessEnv,
): Promise<GuardDecision> {
  return new Promise((resolveP) => {
    let child: ChildProcess;
    let input: ReturnType<typeof preparedGuardInput> | undefined;
    try {
      input = preparedGuardInput(hookInput);
      // Execute the script directly so its shebang picks the interpreter. Callers
      // may add per-lane topology without mutating shared process.env; otherwise
      // Node inherits the parent environment unchanged.
      child = spawn(scriptPath, [], {
        // Bun can lose nested child-pipe stdin. A prewritten private descriptor
        // makes exact hook bytes available before process construction and EOF
        // deterministic without exposing them through argv or environment.
        stdio: [input.fd, "pipe", "pipe"],
        detached: GUARD_POSIX_PROCESS_GROUP,
        ...(env ? { env } : {}),
      });
    } catch {
      input?.dispose();
      return resolveP({ decision: "unavailable", reason: "guard process spawn failed" });
    }
    const stdoutChunks: Buffer[] = [];
    const stderrChunks: Buffer[] = [];
    let outputBytes = 0;
    let settled = false;
    let terminating = false;
    let termTimer: ReturnType<typeof setTimeout> | undefined;
    let killTimer: ReturnType<typeof setTimeout> | undefined;
    const finish = (d: GuardDecision) => {
      if (settled) return;
      settled = true;
      input?.dispose();
      input = undefined;
      clearTimeout(timer);
      if (termTimer) clearTimeout(termTimer);
      if (killTimer) clearTimeout(killTimer);
      resolveP(d);
    };
    const terminate = (decision: GuardDecision) => {
      if (terminating || settled) return;
      terminating = true;
      clearTimeout(timer);
      signalGuardProcessTree(child, "SIGTERM");
      termTimer = setTimeout(() => {
        signalGuardProcessTree(child, "SIGKILL");
        child.stdin?.destroy();
        child.stdout?.destroy();
        child.stderr?.destroy();
        killTimer = setTimeout(() => finish(decision), GUARD_KILL_GRACE_MS);
      }, GUARD_TERM_GRACE_MS);
    };

    const timer = setTimeout(() => {
      terminate({ decision: "unavailable", reason: "guard process timed out" });
    }, timeoutMs);

    const capture = (chunks: Buffer[]) => (chunk: Buffer | string) => {
      if (settled || terminating) return;
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      outputBytes += bytes.byteLength;
      if (!Number.isSafeInteger(outputBytes) || outputBytes > GUARD_OUTPUT_MAX_BYTES) {
        terminate({
          decision: "unavailable",
          reason: "guard process output exceeded bounded size",
        });
        return;
      }
      chunks.push(bytes);
    };
    child.stdout!.on("data", capture(stdoutChunks));
    child.stderr!.on("data", capture(stderrChunks));
    child.on("error", () => {
      if (terminating) return;
      finish({ decision: "unavailable", reason: "guard process unavailable" });
    });

    child.on("close", (code) => {
      if (terminating) return;
      let stdout: string;
      let stderr: string;
      try {
        const decoder = new TextDecoder("utf-8", { fatal: true });
        stdout = decoder.decode(Buffer.concat(stdoutChunks));
        stderr = decoder.decode(Buffer.concat(stderrChunks));
      } catch {
        return finish({
          decision: "unavailable",
          reason: "guard process emitted invalid UTF-8",
        });
      }
      // stdout-JSON deny (the majority of guards) takes precedence, then exit-2 deny.
      const jsonDecision = parseJsonDecision(stdout, scriptPath);
      if (jsonDecision?.decision === "deny") return finish(jsonDecision);
      if (code === 2) {
        const reason = stderr.trim() || "blocked by authoring guard (exit 2)";
        return finish({ decision: "deny", reason });
      }
      if (code !== 0)
        return finish({ decision: "unavailable", reason: `guard process exited ${code}` });
      finish(jsonDecision ?? ALLOW);
    });
  });
}

// Extract a deny reason from a guard's stdout JSON, or null if it isn't a deny.
// A guard may print non-JSON (nothing, additionalContext-only, log noise) — all null.
function parseJsonDecision(stdout: string, scriptPath: string): GuardDecision | null {
  const s = stdout.trim();
  if (!s) return null;
  let obj: any;
  try {
    obj = parseStrictJson(s, "authoring guard output", {
      maxBytes: GUARD_OUTPUT_MAX_BYTES,
      maxDepth: 32,
      maxNodes: 4_096,
    });
  } catch {
    return null; // non-JSON stdout -> no opinion
  }
  const hso = obj?.hookSpecificOutput;
  const observation = basename(scriptPath) === "firn-system-policy"
      && typeof hso?.additionalContext === "string"
    ? parseInvocationObservationReceipt(hso.additionalContext)
    : undefined;
  if (hso?.permissionDecision === "deny") {
    return {
      decision: "deny",
      reason: typeof hso.permissionDecisionReason === "string"
        ? hso.permissionDecisionReason
        : "blocked by authoring guard",
      ...(observation?.observation.decision === "deny" ? { observation } : {}),
    };
  }
  return observation?.observation.decision === "pass"
    ? { decision: "allow", observation }
    : null;
}

// Run a chain of guards in order; FIRST DENY WINS and short-circuits the rest
// (mirrors the CLI, where any matcher-hook's deny stops the tool). All-allow -> allow.
export async function evaluateGuards(
  scriptPaths: string[],
  hookInput: unknown,
  timeoutMs = 10000,
  env?: NodeJS.ProcessEnv,
): Promise<GuardDecision> {
  let observation: InvocationObservationReceipt | undefined;
  for (const p of scriptPaths) {
    const d = await runGuardScript(p, hookInput, timeoutMs, env);
    if (d.decision === "deny") return d;
    if (d.decision === "allow" && d.observation && !observation)
      observation = d.observation;
  }
  return observation ? { decision: "allow", observation } : ALLOW;
}
