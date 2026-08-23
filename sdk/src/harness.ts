// The provider-neutral harness contract. One place builds the query Options that
// both the Claude SDK and Codex adapter consume, so coordination tools, Orchestration authority,
// topology enforcement, reasoning, model calibration, and system instructions
// stay identical across dispatch.ts and spawn.ts.
//
// The two things that make a North-orchestrated agent more than a generic run:
//   1. north MCP — native coordination-graph verbs (capture/tell/ready/next/...),
//      so agents act on facts, not by Edit-ing text files.
//   2. explicit orchestrator topology — and only that topology — may dispatch or
//      command peers through North. Workers remain without coordination authority.
import { createSdkMcpServer, tool } from "@anthropic-ai/claude-agent-sdk";
import type { Options } from "@anthropic-ai/claude-agent-sdk";
import { createHash } from "node:crypto";
import { z } from "zod";
import { execFileSync } from "node:child_process";
import {
  accessSync, constants, existsSync, readFileSync, readdirSync, realpathSync, statSync,
} from "node:fs";
import { delimiter, dirname, relative, resolve, sep } from "node:path";
import {
  evaluateGuards, resolveManagedGuardChain,
} from "./authoring-guards";
import { recordDenial } from "./guard-log";
import {
  observeProviderContextWindow, resolveModelAlias, resolveModelDelta, resolveTier,
} from "./providers/catalog";
import type { ProviderId } from "./providers/types";
import {
  type RoutingDraft, type RoutingOverrideField, type RoutingRequest, type Topology,
} from "./routing-metadata";
import { admitRoutingRequest } from "./routing-admission";
import { orchestrationCapabilities } from "./orchestration-staffing";
import {
  hasAuthoringCapability, type OrchestrationCapability,
} from "./orchestration-capabilities";
import {
  BESPOKE_FINGERPRINT_DOMAIN, BESPOKE_FINGERPRINT_VERSION,
  bespokeContractFingerprint, canonicalOrchestrationCapabilities,
} from "./bespoke-contract";
import { assertCoordinationAuthority } from "./topology-authority";
import {
  MAX_READONLY_COMMAND_BYTES, READONLY_SHELL_SERVER, READONLY_SHELL_TOOL, runReadonlyShell,
} from "./readonly-shell";
import { managedNorthMcpEnvironment } from "./execution-admission";
import { managedCodexNetworkPolicy } from "./providers/codex-network-policy";
import { requireJudgmentGrade } from "./judgment-grade";
import {
  providerModelObservationPath,
  type ProviderModelAdmissionReceipt,
} from "./provider-model-observation-store";
import {
  buildEnvironmentReceipt, buildPromptReceipt, sha256Bytes,
  type EnvironmentArtifact, type EnvironmentReceipt, type PromptReceipt,
} from "./composition-receipt";
import {
  beagleStoreBabashkaArguments,
  beagleStoreCoordinatorChildTimeout,
  beagleStoreEnvironment,
} from "./beagle-store";
import {
  loadPresenceFence, parsePresenceFence, persistPresenceFence, presenceFenceJson,
} from "./presence-fence";

// sdk/src/harness.ts -> its relocatable runtime root.
const REPO = resolve(import.meta.dir, "../..");
const ENGINE = `${REPO}/bin/north`;
const MCP = `${REPO}/bin/north-mcp`;
const MSG_CLI = `${REPO}/cli/msg-cli.clj`;

// Prompt-EMITTED paths (unlike this process's own ENGINE/MCP) must survive a
// rebuild that outlives the session, so they resolve store REPOs to a stable alias.
const isStoreRepo = (repo: string): boolean => repo.startsWith("/nix/store/");
// Composed, never written literally: the package purity scan rejects the system
// profile path in a packaged file, and this is the root-managed alias, not a store pin.
const STABLE_SYSTEM_BIN = ["", "run", "current-system", "sw", "bin"].join("/");
// Verified present in the system profile; others fall back to a bare name.
const STABLE_SYSTEM_BINARIES = new Set(["north", "concern"]);

function stableBinPath(name: string, repo: string = REPO): string {
  if (!isStoreRepo(repo)) return `${repo}/bin/${name}`;
  return STABLE_SYSTEM_BINARIES.has(name) ? `${STABLE_SYSTEM_BIN}/${name}` : name;
}

// No stable system alias exists for docs; prefer the live checkout over the store copy.
function esoSpecPath(env: NodeJS.ProcessEnv = process.env, repo: string = REPO): string {
  const relative = "sdk/src/vendor/eso/SPEC.md";
  if (!isStoreRepo(repo)) return `${repo}/${relative}`;
  const checkout = env.NORTH_HOME;
  if (checkout && !isStoreRepo(checkout)) {
    const candidate = resolve(checkout, relative);
    if (existsSync(candidate)) return candidate;
  }
  return `${repo}/${relative}`;
}
const northPort = () => process.env.NORTH_PORT ?? "7977";
const peerBb = () => process.env.NORTH_PEER_BB ?? "bb";

function currentPathExecutable(name: string, env: NodeJS.ProcessEnv = process.env): string {
  if (name.includes("/")) return name;
  for (const directory of (env.PATH ?? "").split(delimiter)) {
    if (!directory) continue;
    const candidate = resolve(directory, name);
    try {
      accessSync(candidate, constants.X_OK);
      return candidate;
    } catch { /* keep searching the current PATH */ }
  }
  return name;
}

const presenceBb = () => currentPathExecutable(process.env.NORTH_PEER_BB ?? "bb");

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

// Minimal EDN for a flat args map (the envelope contract's :args are flat):
// keywordize keys; @refs and :keywords pass bare; everything else is a quoted
// string (EDN strings are JSON-compatible); numbers/bools bare.
function ednArgs(args: Record<string, unknown>): string {
  const val = (v: unknown): string => {
    if (typeof v === "number" || typeof v === "boolean") return String(v);
    const s = typeof v === "object" && v !== null ? JSON.stringify(v) : String(v);
    return /^[@:]/.test(s) ? s : JSON.stringify(s);
  };
  return `{${Object.entries(args).map(([k, v]) => `:${k} ${val(v)}`).join(" ")}}`;
}

const PEER_ROUTING_FIELDS = [
  "role", "taskGrade", "domainRequirements", "topology", "tier", "reasoning", "posture", "composition",
] as const;
const PEER_ROUTE_ADAPTER_FIELDS = ["provider", "target", "model"] as const;
type PeerOperation = "spawn" | "dispatch" | "tell" | "acquire";

function exactPeerFields(args: Record<string, unknown>, allowed: readonly string[], operation: string): void {
  const unknown = Object.keys(args).filter((field) => !allowed.includes(field));
  if (unknown.length) throw new Error(`${operation} has unknown field(s): ${unknown.join(", ")}`);
}

/** Validate the fact-envelope before msg-cli can publish its routing key. */
export function validatePeerCommandArgs(op: PeerOperation, args: Record<string, unknown>): void {
  if (args == null || typeof args !== "object" || Array.isArray(args))
    throw new Error(`${op} args must be an object`);
  const nonEmpty = (field: string) => typeof args[field] === "string" && Boolean((args[field] as string).trim());
  if (op === "tell") {
    exactPeerFields(args, ["id", "pred", "value"], op);
    if (!["id", "pred", "value"].every(nonEmpty)) throw new Error("tell requires id, pred, and value");
    if (args.pred === "judgment_grade") requireJudgmentGrade(args.value);
    return;
  }
  if (op === "acquire") {
    exactPeerFields(args, ["resource", "holder"], op);
    if (!nonEmpty("resource")) throw new Error("acquire requires resource");
    return;
  }
  const workField = op === "spawn" ? "prompt" : "thread";
  exactPeerFields(args, [workField, ...PEER_ROUTING_FIELDS, ...PEER_ROUTE_ADAPTER_FIELDS], op);
  if (!nonEmpty(workField) || !nonEmpty("role"))
    throw new Error(`${op} requires ${workField} and an explicit Orchestration role`);
  const presentRouting = PEER_ROUTING_FIELDS.filter((field) => Object.hasOwn(args, field));
  if (presentRouting.length !== PEER_ROUTING_FIELDS.length) {
    const missing = PEER_ROUTING_FIELDS.filter((field) => !Object.hasOwn(args, field));
    throw new Error(
      `${op} requires the complete eight-field Orchestration request; missing: ${missing.join(", ")}`
      + " (recover the valid payload shape: north show @contract:dispatch)",
    );
  }
  const metadata = Object.fromEntries(
    PEER_ROUTING_FIELDS.filter((field) => Object.hasOwn(args, field)).map((field) => [field, args[field]]),
  ) as RoutingDraft;
  admitRoutingRequest(metadata, `managed peer ${op}`);
}

export function sendPeerCommand(
  self: string,
  to: string,
  op: PeerOperation,
  args: Record<string, unknown>,
): string {
  assertCoordinationAuthority(`command_peer:${op}`);
  if (op === "spawn" || op === "dispatch") {
    throw new Error(
      `peer ${op} is unsupported until atomic command claim + child reconciliation land; use North MCP/CLI ${op}`,
    );
  }
  validatePeerCommandArgs(op, args);
  const commandArgs = { ...args };
  return execFileSync(peerBb(), beagleStoreBabashkaArguments([
    MSG_CLI, northPort(), "send-cmd", self, to, op, ednArgs(commandArgs),
  ]), {
    encoding: "utf8",
    env: beagleStoreEnvironment(),
    timeout: beagleStoreCoordinatorChildTimeout(),
  });
}

// Repeat-safe peer fact operations. Managed spawn/dispatch stay on North's
// canonical MCP/CLI surfaces until command claims + child reconciliation exist.
export function peerCommandServer(self: string) {
  return createSdkMcpServer({
    name: "north-peer",
    version: "0.1.0",
    tools: [
      tool(
        "command_peer",
        "Command a peer over the North fact feed with repeat-safe operations: " +
          "tell {id, pred, value} | acquire {resource}. Managed spawn/dispatch " +
          "use North's canonical MCP/CLI tools.",
        {
          to: z
            .string()
            .describe("exact recipient agent handle or held role; use literal '*' to broadcast"),
          op: z.enum(["tell", "acquire"]),
          args: z
            .record(z.string(), z.any())
            .describe("op-specific repeat-safe fact arguments"),
        },
        async ({ to, op, args }) => {
          try {
            const out = sendPeerCommand(self, to, op, args);
            return { content: [{ type: "text", text: `sent {:op :${op}} -> ${to}\n${out}`.trim() }] };
          } catch (e: any) {
            return {
              content: [{ type: "text", text: `command_peer failed: ${e?.stderr ?? e?.message ?? e}` }],
              isError: true,
            };
          }
        }
      ),
    ],
  });
}

// Coordination tools are universal; orchestration tools are positive authority.
export const COORDINATION_TOOLS = [
  "mcp__north__capture",
  "mcp__north__tell",
  "mcp__north__evidence_record",
  "mcp__north__show",
  "mcp__north__search",
  "mcp__north__artifact_read",
  "mcp__north__ready",
  "mcp__north__next",
  "mcp__north__threads",
];
export const ORCHESTRATION_TOOLS = [
  "mcp__north__dispatch",
  "mcp__north__spawn",
  "mcp__north-peer__command_peer",
];
export const NATIVE_AGENT_TOOLS = ["Agent", "Task", "Workflow"];
export const NORTH_MCP_TOOL_NAMES = [
  "ready",
  "next",
  "threads",
  "blocked",
  "agenda",
  "leverage",
  "needs_review",
  "validate",
  "show",
  "search",
  "artifact_read",
  "capture",
  "tell",
  "evidence_record",
  "retract",
  "presentation",
  "linear_get",
  "linear_import",
  "linear_plan",
  "linear_sync",
  "dispatch",
  "spawn",
] as const;
const ALL_NORTH_MCP_TOOLS = NORTH_MCP_TOOL_NAMES.map((name) => `mcp__north__${name}`);
const CAPABILITY_TOOLS: Record<OrchestrationCapability, string[]> = {
  "filesystem.read": ["Read"],
  "filesystem.search": ["Grep", "Glob"],
  "filesystem.write": ["Edit", "Write", "NotebookEdit"],
  shell: ["Bash"],
  "shell.readonly": [READONLY_SHELL_TOOL],
  web: ["WebSearch", "WebFetch"],
  coordination: ORCHESTRATION_TOOLS,
};
const ALL_CAPABILITY_TOOLS = [...new Set(Object.values(CAPABILITY_TOOLS).flat())];

export interface ManagedToolPolicy {
  /** Exact Claude SDK built-in availability surface. MCP tools are configured separately. */
  tools: string[];
  /** Auto-approval policy only; never interpreted as availability. */
  allowedTools: string[];
  /** Explicit defense-in-depth denies, including every noncontract North MCP tool. */
  disallowedTools: string[];
}

export function managedToolPolicy(
  capabilities: readonly OrchestrationCapability[],
): ManagedToolPolicy {
  const selectedCapabilityTools = [
    ...new Set(capabilities.flatMap((capability) => CAPABILITY_TOOLS[capability])),
  ];
  const orchestrationAllowed = capabilities.includes("coordination");
  const allowedTools = [...new Set([
    ...selectedCapabilityTools,
    ...COORDINATION_TOOLS,
    ...(orchestrationAllowed ? ORCHESTRATION_TOOLS : []),
  ])];
  const disallowedTools = [...new Set([
    ...NATIVE_AGENT_TOOLS,
    ...ALL_CAPABILITY_TOOLS.filter((toolName) => !selectedCapabilityTools.includes(toolName)),
    ...ALL_NORTH_MCP_TOOLS.filter((toolName) => !allowedTools.includes(toolName)),
    ...(!orchestrationAllowed ? ["mcp__north-peer__command_peer"] : []),
  ])];
  return {
    tools: selectedCapabilityTools.filter((toolName) => !toolName.startsWith("mcp__")),
    allowedTools,
    disallowedTools,
  };
}

function readonlyShellServer(
  cwd: string,
  environment: NodeJS.ProcessEnv,
  abortSignal?: AbortSignal,
) {
  return createSdkMcpServer({
    name: READONLY_SHELL_SERVER,
    version: "0.1.0",
    tools: [
      tool(
        "run",
        "Run one command in North's network-isolated read-only shell. The checkout and host "
          + "filesystem are read-only; only an ephemeral /tmp is writable.",
        {
          command: z.string().min(1).max(MAX_READONLY_COMMAND_BYTES)
            .describe("Command interpreted intentionally by bash -lc inside the read-only sandbox"),
          timeoutMs: z.number().finite().int().min(100).max(120_000).optional()
            .describe("Bounded command timeout in milliseconds (default: 30000; maximum: 120000)"),
        },
        async ({ command, timeoutMs }) => {
          try {
            const result = await runReadonlyShell(
              command, cwd, timeoutMs, environment, abortSignal,
            );
            return {
              content: [{ type: "text", text: JSON.stringify(result) }],
              ...(!result.ok ? { isError: true } : {}),
            };
          } catch (error: any) {
            return {
              content: [{
                type: "text",
                text: JSON.stringify({
                  ok: false,
                  error: error?.code ?? "readonly_shell_unavailable",
                  message: error?.message ?? String(error),
                }),
              }],
              isError: true,
            };
          }
        },
      ),
    ],
  });
}

export interface HarnessOpts {
  self: string; // this agent's id/handle (peer commands + stream identity)
  extraTools?: string[]; // posture file-tools (Read/Edit/Write/...)
  model?: string;
  systemPrompt?: string;
  maxTurns?: number;
  /** Outer host lifecycle; provider adapters bridge this into owned processes. */
  abortController?: AbortController;
  provider?: ProviderId;
  routingMetadata?: RoutingRequest;
  /** A live run may change models in-place, so no exact-model delta can remain valid. */
  omitModelDeltaReason?: string;
  /** Original route intent plus target evidence, sealed into provider authority. */
  modelAvailability?: {
    exactModelPinned: boolean;
    targetId: string;
    receipt?: ProviderModelAdmissionReceipt;
  };
  cwd?: string; // provider working directory; dispatch resolves this from thread repo facts, spawn from opt-in worktree provisioning
  /** Capability-bound delivery context reserved before provider execution. */
  deliveryRun?: {
    runId: string;
    threadId: string;
    capability: string;
  };
  /** Sealed, run-scoped artifact directory exposed only to the managed North MCP. */
  artifactDirectory?: string;
  /** Test seam: false suppresses graph presence; a function captures registration hermetically. */
  presenceRegistrar?: false | ((self: string, cwd: string) => void);
  /** Matching heartbeat seam. Omit with production registration for the real renewer. */
  presenceRenewer?: false | ((self: string) => void);
  /** Explicit resource reads observed by the caller; absence remains unknown. */
  activatedResources?: readonly EnvironmentArtifact[];
  /** Explicit available-skill catalog observation; absence remains unknown. */
  availableSkills?: readonly EnvironmentArtifact[];
  /** Sealed zero-tool execution for data transforms whose input carries no authority. */
  dataOnly?: boolean;
  /** Provider-native structured output contract sealed with the managed lane. */
  outputFormat?: Options["outputFormat"];
  /** Explicit session-persistence policy sealed with the managed lane. */
  persistSession?: boolean;
}

// Auto-connect every SDK-spawned agent to north coordination — the SDK twin of
// the bin/north-on-spawn SessionStart hook. Presence so it shows on the roster;
// the concern protocol appended to the system prompt so it self-coordinates.
function registerPresence(self: string, cwd: string): void {
  // The canonical :7977 log — NOT a separate daemon: presence on :7978
  // stranded, invisible to concern/roster/board which all read :7977.
  // Resolve the port at dispatch time: Bun caches this module across test files,
  // while each hermetic spawn test installs its own transport after import.
  // Registration is a bounded synchronous admission edge. A detached child can
  // otherwise land its lease after a fast provider-preflight terminal has
  // already withdrawn presence, resurrecting a 30-minute ghost roster row.
  const output = execFileSync(presenceBb(), beagleStoreBabashkaArguments([
    `${REPO}/cli/presence-cli.clj`, northPort(), "register", self, cwd, self,
  ]), {
    env: beagleStoreEnvironment(),
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    timeout: beagleStoreCoordinatorChildTimeout(5_000),
  });
  persistPresenceFence(self, process.env.NORTH_IDENTITY_TEST_REDIRECT === "1"
    ? { resource: `session:${self}`, holder: self, epoch: 1 }
    : parsePresenceFence(output, self));
}

// SDK-lane presence heartbeat (F2). registerPresence writes the lease ONCE at
// spawn; the 30min TTL then lapses under any lane working longer — falsely
// `lapsed` while alive, which the concern-decay machinery reads as STALE. Fix:
// renew the lease on ACTIVITY. This is the SDK twin of bin/north-on-tooluse (the
// Claude Code PostToolUse hook) — renewal MEANS "this agent ran a tool just now"
// (IS-WORKING), so a lapsed lease stays a real death signal. NOT a setInterval:
// a timer on a hung-but-alive process would renew forever and defeat the
// lane lifecycle janitor's stuck-fork reaping (lapsed>30min + no outcome -> died-unreported).
// Throttle ≥60s per agent (a bb spawn per tool call is pure waste against a
// 30min lease); marker is an in-process Map (the hook callback runs in this host
// process, so no XDG marker file needed — and it can't alias across agents).
const RENEW_THROTTLE_MS = 60_000;
const lastRenew = new Map<string, number>();
function renewPresence(self: string): void {
  const now = Date.now();
  const prev = lastRenew.get(self) ?? 0;
  if (now - prev < RENEW_THROTTLE_MS) return;
  lastRenew.set(self, now); // stamp before dispatch so a burst of tool calls spawns one bb
  // Best-effort + timeout-bounded: complete the renewal before the tool hook
  // returns so no detached renew can recreate presence after terminal cleanup.
  // On failure, roll the stamp back so the next tool call retries.
  try {
    const fence = loadPresenceFence(self);
    const fenceJson = presenceFenceJson(fence);
    const output = execFileSync(presenceBb(), beagleStoreBabashkaArguments([
      `${REPO}/cli/presence-cli.clj`, northPort(), "renew", self, fenceJson,
    ]), {
      env: beagleStoreEnvironment(),
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: beagleStoreCoordinatorChildTimeout(5_000),
    });
    const renewed = process.env.NORTH_IDENTITY_TEST_REDIRECT === "1"
      ? fence
      : parsePresenceFence(output, self);
    if (process.env.NORTH_IDENTITY_TEST_REDIRECT !== "1" && renewed.epoch <= fence.epoch)
      throw new Error("presence renewal did not advance the exact lease fence");
    persistPresenceFence(self, renewed);
    // Throttle observability: a dispatched renewal is otherwise indistinguishable
    // from a throttled no-op, so the ≥60s rule has no field evidence. Off by default.
    if (process.env.NORTH_PRESENCE_DEBUG === "1")
      console.error(`[presence] @agent:${self} lease renewed (activity heartbeat)`);
  } catch (err) {
    if (lastRenew.get(self) === now) lastRenew.set(self, prev);
    // Fail-soft must not mean fail-silent: presence-cli exits nonzero on a
    // REJECTED lease, and swallowing that is exactly how a lapsed lane went on
    // reading as renewed. The lane keeps working; the operator hears about it.
    const e = err as { stderr?: Buffer | string; message?: string };
    const lines = (typeof e.stderr === "string" ? e.stderr : e.stderr?.toString() ?? "")
      .split("\n").map((l) => l.trim()).filter(Boolean);
    // The reject reason, not the bb stack trace that carried it.
    const detail = lines.find((l) => l.startsWith("presence:"))
      ?? lines.find((l) => l.startsWith("Message:")) ?? e.message ?? String(err);
    console.error(
      `[presence] @agent:${self} lease renewal FAILED — lane continues, roster may read lapsed: ${detail}`,
    );
  }
}
// The per-lane UNIQUE tail: agent id + repo are the only truly lane-specific
// bytes in the prompt, so they land LAST (P1) — after every shared tier — instead
// of at ~byte 330, where they used to defeat cross-lane prefix-cache sharing.
/**
 * A managed workspace-write Codex lane has network access. The Gitiles proxy
 * remains web-capability-only, and read-only lanes remain network-off. Every
 * North CLI write (`north tell`, `north evidence
 * record`, `bin/concern …`) reaches the graph through the coordinator socket,
 * so from inside that sandbox it CANNOT succeed — observed 2026-07-26:
 * `north tell` returns "no coordinator on 127.0.0.1:7977" with networkAccess
 * false and commits the fact when network is allowed. The North MCP server is
 * spawned by Codex OUTSIDE the sandbox, and its `tell` tool was observed
 * writing a durable fact from inside a managed thread. So: tell Codex agents
 * the write path they actually have, instead of one that is guaranteed to fail.
 *
 * READS go the same way. The coordinator socket is unreachable in either
 * direction from an unshared network namespace, so `north show` from the shell
 * is no more available than `north tell`; the MCP `show`/`threads` tools are the
 * lane's read path as well.
 */
function managedCodexShellBoundary(capabilities: readonly OrchestrationCapability[] = []): string {
  const network = managedCodexNetworkPolicy({
    sandbox: capabilities.includes("shell.readonly") ? "read-only" : "workspace-write",
    capabilities,
  });
  const networkBoundary = !network.networkAccess
    ? "Your shell has NO network."
    : capabilities.includes("web")
    ? "Your shell has network access; the managed web proxy is limited to chromium.googlesource.com."
    : "Your shell has network access; the managed web proxy remains disabled.";
  return [
    ``, ``, `## managed Codex sandbox — your actual write paths`,
    `${networkBoundary} Every North CLI that writes the graph (\`north tell\`,`,
    `\`north evidence record\`, \`bin/concern …\`) talks to the coordinator over a socket,`,
    `so from your shell it fails — a graph write attempted that way is a lost write.`,
    `Write the graph with the north MCP tools instead (tell, evidence_record, capture,`,
    `show, ready, next, threads). They run outside the sandbox and are the ONLY graph path`,
    `you have — use the MCP \`show\`/\`threads\` tools to READ too, not the shell CLI.`,
    `Your workspace IS writable, including its git metadata: stage and COMMIT on your`,
    `lane branch. You cannot push and must not try — your commits are`,
    `harvested to the canonical checkout when the lane settles, and the coordinator lands`,
    `them. Anything outside the workspace is read-only.`,
  ].join("\n");
}

function coordinationBlock(
  self: string, cwd: string, provider?: ProviderId,
  capabilities?: readonly OrchestrationCapability[],
): string {
  const repo = cwd.split("/").filter(Boolean).pop() ?? "repo";
  const proto = [
    ``, ``, `## north coordination`,
    `You are agent "${self}" in "${repo}". Other agents may work here concurrently.`,
    `Coordinate through CONCERNS, not locks — work coexists; declaring never blocks. Before`,
    `editing code for a feature, declare it so others can see + shape around your work:`,
    `  ${stableBinPath("concern")} declare ${self} ${repo} "<what you're building>" <file1,file2,...>`,
    `  ${stableBinPath("concern")} overlap <id>   # who's in your footprint; likely-to-land marked — build against it`,
    `  ${stableBinPath("concern")} candidate <id> [git-rev] · done <id> · ls [repo]`,
    ``,
    `Internal notes / status / scratch / handoffs -> docs/private/ (gitignored), NEVER public docs/.`,
    `Run \`${stableBinPath("ensure-private-docs")}\` to set up the ignore in a repo before writing there.`,
  ].join("\n");
  return provider === "openai" ? proto + managedCodexShellBoundary(capabilities) : proto;
}

// AGENT_ESO=on|off — appends dense-handoff instruction to every spawned agent.
// When on, agents emit uniform arrays of ≥10 records as ESO instead of JSON/markdown.
function esoAppendix(env: NodeJS.ProcessEnv = process.env): string {
  const mode = env.AGENT_ESO ?? "on";
  if (mode !== "on") return "";
  return "\n\n" +
    "DENSE HANDOFF — when a final report contains a uniform array of ≥10 similar records " +
    "(grep hits, findings, file lists), emit it in ESO format instead of JSON or markdown table.\n" +
    `Mini-syntax (full spec: ${esoSpecPath()}):\n` +
    "  !eso/1              ← required header\n" +
    "  name=value          ← scalar field\n" +
    "  items[N]{a,b,c}     ← N records, schema declared once; N is a checksum\n" +
    "  val1\\tval2\\tval3   ← one tab-delimited row per record (strings with tabs/newlines use JSON quoting)";
}

// AGENT_LAWS=on|off — appends the user's provider-neutral global AGENTS.md to Anthropic
// workers. Codex loads the same global file natively; injecting it there would duplicate
// the bootstrap. Project AGENTS files are composed explicitly for both providers below.
// A custom-string systemPrompt bypasses the SDK's claude_code preset, which is the
// only path that injects CLAUDE.md — so without this, workers get NONE of the
// global laws interactive sessions live under. The provider-neutral bootstrap
// source resolves to an exact AGENT_LAWS_PATH or, failing that, ~/.agents/AGENTS.md —
// never a provider home like ~/.codex. Missing, replaced, unreadable, malformed,
// or oversized authority is a hard configuration error; AGENT_LAWS=off is the
// sole explicit escape hatch.
export const GLOBAL_AGENTS_MAX_BYTES = 32 * 1024;

export interface CanonicalGlobalAgents {
  path: string;
  realpath: string;
  bytes: Buffer;
  text: string;
}

function agentLawsEnabled(env: NodeJS.ProcessEnv = process.env): boolean {
  const mode = env.AGENT_LAWS ?? "on";
  if (mode === "on") return true;
  if (mode === "off") return false;
  throw new Error("AGENT_LAWS must be exactly 'on' or 'off'");
}

function readGlobalAgents(path: string, label: string): Omit<CanonicalGlobalAgents, "path" | "realpath"> {
  let info;
  try { info = statSync(path); }
  catch (cause) {
    throw new Error(`global AGENTS bootstrap cannot inspect ${label}: ${path}`, { cause });
  }
  if (!info.isFile())
    throw new Error(`global AGENTS bootstrap ${label} is not a regular file: ${path}`);
  if (info.size > GLOBAL_AGENTS_MAX_BYTES)
    throw new Error(`global AGENTS bootstrap exceeds ${GLOBAL_AGENTS_MAX_BYTES} bytes at: ${path}`);

  let bytes: Buffer;
  try { bytes = readFileSync(path); }
  catch (cause) {
    throw new Error(`global AGENTS bootstrap cannot read ${label}: ${path}`, { cause });
  }
  if (bytes.byteLength > GLOBAL_AGENTS_MAX_BYTES)
    throw new Error(`global AGENTS bootstrap exceeds ${GLOBAL_AGENTS_MAX_BYTES} bytes at: ${path}`);

  let text: string;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
  catch (cause) {
    throw new Error(`global AGENTS bootstrap is not valid UTF-8 at: ${path}`, { cause });
  }
  if (!text.trim()) throw new Error(`global AGENTS bootstrap is empty at: ${path}`);
  return { bytes, text };
}

/**
 * The exact provider-neutral global-authority path: an explicit AGENT_LAWS_PATH
 * wins outright, otherwise ~/.agents/AGENTS.md. Never a provider config home
 * (~/.codex, ~/.claude) — those remain each provider's own native surface.
 */
export function globalLawsPath(env: NodeJS.ProcessEnv = process.env): string {
  const override = env.AGENT_LAWS_PATH?.trim();
  if (override) return resolve(override);
  const home = env.HOME?.trim();
  if (!home) throw new Error("global AGENTS bootstrap requires AGENT_LAWS_PATH or HOME");
  return resolve(home, ".agents", "AGENTS.md");
}

/** The exact provider-neutral global authority source for this process home. */
export function canonicalGlobalAgents(
  env: NodeJS.ProcessEnv = process.env,
): CanonicalGlobalAgents | undefined {
  if (!agentLawsEnabled(env)) return undefined;
  const path = globalLawsPath(env);
  const source = readGlobalAgents(path, "canonical source");
  let canonicalPath: string;
  try { canonicalPath = realpathSync(path); }
  catch (cause) {
    throw new Error(`global AGENTS bootstrap cannot resolve canonical source: ${path}`, { cause });
  }
  return { path, realpath: canonicalPath, ...source };
}

function globalLawsAppendix(env: NodeJS.ProcessEnv = process.env): string {
  const laws = canonicalGlobalAgents(env);
  if (!laws) return "";
  const trailingNewline = laws.text.endsWith("\n") ? "" : "\n";
  return `\n\n## Global laws — ${laws.path} (binds every provider and agent)\n\n`
    + laws.text + trailingNewline;
}

export const PROJECT_AGENTS_MAX_BYTES = 32 * 1024;

function gitRootForProject(cwd: string): { cwd: string; root: string } {
  let canonicalCwd: string;
  try {
    canonicalCwd = realpathSync(cwd);
    if (!statSync(canonicalCwd).isDirectory())
      throw new Error(`working directory is not a directory: ${canonicalCwd}`);
  } catch (cause) {
    throw new Error(`project AGENTS bootstrap cannot resolve cwd: ${cwd}`, { cause });
  }
  let cursor = canonicalCwd;
  while (true) {
    const marker = resolve(cursor, ".git");
    try {
      const markerStat = statSync(marker);
      if (!markerStat.isDirectory() && !markerStat.isFile())
        throw new Error(`Git marker is neither file nor directory: ${marker}`);
      return { cwd: canonicalCwd, root: cursor };
    } catch (error: any) {
      if (error?.code !== "ENOENT")
        throw new Error(`project AGENTS bootstrap cannot inspect Git marker: ${marker}`, { cause: error });
    }
    const parent = dirname(cursor);
    if (parent === cursor) return { cwd: canonicalCwd, root: canonicalCwd };
    cursor = parent;
  }
}

function projectInstructionFile(directory: string): string | undefined {
  for (const name of ["AGENTS.override.md", "AGENTS.md"]) {
    const path = resolve(directory, name);
    let info;
    try {
      info = statSync(path);
    } catch (error: any) {
      if (error?.code === "ENOENT") continue;
      throw new Error(`project AGENTS bootstrap cannot inspect: ${path}`, { cause: error });
    }
    if (!info.isFile())
      throw new Error(`project instruction source is not a regular file: ${path}`);
    if (info.size > PROJECT_AGENTS_MAX_BYTES) {
      throw new Error(
        `project AGENTS bootstrap exceeds ${PROJECT_AGENTS_MAX_BYTES} bytes at: ${path}`,
      );
    }
    return path;
  }
  return undefined;
}

/**
 * Deterministic, bounded policy-root-to-cwd project instruction composition.
 *
 * Managed Codex disables native project-doc loading and consumes this same block,
 * while Anthropic receives it directly because the SDK's settings sources are
 * sealed off. A discovered but unreadable/malformed/oversized instruction source
 * blocks the spawn instead of silently creating a provider-specific authority gap.
 */
export function projectAgentsAppendix(
  cwd: string,
  env: NodeJS.ProcessEnv = process.env,
): string {
  if (!agentLawsEnabled(env)) return "";
  const project = gitRootForProject(cwd);
  let policyRoot = project.root;
  const home = env.HOME?.trim();
  if (home) {
    try {
      const canonicalHome = realpathSync(home);
      const fromHome = relative(canonicalHome, project.cwd);
      if (fromHome !== ".." && !fromHome.startsWith(`..${sep}`) && !fromHome.startsWith(sep))
        policyRoot = canonicalHome;
    } catch { /* a missing HOME cannot widen authority beyond the Git root */ }
  }
  const rel = relative(policyRoot, project.cwd);
  if (rel === ".." || rel.startsWith(`..${sep}`))
    throw new Error(`project AGENTS bootstrap cwd escapes policy root: ${project.cwd}`);
  const directories = [policyRoot];
  let cursor = policyRoot;
  for (const segment of rel.split(sep).filter(Boolean)) {
    cursor = resolve(cursor, segment);
    directories.push(cursor);
  }

  const global = canonicalGlobalAgents(env);
  const seenRealpaths = new Set<string>();
  const sections: string[] = [];
  for (const directory of directories) {
    const path = projectInstructionFile(directory);
    if (!path) continue;
    let sourceRealpath: string;
    try { sourceRealpath = realpathSync(path); }
    catch (cause) {
      throw new Error(`project AGENTS bootstrap cannot resolve: ${path}`, { cause });
    }
    if (sourceRealpath === global?.realpath || seenRealpaths.has(sourceRealpath)) continue;
    let source: Buffer;
    try { source = readFileSync(path); }
    catch (cause) {
      throw new Error(`project AGENTS bootstrap cannot read: ${path}`, { cause });
    }
    if (global?.bytes.equals(source)) continue;
    let text: string;
    try { text = new TextDecoder("utf-8", { fatal: true }).decode(source).trim(); }
    catch (cause) {
      throw new Error(`project AGENTS bootstrap is not valid UTF-8: ${path}`, { cause });
    }
    if (!text) continue;
    seenRealpaths.add(sourceRealpath);
    const next = [...sections, `### ${path}\n\n${text}`];
    const appendix = `\n\n## Project instructions — policy root to cwd\n\n${next.join("\n\n")}`;
    if (Buffer.byteLength(appendix, "utf8") > PROJECT_AGENTS_MAX_BYTES) {
      throw new Error(
        `project AGENTS bootstrap exceeds ${PROJECT_AGENTS_MAX_BYTES} bytes at: ${path}`,
      );
    }
    sections.push(next.at(-1)!);
  }
  return sections.length
    ? `\n\n## Project instructions — policy root to cwd\n\n${sections.join("\n\n")}`
    : "";
}

function assertCanonicalGlobalAgentsExactlyOnce(
  prompt: string,
  env: NodeJS.ProcessEnv = process.env,
): void {
  const canonical = canonicalGlobalAgents(env);
  if (!canonical) return;
  const needle = canonical.text.trim();
  let count = 0;
  let offset = 0;
  while ((offset = prompt.indexOf(needle, offset)) !== -1) {
    count++;
    offset += needle.length;
  }
  if (count !== 1)
    throw new Error(`Anthropic global AGENTS bootstrap expected exactly once, observed ${count}`);
}

function orchestrationHome(env: NodeJS.ProcessEnv = process.env): string {
  return resolve(env.NORTH_ORCHESTRATION_HOME ?? resolve(import.meta.dir, "..", "..", "orchestration"));
}

function orchestrationDocs(env: NodeJS.ProcessEnv = process.env): string {
  return resolve(orchestrationHome(env), "docs");
}

function extractFenceFromSection(text: string, heading: string): string | null {
  const lines = text.split("\n");
  const headingLower = `## ${heading.toLowerCase()}`;
  let sectionStart = -1;
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].trim().toLowerCase() === headingLower) { sectionStart = i + 1; break; }
  }
  if (sectionStart === -1) return null;
  let fenceOpen = -1;
  for (let i = sectionStart; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (fenceOpen === -1 && trimmed.startsWith("## ")) break; // next heading, no fence found
    if (fenceOpen === -1 && trimmed.startsWith("```")) { fenceOpen = i + 1; continue; }
    if (fenceOpen !== -1 && trimmed.startsWith("```")) return lines.slice(fenceOpen, i).join("\n");
  }
  return null;
}

function extractFirstFence(text: string): string | null {
  const lines = text.split("\n");
  let fenceOpen = -1;
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (fenceOpen === -1 && trimmed.startsWith("```")) { fenceOpen = i + 1; continue; }
    if (fenceOpen !== -1 && trimmed.startsWith("```")) return lines.slice(fenceOpen, i).join("\n");
  }
  return null;
}

function exactSectionFence(path: string, heading: string, label: string): string {
  let source: string;
  try { source = readFileSync(path, "utf8"); }
  catch { throw new Error(`Orchestration contract unavailable: ${label} (${path})`); }
  const block = extractFenceFromSection(source, heading);
  if (!block?.trim()) throw new Error(`Orchestration contract malformed: ${label} has no fenced block (${path})`);
  return block;
}

function exactFirstFence(path: string, label: string): string {
  let source: string;
  try { source = readFileSync(path, "utf8"); }
  catch { throw new Error(`Orchestration contract unavailable: ${label} (${path})`); }
  const block = extractFirstFence(source);
  if (!block?.trim()) throw new Error(`Orchestration contract malformed: ${label} has no fenced block (${path})`);
  return block;
}

function listLines(values: string[]): string {
  return values.map((value) => `- ${value}`).join("\n");
}

function bespokeRoleBlock(metadata: RoutingRequest): string {
  if (metadata.composition?.kind !== "bespoke") throw new Error("bespoke role block requires bespoke composition");
  const c = metadata.composition.contract;
  return [
    `ROLE: BESPOKE ${metadata.composition.id.toUpperCase()}.`,
    `Responsibility: ${c.responsibility}`,
    `Deliverable: ${c.deliverable}`,
    "May decide:", listLines(c.mayDecide),
    "Must escalate:", listLines(c.mustEscalate),
    "Done when:", listLines(c.doneWhen),
    `REPORT: ${c.report}`,
    `Why bespoke: ${metadata.composition.bespokeReason}`,
    `Promotion candidate: ${metadata.composition.promotionCandidate ? "yes" : "no"}.`,
  ].join("\n");
}

function requirementSlug(requirement: string): string {
  return requirement.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}

/**
 * The active shared-skill root from North's current immutable generation.
 */
export function domainSkillsDir(env: NodeJS.ProcessEnv = process.env): string {
  const override = env.NORTH_AGENT_SKILLS?.trim();
  if (override) return resolve(override);
  const stateRoot = env.NORTH_AGENT_STATE_ROOT?.trim()
    || resolve(env.HOME ?? "", ".local", "state", "north", "agents");
  return resolve(stateRoot, "current", "skills", "shared");
}

export interface ActiveSkillCandidate {
  readonly name: string;
  readonly description: string;
  readonly path: string;
}

export interface ActiveSkillCatalog {
  readonly root: string;
  readonly candidates: readonly ActiveSkillCandidate[];
  readonly appendix: string;
}

const SKILL_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

function skillTriggerMetadata(path: string, folder: string): ActiveSkillCandidate {
  let bytes: Buffer;
  try {
    const info = statSync(path);
    if (!info.isFile()) throw new Error(`active skill source is not a regular file: ${path}`);
    bytes = readFileSync(path);
  } catch (cause) {
    throw new Error(`active skill source is stale or unreadable: ${path}`, { cause });
  }
  let text: string;
  try { text = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
  catch (cause) { throw new Error(`active skill source is not valid UTF-8: ${path}`, { cause }); }
  const frontmatter = text.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/)?.[1];
  if (frontmatter === undefined) throw new Error(`active skill frontmatter is malformed: ${path}`);
  let parsed: unknown;
  try { parsed = Bun.YAML.parse(frontmatter); }
  catch (cause) { throw new Error(`active skill frontmatter is invalid YAML: ${path}`, { cause }); }
  const name = (parsed as any)?.name;
  const rawDescription = (parsed as any)?.description;
  if (typeof name !== "string" || !SKILL_NAME.test(name) || name !== folder)
    throw new Error(`active skill name must equal its folder ${folder}: ${path}`);
  if (typeof rawDescription !== "string" || !rawDescription.trim())
    throw new Error(`active skill description is missing: ${path}`);
  return Object.freeze({
    name,
    description: rawDescription.replace(/\s+/g, " ").trim(),
    path,
  });
}

/** Metadata-only view of the active provider-neutral skill farm. */
export function activeSkillCatalog(
  env: NodeJS.ProcessEnv = process.env,
): ActiveSkillCatalog {
  const root = domainSkillsDir(env);
  let entries: import("node:fs").Dirent[];
  try {
    const info = statSync(root);
    if (!info.isDirectory()) throw new Error(`active skill catalog is not a directory: ${root}`);
    entries = readdirSync(root, { withFileTypes: true });
  } catch (cause: any) {
    if (cause?.code === "ENOENT") return Object.freeze({ root, candidates: Object.freeze([]), appendix: "" });
    throw new Error(`active skill catalog is unreadable: ${root}`, { cause });
  }
  const candidates = entries
    .sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0)
    .map((entry) => {
      const directory = resolve(root, entry.name);
      try {
        if (!statSync(directory).isDirectory())
          throw new Error(`active skill entry is not a directory: ${directory}`);
      } catch (cause) {
        throw new Error(`active skill entry is stale or unreadable: ${directory}`, { cause });
      }
      return skillTriggerMetadata(resolve(directory, "SKILL.md"), entry.name);
    });
  const rows = candidates.map((candidate) => `- ${JSON.stringify(candidate)}`);
  const appendix = rows.length ? [
    "", "", `## Active skill candidates — ${root}`,
    "Trigger metadata only. Match the request against every description and load each",
    "matching SKILL.md under the global skill-loading law; no skill body is injected here.",
    ...rows,
  ].join("\n") : "";
  return Object.freeze({ root, candidates: Object.freeze(candidates), appendix });
}

function domainContextCandidates(
  cwd: string,
  requirement: string,
  env: NodeJS.ProcessEnv = process.env,
): string[] {
  const slug = requirementSlug(requirement);
  const candidates = [
    resolve(cwd, "AGENTS.md"),
    resolve(cwd, "docs", `${slug}.md`),
    resolve(cwd, "docs", "domains", `${slug}.md`),
    resolve(domainSkillsDir(env), slug, "SKILL.md"),
    resolve(orchestrationHome(env), "docs", "domains", `${slug}.md`),
  ];
  return [...new Set(candidates.filter(existsSync))];
}

function domainContextGate(
  requirements: string[], cwd: string, env: NodeJS.ProcessEnv = process.env,
): string {
  if (!requirements.length) return "";
  const entries = requirements.map((requirement) => {
    const candidates = domainContextCandidates(cwd, requirement, env);
    return [
      `### ${requirement}`,
      candidates.length
        ? `Candidate entry points (candidates are not proof of expertise):\n${listLines(candidates)}`
        : "No context candidate was discovered by the harness.",
    ].join("\n");
  });
  return [
    "## Orchestration domain-context gate",
    "Before any side effect, satisfy every domain requirement by reading the relevant",
    "repo-local authoritative docs, triggered skills, or provider capability contract.",
    "For each requirement, name the exact artifact actually read and apply it. A candidate",
    "path is only an entry point, never evidence that you possess the expertise. If no",
    "authoritative context exists or access is missing, report `DOMAIN CONTEXT MISSING:",
    "<requirement>` to the orchestrator and stop before side effects; never fake expertise.",
    ...entries,
  ].join("\n");
}

export interface ModelDeltaEvidence {
  provider?: ProviderId;
  model?: string;
  kind: "calibrated" | "none" | "omitted";
  path?: string;
  reason?: string;
}

export interface HarnessCompositionEvidence {
  roleKind?: "template" | "bespoke";
  roleId?: string;
  bespokeContractHash?: string;
  bespokeContractFingerprintVersion?: string;
  bespokeContractFingerprintDomain?: string;
  templateOverrides?: RoutingOverrideField[];
  templateOverrideReasonHash?: string;
  capabilities?: OrchestrationCapability[];
  commsContractHash?: string;
  taskGrade?: string;
  domainRequirements?: string[];
  topology?: Topology;
  tier?: string;
  reasoning?: string;
  posture?: string;
  modelDelta?: ModelDeltaEvidence;
  promptEconomics?: PromptEconomicsEvidence;
  promptReceipt?: PromptReceipt;
  environmentReceipt?: EnvironmentReceipt;
}

export const PROMPT_COMPOSITION_VERSION = "north-harness-prompt:v2";
export const COMPACTION_POLICY_VERSION = "north-native-auto-compact:v1";

export interface PromptEconomicsEvidence {
  compositionVersion: typeof PROMPT_COMPOSITION_VERSION;
  compositionDigest: string;
  capabilityClass: "unknown" | "readonly" | "readonly-web" | "authoring" | "orchestrator";
  capabilityCount: number;
  stablePrefixBytes: number;
  uniqueTailBytes: number;
  totalBytes: number;
  byteMeasurementSource: "node-buffer-byte-length:utf8";
  stablePrefixTokens?: number;
  uniqueTailTokens?: number;
  totalCompositionTokens?: number;
  tokenMeasurementStatus: "observed" | "unknown";
  tokenMeasurementSource: "authoritative-tokenizer-unavailable" | "provider-authoritative-tokenizer";
  providerContextWindowTokens?: number;
  contextWindowEffectiveFrom?: string;
  contextWindowStatus: "observed" | "unknown";
  contextWindowSource: "orchestration-provider-catalog" | "provider-or-model-unresolved" | "catalog-metadata-unavailable";
  effectiveContextBudgetTokens?: number;
  contextBudgetStatus: "unknown";
  contextBudgetSource: "north-harness-unconfigured";
  compactionPolicy: "native-auto-compact-enabled";
  compactionPolicyVersion: typeof COMPACTION_POLICY_VERSION;
}

function capabilityClass(
  capabilities: readonly OrchestrationCapability[] | undefined,
  topology: Topology | undefined,
): PromptEconomicsEvidence["capabilityClass"] {
  if (!capabilities) return "unknown";
  if (topology === "orchestrator" && capabilities.includes("coordination")) return "orchestrator";
  if (hasAuthoringCapability(capabilities)) return "authoring";
  if (capabilities.includes("web")) return "readonly-web";
  return "readonly";
}

interface HarnessCompositionState {
  // Prompt ingredients are snapshotted once and recomposed identically on every
  // provider fallback.
  self: string;
  basePrompt: string;
  skillCatalog: ActiveSkillCatalog;
  orchestrationAppendix: string;
  capabilities?: OrchestrationCapability[];
  cwd: string;
  evidence: HarnessCompositionEvidence;
  routingRequest?: RoutingRequest;
  routeBase?: Options;
  initialProvider?: ProviderId;
  initialModel?: string;
  initialEffort?: Effort;
  omitModelDeltaReason?: string;
  exactModelPinned: boolean;
  dataOnly: boolean;
  /** Immutable composer input; dynamic branches may not reread ambient env. */
  environment: NodeJS.ProcessEnv;
}

// Anthropic receives the byte-exact canonical bootstrap; Codex loads the same
// file natively. Both providers receive the same metadata-only active-skill
// catalog and managed project instructions before the per-lane tail.
function composeSystemPrompt(
  state: HarnessCompositionState,
  provider: ProviderId | undefined,
  model: string | undefined,
): {
  prompt: string;
  deltaEvidence: ModelDeltaEvidence;
  economics: PromptEconomicsEvidence;
  receipt: PromptReceipt;
} {
  const includeBootstrap = provider === undefined || provider === "anthropic";
  const bootstrap = includeBootstrap ? globalLawsAppendix(state.environment) : "";
  const skillCatalog = state.dataOnly ? "" : state.skillCatalog.appendix;
  const delta = modelDeltaAppendix(
    provider,
    model,
    state.dataOnly ? "data-only contract excludes model prompt deltas" : state.omitModelDeltaReason,
  );
  const core = state.basePrompt + bootstrap + skillCatalog;
  const cap = state.orchestrationAppendix;
  const project = projectAgentsAppendix(state.cwd, state.environment);
  const repo = project;
  const coordination = state.dataOnly
    ? ""
    : coordinationBlock(state.self, state.cwd, provider, state.capabilities);
  const tail = coordination + delta.appendix;
  const stablePrefix = core + cap + repo;
  const prompt = stablePrefix + tail;
  const chunks = [
    ["core-base", state.basePrompt],
    ["global-bootstrap", bootstrap],
    ["active-skill-catalog", skillCatalog],
    ["orchestration", state.orchestrationAppendix],
    ["project-instructions", project],
    ["coordination", coordination],
    ["model-delta", delta.appendix],
  ] as const;
  const receipt = buildPromptReceipt({
    coverage: "exact",
    wirePrompt: prompt,
    modules: chunks.map(([id, rendered], position) => ({
      id, schemaVersion: "v1", position,
      dependencies: position === 0 ? [] : [chunks[position - 1]![0]],
      sourceSha256: sha256Bytes(rendered), rendered,
      ...(id === "core-base" ? {
        parameterDigests: {
          esoMode: sha256Bytes(state.environment.AGENT_ESO ?? "on"),
          lawsMode: sha256Bytes(state.environment.AGENT_LAWS ?? "on"),
        },
      } : {}),
    })),
    branches: [
      {
        ruleId: "global-bootstrap-provider", conditionId: "provider-kind",
        inputDigest: sha256Bytes(provider ?? "unresolved"),
        branch: includeBootstrap ? "included" : "native-provider",
      },
      {
        ruleId: "model-delta", conditionId: "resolved-model",
        inputDigest: sha256Bytes(`${provider ?? "unresolved"}:${model ?? "unresolved"}`),
        branch: delta.evidence.kind,
      },
      {
        ruleId: "capability-class", conditionId: "capability-set",
        inputDigest: sha256Bytes(JSON.stringify([...(state.capabilities ?? [])].sort())),
        branch: capabilityClass(state.capabilities, state.evidence.topology),
      },
    ],
  });
  let contextWindow: ReturnType<typeof observeProviderContextWindow>;
  try {
    contextWindow = provider && model ? observeProviderContextWindow(provider, model) : undefined;
  } catch { contextWindow = undefined; }
  const economics: PromptEconomicsEvidence = {
    compositionVersion: PROMPT_COMPOSITION_VERSION,
    compositionDigest: createHash("sha256").update(prompt).digest("hex"),
    capabilityClass: capabilityClass(state.capabilities, state.evidence.topology),
    capabilityCount: state.capabilities?.length ?? 0,
    stablePrefixBytes: Buffer.byteLength(stablePrefix, "utf8"),
    uniqueTailBytes: Buffer.byteLength(tail, "utf8"),
    totalBytes: Buffer.byteLength(prompt, "utf8"),
    byteMeasurementSource: "node-buffer-byte-length:utf8",
    tokenMeasurementStatus: "unknown",
    tokenMeasurementSource: "authoritative-tokenizer-unavailable",
    ...(contextWindow ? {
      providerContextWindowTokens: contextWindow.tokens,
      contextWindowEffectiveFrom: contextWindow.effectiveFrom,
    } : {}),
    contextWindowStatus: contextWindow ? "observed" : "unknown",
    contextWindowSource: contextWindow
      ? "orchestration-provider-catalog"
      : provider && model ? "catalog-metadata-unavailable" : "provider-or-model-unresolved",
    contextBudgetStatus: "unknown",
    contextBudgetSource: "north-harness-unconfigured",
    compactionPolicy: "native-auto-compact-enabled",
    compactionPolicyVersion: COMPACTION_POLICY_VERSION,
  };
  return { prompt, deltaEvidence: delta.evidence, economics, receipt };
}

const harnessComposition = new WeakMap<object, HarnessCompositionState>();
const appliedEvidence = new WeakMap<object, HarnessCompositionEvidence>();
const harnessActivityRenewers = new WeakMap<object, () => void>();
interface HarnessAuthoritySeal {
  provider: ProviderId;
  optionKeys: readonly string[];
  optionValues: readonly unknown[];
  systemPrompt: string;
  routingRequest: RoutingRequest;
  capabilities: readonly OrchestrationCapability[];
  evidence: HarnessCompositionEvidence;
  env: object;
  mcpServers: object;
  mcpServerEntries: Array<[string, unknown]>;
  northServer: object;
  tools?: unknown;
  allowedTools: unknown;
  disallowedTools?: unknown;
  settingSources?: unknown;
  strictMcpConfig?: unknown;
  permissionMode?: unknown;
  agentId: string;
  managedLane: "1";
  topology: Topology;
  cwd: string;
  effort?: Effort;
  model?: string;
  maxTurns?: number;
  modelAvailability: HarnessModelAvailabilityBinding;
  dataOnly: boolean;
}
const harnessAuthoritySeals = new WeakMap<object, HarnessAuthoritySeal>();
export interface HarnessModelAvailabilityBinding {
  readonly required: boolean;
  readonly targetId: string;
  readonly model?: string;
  readonly receipt?: ProviderModelAdmissionReceipt;
  readonly observationPath: string;
}
interface AuthoringHookSealEntry {
  matcher?: string;
  hooks: unknown[];
}
interface AuthoringHookSeal {
  topology?: string;
  entries: AuthoringHookSealEntry[];
  postEntries: AuthoringHookSealEntry[];
  mcpServers: Array<[string, unknown]>;
}
const authoringHookSeals = new WeakMap<object, AuthoringHookSeal>();

function deepFreeze<T>(value: T): T {
  if (value && typeof value === "object" && !Object.isFrozen(value)) {
    for (const child of Object.values(value as Record<string, unknown>)) deepFreeze(child);
    Object.freeze(value);
  }
  return value;
}

function sealHarnessAuthority(options: Options, provider: ProviderId): void {
  const raw = options as any;
  if (!raw.northRoutingRequest || !raw.northCapabilities) return;
  const evidence = appliedEvidence.get(options as object);
  const northServer = raw.mcpServers?.north as object | undefined;
  if (!evidence || typeof raw.systemPrompt !== "string"
      || !raw.env || (!northServer && raw.northDataOnly !== true)
      || typeof raw.cwd !== "string") return;
  const optionKeys = Object.keys(raw).sort();
  harnessAuthoritySeals.set(options as object, {
    provider,
    optionKeys: Object.freeze(optionKeys),
    optionValues: Object.freeze(optionKeys.map((key) => deepFreeze(raw[key]))),
    systemPrompt: raw.systemPrompt,
    routingRequest: raw.northRoutingRequest,
    capabilities: raw.northCapabilities,
    evidence,
    env: raw.env,
    mcpServers: raw.mcpServers,
    mcpServerEntries: Object.entries(raw.mcpServers),
    northServer: northServer ?? raw.mcpServers,
    tools: raw.tools,
    allowedTools: raw.allowedTools,
    disallowedTools: raw.disallowedTools,
    settingSources: raw.settingSources,
    strictMcpConfig: raw.strictMcpConfig,
    permissionMode: raw.permissionMode,
    agentId: raw.env.AGENT_ID,
    managedLane: raw.env.NORTH_MANAGED_LANE,
    topology: raw.env.AGENT_TOPOLOGY,
    cwd: raw.cwd,
    effort: raw.effort,
    model: raw.model,
    maxTurns: raw.maxTurns,
    modelAvailability: raw.northModelAvailability,
    dataOnly: raw.northDataOnly === true,
  });
}

/** Exact harness-owned authority receipt consumed by both provider adapters. */
export function hasCanonicalHarnessAuthority(options: Options, provider: ProviderId): boolean {
  const raw = options as any;
  const seal = harnessAuthoritySeals.get(options as object);
  const mcpServerEntries = Object.entries(raw.mcpServers ?? {});
  const optionKeys = Object.keys(raw).sort();
  return Boolean(
    seal
    && seal.provider === provider
    && optionKeys.length === seal.optionKeys.length
    && optionKeys.every((key, index) =>
      key === seal.optionKeys[index]
      && Object.is(raw[key], seal.optionValues[index]))
    && raw.systemPrompt === seal.systemPrompt
    && raw.northRoutingRequest === seal.routingRequest
    && raw.northCapabilities === seal.capabilities
    && appliedEvidence.get(options as object) === seal.evidence
    && raw.env === seal.env
    && raw.mcpServers === seal.mcpServers
    && mcpServerEntries.length === seal.mcpServerEntries.length
    && mcpServerEntries.every(([name, server], index) =>
      name === seal.mcpServerEntries[index]?.[0]
      && server === seal.mcpServerEntries[index]?.[1])
    && (raw.northDataOnly === true
      ? raw.mcpServers === seal.northServer
      : raw.mcpServers?.north === seal.northServer)
    && raw.tools === seal.tools
    && raw.allowedTools === seal.allowedTools
    && raw.disallowedTools === seal.disallowedTools
    && raw.settingSources === seal.settingSources
    && raw.strictMcpConfig === seal.strictMcpConfig
    && raw.permissionMode === seal.permissionMode
    && raw.env.AGENT_ID === seal.agentId
    && raw.env.NORTH_MANAGED_LANE === seal.managedLane
    && raw.env.AGENT_TOPOLOGY === seal.topology
    && raw.cwd === seal.cwd
    && raw.effort === seal.effort
    && raw.model === seal.model
    && raw.maxTurns === seal.maxTurns
    && raw.northModelAvailability === seal.modelAvailability
    && (raw.northDataOnly === true) === seal.dataOnly,
  );
}

/** Availability authority is readable only from an otherwise canonical seal. */
export function canonicalHarnessModelAvailability(
  options: Options,
  provider: ProviderId,
): HarnessModelAvailabilityBinding | undefined {
  if (!hasCanonicalHarnessAuthority(options, provider)) return undefined;
  return harnessAuthoritySeals.get(options as object)?.modelAvailability;
}

function sealAuthoringHooks(options: Options): void {
  const entries = (options.hooks as any)?.PreToolUse;
  const postEntries = (options.hooks as any)?.PostToolUse;
  if (!Array.isArray(entries) || !Array.isArray(postEntries)) return;
  const snapshot = (values: any[]): AuthoringHookSealEntry[] =>
    values.map((entry: any) => ({
      matcher: entry?.matcher,
      hooks: Array.isArray(entry?.hooks) ? [...entry.hooks] : [],
    }));
  authoringHookSeals.set(options as object, {
    topology: (options.env as any)?.AGENT_TOPOLOGY,
    entries: snapshot(entries),
    postEntries: snapshot(postEntries),
    mcpServers: Object.entries((options.mcpServers as any) ?? {}),
  });
}

function inheritAuthoringHookSeal(source: Options, target: Options): void {
  const seal = authoringHookSeals.get(source as object);
  if (seal) authoringHookSeals.set(target as object, seal);
}

/**
 * Provider admission proof that the SDK-only guard chain and exact MCP server
 * instances came from harnessOptions and were not replaced before the model turn.
 */
export function hasCanonicalAuthoringHooks(options: Options): boolean {
  const seal = authoringHookSeals.get(options as object);
  const hookSurface = options.hooks as any;
  const entries = (options.hooks as any)?.PreToolUse;
  const postEntries = (options.hooks as any)?.PostToolUse;
  const mcpServers = Object.entries((options.mcpServers as any) ?? {});
  if (!seal
      || !hookSurface
      || Object.keys(hookSurface).sort().join(",") !== "PostToolUse,PreToolUse"
      || (options.env as any)?.AGENT_TOPOLOGY !== seal.topology
      || !Array.isArray(entries)
      || !Array.isArray(postEntries)
      || entries.length !== seal.entries.length
      || postEntries.length !== seal.postEntries.length
      || mcpServers.length !== seal.mcpServers.length
      || mcpServers.some(([name, server], index) =>
        name !== seal.mcpServers[index][0] || server !== seal.mcpServers[index][1])) return false;
  const exactEntries = (actualEntries: any[], expectedEntries: AuthoringHookSealEntry[]) =>
    expectedEntries.every((expected, index) => {
      const actual = actualEntries[index];
      const expectedKeys = expected.matcher === undefined ? ["hooks"] : ["hooks", "matcher"];
      return actual && typeof actual === "object" && !Array.isArray(actual)
        && Object.keys(actual).sort().join(",") === expectedKeys.join(",")
        && actual.matcher === expected.matcher
        && Array.isArray(actual?.hooks)
        && actual.hooks.length === expected.hooks.length
        && actual.hooks.every((hook: unknown, hookIndex: number) =>
          hook === expected.hooks[hookIndex]);
    });
  return exactEntries(entries, seal.entries)
    && exactEntries(postEntries, seal.postEntries);
}

/** Compose Orchestration's authority contracts. Missing canonical artifacts are fatal. */
export function orchestrationAppendix(
  metadata: RoutingDraft | undefined,
  cwd = process.cwd(),
  env: NodeJS.ProcessEnv = process.env,
): { appendix: string; evidence: HarnessCompositionEvidence } {
  if (!metadata || Object.keys(metadata).length === 0) return { appendix: "", evidence: {} };
  // Axis-only appendix composition remains useful for native/prompt tests, but
  // selecting a managed role is an execution-grade act and therefore admits
  // only the complete request before any authority prompt is constructed.
  const admitted = metadata.role || metadata.composition
    ? admitRoutingRequest(metadata, "Orchestration appendix")
    : undefined;
  const routing: RoutingDraft = admitted ?? metadata;
  const blocks: string[] = [];
  const evidence: HarnessCompositionEvidence = {};
  if (admitted) {
    const composition = admitted.composition;
    if (composition.id !== admitted.role)
      throw new Error(`Orchestration composition ${composition.id} does not match role ${admitted.role}`);
    if (composition.kind === "template") {
      const role = exactSectionFence(resolve(orchestrationDocs(env), "roles.md"), admitted.role, `role:${admitted.role}`);
      blocks.push(`## Orchestration role contract — template:${admitted.role}\n${role}`);
      if (composition.overrides.length) {
        blocks.push([
          "## Orchestration template override",
          `Axes changed: ${composition.overrides.join(", ")}.`,
          `Reason: ${composition.overrideReason}`,
        ].join("\n"));
        evidence.templateOverrides = [...composition.overrides];
        evidence.templateOverrideReasonHash = createHash("sha256")
          .update(composition.overrideReason!).digest("hex");
      }
    } else {
      blocks.push(`## Orchestration role contract — bespoke:${composition.id}\n${bespokeRoleBlock(admitted)}`);
      evidence.bespokeContractHash = bespokeContractFingerprint(composition.contract);
      evidence.bespokeContractFingerprintVersion = BESPOKE_FINGERPRINT_VERSION;
      evidence.bespokeContractFingerprintDomain = BESPOKE_FINGERPRINT_DOMAIN;
    }
    evidence.roleKind = composition.kind;
    evidence.roleId = composition.id;
    evidence.capabilities = composition.kind === "bespoke"
      ? canonicalOrchestrationCapabilities(composition.contract.capabilities)
      : orchestrationCapabilities(admitted);
    const comms = exactSectionFence(resolve(orchestrationDocs(env), "comms.md"), "universal", "comms:universal");
    blocks.push(`## Orchestration communication contract — universal\n${comms}`);
    evidence.commsContractHash = createHash("sha256").update(comms).digest("hex");
  }
  if (routing.taskGrade) {
    const block = exactSectionFence(
      resolve(orchestrationDocs(env), "task-grades.md"), routing.taskGrade, `task-grade:${routing.taskGrade}`,
    );
    blocks.push(`## Orchestration task grade — ${routing.taskGrade}\n${block}`);
    evidence.taskGrade = routing.taskGrade;
  }
  if (routing.domainRequirements?.length) {
    blocks.push(domainContextGate(routing.domainRequirements, cwd, env));
    evidence.domainRequirements = [...routing.domainRequirements];
  }
  if (routing.topology) {
    const block = exactSectionFence(
      resolve(orchestrationDocs(env), "topologies.md"), routing.topology, `topology:${routing.topology}`,
    );
    blocks.push(`## Orchestration topology — ${routing.topology}\n${block}`);
    evidence.topology = routing.topology;
  }
  if (routing.tier || routing.reasoning) {
    blocks.push([
      "## Orchestration capacity route",
      `Semantic tier: ${routing.tier ?? "unselected"}.`,
      `Reasoning: ${routing.reasoning ?? "unselected"}.`,
      "Capacity does not widen the role, grade, topology, or domain authority above.",
    ].join("\n"));
    evidence.tier = routing.tier;
    evidence.reasoning = routing.reasoning;
  }
  if (routing.posture) {
    const block = exactSectionFence(
      resolve(orchestrationDocs(env), "postures.md"), routing.posture, `posture:${routing.posture}`,
    );
    blocks.push(`## Orchestration posture — ${routing.posture}\n${block}`);
    evidence.posture = routing.posture;
  }
  return { appendix: blocks.length ? `\n\n${blocks.join("\n\n")}` : "", evidence };
}

function modelDeltaAppendix(provider?: ProviderId, model?: string, omitReason?: string): {
  appendix: string;
  evidence: ModelDeltaEvidence;
} {
  if (omitReason) return { appendix: "", evidence: { provider, model, kind: "omitted", reason: omitReason } };
  if (!provider || !model) return {
    appendix: "", evidence: { provider, model, kind: "omitted", reason: !provider ? "provider_unresolved" : "model_unresolved" },
  };
  const delta = resolveModelDelta(provider, model);
  if (delta.kind === "none") return {
    appendix: "", evidence: { provider, model, kind: "none", reason: delta.reason },
  };
  const block = exactFirstFence(delta.absolutePath!, `model-delta:${provider}:${model}`);
  return {
    appendix: `\n\n## Orchestration exact-model delta — ${provider}:${model}\n${block}`,
    evidence: { provider, model, kind: "calibrated", path: delta.path },
  };
}

/** Rebuild a harness prompt for an exact provider/model route; never inherit a stale delta. */
export function applyHarnessRoute(
  options: Options,
  provider: ProviderId,
  model?: string,
  effort?: Effort,
  availability?: {
    targetId: string;
    receipt?: ProviderModelAdmissionReceipt;
  },
): {
  options: Options;
  evidence?: HarnessCompositionEvidence;
} {
  const state = harnessComposition.get(options as object);
  if (!state) return { options };
  const sourceSeal = harnessAuthoritySeals.get(options as object);
  if (sourceSeal && !hasCanonicalHarnessAuthority(options, sourceSeal.provider))
    throw new Error("harness authority source mutated before route application");
  if (state.routingRequest
      && ((options as any).northRoutingRequest !== state.routingRequest
        || (options as any).northCapabilities !== state.capabilities
        || !hasCanonicalAuthoringHooks(options))) {
    throw new Error("harness composition root mutated before route application");
  }
  const concreteModel = resolveModelAlias(provider, model);
  const composed = composeSystemPrompt(state, provider, concreteModel);
  if (provider === "anthropic")
    assertCanonicalGlobalAgentsExactlyOnce(composed.prompt, state.environment);
  let modelAvailabilityRequired = false;
  if (provider === "anthropic" || provider === "openai") {
    if (state.exactModelPinned) modelAvailabilityRequired = true;
    else if (state.routingRequest?.tier && concreteModel) {
      try {
        const canonical = resolveTier(
          provider, state.routingRequest.tier, undefined, effort,
        ).model;
        modelAvailabilityRequired = concreteModel !== canonical;
      } catch {
        // A route that cannot be compared with its canonical row is never
        // silently treated as the canonical default.
        modelAvailabilityRequired = true;
      }
    }
  }
  const targetId = availability?.targetId
    ?? (options as any).northModelAvailability?.targetId
    ?? provider;
  const modelAvailability = deepFreeze({
    required: modelAvailabilityRequired,
    targetId,
    ...(concreteModel ? { model: concreteModel } : {}),
    ...(modelAvailabilityRequired && availability?.receipt
      ? { receipt: availability.receipt }
      : {}),
    observationPath: (options as any).northModelAvailability?.observationPath
      ?? providerModelObservationPath((options as any).env ?? process.env),
  } satisfies HarnessModelAvailabilityBinding);
  const next = {
    ...(state.routeBase ?? options),
    model: concreteModel ?? state.initialModel,
    effort: effort ?? state.initialEffort,
    systemPrompt: composed.prompt,
    northModelAvailability: modelAvailability,
  } as Options;
  harnessComposition.set(next as object, state);
  const renewActivity = harnessActivityRenewers.get(options as object);
  if (renewActivity) harnessActivityRenewers.set(next as object, renewActivity);
  inheritAuthoringHookSeal(options, next);
  const evidence = {
    ...state.evidence,
    modelDelta: composed.deltaEvidence,
    promptEconomics: composed.economics,
    promptReceipt: composed.receipt,
  };
  appliedEvidence.set(next as object, deepFreeze(evidence));
  sealHarnessAuthority(next, provider);
  return { options: next, evidence };
}

export function harnessRouteSeed(options: Options): { provider?: ProviderId; model?: string } | undefined {
  const state = harnessComposition.get(options as object);
  return state ? { provider: state.initialProvider, model: state.initialModel } : undefined;
}

export function harnessCompositionEvidence(options: Options): HarnessCompositionEvidence | undefined {
  return appliedEvidence.get(options as object) ?? harnessComposition.get(options as object)?.evidence;
}

/** Provider-neutral activity heartbeat used by both SDK and CLI adapters. */
export function renewHarnessPresence(options: Options): void {
  harnessActivityRenewers.get(options as object)?.();
}

/** Compatibility name for callers that only need role/posture blocks. */
export function praxisAppendix(_model?: string, role?: string, posture?: string): string {
  const blocks: string[] = [];
  if (role) blocks.push(`## Praxis — role: ${role}\n${exactSectionFence(
    resolve(orchestrationDocs(), "roles.md"), role, `role:${role}`,
  )}`);
  if (posture) blocks.push(`## Praxis — posture: ${posture}\n${exactSectionFence(
    resolve(orchestrationDocs(), "postures.md"), posture, `posture:${posture}`,
  )}`);
  return blocks.length ? `\n\n${blocks.join("\n\n")}` : "";
}

// SDK workers execute the catalog-owned adapters from the current immutable
// activation generation. Every adapter applies its own UnitId gate before owner
// behavior, so the activation document is the only hook authority.
//   Edit|Write|MultiEdit -> worktree, firn
//   Bash                 -> worktree, blind-stage, tripwire, firn, corpus-scan
// The worktree guard is on BOTH entrances because a write into a protected `main`
// checkout arrives as an Edit or as a shell command, and enforcement on one entrance
// is not enforcement. The blind-stage and corpus-scan guards read only
// tool_input.command and return early for any other tool_name, so they are Bash-only
// by construction.
// BASH_GUARDS vs WORKER_BASH_GUARDS differ ONLY by orchestration permission
// (agent-spawn-guard): repository layout and staging discipline bind every lane.
const EDIT_GUARDS = resolveManagedGuardChain([
  "launch-critical-worktree-guard.sh", "firn-system-policy",
]);
const BASH_GUARDS = resolveManagedGuardChain([
  "launch-critical-worktree-guard.sh", "git-blind-stage-guard.sh",
  "tripwire-guard.sh", "firn-system-policy", "corpus-scan-guard.sh",
  "session-kill-guard.sh",
]);
const WORKER_BASH_GUARDS = resolveManagedGuardChain([
  "agent-spawn-guard.sh",
  "launch-critical-worktree-guard.sh", "git-blind-stage-guard.sh",
  "tripwire-guard.sh", "firn-system-policy", "corpus-scan-guard.sh",
  "session-kill-guard.sh",
]);

function receiptFileArtifact(id: string, path: string): EnvironmentArtifact {
  try {
    const info = statSync(path);
    if (!info.isFile()) return { id, coverage: "unknown" };
    return { id, sha256: sha256Bytes(readFileSync(path)), coverage: "exact" };
  } catch {
    return { id, coverage: "unknown" };
  }
}

function harnessEnvironmentReceipt(args: {
  env: NodeJS.ProcessEnv;
  cwd: string;
  allowedTools: readonly string[];
  disallowedTools: readonly string[];
  routingMetadata?: RoutingRequest;
  activatedResources?: readonly EnvironmentArtifact[];
  availableSkills?: readonly EnvironmentArtifact[];
  skillCatalog: ActiveSkillCatalog;
}): EnvironmentReceipt {
  const toolNames = [...args.allowedTools.map((name) => `allow:${name}`),
    ...args.disallowedTools.map((name) => `deny:${name}`)].sort();
  const tools = toolNames.map((name, index) => ({
    id: `tool-${index}`, sha256: sha256Bytes(name), coverage: "exact" as const,
  }));
  const global = canonicalGlobalAgents(args.env);
  const project = projectAgentsAppendix(args.cwd, args.env);
  return buildEnvironmentReceipt({
    availableSkills: args.availableSkills ?? args.skillCatalog.candidates.map((candidate) => ({
      id: `skill:${candidate.name}`,
      sha256: sha256Bytes(JSON.stringify(candidate)),
      coverage: "exact" as const,
    })),
    activatedResources: args.activatedResources ?? [
      { id: "activated-resource-observation", coverage: "unknown" },
    ],
    tools,
    hooks: [...EDIT_GUARDS, ...WORKER_BASH_GUARDS]
      .filter((path, index, values) => values.indexOf(path) === index)
      .map((path, index) => receiptFileArtifact(`hook-${index}`, path)),
    configs: [
      {
        id: "routing-request", coverage: "exact",
        sha256: sha256Bytes(JSON.stringify(args.routingMetadata ?? {})),
      },
      receiptFileArtifact(
        "learning-policy",
        args.env.NORTH_LEARNING_POLICY
          ?? resolve(args.env.HOME ?? "", ".config/north/learning-policy.json"),
      ),
    ],
    executables: [
      receiptFileArtifact("north-executable", ENGINE),
      receiptFileArtifact("north-mcp-executable", MCP),
      receiptFileArtifact(
        "babashka-executable",
        currentPathExecutable(args.env.NORTH_PEER_BB ?? "bb", args.env),
      ),
    ],
    instructions: [
      ...(global ? [{
        id: "global-instructions", sha256: sha256Bytes(global.bytes), coverage: "exact" as const,
      }] : []),
      {
        id: "project-instructions", sha256: sha256Bytes(project), coverage: "exact" as const,
      },
    ],
    coverageReason: args.activatedResources
      ? undefined : "activated-resource-observation-unavailable",
  });
}

// One matcher's callback: run its guard chain (first deny wins) over the hook input,
// translate to HookJSONOutput. A deny blocks THIS tool call (permissionDecision:deny)
// but does NOT halt the agent (`continue` stays default-true) — the worker sees the
// reason and can correct the command, exactly like the interactive deny.
async function guardHook(self: string, scripts: string[], input: unknown, topology?: Topology) {
  const env = topology ? { ...process.env, AGENT_TOPOLOGY: topology } : process.env;
  const deny = (reason: string) => {
    recordDenial(self, reason, input);
    return {
      hookSpecificOutput: {
        hookEventName: "PreToolUse" as const,
        permissionDecision: "deny" as const,
        permissionDecisionReason: reason,
      },
    };
  };
  try {
    const d = await evaluateGuards(scripts, input, 10000, env);
    if (d.decision === "deny") {
      // Durable trail: record the denial as a `kind guard_denial` fact so a worker
      // block is learnable after the fact (which agent, which guard, what target).
      // Fire-and-forget — never delay or break the tool call the guard decided.
      return deny(d.reason);
    }
  } catch { return deny("authoring_guard_unavailable"); }
  return { continue: true };
}

export function harnessOptions(o: HarnessOpts): Options {
  const cwd = o.cwd ?? process.cwd();
  // Freeze every composer-visible ambient input once. Prompt branches below
  // consume this snapshot; mid-assembly env mutation cannot alter the wire.
  const composerEnvironment = Object.freeze({ ...process.env }) as NodeJS.ProcessEnv;
  const metadata = o.routingMetadata
    ? admitRoutingRequest(o.routingMetadata, "managed North harness")
    : undefined;
  const effectiveEffort = metadata?.reasoning;
  const effectiveModel = o.provider && metadata
    ? resolveTier(o.provider, metadata.tier, o.model, effectiveEffort).model
    : o.model;
  const topology = metadata?.topology;
  const orchestration = orchestrationAppendix(metadata, cwd, composerEnvironment);
  const capabilities = orchestration.evidence.capabilities;
  const skillCatalog: ActiveSkillCatalog = o.dataOnly
    ? Object.freeze({
      root: domainSkillsDir(composerEnvironment), candidates: Object.freeze([]), appendix: "",
    })
    : activeSkillCatalog(composerEnvironment);
  // Shared head: DEFAULT (or override) + ESO. The canonical bootstrap, skill
  // catalog, orchestration contracts, project instructions, and unique tail are
  // composed from the immutable state below.
  const basePrompt = (o.systemPrompt ?? DEFAULT_SYSTEM_PROMPT)
    + (o.dataOnly ? "" : esoAppendix(composerEnvironment));
  // Orchestration is positive authority, never an ambient default. A lane with
  // no topology remains prompt-neutral but receives coordination-only tools.
  const orchestrationAllowed = topology === "orchestrator"
    && capabilities?.includes("coordination") === true;
  const capabilityPolicy = capabilities ? managedToolPolicy(capabilities) : undefined;
  const policy = o.dataOnly && capabilityPolicy
    ? {
      tools: [],
      allowedTools: [],
      disallowedTools: [...new Set([
        ...capabilityPolicy.allowedTools,
        ...capabilityPolicy.disallowedTools,
      ])],
    }
    : capabilityPolicy;
  const disallowedTools = policy?.disallowedTools ?? [...new Set([
    ...NATIVE_AGENT_TOOLS,
    ...(orchestrationAllowed ? [] : ORCHESTRATION_TOOLS),
  ])];
  const allowedTools = policy?.allowedTools ?? [...new Set([
    ...(o.extraTools ?? []).filter((name) => !disallowedTools.includes(name)),
    ...COORDINATION_TOOLS,
    ...(orchestrationAllowed ? ORCHESTRATION_TOOLS : []),
  ])];
  const enforcementTopology: Topology = orchestrationAllowed ? "orchestrator" : "worker";
  const {
    NORTH_DISPATCH_DRIVER_PRECLAIMED: _inheritedPreclaim,
    NORTH_RUN_ID: _inheritedRun,
    NORTH_THREAD_ID: _inheritedThread,
    NORTH_RUN_CAPABILITY: _inheritedCapability,
    NORTH_RUN_ARTIFACT_DIR: _inheritedArtifactDirectory,
    NORTH_MANAGED_LANE: _inheritedManagedLane,
    NORTH_ORCHESTRATION_ROLE: _inheritedOrchestrationRole,
    NORTH_CODEX_BIN: _inheritedCodexOverride,
    NORTH_BIN: _inheritedNorthBin,
    // Never let a parent's pinned dials leak into a child's bootstrap: every
    // managed spawn re-resolves model/effort from its Orchestration tier, but a
    // tier-less import.meta.main bootstrap reads process.env.AGENT_MODEL, so an
    // inherited value would silently pin the child. Strip them at the boundary.
    AGENT_MODEL: _inheritedModel,
    AGENT_REASONING: _inheritedReasoning,
    AGENT_TIER: _inheritedTier,
    ...ambientEnv
  } = process.env;
  const managedNorthBinDir = dirname(ENGINE);
  const childEnv = Object.freeze({
    ...ambientEnv,
    // Provider subprocesses and their shells share the same relocatable North
    // package authority as MCP. A system-generation `north` must never outrank
    // the package that admitted this lane.
    NORTH_BIN: ENGINE,
    PATH: ambientEnv.PATH
      ? `${managedNorthBinDir}${delimiter}${ambientEnv.PATH}`
      : managedNorthBinDir,
    AGENT_ID: o.self,
    AGENT_TOPOLOGY: enforcementTopology,
    ...(metadata?.reasoning ? { AGENT_REASONING: metadata.reasoning } : {}),
    ...(metadata?.role ? { NORTH_ORCHESTRATION_ROLE: metadata.role } : {}),
    // Sealed authority marker consumed by the system-managed Codex lifecycle
    // wrappers. Ambient callers cannot inherit or forge managed-lane behavior.
    NORTH_MANAGED_LANE: "1",
    ...(o.deliveryRun ? {
      NORTH_RUN_ID: o.deliveryRun.runId,
      NORTH_THREAD_ID: o.deliveryRun.threadId,
      NORTH_RUN_CAPABILITY: o.deliveryRun.capability,
    } : {}),
    // One explicit value feeds lane presence, provider CLI, North MCP, and
    // admission. Never let a later process ambient choose a different graph.
    NORTH_PORT: northPort(),
  });
  // An injected registrar denotes a hermetic boundary: never pair it with a
  // real graph renewer implicitly. Tests/adapters that want both injected
  // phases supply presenceRenewer explicitly. Production (both omitted) keeps
  // the real register + activity heartbeat pair.
  const presenceRenewer = o.presenceRenewer === false
    ? undefined
    : o.presenceRenewer ?? (o.presenceRegistrar === undefined ? renewPresence : undefined);
  const readonlyShell = capabilities?.includes("shell.readonly") === true;
  const northMcpEnv = Object.freeze(
    managedNorthMcpEnvironment({
      ...childEnv,
      NORTH_BIN: ENGINE,
      ...(o.artifactDirectory === undefined
        ? {}
        : { NORTH_RUN_ARTIFACT_DIR: o.artifactDirectory }),
    }),
  );
  const northMcpServer = o.dataOnly ? undefined : Object.freeze({
    type: "stdio", command: MCP,
    args: Object.freeze([]) as unknown as string[],
    env: northMcpEnv,
  });
  const mcpServers = Object.freeze({
    ...(northMcpServer ? { north: northMcpServer } : {}),
    ...(orchestrationAllowed && !o.dataOnly
      ? { "north-peer": Object.freeze(peerCommandServer(o.self)) }
      : {}),
    // Compile the minimum authority surface for every retry-safe route up
    // front. Codex ignores Claude SDK tool allowlists and independently
    // enforces --sandbox read-only; an Anthropic fallback must still inherit
    // denied native Bash plus North's isolated read-only shell.
    ...(readonlyShell && !o.dataOnly
      ? { [READONLY_SHELL_SERVER]: Object.freeze(
        readonlyShellServer(cwd, childEnv, o.abortController?.signal),
      ) }
      : {}),
  });
  const sealedTools = policy
    ? Object.freeze([...policy.tools]) as unknown as string[]
    : undefined;
  const sealedAllowedTools = Object.freeze([...allowedTools]) as unknown as string[];
  const sealedDisallowedTools = disallowedTools.length
    ? Object.freeze([...disallowedTools]) as unknown as string[]
    : undefined;
  const sealedSettingSources = policy
    ? Object.freeze([]) as unknown as NonNullable<Options["settingSources"]>
    : undefined;
  const environmentReceipt = harnessEnvironmentReceipt({
    env: composerEnvironment,
    cwd,
    allowedTools,
    disallowedTools,
    routingMetadata: metadata,
    activatedResources: o.activatedResources,
    availableSkills: o.availableSkills,
    skillCatalog,
  });
  // Prompt ingredients are rebuilt identically on every provider route from
  // this seed; routeBase and the sealed capability list attach afterward.
  const compositionSeed: HarnessCompositionState = {
    self: o.self,
    basePrompt,
    skillCatalog,
    orchestrationAppendix: orchestration.appendix,
    capabilities: capabilities ? [...capabilities] : undefined,
    cwd,
    evidence: { ...orchestration.evidence, capabilities, environmentReceipt },
    routingRequest: metadata,
    initialProvider: o.provider,
    initialModel: effectiveModel,
    initialEffort: effectiveEffort,
    omitModelDeltaReason: o.omitModelDeltaReason,
    // A direct managed caller that supplies a model is conservatively an exact
    // pin. Production explicitly supplies false for a canonical tier default.
    exactModelPinned: o.modelAvailability?.exactModelPinned ?? o.model !== undefined,
    dataOnly: o.dataOnly === true,
    environment: composerEnvironment,
  };
  const initialRouteModel = o.provider
    ? resolveModelAlias(o.provider, effectiveModel)
    : effectiveModel;
  const initialComposition = composeSystemPrompt(compositionSeed, o.provider, initialRouteModel);
  const initialSystemPrompt = initialComposition.prompt;
  if (!o.provider)
    assertCanonicalGlobalAgentsExactlyOnce(initialSystemPrompt, composerEnvironment);
  const options = {
    mcpServers,
    ...(policy ? {
      tools: sealedTools,
      settingSources: sealedSettingSources,
      strictMcpConfig: true,
    } : {}),
    allowedTools: sealedAllowedTools,
    ...(sealedDisallowedTools ? { disallowedTools: sealedDisallowedTools } : {}),
    model: o.provider ? resolveModelAlias(o.provider, effectiveModel) : effectiveModel,
    effort: effectiveEffort,
    env: childEnv,
    permissionMode: capabilities && !capabilities.includes("filesystem.write") ? "default" : "acceptEdits",
    ...(capabilities ? {
      northCapabilities: Object.freeze([...capabilities]) as unknown as OrchestrationCapability[],
    } : {}),
    ...(metadata ? { northRoutingRequest: metadata } : {}),
    ...(o.dataOnly ? { northDataOnly: true } : {}),
    ...(o.outputFormat ? { outputFormat: o.outputFormat } : {}),
    ...(o.persistSession === undefined ? {} : { persistSession: o.persistSession }),
    cwd,
    systemPrompt: initialSystemPrompt,
    maxTurns: o.maxTurns ?? (Number(process.env.AGENT_MAX_TURNS) || 200),
    ...(o.abortController ? { abortController: o.abortController } : {}),
    northModelAvailability: deepFreeze({
      required: false,
      targetId: o.modelAvailability?.targetId ?? o.provider ?? "unresolved",
      ...(initialRouteModel ? { model: initialRouteModel } : {}),
      observationPath: providerModelObservationPath(childEnv),
    } satisfies HarnessModelAvailabilityBinding),
    // Pin auto-compaction explicitly (audit fix 4). Managed lanes run with
    // settingSources: [], so the SDK's compaction behavior would otherwise ride a
    // silent library default that a future bump could flip. This pins the ENABLED
    // state (today's default) via the highest-priority flag-settings layer — not a
    // behavior change, an anti-drift lock; every compact_boundary is counted onto
    // @run and breadcrumbed on stderr by the spawn/dispatch stream loops.
    settings: { autoCompactEnabled: true },
    hooks: {
      // PreToolUse authoring-guard parity — the fix for worker edits running with
      // ZERO guards. Matchers +
      // guard chains mirror settings.json; first deny in a chain blocks the tool.
      PreToolUse: [
        { matcher: "Edit|Write|MultiEdit", hooks: [async (input: unknown) => guardHook(o.self, EDIT_GUARDS, input)] },
        { matcher: "Bash", hooks: [async (input: unknown) => guardHook(
          o.self, orchestrationAllowed ? BASH_GUARDS : WORKER_BASH_GUARDS, input, enforcementTopology,
        )] },
      ],
      // Presence heartbeat: renew the lease on tool activity (F2). Fire-and-forget +
      // never block/fail the tool call; always continue. PRESERVED exactly.
      PostToolUse: [{ hooks: [async () => {
        presenceRenewer?.(o.self);
        return { continue: true };
      }] }],
    },
  } as Options & { northCapabilities?: OrchestrationCapability[] };
  // Hooks are executable authority, not an advisory bag. Freeze the exact
  // harness-owned surface and make every routed provider rebuild from this
  // canonical root rather than spreading a caller-mutated retry object.
  deepFreeze((options as any).hooks);
  const state: HarnessCompositionState = {
    ...compositionSeed,
    capabilities: (options as any).northCapabilities ?? compositionSeed.capabilities,
    routeBase: Object.freeze({ ...options }) as Options,
  };
  harnessComposition.set(options as object, state);
  if (presenceRenewer)
    harnessActivityRenewers.set(options as object, () => presenceRenewer(o.self));
  appliedEvidence.set(options as object, deepFreeze({
    ...compositionSeed.evidence,
    modelDelta: initialComposition.deltaEvidence,
    promptEconomics: initialComposition.economics,
    promptReceipt: initialComposition.receipt,
  }));
  sealAuthoringHooks(options);
  // Presence is an assertion that a runnable lane exists. Every synchronous
  // prompt/bootstrap contract for the initial route must succeed first, or a
  // malformed AGENTS/Orchestration/model source would leave a ghost roster entry.
  const routedOptions = o.provider
    ? applyHarnessRoute(
        options, o.provider, effectiveModel, effectiveEffort,
        {
          targetId: o.modelAvailability?.targetId ?? o.provider,
          ...(o.modelAvailability?.receipt ? { receipt: o.modelAvailability.receipt } : {}),
        },
      ).options
    : options;
  if (o.presenceRegistrar !== false) (o.presenceRegistrar ?? registerPresence)(o.self, cwd);
  return routedOptions;
}

export const DEFAULT_SYSTEM_PROMPT =
  "You are a north agent on a shared coordination graph — recursive Triples with " +
  "assertion history. Prefer native north coordination " +
  "tools over editing coordination state: capture/tell to record work and ready/next " +
  "to find it. Your Orchestration topology contract, when present, is the sole source of " +
  "delegation authority. Acquire before editing shared code. Report concisely.";
