import type { Options } from "@anthropic-ai/claude-agent-sdk";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createHash } from "node:crypto";
import {
  chmodSync, closeSync, fsyncSync, lstatSync, mkdtempSync, openSync, readFileSync,
  realpathSync, renameSync, rmSync, statSync, writeSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  providerPreacceptError, ProviderRetrySafeError,
  type AgentProvider, type ProviderAvailability,
} from "./types";
import type { RoutingTarget } from "./types";
import { probeOpenAI } from "../provider-routing";
import { codexConfigArguments, providerEnvironmentForTarget } from "../accounts";
import type { OrchestrationCapability } from "../orchestration-capabilities";
import {
  unknownNativeCommandActivity,
  type NativeCommandActivityObservation,
} from "../native-command-activity";
import {
  admitExecution, admitPinnedProvider, consumeExecutionAdmission,
  managedNorthMcpEnvironment, validateManagedExecutionEnvelope,
} from "../execution-admission";
import {
  canonicalGlobalAgents, canonicalHarnessModelAvailability, GLOBAL_AGENTS_MAX_BYTES,
  hasCanonicalHarnessAuthority,
  renewHarnessPresence,
} from "../harness";
import { validateModelAdmissionReceipt } from "../provider-model-observation-store";
import { createExecutionActivityEmitter } from "../execution-activity";
import {
  CODEX_WORKER_NORTH_ENABLED_TOOLS, compileProviderAuthoritySurface,
  type OpenAIAuthoritySurface,
} from "./authority";
import { parseStrictJson, StrictJsonlFrames } from "../strict-json";
import { assertInstalledManagedCodexHooks } from "./codex-managed-hooks";
import {
  trustedGitProjectRoot, trustedManagedCodexExecutable,
} from "../trusted-runtime";
import {
  MANAGED_CODEX_DISABLED_FEATURES, MANAGED_CODEX_ENABLED_FEATURES,
  ManagedCodexAppServerRun, ManagedCodexHarvestError, ManagedCodexPreThreadError,
  type ManagedCodexAppServerOptions, type ManagedCodexInterruptEvidence,
} from "./codex-app-server";
import { managedCodexNetworkArguments } from "./codex-network-policy";
import { providerJoinEvidence } from "./provider-join";
import {
  prepareManagedCodexHome, type PreparedManagedCodexHome,
} from "./managed-codex-home";
import { CODEX_SUPERVISOR_STATUS_PREFIX } from "./codex-supervisor-protocol";
import { type McpActivityObservation, unknownMcpActivity } from "../tool-activity";
import {
  WireEventWriter,
  wireMessageId,
  wireModelCallId,
  wireToolCallId,
  type WireCompletionEvidence,
  type WireCompletionInterruptEvidence,
  type WireArtifactSink,
  type WireEvent,
  type WireEventDraft,
  type WireKnownEvent,
  type WireMessageId,
  type WireModelCallId,
  type WireQuery,
  type WireQueryInput,
  type WireQueryRoute,
  type WireToolCallId,
  type WireUsageSnapshot,
  type WireUserInputFrame,
} from "../wire";
import {
  OpenAIWireNormalizer,
  type OpenAIWireTurnSettlementInput,
} from "./openai-wire";

type ManagedCommandResolver = () => string;
type ManagedRunFactory = (options: ManagedCodexAppServerOptions) => ManagedCodexAppServerRun;

interface ManagedCodexLaunchReceipt {
  command: string;
  home: PreparedManagedCodexHome;
}

const managedLaunchReceipts = new WeakMap<object, ManagedCodexLaunchReceipt>();

function optionRecord(options: Options): Record<string, unknown> {
  return options as unknown as Record<string, unknown>;
}

function isManagedOpenAI(options: Options): boolean {
  return optionRecord(options).northCapabilities !== undefined;
}

function resolveManagedCommand(resolver: ManagedCommandResolver): string {
  try {
    const resolved = resolver();
    if (typeof resolved !== "string" || !resolved.trim())
      throw new Error("empty managed Codex executable");
    return resolved;
  } catch (cause) {
    throw new ProviderRetrySafeError(
      "openai_provider_executable_unavailable_before_acceptance", { cause },
    );
  }
}

function recordManagedLaunch(options: unknown, receipt: ManagedCodexLaunchReceipt): void {
  if ((typeof options !== "object" && typeof options !== "function") || options === null)
    throw new ProviderRetrySafeError("openai_managed_command_receipt_unavailable");
  managedLaunchReceipts.set(options as object, receipt);
}

function takeManagedLaunch(options: unknown): ManagedCodexLaunchReceipt | undefined {
  if ((typeof options !== "object" && typeof options !== "function") || options === null)
    return undefined;
  const key = options as object;
  const receipt = managedLaunchReceipts.get(key);
  managedLaunchReceipts.delete(key);
  return receipt;
}
const CODEX_SUPERVISOR = resolve(import.meta.dir, "codex-supervisor.ts");

/** Per-invocation Codex restrictions derived from the provider-neutral harness contract. */
export function codexHarnessArguments(options: Options): string[] {
  const denied = new Set(options.disallowedTools ?? []);
  const surface = !isManagedOpenAI(options)
    ? undefined
    : compileProviderAuthoritySurface("openai", options) as OpenAIAuthoritySurface;
  if (surface) return managedCodexAuthorityArguments(options, surface);
  const args: string[] = [];
  if (["Agent", "Task", "Workflow"].some((tool) => denied.has(tool))) {
    // North is the canonical managed spawn surface; native Codex subagents would
    // bypass per-child lifecycle, routing, budget, and telemetry contracts.
    args.push("--disable", "multi_agent");
  }
  if (denied.has("mcp__north__spawn") || denied.has("mcp__north__dispatch")) {
    args.push("--config", `mcp_servers.north.enabled_tools=${JSON.stringify(CODEX_WORKER_NORTH_ENABLED_TOOLS)}`);
  }
  return args;
}

function defaultCodexProjectRoot(cwd: string): string {
  return trustedGitProjectRoot(cwd);
}

function managedDeveloperInstructions(options: Options): string {
  if (typeof options.systemPrompt !== "string" || !options.systemPrompt.trim())
    throw new ProviderRetrySafeError("openai_developer_instructions_contract_missing");
  return options.systemPrompt;
}

function isMissing(error: unknown): boolean {
  return error instanceof Error && "code" in error && error.code === "ENOENT";
}

/**
 * Prove the selected Codex home will natively load the one canonical global
 * AGENTS source. Project instructions remain explicit developer instructions;
 * duplicating the global constitution there would give it two prompt entries.
 */
export function assertCodexGlobalAgentsForEnvironment(
  env: NodeJS.ProcessEnv,
  developerInstructions: string,
): void {
  // Codex has no supported switch that suppresses only CODEX_HOME/AGENTS.md:
  // project_doc_max_bytes=0 suppresses project discovery, not global guidance.
  // Pretending AGENT_LAWS=off worked would silently diverge from Anthropic.
  if (env.AGENT_LAWS === "off")
    throw new ProviderRetrySafeError("openai_agent_laws_opt_out_unenforceable");
  let canonical;
  try { canonical = canonicalGlobalAgents(env); }
  catch (cause) {
    throw new ProviderRetrySafeError("openai_canonical_global_agents_unavailable", { cause });
  }
  if (!canonical) return;

  const codexHome = env.CODEX_HOME?.trim();
  if (!codexHome)
    throw new ProviderRetrySafeError("openai_codex_home_missing");
  const target = resolve(codexHome, "AGENTS.md");
  const override = resolve(codexHome, "AGENTS.override.md");
  try {
    lstatSync(override);
    throw new ProviderRetrySafeError("openai_global_agents_override_present");
  } catch (error) {
    if (error instanceof ProviderRetrySafeError) throw error;
    if (!isMissing(error))
      throw new ProviderRetrySafeError("openai_global_agents_override_uninspectable", { cause: error });
  }

  let targetRealpath: string;
  let targetInfo;
  try {
    targetInfo = statSync(target);
    targetRealpath = realpathSync(target);
  } catch (cause) {
    throw new ProviderRetrySafeError("openai_target_global_agents_unavailable", { cause });
  }
  if (!targetInfo.isFile())
    throw new ProviderRetrySafeError("openai_target_global_agents_not_regular_file");
  let targetBytes: Buffer;
  try { targetBytes = readFileSync(target); }
  catch (cause) {
    throw new ProviderRetrySafeError("openai_target_global_agents_unavailable", { cause });
  }
  if (targetInfo.size > GLOBAL_AGENTS_MAX_BYTES || targetBytes.byteLength > GLOBAL_AGENTS_MAX_BYTES)
    throw new ProviderRetrySafeError("openai_target_global_agents_oversized");
  try { new TextDecoder("utf-8", { fatal: true }).decode(targetBytes); }
  catch (cause) {
    throw new ProviderRetrySafeError("openai_target_global_agents_invalid_utf8", { cause });
  }
  if (targetRealpath !== canonical.realpath || !targetBytes.equals(canonical.bytes))
    throw new ProviderRetrySafeError("openai_target_global_agents_not_canonical");
  if (developerInstructions.includes(canonical.text.trim()))
    throw new ProviderRetrySafeError("openai_global_agents_duplicated_in_developer_instructions");
}

function managedCodexTargetEnvironment(
  options: Options,
  target: RoutingTarget | undefined,
): PreparedManagedCodexHome {
  let accountEnv: NodeJS.ProcessEnv;
  try {
    accountEnv = providerEnvironmentForTarget("openai", target, { env: options.env });
  } catch (cause) {
    throw new ProviderRetrySafeError("openai_target_environment_invalid", { cause });
  }
  const developerInstructions = managedDeveloperInstructions(options);
  assertCodexGlobalAgentsForEnvironment(accountEnv, developerInstructions);
  let prepared: PreparedManagedCodexHome;
  try { prepared = prepareManagedCodexHome(accountEnv); }
  catch (cause) {
    throw new ProviderRetrySafeError("openai_managed_home_preparation_failed", { cause });
  }
  try {
    assertCodexGlobalAgentsForEnvironment(prepared.env, developerInstructions);
    return prepared;
  } catch (error) {
    prepared.dispose();
    throw error;
  }
}

function managedCodexAuthorityArguments(
  options: Options,
  surface: OpenAIAuthoritySurface,
): string[] {
  // This helper is also exported indirectly through codexHarnessArguments, so
  // retain the same fail-closed envelope check as the executable adapter.
  if (!hasCanonicalHarnessAuthority(options, "openai"))
    throw new ProviderRetrySafeError("openai_harness_authority_seal_missing");
  validateManagedExecutionEnvelope("openai", [...surface.capabilities], options);
  admitPinnedProvider("openai", surface.capabilities);
  managedDeveloperInstructions(options);
  // The executable managed adapter builds the complete session layer only
  // after resolving the selected account. This exported preview intentionally
  // exposes just the closed feature contract; exec-only ignore flags are not a
  // valid app-server authority boundary.
  return [
    ...MANAGED_CODEX_ENABLED_FEATURES.flatMap((name) => ["--enable", name]),
    ...managedCodexNetworkArguments(surface),
    ...MANAGED_CODEX_DISABLED_FEATURES.flatMap((name) => ["--disable", name]),
  ];
}

export function codexGlobalArguments(options: Options): string[] {
  void options;
  return [];
}

export function probeCodex(target?: RoutingTarget): ProviderAvailability {
  return probeOpenAI(target);
}

function validateOpenAIHarness(options: Options): OrchestrationCapability[] | undefined {
  if (!isManagedOpenAI(options)) return undefined;
  if (!hasCanonicalHarnessAuthority(options, "openai"))
    throw new ProviderRetrySafeError("openai_harness_authority_seal_missing");
  const surface = compileProviderAuthoritySurface("openai", options) as OpenAIAuthoritySurface;
  const capabilities = [...surface.capabilities];
  validateManagedExecutionEnvelope("openai", capabilities, options);
  admitPinnedProvider("openai", capabilities);
  managedDeveloperInstructions(options);
  if (surface.sandbox === "workspace-write") {
    let cwd: string;
    let projectRoot: string;
    try {
      cwd = realpathSync(options?.cwd ?? process.cwd());
      projectRoot = defaultCodexProjectRoot(cwd);
    } catch (cause) {
      throw new ProviderRetrySafeError("openai_write_workspace_identity_unavailable", { cause });
    }
    // Codex's unified-exec hook intentionally omits a per-call workdir. The
    // authoring invariants therefore rely on the other half of the executable
    // boundary too: workspace-write has no --add-dir, and the admitted cwd is
    // the canonical project root. A client project root appears in common hook
    // cwd; a non-client root cannot sandbox-write a client checkout.
    if (cwd !== projectRoot)
      throw new ProviderRetrySafeError("openai_write_workspace_must_be_project_root");
  }
  return capabilities;
}

async function validateOpenAIModelAdmission(
  options: Options,
  target: RoutingTarget | undefined,
): Promise<void> {
  const modelAvailability = canonicalHarnessModelAvailability(options, "openai");
  if (!modelAvailability)
    throw providerPreacceptError("openai_model_availability_authority_missing");
  if (!modelAvailability.required) return;
  if (!target || modelAvailability.targetId !== target.id
      || modelAvailability.model !== options.model
      || typeof options.model !== "string"
      || !await validateModelAdmissionReceipt(
        modelAvailability.receipt,
        target,
        options.model,
        modelAvailability.observationPath,
      )) {
    throw providerPreacceptError("openai_model_availability_unproven");
  }
}

type ManagedHooksProbe = () => void;

async function admitOpenAIWithManagedHooksProbe(
  options: Options,
  target: RoutingTarget | undefined,
  assertManagedHooks: ManagedHooksProbe,
  resolveCommand: ManagedCommandResolver = trustedManagedCodexExecutable,
): Promise<void> {
  const capabilities = validateOpenAIHarness(options);
  if (!capabilities) return;
  assertManagedHooks();
  // Resolve every fallible target-local prerequisite before admission can
  // publish a route or construct a provider query. The one-use executable
  // receipt closes the async admit -> synchronous query seam without repeating
  // trusted-path resolution after onRoute.
  const resolvedCommand = resolveManagedCommand(resolveCommand);
  const prepared = managedCodexTargetEnvironment(options, target);
  try {
    await validateOpenAIModelAdmission(options, target);
    await admitExecution("openai", capabilities, options?.cwd ?? process.cwd(), options, target);
    recordManagedLaunch(options, { command: resolvedCommand, home: prepared });
  } catch (error) {
    prepared.dispose();
    throw error;
  }
}

export async function admitOpenAI(options: Options, target?: RoutingTarget): Promise<void> {
  await admitOpenAIWithManagedHooksProbe(
    options, target, assertInstalledManagedCodexHooks,
  );
}

function frameText(frame: WireUserInputFrame): string {
  if (frame.kind !== "user.input" || typeof frame.text !== "string") {
    throw new TypeError("OpenAI input frame is invalid");
  }
  return frame.text;
}

async function initialPrompt(value: WireQueryInput): Promise<string> {
  if (typeof value === "string") return value;
  const it = value[Symbol.asyncIterator]();
  try {
    const first = await it.next();
    if (first.done) return "";
    return frameText(first.value);
  } finally {
    try { await it.return?.(); } catch { /* provider teardown owns the terminal error */ }
  }
}

// A persistent view over the streamed North input: `first()` is the launch
// prompt, `next()` yields each LATER frame the orchestrator loop pushes on the
// same provider thread (or `undefined` when the channel closes). A string
// prompt is single-turn. Unlike `initialPrompt`, the iterator is NOT closed
// after the first frame — continuation depends on pulling later frames.
interface PromptFrames {
  readonly streaming: boolean;
  first(): Promise<string>;
  next(): Promise<string | undefined>;
  close(): Promise<void>;
}

function promptFrames(value: WireQueryInput): PromptFrames {
  if (typeof value === "string") {
    return {
      streaming: false,
      async first() { return value; },
      async next() { return undefined; },
      async close() { /* no iterator to release */ },
    };
  }
  const it = value[Symbol.asyncIterator]();
  let done = false;
  const pull = async (): Promise<string | undefined> => {
    if (done) return undefined;
    const frame = await it.next();
    if (frame.done) { done = true; return undefined; }
    return frameText(frame.value);
  };
  return {
    streaming: true,
    async first() { return (await pull()) ?? ""; },
    async next() { return pull(); },
    async close() {
      if (done) return;
      done = true;
      try { await it.return?.(); } catch { /* provider teardown owns the terminal error */ }
    },
  };
}

function modelForCodex(model?: string): string | undefined {
  // Anthropic aliases have no valid cross-provider meaning. An explicit OpenAI
  // model is honored; semantic/default aliases defer to the user's Codex config.
  if (!model || /^(sonnet|opus|haiku|fable|economy|standard|senior|frontier)/.test(model)) return undefined;
  return model;
}

const CODEX_SUPERVISOR_GRACE_MS = 1_750;
const CODEX_SUPERVISOR_KILL_MS = 750;
const CODEX_PROMPT_HEADER = "NORTH_CODEX_PROMPT ";
const CODEX_PROMPT_MAX_BYTES = 16 * 1024 * 1024;
const CODEX_SUPERVISOR_STATUS_MAX_BYTES = 4 * 1024;
const CODEX_SUPERVISOR_STATUS_MAX_FRAMES = 4;

function supervisorExited(child: ChildProcessWithoutNullStreams): boolean {
  // An async spawn failure has no pid and emits `error`, not `exit`.
  return child.pid === undefined || child.exitCode !== null || child.signalCode !== null;
}

function waitForExitBounded(
  child: ChildProcessWithoutNullStreams,
  timeoutMs: number,
): Promise<boolean> {
  if (supervisorExited(child)) return Promise.resolve(true);
  const settled = Promise.withResolvers<boolean>();
  let finished = false;
  let timer: NodeJS.Timeout | undefined;
  const finish = (exited: boolean): void => {
    if (finished) return;
    finished = true;
    if (timer) clearTimeout(timer);
    child.off("exit", onExit);
    settled.resolve(exited);
  };
  const onExit = (): void => finish(true);
  // Listen first, then re-check state to close the exit-before-listener race.
  child.once("exit", onExit);
  if (supervisorExited(child)) finish(true);
  else timer = setTimeout(() => finish(supervisorExited(child)), timeoutMs);
  return settled.promise;
}

function closeSupervisorControl(child: ChildProcessWithoutNullStreams): void {
  try { child.stdin.end(); } catch { /* already closed */ }
}

function destroySupervisorControl(child: ChildProcessWithoutNullStreams): void {
  try { child.stdin.destroy(); } catch { /* already closed */ }
}

interface DestroyableWritable extends NodeJS.WritableStream {
  destroy(): void;
}

function destroyCodexPipes(child: ChildProcessWithoutNullStreams): void {
  try { child.stdin.destroy(); } catch { /* already closed */ }
  try { child.stdout.destroy(); } catch { /* already closed */ }
  try { child.stderr.destroy(); } catch { /* already closed */ }
  // POSIX never gives Bun a numeric prompt descriptor: the supervisor consumes
  // a private spool and FIFO. Only Windows creates an owned fd-4 pipe.
  if (process.platform === "win32") {
    const prompt = (child.stdio as unknown as Array<DestroyableWritable | null>)[4];
    try { prompt?.destroy(); } catch { /* already closed */ }
  }
  destroySupervisorControl(child);
}

async function terminateCodexProcessTree(child: ChildProcessWithoutNullStreams): Promise<void> {
  // Closing supervisor stdin asks it to terminate and reap the complete Codex
  // process group. Stdin is only the liveness lease; prompt delivery is an
  // independent bounded channel. The kernel therefore generates the same EOF
  // if North is SIGKILLed, so cleanup does not depend on a live Bun callback.
  closeSupervisorControl(child);
  if (!await waitForExitBounded(child, CODEX_SUPERVISOR_GRACE_MS)) {
    try { child.kill("SIGTERM"); } catch { /* already gone */ }
    if (!await waitForExitBounded(child, CODEX_SUPERVISOR_KILL_MS)) {
      try { child.kill("SIGKILL"); } catch { /* already gone */ }
      await waitForExitBounded(child, CODEX_SUPERVISOR_KILL_MS);
    }
  }
  destroyCodexPipes(child);
}

interface SupervisorObservation {
  started: Promise<"started" | "unavailable">;
  completed: Promise<number>;
}

function observeSupervisor(
  child: ChildProcessWithoutNullStreams,
): SupervisorObservation {
  const status = child.stderr;
  let startedSettled = false;
  const startedDeferred = Promise.withResolvers<"started" | "unavailable">();
  const started = startedDeferred.promise;
  const settleStarted = (value: "started" | "unavailable") => {
    if (startedSettled) throw new Error("openai_provider_execution_failed", {
      cause: new Error("Codex supervisor status emitted more than one start receipt"),
    });
    startedSettled = true;
    startedDeferred.resolve(value);
  };
  const completed = (async (): Promise<number> => {
    if (!status) throw new Error("openai_provider_execution_failed", {
      cause: new Error("Codex supervisor stderr status channel is unavailable"),
    });
    const frames = new StrictJsonlFrames({
      label: "Codex supervisor status",
      maxLineBytes: CODEX_SUPERVISOR_STATUS_MAX_BYTES,
      maxTotalBytes: CODEX_SUPERVISOR_STATUS_MAX_BYTES,
      maxFrames: CODEX_SUPERVISOR_STATUS_MAX_FRAMES,
    });
    let unavailable = false;
    for await (const chunk of status) {
      for (const line of frames.push(
        Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk),
      )) {
        const statusLine = line.startsWith(CODEX_SUPERVISOR_STATUS_PREFIX)
          ? line.slice(CODEX_SUPERVISOR_STATUS_PREFIX.length)
          : undefined;
        if (statusLine === "STARTED") {
          if (unavailable) throw new Error("openai_provider_execution_failed", {
            cause: new Error("Codex supervisor emitted STARTED after UNAVAILABLE"),
          });
          settleStarted("started");
          continue;
        }
        if (statusLine === "UNAVAILABLE") {
          if (startedSettled || unavailable)
            throw new Error("openai_provider_execution_failed", {
              cause: new Error("Codex supervisor emitted duplicate or late UNAVAILABLE receipt"),
            });
          unavailable = true;
          settleStarted("unavailable");
          continue;
        }
        const exit = statusLine === undefined
          ? null
          : /^EXIT (0|[1-9][0-9]{0,2})$/.exec(statusLine);
        const code = exit ? Number(exit[1]) : NaN;
        if (!Number.isInteger(code) || code > 255 || !startedSettled)
          throw new Error("openai_provider_execution_failed", {
            cause: new Error(
              `Codex supervisor emitted invalid exit receipt (started=${String(startedSettled)})`,
            ),
          });
        return code;
      }
    }
    frames.finish();
    throw new Error("openai_provider_execution_failed", {
      cause: new Error("Codex supervisor status channel closed without an EXIT receipt"),
    });
  })();
  void completed.catch((error) => {
    if (!startedSettled) {
      startedSettled = true;
      startedDeferred.reject(error);
    }
  });
  return { started, completed };
}

function supervisorPromptFrame(prompt: string): Buffer {
  const bytes = Buffer.from(prompt, "utf8");
  if (bytes.byteLength > CODEX_PROMPT_MAX_BYTES)
    throw new Error("openai_provider_execution_failed", {
      cause: new Error(`Codex supervisor prompt frame exceeds ${CODEX_PROMPT_MAX_BYTES} bytes`),
    });
  return Buffer.concat([
    Buffer.from(`${CODEX_PROMPT_HEADER}${bytes.byteLength}\n`, "utf8"),
    bytes,
  ]);
}

interface SupervisorPromptTransport {
  supervisorArguments: readonly string[];
  fd4: "pipe" | undefined;
  send(child: ChildProcessWithoutNullStreams): Promise<void>;
  abort(): void;
}

function supervisorPromptTransport(prompt: string): SupervisorPromptTransport {
  const promptBytes = Buffer.from(prompt, "utf8");
  if (promptBytes.byteLength > CODEX_PROMPT_MAX_BYTES)
    throw new Error("openai_provider_execution_failed", {
      cause: new Error(`Codex supervisor prompt transport exceeds ${CODEX_PROMPT_MAX_BYTES} bytes`),
    });
  if (process.platform === "win32") return {
    supervisorArguments: [],
    fd4: "pipe",
    async send(child) {
      const frame = supervisorPromptFrame(prompt);
      const target = (child.stdio as unknown as Array<DestroyableWritable | null>)[4];
      if (!target) throw new Error("openai_provider_execution_failed", {
        cause: new Error("Codex supervisor prompt pipe fd 4 is unavailable"),
      });
      const write = Promise.withResolvers<void>();
      const onError = (error: Error) => write.reject(error);
        target.once("error", onError);
        target.end(frame, () => {
          target.removeListener("error", onError);
          write.resolve();
        });
      await write.promise;
    },
    abort() {},
  };

  // Bun corrupts later process stdio when a numeric descriptor is passed to a
  // nested child. Use the supervisor's bounded spool/FIFO path instead: argv
  // contains only an opaque private directory, never prompt content.
  const directory = mkdtempSync(join(tmpdir(), "north-codex-prompt-"));
  chmodSync(directory, 0o700);
  let active = true;
  const abort = () => {
    if (!active) return;
    active = false;
    try { rmSync(directory, { recursive: true, force: true }); } catch {}
  };
  return {
    supervisorArguments: ["--oneshot-spool", directory],
    fd4: undefined,
    async send() {
      if (!active) throw new Error("openai_provider_execution_failed", {
        cause: new Error("Codex supervisor one-shot prompt spool is no longer active"),
      });
      const digest = createHash("sha256").update(promptBytes).digest("hex");
      const frame = Buffer.concat([
        Buffer.from(`NORTH_CODEX_RPC 1 ${promptBytes.byteLength} ${digest}\n`, "ascii"),
        promptBytes,
      ]);
      const temporary = join(directory, `.000000000001.${process.pid}.tmp`);
      const request = join(directory, "000000000001.req");
      let fd: number | undefined;
      try {
        fd = openSync(temporary, "wx", 0o600);
        let offset = 0;
        while (offset < frame.byteLength)
          offset += writeSync(fd, frame, offset, frame.byteLength - offset);
        fsyncSync(fd);
        closeSync(fd);
        fd = undefined;
        renameSync(temporary, request);
      } catch (error) {
        try { if (fd !== undefined) closeSync(fd); } catch {}
        abort();
        throw error;
      }
    },
    abort,
  };
}

type JsonObject = Record<string, unknown>;
interface ExactCodexUsage {
  input_tokens: number;
  cached_input_tokens: number;
  cache_write_input_tokens: number;
  output_tokens: number;
  reasoning_output_tokens: number;
}

const CODEX_JSONL_MAX_LINE_BYTES = 1024 * 1024;
const CODEX_JSONL_MAX_TOTAL_BYTES = 16 * 1024 * 1024;
const CODEX_JSONL_MAX_EVENTS = 10_000;
const CODEX_ID_MAX_BYTES = 512;

function objectValue(value: unknown, label: string): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value))
    throw new Error(`${label} must be an object`);
  return value as JsonObject;
}

function exactKeys(value: JsonObject, expected: readonly string[], label: string): void {
  const actual = Object.keys(value).sort();
  const canonical = [...expected].sort();
  if (actual.length !== canonical.length
      || actual.some((key, index) => key !== canonical[index])) {
    throw new Error(`${label} has an unknown or missing field`);
  }
}

function boundedProtocolString(value: unknown, label: string, maxBytes = CODEX_ID_MAX_BYTES): string {
  if (typeof value !== "string" || !value || value !== value.trim()
      || Buffer.byteLength(value, "utf8") > maxBytes || /[\u0000-\u001f\u007f]/.test(value)) {
    throw new Error(`${label} must be a bounded canonical string`);
  }
  return value;
}

function protocolId(value: unknown, label: string): string {
  const id = boundedProtocolString(value, label);
  if (!/^[A-Za-z0-9._:-]+$/.test(id))
    throw new Error(`${label} must be a canonical protocol id`);
  return id;
}

interface ValidatedCodexItem {
  id: string;
  type: string;
  text?: string;
  status?: "in_progress" | "completed" | "failed";
}

function validateCodexItem(value: unknown): ValidatedCodexItem {
  const item = objectValue(value, "Codex item");
  const type = boundedProtocolString(item.type, "Codex item type");
  const id = protocolId(item.id, "Codex item id");
  // Item payloads are provider-incidental, not North authority. Keep them
  // strictly framed/parsed/bounded and require stable identity, but do not
  // freeze Codex's evolving command/MCP/web/todo payload union here. Only the
  // final agent text is consumed by North, so that field alone is typed.
  if (type === "agent_message") {
    if (typeof item.text !== "string")
      throw new Error("Codex agent-message text must be a string");
    return { id, type, text: item.text };
  }
  if (type === "error") {
    boundedProtocolString(item.message, "Codex error-item message", CODEX_JSONL_MAX_LINE_BYTES);
    return { id, type, status: "failed" };
  }
  if (item.status === undefined) return { id, type };
  if (item.status !== "in_progress" && item.status !== "completed" && item.status !== "failed") {
    throw new Error("Codex item status is invalid");
  }
  return { id, type, status: item.status };
}

function exactUsage(value: unknown): ExactCodexUsage {
  const usage = objectValue(value, "Codex terminal usage");
  const baseKeys = [
    "cached_input_tokens", "input_tokens", "output_tokens", "reasoning_output_tokens",
  ] as const;
  exactKeys(usage, usage.cache_write_input_tokens === undefined
    ? baseKeys : [...baseKeys, "cache_write_input_tokens"], "Codex terminal usage");
  const counter = (name: string): number => {
    const token = usage[name];
    if (typeof token !== "number" || !Number.isSafeInteger(token) || token < 0)
      throw new Error(`Codex terminal usage ${name} is invalid`);
    return token;
  };
  const inputTokens = counter("input_tokens");
  const cachedInputTokens = counter("cached_input_tokens");
  const cacheWriteInputTokens = usage.cache_write_input_tokens === undefined
    ? 0 : counter("cache_write_input_tokens");
  const outputTokens = counter("output_tokens");
  const reasoningOutputTokens = counter("reasoning_output_tokens");
  if (cachedInputTokens > inputTokens || reasoningOutputTokens > outputTokens
      || !Number.isSafeInteger(inputTokens + outputTokens)) {
    throw new Error("Codex terminal usage counters are incoherent");
  }
  return {
    input_tokens: inputTokens,
    cached_input_tokens: cachedInputTokens,
    cache_write_input_tokens: cacheWriteInputTokens,
    output_tokens: outputTokens,
    reasoning_output_tokens: reasoningOutputTokens,
  };
}

interface CodexProtocolResult {
  events: readonly WireKnownEvent[];
  activityKind?: string;
  failure?: Error;
}

type CodexExecOpenItem =
  | { category: "message"; type: "agent_message"; messageId: WireMessageId; text: string }
  | { category: "tool"; type: string; toolCallId: WireToolCallId }
  | { category: "ignored"; type: "reasoning" };

function codexExecToolName(type: string): string {
  if (type === "command_execution" || type === "commandExecution") return "command";
  if (type === "file_change" || type === "fileChange") return "file-change";
  if (type === "mcp_tool_call" || type === "mcpToolCall") return "mcp-tool";
  if (type === "web_search" || type === "webSearch") return "web-search";
  if (type === "todo_list" || type === "todoList") return "todo-list";
  if (type === "error") return "provider-error";
  return "provider-item";
}

class CodexExecProtocol {
  #phase: "thread" | "turn" | "running" | "completed" = "thread";
  #usage?: ExactCodexUsage;
  #threadId?: string;
  #modelCallId?: WireModelCallId;
  #openItems = new Map<string, CodexExecOpenItem>();
  #completedItems = new Set<string>();
  // Codex nests every tool/command/file-change item *inside* one turn; a turn
  // count is therefore never a tool-loop proxy (thread 019f9c36). This counts
  // completed non-agent_message items — the one honest, provider-internal
  // "how much did it actually do" signal `codex exec` exposes. It is NOT the
  // same quantity as Claude SDK's num_turns and must never be stored or
  // reported under that name.
  #toolItemCount = 0;
  readonly #writer: WireEventWriter;
  readonly #route: WireQueryRoute;

  constructor(
    writer: WireEventWriter,
    route: WireQueryRoute,
  ) {
    this.#writer = writer;
    this.#route = route;
  }

  #events(drafts: readonly WireEventDraft[]): readonly WireKnownEvent[] {
    return drafts.length ? this.#writer.appendAll(drafts) : Object.freeze([]);
  }

  #usageSnapshot(usage: ExactCodexUsage): WireUsageSnapshot {
    const contextTokens = usage.input_tokens + usage.output_tokens;
    if (this.#route.contextWindow !== undefined && contextTokens > this.#route.contextWindow) {
      throw new Error("Codex terminal usage exceeds its semantic context window");
    }
    return {
      lifetime: {
        inputTokens: usage.input_tokens,
        outputTokens: usage.output_tokens,
        cacheReadTokens: usage.cached_input_tokens,
        cacheWriteTokens: usage.cache_write_input_tokens,
        reasoningTokens: usage.reasoning_output_tokens,
        modelCalls: this.#writer.snapshot()!.usage.lifetime.modelCalls,
      },
      context: {
        tokens: contextTokens,
        ...(this.#route.contextWindow === undefined ? {} : { window: this.#route.contextWindow }),
      },
    };
  }

  #completionEvidence(failure?: string): WireCompletionEvidence {
    return {
      ...(this.#threadId === undefined ? {} : {
        providerJoin: providerJoinEvidence("openai", {
          sessionId: this.#threadId,
          sessionPersistence: "persisted",
        }),
      }),
      turns: {
        unit: "provider-turn",
        count: 1,
        toolItems: this.#toolItemCount,
        comparable: false,
      },
      ...(failure === undefined ? {} : { failure: { detail: failure } }),
    };
  }

  #settle(
    status: "failed" | "cancelled",
    origin: "provider" | "north",
    errorCode: string,
  ): readonly WireKnownEvent[] {
    if (this.#phase !== "running" || !this.#modelCallId) return Object.freeze([]);
    const drafts: WireEventDraft[] = [];
    for (const item of this.#openItems.values()) {
      if (item.category === "message") drafts.push({
        kind: "message.recorded",
        messageId: item.messageId,
        modelCallId: this.#modelCallId,
        stage: "completed",
        role: "assistant",
      });
      else if (item.category === "tool") drafts.push({
        kind: "tool.terminal",
        toolCallId: item.toolCallId,
        status: status === "cancelled" ? "cancelled" : "synthetic_failure",
        origin: "north",
        errorCode,
      });
    }
    const snapshot = this.#writer.snapshot()!;
    drafts.push({
      kind: "model-call.completed",
      modelCallId: this.#modelCallId,
      status,
      origin,
      usage: snapshot.usage,
      usageCoverage: "unavailable",
      errorCode,
      evidence: this.#completionEvidence(errorCode),
    });
    this.#openItems.clear();
    this.#phase = "completed";
    return this.#events(drafts);
  }

  accept(line: string): CodexProtocolResult {
    if (this.#phase === "completed")
      throw new Error("Codex emitted an event after its terminal");
    const event = objectValue(parseStrictJson(line, "Codex exec event", {
      maxBytes: CODEX_JSONL_MAX_LINE_BYTES,
      maxDepth: 64,
      maxNodes: 50_000,
    }), "Codex exec event");
    const type = event.type;
    if (type === "error") {
      exactKeys(event, ["message", "type"], "Codex error event");
      // Validated bounded + control-char-free, then thrown away: the provider's
      // own account of the failure died here (thread 019f9cec). The OUTER
      // message stays the stable classification — provider prose must never
      // become a machine reason — but the payload now rides the cause so the
      // lane log and the run fact can name what actually happened.
      const message = boundedProtocolString(
        event.message, "Codex error message", CODEX_JSONL_MAX_LINE_BYTES,
      );
      const failure = new Error("Codex emitted an unrecoverable error", {
        cause: new Error(`provider error event: ${message.slice(0, 600)}`),
      });
      return {
        events: this.#settle("failed", "provider", "provider_error_event"),
        failure,
      };
    }
    if (type === "thread.started") {
      if (this.#phase !== "thread") throw new Error("Codex thread start is out of order");
      exactKeys(event, ["thread_id", "type"], "Codex thread-start event");
      this.#threadId = protocolId(event.thread_id, "Codex thread id");
      this.#phase = "turn";
      return { events: Object.freeze([]) };
    }
    if (type === "turn.started") {
      if (this.#phase !== "turn") throw new Error("Codex turn start is out of order");
      exactKeys(event, ["type"], "Codex turn-start event");
      const modelCallId = wireModelCallId(`model-call:${crypto.randomUUID()}`);
      this.#modelCallId = modelCallId;
      this.#phase = "running";
      return {
        events: this.#events([{
          kind: "model-call.started",
          modelCallId,
          model: { ...this.#route.model, provider: "openai" },
          effort: this.#route.effort,
          attempt: this.#route.attempt,
        }]),
        activityKind: "provider.codex.turn.started",
      };
    }
    if (type === "turn.failed") {
      if (this.#phase !== "running") throw new Error("Codex turn failure is out of order");
      exactKeys(event, ["error", "type"], "Codex turn-failed event");
      const error = objectValue(event.error, "Codex turn failure");
      exactKeys(error, ["message"], "Codex turn failure");
      const message = boundedProtocolString(
        error.message, "Codex turn failure message", CODEX_JSONL_MAX_LINE_BYTES,
      );
      const failure = new Error("Codex turn failed", {
        cause: new Error(`provider turn failure: ${message.slice(0, 600)}`),
      });
      return {
        events: this.#settle("failed", "provider", "provider_turn_failed"),
        activityKind: "provider.codex.turn.completed",
        failure,
      };
    }
    if (type === "turn.completed") {
      if (this.#phase !== "running" || !this.#modelCallId) {
        throw new Error("Codex turn terminal is out of order");
      }
      if (this.#openItems.size) throw new Error("Codex turn completed with open item lifecycles");
      exactKeys(event, ["type", "usage"], "Codex turn-completed event");
      this.#usage = exactUsage(event.usage);
      const usage = this.#usageSnapshot(this.#usage);
      const events = this.#events([
        { kind: "run.progress", lifecycle: "running", progress: { usage } },
        {
          kind: "model-call.completed",
          modelCallId: this.#modelCallId,
          status: "succeeded",
          origin: "provider",
          usage,
          usageCoverage: "exact",
          evidence: this.#completionEvidence(),
        },
      ]);
      this.#phase = "completed";
      return {
        events,
        activityKind: "provider.codex.turn.completed",
      };
    }
    if (type === "item.started" || type === "item.updated" || type === "item.completed") {
      if (this.#phase !== "running" || !this.#modelCallId) {
        throw new Error("Codex item event is out of order");
      }
      exactKeys(event, ["item", "type"], "Codex item event");
      const item = validateCodexItem(event.item);
      const drafts: WireEventDraft[] = [];
      const admitItem = (emitInitialText: boolean): CodexExecOpenItem => {
        if (item.type === "agent_message") {
          const messageId = wireMessageId(`message:${crypto.randomUUID()}`);
          drafts.push({
            kind: "message.recorded",
            messageId,
            modelCallId: this.#modelCallId!,
            stage: "started",
            role: "assistant",
          });
          if (emitInitialText && item.text) drafts.push({
            kind: "message.recorded",
            messageId,
            modelCallId: this.#modelCallId!,
            stage: "delta",
            role: "assistant",
            content: item.text,
          });
          return {
            category: "message", type: item.type, messageId,
            text: emitInitialText ? item.text ?? "" : "",
          };
        }
        if (item.type === "reasoning") return { category: "ignored", type: "reasoning" };
        const toolCallId = wireToolCallId(`tool:${crypto.randomUUID()}`);
        drafts.push({
          kind: "tool.admitted",
          toolCallId,
          modelCallId: this.#modelCallId!,
          name: codexExecToolName(item.type),
          schema: {
            status: "unavailable",
            reason: "tool schema unavailable at normalization boundary",
          },
        });
        return { category: "tool", type: item.type, toolCallId };
      };
      if (type === "item.started") {
        if (this.#openItems.has(item.id) || this.#completedItems.has(item.id)) {
          throw new Error("Codex item started more than once");
        }
        if (item.status !== undefined && item.status !== "in_progress") {
          throw new Error("Codex started item has a terminal status");
        }
        this.#openItems.set(item.id, admitItem(true));
      } else {
        if (this.#completedItems.has(item.id)) {
          throw new Error("Codex item completed more than once");
        }
        let open = this.#openItems.get(item.id);
        if (!open) {
          if (type === "item.updated") {
            throw new Error("Codex item update has no open lifecycle");
          }
          // Codex intentionally omits item.started for final agent messages,
          // file changes, warnings, and some other one-shot items. Admit and
          // settle them atomically so the provider-neutral lifecycle remains exact.
          open = admitItem(false);
          this.#openItems.set(item.id, open);
        }
        if (open.type !== item.type) throw new Error("Codex item type changed during its lifecycle");
        if (open.category === "message") {
          const text = item.text ?? "";
          if (!text.startsWith(open.text)) {
            throw new Error("Codex agent message text regressed during its lifecycle");
          }
          const delta = text.slice(open.text.length);
          if (delta) drafts.push({
            kind: "message.recorded",
            messageId: open.messageId,
            modelCallId: this.#modelCallId,
            stage: "delta",
            role: "assistant",
            content: delta,
          });
          open.text = text;
          if (type === "item.completed") drafts.push({
            kind: "message.recorded",
            messageId: open.messageId,
            modelCallId: this.#modelCallId,
            stage: "completed",
            role: "assistant",
          });
        } else if (open.category === "tool") {
          if (type === "item.completed" && item.status === "in_progress") {
            throw new Error("Codex completed item is still in progress");
          }
          const failed = item.status === "failed";
          drafts.push(type === "item.completed" ? {
            kind: "tool.terminal",
            toolCallId: open.toolCallId,
            status: failed ? "failed" : "succeeded",
            origin: "provider",
            ...(failed ? { errorCode: "tool_failed" } : {}),
          } : {
            kind: "tool.progress",
            toolCallId: open.toolCallId,
            progress: { phase: "provider-update" },
          });
        }
        if (type === "item.completed") {
          this.#openItems.delete(item.id);
          this.#completedItems.add(item.id);
          if (item.type !== "agent_message" && item.type !== "reasoning") {
            this.#toolItemCount += 1;
          }
        }
      }
      return {
        events: this.#events(drafts),
        activityKind: type === "item.started"
          ? "provider.codex.item.started"
          : type === "item.updated"
            ? "provider.codex.item.updated"
            : "provider.codex.item.completed",
      };
    }
    throw new Error("Codex emitted an unknown event");
  }

  finish(): ExactCodexUsage {
    if (this.#phase !== "completed" || !this.#usage)
      throw new Error("Codex closed without one successful terminal");
    return this.#usage;
  }

  settleFailure(errorCode: string, cancelled: boolean): readonly WireKnownEvent[] {
    return this.#settle(
      cancelled ? "cancelled" : "failed",
      "north",
      errorCode,
    );
  }
}

function publicManagedInterruptCode(
  reason: ManagedCodexInterruptEvidence["reason"],
): WireCompletionInterruptEvidence["reason"] {
  if (reason === "turn_deadline") return "north_turn_deadline";
  if (reason === "post_tool_silence") return "north_post_tool_silence";
  return "north_in_flight_item_ceiling";
}

/** Build one privacy-bounded terminal witness from a managed Codex harvest. */
export function managedCodexHarvestEvidence(
  error: ManagedCodexHarvestError,
): WireCompletionEvidence {
  const harvest = error.harvest;
  const interrupt = harvest.interrupt;
  const failureCode = interrupt === undefined
    ? "provider_execution_failed"
    : publicManagedInterruptCode(interrupt.reason);
  return {
    providerJoin: providerJoinEvidence("openai", {
      sessionId: harvest.threadId,
      turnIds: harvest.turnIds,
      sessionPersistence: "ephemeral",
    }),
    turns: {
      unit: "provider-turn",
      count: 1,
      ...(harvest.toolItems === undefined ? {} : { toolItems: harvest.toolItems }),
      comparable: false,
    },
    failure: {
      detail: failureCode,
      landed: {
        completedTurns: harvest.completedTurns,
        ...(harvest.toolItems === undefined ? {} : { toolItems: harvest.toolItems }),
        ...(harvest.mcp.totalCalls === undefined ? {} : { mcpCalls: harvest.mcp.totalCalls }),
        ...(harvest.nativeCommands.totalCommands === undefined
          ? {} : { nativeCommands: harvest.nativeCommands.totalCommands }),
      },
    },
    ...(interrupt ? {
      interrupt: {
        reason: publicManagedInterruptCode(interrupt.reason),
        deadlineMs: interrupt.deadlineMs,
        inactivityThresholdMs: interrupt.inactivityThresholdMs,
        lastActivityAgeMs: interrupt.lastActivityAgeMs,
        openItemCount: interrupt.openItemCount,
        ...(interrupt.openItem === null ? {} : {
          openItem: {
            kind: codexExecToolName(interrupt.openItem.kind),
            ageMs: interrupt.openItem.ageMs,
          },
        }),
        eventCount: interrupt.eventCount,
      },
    } : {}),
  };
}

const MAX_PENDING_CODEX_CONTINUATIONS = 64;

class ManagedInputQueue {
  #values: string[] = [];
  #waiters: Array<PromiseWithResolvers<string | undefined>> = [];
  #closed = false;

  push(value: string): void {
    if (this.#closed) throw new Error("Codex continuation queue is closed");
    const waiter = this.#waiters.shift();
    if (waiter) {
      waiter.resolve(value);
      return;
    }
    if (this.#values.length >= MAX_PENDING_CODEX_CONTINUATIONS) {
      throw new Error("Codex continuation queue exceeded its bound");
    }
    this.#values.push(value);
  }

  next(): Promise<string | undefined> {
    const value = this.#values.shift();
    if (value !== undefined) return Promise.resolve(value);
    if (this.#closed) return Promise.resolve(undefined);
    const waiter = Promise.withResolvers<string | undefined>();
    this.#waiters.push(waiter);
    return waiter.promise;
  }

  close(): void {
    if (this.#closed) return;
    this.#closed = true;
    for (const waiter of this.#waiters.splice(0)) waiter.resolve(undefined);
  }
}

type ManagedInputResult =
  | { source: "frames"; value: string | undefined }
  | { source: "continuations"; value: string | undefined };

function mergedManagedInput(
  frames: PromptFrames,
  continuations: ManagedInputQueue,
): () => Promise<string | undefined> {
  let framesOpen = frames.streaming;
  let continuationsOpen = true;
  let framePull: Promise<ManagedInputResult> | undefined;
  let continuationPull: Promise<ManagedInputResult> | undefined;
  return async () => {
    while (framesOpen || continuationsOpen) {
      const candidates: Array<Promise<ManagedInputResult>> = [];
      if (framesOpen) {
        framePull ??= frames.next().then((value) => ({ source: "frames", value }));
        candidates.push(framePull);
      }
      if (continuationsOpen) {
        continuationPull ??= continuations.next()
          .then((value) => ({ source: "continuations", value }));
        candidates.push(continuationPull);
      }
      const result = await Promise.race(candidates);
      if (result.source === "frames") {
        framePull = undefined;
        if (result.value === undefined) {
          framesOpen = false;
          continue;
        }
      } else {
        continuationPull = undefined;
        if (result.value === undefined) {
          continuationsOpen = false;
          continue;
        }
      }
      return result.value;
    }
    return undefined;
  };
}

class CodexQuery implements WireQuery {
  readonly #activity = createExecutionActivityEmitter();
  readonly #providerEventListeners = new Set<(event: WireEvent) => void>();
  readonly #input: WireQueryInput;
  readonly #options: Options;
  readonly #writer: WireEventWriter;
  readonly #route: WireQueryRoute;
  readonly #artifacts: WireArtifactSink | undefined;
  readonly #target?: RoutingTarget;
  readonly #assertManagedHooks: ManagedHooksProbe;
  readonly #resolveManagedCommand: ManagedCommandResolver;
  readonly #createManagedRun: ManagedRunFactory;
  #admitted: boolean;
  #admittedManagedLaunch?: ManagedCodexLaunchReceipt;
  #child?: ChildProcessWithoutNullStreams;
  #managedRun?: ManagedCodexAppServerRun;
  #interruptPromise?: Promise<void>;
  #completedMcpActivity?: McpActivityObservation;
  #completedNativeCommandActivity?: NativeCommandActivityObservation;
  #managedEvents: WireKnownEvent[] = [];
  #managedNormalizer?: OpenAIWireNormalizer;
  #continuations?: ManagedInputQueue;
  #iterated = false;
  #closed = false;
  #interrupted = false;

  constructor(
    input: WireQueryInput,
    options: Options,
    writer: WireEventWriter,
    route: WireQueryRoute,
    artifacts: WireArtifactSink | undefined,
    target?: RoutingTarget,
    admitted = false,
    assertManagedHooks: ManagedHooksProbe = assertInstalledManagedCodexHooks,
    resolveManagedCommand: ManagedCommandResolver = trustedManagedCodexExecutable,
    admittedManagedLaunch?: ManagedCodexLaunchReceipt,
    createManagedRun: ManagedRunFactory = (options) => new ManagedCodexAppServerRun(options),
  ) {
    this.#input = input;
    this.#options = options;
    this.#writer = writer;
    this.#route = route;
    this.#artifacts = artifacts;
    this.#target = target;
    this.#admitted = admitted;
    this.#assertManagedHooks = assertManagedHooks;
    this.#resolveManagedCommand = resolveManagedCommand;
    this.#admittedManagedLaunch = admittedManagedLaunch;
    this.#createManagedRun = createManagedRun;
  }

  get executionTransport(): "managed-app-server" | "cli-jsonl" {
    return isManagedOpenAI(this.#options) ? "managed-app-server" : "cli-jsonl";
  }

  get executionActivity() {
    return this.#activity.source;
  }

  subscribeProviderEvents(listener: (event: WireEvent) => void): () => void {
    this.#providerEventListeners.add(listener);
    return () => { this.#providerEventListeners.delete(listener); };
  }

  #publishEvents(events: readonly WireKnownEvent[]): void {
    for (const event of events) {
      this.#managedEvents.push(event);
      this.#notifyEvent(event);
    }
  }

  #notifyEvent(event: WireEvent): void {
    for (const listener of this.#providerEventListeners) {
      try { listener(event); }
      catch { /* Presentation observers cannot change provider execution. */ }
    }
  }

  supportsInFlightEscalation(): boolean { return false; }

  mcpActivity(): McpActivityObservation {
    return this.#managedRun?.mcpActivity() ?? this.#completedMcpActivity
      ?? unknownMcpActivity(this.executionTransport === "cli-jsonl"
        ? "codex-cli:structured-mcp-unavailable" : "codex-app-server:unsettled");
  }

  nativeCommandActivity(): NativeCommandActivityObservation {
    return this.#managedRun?.nativeCommandActivity() ?? this.#completedNativeCommandActivity
      ?? unknownNativeCommandActivity(this.executionTransport === "cli-jsonl"
        ? "codex-cli:structured-command-unavailable" : "codex-app-server:unsettled");
  }

  async interruptTurn(): Promise<void> {
    const managedRun = this.#managedRun;
    if (!managedRun) throw new Error("provider_has_no_active_session");
    await managedRun.interruptTurn();
  }

  async interrupt(): Promise<void> {
    this.#interrupted = true;
    if (this.#interruptPromise) return this.#interruptPromise;
    const managedRun = this.#managedRun;
    if (managedRun) {
      const cleanup = managedRun.interrupt();
      this.#interruptPromise = cleanup;
      try { await cleanup; }
      finally {
        if (this.#interruptPromise === cleanup) this.#interruptPromise = undefined;
      }
      return;
    }
    const child = this.#child;
    if (!child) return;
    const cleanup = (async () => {
      // Always address the process group, even after the direct child exited.
      await terminateCodexProcessTree(child);
    })();
    this.#interruptPromise = cleanup;
    try { await cleanup; }
    finally {
      if (this.#interruptPromise === cleanup) this.#interruptPromise = undefined;
    }
  }

  async continueTurn(input: WireQueryInput): Promise<void> {
    if (this.executionTransport !== "managed-app-server") {
      throw new Error("Codex CLI does not retain a continuation thread");
    }
    if (this.#closed) throw new Error("Codex query is closed");
    const text = await initialPrompt(input);
    (this.#continuations ??= new ManagedInputQueue()).push(text);
  }

  async close(): Promise<void> {
    if (this.#closed) return;
    this.#closed = true;
    this.#continuations?.close();
    await this.interrupt();
  }

  async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
    if (this.#iterated) throw new Error("Codex query can only be iterated once");
    this.#iterated = true;
    const admitted = this.#admitted;
    this.#admitted = false;
    if (admitted) validateOpenAIHarness(this.#options);
    else await admitOpenAIWithManagedHooksProbe(
      this.#options, this.#target, this.#assertManagedHooks, this.#resolveManagedCommand,
    );
    const managed = isManagedOpenAI(this.#options);
    const managedLaunch = managed
      ? this.#admittedManagedLaunch ?? takeManagedLaunch(this.#options)
      : undefined;
    this.#admittedManagedLaunch = undefined;
    if (managed && !managedLaunch)
      throw new ProviderRetrySafeError("openai_managed_command_receipt_unavailable");
    if (managed) {
      let frames: PromptFrames | undefined;
      try {
        // Repeat both root-managed hook and pristine-home proof at the final
        // pre-spawn seam. The prepared home is the exact one admitted before
        // route publication; interactive account config is never consulted.
        this.#assertManagedHooks();
        assertCodexGlobalAgentsForEnvironment(
          managedLaunch!.home.env, managedDeveloperInstructions(this.#options),
        );
        const surface = compileProviderAuthoritySurface(
          "openai", this.#options,
        ) as OpenAIAuthoritySurface;
        const model = modelForCodex(this.#options.model);
        if (!model)
          throw new ProviderRetrySafeError("openai_exact_model_resolution_missing");
        const servers = objectValue(this.#options.mcpServers, "OpenAI MCP servers");
        const north = objectValue(servers.north, "OpenAI North MCP server");
        const northCommand = boundedProtocolString(north.command, "OpenAI North MCP command", 4_096);
        if (!Array.isArray(north.args)
            || !north.args.every((value) => typeof value === "string")) {
          throw new ProviderRetrySafeError("openai_north_mcp_arguments_invalid");
        }
        const northEnvironment = objectValue(
          north.env,
          "OpenAI North MCP environment",
        ) as unknown as NodeJS.ProcessEnv;
        // The launch prompt is the first North frame; later frames (an
        // orchestrator's post-settlement reduction directive, a live message) are
        // consumed as additional turns on the SAME provider thread. A string
        // prompt or a channel that closes after one frame stays single-turn.
        frames = promptFrames(this.#input);
        const launchPrompt = await frames.first();
        this.#continuations ??= new ManagedInputQueue();
        const normalizer = new OpenAIWireNormalizer({
          writer: this.#writer,
          route: this.#route,
          ...(this.#artifacts === undefined ? {} : { artifacts: this.#artifacts }),
        });
        this.#managedNormalizer = normalizer;
        // Admission can precede query construction and prompt acquisition.
        // Re-read the selected target's receipt at the final pre-launch seam so
        // a newer empty or failed model/list result cannot race into execution.
        await validateOpenAIModelAdmission(this.#options, this.#target);
        const run = this.#createManagedRun({
          command: managedLaunch!.command,
          env: managedLaunch!.home.env,
          cwd: this.#options.cwd ?? process.cwd(),
          prompt: launchPrompt,
          model,
          effort: this.#options.effort,
          developerInstructions: managedDeveloperInstructions(this.#options),
          surface,
          north: {
            command: northCommand,
            args: [...north.args],
            env: managedNorthMcpEnvironment(northEnvironment),
          },
          beforeLaunch: () => validateOpenAIModelAdmission(this.#options, this.#target),
          onActivity: (kind) => {
            this.#activity.record("provider", kind);
            renewHarnessPresence(this.#options);
          },
          onEvent: (method, params) => {
            const normalized = normalizer.normalize(method, params);
            this.#publishEvents(normalized.events);
          },
          onRespawn: () => {
            if (!normalizer.hasActiveTurn()) return;
            this.#publishEvents(normalizer.settleProviderRespawn().events);
          },
        });
        this.#managedRun = run;
        const nextInput = mergedManagedInput(frames, this.#continuations);
        for await (const completed of run.session(nextInput)) {
          const terminal = [...this.#managedEvents].reverse().find(
            (event) => event.kind === "model-call.completed",
          );
          const providerUsage = normalizer.lastCompletedProviderUsage();
          if (!terminal || terminal.kind !== "model-call.completed"
              || terminal.status !== "succeeded"
              || providerUsage === undefined
              || terminal.evidence?.providerDurationMs !== completed.providerDurationMs
              || terminal.evidence?.turns?.unit !== "provider-turn"
              || terminal.evidence.turns.toolItems !== completed.toolItems
              || JSON.stringify(terminal.evidence.providerJoin) !== JSON.stringify(completed.providerJoin)
              || providerUsage.inputTokens !== completed.usage.input_tokens
              || providerUsage.outputTokens !== completed.usage.output_tokens
              || providerUsage.cacheReadTokens !== completed.usage.cached_input_tokens
              || providerUsage.reasoningTokens !== completed.usage.reasoning_output_tokens) {
            throw new Error("managed Codex result diverged from normalized terminal evidence");
          }
          for (const event of this.#managedEvents.splice(0)) yield event;
        }
        return;
      } catch (error) {
        if (error instanceof ManagedCodexPreThreadError)
          throw new ProviderRetrySafeError(error.message, { cause: error });
        const normalizer = this.#managedNormalizer;
        if (normalizer?.hasActiveTurn()) {
          const interrupt = error instanceof ManagedCodexHarvestError
            ? error.harvest.interrupt : undefined;
          const errorCode = interrupt === undefined
            ? "provider_execution_failed"
            : publicManagedInterruptCode(interrupt.reason);
          const settlement: OpenAIWireTurnSettlementInput = {
            status: this.#interrupted || interrupt ? "cancelled" : "failed",
            origin: "north",
            errorCode,
            ...(error instanceof ManagedCodexHarvestError
              ? { evidence: managedCodexHarvestEvidence(error) }
              : { evidence: { failure: { detail: errorCode } } }),
          };
          this.#publishEvents(normalizer.settleTurn(settlement).events);
        }
        for (const event of this.#managedEvents.splice(0)) yield event;
        throw error;
      } finally {
        this.#continuations?.close();
        await frames?.close().catch(() => { /* teardown owns the terminal error */ });
        this.#completedMcpActivity = this.#managedRun?.mcpActivity();
        this.#completedNativeCommandActivity = this.#managedRun?.nativeCommandActivity();
        this.#managedRun = undefined;
        this.#managedNormalizer = undefined;
        this.#closed = true;
        managedLaunch!.home.dispose();
      }
    }
    const env = providerEnvironmentForTarget("openai", this.#target, { env: this.#options.env });
    const task = await initialPrompt(this.#input);
    const prompt = this.#options.systemPrompt
      ? `${this.#options.systemPrompt}\n\n## Task\n${task}`
      : task;
    const args = [
      ...codexGlobalArguments(this.#options),
      "exec", ...codexConfigArguments(env), ...codexHarnessArguments(this.#options),
      "--json", "--color", "never", "--skip-git-repo-check",
    ];
    const model = modelForCodex(this.#options.model);
    if (model) args.push("--model", model);
    if (this.#options.effort) args.push("--config", `model_reasoning_effort=${JSON.stringify(this.#options.effort)}`);
    if (this.#options.cwd) args.push("--cd", this.#options.cwd);
    args.push("-");
    const promptTransport = supervisorPromptTransport(prompt);
    const supervisorStdio: Array<"pipe" | "ignore" | number> = ["pipe", "pipe", "pipe"];
    if (promptTransport.fd4) supervisorStdio.push("ignore", promptTransport.fd4);
    let child: ChildProcessWithoutNullStreams;
    try {
      child = spawn(
        process.execPath,
        [
          CODEX_SUPERVISOR,
          ...promptTransport.supervisorArguments,
          env.NORTH_CODEX_BIN ?? "codex",
          ...args,
        ],
        {
          cwd: this.#options.cwd ?? process.cwd(),
          env,
          // fd 0 is a liveness lease: kernel EOF means the North host died.
          // POSIX prompt delivery uses the private one-shot spool; Windows fd 4
          // remains a dedicated pipe because its process transport differs.
          stdio: supervisorStdio,
          detached: false,
        },
      ) as unknown as ChildProcessWithoutNullStreams;
    } catch (error) {
      promptTransport.abort();
      throw error;
    }
    this.#child = child;
    child.stdin.on("error", () => { /* child process error is classified below */ });
    const supervision = observeSupervisor(child);
    let providerStarted = false;
    const frames = new StrictJsonlFrames({
      label: "Codex exec",
      maxLineBytes: CODEX_JSONL_MAX_LINE_BYTES,
      maxTotalBytes: CODEX_JSONL_MAX_TOTAL_BYTES,
      maxFrames: CODEX_JSONL_MAX_EVENTS,
    });
    const protocol = new CodexExecProtocol(this.#writer, this.#route);
    try {
      // Publish the bounded prompt frame immediately after the supervisor
      // exists. Waiting for the provider's STARTED receipt lets a valid
      // short-lived provider exit before its stdin becomes readable.
      let promptSendError: unknown;
      try { await promptTransport.send(child); }
      catch (error) { promptSendError = error; }
      const startStatus = await supervision.started;
      if (startStatus === "unavailable") {
        throw new ProviderRetrySafeError(
          "openai_provider_executable_unavailable_before_acceptance",
        );
      }
      if (promptSendError) throw promptSendError;
      providerStarted = true;
      for await (const chunk of child.stdout) {
        for (const line of frames.push(chunk)) {
          const accepted = protocol.accept(line);
          if (accepted.activityKind) {
            this.#activity.record("provider", accepted.activityKind);
            renewHarnessPresence(this.#options);
          }
          for (const event of accepted.events) {
            this.#notifyEvent(event);
            yield event;
          }
          if (accepted.failure) throw accepted.failure;
        }
      }
      frames.finish();
      const supervisorExit = await supervision.completed;
      if (supervisorExit !== 0)
        throw new Error("openai_provider_execution_failed", {
          cause: new Error(`Codex supervisor exited with status ${supervisorExit}`),
        });
      protocol.finish();
    } catch (error) {
      const terminalEvents = protocol.settleFailure(
        this.#interrupted ? "cancelled" : "provider_execution_failed",
        this.#interrupted,
      );
      for (const event of terminalEvents) {
        this.#notifyEvent(event);
        yield event;
      }
      try { await this.interrupt(); } catch { /* cleanup must not replace the provider error */ }
      try { await supervision.completed; } catch { /* preserve the provider error */ }
      if (error instanceof ProviderRetrySafeError && !providerStarted)
        throw error;
      // The classification-shaped local observation is NOT a substitute for the
      // error actually caught here (the protocol/parse/transport failure) — chain
      // both, generic first, so causeChain renders "what stage" then "what went
      // wrong", down to the provider payload the event carried.
      throw new Error("openai_provider_execution_failed", {
        cause: new Error(
          "Codex legacy supervisor execution failed while sending or parsing a provider turn",
          { cause: error },
        ),
      });
    } finally {
      promptTransport.abort();
      destroyCodexPipes(child);
      this.#child = undefined;
      this.#closed = true;
    }
  }
}

/**
 * @internal Hermetic test seam. Deliberately not exported by providers/index;
 * production remains closed over assertInstalledManagedCodexHooks below.
 */
export interface InternalOpenAIProviderTestRuntime {
  resolveManagedCommand?: () => string;
  onQueryConstruction?: () => void;
  createManagedRun?: (options: ManagedCodexAppServerOptions) => ManagedCodexAppServerRun;
}

export function internalOpenAIProviderWithManagedHooksProbeForTest(
  assertManagedHooks: ManagedHooksProbe,
  runtime: InternalOpenAIProviderTestRuntime = {},
): AgentProvider {
  const resolveCommand = runtime.resolveManagedCommand ?? trustedManagedCodexExecutable;
  return {
    id: "openai",
    liveInput: "turn-framed",
    probe: probeCodex,
    admit: ({ options, target }) =>
      admitOpenAIWithManagedHooksProbe(options, target, assertManagedHooks, resolveCommand),
    query: ({ input, options, target, context }) => {
      runtime.onQueryConstruction?.();
      const admitted = consumeExecutionAdmission("openai", options);
      return new CodexQuery(
        input,
        options,
        context.writer,
        context.route,
        context.artifacts,
        target,
        admitted,
        assertManagedHooks,
        resolveCommand,
        admitted ? takeManagedLaunch(options) : undefined,
        runtime.createManagedRun,
      );
    },
  };
}

export const openaiProvider: AgentProvider = Object.freeze(
  internalOpenAIProviderWithManagedHooksProbeForTest(assertInstalledManagedCodexHooks),
);
