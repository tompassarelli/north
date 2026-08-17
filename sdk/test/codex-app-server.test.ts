import { afterEach, expect, test } from "bun:test";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { EventEmitter } from "node:events";
import {
  chmodSync, closeSync, existsSync, fsyncSync, mkdirSync, mkdtempSync, openSync,
  readFileSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { PassThrough } from "node:stream";
import type { ChildProcessWithoutNullStreams } from "node:child_process";
import {
  MANAGED_CODEX_DISABLED_FEATURES, MANAGED_CODEX_ENABLED_FEATURES, MANAGED_CODEX_VERSION,
  ManagedCodexAppServerRun, ManagedCodexHarvestError, ManagedCodexPreThreadError,
  managedCodexAppServerLaunch, managedCodexRecoveredContext,
} from "../src/providers/codex-app-server";
import { managedCodexHarvestEvidence } from "../src/providers/openai";
import { OpenAIWireNormalizer } from "../src/providers/openai-wire";
import {
  WireEventWriter, wireEventId, wireRunId,
} from "../src/wire";
import { causeChain } from "../src/death";
import { expectedManagedCodexHooks } from "../src/providers/codex-managed-hooks";
import {
  CODEX_SUPERVISOR_STATUS_PREFIX, codexSupervisorStatusLine, codexSupervisorStderrLine,
  codexSupervisorStderrStatus,
} from "../src/providers/codex-supervisor-protocol";
import {
  compileProviderAuthoritySurface, type OpenAIAuthoritySurface,
} from "../src/providers/authority";
import { harnessOptions } from "../src/harness";
import { applyOrchestrationStaffing } from "../src/orchestration-staffing";
import { providerSessionKey, providerTurnKey } from "../src/providers/provider-join";
import { NORTH_BINARY_PROBE_SCRIPT } from "../src/native-command-activity";
import { managedCodexWritableRoots } from "../src/providers/codex-app-server";
import { RETAINED_PROVIDER_PREVIEW_MAX_BYTES } from "../src/providers/retained-artifact";

function firstLine(stream: NodeJS.ReadableStream, label: string): Promise<string> {
  return new Promise((resolveLine, reject) => {
    let buffer = "";
    const timer = setTimeout(() => finish(new Error(`${label} timed out`)), 2_000);
    const cleanup = () => {
      clearTimeout(timer);
      stream.removeListener("data", onData);
      stream.removeListener("error", onError);
      stream.removeListener("end", onEnd);
    };
    const finish = (error?: Error, line?: string) => {
      cleanup();
      if (error) reject(error);
      else resolveLine(line!);
    };
    const onData = (chunk: Buffer | string) => {
      buffer += chunk.toString();
      const newline = buffer.indexOf("\n");
      if (newline >= 0) finish(undefined, buffer.slice(0, newline));
    };
    const onError = (error: Error) => finish(error);
    const onEnd = () => finish(new Error(`${label} ended before a frame`));
    stream.on("data", onData);
    stream.once("error", onError);
    stream.once("end", onEnd);
  });
}

function supervisorFrame(line: string, digest?: string): Buffer {
  const payload = Buffer.from(line, "utf8");
  const checksum = digest ?? createHash("sha256").update(payload).digest("hex");
  return Buffer.concat([
    Buffer.from(`NORTH_CODEX_RPC 1 ${payload.byteLength} ${checksum}\n`, "ascii"),
    payload,
  ]);
}

function writeAtomicSupervisorFrame(path: string, line: string): void {
  const fd = openSync(path, "wx", 0o600);
  try {
    writeFileSync(fd, supervisorFrame(line));
    fsyncSync(fd);
  } finally {
    closeSync(fd);
  }
}

async function waitForFile(path: string, timeoutMs = 2_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!existsSync(path) && Date.now() < deadline) await Bun.sleep(10);
  if (!existsSync(path)) throw new Error(`timed out waiting for ${path}`);
}

async function waitForProcessGone(pid: number, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try { process.kill(pid, 0); }
    catch { return; }
    await Bun.sleep(10);
  }
  throw new Error(`process ${pid} survived its teardown bound`);
}

function killProcess(pid: number | undefined, group = false): void {
  if (!pid || !Number.isSafeInteger(pid) || pid <= 1) return;
  if (group && process.platform !== "win32") {
    try { process.kill(-pid, "SIGKILL"); } catch {}
  }
  try { process.kill(pid, "SIGKILL"); } catch {}
}

const roots: string[] = [];
afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

const tools = ["ready", "show", "tell"];
const surface = {
  provider: "openai",
  capabilities: ["read", "search", "write", "shell"],
  nativeMultiAgent: "disabled",
  liveInput: "turn-framed",
  authoringHooks: "managed-only",
  northEnabledTools: tools,
  sandbox: "workspace-write",
  web: "disabled",
  managedTools: tools.map((name) => `mcp__north__${name}`),
} as OpenAIAuthoritySurface;

function populatedPersonalCodexConfig(projectRoot: string) {
  // Mirrors the key shape Codex reports from ~/.codex/config.toml when HOME is
  // also the untrusted project root. Values are synthetic and intentionally
  // authority-widening: the regression proves the layer is accepted only
  // because Codex reports it disabled and every effective surface stays sealed.
  return {
    agents: { max_concurrent_threads_per_session: 999 },
    project_doc_fallback_filenames: ["CLAUDE.md"],
    default_permissions: ":danger-full-access",
    approval_policy: "on-request",
    sandbox_mode: "danger-full-access",
    approvals_reviewer: "guardian",
    model: "gpt-hostile",
    model_reasoning_effort: "minimal",
    projects: {
      [projectRoot]: { trust_level: "trusted" },
      [join(projectRoot, "code", "other")]: { trust_level: "trusted" },
    },
    tui: { model_availability_nux: { "gpt-hostile": 1 } },
    features: { browser_use: true, hooks: false, remote_control: true },
    mcp_servers: { hostile: { command: "/tmp/hostile-mcp" } },
    hooks: {
      state: {
        [`${join(projectRoot, ".codex", "hooks.json")}:pre_tool_use:0:0`]: {
          trusted_hash: `sha256:${"a".repeat(64)}`,
        },
      },
    },
    notice: { hide_full_access_warning: true },
  };
}

function hookRows() {
  const rows: any[] = [];
  for (const [event, groups] of Object.entries(expectedManagedCodexHooks())) {
    for (const group of groups) for (const hook of group.hooks) rows.push({
      key: `${event}:${rows.length}`,
      eventName: event[0]!.toLowerCase() + event.slice(1),
      handlerType: "command",
      matcher: group.matcher ?? null,
      command: hook.command,
      timeoutSec: hook.timeout,
      statusMessage: null,
      sourcePath: "/etc/codex/hooks",
      source: "system",
      pluginId: null,
      displayOrder: rows.length,
      enabled: true,
      isManaged: true,
      currentHash: `sha256:${String(rows.length).padStart(64, "0")}`,
      trustStatus: "managed",
    });
  }
  return rows;
}

function turn(id: string, status: "inProgress" | "completed") {
  return {
    id, items: [], itemsView: "notLoaded", status, error: null,
    startedAt: 1, completedAt: status === "completed" ? 2 : null,
    durationMs: status === "completed" ? 1 : null,
  };
}

function hookRun(
  id: string,
  eventName: string,
  status: string,
  overrides: Record<string, unknown> = {},
) {
  const completed = status !== "running";
  const policyBlocked = status === "blocked";
  const failed = completed && status !== "completed" && !policyBlocked;
  return {
    id, eventName, handlerType: "command", executionMode: "sync",
    scope: eventName === "sessionStart" ? "thread" : "turn",
    sourcePath: "/etc/codex/hooks", source: "system", displayOrder: 0,
    status, statusMessage: failed ? "fixture failure" : null,
    startedAt: 1, completedAt: completed ? 2 : null, durationMs: completed ? 1 : null,
    entries: policyBlocked
      ? [{ kind: "feedback", text: "fixture policy denial" }]
      : failed ? [{ kind: "error", text: "fixture failure" }] : [],
    ...overrides,
  };
}

function setup(mode = "ok") {
  const root = mkdtempSync(join(tmpdir(), "north-managed-codex-"));
  roots.push(root);
  const codexHome = join(root, "codex-home");
  const sqliteHome = join(codexHome, "sqlite");
  mkdirSync(sqliteHome, { recursive: true });
  writeFileSync(join(codexHome, "AGENTS.md"), "canonical global instructions\n");
  const executable = join(root, "codex");
  writeFileSync(executable, "#!/bin/sh\nexit 1\n");
  chmodSync(executable, 0o700);
  const projectRoot = realpathSync(join(import.meta.dir, "../.."));
  const cwd = mode === "nested-project-warning"
    ? realpathSync(join(projectRoot, "sdk"))
    : projectRoot;
  const requests: any[] = [];
  const webNetwork = [
    "web-network", "web-network-boolean-drift", "web-network-object-drift",
    "web-network-session-drift", "web-network-thread-drift",
  ]
    .includes(mode);
  const sandboxNetwork = true;
  const features = Object.fromEntries([
    ...MANAGED_CODEX_ENABLED_FEATURES.map((name) => [name, true]),
    ...MANAGED_CODEX_DISABLED_FEATURES.map((name) => [name, false]),
    ["network_proxy", webNetwork
      ? { enabled: true, domains: { "chromium.googlesource.com": "allow" } }
      : false],
  ]);
  const effectiveFeatures = { ...features, remote_control: false };
  const sessionFeatures = {
    ...features,
    network_proxy: webNetwork
      ? { enabled: true, domains: { "chromium.googlesource.com": "allow" } }
      : false,
  };
  const north = {
    command: "/nix/store/north/bin/north-mcp",
    args: [] as string[],
    env: { NORTH_BIN: "/nix/store/north/bin/north" },
  };
  const engine = join(realpathSync(join(import.meta.dir, "../..")), "bin/north");
  const managedPath = `${dirname(engine)}${delimiter}${process.env.PATH ?? ""}`;
  const shellEnvironmentPolicy = {
    inherit: "core",
    set: { PATH: managedPath, NORTH_BIN: engine },
  };
  const effectiveShellEnvironmentPolicy = {
    ...shellEnvironmentPolicy,
    ignore_default_excludes: null,
    exclude: null,
    include_only: null,
    filters: null,
    experimental_use_profile: null,
  };
  // The workspace-write sandbox grants the checkout's Git metadata roots + North state root
  // so a managed lane can commit what it wrote; the fixture mirrors production
  // rather than restating a hard-coded path.
  const writableRoots = managedCodexWritableRoots(cwd);
  const session = {
    cli_auth_credentials_store: "file",
    forced_login_method: "chatgpt",
    model_provider: "openai",
    sqlite_home: sqliteHome,
    ...(writableRoots.length ? { sandbox_workspace_write: {
      writable_roots: writableRoots, network_access: sandboxNetwork,
    } } : {}),
    project_root_markers: [".git"],
    projects: { [projectRoot]: { trust_level: "untrusted" } },
    project_doc_max_bytes: 0,
    allow_login_shell: false,
    shell_environment_policy: shellEnvironmentPolicy,
    mcp_servers: {
      north: {
        command: north.command, args: [], env: north.env,
        enabled: true, required: true, enabled_tools: tools,
      },
    },
    web_search: webNetwork ? "cached" : "disabled",
    features: sessionFeatures,
  };
  const baseConfig = {
    config: {
      features: effectiveFeatures,
      mcp_servers: {
        north: {
          ...session.mcp_servers.north, environment_id: "local", tool_timeout_sec: null,
        },
      },
      projects: session.projects,
      shell_environment_policy: effectiveShellEnvironmentPolicy,
      project_doc_max_bytes: 0, model_provider: "openai",
      cli_auth_credentials_store: "file", forced_login_method: "chatgpt",
      sqlite_home: sqliteHome, allow_login_shell: false,
      apps: null, plugins: {}, marketplaces: {},
    },
    origins: {},
    layers: [
      { name: { type: "sessionFlags" }, version: `sha256:${"1".repeat(64)}`, config: session },
      { name: { type: "project", dotCodexFolder: join(projectRoot, ".codex") },
        version: `sha256:${"2".repeat(64)}`, config: {}, disabledReason: "untrusted" },
      { name: { type: "user", file: join(codexHome, "config.toml"), profile: null },
        version: `sha256:${"3".repeat(64)}`, config: {} },
      { name: { type: "system", file: "/etc/codex/config.toml" },
        version: `sha256:${"4".repeat(64)}`, config: {} },
    ],
  };
  let configReads = 0;
  let nextPid = 4100;
  // Which provider PROCESS this is for the lane. A respawn calls spawnProcess
  // again, so a mode can behave differently per attempt — that is the whole
  // observable difference between "the lane died" and "the lane survived".
  let attempts = 0;
  // Every turn/start input text the lane sent, across all attempts: how a test
  // proves the recovered context actually reached the new provider session.
  const turnInputs: string[] = [];
  const replacementPreflightEntered = Promise.withResolvers<void>();
  const replacementPreflightRelease = Promise.withResolvers<void>();
  let replacementPreflightHeld = false;
  const spawnProcess = (() => {
    const attempt = ++attempts;
    const child = new EventEmitter() as ChildProcessWithoutNullStreams & {
      exitCode: number | null; signalCode: NodeJS.Signals | null;
    };
    const stdin = new PassThrough();
    const stdout = new PassThrough();
    const stderr = new PassThrough();
    Object.assign(child, {
      stdin, stdout, stderr, stdio: [stdin, stdout, stderr],
      pid: nextPid++, exitCode: null, signalCode: null, killed: false,
    });
    // `dying` is set SYNCHRONOUSLY at the kill point while the real exit lands a
    // microtask later, so a fixture that dies mid-turn stops mid-turn instead of
    // finishing its script into an ended stream.
    let dying = false;
    const send = (value: unknown) => {
      if (dying) return;
      stdout.write(`${JSON.stringify(value)}\n`);
    };
    const result = (request: any, value: unknown) => send({ id: request.id, result: value });
    const fail = (request: any) => send({
      id: request.id, error: { code: -32000, message: "fixture failure" },
    });
    const notify = (method: string, params: unknown) => {
      const envelope: any = { method, params };
      if (mode !== "notification-emitted-at-omitted") envelope.emittedAtMs = 1;
      if (mode === "notification-emitted-at-negative") envelope.emittedAtMs = -1;
      if (mode === "notification-emitted-at-string") envelope.emittedAtMs = "1";
      if (mode === "notification-envelope-extra") envelope.futureEnvelope = true;
      send(envelope);
    };
    const respawningMode = mode === "respawn-after-third-item"
      || mode === "respawn-interrupt-gap"
      || mode === "respawn-interrupt-multigap"
      || mode === "provider-death-mid-turn"
      || mode === "respawn-exhausted";
    const respawnThreadIds = [
      "019f7abc-0000-7000-8000-000000000001",
      "019f7abc-0000-7000-8000-000000000005",
      "019f7abc-0000-7000-8000-000000000009",
    ];
    const threadId = respawningMode
      ? respawnThreadIds[Math.min(attempt - 1, respawnThreadIds.length - 1)]!
      : respawnThreadIds[0]!;
    const turnIds = [
      "019f7abc-0000-7000-8000-000000000002",
      "019f7abc-0000-7000-8000-000000000003",
      "019f7abc-0000-7000-8000-000000000004",
    ];
    let turnStarts = 0;
    // The live turn id for the turn currently being served. Continuation turns
    // reuse the same thread but MUST carry a distinct turn id.
    let turnId = turnIds[0]!;
    const item = (id: string, type: string, extra: Record<string, unknown> = {}) => ({ id, type, ...extra });
    // The provider process dies where a real one does: after some of the turn's
    // work has already landed, with its own account of why on stderr.
    const die = (code: number, ...lines: string[]) => {
      if (dying) return;
      dying = true;
      for (const line of lines) stderr.write(`${line}\n`);
      queueMicrotask(() => exit(code, null));
    };
    let completedItems = 0;
    const lifecycle = (kind: "started" | "completed", value: any, at: number) => {
      notify(
        `item/${kind}`,
        { item: value, threadId, turnId, [kind === "started" ? "startedAtMs" : "completedAtMs"]: at },
      );
      if (kind !== "completed") return;
      completedItems += 1;
      // Exactly the shape a managed write-lane dies in: real work landed, then
      // the app-server is gone. Only the FIRST provider process dies here.
      if ((mode === "respawn-after-third-item" || mode === "respawn-interrupt-gap"
          || mode === "respawn-interrupt-multigap"
          || mode === "respawn-preflight-broken")
          && attempt === 1 && completedItems === 3) {
        if (mode === "respawn-after-third-item" || mode === "respawn-interrupt-gap"
            || mode === "respawn-interrupt-multigap") {
          notify("item/started", {
            threadId,
            turnId,
            startedAtMs: 16,
            item: item("command-open-before-respawn", "commandExecution", {
              command: "printf unfinished",
              cwd,
              processId: null,
              source: "agent",
              status: "inProgress",
              commandActions: [{ type: "unknown", command: "printf unfinished" }],
              aggregatedOutput: null,
              exitCode: null,
              durationMs: null,
              pluginId: null,
              scriptPath: null,
            }),
          });
          notify("thread/tokenUsage/updated", { threadId, turnId, tokenUsage: { total: {
            totalTokens: 100, inputTokens: 80, cachedInputTokens: 30,
            outputTokens: 20, reasoningOutputTokens: 5,
          } } });
        }
        // The managed account disappearing between provider sessions is the
        // cheapest way to make the RE-preflight fail the way a real one would.
        if (mode === "respawn-preflight-broken")
          rmSync(codexHome, { recursive: true, force: true });
        die(9, "codex: ERROR responses stream closed unexpectedly");
      }
    };
    const emitHook = (event: string, terminalStatus = "completed", id = `hook-${event}`) => {
      let hookTurnId: string | null = turnId;
      let scope = event === "sessionStart" ? "thread" : "turn";
      if (mode === "hook-session-invalid-turn" && event === "sessionStart") hookTurnId = "bad/value";
      if (mode === "hook-session-scope" && event === "sessionStart") scope = "turn";
      if (mode === "hook-tool-null-turn" && event === "preToolUse") hookTurnId = null;
      if (mode === "hook-tool-thread-scope" && event === "preToolUse") scope = "thread";
      notify("hook/started", { threadId, turnId: hookTurnId,
        run: hookRun(id, event, "running", { scope }) });
      if (mode === "hook-missing-completion" && event === "preToolUse") return;
      const completion = hookRun(id, mode === "hook-completion-event-drift" && event === "preToolUse"
        ? "postToolUse" : event, terminalStatus, { scope });
      if (mode === "hook-completion-summary-drift" && event === "sessionStart") {
        completion.displayOrder = 99;
        completion.startedAt = 99;
        completion.completedAt = 100;
        completion.durationMs = 1;
      }
      const completionTurnId = mode === "hook-completion-summary-drift" && event === "sessionStart"
        ? turnIds[1] : hookTurnId;
      notify("hook/completed", { threadId, turnId: completionTurnId,
        run: completion });
      if (mode === "hook-duplicate-completion" && event === "preToolUse")
        notify("hook/completed", { threadId, turnId, run: hookRun(id, event, terminalStatus) });
    };
    const mcpServer = () => {
      const server: any = {
        name: "north",
        serverInfo: {
          name: "north", title: null, version: "0.1.0", description: null,
          icons: null, websiteUrl: null,
        },
        tools: Object.fromEntries(tools.map((name) => [name, {
          name, inputSchema: { type: "object" }, description: null, annotations: null,
        }])),
        resources: [], resourceTemplates: [], authStatus: "unsupported",
      };
      if (mode === "mcp-resource") server.resources = [{ uri: "file:///hostile" }];
      if (mode === "mcp-template") server.resourceTemplates = [{ uriTemplate: "file:///{x}" }];
      if (mode === "mcp-auth") server.authStatus = "oauth";
      if (mode === "mcp-server-info") server.serverInfo.version = "9.9.9";
      return server;
    };
    const mcpInventory = () => [mcpServer()];
    const startedThread = (request: any) => {
      const thread: any = {
        id: threadId, extra: null, sessionId: "019f7abc-0000-7000-8000-000000000000",
        forkedFromId: null, parentThreadId: null, preview: "", ephemeral: true,
        historyMode: "legacy", modelProvider: "openai", createdAt: 1, updatedAt: 1,
        recencyAt: 1, status: { type: "idle" }, path: null, cwd, cliVersion: "0.146.0",
        source: "appServer", threadSource: null, agentNickname: null, agentRole: null,
        gitInfo: null, name: null, turns: [],
      };
      const response: any = {
        thread, model: request.params.model, modelProvider: request.params.modelProvider,
        // Codex echoes the cwd PLUS every configured sandbox writable root here.
        // The pre-2026-07-26 fixture hardcoded [cwd] while the launch contract
        // granted Git metadata roots, so the contract drift that killed every
        // managed workspace-write lane at thread/start passed this suite green.
        serviceTier: null, cwd, runtimeWorkspaceRoots: [cwd, ...writableRoots],
        instructionSources: [join(codexHome, "AGENTS.md")], approvalPolicy: "never",
        approvalsReviewer: "user",
        sandbox: {
          type: "workspaceWrite", writableRoots, networkAccess: sandboxNetwork,
          excludeTmpdirEnvVar: false, excludeSlashTmp: false,
        },
        activePermissionProfile: null, reasoningEffort: "high",
        multiAgentMode: "explicitRequestOnly",
      };
      const mutations: Record<string, () => void> = {
        "thread-model": () => { response.model = "wrong"; },
        "thread-provider": () => { response.modelProvider = "hostile"; },
        "thread-service-tier": () => { response.serviceTier = "priority"; },
        "thread-cwd": () => { response.cwd = root; },
        "thread-roots": () => { response.runtimeWorkspaceRoots = [root]; },
        // The exact shape that shipped broken: the runtime echoes only the cwd
        // while the launch contract granted Git metadata writable roots.
        "thread-roots-drop-grant": () => { response.runtimeWorkspaceRoots = [cwd]; },
        "thread-roots-widened": () => { response.runtimeWorkspaceRoots = [cwd, ...writableRoots, root]; },
        "thread-roots-reordered": () => {
          response.runtimeWorkspaceRoots = [...writableRoots, cwd];
        },
        "thread-sources": () => { response.instructionSources.push(join(cwd, "AGENTS.md")); },
        "thread-approval": () => { response.approvalPolicy = "on-request"; },
        "thread-reviewer": () => { response.approvalsReviewer = "auto_review"; },
        "thread-sandbox": () => { response.sandbox.networkAccess = false; },
        "web-network-thread-drift": () => { response.sandbox.networkAccess = false; },
        "thread-profile": () => { response.activePermissionProfile = { id: ":workspace", extends: null }; },
        "thread-effort": () => { response.reasoningEffort = "low"; },
        "thread-multi-agent": () => { response.multiAgentMode = "proactive"; },
        "thread-ephemeral": () => { thread.ephemeral = false; },
        "thread-object-provider": () => { thread.modelProvider = "hostile"; },
        "thread-object-cwd": () => { thread.cwd = root; },
        "thread-extra-authority": () => { response.futureAuthority = true; },
        "thread-id-missing": () => {
          delete thread.id;
          delete thread.sessionId;
        },
        "thread-id-malformed": () => { thread.id = "not a protocol id"; },
      };
      mutations[mode]?.();
      if (mode === "driver-shape-tolerance") {
        delete thread.id;
        delete thread.sessionId;
        thread.futureMetadata = { providerRevision: 2 };
        response.sessionId = threadId;
        response.futureMetadata = { providerRevision: 2 };
      }
      if (mode === "thread-id-root-thread-variant") {
        delete thread.id;
        delete thread.sessionId;
        response.threadId = threadId;
      }
      return response;
    };
    const emitRuntime = () => {
      const startedTurn: any = turn(turnId, "inProgress");
      if (mode === "notification-turn-extra" || mode === "driver-shape-tolerance")
        startedTurn.futureMetadata = { providerRevision: 2 };
      if (mode === "turn-id-mismatch-notification")
        startedTurn.id = "019f7abc-0000-7000-8000-000000000099";
      notify("turn/started", { threadId, turn: startedTurn });
      if (mode === "passive-items" || mode === "provider-death-open-passive") {
        for (const passive of [
          item("user-message-1", "userMessage"),
          item("hook-prompt-1", "hookPrompt"),
        ]) {
          lifecycle("started", passive, 1);
          if (mode === "passive-items") lifecycle("completed", passive, 2);
        }
        if (mode === "provider-death-open-passive") {
          die(9, "codex: provider died with passive inputs open");
          return;
        }
      }
      // Leave the replacement turn live until the queued control request is
      // delivered. The first provider attempt still follows the ordinary work
      // path below and dies after its third completed item.
      if ((mode === "respawn-interrupt-gap" && attempt > 1)
          || (mode === "respawn-interrupt-multigap" && attempt > 2)) return;
      // Every provider process this lane gets dies the moment it has a turn:
      // the respawn budget is spent and the lane still fails.
      if (mode === "respawn-exhausted") {
        die(7, `codex: fatal: provider session ${attempt} refused to start work`);
        return;
      }
      // Accepted, then never another word: only the overall turn deadline can
      // end this one.
      if (mode === "turn-silent-before-tool" || mode === "turn-interrupt-refused"
          || (mode === "external-turn-interrupt" && turnStarts === 1)) return;
      if (mode.startsWith("safety-buffering")) {
        const safetyBuffering: any = {
          threadId, turnId, model: "gpt-fixture-exact",
          useCases: ["long-running-agent-work"], reasons: ["safety-buffering-eligible"],
          showBufferingUi: false, fasterModel: null,
        };
        if (mode === "safety-buffering-without-faster-model") delete safetyBuffering.fasterModel;
        if (mode === "safety-buffering-with-faster-model")
          safetyBuffering.fasterModel = "gpt-faster-fixture";
        if (mode === "safety-buffering-extra") safetyBuffering.futureAuthority = true;
        if (mode === "safety-buffering-wrong-thread") safetyBuffering.threadId = "wrong";
        if (mode === "safety-buffering-wrong-turn") safetyBuffering.turnId = "wrong";
        if (mode === "safety-buffering-wrong-model") safetyBuffering.model = "hostile-model";
        if (mode === "safety-buffering-missing-reasons") delete safetyBuffering.reasons;
        if (mode === "safety-buffering-invalid-reason") safetyBuffering.reasons = [7];
        if (mode === "safety-buffering-too-many-reasons") safetyBuffering.reasons = Array(65).fill("x");
        if (mode === "safety-buffering-oversized-use-case")
          safetyBuffering.useCases = ["x".repeat(4097)];
        if (mode === "safety-buffering-invalid-ui") safetyBuffering.showBufferingUi = "false";
        if (mode === "safety-buffering-invalid-faster-model") safetyBuffering.fasterModel = 7;
        notify("model/safetyBuffering/updated", safetyBuffering);
      }
      emitHook("preToolUse", mode === "hook-pretool-failed" ? "failed"
        : mode === "hook-pretool-blocked" ? "blocked"
        : mode === "hook-pretool-stopped" ? "stopped" : "completed", "hook-pre");
      if (mode !== "command-none") {
        const probeCommand = `/bin/bash -c '${NORTH_BINARY_PROBE_SCRIPT}'`;
        const action: any = { type: "unknown", command: NORTH_BINARY_PROBE_SCRIPT };
        if (mode === "command-action-extra") action.futureAuthority = true;
        const startedCommand = item("command-1", "commandExecution", {
          command: probeCommand, cwd, processId: "process-1", source: "unifiedExecStartup",
          status: "inProgress", commandActions: [action],
          aggregatedOutput: null, exitCode: null, durationMs: null,
          pluginId: mode === "command-plugin-attributed" ? "first-party" : null,
          scriptPath: mode === "command-plugin-attributed" ? "bin/run.sh" : null,
        });
        lifecycle("started", startedCommand, 10);
        const completedCommand: any = {
          ...startedCommand,
          status: mode === "command-failed" ? "failed" : "completed",
          command: mode === "command-wrapper-mismatch"
            ? `/bin/bash -lc '${NORTH_BINARY_PROBE_SCRIPT}'` : probeCommand,
          aggregatedOutput: mode === "command-output-mismatch"
            ? "/run/current-system/sw/bin/north\n" : `${engine}\n${engine}\n`,
          exitCode: mode === "command-failed" ? 1 : 0,
          durationMs: 1,
        };
        if (mode === "command-schema-extra") completedCommand.futureAuthority = true;
        if (mode === "turn-silent-open-item") return;
        if (mode === "turn-silent-open-item-completes") {
          setTimeout(() => {
            lifecycle("completed", completedCommand, 11);
            finishRuntime();
          }, 250);
          return;
        }
        notify("item/commandExecution/outputDelta", {
          threadId, turnId, itemId: startedCommand.id, delta: "ok\n",
        });
        notify("item/commandExecution/terminalInteraction", {
          threadId, turnId, itemId: startedCommand.id, processId: "process-1", stdin: "",
        });
        if (mode !== "command-missing-completion") lifecycle("completed", completedCommand, 11);
      }
      // Codex dies mid-turn after landing real work, with its own account of
      // why on stderr (and one credential in it, to prove redaction).
      if (mode === "provider-death-mid-turn") {
        stderr.write("codex: ERROR responses stream closed unexpectedly\n");
        stderr.write("codex: retry with Authorization: Bearer sk-fixturesecretvalue0123\n");
        queueMicrotask(() => exit(9, null));
        return;
      }
      if (mode === "pending-summary-bound-death") {
        for (let index = 0; index < 18; index += 1) {
          lifecycle("started", item(`mcp-private-${index}`, "mcpToolCall", {
            server: "north",
            tool: `pending_${String(index).padStart(2, "0")}`,
            arguments: { message: `CANARY-private-pending-${index}` },
          }), 20 + index);
        }
        die(9, "codex: bounded pending summary fixture died");
        return;
      }
      if (mode === "pending-side-effect-death") {
        lifecycle("started", item("reasoning-private-id", "reasoning"), 20);
        lifecycle("started", item("message-private-id", "agentMessage"), 21);
        lifecycle("started", item("mcp-private-id", "mcpToolCall", {
          server: "north",
          tool: "tell",
          arguments: { message: "CANARY-private-pending-argument" },
        }), 22);
        die(9, "codex: pending side effect lost its terminal");
        return;
      }
      // One completed tool item, then silence: the post-tool quiet watchdog's
      // exact shape (a wedged tool loop that never speaks again).
      if (mode === "turn-silent-after-tool") return;
      // Slow but unmistakably alive: a tool item lands every 50ms with plain
      // activity in between, so a correct watchdog re-arms and never fires.
      if (mode === "turn-slow-active") {
        let tick = 0;
        const beat = () => {
          if (tick >= 5) { finishRuntime(); return; }
          tick += 1;
          const slow = item(`slow-${tick}`, "fileChange");
          lifecycle("started", slow, 30 + tick);
          notify("item/fileChange/outputDelta", {
            threadId, turnId, itemId: slow.id, delta: "chunk",
          });
          lifecycle("completed", slow, 31 + tick);
          setTimeout(beat, 50);
        };
        setTimeout(beat, 50);
        return;
      }
      // Reasoning deltas are provider execution activity even without an item
      // lifecycle envelope; they keep the ordinary inactivity gate open.
      if (mode === "turn-slow-reasoning") {
        let tick = 0;
        const beat = () => {
          if (tick >= 5) { finishRuntime(); return; }
          tick += 1;
          notify("item/reasoning/textDelta", {
            threadId, turnId, itemId: "reasoning-slow", delta: "thinking", contentIndex: 0,
          });
          setTimeout(beat, 50);
        };
        setTimeout(beat, 50);
        return;
      }
      // The killer shape, live-observed as `Codex started command execution
      // lifecycle is invalid` after 79 good commands (lane ms1fhh0v): codex's
      // shell tool carries a per-command `workdir`, so the item echoes the
      // SUBPROCESS directory. Every one of these is a legitimate agent move.
      if (mode.startsWith("command-cwd-")) {
        const workdir = mode === "command-cwd-subdir" ? join(cwd, "sdk")
          : mode === "command-cwd-sibling" ? root
          : mode === "command-cwd-scratch" ? "/tmp"
          : mode === "command-cwd-relative" ? "sdk"
          : mode === "command-cwd-traversal" ? `${cwd}/../escape`
          : mode === "command-cwd-dot" ? `${cwd}/./sdk`
          // Benign: codex canonicalizes workdirs, but a trailing separator is
          // not a defect — it must not cost a lane its turn.
          : mode === "command-cwd-trailing-slash" ? `${join(cwd, "sdk")}/`
          : "";
        const started = item("command-2", "commandExecution", {
          command: "bun --version", cwd: workdir, processId: null, source: "agent",
          status: "inProgress", commandActions: [{ type: "unknown", command: "bun --version" }],
          aggregatedOutput: null, exitCode: null, durationMs: null,
          pluginId: null, scriptPath: null,
        });
        lifecycle("started", started, 20);
        lifecycle("completed", {
          ...started, status: "completed", aggregatedOutput: "1.3.10\n",
          exitCode: 0, durationMs: 4,
        }, 21);
      }
      const file = item("file-1", "fileChange");
      lifecycle("started", file, 12);
      notify("item/fileChange/outputDelta", { threadId, turnId, itemId: file.id, delta: "patched" });
      notify("item/fileChange/patchUpdated", { threadId, turnId, itemId: file.id, changes: [] });
      lifecycle("completed", file, 13);
      const mcp = item("mcp-1", "mcpToolCall", {
        server: "north", tool: "tell",
        arguments: { secret: "CANARY-private-argument" },
        result: "CANARY-private-result",
      });
      if (mode === "mcp-identity-loss") delete mcp.tool;
      lifecycle("started", mcp, 14);
      notify("item/mcpToolCall/progress", { threadId, turnId, itemId: mcp.id, message: "working" });
      lifecycle("completed", mcp, 15);
      const reasoning = item("reasoning-1", "reasoning");
      lifecycle("started", reasoning, 16);
      notify("item/reasoning/summaryPartAdded", { threadId, turnId, itemId: reasoning.id, summaryIndex: 0 });
      notify("item/reasoning/summaryTextDelta", {
        threadId, turnId, itemId: reasoning.id, delta: "summary", summaryIndex: 0,
      });
      notify("item/reasoning/textDelta", {
        threadId, turnId, itemId: reasoning.id, delta: "reasoning", contentIndex: 0,
      });
      lifecycle("completed", reasoning, 17);
      notify("item/plan/delta", { threadId, turnId, itemId: "plan-1", delta: "plan" });
      notify("turn/plan/updated", {
        threadId, turnId, explanation: null, plan: [{ step: "work", status: "completed" }],
      });
      notify("turn/diff/updated", { threadId, turnId, diff: "diff --git a/a b/a" });
      finishRuntime();
    };
    // The closing sequence of a turn, split out so a mode can interpose its own
    // traffic (slow-but-active) before the terminal.
    const finishRuntime = () => {
      // A respawned session echoes the frame it was handed, so a test can prove
      // the recovered context reached the NEW provider process — not merely
      // that a second process existed.
      const answer = item("answer-1", "agentMessage", {
        text: mode === "respawn-after-third-item" && attempt > 1
          ? `managed answer after recovery\n${turnInputs.at(-1) ?? ""}`
          : "managed answer",
      });
      if (mode === "respawn-after-third-item" && attempt > 1) {
        lifecycle("started", answer, 18);
        lifecycle("completed", answer, 19);
      } else {
        notify("item/agentMessage/delta", {
          threadId, turnId, itemId: answer.id,
          delta: mode === "large-agent-message-delta" ? "x".repeat(1024 * 1024 + 1) : "managed answer",
        });
        lifecycle("completed", answer, 18);
      }
      emitHook("postToolUse", mode === "hook-posttool-stopped" ? "stopped"
        : mode === "hook-posttool-failed" ? "failed" : "completed", "hook-post");
      notify("thread/tokenUsage/updated", { threadId, turnId, tokenUsage: { total: {
        totalTokens: 12, inputTokens: 9, cachedInputTokens: 4,
        outputTokens: 3, reasoningOutputTokens: 1,
      } } });
      if (mode === "notification-unknown-flood")
        for (let index = 0; index <= 16; index += 1)
          notify(`future/authority/${index}`, { enabled: true });
      if (mode === "notification-wrong-thread")
        notify("turn/diff/updated", { threadId: "wrong", turnId, diff: "x" });
      if (mode === "notification-malformed")
        notify("item/mcpToolCall/progress", { threadId, turnId, itemId: "mcp-1", message: 7 });
      const completedTurn: any = turn(turnId, "completed");
      if (mode === "driver-shape-tolerance")
        completedTurn.futureMetadata = { providerRevision: 2 };
      if (mode === "turn-summary-items-view") completedTurn.itemsView = "summary";
      if (mode === "turn-items-view-hostile") completedTurn.itemsView = "fullyLoaded";
      if (mode === "notification-terminal-error") completedTurn.error = { message: "hidden failure" };
      notify("turn/completed", { threadId, turn: completedTurn });
    };
    const handle = (request: any) => {
      requests.push(structuredClone(request));
      if (request.method === "initialized") return;
      if (request.method === "initialize") {
        const userAgent = mode === "runtime-version" ? "north/0.145.0 (test)"
          : mode === "runtime-version-prefix" ? "hostile/north/0.146.0 (test)"
          : mode === "runtime-version-suffix" ? "north/0.146.0-hostile (test)"
          : "north/0.146.0 (test)";
        result(request, {
          userAgent,
          codexHome, platformFamily: "unix", platformOs: "linux",
        });
        if (mode === "config-warning" || mode === "config-warning-drift"
            || mode === "config-warning-wrong-identifiers"
            || mode === "driver-shape-tolerance"
            || mode === "project-disabled-tracked"
            || mode === "nested-project-warning"
            || mode === "project-disabled-global-profile"
            || mode === "project-disabled-global-profile-effective-widened"
            || mode === "project-global-profile-enabled-with-warning"
            || mode === "project-disabled-unknown") {
          const expectedSummary = "Project-local config, hooks, and exec policies are disabled in the following folders until the project is trusted, but skills still load.\n"
            + `    1. ${projectRoot}/.codex\n`
            + `       ${projectRoot} is marked as untrusted in ${codexHome}/config.toml. To load project-local config, hooks, and exec policies, mark it trusted.\n`;
          const tolerantSummary = `Project settings at ${projectRoot}/.codex are inactive; `
            + `trust is configured through ${codexHome}/config.toml.`;
          const wrongSummary = `Project settings at ${root}/other/.codex are inactive; `
            + `trust is configured through ${root}/other/config.toml.`;
          notify("configWarning", {
            summary: mode === "config-warning-drift" ? `${expectedSummary}drift`
              : mode === "driver-shape-tolerance" ? tolerantSummary
              : mode === "config-warning-wrong-identifiers" ? wrongSummary
              : expectedSummary,
            details: null,
            ...(mode === "driver-shape-tolerance"
              ? { futureMetadata: { providerRevision: 2 } }
              : {}),
          });
        }
        const remote: any = {
          status: "disabled", serverName: "fixture", installationId: "fixture-installation",
          environmentId: null,
        };
        if (mode === "remote-enabled") remote.status = "enabled";
        if (mode === "remote-extra-field") remote.futureAuthority = true;
        if (mode === "remote-missing-installation") delete remote.installationId;
        notify("remoteControl/status/changed", remote);
        const deprecation: any = { summary: "fixture deprecation", details: "fixture details" };
        if (mode === "deprecation-extra-field") deprecation.futureAuthority = true;
        notify("deprecationNotice", deprecation);
        if (mode === "notification-unknown-prethread")
          notify("future/authority", { enabled: true });
        if (mode === "server-request-prethread")
          send({ id: "provider-request", method: "future/request", params: {} });
        notify("account/rateLimits/updated", { rateLimits: {
          limitId: null, limitName: null, primary: null, secondary: null, credits: null,
          individualLimit: null, planType: null, rateLimitReachedType: null,
        } });
        return;
      }
      if (request.method === "account/read") {
        const respond = () => result(request, {
          account: { type: "chatgpt", email: "fixture@example.test", planType: "pro" },
          requiresOpenaiAuth: true,
        });
        if ((mode === "respawn-interrupt-gap" || mode === "respawn-interrupt-multigap")
            && attempt === 2 && !replacementPreflightHeld) {
          replacementPreflightHeld = true;
          replacementPreflightEntered.resolve();
          void replacementPreflightRelease.promise.then(() => {
            if (mode === "respawn-interrupt-multigap") {
              die(8, "codex: replacement died before thread authority");
              return;
            }
            respond();
          });
          return;
        }
        respond();
        return;
      }
      if (request.method === "config/read") {
        configReads += 1;
        const current = structuredClone(baseConfig);
        if (mode === "project-disabled-tracked" || mode === "project-disabled-no-warning"
            || mode === "nested-project-warning") {
          current.layers[1].config = {
            mcp_servers: { "beagle-store": {
              command: "/home/tom/code/beagle/main/store/bin/beagle-store-mcp",
              args: [],
              env: { BEAGLE_STORE_FLIP: "1", BEAGLE_STORE_GRAPH_EDIT: "1" },
            } },
          };
          current.layers[1].disabledReason = `${projectRoot} is marked as untrusted in ${codexHome}/config.toml. To load project-local config, hooks, and exec policies, mark it trusted.`;
        }
        if (mode === "project-disabled-global-profile"
            || mode === "project-disabled-global-profile-effective-widened"
            || mode === "project-global-profile-enabled-with-warning"
            || mode === "project-disabled-unknown") {
          current.layers[1].config = populatedPersonalCodexConfig(projectRoot);
          current.layers[1].disabledReason = `${projectRoot} is marked as untrusted in ${codexHome}/config.toml. To load project-local config, hooks, and exec policies, mark it trusted.`;
        }
        if (mode === "project-global-profile-enabled-with-warning")
          delete (current.layers[1] as any).disabledReason;
        if (mode === "project-disabled-unknown")
          current.layers[1].config.skills = [{ path: "/tmp/hostile-skill" }];
        if (mode === "project-disabled-global-profile-effective-widened")
          current.config.features.browser_use = true;
        if (mode === "project-enabled") {
          current.layers[1].config = { mcp_servers: { hostile: { command: "hostile" } } };
          delete (current.layers[1] as any).disabledReason;
        }
        if (mode === "user-layer-nonempty")
          current.layers[2].config = { model: "gpt-5.6-sol", approval_policy: "never" };
        if (mode === "system-layer-nonempty")
          current.layers[3].config = { sandbox_mode: "danger-full-access" };
        if (mode === "feature-default-enabled") current.config.features.browser_use = true;
        if (mode === "feature-omitted") delete current.config.features.browser_use;
        if (mode === "web-network-boolean-drift") current.config.features.network_proxy = true;
        if (mode === "web-network-object-drift")
          current.config.features.network_proxy = { enabled: true, domains: { "example.test": "allow" } };
        if (mode === "web-network-session-drift") current.layers[0].config.features.network_proxy = false;
        if (mode === "shell-policy-missing")
          delete current.layers[0].config.shell_environment_policy;
        if (mode === "shell-policy-wrong-inherit")
          current.config.shell_environment_policy.inherit = "all";
        if (mode === "shell-policy-drift")
          current.config.shell_environment_policy.set.PATH = "/run/current-system/sw/bin";
        if (mode === "shell-policy-extra-key")
          current.config.shell_environment_policy.future_authority = true;
        if (mode === "shell-policy-filters-set")
          current.config.shell_environment_policy.filters = [{ shell: "bash", exclude: ["*"] }];
        if (mode === "login-shell-enabled") current.config.allow_login_shell = true;
        if (mode === "fingerprint-mutation" && configReads > 1)
          current.layers[0].version = `sha256:${"f".repeat(64)}`;
        if (mode === "terminal-notification-unknown" && configReads > 2)
          notify("future/authority", { enabled: true });
        if (mode === "settlement-config-drift" && configReads > 2)
          current.layers[0].version = `sha256:${"e".repeat(64)}`;
        result(request, current);
        return;
      }
      if (request.method === "configRequirements/read") {
        const failureMode = mode === "hook-failure-continue" ? "continue"
          : mode === "hook-failure-unrecognized" ? "future-mode"
          : "block";
        result(request, { requirements: {
          allowManagedHooksOnly: true, allowRemoteControl: false,
          ...(mode === "hook-failure-unattested" ? {} : { managedHookFailureMode: failureMode }),
          featureRequirements: { hooks: true }, hooks: { managedDir: "/etc/codex/hooks" },
        } });
        return;
      }
      if (request.method === "hooks/list") {
        result(request, { data: [{ cwd: request.params.cwds[0], hooks: hookRows(),
          warnings: mode === "hook-warning" ? ["fixture warning"] : [], errors: [] }] });
        return;
      }
      if (request.method === "mcpServerStatus/list") {
        expect(request.params.detail).toBe("full");
        if (!request.params.cursor) result(request, { data: [], nextCursor: "north-page" });
        else result(request, { data: mcpInventory(), nextCursor: null });
        return;
      }
      if (request.method === "command/exec") {
        const response: any = {
          exitCode: 0,
          stdout: mode === "preflight-system-wrapper"
            ? `/run/current-system/sw/bin/north\n${engine}\n`
            : `${engine}\n${engine}\n`,
          stderr: "",
        };
        if (mode === "preflight-malformed-response") response.futureProtocol = true;
        if (mode === "preflight-nonzero") response.exitCode = 17;
        result(request, response);
        return;
      }
      if (request.method === "thread/start") {
        if (mode === "thread-failure") { fail(request); return; }
        if (mode === "mcp-startup-before-thread-response"
            || mode === "mcp-startup-wrong-thread-before-thread-response") {
          const startupThreadId = mode === "mcp-startup-wrong-thread-before-thread-response"
            ? "019f7abc-0000-7000-8000-000000000099"
            : threadId;
          notify("mcpServer/startupStatus/updated", {
            threadId: startupThreadId, name: "north", status: "starting",
            error: null, failureReason: null,
          });
          notify("mcpServer/startupStatus/updated", {
            threadId: startupThreadId, name: "north", status: "ready",
            error: null, failureReason: null,
          });
        }
        result(request, startedThread(request));
        const notificationThread = startedThread(request).thread;
        if (mode === "driver-shape-tolerance") notificationThread.sessionId = threadId;
        if (mode === "thread-id-root-thread-variant") notificationThread.id = threadId;
        if (mode === "thread-id-mismatch-notification")
          notificationThread.id = "019f7abc-0000-7000-8000-000000000099";
        if (mode === "notification-thread-cwd") notificationThread.cwd = root;
        notify("thread/started", { thread: notificationThread });
        emitHook("sessionStart", mode === "hook-session-failed" ? "failed"
          : mode === "hook-session-stopped" ? "stopped" : "completed", "hook-session");
        return;
      }
      if (request.method === "turn/start") {
        if (mode === "continuation-turn-failure" && turnStarts > 0) {
          fail(request);
          return;
        }
        turnInputs.push(String(request.params.input[0].text));
        turnId = turnIds[Math.min(turnStarts, turnIds.length - 1)]!;
        turnStarts += 1;
        const startedTurn: any = turn(turnId, "inProgress");
        if (mode === "turn-id-missing") delete startedTurn.id;
        if (mode === "turn-id-malformed") startedTurn.id = "not a protocol id";
        if (mode === "driver-shape-tolerance")
          startedTurn.futureMetadata = { providerRevision: 2 };
        result(request, {
          turn: startedTurn,
          ...(mode === "driver-shape-tolerance"
            ? { futureMetadata: { providerRevision: 2 } }
            : {}),
        });
        if (mode === "malformed-jsonl") {
          stdout.write("not JSON\n");
          return;
        }
        queueMicrotask(emitRuntime);
        return;
      }
      if (request.method === "turn/interrupt") {
        // A wedged-but-reachable provider still answers its control plane.
        if (mode === "turn-interrupt-refused") { fail(request); return; }
        result(request, {});
        const settleInterruptedTurn = () => {
          notify("thread/tokenUsage/updated", { threadId, turnId, tokenUsage: { total: {
            totalTokens: 1, inputTokens: 1, cachedInputTokens: 0,
            outputTokens: 0, reasoningOutputTokens: 0,
          } } });
          notify("turn/completed", { threadId, turn: turn(turnId, "completed") });
        };
        if (mode === "external-turn-interrupt") settleInterruptedTurn();
        // turn/start already queued its public start notification. Preserve that
        // observable ordering before the replacement interrupt terminal lands.
        if (mode === "respawn-interrupt-gap" || mode === "respawn-interrupt-multigap")
          queueMicrotask(settleInterruptedTurn);
        return;
      }
      fail(request);
    };
    let buffer = "";
    stdin.on("data", (chunk) => {
      buffer += chunk.toString("utf8");
      while (buffer.includes("\n")) {
        const index = buffer.indexOf("\n");
        const line = buffer.slice(0, index);
        buffer = buffer.slice(index + 1);
        if (line) handle(JSON.parse(line));
      }
    });
    let exited = false;
    const exit = (code: number | null, signal: NodeJS.Signals | null) => {
      if (exited) return;
      exited = true;
      child.exitCode = code;
      child.signalCode = signal;
      stdout.end();
      stderr.end();
      queueMicrotask(() => { child.emit("exit", code, signal); child.emit("close", code, signal); });
    };
    stdin.on("finish", () => exit(0, null));
    (child as any).kill = (signal: NodeJS.Signals = "SIGTERM") => { exit(null, signal); return true; };
    queueMicrotask(() => child.emit("spawn"));
    return child;
  }) as any;
  const env = {
    ...process.env,
    CODEX_HOME: codexHome,
    CODEX_SQLITE_HOME: sqliteHome,
    NORTH_BIN: engine,
    PATH: managedPath,
  };
  const options = {
    command: executable,
    testExpectedExecutable: executable,
    useSupervisor: false,
    spawnProcess,
    env,
    cwd,
    prompt: "perform managed work",
    model: "gpt-fixture-exact",
    effort: "high",
    developerInstructions: "bounded developer contract",
    surface: webNetwork
      ? { ...surface, capabilities: [...surface.capabilities, "web"], web: "cached" }
      : surface,
    north,
    timeoutMs: 500,
  };
  return {
    root, codexHome, executable, requests, options,
    turnInputs, attempts: () => attempts,
    replacementPreflightEntered: replacementPreflightEntered.promise,
    releaseReplacementPreflight: () => replacementPreflightRelease.resolve(),
  };
}

test("one app-server proves authority and executes realistic shell/file/MCP traffic", async () => {
  const { options, requests } = setup();
  const run = new ManagedCodexAppServerRun(options);
  const result = await run.execute();
  expect(result).toEqual({
    text: "managed answer",
    providerDurationMs: 1,
    usage: { input_tokens: 9, cached_input_tokens: 4, output_tokens: 3, reasoning_output_tokens: 1 },
    // Counted from the observed item/completed stream this turn emitted:
    // commandExecution + fileChange + mcpToolCall. The agentMessage and the
    // reasoning block are deliberately NOT work items (thread 019f9cc2).
    toolItems: 3,
    providerJoin: {
      version: "north-provider-join:v1",
      sessionKey: providerSessionKey("019f7abc-0000-7000-8000-000000000001"),
      turnKeys: [providerTurnKey("openai", "019f7abc-0000-7000-8000-000000000002")],
      sessionPersistence: "ephemeral",
      coverage: "exact",
    },
  });
  expect(requests.filter(({ method }) => method === "initialize")).toHaveLength(1);
  expect(requests.filter(({ method }) => method === "config/read")).toHaveLength(3);
  expect(requests.filter(({ method }) => method === "hooks/list")).toHaveLength(2);
  expect(requests.filter(({ method }) => method === "mcpServerStatus/list")).toHaveLength(4);
  const preflight = requests.find(({ method }) => method === "command/exec");
  const thread = requests.find(({ method }) => method === "thread/start");
  const turnRequest = requests.find(({ method }) => method === "turn/start");
  expect(preflight.params).toEqual({
    command: ["bash", "--noprofile", "--norc", "-c", NORTH_BINARY_PROBE_SCRIPT],
    processId: null,
    tty: false,
    streamStdin: false,
    streamStdoutStderr: false,
    outputBytesCap: 4_096,
    disableOutputCap: false,
    disableTimeout: false,
    timeoutMs: 5_000,
    cwd: options.cwd,
    env: { PATH: options.env.PATH, NORTH_BIN: options.env.NORTH_BIN },
    size: null,
    sandboxPolicy: { type: "readOnly", networkAccess: false },
    permissionProfile: null,
  });
  expect(thread.params).toEqual({
    model: "gpt-fixture-exact", modelProvider: "openai", approvalPolicy: "never",
    approvalsReviewer: "user", sandbox: "workspace-write",
    config: { model_reasoning_effort: "high" },
    developerInstructions: "bounded developer contract", ephemeral: true,
  });
  expect(turnRequest.params).toEqual({
    threadId: "019f7abc-0000-7000-8000-000000000001",
    input: [{ type: "text", text: "perform managed work" }], effort: "high",
  });
  expect(run.mcpActivity()).toEqual({
    source: "codex-app-server:item-completed", coverage: "exact", totalCalls: 1,
    tools: [{ server: "north", tool: "tell", count: 1 }],
    operationReceipts: [{ tool: "north/tell", operation: "tell", durationMs: 1, resultSize: 23, outcome: "ok" }],
    operationAggregates: [{ operation: "tell", count: 1, totalDurationMs: 1, meanDurationMs: 1, failureCount: 0 }],
  });
  expect(run.nativeCommandActivity()).toMatchObject({
    source: "codex-app-server:item-completed",
    coverage: "exact",
    totalCommands: 1,
    successfulCommands: 1,
    failedCommands: 0,
    declinedCommands: 0,
    northBinaryProbe: "passed",
  });
  expect(JSON.stringify(run.nativeCommandActivity())).not.toContain(NORTH_BINARY_PROBE_SCRIPT);
  expect(JSON.stringify(run.nativeCommandActivity())).not.toContain(options.env.NORTH_BIN);
  expect(JSON.stringify(run.mcpActivity())).not.toContain("CANARY");
});

test("provider input and hook prompts do not inflate managed tool accounting", async () => {
  const { options } = setup("passive-items");
  const result = await new ManagedCodexAppServerRun(options).execute();
  expect(result.toolItems).toBe(3);
});

test("open provider input and hook prompts are not harvested as possible side effects", async () => {
  const { options } = setup("provider-death-open-passive");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({ ...options, maxRespawns: 0 }).execute();
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.pendingItemCount).toBe(0);
  expect(harvest.pendingItems).toEqual([]);
  expect(harvest.landedWork).toBe(false);
});

test("notification processing waits for the event durability callback before advancing", async () => {
  const { options } = setup();
  const admitted = Promise.withResolvers<void>();
  const release = Promise.withResolvers<void>();
  const observed: string[] = [];
  const run = new ManagedCodexAppServerRun({
    ...options,
    async onEvent(method, params) {
      const item = (params as { item?: { type?: unknown } }).item;
      if (item?.type !== "mcpToolCall") return;
      observed.push(method);
      if (method === "item/started") {
        admitted.resolve();
        await release.promise;
      }
    },
  });
  let settled = false;
  const execution = run.execute();
  void execution.then(() => { settled = true; }, () => { settled = true; });

  await admitted.promise;
  await Promise.resolve();
  expect(observed).toEqual(["item/started"]);
  expect(run.mcpActivity().totalCalls ?? 0).toBe(0);
  expect(settled).toBe(false);

  release.resolve();
  await expect(execution).resolves.toMatchObject({ text: "managed answer" });
  expect(observed).toEqual(["item/started", "item/completed"]);
  expect(run.mcpActivity().totalCalls).toBe(1);
});

test("only validated turn, item, command, and MCP execution frames emit liveness", async () => {
  const { options } = setup();
  const activity: string[] = [];
  const run = new ManagedCodexAppServerRun({
    ...options,
    onActivity: (kind) => activity.push(kind),
  });
  await expect(run.execute()).resolves.toMatchObject({ text: "managed answer" });
  expect(activity).toContain("provider.codex.turn.started");
  expect(activity).toContain("provider.codex.item.started");
  expect(activity).toContain("provider.codex.item.completed");
  expect(activity).toContain("provider.codex.command.interaction");
  expect(activity).toContain("provider.codex.mcp.progress");
  expect(activity).toContain("provider.codex.turn.completed");
  expect(activity.some((kind) => /status|rate|startup|hook|token|lease/.test(kind))).toBe(false);
});

test("an app-server JSONL response over 1 MiB survives while malformed JSONL stays fatal", async () => {
  const large = setup("large-agent-message-delta");
  await expect(new ManagedCodexAppServerRun(large.options).execute())
    .resolves.toMatchObject({ text: "managed answer" });

  const malformed = setup("malformed-jsonl");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(malformed.options).execute(); }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(Error);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  expect(causeChain(caught)).toContain("managed Codex emitted malformed JSONL");
  expect(causeChain(caught)).toContain("managed Codex JSONL is invalid JSON");
});

test("native command evidence fails closed for absent, failed, and mismatched probes", async () => {
  for (const [mode, expected] of [
    ["command-none", "not_observed"],
    ["command-failed", "failed"],
    ["command-wrapper-mismatch", "failed"],
    ["command-output-mismatch", "failed"],
  ] as const) {
    const { options } = setup(mode);
    const run = new ManagedCodexAppServerRun(options);
    await expect(run.execute()).resolves.toMatchObject({ text: "managed answer" });
    expect(run.nativeCommandActivity().coverage).toBe("exact");
    expect(run.nativeCommandActivity().northBinaryProbe).toBe(expected);
  }
});

test("model-free shell readiness fails retry-safe before thread or provider turn", async () => {
  for (const mode of [
    "preflight-system-wrapper", "preflight-malformed-response", "preflight-nonzero",
  ]) {
    const { options, requests } = setup(mode);
    await expect(new ManagedCodexAppServerRun(options).execute())
      .rejects.toBeInstanceOf(ManagedCodexPreThreadError);
    expect(requests.filter(({ method }) => method === "command/exec")).toHaveLength(1);
    expect(requests.some(({ method }) => method === "thread/start")).toBe(false);
    expect(requests.some(({ method }) => method === "turn/start")).toBe(false);
  }
});

test("a command that runs outside the turn cwd is work, not a lifecycle defect", async () => {
  // Pre-fix, EVERY one of these killed the lane mid-turn with
  // `openai_provider_execution_failed` (started lifecycle invalid) — the named
  // cause of today's command-heavy managed codex deaths.
  for (const mode of [
    "command-cwd-subdir", "command-cwd-sibling", "command-cwd-scratch",
    "command-cwd-trailing-slash",
  ]) {
    const { options } = setup(mode);
    const run = new ManagedCodexAppServerRun(options);
    await expect(run.execute()).resolves.toMatchObject({ text: "managed answer" });
    const activity = run.nativeCommandActivity();
    expect(activity.coverage).toBe("exact");
    expect(activity.totalCommands).toBe(2);
    expect(activity.successfulCommands).toBe(2);
    // The probe still ran at the lane root, so its evidence survives the
    // widened cwd rule: the accumulator now compares the OBSERVED directory.
    expect(activity.northBinaryProbe).toBe("passed");
  }
});

test("a malformed command cwd still fails closed with the observed path named", async () => {
  for (const mode of [
    "command-cwd-relative", "command-cwd-traversal", "command-cwd-dot",
  ]) {
    const { options } = setup(mode);
    const error = await new ManagedCodexAppServerRun(options).execute()
      .then(() => null, (thrown: Error) => thrown);
    expect(error?.message).toBe("openai_provider_execution_failed");
    const chain: string[] = [];
    for (let cause = error?.cause as Error | undefined; cause; cause = cause.cause as Error | undefined)
      chain.push(`${cause.message}`);
    expect(chain.join(" | ")).toContain("cwd is not an absolute traversal-free path");
    expect(chain.join(" | ")).toContain("observed=");
  }
});

test("native command lifecycle and completion schema fail closed", async () => {
  for (const mode of [
    "command-schema-extra", "command-action-extra", "command-missing-completion",
  ]) {
    const { options } = setup(mode);
    await expect(new ManagedCodexAppServerRun(options).execute())
      .rejects.toThrow("openai_provider_execution_failed");
  }
});

// 0.146 attributes a command to a first-party plugin script. North's contract
// closes `plugins`, so any attribution names an execution path North never sealed.
test("a plugin-attributed native command fails closed and names the attribution", async () => {
  const { options } = setup("command-plugin-attributed");
  const caught = await new ManagedCodexAppServerRun(options).execute()
    .then(() => undefined, (error: Error) => error);
  expect(caught).toBeDefined();
  const text = causeChain(caught!);
  expect(text).toContain("Codex started command execution was attributed to a plugin script");
  expect(text).toContain("first-party");
});

test("launch seals the exact package shell environment policy", () => {
  const { options } = setup();
  const launch = managedCodexAppServerLaunch(options);
  expect(launch.expectedSessionConfig.shell_environment_policy).toEqual({
    inherit: "core",
    set: { PATH: options.env.PATH, NORTH_BIN: options.env.NORTH_BIN },
  });
  expect(launch.args).toContain('shell_environment_policy.inherit="core"');
  expect(launch.args).toContain("allow_login_shell=false");
  expect(launch.args).toContain(
    `shell_environment_policy.set={"NORTH_BIN"=${JSON.stringify(options.env.NORTH_BIN)},"PATH"=${JSON.stringify(options.env.PATH)}}`,
  );
});

test("effective native shell keeps the sealed package environment without manual export", () => {
  const { options, root } = setup();
  const hostileHome = join(root, "hostile-login-home");
  mkdirSync(hostileHome);
  writeFileSync(join(hostileHome, ".bash_profile"), 'PATH="/hostile/login/path"\n');
  const launch = managedCodexAppServerLaunch(options);
  const policy = launch.expectedSessionConfig.shell_environment_policy as {
    inherit: string;
    set: { PATH: string; NORTH_BIN: string };
  };
  expect(launch.expectedSessionConfig.allow_login_shell).toBe(false);
  const command = 'command -v north; printf "%s\\n" "$NORTH_BIN"';
  const shellArgs = launch.expectedSessionConfig.allow_login_shell === false
    ? ["--noprofile", "--norc", "-c", command]
    : ["-lc", command];
  const result = spawnSync(Bun.which("bash")!, shellArgs, {
    cwd: options.cwd,
    env: {
      HOME: hostileHome,
      USER: process.env.USER ?? "north-test",
      SHELL: Bun.which("bash")!,
      ...policy.set,
    },
    encoding: "utf8",
  });
  expect(result.status).toBe(0);
  expect(result.stderr).toBe("");
  expect(result.stdout.trim().split("\n")).toEqual([
    options.env.NORTH_BIN,
    options.env.NORTH_BIN,
  ]);
});

test("real expanded effective shell policy is accepted without widening the session request", async () => {
  const { options, requests } = setup();
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
  expect(requests.some(({ method }) => method === "thread/start")).toBe(true);
});

test("a first-result consumer sees exact MCP activity without resuming the session", async () => {
  const { options } = setup();
  const run = new ManagedCodexAppServerRun(options);
  const session = run.session(async () => undefined);
  const first = await session.next();
  expect(first.done).toBe(false);
  expect(run.mcpActivity()).toEqual({
    source: "codex-app-server:item-completed", coverage: "exact", totalCalls: 1,
    tools: [{ server: "north", tool: "tell", count: 1 }],
    operationReceipts: [{ tool: "north/tell", operation: "tell", durationMs: 1, resultSize: 23, outcome: "ok" }],
    operationAggregates: [{ operation: "tell", count: 1, totalDurationMs: 1, meanDurationMs: 1, failureCount: 0 }],
  });
  await session.return(first.value!);
  expect(run.mcpActivity().coverage).toBe("exact");
});

test("a clean terminal with MCP identity loss settles as partial", async () => {
  const { options } = setup("mcp-identity-loss");
  const run = new ManagedCodexAppServerRun(options);
  await expect(run.execute()).resolves.toMatchObject({ text: "managed answer" });
  expect(run.mcpActivity()).toEqual({
    source: "codex-app-server:item-completed", coverage: "partial", totalCalls: 1,
    tools: [], operationReceipts: [], operationAggregates: [],
  });
});

test("thread-scoped MCP startup may precede the thread/start response", async () => {
  const { options } = setup("mcp-startup-before-thread-response");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toMatchObject({
    text: "managed answer",
    usage: { input_tokens: 9, cached_input_tokens: 4, output_tokens: 3, reasoning_output_tokens: 1 },
  });
});

test("queued MCP startup remains bound to the exact thread/start response", async () => {
  const { options } = setup("mcp-startup-wrong-thread-before-thread-response");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(options).execute(); } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(Error);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  expect((caught as Error).cause).toBeInstanceOf(Error);
  expect(((caught as Error).cause as Error).message)
    .toContain("expected threadId null or \"019f7abc-0000-7000-8000-000000000001\"");
  expect(((caught as Error).cause as Error).message)
    .toContain("\"threadId\":\"019f7abc-0000-7000-8000-000000000099\"");
});

test("pinned safety-buffering notifications accept bounded, nullable, or omitted fasterModel", async () => {
  for (const mode of [
    "safety-buffering-valid", "safety-buffering-without-faster-model",
    "safety-buffering-with-faster-model",
  ]) {
    const { options } = setup(mode);
    await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toMatchObject({
      text: "managed answer",
      usage: { input_tokens: 9, cached_input_tokens: 4, output_tokens: 3, reasoning_output_tokens: 1 },
    });
  }
});

test("hostile safety-buffering payloads fail closed", async () => {
  const modes = [
    "safety-buffering-extra", "safety-buffering-wrong-thread",
    "safety-buffering-wrong-turn", "safety-buffering-wrong-model",
    "safety-buffering-missing-reasons", "safety-buffering-invalid-reason",
    "safety-buffering-too-many-reasons", "safety-buffering-oversized-use-case",
    "safety-buffering-invalid-ui", "safety-buffering-invalid-faster-model",
  ];
  for (const mode of modes) {
    const { options } = setup(mode);
    let caught: unknown;
    try { await new ManagedCodexAppServerRun(options).execute(); } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(Error);
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
  }
});

test("a later North frame drives a same-thread continuation turn under re-proven authority", async () => {
  const { options, requests } = setup();
  const reduction = "reconcile the settled child lanes into the parent thread";
  const later: Array<string | undefined> = [reduction];
  const run = new ManagedCodexAppServerRun(options);
  const settlements: Array<{ text: string; usage: unknown }> = [];
  // First frame is the launch prompt; `nextInput` supplies exactly one later
  // frame, then settles the session.
  for await (const turnResult of run.session(async () => later.shift())) {
    settlements.push(turnResult);
  }
  const expected = (turnId: string) => ({
    text: "managed answer",
    providerDurationMs: 1,
    usage: { input_tokens: 9, cached_input_tokens: 4, output_tokens: 3, reasoning_output_tokens: 1 },
    // Per-TURN work-item count, reset at each turn start — never cumulative
    // here; the adapter sums it across turns for the run record.
    toolItems: 3,
    providerJoin: {
      version: "north-provider-join:v1",
      sessionKey: providerSessionKey("019f7abc-0000-7000-8000-000000000001"),
      turnKeys: [providerTurnKey("openai", turnId)],
      sessionPersistence: "ephemeral",
      coverage: "exact",
    },
  });
  // One terminal result per consumed frame.
  expect(settlements).toEqual([
    expected("019f7abc-0000-7000-8000-000000000002"),
    expected("019f7abc-0000-7000-8000-000000000003"),
  ]);
  expect(run.mcpActivity()?.totalCalls).toBe(2);
  expect(run.nativeCommandActivity()).toMatchObject({
    coverage: "exact", totalCommands: 2, successfulCommands: 2,
    northBinaryProbe: "passed",
  });
  expect(run.nativeCommandActivity().completions).toHaveLength(2);

  // Exactly one provider thread, two turns bound to it.
  expect(requests.filter(({ method }) => method === "thread/start")).toHaveLength(1);
  const turnStarts = requests.filter(({ method }) => method === "turn/start");
  expect(turnStarts).toHaveLength(2);
  expect(turnStarts.map((request) => request.params.threadId)).toEqual([
    "019f7abc-0000-7000-8000-000000000001",
    "019f7abc-0000-7000-8000-000000000001",
  ]);
  // The continuation turn consumed the LATER North frame, not a replay of the
  // launch prompt.
  expect(turnStarts[0].params.input).toEqual([{ type: "text", text: "perform managed work" }]);
  expect(turnStarts[1].params.input).toEqual([{ type: "text", text: reduction }]);

  // The session initializes once but re-proves the exact authority surface on
  // every turn: web-disabled config, hook set, and MCP tool grant are all
  // re-read pre-turn and the config fingerprint is re-attested post-turn.
  expect(requests.filter(({ method }) => method === "initialize")).toHaveLength(1);
  expect(requests.filter(({ method }) => method === "config/read")).toHaveLength(5);
  expect(requests.filter(({ method }) => method === "hooks/list")).toHaveLength(3);
  expect(requests.filter(({ method }) => method === "mcpServerStatus/list")).toHaveLength(6);
});

test("public turn interruption retains the provider thread for a later turn", async () => {
  const { options, requests } = setup("external-turn-interrupt");
  const later: Array<string | undefined> = ["continue after interrupt"];
  const run = new ManagedCodexAppServerRun(options);
  const session = run.session(async () => later.shift());
  const first = session.next();
  const deadline = Date.now() + 2_000;
  while (!requests.some(({ method }) => method === "turn/start") && Date.now() < deadline)
    await Bun.sleep(5);
  expect(requests.some(({ method }) => method === "turn/start")).toBe(true);

  await run.interruptTurn();
  expect((await first).done).toBe(false);
  expect((await session.next()).done).toBe(false);
  expect((await session.next()).done).toBe(true);

  expect(requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(1);
  expect(requests.filter(({ method }) => method === "thread/start")).toHaveLength(1);
  const starts = requests.filter(({ method }) => method === "turn/start");
  expect(starts).toHaveLength(2);
  expect(starts.map(({ params }) => params.threadId)).toEqual([
    "019f7abc-0000-7000-8000-000000000001",
    "019f7abc-0000-7000-8000-000000000001",
  ]);
});

test("public turn interruption maps a provider refusal to a stable generic error", async () => {
  const { options, requests } = setup("turn-interrupt-refused");
  const run = new ManagedCodexAppServerRun(options);
  const execution = run.execute();
  void execution.catch(() => {});
  const deadline = Date.now() + 2_000;
  while (!requests.some(({ method }) => method === "turn/start") && Date.now() < deadline)
    await Bun.sleep(5);
  expect(requests.some(({ method }) => method === "turn/start")).toBe(true);
  await expect(run.interruptTurn()).rejects.toThrow("provider_turn_interrupt_failed");
  await run.interrupt();
  await expect(execution).rejects.toBeInstanceOf(ManagedCodexHarvestError);
});

test("an admitted continuation makes MCP coverage unknown until its terminal succeeds", async () => {
  const { options } = setup("continuation-turn-failure");
  const run = new ManagedCodexAppServerRun(options);
  const session = run.session(async () => "continue after the first terminal");
  const first = await session.next();
  expect(first.done).toBe(false);
  expect(run.mcpActivity().coverage).toBe("exact");

  await expect(session.next()).rejects.toThrow("openai_provider_execution_failed");
  expect(run.mcpActivity()).toEqual({
    source: "codex-app-server:item-completed", coverage: "unknown", tools: [], operationReceipts: [], operationAggregates: [],
  });
});

test("a continuation turn that widens config authority fails closed", async () => {
  // `fingerprint-mutation` mutates the sessionFlags layer version on the 2nd+
  // config/read. The launch turn's pre-turn re-read (configReads>1) already
  // trips it, so the very first turn fails and no continuation is served.
  const { options, requests } = setup("fingerprint-mutation");
  const run = new ManagedCodexAppServerRun(options);
  let caught: unknown;
  try {
    for await (const _ of run.session(async () => "a later frame")) { /* unreachable */ }
  } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(Error);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  // The authority regression is caught before a second turn can start.
  expect(requests.filter(({ method }) => method === "turn/start").length).toBeLessThanOrEqual(1);
});

function supervisedStatusChild(drive: (stderr: PassThrough) => void): any {
  return (() => {
    const child = new EventEmitter() as ChildProcessWithoutNullStreams & {
      exitCode: number | null; signalCode: NodeJS.Signals | null;
    };
    const stdin = new PassThrough();
    const stdout = new PassThrough();
    const stderr = new PassThrough();
    Object.assign(child, {
      stdin, stdout, stderr, stdio: [stdin, stdout, stderr],
      pid: 5100, exitCode: null, signalCode: null, killed: false,
    });
    let exited = false;
    const exit = (code: number | null, signal: NodeJS.Signals | null) => {
      if (exited) return;
      exited = true;
      child.exitCode = code;
      child.signalCode = signal;
      try { stdout.end(); } catch { /* already closed */ }
      queueMicrotask(() => { child.emit("exit", code, signal); child.emit("close", code, signal); });
    };
    stdin.on("finish", () => exit(null, "SIGTERM"));
    (child as any).kill = (signal: NodeJS.Signals = "SIGTERM") => { exit(null, signal); return true; };
    queueMicrotask(() => { child.emit("spawn"); drive(stderr); });
    return child;
  }) as any;
}

test("a supervised launch whose status channel closes before STARTED fails preflight loud", async () => {
  const { options } = setup();
  const run = new ManagedCodexAppServerRun({
    ...options,
    useSupervisor: true,
    spawnProcess: supervisedStatusChild((stderr) => { stderr.end(); }),
  });
  let caught: unknown;
  try { await run.execute(); } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  expect((caught as Error).message).toBe("openai_codex_authority_preflight_failed");
  expect((caught as Error).cause).toBeInstanceOf(Error);
  expect(((caught as Error).cause as Error).message)
    .toBe("Codex supervisor closed before authority preflight");
});

test("a supervised launch reads UNAVAILABLE off the supervisor stderr status channel", async () => {
  const { options } = setup();
  const run = new ManagedCodexAppServerRun({
    ...options,
    useSupervisor: true,
    spawnProcess: supervisedStatusChild((stderr) => {
      stderr.write(`${codexSupervisorStatusLine("UNAVAILABLE")}\n`);
    }),
  });
  let caught: unknown;
  try { await run.execute(); } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  expect((caught as Error).message).toBe("openai_codex_authority_preflight_failed");
  expect(((caught as Error).cause as Error).message).toBe("Codex executable unavailable");
});

test("the production duplex supervisor carries RPC and bounds host-EOF cleanup", async () => {
  const supervisor = join(import.meta.dir, "../src/providers/codex-supervisor.ts");
  const fixture = join(import.meta.dir, "fixtures/fake-codex-app-server.mjs");
  const controlRoot = mkdtempSync(join(tmpdir(), "north-codex-control-test-"));
  roots.push(controlRoot);
  const child = spawn(process.execPath, [
    supervisor, "--duplex", controlRoot, process.execPath, fixture,
  ], {
    env: {
      ...process.env,
      NORTH_MKFIFO_BIN: realpathSync(Bun.which("mkfifo")!),
      FAKE_CODEX_RESPONSES: JSON.stringify({ probe: { transport: "exact" } }),
    },
    stdio: ["pipe", "pipe", "pipe"],
  }) as ChildProcessWithoutNullStreams;
  const status = child.stderr;
  let output = "";
  child.stdout.on("data", (chunk) => { output += chunk.toString(); });
  try {
    expect(await firstLine(status, "Codex supervisor start receipt"))
      .toBe(codexSupervisorStatusLine("STARTED"));
    const firstTemporary = join(controlRoot, ".000000000001.test.tmp");
    writeAtomicSupervisorFrame(
      firstTemporary, `${JSON.stringify({ id: 1, method: "probe", params: {} })}\n`,
    );
    const secondTemporary = join(controlRoot, ".000000000002.test.tmp");
    writeAtomicSupervisorFrame(
      secondTemporary, `${JSON.stringify({ id: 2, method: "probe", params: {} })}\n`,
    );
    renameSync(secondTemporary, join(controlRoot, "000000000002.req"));
    await Bun.sleep(40);
    expect(output).toBe("");
    renameSync(firstTemporary, join(controlRoot, "000000000001.req"));
    const deadline = Date.now() + 2_000;
    while (output.trim().split("\n").filter(Boolean).length < 2 && Date.now() < deadline)
      await Bun.sleep(10);
    expect(output.trim().split("\n").map((line) => JSON.parse(line))).toEqual([
      { id: 1, result: { transport: "exact" } },
      { id: 2, result: { transport: "exact" } },
    ]);
  } finally {
    child.stdin.end();
    const closed = await Promise.race([
      new Promise<boolean>((resolveClose) => child.once("close", () => resolveClose(true))),
      new Promise<boolean>((resolveClose) => setTimeout(() => resolveClose(false), 3_000)),
    ]);
    if (!closed) child.kill("SIGKILL");
    expect(closed).toBe(true);
  }
  expect(existsSync(controlRoot)).toBe(false);
}, 5_000);

test("the one-shot supervisor transfers an exact bounded prompt without argv or env exposure", async () => {
  if (process.platform === "win32") return;
  const root = mkdtempSync(join(tmpdir(), "north-codex-oneshot-test-"));
  roots.push(root);
  const controlRoot = mkdtempSync(join(root, "control-"));
  const supervisor = join(import.meta.dir, "../src/providers/codex-supervisor.ts");
  const provider = join(root, "provider.mjs");
  const prompt = "exact prompt\nwith unicode 🧭 and a NUL \u0000 tail";
  writeFileSync(provider, `
const canary = ${JSON.stringify(prompt)};
if (process.argv.some((value) => value.includes(canary))
    || Object.values(process.env).some((value) => value?.includes(canary))) process.exit(41);
const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
process.stdout.write(Buffer.concat(chunks));
`);
  const child = spawn(process.execPath, [
    supervisor, "--oneshot-spool", controlRoot, process.execPath, provider,
  ], {
    env: { ...process.env, NORTH_MKFIFO_BIN: realpathSync(Bun.which("mkfifo")!) },
    stdio: ["pipe", "pipe", "pipe"],
  }) as ChildProcessWithoutNullStreams;
  const status = child.stderr;
  const output: Buffer[] = [];
  child.stdout.on("data", (chunk) => output.push(Buffer.from(chunk)));
  try {
    expect(await firstLine(status, "Codex one-shot start receipt"))
      .toBe(codexSupervisorStatusLine("STARTED"));
    const temporary = join(controlRoot, ".000000000001.test.tmp");
    writeAtomicSupervisorFrame(temporary, prompt);
    renameSync(temporary, join(controlRoot, "000000000001.req"));
    const closed = await Promise.race([
      new Promise<boolean>((resolveClose) => child.once("close", () => resolveClose(true))),
      new Promise<boolean>((resolveClose) => setTimeout(() => resolveClose(false), 3_000)),
    ]);
    expect(closed).toBe(true);
    expect(child.exitCode).toBe(0);
    expect(Buffer.concat(output).toString("utf8")).toBe(prompt);
    expect(existsSync(controlRoot)).toBe(false);
  } finally {
    try { child.stdin.end(); } catch {}
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
}, 5_000);

test("the duplex supervisor rejects symlinked, oversized, corrupt, and over-permissive frames", async () => {
  const supervisor = join(import.meta.dir, "../src/providers/codex-supervisor.ts");
  const fixture = join(import.meta.dir, "fixtures/fake-codex-app-server.mjs");
  for (const mode of ["symlink", "oversized", "corrupt", "permissions"] as const) {
    const controlRoot = mkdtempSync(join(tmpdir(), "north-codex-control-invalid-"));
    roots.push(controlRoot);
    const child = spawn(process.execPath, [
      supervisor, "--duplex", controlRoot, process.execPath, fixture,
    ], {
      env: {
        ...process.env,
        NORTH_MKFIFO_BIN: realpathSync(Bun.which("mkfifo")!),
        FAKE_CODEX_RESPONSES: JSON.stringify({ probe: { transport: "must-not-run" } }),
      },
      stdio: ["pipe", "pipe", "pipe"],
    }) as ChildProcessWithoutNullStreams;
    child.stdout.resume();
    const closed = new Promise<boolean>((resolveClose) => child.once("close", () => resolveClose(true)));
    try {
      const status = child.stderr;
      expect(await firstLine(status, `Codex ${mode} start receipt`))
        .toBe(codexSupervisorStatusLine("STARTED"));
      const request = join(controlRoot, "000000000001.req");
      if (mode === "symlink") {
        writeFileSync(join(controlRoot, "target"), "hostile\n", { mode: 0o600 });
        symlinkSync("target", request);
      } else if (mode === "oversized") {
        writeFileSync(request, Buffer.alloc(1024 * 1024 + 1), { mode: 0o600 });
      } else if (mode === "corrupt") {
        writeFileSync(request, supervisorFrame("hostile\n", "0".repeat(64)), { mode: 0o600 });
      } else {
        writeFileSync(request, "hostile\n", { mode: 0o644 });
      }
      const didClose = await Promise.race([
        closed,
        new Promise<boolean>((resolveClose) => setTimeout(() => resolveClose(false), 3_000)),
      ]);
      expect(didClose).toBe(true);
      expect(existsSync(controlRoot)).toBe(false);
    } finally {
      try { child.stdin.end(); } catch {}
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
    }
  }
}, 12_000);

test("kernel host EOF reaps a non-reading provider group within the derived bound", async () => {
  if (process.platform === "win32") return;
  const root = mkdtempSync(join(tmpdir(), "north-codex-host-death-"));
  roots.push(root);
  const controlRoot = mkdtempSync(join(root, "control-"));
  const supervisor = join(import.meta.dir, "../src/providers/codex-supervisor.ts");
  const provider = join(root, "provider.mjs");
  const hostScript = join(root, "host.mjs");
  const supervisorPidPath = join(root, "supervisor.pid");
  const providerPidPath = join(root, "provider.pid");
  const descendantPidPath = join(root, "descendant.pid");
  writeFileSync(provider, `
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
writeFileSync(${JSON.stringify(providerPidPath)}, String(process.pid));
process.on("SIGTERM", () => {});
const descendant = spawn(process.execPath, ["-e",
  'process.on("SIGTERM",()=>{}); setInterval(()=>{}, 1000)'], { stdio: "ignore" });
writeFileSync(${JSON.stringify(descendantPidPath)}, String(descendant.pid));
setInterval(() => {}, 1000);
`);
  writeFileSync(hostScript, `
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
const supervisor = spawn(process.execPath, ${JSON.stringify([
    supervisor, "--duplex", controlRoot, process.execPath, provider,
  ])}, { env: process.env, stdio: ["pipe", "ignore", "ignore"] });
writeFileSync(${JSON.stringify(supervisorPidPath)}, String(supervisor.pid));
setInterval(() => {}, 1000);
`);
  const host = spawn(process.execPath, [hostScript], {
    env: { ...process.env, NORTH_MKFIFO_BIN: realpathSync(Bun.which("mkfifo")!) },
    stdio: "ignore",
  });
  let supervisorPid: number | undefined;
  let providerPid: number | undefined;
  let descendantPid: number | undefined;
  try {
    await Promise.all([
      waitForFile(supervisorPidPath), waitForFile(providerPidPath), waitForFile(descendantPidPath),
    ]);
    supervisorPid = Number(readFileSync(supervisorPidPath, "utf8"));
    providerPid = Number(readFileSync(providerPidPath, "utf8"));
    descendantPid = Number(readFileSync(descendantPidPath, "utf8"));
    const temporary = join(controlRoot, ".000000000001.test.tmp");
    writeAtomicSupervisorFrame(temporary, "x".repeat(1024 * 1024));
    renameSync(temporary, join(controlRoot, "000000000001.req"));
    await Bun.sleep(50);

    const startedAt = Date.now();
    host.kill("SIGKILL");
    await new Promise<void>((resolveClose) => host.once("close", () => resolveClose()));
    await Promise.all([
      waitForProcessGone(supervisorPid, 3_000),
      waitForProcessGone(providerPid, 3_000),
      waitForProcessGone(descendantPid, 3_000),
    ]);
    expect(Date.now() - startedAt).toBeLessThan(2_750);
    expect(existsSync(controlRoot)).toBe(false);
  } finally {
    killProcess(host.pid);
    killProcess(supervisorPid);
    killProcess(providerPid, true);
    killProcess(descendantPid);
  }
}, 8_000);

test("spooled supervisors build their control FIFO with a canonical store coreutils", () => {
  const supervisor = join(import.meta.dir, "../src/providers/codex-supervisor.ts");
  const fixture = join(import.meta.dir, "fixtures/fake-codex-app-server.mjs");
  const spawnSupervisor = (inputMode: string, mkfifo: string | undefined) => {
    const controlRoot = mkdtempSync(join(tmpdir(), "north-codex-control-mkfifo-"));
    roots.push(controlRoot);
    const env = { ...process.env, NORTH_MKFIFO_BIN: mkfifo };
    if (mkfifo === undefined) delete env.NORTH_MKFIFO_BIN;
    const child = spawnSync(process.execPath, [
      supervisor, inputMode, controlRoot, process.execPath, fixture,
    ], {
      env,
      encoding: "utf8",
      stdio: ["ignore", "ignore", "pipe"],
      timeout: 5_000,
    });
    expect(child.error).toBeUndefined();
    expect(child.signal).toBeNull();
    return { child, controlRoot };
  };
  for (const inputMode of ["--duplex", "--oneshot-spool"]) {
    // A forged, writable "mkfifo" is never executed — it fails the canonical
    // /nix/store proof and is SKIPPED, so a store coreutils still builds the
    // FIFO. (If the shim had been trusted it could not create one at all, and
    // the receipt would be UNAVAILABLE. Fail-closed when NOTHING resolves is
    // covered at the resolver in trusted-runtime.test.ts, which is the only
    // place a NixOS host can have an empty ladder.)
    // Absent injection is likewise NOT absent trust: a checkout-driven managed
    // lane never inherits NORTH_MKFIFO_BIN, and failing closed on that absence
    // is what killed those lanes at authority preflight.
    for (const pointer of [fixture, undefined]) {
      const { child } = spawnSupervisor(inputMode, pointer);
      expect(child.stderr.split("\n")[0]!.trim())
        .toBe(codexSupervisorStatusLine("STARTED"));
    }
  }
});

test("pre-thread authority mutants fail before thread/start", async () => {
  const modes = [
    "runtime-version", "runtime-version-prefix", "runtime-version-suffix",
    "notification-emitted-at-negative", "notification-emitted-at-string",
    "notification-envelope-extra",
    "project-enabled", "hook-warning", "hook-failure-unattested",
    "hook-failure-continue", "hook-failure-unrecognized",
    "feature-default-enabled", "feature-omitted", "mcp-resource", "mcp-template", "mcp-auth",
    "shell-policy-missing", "shell-policy-wrong-inherit", "shell-policy-drift",
    "shell-policy-extra-key", "shell-policy-filters-set", "login-shell-enabled",
    "mcp-server-info", "remote-enabled", "remote-extra-field", "remote-missing-installation",
    "deprecation-extra-field", "server-request-prethread",
    "config-warning-wrong-identifiers",
  ];
  for (const mode of modes) {
    const { options, requests } = setup(mode);
    await expect(new ManagedCodexAppServerRun(options).execute())
      .rejects.toBeInstanceOf(ManagedCodexPreThreadError);
    expect(requests.some(({ method }) => method === "thread/start")).toBe(false);
  }
});

test("the managed feature manifest covers the Codex 0.146 additions", () => {
  expect(MANAGED_CODEX_VERSION).toBe("0.146.0");
  expect(MANAGED_CODEX_DISABLED_FEATURES).toEqual(expect.arrayContaining([
    "code_mode_buffered_exec",
    "deferred_tool_world_state",
    "executor_capability_discovery",
    "external_agent_memory_import",
    "guardianv2",
    "in_app_updates",
    "mcp_2026_07_28",
    "skill_search",
  ]));
});

test("a notification may omit its provider emission timestamp", async () => {
  const { options, requests } = setup("notification-emitted-at-omitted");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
  expect(requests.some(({ method }) => method === "thread/start")).toBe(true);
});

test("the canonical untrusted-project config warning is accepted before thread/start", async () => {
  const { options, requests } = setup("config-warning");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
  expect(requests.some(({ method }) => method === "thread/start")).toBe(true);
});

test("provider-revision IDs and cosmetic fields are tolerated at all four driver seams", async () => {
  const { options, requests } = setup("driver-shape-tolerance");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
  expect(requests.some(({ method }) => method === "thread/start")).toBe(true);
  expect(requests.some(({ method }) => method === "turn/start")).toBe(true);

  const rootThreadVariant = setup("thread-id-root-thread-variant");
  await expect(new ManagedCodexAppServerRun(rootThreadVariant.options).execute())
    .resolves.toBeDefined();

  for (const mode of [
    "thread-extra-authority",
    "notification-turn-extra",
    "config-warning-drift",
  ]) {
    const variant = setup(mode);
    await expect(new ManagedCodexAppServerRun(variant.options).execute()).resolves.toBeDefined();
  }
});

// 0.146 hydrates a completed turn's items as a summarized view. North admits
// the observed vocabulary ("notLoaded", "summary") and any other view still
// names the drift and fails closed.
test("a summarized completed-turn items view is tolerated; other views fail closed", async () => {
  const { options } = setup("turn-summary-items-view");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();

  const hostile = setup("turn-items-view-hostile");
  const caught = await new ManagedCodexAppServerRun(hostile.options).execute()
    .then(() => undefined, (error: Error) => error);
  expect(caught).toBeDefined();
  expect(causeChain(caught!)).toContain('itemsView "fullyLoaded"');
});

test("missing, malformed, and mismatched driver identities fail closed", async () => {
  for (const mode of [
    "thread-id-missing",
    "thread-id-malformed",
    "thread-id-mismatch-notification",
    "turn-id-missing",
    "turn-id-malformed",
    "turn-id-mismatch-notification",
    "config-warning-wrong-identifiers",
  ]) {
    const { options } = setup(mode);
    await expect(new ManagedCodexAppServerRun(options).execute()).rejects.toBeInstanceOf(Error);
  }
});

test("the untrusted-project warning is rooted at the Git project, not a nested cwd", async () => {
  const { options, requests } = setup("nested-project-warning");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
  expect(options.cwd.endsWith("/sdk")).toBe(true);
  expect(requests.some(({ method }) => method === "thread/start")).toBe(true);
});

test("tracked project config remains inert while the exact layer is untrusted", async () => {
  const { options, requests } = setup("project-disabled-tracked");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
  expect(requests.some(({ method }) => method === "thread/start")).toBe(true);

  const missing = setup("project-disabled-no-warning");
  await expect(new ManagedCodexAppServerRun(missing.options).execute())
    .rejects.toBeInstanceOf(ManagedCodexPreThreadError);
  expect(missing.requests.some(({ method }) => method === "thread/start")).toBe(false);
});

test("a populated global profile is inert only behind every disabled-project proof", async () => {
  const accepted = setup("project-disabled-global-profile");
  await expect(new ManagedCodexAppServerRun(accepted.options).execute()).resolves.toBeDefined();
  expect(accepted.requests.some(({ method }) => method === "thread/start")).toBe(true);
  expect(accepted.requests.some(({ method }) => method === "turn/start")).toBe(true);

  // A path-correlated warning does not make an enabled layer harmless. The
  // structured layer state itself must independently say the payload is
  // disabled.
  const enabled = setup("project-global-profile-enabled-with-warning");
  const enabledError = await new ManagedCodexAppServerRun(enabled.options).execute()
    .then(() => undefined, (error: Error) => error);
  expect(enabledError).toBeInstanceOf(ManagedCodexPreThreadError);
  expect(causeChain(enabledError!))
    .toContain("Codex populated project layer lacks its exact structured disabled reason");
  expect(enabled.requests.some(({ method }) => method === "thread/start")).toBe(false);

  // Codex explicitly says skills still load for an untrusted project. That key
  // is therefore live authority, not inert evidence, and must remain denied.
  const unknown = setup("project-disabled-unknown");
  const unknownError = await new ManagedCodexAppServerRun(unknown.options).execute()
    .then(() => undefined, (error: Error) => error);
  expect(unknownError).toBeInstanceOf(ManagedCodexPreThreadError);
  const unknownChain = causeChain(unknownError!);
  expect(unknownChain).toContain("Codex disabled project config widened authority: skills");
  expect(unknown.requests.some(({ method }) => method === "thread/start")).toBe(false);

  // Disabled-layer metadata is insufficient if any reviewed value leaks into
  // effective authority. The existing exact feature proof remains decisive.
  const widened = setup("project-disabled-global-profile-effective-widened");
  const widenedError = await new ManagedCodexAppServerRun(widened.options).execute()
    .then(() => undefined, (error: Error) => error);
  expect(widenedError).toBeInstanceOf(ManagedCodexPreThreadError);
  expect(causeChain(widenedError!))
    .toContain("Codex effective feature set does not match North's exact managed Codex contract");
  expect(widened.requests.some(({ method }) => method === "thread/start")).toBe(false);
});

test("a non-empty user layer is refused and NAMES what it carries", async () => {
  // Same defect class as the widened project layer: the contract is "this layer
  // holds nothing", and `exact(layer, {})` reported only that it differed — the
  // one fact a reader needs is WHICH key appeared. Keys only; a value may be a
  // token, an option name never is.
  const { options, requests } = setup("user-layer-nonempty");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun(options).execute();
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  const text = String((caught as Error).cause ?? caught);
  expect(text).toContain("Codex user layer must be empty but carries");
  expect(text).toContain("approval_policy");
  expect(text).toContain("model");
  // The VALUES must never reach the message.
  expect(text).not.toContain("gpt-5.6-sol");
  expect(text).not.toContain("never");
  expect(requests.some(({ method }) => method === "thread/start")).toBe(false);
});

test("a non-empty system layer is refused and NAMES what it carries", async () => {
  const { options } = setup("system-layer-nonempty");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun(options).execute();
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  const text = String((caught as Error).cause ?? caught);
  expect(text).toContain("Codex system layer must be empty but carries");
  expect(text).toContain("sandbox_mode");
  expect(text).not.toContain("danger-full-access");
});

test("a drifted shell environment policy NAMES the field, never its value", async () => {
  for (const [mode, field] of [
    ["shell-policy-extra-key", "future_authority"],
    ["shell-policy-filters-set", "filters"],
    ["shell-policy-wrong-inherit", "inherit"],
  ] as const) {
    const { options } = setup(mode);
    const caught = await new ManagedCodexAppServerRun(options).execute()
      .then(() => undefined, (error: Error) => error);
    expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
    const text = causeChain(caught!);
    expect(text)
      .toContain("Codex effective shell environment policy does not match North's exact managed");
    expect(text).toContain(field);
    // Keys only: the environment map's contents never reach the message.
    expect(text).not.toContain("/run/current-system/sw/bin");
  }
});

test("every security-relevant thread/start response field is attested independently", async () => {
  const modes = [
    "thread-model", "thread-provider", "thread-service-tier", "thread-cwd", "thread-roots",
    "thread-sources", "thread-approval", "thread-reviewer", "thread-sandbox", "thread-profile",
    "thread-effort", "thread-multi-agent", "thread-ephemeral", "thread-object-provider",
    "thread-object-cwd",
    "thread-roots-drop-grant", "thread-roots-widened",
  ];
  for (const mode of modes) {
    const { options, requests } = setup(mode);
    let caught: unknown;
    try { await new ManagedCodexAppServerRun(options).execute(); } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(Error);
    expect(caught).not.toBeInstanceOf(ManagedCodexPreThreadError);
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
    expect(requests.some(({ method }) => method === "turn/start")).toBe(false);
  }
});

test("runtime workspace roots are a SET: the granted roots in any order are accepted", async () => {
  // Root order is not authority. Fixing the grant/assertion drift must not
  // replace it with an order dependency on however Codex happens to echo them.
  const { options } = setup("thread-roots-reordered");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
});

test("a workspace-roots mismatch names the observed and expected sets on its cause", async () => {
  // The 2026-07-26 forensic hole: the failure said only WHICH check failed, so
  // the drift cost a live reproduction to see. The cause must carry both sides.
  const { options } = setup("thread-roots-drop-grant");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(options).execute(); } catch (error) { caught = error; }
  const chain = causeChain(caught);
  expect(chain).toContain("Codex thread runtime workspace roots does not match");
  expect(chain).toContain("observed=");
  expect(chain).toContain("expected=");
});

test("a provider-side failure names its payload on the cause, not just the classification", async () => {
  // thread 019f9cec: the two seams where the provider itself says what went
  // wrong — a JSON-RPC error response and a turn that reports its own error —
  // both parsed the payload and then threw it away. Every managed lane that
  // died this way (three on 2026-07-26) left no account of the failure at all.
  for (const [mode, expected] of [
    ["thread-failure", "provider error response:"],
    ["notification-terminal-error", "provider turn error:"],
  ] as const) {
    const { options } = setup(mode);
    let caught: unknown;
    try { await new ManagedCodexAppServerRun(options).execute(); } catch (error) { caught = error; }
    const chain = causeChain(caught);
    expect(chain).toContain("openai_provider_execution_failed");
    expect(chain).toContain(expected);
    expect(chain).toContain(mode === "thread-failure" ? "fixture failure" : "hidden failure");
  }
});

test("a PreToolUse policy block is not a managed-lane failure", async () => {
  const { options } = setup("hook-pretool-blocked");
  await expect(new ManagedCodexAppServerRun(options).execute())
    .resolves.toMatchObject({ text: "managed answer" });
});

test("post-thread drift, rejection, malformed traffic, and hook failures are never retry-safe", async () => {
  const modes = [
    "fingerprint-mutation", "thread-failure", "notification-wrong-thread", "notification-malformed",
    "notification-thread-cwd", "notification-terminal-error",
    "hook-session-failed", "hook-session-stopped", "hook-pretool-failed",
    "hook-pretool-stopped", "hook-posttool-stopped", "hook-posttool-failed",
    "hook-missing-completion", "hook-duplicate-completion", "hook-session-invalid-turn",
    "hook-session-scope", "hook-tool-null-turn", "hook-tool-thread-scope",
  ];
  for (const mode of modes) {
    const { options } = setup(mode);
    let caught: unknown;
    try { await new ManagedCodexAppServerRun(options).execute(); } catch (error) { caught = error; }
    expect(caught).toBeInstanceOf(Error);
    expect(caught).not.toBeInstanceOf(ManagedCodexPreThreadError);
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
  }
});

test("hook completion pairs with its start by run id despite summary drift", async () => {
  const { options } = setup("hook-completion-summary-drift");
  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toBeDefined();
});

function expectLaunchPreflightFailure(
  options: ReturnType<typeof setup>["options"],
  code: string,
): ManagedCodexPreThreadError {
  const remoteControlBefore = options.env.CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED;
  let caught: unknown;
  try { managedCodexAppServerLaunch(options); } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  expect((caught as Error).message).toBe(code);
  expect(options.env.CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED)
    .toBe(remoteControlBefore);
  return caught as ManagedCodexPreThreadError;
}

test("every launch preflight cause has stable diagnosis and fails before process authority", () => {
  for (const missing of ["CODEX_HOME", "CODEX_SQLITE_HOME"] as const) {
    const { options, requests } = setup();
    delete options.env[missing];
    const error = expectLaunchPreflightFailure(options, "openai_target_state_roots_missing");
    expect(error.cause).toBeUndefined();
    expect(requests).toEqual([]);
  }

  {
    const { options, root, requests } = setup();
    options.env.CODEX_HOME = join(root, "missing-codex-home");
    const error = expectLaunchPreflightFailure(options, "openai_codex_state_root_unresolvable");
    expect((error.cause as NodeJS.ErrnoException).code).toBe("ENOENT");
    expect(requests).toEqual([]);
  }
  {
    const { options, codexHome, requests } = setup();
    rmSync(join(codexHome, "sqlite"), { recursive: true });
    const error = expectLaunchPreflightFailure(options, "openai_codex_state_root_unresolvable");
    expect((error.cause as NodeJS.ErrnoException).code).toBe("ENOENT");
    expect(requests).toEqual([]);
  }
  {
    const { options, root, requests } = setup();
    options.cwd = join(root, "missing-cwd");
    const error = expectLaunchPreflightFailure(options, "openai_codex_cwd_unresolvable");
    expect((error.cause as NodeJS.ErrnoException).code).toBe("ENOENT");
    expect(requests).toEqual([]);
  }
  {
    const { options, root, requests } = setup();
    const hostileCwd = join(root, "hostile-git-root");
    mkdirSync(hostileCwd);
    writeFileSync(join(hostileCwd, ".git"), "gitdir: /north-test-missing-git-dir\n");
    options.cwd = hostileCwd;
    const error = expectLaunchPreflightFailure(options, "openai_codex_project_root_untrusted");
    expect((error.cause as Error).name).toBe("TrustedGitOracleError");
    expect(requests).toEqual([]);
  }

  for (const missing of ["command", "expected"] as const) {
    const { options, root, requests } = setup();
    if (missing === "command") options.command = join(root, "missing-command");
    else options.testExpectedExecutable = join(root, "missing-expected-command");
    const error = expectLaunchPreflightFailure(options, "openai_codex_executable_pin_mismatch");
    expect((error.cause as NodeJS.ErrnoException).code).toBe("ENOENT");
    expect(requests).toEqual([]);
  }
  {
    const { options, root, requests } = setup();
    const other = join(root, "other-codex");
    writeFileSync(other, "#!/bin/sh\nexit 1\n");
    chmodSync(other, 0o700);
    options.command = other;
    const error = expectLaunchPreflightFailure(options, "openai_codex_executable_pin_mismatch");
    expect((error.cause as Error).message).toContain("is not the pinned provider binary");
    expect(requests).toEqual([]);
  }

  for (const authority of ["config.toml", "hooks.json", "rules"] as const) {
    const { options, codexHome, requests } = setup();
    const path = join(codexHome, authority);
    if (authority === "rules") mkdirSync(path);
    else writeFileSync(path, "hostile\n");
    const error = expectLaunchPreflightFailure(
      options, "openai_codex_authority_filesystem_invalid",
    );
    expect((error.cause as Error).message)
      .toBe(`managed Codex account contains authority-bearing ${authority}`);
    expect(requests).toEqual([]);
  }
});

test("a workspace-write lane is granted its Git metadata roots + the North state root, and no more", async () => {
  const { options, requests } = setup();
  const expectedRoots = managedCodexWritableRoots(options.cwd);
  expect(expectedRoots.length).toBeGreaterThan(0);

  const contract = managedCodexAppServerLaunch(options as any);
  expect(contract.writableRoots).toEqual(expectedRoots);
  const rootsArgument = `sandbox_workspace_write.writable_roots=${JSON.stringify(expectedRoots)}`;
  expect(contract.args).toContain(rootsArgument);
  expect((contract.expectedSessionConfig as any).sandbox_workspace_write)
    .toEqual({ writable_roots: expectedRoots, network_access: true });
  // The grant is Git metadata + the North state root, and nothing else — never
  // the bare home, never an arbitrary repo, never `/`. The state root is in the
  // grant because every lane brief tells agents to run `north tell`, which was
  // otherwise guaranteed EROFS inside the sandbox; see sandboxWritableRoots.
  const northStateRoot = resolve(homedir(), ".local/state/north");
  for (const root of contract.writableRoots) expect(root.endsWith(".git")
    || root.includes("/.git/worktrees/")
    || root === northStateRoot).toBe(true);
  expect(contract.writableRoots).toContain(northStateRoot);
  expect(contract.writableRoots).not.toContain(homedir());

  await expect(new ManagedCodexAppServerRun(options).execute()).resolves.toMatchObject({
    text: "managed answer",
  });
  expect(requests.some(({ method }) => method === "turn/start")).toBe(true);
});

test("only a web-capable workspace-write lane receives the exact Gitiles proxy policy", async () => {
  const { options } = setup();
  const authorized = {
    ...options,
    surface: {
      ...surface,
      capabilities: [...surface.capabilities, "web"],
      web: "cached",
    } as OpenAIAuthoritySurface,
  };
  const contract = managedCodexAppServerLaunch(authorized);
  expect((contract.expectedSessionConfig as any).sandbox_workspace_write.network_access).toBe(true);
  expect((contract.expectedSessionConfig as any).features.network_proxy).toEqual({
    enabled: true,
    domains: { "chromium.googlesource.com": "allow" },
  });
  expect(contract.args).toContain("sandbox_workspace_write.network_access=true");
  expect(contract.args.filter((argument) => argument.includes("network_proxy"))).toEqual([
    "features.network_proxy.enabled=true",
    'features.network_proxy.domains={"chromium.googlesource.com"="allow"}',
  ]);
  expect(contract.args).not.toEqual(expect.arrayContaining(["--enable", "network_proxy"]));

  const denied = managedCodexAppServerLaunch(options);
  expect((denied.expectedSessionConfig as any).sandbox_workspace_write.network_access).toBe(true);
  expect((denied.expectedSessionConfig as any).features.network_proxy).toBe(false);
  expect(denied.args.filter((argument) => argument.includes("network_proxy"))).toEqual(["network_proxy"]);
  expect(denied.args).toEqual(expect.arrayContaining(["--disable", "network_proxy"]));
});

test("the Gitiles network config accepts the production object and fails closed on drift", async () => {
  const enabled = setup("web-network");
  await expect(new ManagedCodexAppServerRun(enabled.options).execute()).resolves.toBeDefined();
  expect(enabled.requests.some(({ method }) => method === "thread/start")).toBe(true);

  const disabled = setup();
  await expect(new ManagedCodexAppServerRun(disabled.options).execute()).resolves.toBeDefined();
  expect(disabled.requests.some(({ method }) => method === "thread/start")).toBe(true);

  for (const [mode, error] of [
    ["web-network-boolean-drift", "openai_codex_authority_preflight_failed"],
    ["web-network-object-drift", "openai_codex_authority_preflight_failed"],
    ["web-network-session-drift", "openai_codex_authority_preflight_failed"],
    ["web-network-thread-drift", "openai_provider_execution_failed"],
  ] as const) {
    const caught = await new ManagedCodexAppServerRun(setup(mode).options).execute()
      .then(() => null, (thrown: Error) => thrown);
    expect(caught?.message).toBe(error);
    if (mode === "web-network-boolean-drift") {
      expect(causeChain(caught!)).toContain("Codex effective network proxy policy does not match");
      expect(causeChain(caught!)).toContain("observed=true expected={");
    }
    if (mode === "web-network-object-drift") {
      expect(causeChain(caught!)).toContain("Codex effective network proxy policy does not match");
      expect(causeChain(caught!)).toContain("example.test");
    }
    if (mode === "web-network-session-drift") {
      expect(causeChain(caught!)).toContain("Codex session network proxy policy does not match");
      expect(causeChain(caught!)).toContain("observed=false expected={");
    }
  }
});

test("a read-only lane is granted no writable root at all", () => {
  const { options } = setup();
  const readOnly = {
    ...options,
    surface: { ...surface, sandbox: "read-only" } as OpenAIAuthoritySurface,
  };
  const contract = managedCodexAppServerLaunch(readOnly as any);
  expect(contract.writableRoots).toEqual([]);
  expect((contract.expectedSessionConfig as any).sandbox_workspace_write).toBeUndefined();
  expect(contract.args.some((argument) => argument.startsWith("sandbox_workspace_write")))
    .toBe(false);
});

test("a Codex director coordinates through required host-side North MCP without shell network", () => {
  const { options } = setup();
  const director = harnessOptions({
    self: "codex-app-server-director-authority",
    provider: "openai",
    cwd: options.cwd,
    routingMetadata: applyOrchestrationStaffing({ role: "director" }),
    presenceRegistrar: false,
  }) as any;
  const directorSurface = compileProviderAuthoritySurface("openai", director);
  const contract = managedCodexAppServerLaunch({ ...options, surface: directorSurface });
  const north = (contract.expectedSessionConfig as any).mcp_servers.north;

  expect(directorSurface.sandbox).toBe("read-only");
  expect(contract.network.networkAccess).toBe(false);
  expect(contract.writableRoots).toEqual([]);
  expect((contract.expectedSessionConfig as any).sandbox_workspace_write).toBeUndefined();
  expect(north).toMatchObject({ enabled: true, required: true });
  expect(north.enabled_tools).toEqual(expect.arrayContaining(["spawn", "dispatch"]));
  expect(directorSurface.managedTools).toEqual(expect.arrayContaining([
    "mcp__north__spawn", "mcp__north__dispatch",
  ]));
});

test("a read-only lane keeps declared web access — orchestrators are read-only by design", () => {
  // Every orchestrator template (director, team-lead, program, portfolio) carries
  // `shell.readonly`, so gating `web` on workspace-write silently stripped web
  // from every managed OpenAI orchestrator while the unsandboxed Anthropic
  // orchestrator kept it. Read-only + web is the orchestrator shape: coordinate
  // and research, execute nothing.
  const { options } = setup();
  const readOnlyWeb = {
    ...options,
    surface: {
      ...surface,
      sandbox: "read-only",
      capabilities: [...surface.capabilities, "web"],
      web: "cached",
    } as OpenAIAuthoritySurface,
  };
  const contract = managedCodexAppServerLaunch(readOnlyWeb as any);

  // Web survives the read-only shell surface.
  expect((contract.expectedSessionConfig as any).features.network_proxy).toEqual({
    enabled: true,
    domains: { "chromium.googlesource.com": "allow" },
  });
  expect(contract.args).toContain("features.network_proxy.enabled=true");
  expect(contract.args).not.toEqual(expect.arrayContaining(["--disable", "network_proxy"]));

  // The shell surface itself stays closed: still no writable root, still no
  // workspace-write network grant. Web is not a widening of the shell sandbox.
  expect(contract.writableRoots).toEqual([]);
  expect((contract.expectedSessionConfig as any).sandbox_workspace_write).toBeUndefined();
});

test("a read-only lane WITHOUT web capability still gets no proxy", () => {
  const { options } = setup();
  const readOnly = {
    ...options,
    surface: { ...surface, sandbox: "read-only" } as OpenAIAuthoritySurface,
  };
  const contract = managedCodexAppServerLaunch(readOnly as any);
  expect((contract.expectedSessionConfig as any).features.network_proxy).toBe(false);
  expect(contract.args).toEqual(expect.arrayContaining(["--disable", "network_proxy"]));
});

test("an unrecognized notification is ignored, counted, and never terminal — a flood still is", async () => {
  for (const mode of ["notification-unknown-prethread", "terminal-notification-unknown"]) {
    const { options } = setup(mode);
    const run = new ManagedCodexAppServerRun(options);
    // A new provider build adding one telemetry notification used to kill the
    // lane and orphan its work. The turn now completes.
    await expect(run.execute()).resolves.toMatchObject({ text: "managed answer" });
  }

  const flood = setup("notification-unknown-flood");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(flood.options).execute(); }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
});

test("a failure after landed work carries a harvest instead of erasing the turn", async () => {
  const { options } = setup("notification-terminal-error");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(options).execute(); }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.landedWork).toBe(true);
  expect(harvest.text).toBe("managed answer");
  expect(harvest.threadId).toBe("019f7abc-0000-7000-8000-000000000001");
  expect(harvest.turnIds).toEqual(["019f7abc-0000-7000-8000-000000000002"]);
  expect(harvest.toolItems).toBe(3);
  expect(harvest.pendingItemCount).toBe(0);
  expect(harvest.pendingItems).toEqual([]);
  expect(harvest.usage).toMatchObject({ input_tokens: 9, output_tokens: 3 });
  expect(harvest.mcp).toMatchObject({ totalCalls: 1 });
  expect(harvest.nativeCommands).toMatchObject({ totalCommands: 1 });

  const evidence = managedCodexHarvestEvidence(caught as ManagedCodexHarvestError);
  expect(evidence.turns).toEqual({
    unit: "provider-turn", count: 1, toolItems: 3, comparable: false,
  });
  expect(evidence.failure).toEqual({
    detail: "provider_execution_failed",
    landed: { completedTurns: 0, toolItems: 3, mcpCalls: 1, nativeCommands: 1 },
  });
  expect(evidence.providerJoin).toEqual({
    version: "north-provider-join:v1",
    sessionKey: providerSessionKey("019f7abc-0000-7000-8000-000000000001"),
    turnKeys: [providerTurnKey("openai", "019f7abc-0000-7000-8000-000000000002")],
    sessionPersistence: "ephemeral",
    coverage: "exact",
  });
  expect(JSON.stringify(evidence)).not.toContain("019f7abc");

  const unobserved = new ManagedCodexHarvestError({
    turnIds: [], completedTurns: 0, text: "partial answer", landedWork: true,
    mcp: { source: "fixture", coverage: "unknown", tools: [], operationReceipts: [], operationAggregates: [] },
    nativeCommands: {
      source: "fixture", coverage: "unknown", northBinaryProbe: "not_observed", completions: [],
    },
    unsupportedNotifications: {},
  });
  const unobservedEvidence = managedCodexHarvestEvidence(unobserved);
  expect(unobservedEvidence.turns).not.toHaveProperty("toolItems");
  expect(unobservedEvidence.failure?.landed).not.toHaveProperty("toolItems");
});

test("a pre-thread failure harvests nothing, preserving the provider-death retry gate", async () => {
  const { options } = setup("mcp-auth");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(options).execute(); }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  expect(caught).not.toBeInstanceOf(ManagedCodexHarvestError);
});

test("config drift observed AFTER a completed turn delivers the turn and refuses continuation", async () => {
  const { options } = setup("settlement-config-drift");
  const run = new ManagedCodexAppServerRun(options);
  // The turn already ran under authority proven at its start; discarding its
  // result over a post-hoc drift is exactly how landed work got orphaned.
  await expect(run.execute()).resolves.toMatchObject({ text: "managed answer" });

  const continued = setup("settlement-config-drift");
  const session = new ManagedCodexAppServerRun(continued.options)
    .session(async () => "second frame");
  await expect(session.next()).resolves.toMatchObject({
    value: { text: "managed answer" },
  });
  await expect(session.next()).rejects.toBeInstanceOf(ManagedCodexHarvestError);
});

// ---------------------------------------------------------------------------
// Provider diagnostics + turn watchdogs. Both behaviors are adapted from
// hermes-agent (MIT, Copyright (c) 2025 Nous Research): the redacted stderr
// tail on every failure (codex_app_server.py:353-368,
// codex_app_server_session.py:327-362) and the per-turn watchdog loop
// (codex_app_server_session.py:447-495).
// ---------------------------------------------------------------------------

test("a provider that dies mid-turn carries its own stderr and exit code into the harvest", async () => {
  const { options } = setup("provider-death-mid-turn");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(options).execute(); }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  // The whole point: "exited unexpectedly" now says what codex said.
  expect(harvest.stderrTail?.length).toBeGreaterThan(0);
  expect(harvest.stderrTail).toContain("codex: ERROR responses stream closed unexpectedly");
  expect(harvest.exitCode).toBe(9);
  // Work that landed before the death is still harvested.
  expect(harvest.landedWork).toBe(true);
  // The credential in that stderr never leaves the redactor.
  const rendered = harvest.stderrTail!.join("\n");
  expect(rendered).not.toContain("sk-fixturesecretvalue0123");
  expect(rendered).toContain("REDACTED");
  // And it reaches the failure MESSAGE, which is all a dead lane leaves behind.
  const chain = causeChain(caught as Error, 8, 4_000);
  expect(chain).toContain("managed Codex app-server exited unexpectedly");
  expect(chain).toContain("provider exit code 9");
  expect(chain).toContain("responses stream closed unexpectedly");
  expect((caught as ManagedCodexHarvestError).diagnostics?.stderrTail?.length)
    .toBeGreaterThan(0);
});

test("the supervisor status channel carries forwarded stderr and its EXIT receipt", async () => {
  const { options } = setup();
  const run = new ManagedCodexAppServerRun({
    ...options,
    useSupervisor: true,
    spawnProcess: supervisedStatusChild((stderr) => {
      stderr.write(`${codexSupervisorStatusLine("STARTED")}\n`);
      for (const line of [
        "codex: thread/start failed: Internal error",
        "codex: token=sk-supervisorsecret012345",
      ]) stderr.write(`${codexSupervisorStatusLine(codexSupervisorStderrStatus(line))}\n`);
      stderr.write(`${codexSupervisorStatusLine("EXIT 9")}\n`);
    }),
  });
  let caught: unknown;
  try { await run.execute(); } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexPreThreadError);
  const diagnostics = (caught as ManagedCodexPreThreadError).diagnostics!;
  expect(diagnostics.exitCode).toBe(9);
  expect(diagnostics.stderrTail).toContain("codex: thread/start failed: Internal error");
  expect(diagnostics.stderrTail.join("\n")).not.toContain("sk-supervisorsecret012345");
  const chain = causeChain(caught as Error, 8, 4_000);
  expect(chain).toContain("provider exit code 9");
  expect(chain).toContain("thread/start failed: Internal error");
});

test("the production supervisor forwards a redacted stderr tail and its exit receipt", async () => {
  const supervisor = join(import.meta.dir, "../src/providers/codex-supervisor.ts");
  const fixture = join(import.meta.dir, "fixtures/fake-codex-app-server.mjs");
  const controlRoot = mkdtempSync(join(tmpdir(), "north-codex-stderr-tail-"));
  roots.push(controlRoot);
  const child = spawn(process.execPath, [
    supervisor, "--duplex", controlRoot, "--stderr-tail", process.execPath, fixture,
  ], {
    env: {
      ...process.env,
      NORTH_MKFIFO_BIN: realpathSync(Bun.which("mkfifo")!),
      FAKE_CODEX_RESPONSES: JSON.stringify({
        probe: { $diagnosticExit: {
          code: 9,
          lines: [
            "codex: fatal: responses stream closed",
            "codex: Authorization: Bearer sk-productionsecret0123",
          ],
        } },
      }),
    },
    stdio: ["pipe", "pipe", "pipe"],
  }) as ChildProcessWithoutNullStreams;
  child.stdout.resume();
  const receipts: string[] = [];
  let buffered = "";
  child.stderr.on("data", (chunk) => {
    buffered += chunk.toString();
    while (buffered.includes("\n")) {
      const index = buffered.indexOf("\n");
      receipts.push(buffered.slice(0, index));
      buffered = buffered.slice(index + 1);
    }
  });
  try {
    const temporary = join(controlRoot, ".000000000001.test.tmp");
    writeAtomicSupervisorFrame(
      temporary, `${JSON.stringify({ id: 1, method: "probe", params: {} })}\n`,
    );
    renameSync(temporary, join(controlRoot, "000000000001.req"));
    const closed = await Promise.race([
      new Promise<boolean>((resolveClose) => child.once("close", () => resolveClose(true))),
      new Promise<boolean>((resolveClose) => setTimeout(() => resolveClose(false), 4_000)),
    ]);
    expect(closed).toBe(true);
  } finally {
    try { child.stdin.end(); } catch {}
    if (child.exitCode === null && child.signalCode === null) child.kill("SIGKILL");
  }
  const statuses = receipts
    .filter((line) => line.startsWith(CODEX_SUPERVISOR_STATUS_PREFIX))
    .map((line) => line.slice(CODEX_SUPERVISOR_STATUS_PREFIX.length));
  expect(statuses[0]).toBe("STARTED");
  expect(statuses.at(-1)).toBe("EXIT 9");
  const forwarded = statuses
    .map((status) => codexSupervisorStderrLine(status))
    .filter((line): line is string => line !== undefined);
  expect(forwarded).toContain("codex: fatal: responses stream closed");
  expect(forwarded.join("\n")).not.toContain("sk-productionsecret0123");
}, 8_000);

test("a tool completion followed by silence settles the turn as interrupted", async () => {
  const { options, requests } = setup("turn-silent-after-tool");
  const startedAt = Date.now();
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({
      ...options, postToolQuietMs: 150, turnDeadlineMs: 30_000,
    }).execute();
  } catch (error) { caught = error; }
  const elapsed = Date.now() - startedAt;
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const chain = causeChain(caught as Error, 8, 4_000);
  expect(chain).toContain("codex went silent for 150ms after a completed tool item");
  expect(chain).toContain("provider still running");
  expect(chain).toContain("turn/interrupt accepted");
  // The turn is interrupted, not the process: the interrupt actually went out.
  expect(requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(1);
  // Settled inside the quiet window, not the (30s) overall deadline.
  expect(elapsed).toBeLessThan(5_000);
  // The tool work that landed before the wedge is still harvested.
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.toolItems).toBe(1);
  expect(harvest.interrupt).toMatchObject({
    reason: "post_tool_silence", deadlineMs: 150, inactivityThresholdMs: 150,
    openItemCount: 0, openItem: null,
  });
});

test("a silent open item suspends inactivity expiry until it completes", async () => {
  const { options, requests } = setup("turn-silent-open-item-completes");
  const result = await new ManagedCodexAppServerRun({
    ...options, turnDeadlineMs: 100, turnDeadlineInactivityMs: 100,
    inFlightItemCeilingMs: 1_000, postToolQuietMs: 30_000,
  }).execute();
  expect(result.text).toBe("managed answer");
  expect(result.toolItems).toBe(1);
  expect(requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(0);
});

test("a silent open item expires at its own ceiling with item evidence", async () => {
  const { options } = setup("turn-silent-open-item");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({
      ...options, turnDeadlineMs: 100, turnDeadlineInactivityMs: 100,
      inFlightItemCeilingMs: 200, postToolQuietMs: 30_000,
    }).execute();
  } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  expect(causeChain(caught as Error, 8, 4_000)).toContain(
    "codex in-flight commandExecution item command-1 exceeded its 200ms ceiling",
  );
  const interrupt = (caught as ManagedCodexHarvestError).harvest.interrupt;
  expect(interrupt).toMatchObject({
    reason: "in_flight_item_ceiling", deadlineMs: 200, inactivityThresholdMs: 100,
    openItemCount: 1,
    openItem: { id: "command-1", kind: "commandExecution" },
  });
  expect(interrupt!.openItem!.ageMs).toBeGreaterThanOrEqual(150);
  expect(managedCodexHarvestEvidence(caught as ManagedCodexHarvestError).interrupt)
    .toMatchObject({
      reason: "north_in_flight_item_ceiling", openItemCount: 1,
      openItem: { kind: "command" },
    });
});

test("a slow but active turn is not killed after crossing its wall deadline", async () => {
  const { options, requests } = setup("turn-slow-active");
  const result = await new ManagedCodexAppServerRun({
    ...options, postToolQuietMs: 150, turnDeadlineMs: 100,
    turnDeadlineInactivityMs: 100, timeoutMs: 5_000,
  }).execute();
  expect(result.text).toBe("managed answer");
  // 5 interposed fileChange items + the launch command item.
  expect(result.toolItems).toBe(6);
  expect(requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(0);
});

test("reasoning deltas alone keep a turn alive after its wall deadline", async () => {
  const { options, requests } = setup("turn-slow-reasoning");
  const result = await new ManagedCodexAppServerRun({
    ...options, turnDeadlineMs: 100, turnDeadlineInactivityMs: 100,
    postToolQuietMs: 30_000,
  }).execute();
  expect(result.text).toBe("managed answer");
  expect(requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(0);
});

test("a turn silent past both deadline and inactivity threshold carries structured evidence", async () => {
  const { options } = setup("turn-silent-before-tool");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({
      ...options, turnDeadlineMs: 150, turnDeadlineInactivityMs: 150,
      postToolQuietMs: 30_000,
    }).execute();
  } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const chain = causeChain(caught as Error, 8, 4_000);
  expect(chain).toContain("codex turn exceeded its 150ms deadline");
  expect(chain).toContain("turn/interrupt accepted");
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.interrupt).toMatchObject({
    reason: "turn_deadline", deadlineMs: 150, inactivityThresholdMs: 150,
    openItemCount: 0, openItem: null, eventCount: 1,
  });
  expect(harvest.interrupt!.lastActivityAgeMs).toBeGreaterThanOrEqual(100);
  const evidence = managedCodexHarvestEvidence(caught as ManagedCodexHarvestError);
  expect(evidence.interrupt).toMatchObject({
    reason: "north_turn_deadline", deadlineMs: 150, inactivityThresholdMs: 150,
    openItemCount: 0, eventCount: 1,
  });
  expect(evidence.interrupt).not.toHaveProperty("openItem");
});

// ---------------------------------------------------------------------------
// Retire-and-respawn on provider death. Adapted from hermes-agent (MIT,
// Copyright (c) 2025 Nous Research): TurnResult.should_retire
// (codex_app_server_session.py:79-85) and its consumption in
// codex_runtime.py:694-731, which drops the dead session so the next turn
// respawns codex from scratch. North re-sends the accumulated context itself
// because its provider thread is the only transcript there ever was.
// ---------------------------------------------------------------------------

test("a turn interrupt waits through replacement preflight and dispatches exactly once", async () => {
  const fixture = setup("respawn-interrupt-gap");
  const writer = new WireEventWriter({
    runId: wireRunId("run:managed-codex-respawn-interrupt-wire"),
    eventId: (sequence) => wireEventId(`event:managed-codex-respawn-interrupt-wire:${sequence}`),
  });
  writer.append({ kind: "run.started", lifecycle: "running" });
  const normalizer = new OpenAIWireNormalizer({
    writer,
    route: {
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    },
  });
  const run = new ManagedCodexAppServerRun({
    ...fixture.options,
    onEvent(method, params) { normalizer.normalize(method, params); },
    onRespawn() {
      if (normalizer.hasActiveTurn()) normalizer.settleProviderRespawn();
    },
  });
  const execution = run.execute();
  await Promise.race([
    fixture.replacementPreflightEntered,
    Bun.sleep(2_000).then(() => { throw new Error("replacement preflight gate was not reached"); }),
  ]);

  const firstInterrupt = run.interruptTurn();
  const duplicateInterrupt = run.interruptTurn();
  expect(duplicateInterrupt).toBe(firstInterrupt);
  let interruptSettled = false;
  void firstInterrupt.then(
    () => { interruptSettled = true; },
    () => { interruptSettled = true; },
  );
  await Bun.sleep(0);
  expect(interruptSettled).toBe(false);

  fixture.releaseReplacementPreflight();
  await expect(firstInterrupt).resolves.toBeUndefined();
  await expect(duplicateInterrupt).resolves.toBeUndefined();
  const result = await execution;
  expect(result.text).toBe("");
  expect(fixture.attempts()).toBe(2);
  expect(fixture.requests.filter(({ method }) => method === "initialize")).toHaveLength(2);
  expect(fixture.requests.filter(({ method }) => method === "thread/start")).toHaveLength(2);
  expect(fixture.requests.filter(({ method }) => method === "turn/start")).toHaveLength(2);
  expect(fixture.requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(1);

  const modelStarts = writer.events().filter((event) => event.kind === "model-call.started");
  const modelTerminals = writer.events().filter((event) => event.kind === "model-call.completed");
  expect(modelStarts).toHaveLength(2);
  expect(modelTerminals).toHaveLength(2);
  expect(modelTerminals[0]).toMatchObject({
    status: "failed", origin: "north", errorCode: "provider_session_replaced",
  });
  expect(modelTerminals[1]).toMatchObject({ status: "succeeded", origin: "provider" });
  expect(modelStarts.every((start) => modelTerminals
    .filter((terminal) => terminal.modelCallId === start.modelCallId).length === 1)).toBe(true);
  const toolAdmissions = writer.events().filter((event) => event.kind === "tool.admitted");
  const toolTerminals = writer.events().filter((event) => event.kind === "tool.terminal");
  expect(toolAdmissions.length).toBeGreaterThan(0);
  expect(toolTerminals).toHaveLength(toolAdmissions.length);
  expect(toolAdmissions.every((admission) => toolTerminals
    .filter((terminal) => terminal.toolCallId === admission.toolCallId).length === 1)).toBe(true);
  const mcpAdmission = toolAdmissions.find((event) => event.name === "mcp:north/tell");
  expect(mcpAdmission).toBeDefined();
  const mcpTerminal = toolTerminals.find((event) =>
    event.toolCallId === mcpAdmission?.toolCallId);
  expect(mcpTerminal).toMatchObject({
    status: "succeeded", origin: "provider", resultPreview: "CANARY-private-result",
  });
  expect(new TextEncoder().encode(mcpTerminal?.resultPreview).byteLength)
    .toBeLessThanOrEqual(RETAINED_PROVIDER_PREVIEW_MAX_BYTES);

  writer.append({
    kind: "run.terminated", lifecycle: "completed", reason: { code: "completed" },
  });
  const snapshot = writer.snapshot()!;
  expect(snapshot.lifecycle).toBe("completed");
  expect(Object.values(snapshot.modelCalls).some((call) => call.status === "running")).toBe(false);
  expect(Object.values(snapshot.toolCalls).some((call) => call.status === "pending")).toBe(false);
  expect(Object.values(snapshot.messages).some((message) => message.stage !== "completed")).toBe(false);

  const serializedWire = JSON.stringify(writer.events());
  for (const privateMaterial of [
    "019f7abc-0000-7000-8000-000000000001",
    "019f7abc-0000-7000-8000-000000000005",
    "019f7abc-0000-7000-8000-000000000002",
    "gpt-fixture-exact",
    "fixture@example.test",
    "CANARY-private-argument",
  ]) expect(serializedWire).not.toContain(privateMaterial);
  const wireWithoutMcpPreview = writer.events().map((event) =>
    event === mcpTerminal ? { ...event, resultPreview: undefined } : event);
  expect(JSON.stringify(wireWithoutMcpPreview)).not.toContain("CANARY-private-result");
});

test("a queued turn interrupt survives an eligible replacement that dies before its turn", async () => {
  const fixture = setup("respawn-interrupt-multigap");
  const run = new ManagedCodexAppServerRun(fixture.options);
  const execution = run.execute();
  await Promise.race([
    fixture.replacementPreflightEntered,
    Bun.sleep(2_000).then(() => { throw new Error("replacement preflight gate was not reached"); }),
  ]);
  const pendingInterrupt = run.interruptTurn();
  fixture.releaseReplacementPreflight();
  await Promise.race([
    pendingInterrupt,
    Bun.sleep(2_000).then(() => { throw new Error("replacement interrupt did not settle"); }),
  ]);
  await expect(pendingInterrupt).resolves.toBeUndefined();
  await expect(execution).resolves.toMatchObject({ text: "" });
  expect(fixture.attempts()).toBe(3);
  expect(run.respawnRecord().respawnCount).toBe(2);
  expect(fixture.requests.filter(({ method }) => method === "initialize")).toHaveLength(3);
  expect(fixture.requests.filter(({ method }) => method === "thread/start")).toHaveLength(2);
  expect(fixture.requests.filter(({ method }) => method === "turn/start")).toHaveLength(2);
  expect(fixture.requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(1);
  const recovered = fixture.turnInputs.at(-1)!;
  expect(recovered).toContain("1 item(s) were open");
  expect(recovered).toContain("commandExecution/command×1");
  expect(recovered).toContain("success is unknown");
  expect(recovered).not.toContain("command-open-before-respawn");
  expect(recovered).not.toContain("printf unfinished");
});

test("a queued replacement interrupt rejects generically when preflight never reaches a turn", async () => {
  const fixture = setup("respawn-preflight-broken");
  let pendingInterrupt: Promise<void> | undefined;
  let run!: ManagedCodexAppServerRun;
  run = new ManagedCodexAppServerRun({
    ...fixture.options,
    onRespawn() { pendingInterrupt = run.interruptTurn(); },
  });
  await expect(run.execute()).rejects.toBeInstanceOf(ManagedCodexHarvestError);
  expect(pendingInterrupt).toBeDefined();
  await expect(pendingInterrupt!).rejects.toThrow("managed_provider_replacement_turn_unavailable");
  expect(fixture.attempts()).toBe(1);
});

test("a full interrupt from respawn settlement cancels the queued turn and prevents attempt two", async () => {
  const fixture = setup("respawn-after-third-item");
  let pendingInterrupt: Promise<void> | undefined;
  let fullInterrupt: Promise<void> | undefined;
  let run!: ManagedCodexAppServerRun;
  run = new ManagedCodexAppServerRun({
    ...fixture.options,
    onRespawn() {
      pendingInterrupt = run.interruptTurn();
      fullInterrupt = run.interrupt();
    },
  });
  await expect(run.execute()).rejects.toBeInstanceOf(ManagedCodexHarvestError);
  await expect(fullInterrupt!).resolves.toBeUndefined();
  await expect(pendingInterrupt!).rejects.toThrow("managed_provider_replacement_turn_unavailable");
  expect(fixture.attempts()).toBe(1);
  expect(fixture.requests.filter(({ method }) => method === "thread/start")).toHaveLength(1);
  expect(fixture.requests.filter(({ method }) => method === "turn/start")).toHaveLength(1);
  await expect(run.interruptTurn()).rejects.toThrow("provider_has_no_active_turn");
});

test("a provider death after landed work respawns the lane with recovered context", async () => {
  const { options, turnInputs, attempts } = setup("respawn-after-third-item");
  const writer = new WireEventWriter({
    runId: wireRunId("run:managed-codex-respawn-wire"),
    eventId: (sequence) => wireEventId(`event:managed-codex-respawn-wire:${sequence}`),
  });
  writer.append({ kind: "run.started", lifecycle: "running" });
  const normalizer = new OpenAIWireNormalizer({
    writer,
    route: {
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    },
  });
  const run = new ManagedCodexAppServerRun({
    ...options,
    onEvent(method, params) { normalizer.normalize(method, params); },
    onRespawn() {
      if (normalizer.hasActiveTurn()) normalizer.settleProviderRespawn();
    },
  });
  const result = await run.execute();

  // Two provider processes, one lane, one delivered result.
  expect(attempts()).toBe(2);
  const record = run.respawnRecord();
  expect(record.respawnCount).toBe(1);
  expect(record.completedTurns).toBe(1);
  expect(record.respawns[0]).toMatchObject({
    attempt: 1,
    threadId: "019f7abc-0000-7000-8000-000000000001",
    completedTurns: 0,
    exitCode: 9,
  });
  expect(record.respawns[0]!.reason).toContain("exited unexpectedly");
  expect(record.respawns[0]!.stderrTail)
    .toContain("codex: ERROR responses stream closed unexpectedly");

  // The second turn/start carries the brief AND the pre-crash work, marked as
  // recovered — not a silent replay of the original prompt.
  expect(turnInputs).toHaveLength(2);
  expect(turnInputs[0]).toBe("perform managed work");
  const recovered = turnInputs[1]!;
  expect(recovered).toContain("perform managed work");
  expect(recovered).toContain("=== recovered context from a crashed provider session ===");
  expect(recovered).toContain("recovery cause: provider_process_died");
  expect(recovered).not.toContain("019f7abc-0000-7000-8000-000000000001");
  expect(recovered).not.toContain("019f7abc-0000-7000-8000-000000000005");
  expect(recovered).not.toContain("019f7abc-0000-7000-8000-000000000002");
  expect(recovered).not.toContain("gpt-fixture-exact");
  expect(recovered).not.toContain(record.respawns[0]!.reason);
  expect(recovered).toContain("2 native command(s)");
  expect(recovered).toContain("north/tell");
  expect(recovered).toContain("1 item(s) were open");
  expect(recovered).toContain("commandExecution/command×1");
  expect(recovered).toContain("success is unknown");
  expect(recovered).toContain("Inspect current state before deciding whether to retry");
  expect(recovered).not.toContain("command-open-before-respawn");
  expect(recovered).not.toContain("printf unfinished");
  expect(recovered).not.toContain("CANARY-private-argument");
  expect(recovered).not.toContain("CANARY-private-result");
  // The pre-crash work reaches the model, and the delivered result is the NEW
  // session's — the lane completed instead of dying.
  expect(result.text).toContain("managed answer after recovery");
  expect(result.text).toContain("recovered context from a crashed provider session");
  expect(result.text).not.toContain("019f7abc-0000-7000-8000-000000000001");
  expect(result.text).not.toContain(record.respawns[0]!.reason);
  expect(result.usage).toEqual({
    input_tokens: 9,
    cached_input_tokens: 4,
    output_tokens: 3,
    reasoning_output_tokens: 1,
  });

  const modelStarts = writer.events().filter((event) => event.kind === "model-call.started");
  const modelTerminals = writer.events().filter((event) => event.kind === "model-call.completed");
  expect(modelStarts).toHaveLength(2);
  expect(modelTerminals).toHaveLength(2);
  expect(modelTerminals[0]).toMatchObject({
    kind: "model-call.completed",
    status: "failed",
    origin: "north",
    usageCoverage: "unavailable",
    errorCode: "provider_session_replaced",
    usage: {
      lifetime: {
        inputTokens: 80,
        outputTokens: 20,
        cacheReadTokens: 30,
        cacheWriteTokens: 0,
        reasoningTokens: 5,
        modelCalls: 1,
      },
      context: { tokens: 0 },
    },
  });
  expect(modelTerminals[1]).toMatchObject({
    kind: "model-call.completed",
    status: "succeeded",
    origin: "provider",
    usageCoverage: "exact",
    usage: {
      lifetime: {
        inputTokens: 89,
        outputTokens: 23,
        cacheReadTokens: 34,
        cacheWriteTokens: 0,
        reasoningTokens: 6,
        modelCalls: 2,
      },
      context: { tokens: 0 },
    },
  });
  expect(modelStarts[0]!.sequence).toBeLessThan(modelTerminals[0]!.sequence);
  expect(modelTerminals[0]!.sequence).toBeLessThan(modelStarts[1]!.sequence);
  expect(modelStarts[1]!.sequence).toBeLessThan(modelTerminals[1]!.sequence);
  const toolAdmissions = writer.events().filter((event) => event.kind === "tool.admitted");
  const toolTerminals = writer.events().filter((event) => event.kind === "tool.terminal");
  expect(toolAdmissions).toHaveLength(7);
  expect(toolTerminals).toHaveLength(7);
  expect(new Set(toolTerminals.map((event) => event.toolCallId)).size).toBe(7);
  expect(modelStarts.every((start) => modelTerminals
    .filter((terminal) => terminal.modelCallId === start.modelCallId).length === 1)).toBe(true);

  expect(run.mcpActivity()).toMatchObject({
    coverage: "partial",
    totalCalls: 2,
    tools: [{ server: "north", tool: "tell", count: 2 }],
  });
  expect(run.mcpActivity().operationReceipts).toHaveLength(2);
  expect(run.nativeCommandActivity()).toMatchObject({
    coverage: "partial",
    totalCommands: 2,
    successfulCommands: 2,
    failedCommands: 0,
    declinedCommands: 0,
    northBinaryProbe: "failed",
  });
  expect(run.nativeCommandActivity()).not.toHaveProperty("openCommands");
  expect(run.nativeCommandActivity().completions).toHaveLength(2);

  expect(writer.snapshot()).toMatchObject({
    lifecycle: "running",
    usage: {
      lifetime: {
        inputTokens: 89,
        outputTokens: 23,
        cacheReadTokens: 34,
        cacheWriteTokens: 0,
        reasoningTokens: 6,
        modelCalls: 2,
      },
      context: { tokens: 0 },
    },
    usageCoverage: {
      providerTerminalCount: 1,
      scope: "wire_run_cumulative",
      totalStatus: "partial",
    },
  });
  expect(writer.events()).toContainEqual(expect.objectContaining({
    kind: "message.recorded",
    stage: "completed",
    content: expect.stringContaining("recovered context from a crashed provider session"),
  }));
  const serializedWire = JSON.stringify(writer.events());
  expect(serializedWire).not.toContain("019f7abc-0000-7000-8000-000000000001");
  expect(serializedWire).not.toContain("019f7abc-0000-7000-8000-000000000005");
  expect(serializedWire).not.toContain("019f7abc-0000-7000-8000-000000000002");
  expect(serializedWire).not.toContain("gpt-fixture-exact");
  expect(serializedWire).not.toContain(record.respawns[0]!.reason);

});

test("a dead turn harvests bounded normalized pending items without private payloads", async () => {
  const { options } = setup("respawn-after-third-item");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({ ...options, maxRespawns: 0 }).execute();
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.pendingItemCount).toBe(1);
  expect(harvest.pendingItems).toEqual([{
    kind: "commandExecution", name: "command", count: 1,
  }]);
  const serialized = JSON.stringify(harvest.pendingItems);
  expect(serialized).not.toContain("command-open-before-respawn");
  expect(serialized).not.toContain("printf unfinished");
  expect(serialized).not.toContain(options.cwd);
  expect(serialized).not.toContain("CANARY-private-argument");
  expect(serialized).not.toContain("CANARY-private-result");
});

test("pending harvest reuses Wire tool identity and excludes open reasoning and messages", async () => {
  const { options } = setup("pending-side-effect-death");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({ ...options, maxRespawns: 0 }).execute();
  } catch (error) {
    caught = error;
  }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.pendingItemCount).toBe(1);
  expect(harvest.pendingItems).toEqual([{
    kind: "mcpToolCall", name: "mcp:north/tell", count: 1,
  }]);
  const serialized = JSON.stringify(harvest.pendingItems);
  expect(serialized).not.toContain("reasoning-private-id");
  expect(serialized).not.toContain("message-private-id");
  expect(serialized).not.toContain("mcp-private-id");
  expect(serialized).not.toContain("CANARY-private-pending-argument");
});

test("pending harvest bounds normalized summaries while retaining the exact count", async () => {
  const { options } = setup("pending-summary-bound-death");
  const caught = await new ManagedCodexAppServerRun({ ...options, maxRespawns: 0 }).execute()
    .then(() => undefined, (error: Error) => error);
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.pendingItemCount).toBe(18);
  expect(harvest.pendingItems).toHaveLength(16);
  expect(harvest.pendingItems?.every((item) => item.kind === "mcpToolCall"
    && item.name.startsWith("mcp:north/pending_") && item.count === 1)).toBe(true);
  const recovered = managedCodexRecoveredContext("brief", [], harvest);
  expect(recovered).toContain("18 item(s)");
  expect(recovered).toContain("2 other item(s)");
  expect(recovered).toContain("success is unknown");
  const serialized = JSON.stringify({ pendingItems: harvest.pendingItems, recovered });
  expect(serialized).not.toContain("mcp-private-");
  expect(serialized).not.toContain("CANARY-private-pending-");
});

test("an exhausted respawn budget fails exactly as before, with every attempt's diagnostics", async () => {
  const { options, attempts } = setup("respawn-exhausted");
  const run = new ManagedCodexAppServerRun(options);
  let caught: unknown;
  try { await run.execute(); } catch (error) { caught = error; }

  // The default budget is 2, so the lane spends three provider processes.
  expect(attempts()).toBe(3);
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  const harvest = (caught as ManagedCodexHarvestError).harvest;
  expect(harvest.respawnCount).toBe(2);
  expect(harvest.respawns?.map((entry) => entry.attempt)).toEqual([1, 2]);
  expect(harvest.respawns?.every((entry) => entry.exitCode === 7)).toBe(true);
  expect(harvest.respawns?.[0]!.stderrTail)
    .toContain("codex: fatal: provider session 1 refused to start work");
  expect(harvest.respawns?.[1]!.stderrTail)
    .toContain("codex: fatal: provider session 2 refused to start work");
  // The final failure keeps its own diagnostics, unchanged by the respawns.
  expect(harvest.exitCode).toBe(7);
  expect(harvest.stderrTail)
    .toContain("codex: fatal: provider session 3 refused to start work");
  const evidence = managedCodexHarvestEvidence(caught as ManagedCodexHarvestError);
  expect(evidence.failure).toMatchObject({
    detail: "provider_execution_failed",
    landed: { completedTurns: 0 },
  });
  expect(JSON.stringify(evidence)).not.toContain("respawnCount");

  // Budget 0 restores the pre-respawn behavior exactly: one process, one death.
  const disabled = setup("respawn-exhausted");
  let bare: unknown;
  try {
    await new ManagedCodexAppServerRun({ ...disabled.options, maxRespawns: 0 }).execute();
  } catch (error) { bare = error; }
  expect(disabled.attempts()).toBe(1);
  expect(bare).toBeInstanceOf(ManagedCodexHarvestError);
  expect((bare as ManagedCodexHarvestError).harvest.respawnCount).toBeUndefined();
  expect((bare as ManagedCodexHarvestError).harvest.landedWork).toBe(false);
  const providerExit = managedCodexHarvestEvidence(bare as ManagedCodexHarvestError);
  expect(providerExit.failure?.detail).toBe("provider_execution_failed");
  expect(providerExit.interrupt).toBeUndefined();
});

test("a respawn whose preflight fails is a harvest, never a retry-safe pre-thread failure", async () => {
  const { options } = setup("respawn-preflight-broken");
  let caught: unknown;
  try { await new ManagedCodexAppServerRun(options).execute(); }
  catch (error) { caught = error; }
  // The lane already started a provider thread and may already have written to
  // the working tree; classifying the re-preflight failure as pre-thread would
  // hand spawn.ts permission to run the whole brief a second time.
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  expect(caught).not.toBeInstanceOf(ManagedCodexPreThreadError);
  expect((caught as ManagedCodexHarvestError).harvest.landedWork).toBe(true);
  expect(causeChain(caught as Error, 8, 4_000)).toContain("openai_codex_state_root_unresolvable");
});

test("every respawn revalidates adapter authority before a replacement process launches", async () => {
  const fixture = setup("respawn-after-third-item");
  let validations = 0;
  const run = new ManagedCodexAppServerRun({
    ...fixture.options,
    async beforeLaunch() {
      validations++;
      if (validations === 2) throw new Error("model receipt revoked");
    },
  });
  let caught: unknown;
  try { await run.execute(); } catch (error) { caught = error; }
  expect(validations).toBe(2);
  expect(fixture.attempts()).toBe(1);
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  expect(caught).not.toBeInstanceOf(ManagedCodexPreThreadError);
  expect(causeChain(caught as Error)).toContain("model receipt revoked");
});

test("a watchdog-interrupted turn settles WITHOUT respawning a live provider", async () => {
  const { options, requests, attempts } = setup("turn-silent-after-tool");
  const run = new ManagedCodexAppServerRun({
    ...options, postToolQuietMs: 150, turnDeadlineMs: 30_000,
  });
  let caught: unknown;
  try { await run.execute(); } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  // The interrupt landed and the provider was alive: a wedged TURN is not a
  // dead process, and buying it two more provider sessions would just wedge
  // three times as slowly.
  expect(requests.filter(({ method }) => method === "turn/interrupt")).toHaveLength(1);
  expect(attempts()).toBe(1);
  expect(run.respawnRecord().respawnCount).toBe(0);
});

test("a refused interrupt is named in the reason instead of replacing it", async () => {
  const { options } = setup("turn-interrupt-refused");
  let caught: unknown;
  try {
    await new ManagedCodexAppServerRun({
      ...options, postToolQuietMs: 30_000, turnDeadlineMs: 150,
      turnDeadlineInactivityMs: 150,
    }).execute();
  } catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ManagedCodexHarvestError);
  const chain = causeChain(caught as Error, 8, 4_000);
  // The watchdog's reason wins over the RPC-level failure its own interrupt
  // provoked — that failure is downstream of the wedge, not the cause of it.
  expect(chain).toContain("codex turn exceeded its 150ms deadline");
});
