/**
 * The managed Codex app-server driver: one exactly-attested provider session.
 *
 * Four behaviors are adapted from hermes-agent (MIT, Copyright (c) 2025 Nous
 * Research): the redacted provider-stderr tail carried by every failure
 * (`agent/transports/codex_app_server_session.py:327-362`), the per-turn
 * watchdog loop — overall deadline, post-tool quiet timer, and child-liveness
 * check — in `run_turn`
 * (`agent/transports/codex_app_server_session.py:447-495`), and
 * retire-and-respawn on provider death (`TurnResult.should_retire`,
 * `agent/transports/codex_app_server_session.py:79-85`, consumed in
 * `agent/codex_runtime.py:694-731`), plus provider-version-tolerant thread ID
 * extraction across `thread.id`, `thread.sessionId`, `sessionId`, and
 * `threadId` (`agent/transports/codex_app_server_session.py:272-284`). North's
 * shape differs where its invariants differ: every selected ID is syntax-
 * checked and later notifications must correlate exactly; the tail arrives
 * over the supervisor status channel; an expired watchdog interrupts the TURN
 * and settles it with the landed-work harvest rather than retiring a reusable
 * session; and because North keeps no transcript outside the provider thread,
 * a respawn re-runs the full launch preflight and re-sends the accumulated
 * context itself instead of waiting for the next user turn to rebuild it.
 */
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash } from "node:crypto";
import {
  closeSync, constants, fsyncSync, lstatSync, mkdtempSync, openSync, realpathSync,
  renameSync, rmSync, unlinkSync, writeSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { codexConfigArguments } from "../accounts";
import { managedNorthMcpEnvironment } from "../execution-admission";
import {
  NativeCommandActivityAccumulator, NORTH_BINARY_PROBE_SCRIPT, unknownNativeCommandActivity,
  type NativeCommandActivityObservation, type NativeCommandStatus,
} from "../native-command-activity";
import { parseStrictJson, StrictJsonlFrames } from "../strict-json";
import {
  trustedGitMetadataRoots, trustedGitProjectRoot, trustedManagedCodexExecutable,
} from "../trusted-runtime";
import type { TerminalTokenUsage } from "../usage";
import {
  McpActivityAccumulator, normalizeCodexMcpIdentity, type McpActivityObservation,
} from "../tool-activity";
import {
  FRAM_GRAPH_AUTHORING_CAPABILITY, FRAM_MCP_SERVER, FRAM_MCP_TOOL_NAMES,
  hasCanonicalFramMcpServer,
} from "../fram-graph-authoring";
import type { OpenAIAuthoritySurface } from "./authority";
import {
  assertInstalledManagedCodexHooks, expectedManagedCodexHooks,
} from "./codex-managed-hooks";
import { managedCodexNetworkArguments, managedCodexNetworkPolicy } from "./codex-network-policy";
import {
  CODEX_SUPERVISOR_STATUS_PREFIX, CODEX_SUPERVISOR_STDERR_FLAG, codexSupervisorStderrLine,
} from "./codex-supervisor-protocol";
import {
  formatProviderStderrTail, ProviderStderrRing, STDERR_TAIL_LINES,
} from "./codex-stderr-tail";
import { providerJoinEvidence, type ProviderJoinEvidence } from "./provider-join";

const SUPERVISOR = resolve(import.meta.dir, "codex-supervisor.ts");
const ENGINE = resolve(import.meta.dir, "../../../bin/north");
const RPC_TIMEOUT_MS = 20_000;
// codex-rs/app-server-transport/src/transport/stdio.rs feeds stdin through
// BufReader::lines and writes every serialized JSON-RPC message plus '\n' to
// stdout; it does not impose a 1 MiB frame ceiling. Keep each app-server frame
// finite, but allow legitimate large tool/result messages within our existing
// 32 MiB cumulative transport budget.
const MAX_LINE_BYTES = 8 * 1024 * 1024;
// 128 MiB: a single atomic graph edit-transaction (36 ops with rendered
// diffs) exceeded 32 MiB cumulative and killed an otherwise-clean lane
// mid-verification (2026-07-28, rt_core annotation salvage).
const MAX_TOTAL_BYTES = 128 * 1024 * 1024;
const MAX_FRAMES = 20_000;
const MAX_INVENTORY_PAGES = 32;
const MAX_MCP_SERVERS = 64;
const MAX_ID_BYTES = 512;
/** A native command's reported working directory: one bounded filesystem path. */
const MAX_CWD_BYTES = 4_096;
const MAX_QUEUED_NOTIFICATIONS = 256;
const MAX_UNSUPPORTED_NOTIFICATION_METHODS = 16;
const MAX_UNSUPPORTED_NOTIFICATIONS_PER_METHOD = 512;
const MAX_DISABLED_PROJECT_CONFIG_BYTES = 64 * 1024;
const MAX_DISABLED_PROJECT_CONFIG_DEPTH = 16;
const MAX_DISABLED_PROJECT_CONFIG_NODES = 2_048;
const MAX_SAFETY_BUFFERING_VALUES = 64;
const MAX_SAFETY_BUFFERING_VALUE_BYTES = 4_096;
export const MANAGED_CODEX_VERSION = "0.146.0";
// The supervisor status channel now carries forwarded provider stderr, so its
// reader is widened DELIBERATELY: one base64 diagnostic line (512 raw bytes)
// plus the receipt prefix fits in 2 KiB, and the supervisor's own lifetime
// forwarding budget (3_950 live + 1 notice + 40 flushed) sits under this frame
// ceiling with STARTED and EXIT to spare.
const SUPERVISOR_STATUS_MAX_LINE_BYTES = 2_048;
const SUPERVISOR_STATUS_MAX_FRAMES = 4_096;
const SUPERVISOR_STATUS_MAX_TOTAL_BYTES = 4 * 1024 * 1024;
// A turn terminal was previously awaited with no bound at all: RPC_TIMEOUT_MS
// only covers an OUTSTANDING request, so a provider that accepts turn/start and
// then stops speaking hung the lane forever.
const TURN_DEADLINE_MS = 600_000;
const POST_TOOL_QUIET_MS = 90_000;
// turn/interrupt is a courtesy, not a contract: bound it so a wedged provider
// cannot also wedge the settlement of the turn it wedged.
const TURN_INTERRUPT_MS = 5_000;
// Retire-and-respawn budget for ONE lane. A provider process death used to end
// the lane permanently, because spawn.ts refuses a process-death retry for any
// authoring-capable lane (it cannot know what the dead turn already wrote; the
// adapter can, from the harvest). Two is a bound, not a policy: a lane that
// cannot survive three provider processes is failing for a reason a fourth will
// not fix.
const MAX_RESPAWNS = 2;
// The recovered-context frame is the lane's whole memory of the dead session,
// and it is also model input: keep it compact enough to leave the continuation
// room to work.
const MAX_RECOVERED_TEXT_BYTES = 8 * 1024;
const MAX_RECOVERED_CONTEXT_BYTES = 96 * 1024;
const SUPERVISOR_FRAME_PREFIX = "NORTH_CODEX_RPC 1 ";
const CODEX_SHELL_PREFLIGHT_TIMEOUT_MS = 5_000;
const CODEX_SHELL_PREFLIGHT_OUTPUT_BYTES = 4_096;
const CODEX_SHELL_PREFLIGHT_COMMAND = Object.freeze([
  "bash", "--noprofile", "--norc", "-c", NORTH_BINARY_PROBE_SCRIPT,
]);

// These classifications cover every non-removed feature in Codex 0.146.0.
// Removed names may remain explicitly false while Codex still recognizes them.
// The version attestation makes a new default fail closed until reviewed; only
// the execution primitives and North's managed hooks remain enabled.
export const MANAGED_CODEX_ENABLED_FEATURES = [
  "hooks",
  "shell_tool",
  "unified_exec",
] as const;
export const MANAGED_CODEX_DISABLED_FEATURES = [
  "apply_patch_freeform",
  "apps",
  "apply_patch_streaming_events",
  "artifact",
  "auth_elicitation",
  "browser_use",
  "browser_use_external",
  "browser_use_full_cdp_access",
  "chronicle",
  "code_mode",
  "code_mode_buffered_exec",
  "code_mode_host",
  "code_mode_only",
  "computer_use",
  "concurrent_reasoning_summaries",
  "current_time_reminder",
  "default_mode_request_user_input",
  "deferred_executor",
  "deferred_tool_world_state",
  "enable_request_compression",
  "enable_fanout",
  "enable_mcp_apps",
  "exec_permission_approvals",
  "executor_capability_discovery",
  "external_agent_memory_import",
  "fast_mode",
  "goals",
  "guardian_approval",
  "guardianv2",
  "image_generation",
  "in_app_browser",
  "in_app_updates",
  "item_ids",
  "local_thread_store_compression",
  "mcp_2026_07_28",
  "memories",
  "mentions_v2",
  "multi_agent",
  "multi_agent_v2",
  "non_prefixed_mcp_tool_names",
  "personality",
  "plugin_sharing",
  "plugins",
  "prevent_idle_sleep",
  "realtime_conversation",
  "remote_compaction_v2",
  "remote_plugin",
  "request_permissions_tool",
  "respect_system_proxy",
  "rollout_budget",
  "runtime_metrics",
  "secret_auth_storage",
  "shell_snapshot",
  "shell_zsh_fork",
  "skill_mcp_dependency_install",
  "skill_search",
  "standalone_web_search",
  "terminal_visualization_instructions",
  "token_budget",
  "tool_call_mcp_elicitation",
  "tool_suggest",
  "unified_exec_zsh_fork",
  "use_agent_identity",
  "use_legacy_landlock",
  "web_search_cached",
  "web_search_request",
  "workspace_dependencies",
] as const;

// Codex reports an untrusted project's config as a disabled layer. These are
// the reviewed top-level keys in the global Codex profile which can therefore
// appear when HOME itself is the project root. Their VALUES remain inert only
// under the structured disabled-reason + correlated warning contract below, while
// the effective config, thread authority, hooks, MCP inventory, sandbox, and
// remote-control state are independently attested. Unknown keys stay denied so
// a newly introduced surface must be reviewed before managed lanes accept it.
const REVIEWED_DISABLED_PROJECT_CONFIG_KEYS = [
  "approval_policy",
  "approvals_reviewer",
  "default_permissions",
  "exec_policy",
  "features",
  "hooks",
  "mcp_servers",
  "model",
  "model_reasoning_effort",
  "notice",
  "project_doc_fallback_filenames",
  "projects",
  "sandbox_mode",
  "tui",
] as const;

type JsonObject = Record<string, unknown>;
type RpcId = number | string;

export interface ManagedCodexNorthServer {
  command: string;
  args: string[];
  env: Record<string, string>;
}

export interface ManagedCodexAppServerOptions {
  command: string;
  /** Test-only executable prefix, e.g. a Bun fixture script. */
  commandPrefix?: string[];
  /** Test seam; production always retains the parent-death supervisor. */
  useSupervisor?: boolean;
  /** Test-only in-memory transport seam. */
  spawnProcess?: typeof spawn;
  /** Test-only canonical executable attestation target. */
  testExpectedExecutable?: string;
  env: NodeJS.ProcessEnv;
  cwd: string;
  prompt: string;
  model: string;
  effort?: string;
  developerInstructions: string;
  surface: OpenAIAuthoritySurface;
  north: ManagedCodexNorthServer;
  fram?: ManagedCodexNorthServer;
  timeoutMs?: number;
  /** Overall bound on one turn; defaults to NORTH_CODEX_TURN_DEADLINE_MS. */
  turnDeadlineMs?: number;
  /** Silence bound armed by a completed tool item; NORTH_CODEX_POST_TOOL_QUIET_MS. */
  postToolQuietMs?: number;
  /** Provider-death respawns this lane may spend; NORTH_CODEX_MAX_RESPAWNS. 0 disables. */
  maxRespawns?: number;
  onActivity?: (kind: string) => void;
}

export interface ManagedCodexResult {
  text: string;
  usage: TerminalTokenUsage & {
    input_tokens: number;
    cached_input_tokens: number;
    output_tokens: number;
    reasoning_output_tokens: number;
  };
  providerJoin: ProviderJoinEvidence;
  /**
   * Work items this turn completed, counted from observed `item/completed`
   * notifications: every completed item that is neither the assistant's own
   * message nor a reasoning block (so commandExecution, fileChange,
   * mcpToolCall, webSearch, todoList, …). Codex nests all of these inside ONE
   * turn, so a turn count can never show whether a tool loop ran; this is the
   * honest per-turn "did work happen" signal on the app-server path
   * (thread 019f9cc2 — every managed lane reported turn units and no item
   * count at all, because this path never counted them).
   */
  toolItems: number;
}

// A later North input frame for the same provider thread, or `undefined` to
// settle the session after the current turn. The session drives one Codex turn
// per resolved frame; every turn re-proves the exact managed authority surface
// before it starts and again at its terminal settlement, so a continuation can
// never widen capability (web stays disabled) mid-session.
export type ManagedCodexNextInput = () => Promise<string | undefined>;

/**
 * What the provider process itself said and did around a failure. Before this,
 * `managed Codex app-server exited unexpectedly` was the whole story: codex's
 * stderr was drained into a void by the supervisor and its EXIT receipt was
 * dropped the moment authority preflight finished.
 */
export interface ManagedCodexDiagnostics {
  /** Redacted provider stderr, oldest line first, bounded to the tail. */
  stderrTail: string[];
  /** The supervisor's own EXIT receipt, or the direct child's exit code. */
  exitCode?: number;
  /** Signal the provider died on, when the host observed one. */
  exitSignal?: string;
  /** Whether the provider was still running when the failure was raised. */
  providerAlive?: boolean;
}

/**
 * One provider session this lane outlived. A respawn is a real event with a
 * real cost, so it is recorded per attempt rather than collapsed to a counter:
 * a lane that respawned twice with the same exit code and the same last stderr
 * line is diagnosing a reproducible provider defect, not bad luck.
 */
export interface ManagedCodexRespawnAttempt {
  /** 1-based index of the DEAD attempt this record describes. */
  attempt: number;
  /** The failure that retired it, as one bounded line. */
  reason: string;
  /** The retired session's provider thread, when it reached thread/start. */
  threadId?: string;
  /** Turns that had already settled on the retired session. */
  completedTurns: number;
  /** Redacted provider stderr tail observed as it died. */
  stderrTail?: string[];
  exitCode?: number;
  exitSignal?: string;
}

/** What a lane's respawns cost it, readable whether the lane succeeded or failed. */
export interface ManagedCodexRespawnRecord {
  respawnCount: number;
  /** Turns that settled across ALL provider sessions this lane used. */
  completedTurns: number;
  respawns: ManagedCodexRespawnAttempt[];
}

export class ManagedCodexPreThreadError extends Error {
  /** Populated as the failure unwinds; see {@link ManagedCodexDiagnostics}. */
  diagnostics?: ManagedCodexDiagnostics;
  constructor(message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "ManagedCodexPreThreadError";
  }
}

/**
 * What a failed managed session actually produced before it failed. A lane that
 * had already written +83/-3 of real code was recorded `result=0b,
 * delivery=blocked` because the adapter threw a bare error and every trace of
 * the turn — its text, its usage, its tool calls — died with the exception
 * (lane-ms0qeuwx, 2026-07-26). The failure is still a failure; it is no longer
 * an amnesia.
 */
export interface ManagedCodexHarvest {
  threadId?: string;
  turnIds: string[];
  /** Turns that yielded a complete result before the failure. */
  completedTurns: number;
  /** Assistant text accumulated by the failing turn, if any. */
  text: string;
  /** Tool work completed before the failure, when the failing turn was observed. */
  toolItems?: number;
  usage?: ManagedCodexResult["usage"];
  mcp: McpActivityObservation;
  nativeCommands: NativeCommandActivityObservation;
  unsupportedNotifications: Record<string, number>;
  /** True when tool work or model text landed before the failure. */
  landedWork: boolean;
  /** Redacted provider stderr tail observed around the failure. */
  stderrTail?: string[];
  /** The supervisor's EXIT receipt, or the direct child's exit code. */
  exitCode?: number;
  /** Signal the provider died on, when the host observed one. */
  exitSignal?: string;
  /** Provider sessions this lane retired before the one that failed. */
  respawnCount?: number;
  /** Per-attempt post-mortem for each retired session; see the record type. */
  respawns?: ManagedCodexRespawnAttempt[];
}

/**
 * Post-thread-start failure carrying its harvest. The message is unchanged
 * (`openai_provider_execution_failed`) so every existing classification,
 * retry gate, and log matcher keeps its exact behavior.
 */
export class ManagedCodexHarvestError extends Error {
  /** Populated as the failure unwinds; mirrored onto {@link harvest}. */
  diagnostics?: ManagedCodexDiagnostics;
  constructor(readonly harvest: ManagedCodexHarvest, options?: ErrorOptions) {
    super("openai_provider_execution_failed", options);
    this.name = "ManagedCodexHarvestError";
  }
}

const DIAGNOSTIC_CAUSE = Symbol.for("north.codex.diagnostics");

/**
 * Hang the diagnostics off the failure so every renderer sees them: the
 * structured fields for machine consumers, and one extra link at the END of the
 * cause chain so `causeChain(...)` — the only durable witness a dead managed
 * lane leaves — carries the tail and the exit code verbatim.
 */
function attachDiagnostics(
  error: ManagedCodexHarvestError | ManagedCodexPreThreadError,
  diagnostics: ManagedCodexDiagnostics,
): void {
  error.diagnostics = diagnostics;
  if (error instanceof ManagedCodexHarvestError) {
    if (diagnostics.stderrTail.length) error.harvest.stderrTail = [...diagnostics.stderrTail];
    if (diagnostics.exitCode !== undefined) error.harvest.exitCode = diagnostics.exitCode;
    if (diagnostics.exitSignal !== undefined) error.harvest.exitSignal = diagnostics.exitSignal;
  }
  const exit = diagnostics.exitCode !== undefined
    ? `provider exit code ${diagnostics.exitCode}`
    : diagnostics.exitSignal !== undefined
      ? `provider died on ${diagnostics.exitSignal}`
      : diagnostics.providerAlive === true
        ? "provider still running"
        : undefined;
  const tail = formatProviderStderrTail(diagnostics.stderrTail);
  const rendered = [exit, tail].filter((part) => part !== undefined).join("\n");
  if (!rendered) return;
  let current: Error = error;
  for (let depth = 0; depth < 8; depth += 1) {
    const cause = (current as { cause?: unknown }).cause;
    if (!(cause instanceof Error)) break;
    current = cause;
  }
  // Re-attachable: the exit receipt lands only as the process closes, after the
  // first (live) snapshot was already appended.
  if (DIAGNOSTIC_CAUSE in current) {
    current.message = rendered;
    return;
  }
  if ((current as { cause?: unknown }).cause !== undefined) return;
  const link = new Error(rendered);
  Object.defineProperty(link, DIAGNOSTIC_CAUSE, { value: true });
  (current as { cause?: unknown }).cause = link;
}

function record(value: unknown, label: string): JsonObject {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value as JsonObject;
}

function boundedString(value: unknown, label: string, maxBytes = MAX_ID_BYTES): string {
  if (typeof value !== "string" || !value || value !== value.trim()
      || Buffer.byteLength(value, "utf8") > maxBytes
      || /[\u0000-\u001f\u007f]/.test(value))
    throw new Error(`${label} must be a bounded canonical string`);
  return value;
}

function boundedProviderProse(value: unknown, label: string, maxBytes: number): string {
  if (typeof value !== "string" || !value || Buffer.byteLength(value, "utf8") > maxBytes
      || /[\u0000\u000b\u000c\u000e-\u001f\u007f]/.test(value))
    throw new Error(`${label} must be bounded provider prose`);
  return value;
}

function protocolId(value: unknown, label: string): string {
  const id = boundedString(value, label);
  if (!/^[A-Za-z0-9._:-]+$/.test(id)) throw new Error(`${label} is invalid`);
  return id;
}

function providerThreadId(
  envelope: JsonObject,
  thread: JsonObject,
  label: string,
): string {
  let selected: string | undefined;
  for (const [value, source] of [
    [thread.id, "thread id"],
    [thread.sessionId, "thread session id"],
    [envelope.sessionId, "session id"],
    [envelope.threadId, "thread id"],
  ] as const) {
    if (value === undefined || value === null) continue;
    const id = protocolId(value, `${label} ${source}`);
    selected ??= id;
  }
  if (!selected) throw new Error(`${label} omitted its protocol id`);
  return selected;
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object")
    return Object.fromEntries(Object.keys(value as object).sort()
      .map((key) => [key, canonical((value as JsonObject)[key])]));
  return value;
}

function exact(value: unknown, expected: unknown, label: string): void {
  if (JSON.stringify(canonical(value)) !== JSON.stringify(canonical(expected)))
    throw new Error(`${label} does not match North's exact managed Codex contract`);
}

// Same equality as `exact`, but carries the observed/expected pair on the
// {cause} so the lane log can name WHAT drifted, not merely that something did.
// Deliberately opt-in: the session-authority layer compared by `exact` embeds
// MCP server env, so only credential-free shapes (workspace roots, sandbox,
// instruction sources) may use this.
function exactDiagnosable(value: unknown, expected: unknown, label: string): void {
  const observed = JSON.stringify(canonical(value));
  const wanted = JSON.stringify(canonical(expected));
  if (observed === wanted) return;
  throw new Error(`${label} does not match North's exact managed Codex contract`, {
    cause: new Error(
      `observed=${String(observed).slice(0, 600)} expected=${String(wanted).slice(0, 600)}`,
    ),
  });
}

// `exact`, but the {cause} names the drifted top-level KEYS only — safe for
// shapes whose values may carry environment content or a token.
function exactNamingKeys(value: unknown, expected: unknown, label: string): void {
  const observed = canonical(value);
  const wanted = canonical(expected);
  if (JSON.stringify(observed) === JSON.stringify(wanted)) return;
  const asRecord = (input: unknown): JsonObject | undefined =>
    input && typeof input === "object" && !Array.isArray(input) ? input as JsonObject : undefined;
  const left = asRecord(observed);
  const right = asRecord(wanted);
  const drift = left && right
    ? [...new Set([...Object.keys(left), ...Object.keys(right)])].sort().flatMap((key) => {
      if (!(key in right)) return [`unexpected ${key}`];
      if (!(key in left)) return [`missing ${key}`];
      return JSON.stringify(left[key]) === JSON.stringify(right[key]) ? [] : [`changed ${key}`];
    })
    : [`not the expected shape`];
  throw new Error(`${label} does not match North's exact managed Codex contract`, {
    cause: new Error(`drifted: ${drift.join(", ") || "ordering"}`),
  });
}

// A config layer North requires to be EMPTY. `exact(layer, {})` reported only
// that it differed, which is the least useful thing to say about a layer whose
// whole contract is "has nothing in it" — the one fact a reader needs is what
// appeared. Keys only, never values: an option name is diagnostic, an option
// value may be a token.
function mustBeEmptyLayer(value: unknown, label: string): void {
  const present = Object.keys(canonical(value) as JsonObject).sort();
  if (present.length)
    throw new Error(`${label} must be empty but carries: ${present.join(", ")}`);
}

function validateShellPreflight(response: unknown): void {
  const result = record(response, "Codex command/exec response");
  onlyKeys(result, ["exitCode", "stdout", "stderr"], "Codex command/exec response");
  const expectedOutput = `${ENGINE}\n${ENGINE}\n`;
  if (!Number.isSafeInteger(result.exitCode) || result.exitCode !== 0
      || result.stdout !== expectedOutput || result.stderr !== "")
    throw new Error("Codex command/exec did not preserve North's managed shell identity");
}

// Field names are diagnostic and never a token, so the drift is always named.
function onlyKeys(value: JsonObject, expected: readonly string[], label: string): void {
  const present = new Set(Object.keys(value));
  const wanted = new Set(expected);
  const drift = [
    ...[...present].filter((key) => !wanted.has(key)).sort().map((key) => `unexpected ${key}`),
    ...[...wanted].filter((key) => !present.has(key)).sort().map((key) => `missing ${key}`),
  ];
  if (drift.length)
    throw new Error(`${label} fields do not match North's exact managed Codex contract`, {
      cause: new Error(`drifted: ${drift.join(", ")}`),
    });
}

function optionalBoundedString(value: unknown, label: string, maxBytes = MAX_ID_BYTES): string | null {
  if (value === null) return null;
  return boundedString(value, label, maxBytes);
}

function tomlStringMap(values: Record<string, string>): string {
  return `{${Object.entries(values).sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${JSON.stringify(key)}=${JSON.stringify(value)}`).join(",")}}`;
}

function tomlProjectMap(root: string): string {
  return `{${JSON.stringify(root)}={trust_level="untrusted"}}`;
}

function assertNoFilesystemAuthority(codexHome: string): void {
  for (const name of ["config.toml", "hooks.json", "rules"] as const) {
    try {
      lstatSync(resolve(codexHome, name));
      throw new Error(`managed Codex account contains authority-bearing ${name}`);
    } catch (error) {
      if (error instanceof Error && "code" in error && error.code === "ENOENT") continue;
      throw error;
    }
  }
}

interface LaunchContract {
  args: string[];
  expectedSessionConfig: JsonObject;
  executable: string;
  codexHome: string;
  sqliteHome: string;
  cwd: string;
  projectRoot: string;
  /** Git metadata roots the workspace-write sandbox may write; [] when read-only. */
  writableRoots: string[];
  network: ReturnType<typeof managedCodexNetworkPolicy>;
  /** Immutable requirements.toml independently attested hook failures as blocking. */
  installedManagedHookFailureMode?: "block";
}

/**
 * Managed write lanes must be able to COMMIT the work they produce — the lane
 * landing protocol is "commit on your lane branch, the coordinator ff-merges".
 * Codex's workspace-write sandbox makes the workspace writable but keeps the
 * Git metadata directory read-only, so `writableRoots: []` made every managed
 * Codex lane structurally unable to land anything (observed: `.git/index.lock:
 * Read-only file system`).
 *
 * The grant is the checkout's own Git metadata — its `--git-dir` and the
 * `--git-common-dir` it shares with a main checkout — plus the North state root.
 *
 * On the state root: it was previously excluded on the reasoning that graph
 * writes go through the North MCP server, which runs outside the sandbox. That
 * held for the MCP path, but it left a live contradiction — every lane brief and
 * the canonical AGENTS.md instruct agents to run `north tell`, and from inside
 * the sandbox that shell call is guaranteed EROFS. Lanes did real work, could
 * not record it, and were scored as delivering nothing (observed 2026-07-26:
 * `lane-ms0qeuwx` wrote +83/-3 of SDK code and was recorded `result=0b`).
 * Closing the contradiction on the sandbox side is the smaller change than
 * rewriting every brief, and it removes the last structural delivery-rate gap
 * between Codex and the unsandboxed Anthropic lanes.
 *
 * What stays closed: everything else on the machine. A lane still cannot write
 * outside its workspace, its git metadata, and North's own state — so this is
 * parity on *delivery*, not the full-access grant Anthropic lanes run with.
 */
export function managedCodexWritableRoots(cwd: string): string[] {
  const northStateRoot = resolve(homedir(), ".local/state/north");
  return [...new Set([...trustedGitMetadataRoots(cwd), northStateRoot])].sort();
}

function sandboxWritableRoots(surface: OpenAIAuthoritySurface, cwd: string): string[] {
  if (surface.sandbox !== "workspace-write") return [];
  return managedCodexWritableRoots(cwd);
}

export function managedCodexAppServerLaunch(
  options: ManagedCodexAppServerOptions,
): LaunchContract {
  const codexHomeValue = options.env.CODEX_HOME?.trim();
  const sqliteHomeValue = options.env.CODEX_SQLITE_HOME?.trim();
  if (!codexHomeValue || !sqliteHomeValue)
    throw new ManagedCodexPreThreadError("openai_target_state_roots_missing");
  // Each preflight cause carries its own code so a swallowed {cause} in the
  // lane log no longer collapses distinct authority failures into one opaque
  // string. Diagnosis reads the code; the {cause} keeps the raw error.
  const stage = <T>(code: string, run: () => T): T => {
    try {
      return run();
    } catch (cause) {
      throw new ManagedCodexPreThreadError(code, { cause });
    }
  };
  const codexHome = stage("openai_codex_state_root_unresolvable",
    () => realpathSync(codexHomeValue));
  const sqliteHome = stage("openai_codex_state_root_unresolvable",
    () => realpathSync(sqliteHomeValue));
  const cwd = stage("openai_codex_cwd_unresolvable", () => realpathSync(options.cwd));
  const projectRoot = stage("openai_codex_project_root_untrusted",
    () => trustedGitProjectRoot(cwd));
  const executable = stage("openai_codex_executable_pin_mismatch", () => {
    const resolved = realpathSync(options.command);
    const expectedExecutable = realpathSync(
      options.spawnProcess && options.testExpectedExecutable
        ? options.testExpectedExecutable
        : trustedManagedCodexExecutable(),
    );
    if (resolved !== expectedExecutable)
      throw new Error(
        `managed Codex executable ${resolved} is not the pinned provider binary ${expectedExecutable}`,
      );
    return resolved;
  });
  // Codex 0.144.4 enforces managed_hook_failure_mode from requirements.toml
  // but its configRequirements/read response omits that field. Production can
  // accept the omission only after re-reading the exact root-managed,
  // Nix-immutable requirements and hook closure at this final launch seam.
  // Synthetic transports deliberately receive no such proof.
  const installedManagedHookFailureMode = options.spawnProcess
    ? undefined
    : stage("openai_managed_hooks_contract_unavailable", () => {
      assertInstalledManagedCodexHooks();
      return "block" as const;
    });
  stage("openai_codex_authority_filesystem_invalid",
    () => assertNoFilesystemAuthority(codexHome));
  options.env.CODEX_HOME = codexHome;
  options.env.CODEX_SQLITE_HOME = sqliteHome;
  options.env.CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED = "1";

  const managedPath = options.env.PATH;
  if (typeof managedPath !== "string" || !managedPath
      || managedPath !== managedPath.trim()
      || managedPath.split(delimiter)[0] !== dirname(ENGINE)
      || options.env.NORTH_BIN !== ENGINE)
    throw new ManagedCodexPreThreadError("openai_managed_shell_environment_invalid");
  const shellEnvironmentPolicy = {
    inherit: "core",
    set: { PATH: managedPath, NORTH_BIN: ENGINE },
  };

  const northEnv = managedNorthMcpEnvironment(options.north.env);
  const graphAuthoring = options.surface.capabilities.includes(FRAM_GRAPH_AUTHORING_CAPABILITY);
  if (graphAuthoring
    ? !options.fram || !hasCanonicalFramMcpServer({
      type: "stdio",
      command: options.fram.command,
      args: options.fram.args,
      env: options.fram.env,
    }, options.cwd)
    : options.fram !== undefined) {
    throw new ManagedCodexPreThreadError("openai_managed_fram_mcp_contract_missing");
  }
  const framConfig = options.fram
    ? {
      [FRAM_MCP_SERVER]: {
        command: options.fram.command,
        args: options.fram.args,
        env: options.fram.env,
        enabled: true,
        required: true,
        enabled_tools: [...FRAM_MCP_TOOL_NAMES],
      },
    }
    : {};
  const network = managedCodexNetworkPolicy(options.surface);
  const features = Object.fromEntries([
    ...MANAGED_CODEX_ENABLED_FEATURES.map((name) => [name, true] as const),
    ...MANAGED_CODEX_DISABLED_FEATURES.map((name) => [name, false] as const),
  ]);
  const sessionFeatures = {
    ...features,
    network_proxy: network.networkProxyEnabled
      ? { enabled: true, domains: network.domains }
      : false,
  };
  const writableRoots = stage("openai_codex_git_metadata_unresolvable",
    () => sandboxWritableRoots(options.surface, cwd));
  const expectedSessionConfig: JsonObject = {
    cli_auth_credentials_store: "file",
    forced_login_method: "chatgpt",
    model_provider: "openai",
    sqlite_home: sqliteHome,
    ...(writableRoots.length
      ? { sandbox_workspace_write: {
        writable_roots: writableRoots,
        network_access: network.networkAccess,
      } }
      : {}),
    project_root_markers: [".git"],
    projects: { [projectRoot]: { trust_level: "untrusted" } },
    project_doc_max_bytes: 0,
    allow_login_shell: false,
    shell_environment_policy: shellEnvironmentPolicy,
    mcp_servers: {
      north: {
        command: options.north.command,
        args: options.north.args,
        env: northEnv,
        enabled: true,
        required: true,
        enabled_tools: options.surface.northEnabledTools,
      },
      ...framConfig,
    },
    web_search: options.surface.web,
    features: sessionFeatures,
  };

  const args = [
    ...codexConfigArguments(options.env),
    "-c", 'project_root_markers=[".git"]',
    ...(writableRoots.length
      ? [
        "-c", `sandbox_workspace_write.writable_roots=${JSON.stringify(writableRoots)}`,
        "-c", `sandbox_workspace_write.network_access=${network.networkAccess}`,
      ]
      : []),
    "-c", `projects=${tomlProjectMap(projectRoot)}`,
    "-c", "project_doc_max_bytes=0",
    "-c", "allow_login_shell=false",
    "-c", 'shell_environment_policy.inherit="core"',
    "-c", `shell_environment_policy.set=${tomlStringMap(shellEnvironmentPolicy.set)}`,
    "-c", `mcp_servers.north.command=${JSON.stringify(options.north.command)}`,
    "-c", `mcp_servers.north.args=${JSON.stringify(options.north.args)}`,
    "-c", `mcp_servers.north.env=${tomlStringMap(northEnv)}`,
    "-c", "mcp_servers.north.enabled=true",
    "-c", "mcp_servers.north.required=true",
    "-c", `mcp_servers.north.enabled_tools=${JSON.stringify(options.surface.northEnabledTools)}`,
    ...(options.fram ? [
      "-c", `mcp_servers.${FRAM_MCP_SERVER}.command=${JSON.stringify(options.fram.command)}`,
      "-c", `mcp_servers.${FRAM_MCP_SERVER}.args=${JSON.stringify(options.fram.args)}`,
      "-c", `mcp_servers.${FRAM_MCP_SERVER}.env=${tomlStringMap(options.fram.env)}`,
      "-c", `mcp_servers.${FRAM_MCP_SERVER}.enabled=true`,
      "-c", `mcp_servers.${FRAM_MCP_SERVER}.required=true`,
      "-c", `mcp_servers.${FRAM_MCP_SERVER}.enabled_tools=${JSON.stringify(FRAM_MCP_TOOL_NAMES)}`,
    ] : []),
    "-c", `web_search=${JSON.stringify(options.surface.web)}`,
    ...MANAGED_CODEX_ENABLED_FEATURES.flatMap((name) => ["--enable", name]),
    ...managedCodexNetworkArguments(options.surface),
    ...MANAGED_CODEX_DISABLED_FEATURES.flatMap((name) => ["--disable", name]),
    "app-server", "--stdio", "--strict-config",
  ];
  return {
    args, expectedSessionConfig, executable, codexHome, sqliteHome, cwd, projectRoot,
    writableRoots, network, installedManagedHookFailureMode,
  };
}

interface Pending {
  method: string;
  timer: ReturnType<typeof setTimeout>;
  resolve(value: unknown): void;
  reject(error: Error): void;
}

type AppServerWriter = (
  line: string,
  callback: (error?: Error | null) => void,
) => void;
type AppServerRequestHandler = (id: RpcId, method: string, params: unknown) => JsonObject | undefined;

interface SupervisorControl {
  path: string;
  connected: Promise<void>;
  writeLine: AppServerWriter;
  close(): void;
}

function createSupervisorControl(): SupervisorControl {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-control-"));
  let sequence = 0;
  let closed = false;
  return {
    path: directory,
    connected: Promise.resolve(),
    writeLine(line, callback) {
      if (closed || Buffer.byteLength(line, "utf8") > MAX_LINE_BYTES) {
        callback(new Error("managed Codex supervisor control is unavailable"));
        return;
      }
      sequence += 1;
      const stem = String(sequence).padStart(12, "0");
      const temporary = join(directory, `.${stem}.${process.pid}.tmp`);
      const request = join(directory, `${stem}.req`);
      let fd: number | undefined;
      try {
        fd = openSync(temporary,
          constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | (constants.O_NOFOLLOW ?? 0),
          0o600);
        const payload = Buffer.from(line, "utf8");
        const digest = createHash("sha256").update(payload).digest("hex");
        const bytes = Buffer.concat([
          Buffer.from(`${SUPERVISOR_FRAME_PREFIX}${payload.byteLength} ${digest}\n`, "ascii"),
          payload,
        ]);
        let offset = 0;
        while (offset < bytes.byteLength)
          offset += writeSync(fd, bytes, offset, bytes.byteLength - offset);
        fsyncSync(fd);
        closeSync(fd);
        fd = undefined;
        renameSync(temporary, request);
        callback();
      } catch (error) {
        try { if (fd !== undefined) closeSync(fd); } catch {}
        try { unlinkSync(temporary); } catch {}
        callback(error as Error);
      }
    },
    close() {
      if (closed) return;
      closed = true;
      try { rmSync(directory, { recursive: true, force: true }); } catch {}
    },
  };
}

const SAFE_NOTIFICATIONS = new Set([
  "configWarning",
  "deprecationNotice",
  "remoteControl/status/changed",
  "mcpServer/startupStatus/updated",
  "model/safetyBuffering/updated",
  "account/rateLimits/updated",
  "serverRequest/resolved",
  "thread/started",
  "thread/status/changed",
  "thread/tokenUsage/updated",
  "turn/started",
  "turn/completed",
  "item/started",
  "item/completed",
  "item/agentMessage/delta",
  "item/plan/delta",
  "item/reasoning/summaryTextDelta",
  "item/reasoning/summaryPartAdded",
  "item/reasoning/textDelta",
  "item/commandExecution/outputDelta",
  "item/commandExecution/terminalInteraction",
  "item/fileChange/outputDelta",
  "item/fileChange/patchUpdated",
  "item/mcpToolCall/progress",
  "turn/diff/updated",
  "turn/plan/updated",
  "hook/started",
  "hook/completed",
]);

class AppServerRpc {
  private nextId = 0;
  private pending = new Map<RpcId, Pending>();
  private frames = new StrictJsonlFrames({
    label: "managed Codex app-server",
    maxLineBytes: MAX_LINE_BYTES,
    maxTotalBytes: MAX_TOTAL_BYTES,
    maxFrames: MAX_FRAMES,
  });
  private terminal?: Error;
  private terminalFromProcessDeath = false;
  private closed = false;
  private terminalListeners = new Set<(error: Error) => void>();
  private unsupported = new Map<string, number>();
  // Direct (unsupervised) launches own the provider's stderr, so the ring lives
  // here; under the supervisor the ring lives there and its tail is forwarded.
  private stderr = new ProviderStderrRing();

  constructor(
    private child: ChildProcessWithoutNullStreams,
    private timeoutMs: number,
    private onNotification: (method: string, params: unknown) => void,
    private onServerRequest: AppServerRequestHandler,
    private writeLine: AppServerWriter = (line, callback) => {
      child.stdin.write(line, callback);
    },
    private ownsStderr = true,
  ) {
    child.stdout.on("data", (chunk: Buffer) => this.onData(chunk));
    child.stdout.on("end", () => {
      try { this.frames.finish(); }
      catch (cause) {
        this.failFromDeath(new Error("managed Codex closed with a partial frame", { cause }));
      }
    });
    child.stdout.on("error", () => this.failFromDeath(new Error("managed Codex stdout failed")));
    if (this.ownsStderr) {
      // Was `resume()` — the provider's account of its own death, discarded.
      child.stderr.on("data", (chunk: Buffer | string) => {
        try { this.stderr.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)); }
        catch { /* diagnostics never fail a turn */ }
      });
      child.stderr.on("end", () => { try { this.stderr.finish(); } catch {} });
      child.stderr.on("error", () => {});
    }
    child.stdin.on("error", () => this.failFromDeath(new Error("managed Codex stdin failed")));
    child.on("error", () => this.failFromDeath(new Error("managed Codex supervisor failed")));
    child.on("exit", () => {
      if (!this.closed)
        this.failFromDeath(new Error("managed Codex app-server exited unexpectedly"));
    });
  }

  /**
   * The provider PROCESS ended this session — a broken pipe, a spawn error, or
   * an exit. Distinct from a protocol/authority terminal because only this kind
   * is a respawn trigger: a live-but-misbehaving provider gets no second
   * process, and a watchdog that interrupts a live turn is not a death at all.
   */
  private failFromDeath(error: Error): void {
    if (!this.terminal) this.terminalFromProcessDeath = true;
    this.fail(error);
  }

  private fail(error: Error): void {
    if (this.terminal) return;
    this.terminal = error;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
    for (const listener of this.terminalListeners) listener(error);
  }

  onTerminal(listener: (error: Error) => void): () => void {
    if (this.terminal) { listener(this.terminal); return () => {}; }
    this.terminalListeners.add(listener);
    return () => this.terminalListeners.delete(listener);
  }

  private rejectServerRequest(id: RpcId): void {
    this.writeLine(`${JSON.stringify({
      id, error: { code: -32601, message: "North does not grant app-server callback authority" },
    })}\n`, () => {});
    this.fail(new Error("managed Codex requested ungranted client authority"));
  }

  private onData(chunk: Buffer): void {
    try {
      for (const line of this.frames.push(chunk)) this.onLine(line);
    } catch (cause) {
      this.fail(new Error("managed Codex emitted invalid JSONL", { cause }));
    }
  }

  private onLine(line: string): void {
    let value: unknown;
    try { value = parseStrictJson(line, "managed Codex JSONL", { maxBytes: MAX_LINE_BYTES }); }
    catch (cause) { this.fail(new Error("managed Codex emitted malformed JSONL", { cause })); return; }
    let message: JsonObject;
    try { message = record(value, "managed Codex message"); }
    catch (error) { this.fail(error as Error); return; }
    if (typeof message.method === "string") {
      if ("id" in message) {
        if (!Object.keys(message).every((key) => ["id", "method", "params"].includes(key))) {
          this.fail(new Error("managed Codex server request envelope is invalid"));
          return;
        }
        const id = message.id;
        if (typeof id !== "number" && typeof id !== "string") {
          this.fail(new Error("managed Codex server request has invalid id"));
          return;
        }
        let result: JsonObject | undefined;
        try { result = this.onServerRequest(id, message.method, message.params); }
        catch (error) { this.fail(error instanceof Error ? error : new Error("managed Codex callback invalid")); return; }
        if (result === undefined) { this.rejectServerRequest(id); return; }
        this.writeLine(`${JSON.stringify({ id, result })}\n`, (error) => {
          if (error) this.fail(new Error("managed Codex callback response failed", { cause: error }));
        });
        return;
      }
      try {
        onlyKeys(message, ["method", "params",
          ...(Object.hasOwn(message, "emittedAtMs") ? ["emittedAtMs"] : []),
        ], "managed Codex notification");
        if ("emittedAtMs" in message
            && (typeof message.emittedAtMs !== "number"
              || !Number.isSafeInteger(message.emittedAtMs)
              || message.emittedAtMs < 0))
          throw new Error("managed Codex notification emittedAtMs is invalid");
      }
      catch (error) { this.fail(error as Error); return; }
      if (!SAFE_NOTIFICATIONS.has(message.method)) {
        // NARROWED TERMINAL (2026-07-26): an unrecognized NOTIFICATION carries no
        // authority, cannot be answered, and does not change what the thread may
        // do — a new Codex build adding one telemetry notification used to kill a
        // lane mid-turn and orphan the code it had already written. Drop it,
        // count it, and surface the counts as evidence. Anything that DOES carry
        // authority (config/hook/MCP drift, account, sandbox, responses, server
        // requests) stays fatal below. A flood or a wide spray of unknown methods
        // is protocol desync, not a new notification, and is still fatal.
        const seen = (this.unsupported.get(message.method) ?? 0) + 1;
        this.unsupported.set(message.method, seen);
        if (this.unsupported.size > MAX_UNSUPPORTED_NOTIFICATION_METHODS
            || seen > MAX_UNSUPPORTED_NOTIFICATIONS_PER_METHOD) {
          this.fail(new Error(
            `managed Codex flooded unsupported notification ${message.method}`,
          ));
          return;
        }
        if (seen === 1)
          console.error(`[codex] ignoring unsupported managed notification ${message.method}`);
        return;
      }
      try { this.onNotification(message.method, message.params); }
      catch (error) { this.fail(error instanceof Error ? error : new Error("managed Codex notification invalid")); }
      return;
    }
    const id = message.id;
    if (typeof id !== "number" && typeof id !== "string") {
      this.fail(new Error("managed Codex response has invalid id"));
      return;
    }
    const pending = this.pending.get(id);
    if (!pending) { this.fail(new Error("managed Codex response id is unknown")); return; }
    this.pending.delete(id);
    clearTimeout(pending.timer);
    if (("result" in message) === ("error" in message)) {
      const error = new Error(`managed Codex ${pending.method} response is malformed`);
      pending.reject(error); this.fail(error); return;
    }
    try { onlyKeys(message, ["id", "result" in message ? "result" : "error"],
      "managed Codex response"); }
    catch (error) { pending.reject(error as Error); this.fail(error as Error); return; }
    if ("error" in message) {
      // The JSON-RPC error object is the provider's own account of the failure
      // and used to die right here (thread 019f9cec). The outer message stays
      // the stable classification; the payload rides the cause, bounded and
      // canonicalized like exactDiagnosable — a JSON-RPC error carries
      // code/message/data, never a credential.
      const error = new Error(`managed Codex ${pending.method} failed`, {
        cause: new Error(
          `provider error response: ${JSON.stringify(canonical(message.error)).slice(0, 600)}`,
        ),
      });
      pending.reject(error); this.fail(error); return;
    }
    pending.resolve(message.result);
  }

  request(method: string, params?: unknown): Promise<unknown> {
    if (this.terminal) throw this.terminal;
    const id = ++this.nextId;
    const envelope = params === undefined ? { id, method } : { id, method, params };
    const line = `${JSON.stringify(envelope)}\n`;
    if (Buffer.byteLength(line, "utf8") > MAX_LINE_BYTES)
      throw new Error(`managed Codex ${method} request is oversized`);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const current = this.pending.get(id);
        if (!current) return;
        this.pending.delete(id);
        const error = new Error(`managed Codex ${method} timed out`);
        current.reject(error);
        this.fail(error);
      }, this.timeoutMs);
      timer.unref?.();
      this.pending.set(id, { method, timer, resolve, reject });
      this.writeLine(line, (error) => {
        if (error) this.fail(new Error(`managed Codex ${method} write failed`));
      });
    });
  }

  notify(method: string, params?: unknown): void {
    if (this.terminal) throw this.terminal;
    const line = `${JSON.stringify(params === undefined ? { method } : { method, params })}\n`;
    this.writeLine(line, (error) => {
      if (error) this.fail(new Error(`managed Codex ${method} notification failed`));
    });
  }

  assertHealthy(): void {
    if (this.terminal) throw this.terminal;
  }

  /** Unrecognized notification methods observed, per method. Evidence, not authority. */
  unsupportedNotifications(): Record<string, number> {
    return Object.fromEntries([...this.unsupported.entries()].sort(
      ([left], [right]) => left.localeCompare(right),
    ));
  }

  /** True when this session ended because the provider process itself ended. */
  diedFromProcessDeath(): boolean { return this.terminalFromProcessDeath; }

  /** Redacted provider stderr tail; empty when the supervisor owns that pipe. */
  stderrTail(count = STDERR_TAIL_LINES): string[] {
    return this.ownsStderr ? this.stderr.tail(count) : [];
  }

  markClosing(): void { this.closed = true; }
}

function configFingerprint(response: unknown): string {
  const body = record(response, "Codex config/read response");
  if (!Array.isArray(body.layers)) throw new Error("Codex config/read omitted layers");
  return JSON.stringify(canonical(body.layers.map((raw) => {
    const layer = record(raw, "Codex config layer");
    return {
      name: layer.name,
      version: layer.version,
      config: layer.config,
      ...(layer.disabledReason === undefined ? {} : { disabledReason: layer.disabledReason }),
    };
  })));
}

function validateDisabledProjectConfig(value: JsonObject): void {
  const serialized = JSON.stringify(value);
  if (typeof serialized !== "string")
    throw new Error("Codex disabled project layer is not JSON-serializable");
  parseStrictJson(serialized, "Codex disabled project layer", {
    maxBytes: MAX_DISABLED_PROJECT_CONFIG_BYTES,
    maxDepth: MAX_DISABLED_PROJECT_CONFIG_DEPTH,
    maxNodes: MAX_DISABLED_PROJECT_CONFIG_NODES,
  });
  const allowed = new Set<string>(REVIEWED_DISABLED_PROJECT_CONFIG_KEYS);
  const widened = Object.keys(value).filter((key) => !allowed.has(key)).sort();
  if (widened.length)
    // Name the offending keys. This is a terminal preflight failure — the lane
    // dies before its first turn — and the bare message left no way to tell
    // which key did it, so the same block recurred with nothing to act on.
    throw new Error(
      `Codex disabled project config widened authority: ${widened.join(", ")}`
      + ` (allowed: ${[...allowed].sort().join(", ")})`,
    );
}

function expectedProjectDisabledReason(contract: LaunchContract): string {
  return `${contract.projectRoot} is marked as untrusted in ${contract.codexHome}/config.toml. `
    + "To load project-local config, hooks, and exec policies, mark it trusted.";
}

function validateProjectConfigWarning(value: unknown, contract: LaunchContract): void {
  const warning = record(value, "Codex config warning");
  const summary = boundedProviderProse(warning.summary, "Codex config warning summary", 8_192);
  const details = warning.details === undefined || warning.details === null
    ? ""
    : boundedProviderProse(warning.details, "Codex config warning details", 8_192);
  const text = `${summary}\n${details}`;
  for (const identifier of [
    resolve(contract.projectRoot, ".codex"),
    resolve(contract.codexHome, "config.toml"),
  ]) {
    if (!text.includes(identifier))
      throw new Error(`Codex config warning omitted expected identifier: ${identifier}`);
  }
}

function validateConfig(
  response: unknown,
  contract: LaunchContract,
  projectWarningSeen = false,
): string {
  const body = record(response, "Codex config/read response");
  const config = record(body.config, "Codex effective config");
  if (!Array.isArray(body.layers)) throw new Error("Codex config/read omitted layers");
  const layers = body.layers.map((raw) => record(raw, "Codex config layer"));
  const seen = new Map<string, number>();
  let projectWarningRequired = false;
  for (const layer of layers) {
    const name = record(layer.name, "Codex config layer name");
    const type = boundedString(name.type, "Codex config layer type", 64);
    seen.set(type, (seen.get(type) ?? 0) + 1);
    if (typeof layer.version !== "string" || !/^sha256:[0-9a-f]{64}$/.test(layer.version))
      throw new Error("Codex config layer has invalid version");
    const layerConfig = record(layer.config, "Codex config layer payload");
    if (type === "sessionFlags") {
      exactDiagnosable(
        record(layerConfig.features, "Codex session authority feature set").network_proxy,
        record(contract.expectedSessionConfig.features, "Codex expected session authority feature set").network_proxy,
        "Codex session network proxy policy",
      );
      // Keys-only diagnosis: this layer embeds MCP server env, so its VALUES
      // may carry a token and must never reach the message.
      exactNamingKeys(layerConfig, contract.expectedSessionConfig,
        "Codex session authority layer");
    } else if (type === "project") {
      onlyKeys(layer, layer.disabledReason === undefined
        ? ["name", "version", "config"]
        : ["name", "version", "config", "disabledReason"], "Codex project layer");
      onlyKeys(name, ["type", "dotCodexFolder"], "Codex project layer name");
      if (layer.disabledReason !== undefined)
        boundedString(layer.disabledReason, "Codex project layer disabled reason", 4_096);
      validateDisabledProjectConfig(layerConfig);
      if (Object.keys(layerConfig).length > 0) {
        if (layer.disabledReason !== expectedProjectDisabledReason(contract))
          throw new Error(
            "Codex populated project layer lacks its exact structured disabled reason",
          );
        projectWarningRequired = true;
      }
      if (boundedString(name.dotCodexFolder, "Codex project layer folder", 4_096)
          !== join(contract.projectRoot, ".codex"))
        throw new Error("Codex project layer names an invalid config folder");
    } else if (type === "user") {
      mustBeEmptyLayer(layerConfig, "Codex user layer");
      if (name.profile !== null || name.file !== resolve(contract.codexHome, "config.toml"))
        throw new Error("Codex user layer names the wrong account");
    } else if (type === "system" || type === "mdm" || type === "enterpriseManaged"
        || type === "legacyManagedConfigTomlFromFile" || type === "legacyManagedConfigTomlFromMdm") {
      mustBeEmptyLayer(layerConfig, `Codex ${type} layer`);
    } else {
      throw new Error(`Codex exposed unknown config layer ${type}`);
    }
  }
  if (seen.get("sessionFlags") !== 1 || seen.get("user") !== 1)
    throw new Error("Codex config layer authority is incomplete");
  if (projectWarningRequired && !projectWarningSeen)
    throw new Error("Codex tracked project layer lacks its correlated disabled warning");

  const expectedFeatures = Object.fromEntries([
    ...MANAGED_CODEX_ENABLED_FEATURES.map((name) => [name, true] as const),
    ...MANAGED_CODEX_DISABLED_FEATURES.map((name) => [name, false] as const),
    ["network_proxy", contract.network.networkProxyEnabled
      ? { enabled: true, domains: contract.network.domains }
      : false] as const,
    ["remote_control", false] as const,
  ]);
  exactDiagnosable(
    record(config.features, "Codex effective feature set").network_proxy,
    expectedFeatures.network_proxy,
    "Codex effective network proxy policy",
  );
  exactNamingKeys(config.features, expectedFeatures, "Codex effective feature set");
  const sessionMcp = record(
    contract.expectedSessionConfig.mcp_servers, "Codex expected MCP session set",
  );
  const expectedEffectiveMcp = Object.fromEntries(Object.entries(sessionMcp).map(
    ([name, raw]) => [name, {
      ...record(raw, `Codex expected MCP session ${name}`),
      environment_id: "local",
      tool_timeout_sec: null,
    }],
  ));
  exactNamingKeys(config.mcp_servers, expectedEffectiveMcp, "Codex effective MCP set");
  exactNamingKeys(config.projects, contract.expectedSessionConfig.projects,
    "Codex project trust set");
  const sessionShellEnvironmentPolicy = record(
    contract.expectedSessionConfig.shell_environment_policy,
    "Codex expected shell environment policy",
  );
  exactNamingKeys(
    config.shell_environment_policy,
    {
      ...sessionShellEnvironmentPolicy,
      ignore_default_excludes: null,
      exclude: null,
      include_only: null,
      // 0.146 supersedes the legacy exclude/include_only pair; unset must stay
      // unset, so an inherited filter set still fails closed.
      filters: null,
      experimental_use_profile: null,
    },
    "Codex effective shell environment policy",
  );
  if (config.project_doc_max_bytes !== 0 || config.model_provider !== "openai"
      || config.cli_auth_credentials_store !== "file" || config.forced_login_method !== "chatgpt"
      || config.sqlite_home !== contract.sqliteHome || config.allow_login_shell !== false
      || config.apps !== null
      || JSON.stringify(config.plugins) !== "{}" || JSON.stringify(config.marketplaces) !== "{}")
    throw new Error("Codex effective authority surface is not closed");
  return configFingerprint(response);
}

function camelEvent(value: string): string {
  return value[0]!.toLowerCase() + value.slice(1);
}

function expectedHookRows(): Array<JsonObject> {
  const rows: Array<JsonObject> = [];
  for (const [event, groups] of Object.entries(expectedManagedCodexHooks())) {
    for (const group of groups) for (const hook of group.hooks) rows.push({
      eventName: camelEvent(event),
      handlerType: "command",
      matcher: group.matcher ?? null,
      command: hook.command,
      timeoutSec: hook.timeout,
      sourcePath: "/etc/codex/hooks",
      source: "system",
      enabled: true,
      isManaged: true,
      trustStatus: "managed",
    });
  }
  return rows.sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)));
}

function validateRequirements(response: unknown, contract: LaunchContract): void {
  const body = record(response, "Codex requirements response");
  const requirements = record(body.requirements, "Codex requirements");
  if (requirements.allowManagedHooksOnly !== true || requirements.allowRemoteControl !== false
      || (requirements.managedHookFailureMode === undefined
        ? contract.installedManagedHookFailureMode !== "block"
        : requirements.managedHookFailureMode !== "block"))
    throw new Error("Codex requirements do not close managed hooks, failures, and remote control");
  exact(requirements.featureRequirements, { hooks: true }, "Codex feature requirements");
  const hooks = record(requirements.hooks, "Codex managed hook requirements");
  if (hooks.managedDir !== "/etc/codex/hooks")
    throw new Error("Codex managed hook requirements name the wrong directory");
}

function validateHooks(response: unknown, cwd: string): void {
  const body = record(response, "Codex hooks/list response");
  if (!Array.isArray(body.data) || body.data.length !== 1)
    throw new Error("Codex hooks/list returned the wrong cwd cardinality");
  const entry = record(body.data[0], "Codex hook cwd entry");
  if (entry.cwd !== cwd || !Array.isArray(entry.hooks)
      || !Array.isArray(entry.warnings) || entry.warnings.length
      || !Array.isArray(entry.errors) || entry.errors.length)
    throw new Error("Codex hook inventory has warnings, errors, or the wrong cwd");
  const rows = entry.hooks.map((raw) => {
    const hook = record(raw, "Codex hook metadata");
    return {
      eventName: hook.eventName,
      handlerType: hook.handlerType,
      matcher: hook.matcher,
      command: hook.command,
      timeoutSec: hook.timeoutSec,
      sourcePath: hook.sourcePath,
      source: hook.source,
      enabled: hook.enabled,
      isManaged: hook.isManaged,
      trustStatus: hook.trustStatus,
    };
  }).sort((left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)));
  exact(rows, expectedHookRows(), "Codex managed hook inventory");
}

/**
 * One MCP server North expects the managed session to be carrying, with the
 * exact tool grant it may expose. `version` is pinned only where North itself
 * ships the server binary: the fram graph-authoring server is a separate
 * deployment (`bin/fram-mcp` out of NORTH_FRAM_HOME) whose version string North
 * does not own, so pinning it here would turn an upstream version bump into a
 * dead codex graph lane. Its identity is still fenced — exact name, no title/
 * description/icons/website, no auth authority, no resources, exact tool set.
 */
interface ExpectedMcpServer {
  name: string;
  tools: readonly string[];
  version?: string;
}

/**
 * The sealed MCP inventory for one authority surface. North is unconditional;
 * fram appears exactly when the surface carries the graph-authoring capability,
 * which is the same condition that injected the server into the launch config.
 */
function expectedMcpInventory(surface: OpenAIAuthoritySurface): readonly ExpectedMcpServer[] {
  return Object.freeze([
    { name: "north", tools: surface.northEnabledTools, version: "0.1.0" },
    ...(surface.capabilities.includes(FRAM_GRAPH_AUTHORING_CAPABILITY)
      ? [{ name: FRAM_MCP_SERVER, tools: FRAM_MCP_TOOL_NAMES as readonly string[] }]
      : []),
  ]);
}

async function validateMcp(
  rpc: AppServerRpc,
  expected: readonly ExpectedMcpServer[],
  threadId?: string,
): Promise<void> {
  const servers: JsonObject[] = [];
  const cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < MAX_INVENTORY_PAGES; page++) {
    const response = record(await rpc.request("mcpServerStatus/list", {
      detail: "full",
      limit: 32,
      ...(cursor ? { cursor } : {}),
      ...(threadId ? { threadId } : {}),
    }), "Codex MCP inventory");
    if (!Array.isArray(response.data)) throw new Error("Codex MCP inventory data is invalid");
    for (const raw of response.data) {
      servers.push(record(raw, "Codex MCP server"));
      if (servers.length > MAX_MCP_SERVERS) throw new Error("Codex MCP inventory is oversized");
    }
    if (response.nextCursor == null) break;
    cursor = boundedString(response.nextCursor, "Codex MCP cursor", 4096);
    if (cursors.has(cursor)) throw new Error("Codex MCP cursor repeated");
    cursors.add(cursor);
    if (page === MAX_INVENTORY_PAGES - 1) throw new Error("Codex MCP inventory did not terminate");
  }
  const expectedNames = expected.map((server) => server.name);
  const observed = new Map<string, JsonObject>();
  for (const server of servers) {
    const name = boundedString(server.name, "Codex MCP server name");
    if (observed.has(name)) throw new Error("Codex MCP inventory repeated a server");
    observed.set(name, server);
  }
  if (observed.size !== expected.length || expectedNames.some((name) => !observed.has(name)))
    throw new Error(
      `Codex MCP inventory is not exactly ${expectedNames.join("+")}: `
      + `observed ${[...observed.keys()].sort().join("+") || "(none)"}`,
    );
  for (const spec of expected) {
    const label = `Codex ${spec.name} MCP server`;
    const server = observed.get(spec.name)!;
    onlyKeys(server, [
      "name", "serverInfo", "tools", "resources", "resourceTemplates", "authStatus",
    ], label);
    const identity = record(server.serverInfo, `${label} identity`);
    exact(identity, {
      name: spec.name,
      title: null,
      version: spec.version ?? boundedString(identity.version, `${label} version`, 64),
      description: null,
      icons: null,
      websiteUrl: null,
    }, `${label} identity`);
    if (server.authStatus !== "unsupported")
      throw new Error(`${label} unexpectedly carries authentication authority`);
    exact(server.resources, [], `${label} resource surface`);
    exact(server.resourceTemplates, [], `${label} resource-template surface`);
    const tools = record(server.tools, `${label} tools`);
    exact(Object.keys(tools).sort(), [...spec.tools].sort(), `${label} tool surface`);
  }
}

function validateAccount(response: unknown): void {
  const body = record(response, "Codex account/read response");
  const account = record(body.account, "Codex authenticated account");
  if (account.type !== "chatgpt" || body.requiresOpenaiAuth !== true)
    throw new Error("Codex selected account is not authenticated ChatGPT");
}

function validateInitialize(response: unknown, contract: LaunchContract): void {
  const initialized = record(response, "Codex initialize response");
  onlyKeys(initialized, ["userAgent", "codexHome", "platformFamily", "platformOs"],
    "Codex initialize response");
  const expectedPlatformOs = process.platform === "darwin" ? "macos"
    : process.platform === "linux" ? "linux"
    : undefined;
  const userAgent = typeof initialized.userAgent === "string"
    ? initialized.userAgent
    : "";
  const expectedUserAgent = `north/${MANAGED_CODEX_VERSION}`;
  if (initialized.codexHome !== contract.codexHome
      || !expectedPlatformOs || initialized.platformFamily !== "unix"
      || initialized.platformOs !== expectedPlatformOs
      || Buffer.byteLength(userAgent, "utf8") > 512
      || /[\u0000-\u001f\u007f]/.test(userAgent)
      || (userAgent !== expectedUserAgent && !userAgent.startsWith(`${expectedUserAgent} `)))
    throw new Error("Codex initialize did not attest the pinned provider runtime");
}

function expectedSandbox(
  surface: OpenAIAuthoritySurface,
  contract: LaunchContract,
): JsonObject {
  const network = managedCodexNetworkPolicy(surface);
  return surface.sandbox === "read-only"
    ? { type: "readOnly", networkAccess: false }
    : {
      type: "workspaceWrite",
      // Exactly the Git metadata roots the contract granted — never a wider set,
      // and its sealed command-network grant. A drift in either direction fails closed.
      writableRoots: contract.writableRoots,
      networkAccess: network.networkAccess,
      excludeTmpdirEnvVar: false,
      excludeSlashTmp: false,
    };
}

/**
 * Codex reports a thread's runtime workspace roots as the cwd PLUS every
 * configured `sandbox_workspace_write.writable_roots` entry. e3921fa granted the
 * Git metadata roots so managed lanes could commit, but left this assertion at
 * `[cwd]` — the grant and its proof are ONE authority statement and drifted
 * apart, so every workspace-write managed Codex lane died at thread/start before
 * its first turn (observed 2026-07-26: observed=[cwd, cwd/.git] expected=[cwd]).
 * Derive the expectation from the same contract field that produced the grant.
 */
function expectedRuntimeWorkspaceRoots(contract: LaunchContract): string[] {
  return [...new Set([contract.cwd, ...contract.writableRoots])].sort();
}

// Root ORDER is not authority — the root SET is. Sort a well-formed string list
// so the comparison stays fail-closed on membership while tolerating whatever
// order Codex echoes; anything else passes through unchanged and fails.
function comparableRootList(value: unknown): unknown {
  return Array.isArray(value) && value.every((entry) => typeof entry === "string")
    ? [...(value as string[])].sort()
    : value;
}

function validateStartedThread(
  response: unknown,
  contract: LaunchContract,
  options: ManagedCodexAppServerOptions,
): string {
  const started = record(response, "Codex thread/start response");
  const thread = record(started.thread, "Codex started thread");
  const threadId = providerThreadId(started, thread, "Codex thread/start response");
  if (started.model !== options.model || started.modelProvider !== "openai"
      || started.serviceTier !== null || started.cwd !== contract.cwd
      || thread.ephemeral !== true || thread.modelProvider !== "openai"
      || thread.cwd !== contract.cwd || thread.parentThreadId !== null
      || started.approvalPolicy !== "never" || started.approvalsReviewer !== "user"
      || started.activePermissionProfile !== null
      || started.reasoningEffort !== (options.effort ?? null)
      || started.multiAgentMode !== "explicitRequestOnly")
    throw new Error("Codex thread/start resolved different execution authority");
  exactDiagnosable(
    comparableRootList(started.runtimeWorkspaceRoots),
    expectedRuntimeWorkspaceRoots(contract),
    "Codex thread runtime workspace roots",
  );
  exactDiagnosable(started.instructionSources, [resolve(contract.codexHome, "AGENTS.md")],
    "Codex thread instruction sources");
  exactDiagnosable(started.sandbox, expectedSandbox(options.surface, contract),
    "Codex thread sandbox");
  return threadId;
}

function validateStartedTurn(response: unknown): string {
  const started = record(response, "Codex turn/start response");
  const turn = record(started.turn, "Codex started turn");
  const turnId = protocolId(turn.id, "Codex turn id");
  if (turn.status !== "inProgress" || turn.error !== null || !Array.isArray(turn.items)
      || turn.items.length !== 0)
    throw new Error("Codex turn did not start with the exact managed lifecycle");
  return turnId;
}

interface RuntimeNotificationState {
  threadId: string;
  cwd: string;
  model: string;
  turnId?: string;
  hookRuns: Set<string>;
  text: string;
  usage?: ManagedCodexResult["usage"];
  terminalSeen: boolean;
  /** Completed non-message, non-reasoning items observed in the LIVE turn. */
  toolItems: number;
  mcpActivity: McpActivityAccumulator;
  nativeCommands: NativeCommandActivityAccumulator;
  /** Names of the MCP servers this session's sealed authority actually grants. */
  mcpServerNames: readonly string[];
}

/**
 * The one definition of a counted work item, shared by both Codex transports
 * (the exec transport spells the same two exclusions `agent_message` /
 * `reasoning`). Reasoning blocks complete on every non-trivial turn whether or
 * not a single tool ran, so counting them would make the item count useless
 * for its only purpose: showing that a tool loop actually happened.
 */
function countsAsToolItem(itemType: string): boolean {
  return itemType !== "agentMessage" && itemType !== "reasoning";
}

function commandText(value: unknown, label: string, maxBytes = MAX_LINE_BYTES): string {
  if (typeof value !== "string" || !value
      || Buffer.byteLength(value, "utf8") > maxBytes)
    throw new Error(`${label} is invalid`);
  return value;
}

/**
 * A commandExecution item reports the SUBPROCESS working directory, not the
 * thread's. Codex's own shell tool takes a per-command `workdir` and its tool
 * guidance instructs the model to "Always set the `workdir` param when using
 * the shell_command function. Do not use `cd` unless absolutely necessary."
 * (observed 2026-07-26 in the pinned codex 0.144.4 binary, alongside
 * `ShellCommandToolCallParams { command, workdir, ... }` and
 * `codex_core::tools::handlers::resolve_workdir_base_path`); real rollouts show
 * workdirs that are subdirectories of the session cwd and sibling checkouts.
 * Requiring `item.cwd === state.cwd` therefore killed every managed lane whose
 * brief reached into a subdirectory — mid-turn, after dozens of good commands
 * (lane ms1fhh0v: 79 native commands, then dead on the 80th).
 *
 * The bound that remains is SHAPE, which is what fail-closed can actually
 * assert here: a bounded, absolute, traversal-free path. Thread and turn
 * identity are checked exactly upstream (`exactRuntimeIds`) and write authority
 * is the sandbox's, sealed and diffed at thread/start — a reported cwd grants
 * nothing, so LOCATION is not a bound North can make load-bearing without
 * reinstating the outage (a lane legitimately reads sibling checkouts, /tmp
 * scratch, and the main tree it writes its report into). The returned path is
 * normalized so evidence comparison is canonical while acceptance stays wide.
 */
function nativeCommandCwd(value: unknown, label: string): string {
  const observed = commandText(value, `${label} cwd`, MAX_CWD_BYTES);
  const segments = observed.split("/");
  if (observed.startsWith("/") && !observed.includes("\0")
      && !segments.includes(".") && !segments.includes(".."))
    return resolve(observed);
  throw new Error(`${label} cwd is not an absolute traversal-free path`, {
    cause: new Error(`observed=${JSON.stringify(observed).slice(0, 600)} `
      + "expected an absolute filesystem path with no \".\"/\"..\" segments"),
  });
}

function nullableCommandText(value: unknown, label: string): string {
  if (value === null) return "";
  if (typeof value !== "string" || Buffer.byteLength(value, "utf8") > MAX_LINE_BYTES)
    throw new Error(`${label} is invalid`);
  return value;
}

function validateCommandAction(value: unknown): void {
  const action = record(value, "Codex command action");
  const type = boundedString(action.type, "Codex command action type", 32);
  if (type === "read") {
    onlyKeys(action, ["type", "command", "name", "path"], "Codex read command action");
    commandText(action.command, "Codex read command action command");
    commandText(action.name, "Codex read command action name", 4_096);
    commandText(action.path, "Codex read command action path");
    return;
  }
  if (type === "listFiles") {
    onlyKeys(action, ["type", "command", "path"], "Codex list-files command action");
    commandText(action.command, "Codex list-files command action command");
    if (action.path !== null) commandText(action.path, "Codex list-files command action path");
    return;
  }
  if (type === "search") {
    onlyKeys(action, ["type", "command", "query", "path"], "Codex search command action");
    commandText(action.command, "Codex search command action command");
    if (action.query !== null) commandText(action.query, "Codex search command action query");
    if (action.path !== null) commandText(action.path, "Codex search command action path");
    return;
  }
  if (type === "unknown") {
    onlyKeys(action, ["type", "command"], "Codex unknown command action");
    commandText(action.command, "Codex unknown command action command");
    return;
  }
  throw new Error("Codex command action type is invalid");
}

/** 0.146 attributes a command to a first-party plugin script; North closes
 * `plugins`, so any attribution at all means an unsealed execution path. */
function assertNoPluginProvenance(item: JsonObject, label: string): void {
  if (item.pluginId !== null || item.scriptPath !== null)
    throw new Error(`${label} was attributed to a plugin script`, {
      cause: new Error(`pluginId=${JSON.stringify(item.pluginId)?.slice(0, 200)} `
        + `scriptPath=${JSON.stringify(item.scriptPath)?.slice(0, 200)}`),
    });
}

function completedNativeCommand(
  item: JsonObject,
  state: RuntimeNotificationState,
): void {
  onlyKeys(item, [
    "id", "type", "command", "cwd", "processId", "source", "status",
    "commandActions", "aggregatedOutput", "exitCode", "durationMs",
    "pluginId", "scriptPath",
  ], "Codex completed command execution");
  assertNoPluginProvenance(item, "Codex completed command execution");
  const id = protocolId(item.id, "Codex completed command execution id");
  if (item.type !== "commandExecution")
    throw new Error("Codex completed command execution changed authority");
  const cwd = nativeCommandCwd(item.cwd, "Codex completed command execution");
  const command = commandText(item.command, "Codex completed command execution command");
  if (item.processId !== null) protocolId(item.processId, "Codex command process id");
  const source = String(item.source);
  if (!["agent", "userShell", "unifiedExecStartup", "unifiedExecInteraction"].includes(source))
    throw new Error("Codex completed command execution source is invalid");
  const status = String(item.status) as NativeCommandStatus;
  if (!["completed", "failed", "declined"].includes(status))
    throw new Error("Codex completed command execution status is not terminal");
  if (!Array.isArray(item.commandActions) || item.commandActions.length > 256)
    throw new Error("Codex completed command actions are invalid");
  for (const action of item.commandActions) validateCommandAction(action);
  const aggregatedOutput = nullableCommandText(
    item.aggregatedOutput, "Codex completed command execution output",
  );
  if (!Number.isSafeInteger(item.exitCode)
      || (item.exitCode as number) < -2_147_483_648
      || (item.exitCode as number) > 2_147_483_647)
    throw new Error("Codex completed command execution exit code is invalid");
  if (!Number.isSafeInteger(item.durationMs) || (item.durationMs as number) < 0)
    throw new Error("Codex completed command execution duration is invalid");
  if (!state.nativeCommands.observe({
    id: `${state.turnId}:${id}`,
    command,
    // The OBSERVED directory, not the thread's: the North binary probe is only
    // evidence when it ran at the lane root, and that comparison is dead if we
    // hand the accumulator the expected value back.
    cwd,
    source: source as "agent" | "userShell" | "unifiedExecStartup" | "unifiedExecInteraction",
    status,
    aggregatedOutput,
    exitCode: item.exitCode as number,
  })) throw new Error("Codex command completion is missing its exact start");
}

function startedNativeCommand(item: JsonObject, state: RuntimeNotificationState): void {
  onlyKeys(item, [
    "id", "type", "command", "cwd", "processId", "source", "status",
    "commandActions", "aggregatedOutput", "exitCode", "durationMs",
    "pluginId", "scriptPath",
  ], "Codex started command execution");
  assertNoPluginProvenance(item, "Codex started command execution");
  const id = protocolId(item.id, "Codex started command execution id");
  nativeCommandCwd(item.cwd, "Codex started command execution");
  if (item.type !== "commandExecution"
      || item.status !== "inProgress" || item.aggregatedOutput !== null
      || item.exitCode !== null || item.durationMs !== null)
    throw new Error("Codex started command execution lifecycle is invalid", {
      cause: new Error("expected type \"commandExecution\", status \"inProgress\", null "
        + "aggregatedOutput/exitCode/durationMs; observed "
        + `type=${JSON.stringify(item.type)} status=${JSON.stringify(item.status)} `
        + `aggregatedOutput=${JSON.stringify(item.aggregatedOutput)?.slice(0, 200)} `
        + `exitCode=${JSON.stringify(item.exitCode)} durationMs=${JSON.stringify(item.durationMs)}`),
    });
  commandText(item.command, "Codex started command execution command");
  if (item.processId !== null) protocolId(item.processId, "Codex command process id");
  if (!["agent", "userShell", "unifiedExecStartup", "unifiedExecInteraction"]
    .includes(String(item.source)))
    throw new Error("Codex started command execution source is invalid");
  if (!Array.isArray(item.commandActions) || item.commandActions.length > 256)
    throw new Error("Codex started command actions are invalid");
  for (const action of item.commandActions) validateCommandAction(action);
  if (!state.nativeCommands.start(`${state.turnId}:${id}`))
    throw new Error("Codex command start lifecycle is invalid");
}

function validateMcpStartupNotification(
  value: unknown,
  expectedThreadId: string | undefined,
  expectedNames: readonly string[],
  allowPendingThreadId = false,
): JsonObject {
  const params = record(value, "Codex MCP startup notification");
  onlyKeys(params, ["threadId", "name", "status", "error", "failureReason"],
    "Codex MCP startup notification");
  let validThreadId = params.threadId === null;
  if (typeof params.threadId === "string") {
    try {
      protocolId(params.threadId, "Codex MCP startup thread id");
      validThreadId = expectedThreadId === undefined
        ? allowPendingThreadId
        : params.threadId === expectedThreadId;
    } catch { validThreadId = false; }
  }
  if (!validThreadId || !expectedNames.includes(String(params.name))
      || !["starting", "ready"].includes(String(params.status))
      || params.error !== null || params.failureReason !== null) {
    const expected = expectedThreadId === undefined
      ? (allowPendingThreadId ? "null or the pending thread/start protocol id" : "null")
      : `null or ${JSON.stringify(expectedThreadId)}`;
    throw new Error(`Codex managed MCP startup status is invalid: expected threadId ${expected}, `
      + `name ${expectedNames.map((name) => JSON.stringify(name)).join("|")}, `
      + `status \"starting\"|\"ready\", error null, failureReason null; `
      + `observed ${JSON.stringify(canonical(params))}`);
  }
  return params;
}

function validateSafetyBufferingNotification(
  value: unknown,
  state: RuntimeNotificationState,
): void {
  const params = record(value, "Codex safety-buffering notification");
  const keys = ["threadId", "turnId", "model", "useCases", "reasons", "showBufferingUi"];
  if ("fasterModel" in params) keys.push("fasterModel");
  onlyKeys(params, keys, "Codex safety-buffering notification");
  exactRuntimeIds(params, state, "Codex safety-buffering notification");
  if (boundedString(params.model, "Codex safety-buffering model") !== state.model)
    throw new Error("Codex safety-buffering notification changed the active model");
  for (const [key, label] of [["useCases", "use case"], ["reasons", "reason"]] as const) {
    const values = params[key];
    if (!Array.isArray(values) || values.length > MAX_SAFETY_BUFFERING_VALUES)
      throw new Error(`Codex safety-buffering ${key} are invalid`);
    values.forEach((entry, index) => boundedString(
      entry, `Codex safety-buffering ${label} ${index}`, MAX_SAFETY_BUFFERING_VALUE_BYTES,
    ));
  }
  if (typeof params.showBufferingUi !== "boolean")
    throw new Error("Codex safety-buffering UI flag is invalid");
  if ("fasterModel" in params)
    optionalBoundedString(params.fasterModel, "Codex safety-buffering faster model");
}

function validateNotifiedTurn(
  value: unknown,
  expectedId: string | undefined,
  expectedStatus: "inProgress" | "completed",
  label: string,
): void {
  const turn = record(value, label);
  const turnId = protocolId(turn.id, `${label} id`);
  // A turn that reports its OWN error is the provider telling us why the lane
  // failed; folding it into the generic "is invalid" below discarded the only
  // account of the failure that ever existed (thread 019f9cec). Name it, and
  // carry the bounded payload on the cause — same discipline as
  // exactDiagnosable: a turn error is credential-free by shape.
  if (turn.error !== null)
    throw new Error(`${label} reported a provider-side turn error`, {
      cause: new Error(
        `provider turn error: ${JSON.stringify(canonical(turn.error)).slice(0, 600)}`,
      ),
    });
  // Names the failing predicate: a bare "is invalid" over seven conditions is
  // the same unnameable-drift class a Codex version bump keeps producing.
  const invalid = (reasons: readonly (string | false)[]): void => {
    const named = reasons.filter((reason): reason is string => reason !== false);
    if (!named.length) return;
    throw new Error(`${label} is invalid`, { cause: new Error(named.join(", ")) });
  };
  invalid([
    !expectedId && "no expected turn id",
    expectedId !== undefined && turnId !== expectedId && "turn id is not the started turn",
    turn.status !== expectedStatus
      && `status ${JSON.stringify(turn.status)} is not ${JSON.stringify(expectedStatus)}`,
    !Array.isArray(turn.items) && "items is not an array",
    // 0.146 hydrates some turns with a summarized items view; neither admitted
    // view's items are consumed, so both stay sealed to lifecycle-only reads.
    turn.itemsView !== "notLoaded" && turn.itemsView !== "summary"
      && `itemsView ${JSON.stringify(turn.itemsView)}`,
    (!Number.isSafeInteger(turn.startedAt) || (turn.startedAt as number) < 0)
      && `startedAt ${JSON.stringify(turn.startedAt)}`,
  ]);
  if (expectedStatus === "inProgress") {
    invalid([
      turn.completedAt !== null && `completedAt ${JSON.stringify(turn.completedAt)}`,
      turn.durationMs !== null && `durationMs ${JSON.stringify(turn.durationMs)}`,
      (turn.items as unknown[]).length !== 0
        && `items carries ${(turn.items as unknown[]).length}`,
    ]);
    return;
  }
  invalid([
    (!Number.isSafeInteger(turn.completedAt) || (turn.completedAt as number) < 0)
      && `completedAt ${JSON.stringify(turn.completedAt)}`,
    (!Number.isSafeInteger(turn.durationMs) || (turn.durationMs as number) < 0)
      && `durationMs ${JSON.stringify(turn.durationMs)}`,
  ]);
}

function exactRuntimeIds(
  params: JsonObject,
  state: RuntimeNotificationState,
  label: string,
  requireTurn = true,
): void {
  if (params.threadId !== state.threadId)
    throw new Error(`${label} belongs to another thread`);
  if (requireTurn && (!state.turnId || params.turnId !== state.turnId))
    throw new Error(`${label} belongs to another turn`);
}

function validateHookNotification(
  method: "hook/started" | "hook/completed",
  value: unknown,
  state: RuntimeNotificationState,
): void {
  const params = record(value, "Codex hook notification");
  onlyKeys(params, ["threadId", "turnId", "run"], "Codex hook notification");
  const run = record(params.run, "Codex hook run");
  onlyKeys(run, [
    "id", "eventName", "handlerType", "executionMode", "scope", "sourcePath", "source",
    "displayOrder", "status", "statusMessage", "startedAt", "completedAt", "durationMs",
    "entries",
  ], "Codex hook run");
  const id = boundedString(run.id, "Codex hook run id", 512);
  const allowedEvents = new Set(Object.keys(expectedManagedCodexHooks()).map(camelEvent));
  const eventName = boundedString(run.eventName, "Codex hook event", 64);
  const threadScoped = eventName === "sessionStart";
  if (params.threadId !== state.threadId)
    throw new Error("Codex hook belongs to another thread");
  if (threadScoped) {
    if (params.turnId !== null) protocolId(params.turnId, "Codex session hook turn id");
  } else if (!state.turnId || params.turnId !== state.turnId) {
    throw new Error("Codex hook belongs to another turn");
  }
  if (!allowedEvents.has(eventName)
      || run.handlerType !== "command" || run.executionMode !== "sync"
      || run.scope !== (threadScoped ? "thread" : "turn")
      || run.sourcePath !== "/etc/codex/hooks" || run.source !== "system"
      || !Number.isSafeInteger(run.displayOrder) || (run.displayOrder as number) < 0
      || !Number.isSafeInteger(run.startedAt) || (run.startedAt as number) < 0
      || !Array.isArray(run.entries) || run.entries.length > 64)
    throw new Error("Codex hook run provenance is invalid");
  let hasNonemptyFeedback = false;
  for (const raw of run.entries) {
    const entry = record(raw, "Codex hook output entry");
    onlyKeys(entry, ["kind", "text"], "Codex hook output entry");
    if (!["warning", "stop", "feedback", "context", "error"].includes(String(entry.kind)))
      throw new Error("Codex hook output kind is invalid");
    if (typeof entry.text !== "string" || Buffer.byteLength(entry.text, "utf8") > 64 * 1024)
      throw new Error("Codex hook output is invalid");
    if (entry.kind === "feedback" && String(entry.text).trim())
      hasNonemptyFeedback = true;
  }
  if (method === "hook/started") {
    if (run.status !== "running" || run.statusMessage !== null || run.completedAt !== null
        || run.durationMs !== null || run.entries.length !== 0 || state.hookRuns.has(id))
      throw new Error("Codex hook start lifecycle is invalid");
    // The app-server protocol carries a fresh HookRunSummary at each lifecycle
    // notification. `id` is the only pairwise lifecycle identity; other fields
    // are notification-local summary/provenance and are validated above.
    state.hookRuns.add(id);
    return;
  }
  if (!state.hookRuns.has(id))
    throw new Error("Codex hook completion is missing its start");
  state.hookRuns.delete(id);
  // A PreToolUse "blocked" with feedback is the guard SUCCEEDING at policy:
  // the command is denied, the model reads the denial, the turn continues.
  const validCompletion = run.status === "completed"
    || (eventName === "preToolUse" && run.status === "blocked" && hasNonemptyFeedback);
  if (!validCompletion || run.completedAt === null || run.durationMs === null
      || !Number.isSafeInteger(run.completedAt) || !Number.isSafeInteger(run.durationMs)
      || (run.completedAt as number) < (run.startedAt as number)
      || (run.durationMs as number) < 0 || run.statusMessage !== null)
    throw new Error("Codex managed hook did not complete successfully");
}

function validateProgressNotification(
  method: string,
  value: unknown,
  state: RuntimeNotificationState,
): void {
  const params = record(value, `Codex ${method} notification`);
  if (method === "thread/started") {
    onlyKeys(params, ["thread"], "Codex thread/started notification");
    const thread = record(params.thread, "Codex thread/started thread");
    if (providerThreadId(params, thread, "Codex thread/started notification") !== state.threadId
        || thread.ephemeral !== true
        || thread.modelProvider !== "openai" || thread.cwd !== state.cwd
        || thread.parentThreadId !== null)
      throw new Error("Codex thread/started notification changed authority");
    return;
  }
  if (method === "thread/status/changed") {
    onlyKeys(params, ["threadId", "status"], "Codex thread status notification");
    exactRuntimeIds(params, state, "Codex thread status", false);
    const status = record(params.status, "Codex thread status");
    if (!["idle", "active"].includes(String(status.type)))
      throw new Error("Codex thread entered an invalid managed status");
    return;
  }
  if (method === "turn/started") {
    onlyKeys(params, ["threadId", "turn"], "Codex turn/started notification");
    if (params.threadId !== state.threadId) throw new Error("Codex turn belongs to another thread");
    validateNotifiedTurn(params.turn, state.turnId, "inProgress", "Codex turn/started notification");
    return;
  }
  if (method === "thread/tokenUsage/updated") {
    state.usage = usageFromNotification(params, state.threadId, state.turnId!);
    return;
  }
  if (method === "item/started" || method === "item/completed") {
    onlyKeys(params, ["item", "threadId", "turnId",
      method === "item/started" ? "startedAtMs" : "completedAtMs"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    const timestamp = params[method === "item/started" ? "startedAtMs" : "completedAtMs"];
    if (!Number.isSafeInteger(timestamp) || (timestamp as number) < 0)
      throw new Error(`Codex ${method} timestamp is invalid`);
    const item = record(params.item, `Codex ${method} item`);
    protocolId(item.id, `Codex ${method} item id`);
    const itemType = boundedString(item.type, `Codex ${method} item type`, 128);
    if (method === "item/completed" && countsAsToolItem(itemType)) state.toolItems += 1;
    if (method === "item/started" && item.type === "commandExecution")
      startedNativeCommand(item, state);
    if (method === "item/completed" && item.type === "agentMessage") {
      if (typeof item.text !== "string") throw new Error("Codex agent message text is invalid");
      state.text = item.text;
    }
    if (method === "item/completed" && item.type === "mcpToolCall") {
      state.mcpActivity.observe(
        `${state.turnId}:${String(item.id)}`,
        normalizeCodexMcpIdentity(item.server, item.tool),
      );
    }
    if (method === "item/completed" && item.type === "commandExecution")
      completedNativeCommand(item, state);
    return;
  }
  if (method === "turn/completed") {
    onlyKeys(params, ["threadId", "turn"], "Codex turn completion");
    if (state.terminalSeen) throw new Error("Codex emitted multiple turn terminals");
    if (params.threadId !== state.threadId) throw new Error("Codex turn terminal is for another thread");
    validateNotifiedTurn(params.turn, state.turnId, "completed", "Codex completed turn");
    if (state.hookRuns.size) throw new Error("Codex turn completed with unfinished managed hooks");
    state.terminalSeen = true;
    return;
  }
  if (method === "hook/started" || method === "hook/completed") {
    validateHookNotification(method, params, state);
    return;
  }
  if (method === "account/rateLimits/updated") {
    onlyKeys(params, ["rateLimits"], "Codex rate-limit notification");
    record(params.rateLimits, "Codex rate-limit snapshot");
    return;
  }
  if (method === "mcpServer/startupStatus/updated") {
    validateMcpStartupNotification(params, state.threadId, state.mcpServerNames);
    return;
  }
  if (method === "model/safetyBuffering/updated") {
    validateSafetyBufferingNotification(params, state);
    return;
  }

  const deltaMethods = new Set([
    "item/agentMessage/delta", "item/plan/delta", "item/reasoning/summaryTextDelta",
    "item/reasoning/textDelta", "item/commandExecution/outputDelta",
    "item/fileChange/outputDelta",
  ]);
  if (deltaMethods.has(method)) {
    const keys = ["threadId", "turnId", "itemId", "delta"];
    if (method === "item/reasoning/summaryTextDelta") keys.push("summaryIndex");
    if (method === "item/reasoning/textDelta") keys.push("contentIndex");
    onlyKeys(params, keys, `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    protocolId(params.itemId, `Codex ${method} item id`);
    if (typeof params.delta !== "string") throw new Error(`Codex ${method} delta is invalid`);
    for (const key of ["summaryIndex", "contentIndex"])
      if (key in params && (!Number.isSafeInteger(params[key]) || (params[key] as number) < 0))
        throw new Error(`Codex ${method} index is invalid`);
    return;
  }
  if (method === "item/reasoning/summaryPartAdded") {
    onlyKeys(params, ["threadId", "turnId", "itemId", "summaryIndex"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    protocolId(params.itemId, `Codex ${method} item id`);
    if (!Number.isSafeInteger(params.summaryIndex) || (params.summaryIndex as number) < 0)
      throw new Error("Codex reasoning summary index is invalid");
    return;
  }
  if (method === "item/commandExecution/terminalInteraction") {
    onlyKeys(params, ["threadId", "turnId", "itemId", "processId", "stdin"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    protocolId(params.itemId, `Codex ${method} item id`);
    protocolId(params.processId, `Codex ${method} process id`);
    if (typeof params.stdin !== "string") throw new Error("Codex terminal interaction is invalid");
    return;
  }
  if (method === "item/fileChange/patchUpdated") {
    onlyKeys(params, ["threadId", "turnId", "itemId", "changes"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    protocolId(params.itemId, `Codex ${method} item id`);
    if (!Array.isArray(params.changes)) throw new Error("Codex file patch changes are invalid");
    return;
  }
  if (method === "item/mcpToolCall/progress") {
    onlyKeys(params, ["threadId", "turnId", "itemId", "message"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    protocolId(params.itemId, `Codex ${method} item id`);
    boundedString(params.message, `Codex ${method} message`, 64 * 1024);
    return;
  }
  if (method === "turn/diff/updated") {
    onlyKeys(params, ["threadId", "turnId", "diff"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    if (typeof params.diff !== "string") throw new Error("Codex turn diff is invalid");
    return;
  }
  if (method === "turn/plan/updated") {
    onlyKeys(params, ["threadId", "turnId", "explanation", "plan"], `Codex ${method}`);
    exactRuntimeIds(params, state, `Codex ${method}`);
    if ((params.explanation !== null && typeof params.explanation !== "string")
        || !Array.isArray(params.plan)) throw new Error("Codex turn plan is invalid");
    return;
  }
  throw new Error(`managed Codex emitted unsupported notification ${method}`);
}

/**
 * Valid protocol traffic is not synonymous with execution. Connection,
 * thread-status, rate-limit, MCP-startup, safety-buffering, token-accounting,
 * and hook frames are deliberately excluded from watchdog liveness.
 */
function providerExecutionActivityKind(
  method: string,
  value: unknown,
): string | undefined {
  if (method === "turn/started") return "provider.codex.turn.started";
  if (method === "turn/completed") return "provider.codex.turn.completed";
  if (method === "item/started") return "provider.codex.item.started";
  if (method === "item/completed") return "provider.codex.item.completed";
  if ([
    "item/agentMessage/delta", "item/plan/delta",
    "item/reasoning/summaryTextDelta", "item/reasoning/textDelta",
    "item/commandExecution/outputDelta", "item/fileChange/outputDelta",
  ].includes(method)) {
    const params = record(value, `Codex ${method} activity`);
    return typeof params.delta === "string" && params.delta.length > 0
      ? "provider.codex.item.delta" : undefined;
  }
  if (method === "item/commandExecution/terminalInteraction")
    return "provider.codex.command.interaction";
  if (method === "item/fileChange/patchUpdated") {
    const params = record(value, "Codex file patch activity");
    return Array.isArray(params.changes) && params.changes.length > 0
      ? "provider.codex.file.patch" : undefined;
  }
  if (method === "item/mcpToolCall/progress")
    return "provider.codex.mcp.progress";
  if (method === "turn/diff/updated") {
    const params = record(value, "Codex turn diff activity");
    return typeof params.diff === "string" && params.diff.length > 0
      ? "provider.codex.turn.diff" : undefined;
  }
  if (method === "turn/plan/updated") {
    const params = record(value, "Codex turn plan activity");
    return (typeof params.explanation === "string" && params.explanation.length > 0)
      || (Array.isArray(params.plan) && params.plan.length > 0)
      ? "provider.codex.turn.plan" : undefined;
  }
  return undefined;
}

function usageFromNotification(value: unknown, threadId: string, turnId: string): ManagedCodexResult["usage"] {
  const params = record(value, "Codex token usage notification");
  if (params.threadId !== threadId || params.turnId !== turnId)
    throw new Error("Codex token usage belongs to another turn");
  const tokenUsage = record(params.tokenUsage, "Codex token usage");
  const total = record(tokenUsage.total, "Codex cumulative token usage");
  const counter = (name: string): number => {
    const number = total[name];
    if (typeof number !== "number" || !Number.isSafeInteger(number) || number < 0)
      throw new Error(`Codex token usage ${name} is invalid`);
    return number;
  };
  const result = {
    input_tokens: counter("inputTokens"),
    cached_input_tokens: counter("cachedInputTokens"),
    output_tokens: counter("outputTokens"),
    reasoning_output_tokens: counter("reasoningOutputTokens"),
  };
  if (counter("totalTokens") !== result.input_tokens + result.output_tokens
      || result.cached_input_tokens > result.input_tokens
      || result.reasoning_output_tokens > result.output_tokens)
    throw new Error("Codex cumulative token usage is incoherent");
  return result;
}

interface SupervisorStatusChannel {
  /** Rejects while the start receipt is still being arbitrated. */
  failure: Promise<never>;
  /** Authority preflight is over; the channel keeps carrying diagnostics. */
  settled(): void;
  /** Redacted provider stderr forwarded by the supervisor, oldest first. */
  stderrTail(count?: number): string[];
  /** The supervisor's own EXIT receipt, once it has been observed. */
  exitCode(): number | undefined;
  /** Detach for good. */
  close(): void;
}

/**
 * Read the supervisor's status channel for the WHOLE session, not just until
 * authority preflight. Before this, `stop()` detached the reader the moment
 * initialize returned, so the supervisor's `EXIT n` receipt — the one fact that
 * says how codex died — was thrown away, and forwarded stderr had nowhere to
 * land.
 */
function supervisorStatusChannel(
  child: ChildProcessWithoutNullStreams,
): SupervisorStatusChannel {
  const status = child.stderr as NodeJS.ReadableStream | undefined;
  if (!status) {
    const absent = Promise.reject<never>(new Error("Codex supervisor status pipe is absent"));
    void absent.catch(() => {});
    return {
      failure: absent,
      settled() {}, stderrTail() { return []; }, exitCode() { return undefined; }, close() {},
    };
  }
  const frames = new StrictJsonlFrames({
    label: "Codex supervisor",
    maxLineBytes: SUPERVISOR_STATUS_MAX_LINE_BYTES,
    maxFrames: SUPERVISOR_STATUS_MAX_FRAMES,
    maxTotalBytes: SUPERVISOR_STATUS_MAX_TOTAL_BYTES,
  });
  const ring = new ProviderStderrRing();
  let preflight = true;
  let closed = false;
  let observedExit: number | undefined;
  let malformedNoted = false;
  let rejectFailure!: (error: Error) => void;
  const failure = new Promise<never>((_resolve, reject) => { rejectFailure = reject; });
  void failure.catch(() => {});
  const failPreflight = (error: Error) => {
    if (!preflight) return;
    preflight = false;
    rejectFailure(error);
  };
  const onLine = (line: string) => {
    const statusLine = line.startsWith(CODEX_SUPERVISOR_STATUS_PREFIX)
      ? line.slice(CODEX_SUPERVISOR_STATUS_PREFIX.length)
      : undefined;
    const forwarded = statusLine === undefined ? undefined : codexSupervisorStderrLine(statusLine);
    if (forwarded !== undefined) { ring.add(forwarded); return; }
    if (statusLine === "STARTED") return;
    if (statusLine === "UNAVAILABLE") {
      failPreflight(new Error("Codex executable unavailable"));
      return;
    }
    const exit = statusLine === undefined ? null : /^EXIT (0|[1-9][0-9]{0,2})$/.exec(statusLine);
    const code = exit ? Number(exit[1]) : NaN;
    if (Number.isInteger(code) && code <= 255) {
      observedExit ??= code;
      failPreflight(new Error(`Codex supervisor exited before authority preflight (exit ${code})`));
      return;
    }
    failPreflight(new Error("Codex supervisor emitted invalid start receipt"));
    // Post-preflight there is no one left to reject: record the defect where a
    // post-mortem will read it instead of dropping it silently.
    if (!malformedNoted) {
      malformedNoted = true;
      ring.add("<supervisor emitted an invalid status frame>");
    }
  };
  const onData = (chunk: Buffer) => {
    if (closed) return;
    try {
      for (const line of frames.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk)))
        onLine(line);
    } catch (error) {
      closed = true;
      failPreflight(error as Error);
      ring.add(`<supervisor status channel bound exceeded: ${(error as Error).message}>`);
    }
  };
  const onEnd = () => {
    failPreflight(new Error("Codex supervisor closed before authority preflight"));
  };
  status.on("data", onData);
  status.on("end", onEnd);
  status.on("error", onEnd);
  return {
    failure,
    settled() { preflight = false; },
    stderrTail(count = STDERR_TAIL_LINES) { return ring.tail(count); },
    exitCode() { return observedExit; },
    close() {
      closed = true;
      preflight = false;
      status.removeListener("data", onData);
      status.removeListener("end", onEnd);
      status.removeListener("error", onEnd);
      try { status.resume(); } catch {}
    },
  };
}

async function closeProcess(
  child: ChildProcessWithoutNullStreams,
  rpc?: AppServerRpc,
  control?: SupervisorControl,
): Promise<void> {
  rpc?.markClosing();
  control?.close();
  const closed = new Promise<boolean>((resolveClose) =>
    child.once("close", () => resolveClose(true)));
  try { child.stdin.end(); } catch {}
  // A provider that is ALREADY gone fired `close` before this listener existed,
  // so racing for it burns the whole 3s teardown bound and then reports the
  // corpse as un-reaped. Harmless once per lane; with a respawn budget it is
  // three wasted seconds per dead provider on the recovery path.
  if (child.exitCode !== null || child.signalCode !== null) {
    for (const stream of [child.stdin, child.stdout, child.stderr]) {
      try { stream?.destroy(); } catch {}
    }
    return;
  }
  const settled = await Promise.race([
    closed,
    // Supervisor owns 750ms TERM + 750ms KILL + one 750ms pipe-close
    // deadline. Three 10ms poll quanta cover its bounded predicate checks.
    new Promise<boolean>((resolveExit) => setTimeout(() => resolveExit(false), 2_280)),
  ]);
  let reaped = settled;
  if (!settled) {
    try { child.kill("SIGKILL"); } catch {}
    reaped = await Promise.race([
      closed,
      new Promise<boolean>((resolveExit) => setTimeout(() => resolveExit(false), 750)),
    ]);
  }
  for (const stream of [child.stdin, child.stdout, child.stderr]) {
    try { stream?.destroy(); } catch {}
  }
  if (!reaped) throw new Error("managed Codex supervisor exceeded its teardown bound");
}

function awaitChildSpawn(child: ChildProcessWithoutNullStreams, timeoutMs: number): Promise<void> {
  if (child.pid !== undefined) return Promise.resolve();
  return new Promise((resolveSpawn, reject) => {
    const timer = setTimeout(() => reject(new Error("managed Codex process spawn timed out")), timeoutMs);
    child.once("spawn", () => { clearTimeout(timer); resolveSpawn(); });
    child.once("error", () => { clearTimeout(timer); reject(new Error("managed Codex process unavailable")); });
  });
}

/**
 * A positive-integer millisecond bound from the environment, or the default.
 * A malformed override is not authority to run unbounded — it is ignored.
 */
function boundedMs(name: string, fallback: number, override?: number): number {
  const candidate = override ?? Number(process.env[name]);
  return Number.isSafeInteger(candidate) && (candidate as number) > 0
    ? candidate as number
    : fallback;
}

/**
 * A non-negative respawn budget from the environment, or the default. Zero is a
 * legitimate value — it restores the pre-respawn behavior exactly — so this
 * cannot reuse {@link boundedMs}, whose floor is 1.
 */
function boundedRespawns(override?: number): number {
  const candidate = override ?? Number(process.env.NORTH_CODEX_MAX_RESPAWNS);
  return Number.isSafeInteger(candidate) && (candidate as number) >= 0
    ? candidate as number
    : MAX_RESPAWNS;
}

function clip(value: string, maxBytes: number): string {
  if (Buffer.byteLength(value, "utf8") <= maxBytes) return value;
  const kept = Buffer.from(value, "utf8").subarray(0, maxBytes).toString("utf8");
  return `${kept}\n… (truncated)`;
}

/**
 * The first input frame of a respawned session: the original brief, plus a
 * compact recap of what the crashed session already produced.
 *
 * This is where North's respawn departs from hermes'. hermes retires the
 * session and lets the next user turn rebuild context from its own durable
 * message list (`agent/codex_runtime.py:694-731`); North's managed lane has no
 * transcript outside the provider thread that just died, so the adapter has to
 * carry the context across itself. It is marked as RECOVERED rather than
 * presented as conversation, because the continuation must re-verify the work
 * on disk instead of trusting a claim about a session it never ran.
 */
export function managedCodexRecoveredContext(
  brief: string,
  completedTurnTexts: readonly string[],
  harvest: ManagedCodexHarvest,
  reason: string,
): string {
  const parts: string[] = [
    brief,
    "",
    "=== recovered context from a crashed provider session ===",
    "The provider process running this lane died and North started a new one."
    + " Nothing below was re-executed: it is a record of work YOUR OWN earlier"
    + " turns already performed in this same working tree. Verify it on disk"
    + " before redoing it, then continue the brief above from there.",
    `retired provider thread: ${harvest.threadId ?? "(never started)"}`,
    `retired after: ${clip(reason, 512)}`,
    `completed turns before the crash: ${completedTurnTexts.length}`,
  ];
  completedTurnTexts.forEach((text, index) => {
    if (!text.trim()) return;
    parts.push(`--- your result from recovered turn ${index + 1} ---`,
      clip(text, MAX_RECOVERED_TEXT_BYTES));
  });
  if (harvest.text.trim())
    parts.push("--- partial output of the turn that was interrupted by the crash ---",
      clip(harvest.text, MAX_RECOVERED_TEXT_BYTES));
  const tools: string[] = [];
  const commands = harvest.nativeCommands.totalCommands ?? 0;
  if (commands)
    tools.push(`${commands} native command(s)`
      + ` (${harvest.nativeCommands.successfulCommands ?? 0} succeeded,`
      + ` ${harvest.nativeCommands.failedCommands ?? 0} failed)`);
  if (harvest.mcp.totalCalls)
    tools.push(`${harvest.mcp.totalCalls} MCP tool call(s): `
      + harvest.mcp.tools.map((tool) => `${tool.server}/${tool.tool}×${tool.count}`).join(", "));
  if (harvest.toolItems) tools.push(`${harvest.toolItems} completed work item(s)`);
  parts.push("--- tool work observed before the crash ---",
    tools.length ? tools.join("; ") : "none observed");
  parts.push("=== end recovered context ===");
  return clip(parts.join("\n"), MAX_RECOVERED_CONTEXT_BYTES);
}

/**
 * Silent-but-alive and silent-and-dead are different failures and want
 * different next moves; hermes checks `is_alive()` each loop iteration for the
 * same reason. The host sees the SUPERVISOR when supervised, so an exit receipt
 * observed on the status channel counts as provider death too.
 */
function providerLiveness(
  child: ChildProcessWithoutNullStreams,
  supervisorExit: number | undefined,
): { alive: boolean; exitCode?: number; exitSignal?: string } {
  const exitSignal = child.signalCode ?? undefined;
  const exitCode = supervisorExit ?? (child.exitCode ?? undefined);
  return {
    alive: child.exitCode === null && child.signalCode === null && supervisorExit === undefined,
    ...(exitCode === undefined ? {} : { exitCode }),
    ...(exitSignal === undefined ? {} : { exitSignal: String(exitSignal) }),
  };
}

export class ManagedCodexAppServerRun {
  private child?: ChildProcessWithoutNullStreams;
  private rpc?: AppServerRpc;
  private control?: SupervisorControl;
  private threadStarted = false;
  private readonly mcp = new McpActivityAccumulator("codex-app-server:item-completed");
  private nativeCommands?: NativeCommandActivityAccumulator;
  private readonly respawns: ManagedCodexRespawnAttempt[] = [];
  /** Turns settled across every provider session this lane has used. */
  private laneCompletedTurns = 0;
  /** Set by an attempt that died of PROVIDER DEATH; the respawn gate reads it. */
  private attemptDeath?: { reason: string; diagnostics: ManagedCodexDiagnostics };
  /** The exact failure object that attempt threw — identity, not shape, gates a respawn. */
  private attemptFailure?: Error;
  private interrupted = false;

  constructor(private options: ManagedCodexAppServerOptions) {}

  mcpActivity() { return this.mcp.snapshot(); }
  nativeCommandActivity(): NativeCommandActivityObservation {
    // Per-SESSION, not per-lane: a respawn starts a fresh accumulator because a
    // retired session's commands may still be open, and `complete()` on an open
    // command is a lifecycle defect. The retired session's command evidence goes
    // into the respawn record and the recovered-context recap instead.
    return this.nativeCommands?.snapshot()
      ?? unknownNativeCommandActivity("codex-app-server:not-started");
  }

  /** What this lane's respawns cost it. Readable on success and on failure alike. */
  respawnRecord(): ManagedCodexRespawnRecord {
    return {
      respawnCount: this.respawns.length,
      completedTurns: this.laneCompletedTurns,
      respawns: this.respawns.map((attempt) => ({ ...attempt })),
    };
  }

  /**
   * The provider-death verdict for `failure`, consumed exactly once.
   *
   * Identity, not shape: an attempt records BOTH the death and the exact error
   * object it threw, so a teardown failure raised while this generator is being
   * CLOSED — a different object, on the same dead attempt — can never buy a new
   * provider process.
   */
  private takeAttemptDeath(
    failure: unknown,
  ): { reason: string; diagnostics: ManagedCodexDiagnostics } | undefined {
    const death = this.attemptDeath;
    const matched = death !== undefined && failure === this.attemptFailure;
    this.attemptDeath = undefined;
    this.attemptFailure = undefined;
    return matched ? death : undefined;
  }

  /** Complete the harvest with the tool-activity this run actually observed. */
  private harvest(
    partial: Omit<ManagedCodexHarvest, "mcp" | "nativeCommands" | "landedWork">,
  ): ManagedCodexHarvest {
    const mcp = this.mcp.harvest();
    const nativeCommands = this.nativeCommands?.harvest()
      ?? unknownNativeCommandActivity("codex-app-server:not-started");
    return {
      ...partial,
      mcp,
      nativeCommands,
      // Work landed on a RETIRED session counts too: the lane wrote it, and a
      // harvest that forgot it would report `result=0b` for a lane that
      // committed code — the exact amnesia the harvest exists to prevent.
      landedWork: partial.completedTurns > 0
        || this.laneCompletedTurns > 0
        || this.respawns.length > 0
        || partial.text.trim() !== ""
        || (mcp.totalCalls ?? 0) > 0
        || (nativeCommands.totalCommands ?? 0) > 0,
      ...(this.respawns.length
        ? { respawnCount: this.respawns.length, respawns: this.respawnRecord().respawns }
        : {}),
    };
  }

  async interrupt(): Promise<void> {
    // Also closes the respawn window: a caller that interrupted between two
    // provider sessions must not get a third one started behind its back.
    this.interrupted = true;
    if (this.child) await closeProcess(this.child, this.rpc, this.control);
  }

  // Single-turn entry point: run exactly one turn on a fresh thread and tear the
  // session down. Preserved verbatim for the ~40 direct call sites and the
  // non-continuation worker path.
  async execute(): Promise<ManagedCodexResult> {
    const session = this.session(async () => undefined);
    const first = await session.next();
    if (first.done || !first.value) {
      // This is the arm 94 of 112 deaths took over 2026-07-22..25, and it threw
      // bare — so death.ts's causeChain had nothing to render and every one of
      // them logged as an unexplained `openai_provider_execution_failed`. Name
      // the observed session state so the transport is diagnosable at all: a
      // generator that completed without yielding is a different failure from
      // one that yielded an empty value.
      throw new Error("openai_provider_execution_failed", {
        cause: new Error(
          `codex app-server session produced no result on first turn `
          + `(done=${String(first.done)}, value=${first.value === undefined ? "undefined" : "empty"})`,
        ),
      });
    }
    // Resume into the generator's finally so teardown (and any unclean-close
    // failure) is observed exactly as the pre-continuation flow observed it.
    await session.return(first.value);
    return first.value;
  }

  /**
   * Same-thread continuation across provider RESTARTS. Yields one terminal
   * result per turn; after each turn the caller supplies the next North input
   * frame (or `undefined` to settle).
   *
   * A provider process death after thread/start used to end the lane, because
   * spawn.ts refuses a process-death retry for any authoring-capable lane —
   * correctly, since at that layer nothing knows what the dead turn already
   * wrote to the working tree. The adapter does know, from the harvest, so the
   * respawn lives here: tear the dead supervisor down, re-run the FULL launch
   * preflight (same admission and attestation battery, no shortcut), start a
   * new thread, and continue by re-sending the accumulated context as the next
   * input frame. spawn.ts's retry policy is untouched.
   *
   * Only actual process death respawns. A watchdog that interrupts a wedged
   * turn while the provider is still alive settles that turn and is NOT a
   * respawn trigger.
   */
  async *session(nextInput: ManagedCodexNextInput): AsyncGenerator<ManagedCodexResult> {
    const maxRespawns = boundedRespawns(this.options.maxRespawns);
    // Every completed turn's text, oldest first: the raw material of the recap
    // a respawned session is handed. The provider thread is the only other copy
    // and it dies with the process.
    const completedTurnTexts: string[] = [];
    let launchPrompt = this.options.prompt;
    while (true) {
      try {
        for await (const result of this.attempt(nextInput, launchPrompt)) {
          this.laneCompletedTurns += 1;
          completedTurnTexts.push(result.text);
          yield result;
        }
        return;
      } catch (error) {
        const death = this.takeAttemptDeath(error);
        if (!death || this.interrupted || this.respawns.length >= maxRespawns) throw error;
        const harvest = (error as ManagedCodexHarvestError).harvest;
        this.respawns.push({
          attempt: this.respawns.length + 1,
          reason: death.reason,
          ...(harvest.threadId ? { threadId: harvest.threadId } : {}),
          completedTurns: harvest.completedTurns,
          ...(death.diagnostics.stderrTail.length
            ? { stderrTail: [...death.diagnostics.stderrTail] } : {}),
          ...(death.diagnostics.exitCode === undefined
            ? {} : { exitCode: death.diagnostics.exitCode }),
          ...(death.diagnostics.exitSignal === undefined
            ? {} : { exitSignal: death.diagnostics.exitSignal }),
        });
        launchPrompt = managedCodexRecoveredContext(
          this.options.prompt, completedTurnTexts, harvest, death.reason,
        );
        console.error(
          `[codex] managed provider session died (${death.reason}) — respawning `
          + `${this.respawns.length}/${maxRespawns} with ${completedTurnTexts.length} `
          + `completed turn(s) of recovered context`,
        );
      }
    }
  }

  // One provider session end to end: launch preflight, thread/start, and the
  // per-frame turn loop. Throws on any failure; `session` above owns whether
  // that failure earns another process.
  private async *attempt(
    nextInput: ManagedCodexNextInput,
    launchPrompt: string,
  ): AsyncGenerator<ManagedCodexResult> {
    let contract: LaunchContract;
    try { contract = managedCodexAppServerLaunch(this.options); }
    catch (error) {
      const failure = error instanceof ManagedCodexPreThreadError
        ? error
        : new ManagedCodexPreThreadError("openai_codex_launch_contract_invalid", { cause: error });
      // A RESPAWN's preflight is not a pre-thread preflight. This lane has
      // already started a provider thread and may have written to the working
      // tree, so its failure must stay a harvest — never the retry-safe
      // pre-thread class spawn.ts is allowed to re-run.
      if (!this.threadStarted) throw failure;
      throw new ManagedCodexHarvestError(this.harvest({
        turnIds: [], completedTurns: 0, text: "", unsupportedNotifications: {},
      }), { cause: failure });
    }
    this.nativeCommands = new NativeCommandActivityAccumulator(contract.cwd, ENGINE);
    // The sealed MCP grant for this session: North always, fram only under the
    // graph-authoring capability. Every inventory read, startup notification,
    // and tool-approval request is proven against exactly this.
    const inventory = expectedMcpInventory(this.options.surface);
    const mcpServerNames = Object.freeze(inventory.map((server) => server.name));
    const supervised = this.options.useSupervisor !== false;
    const spawnProcess = this.options.spawnProcess ?? spawn;
    const control = supervised
      ? createSupervisorControl()
      : undefined;
    this.control = control;
    let child: ChildProcessWithoutNullStreams;
    try {
      child = spawnProcess(
        supervised ? process.execPath : contract.executable,
        supervised
          ? [SUPERVISOR, "--duplex", control!.path, CODEX_SUPERVISOR_STDERR_FLAG,
            contract.executable,
            ...(this.options.commandPrefix ?? []), ...contract.args]
          : [...(this.options.commandPrefix ?? []), ...contract.args], {
        cwd: contract.cwd,
        env: this.options.env,
        stdio: ["pipe", "pipe", "pipe"],
        detached: false,
      }) as unknown as ChildProcessWithoutNullStreams;
    } catch (cause) {
      control?.close();
      this.control = undefined;
      throw new ManagedCodexPreThreadError("openai_codex_supervisor_unavailable", { cause });
    }
    this.child = child;
    let remoteDisabled = false;
    let threadId: string | undefined;
    let turnId: string | undefined;
    let completedTurns = 0;
    const settledTurnIds: string[] = [];
    let runtimeState: RuntimeNotificationState | undefined;
    const approvedServerRequests = new Set<RpcId>();
    const queuedNotifications: Array<{ method: string; value: unknown }> = [];
    let terminalResolve!: () => void;
    let terminalReject!: (error: Error) => void;
    // Reassigned per turn: each continuation turn installs a fresh terminal
    // barrier while `terminalResolve`/`terminalReject` (captured by the
    // notification closures below by reference) always point at the live turn.
    let terminal = new Promise<void>((resolveTerminal, rejectTerminal) => {
      terminalResolve = resolveTerminal;
      terminalReject = rejectTerminal;
    });
    // Authority preflight can fail before the turn waiter is reached. Attach a
    // handler immediately so that the same error is observed by the main flow,
    // never as a detached unhandled rejection.
    void terminal.catch(() => {});
    // Turn-level watchdogs (hermes' run_turn loop, adapted). Two bounds, both
    // env-overridable, both armed per turn: an overall deadline, and a quiet
    // timer armed by a completed tool item and cleared by any other projected
    // activity. Neither existed before: `await terminal` was unbounded, and
    // RPC_TIMEOUT_MS only ever covered an outstanding request.
    const turnDeadlineMs = boundedMs(
      "NORTH_CODEX_TURN_DEADLINE_MS", TURN_DEADLINE_MS, this.options.turnDeadlineMs,
    );
    const postToolQuietMs = boundedMs(
      "NORTH_CODEX_POST_TOOL_QUIET_MS", POST_TOOL_QUIET_MS, this.options.postToolQuietMs,
    );
    let deadlineTimer: ReturnType<typeof setTimeout> | undefined;
    let quietTimer: ReturnType<typeof setTimeout> | undefined;
    let watchdogReason: Error | undefined;
    const clearQuietWatchdog = () => {
      if (quietTimer) clearTimeout(quietTimer);
      quietTimer = undefined;
    };
    const clearWatchdogs = () => {
      clearQuietWatchdog();
      if (deadlineTimer) clearTimeout(deadlineTimer);
      deadlineTimer = undefined;
    };
    const armQuietWatchdog = () => {
      clearQuietWatchdog();
      if (watchdogReason || !runtimeState?.turnId || runtimeState.terminalSeen) return;
      quietTimer = setTimeout(
        () => expireTurn(`codex went silent for ${postToolQuietMs}ms after a completed tool item`),
        postToolQuietMs,
      );
      quietTimer.unref?.();
    };
    const armTurnDeadline = () => {
      if (deadlineTimer) clearTimeout(deadlineTimer);
      deadlineTimer = setTimeout(
        () => expireTurn(`codex turn exceeded its ${turnDeadlineMs}ms deadline`),
        turnDeadlineMs,
      );
      deadlineTimer.unref?.();
    };
    // Best-effort and bounded: a provider wedged enough to trip a watchdog may
    // also ignore the interrupt, and settling this turn must not wait on that.
    const interruptTurn = async (): Promise<string> => {
      if (!threadId || !turnId) return "no live turn to interrupt";
      try {
        record(await Promise.race([
          rpc.request("turn/interrupt", { threadId, turnId }),
          new Promise((_resolve, reject) => {
            const timer = setTimeout(
              () => reject(new Error("turn/interrupt timed out")), TURN_INTERRUPT_MS,
            );
            timer.unref?.();
          }),
        ]), "Codex turn/interrupt response");
        return "turn/interrupt accepted";
      } catch (error) {
        return `turn/interrupt refused: ${error instanceof Error ? error.message : String(error)}`;
      }
    };
    const expireTurn = (bound: string) => {
      if (watchdogReason) return;
      clearWatchdogs();
      // Silent-but-alive and silent-and-dead are different failures; say which.
      const liveness = providerLiveness(child, supervised ? supervisor.exitCode() : undefined);
      const cause = new Error(
        `${bound}; provider ${liveness.alive ? "still running" : "not running"}`
        + `${liveness.exitCode === undefined ? "" : ` (exit ${liveness.exitCode})`}`
        + `${liveness.exitSignal === undefined ? "" : ` (signal ${liveness.exitSignal})`}`,
      );
      // Set synchronously: from here on THIS is the reason the turn ended, even
      // if interrupting it trips an RPC-level failure of its own.
      watchdogReason = new Error("openai_codex_turn_interrupted", { cause });
      const settle = terminalReject;
      void (async () => {
        const outcome = liveness.alive ? await interruptTurn() : "provider already gone";
        cause.message = `${cause.message}; ${outcome}`;
        // A watchdog settles the TURN. The process is never killed from here —
        // ordinary graceful teardown owns that.
        settle(watchdogReason!);
      })();
    };
    let projectWarningSeen = false;
    const validateConnectionNotification = (method: string, value: unknown): boolean => {
      if (method === "configWarning") {
        // The prose changes across Codex versions. Its two path identities are
        // stable and credential-free; structured config-layer validation below
        // remains the authority proof.
        validateProjectConfigWarning(value, contract);
        projectWarningSeen = true;
        return true;
      }
      if (method === "deprecationNotice") {
        const params = record(value, "Codex deprecation notice");
        onlyKeys(params, ["summary", "details"], "Codex deprecation notice");
        boundedString(params.summary, "Codex deprecation summary", 2_048);
        boundedString(params.details, "Codex deprecation details", 4_096);
        return true;
      }
      if (method === "remoteControl/status/changed") {
        const params = record(value, "Codex remote-control status");
        onlyKeys(params, ["status", "serverName", "installationId", "environmentId"],
          "Codex remote-control status");
        if (remoteDisabled || params.status !== "disabled"
            || typeof params.serverName !== "string" || !params.serverName
            || Buffer.byteLength(params.serverName, "utf8") > 256
            || typeof params.installationId !== "string" || !params.installationId
            || Buffer.byteLength(params.installationId, "utf8") > 256
            || (params.environmentId !== null
              && (typeof params.environmentId !== "string" || !params.environmentId
                || Buffer.byteLength(params.environmentId, "utf8") > 256)))
          throw new Error("Codex remote control is not exactly disabled");
        remoteDisabled = true;
        return true;
      }
      if (method === "mcpServer/startupStatus/updated") {
        const pendingThreadStart = threadId === undefined && this.threadStarted;
        const params = validateMcpStartupNotification(
          value, threadId, mcpServerNames, pendingThreadStart,
        );
        // Codex may emit a thread-scoped startup transition before the
        // thread/start response that establishes the exact local thread id.
        // Preserve it until that signed response arrives, then validate the
        // queued id against runtimeState through the normal strict path.
        if (params.threadId !== null && threadId === undefined) return false;
        return true;
      }
      if (method === "account/rateLimits/updated") {
        const params = record(value, "Codex rate-limit notification");
        onlyKeys(params, ["rateLimits"], "Codex rate-limit notification");
        record(params.rateLimits, "Codex rate-limit snapshot");
        return true;
      }
      if (method === "serverRequest/resolved") {
        const params = record(value, "Codex server request resolution");
        onlyKeys(params, ["threadId", "requestId"], "Codex server request resolution");
        const requestId = params.requestId;
        if (params.threadId !== threadId
            || (typeof requestId !== "number" && typeof requestId !== "string")
            || !approvedServerRequests.delete(requestId))
          throw new Error("Codex resolved an unknown server request");
        return true;
      }
      return false;
    };
    const canProcessWithoutTurn = (entry: { method: string; value: unknown }): boolean => {
      if (entry.method === "thread/started" || entry.method === "thread/status/changed"
          || entry.method === "mcpServer/startupStatus/updated") return true;
      if (entry.method === "hook/started" || entry.method === "hook/completed") {
        try {
          const params = record(entry.value, "Codex hook notification");
          const run = record(params.run, "Codex hook run");
          return run.eventName === "sessionStart";
        }
        catch { return true; }
      }
      return false;
    };
    const processRuntime = (entry: { method: string; value: unknown }): void => {
      if (!runtimeState) throw new Error("Codex runtime notification preceded thread authority");
      const wasTerminal = runtimeState.terminalSeen;
      const toolItemsBefore = runtimeState.toolItems;
      validateProgressNotification(entry.method, entry.value, runtimeState);
      const activity = providerExecutionActivityKind(entry.method, entry.value);
      if (activity) this.options.onActivity?.(activity);
      // Post-tool quiet watchdog, hermes' arm/clear rule: a completed tool item
      // arms it, ANY other projected activity means codex is still producing
      // and clears it.
      if (runtimeState.toolItems > toolItemsBefore) armQuietWatchdog();
      else clearQuietWatchdog();
      if (!wasTerminal && runtimeState.terminalSeen) terminalResolve();
    };
    const drainQueued = (withTurn: boolean): void => {
      for (let index = 0; index < queuedNotifications.length;) {
        const entry = queuedNotifications[index]!;
        if (!withTurn && !canProcessWithoutTurn(entry)) { index += 1; continue; }
        queuedNotifications.splice(index, 1);
        processRuntime(entry);
      }
    };
    const onNotification = (method: string, value: unknown) => {
      if (validateConnectionNotification(method, value)) return;
      const entry = { method, value };
      if (!runtimeState || (!runtimeState.turnId && !canProcessWithoutTurn(entry))) {
        if (queuedNotifications.length >= MAX_QUEUED_NOTIFICATIONS)
          throw new Error("Codex queued too many pre-authority notifications");
        queuedNotifications.push(entry);
        return;
      }
      processRuntime(entry);
    };
    const onServerRequest: AppServerRequestHandler = (id, method, value) => {
      if (method !== "item/tool/requestUserInput") return undefined;
      const params = record(value, "Codex tool input request");
      onlyKeys(params, ["threadId", "turnId", "itemId", "questions", "autoResolutionMs"],
        "Codex tool input request");
      if (params.threadId !== threadId || params.turnId !== turnId || params.autoResolutionMs !== null)
        throw new Error("Codex tool input request belongs to another execution");
      const itemId = protocolId(params.itemId, "Codex tool input item id");
      if (!Array.isArray(params.questions) || params.questions.length !== 1)
        throw new Error("Codex tool input request must contain one approval question");
      const question = record(params.questions[0], "Codex managed MCP approval question");
      onlyKeys(question, ["id", "header", "question", "isOther", "isSecret", "options"],
        "Codex managed MCP approval question");
      const questionId = `mcp_tool_call_approval_${itemId}`;
      const prompt = boundedString(question.question, "Codex managed MCP approval prompt", 512);
      // fram's graph-edit verbs are hyphenated (`set-body`), North's are
      // underscored (`evidence_record`); the character class covers both and the
      // grant itself is proven by exact membership in that server's tool list.
      const match = /^Allow the ([a-z][a-z0-9-]*) MCP server to run tool "([a-z][a-z0-9_-]*)"\?$/
        .exec(prompt);
      const granted = match
        ? inventory.find((server) => server.name === match[1])?.tools
        : undefined;
      if (question.id !== questionId || question.header !== "Approve app tool call?"
          || question.isOther !== false || question.isSecret !== false || !match
          || !granted?.includes(match[2]!))
        throw new Error("Codex requested approval outside North's sealed MCP grant");
      exact(question.options, [
        { label: "Allow", description: "Run the tool and continue." },
        { label: "Allow for this session", description: "Run the tool and remember this choice for this session." },
        { label: "Cancel", description: "Cancel this tool call." },
      ], "Codex managed MCP approval options");
      this.options.onActivity?.("provider.codex.mcp.request");
      approvedServerRequests.add(id);
      return { answers: { [questionId]: { answers: ["Allow"] } } };
    };
    const rpc = new AppServerRpc(
      child, this.options.timeoutMs ?? RPC_TIMEOUT_MS, onNotification, onServerRequest, control?.writeLine,
      !supervised,
    );
    this.rpc = rpc;
    // Route an RPC-level terminal failure into whichever turn is currently
    // waiting; the indirection is required so a defect during turn N rejects
    // turn N, not the already-settled turn 1 barrier.
    const removeTerminal = rpc.onTerminal((error) => terminalReject(error));
    const supervisor: SupervisorStatusChannel = supervised
      ? supervisorStatusChannel(child)
      : {
        failure: new Promise<never>(() => {}),
        settled() {}, stderrTail() { return []; }, exitCode() { return undefined; }, close() {},
      };
    // Whichever pipe this launch owns, the diagnostics read the same way.
    const providerDiagnostics = (): ManagedCodexDiagnostics => {
      const liveness = providerLiveness(child, supervised ? supervisor.exitCode() : undefined);
      return {
        stderrTail: supervised ? supervisor.stderrTail() : rpc.stderrTail(),
        ...(liveness.exitCode === undefined ? {} : { exitCode: liveness.exitCode }),
        ...(liveness.exitSignal === undefined ? {} : { exitSignal: liveness.exitSignal }),
        providerAlive: liveness.alive,
      };
    };
    let failure: ManagedCodexHarvestError | ManagedCodexPreThreadError | undefined;
    let primaryFailed = false;
    let protocolSucceeded = false;
    try {
      await awaitChildSpawn(child, this.options.timeoutMs ?? RPC_TIMEOUT_MS);
      if (control) await Promise.race([control.connected, supervisor.failure]);
      const initialized = await Promise.race([
        rpc.request("initialize", {
          clientInfo: { name: "north", title: "North", version: "1" },
          capabilities: { experimentalApi: true },
        }),
        supervisor.failure,
      ]);
      // Preflight arbitration is over; the channel keeps carrying diagnostics
      // and the EXIT receipt for the rest of the session.
      supervisor.settled();
      validateInitialize(initialized, contract);
      rpc.notify("initialized", {});
      validateAccount(await rpc.request("account/read", {}));
      const config = await rpc.request("config/read", { includeLayers: true, cwd: contract.cwd });
      const fingerprint = validateConfig(config, contract, projectWarningSeen);
      validateRequirements(await rpc.request("configRequirements/read"), contract);
      validateHooks(await rpc.request("hooks/list", { cwds: [contract.cwd] }), contract.cwd);
      await validateMcp(rpc, inventory);
      if (!remoteDisabled) throw new Error("Codex did not prove remote control disabled");
      assertNoFilesystemAuthority(contract.codexHome);
      const shellPolicy = record(
        contract.expectedSessionConfig.shell_environment_policy,
        "Codex managed shell policy",
      );
      const shellEnvironment = record(shellPolicy.set, "Codex managed shell environment");
      validateShellPreflight(await rpc.request("command/exec", {
        command: [...CODEX_SHELL_PREFLIGHT_COMMAND],
        processId: null,
        tty: false,
        streamStdin: false,
        streamStdoutStderr: false,
        outputBytesCap: CODEX_SHELL_PREFLIGHT_OUTPUT_BYTES,
        disableOutputCap: false,
        disableTimeout: false,
        timeoutMs: CODEX_SHELL_PREFLIGHT_TIMEOUT_MS,
        cwd: contract.cwd,
        env: { PATH: shellEnvironment.PATH, NORTH_BIN: shellEnvironment.NORTH_BIN },
        size: null,
        sandboxPolicy: { type: "readOnly", networkAccess: false },
        permissionProfile: null,
      }));

      // thread/start may execute SessionStart hooks. From this dispatch onward,
      // every failure is terminal and must never be presented as fallback-safe.
      this.threadStarted = true;
      const started = record(await rpc.request("thread/start", {
        model: this.options.model,
        modelProvider: "openai",
        approvalPolicy: "never",
        approvalsReviewer: "user",
        sandbox: this.options.surface.sandbox,
        config: this.options.effort ? { model_reasoning_effort: this.options.effort } : {},
        developerInstructions: this.options.developerInstructions,
        ephemeral: true,
      }), "Codex thread/start response");
      threadId = validateStartedThread(started, contract, this.options);
      runtimeState = {
        threadId,
        cwd: contract.cwd,
        model: this.options.model,
        hookRuns: new Set(),
        text: "",
        terminalSeen: false,
        toolItems: 0,
        mcpActivity: this.mcp,
        nativeCommands: this.nativeCommands,
        mcpServerNames,
      };
      drainQueued(false);
      if (runtimeState.hookRuns.size || queuedNotifications.length)
        throw new Error("Codex thread/start left unresolved lifecycle notifications");

      // One turn per North input frame, on the same provider thread. The first
      // frame is the launch prompt — the brief on the first attempt, the brief
      // plus recovered context on a respawn; later frames arrive from
      // `nextInput`.
      let input: string | undefined = launchPrompt;
      while (true) {
        // Re-prove the exact authority surface before every turn: a stale or
        // widened config, hook set, or MCP tool grant fails the turn closed
        // rather than executing a continuation under changed capability.
        const repeated = await rpc.request("config/read", { includeLayers: true, cwd: contract.cwd });
        if (validateConfig(repeated, contract, projectWarningSeen) !== fingerprint)
          throw new Error("Codex config authority changed after thread/start");
        validateHooks(await rpc.request("hooks/list", { cwds: [contract.cwd] }), contract.cwd);
        await validateMcp(rpc, inventory, threadId);
        assertNoFilesystemAuthority(contract.codexHome);

        // Fresh terminal barrier and per-turn runtime accumulators. The closures
        // read `terminalResolve`/`runtimeState` by reference, so reassigning here
        // steers every subsequent notification at this turn.
        runtimeState.text = "";
        runtimeState.usage = undefined;
        runtimeState.turnId = undefined;
        runtimeState.terminalSeen = false;
        runtimeState.toolItems = 0;
        terminal = new Promise<void>((resolveTerminal, rejectTerminal) => {
          terminalResolve = resolveTerminal;
          terminalReject = rejectTerminal;
        });
        void terminal.catch(() => {});
        protocolSucceeded = false;

        const turnStart = record(await rpc.request("turn/start", {
          threadId,
          input: [{ type: "text", text: input }],
          ...(this.options.effort ? { effort: this.options.effort } : {}),
        }), "Codex turn/start response");
        turnId = validateStartedTurn(turnStart);
        runtimeState.turnId = turnId;
        armTurnDeadline();
        drainQueued(true);
        try { await terminal; }
        catch (error) {
          // Once a watchdog has fired, ITS reason is the reason: an RPC-level
          // failure observed while interrupting is downstream of it.
          throw watchdogReason ?? error;
        }
        clearWatchdogs();
        if (!runtimeState.terminalSeen || !runtimeState.usage || runtimeState.hookRuns.size
            || queuedNotifications.length)
          throw new Error("Codex closed without exact terminal usage and lifecycle");
        // NARROWED TERMINAL (2026-07-26): the checks above are RESULT integrity
        // — without them the yielded value would be a fiction, so they stay
        // fatal before the yield. The settlement re-proof below is different: it
        // describes whether ANOTHER turn may run, and it happens after this turn
        // has already completed under authority proven at its start. Discarding a
        // finished turn's work over it orphaned real code (defect B). Observe it,
        // deliver the turn, and refuse continuation instead.
        let settlementDefect: Error | undefined;
        try {
          const terminalConfig = await rpc.request("config/read", {
            includeLayers: true, cwd: contract.cwd,
          });
          if (validateConfig(terminalConfig, contract, projectWarningSeen) !== fingerprint)
            throw new Error("Codex config authority changed at terminal settlement");
          rpc.assertHealthy();
        } catch (error) {
          settlementDefect = error instanceof Error ? error : new Error(String(error));
          console.error(
            `[codex] managed thread settlement defect after a completed turn: `
            + `${settlementDefect.message} — delivering the turn, refusing continuation`,
          );
        }
        protocolSucceeded = true;
        this.mcp.complete();
        if (!this.nativeCommands.complete())
          throw new Error("Codex turn completed with unfinished native commands");
        yield {
          text: runtimeState.text,
          usage: runtimeState.usage,
          toolItems: runtimeState.toolItems,
          providerJoin: providerJoinEvidence("openai", {
            sessionId: threadId,
            turnIds: [turnId],
            // thread/start is admitted only with ephemeral:true above. This is
            // positive non-persistence evidence, not an inference from a
            // missing account-log record.
            sessionPersistence: "ephemeral",
          }),
        };

        completedTurns += 1;
        settledTurnIds.push(turnId);

        input = await nextInput();
        if (input === undefined) break;
        if (settlementDefect)
          throw new Error("Codex refused continuation after a terminal settlement defect", {
            cause: settlementDefect,
          });
        this.mcp.reopen();
        this.nativeCommands.reopen();
      }
    } catch (error) {
      primaryFailed = true;
      failure = this.threadStarted
        ? new ManagedCodexHarvestError(this.harvest({
          threadId,
          turnIds: turnId && !settledTurnIds.includes(turnId)
            ? [...settledTurnIds, turnId]
            : [...settledTurnIds],
          completedTurns,
          text: runtimeState?.text ?? "",
          ...(runtimeState ? { toolItems: runtimeState.toolItems } : {}),
          usage: runtimeState?.usage,
          unsupportedNotifications: rpc.unsupportedNotifications(),
        }), { cause: error })
        : new ManagedCodexPreThreadError("openai_codex_authority_preflight_failed", { cause: error });
      // Liveness has to be sampled HERE — teardown below is about to change it.
      const diagnostics = providerDiagnostics();
      attachDiagnostics(failure, diagnostics);
      // Respawn eligibility, decided at the one moment the evidence is still
      // true. A watchdog reason means the TURN was interrupted, not that the
      // process died — settling it is the whole point of that path — so it is
      // excluded even though the provider is torn down moments later.
      if (failure instanceof ManagedCodexHarvestError && !watchdogReason
          && (rpc.diedFromProcessDeath() || diagnostics.providerAlive === false)) {
        this.attemptDeath = {
          reason: error instanceof Error ? error.message : String(error),
          diagnostics,
        };
        this.attemptFailure = failure;
      }
      throw failure;
    } finally {
      clearWatchdogs();
      supervisor.settled();
      removeTerminal();
      try {
        await closeProcess(child, rpc, control);
        if (protocolSucceeded) rpc.assertHealthy();
      } catch (error) {
        if (!primaryFailed)
          throw new Error("openai_provider_execution_failed", { cause: error });
      }
      // The supervisor's EXIT receipt and codex's last words only arrive as the
      // process closes, which is AFTER the throw above built the error. The
      // error object is the same reference, so top it up here rather than
      // reporting the exit-less snapshot the catch could see.
      if (failure) {
        // Liveness stays the FAILURE-time observation; everything else is topped up.
        const alive = failure.diagnostics?.providerAlive;
        attachDiagnostics(failure, {
          ...providerDiagnostics(),
          ...(alive === undefined ? {} : { providerAlive: alive }),
        });
        // The exit receipt is the single most useful field in a respawn record
        // and it only exists after this close, so re-point at the topped-up one.
        if (this.attemptDeath && failure.diagnostics)
          this.attemptDeath.diagnostics = failure.diagnostics;
      }
      supervisor.close();
      this.child = undefined;
      this.rpc = undefined;
      this.control = undefined;
    }
  }
}
