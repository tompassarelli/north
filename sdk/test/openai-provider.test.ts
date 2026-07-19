import { afterEach, expect, test } from "bun:test";
import {
  chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createServer } from "node:net";
import type { AddressInfo } from "node:net";
import { codexHarnessArguments, openaiProvider } from "../src/providers/openai";
import { ProviderRetrySafeError, routedQuery } from "../src/providers";
import { harnessOptions } from "../src/harness";
import { applyGafferStaffing } from "../src/gaffer-staffing";
import { markExecutionAdmission } from "../src/execution-admission";
import { selectProviderFromAvailability } from "../src/provider-routing";

const savedBin = process.env.NORTH_CODEX_BIN;
const savedHome = process.env.HOME;
const savedPort = process.env.NORTH_PORT;
const savedFramLog = process.env.FRAM_LOG;
const savedLaws = process.env.AGENT_LAWS;
const savedGaffer = process.env.GAFFER_HOME;
const northRoot = realpathSync(join(import.meta.dir, "../.."));
const temporary: string[] = [];
const liveProcessPidFiles = new Set<string>();
const codexThreadStarted = JSON.stringify({
  type: "thread.started",
  thread_id: "67e55044-10b1-426f-9247-bb680e5fe0c8",
});
const codexTurnStarted = JSON.stringify({ type: "turn.started" });
const codexTerminal = (usage: Record<string, unknown> = {
  input_tokens: 1,
  cached_input_tokens: 0,
  output_tokens: 1,
  reasoning_output_tokens: 0,
}): string => JSON.stringify({ type: "turn.completed", usage });
const codexSuccess = (
  middle: string[] = [],
  usage?: Record<string, unknown>,
): string[] => [
  codexThreadStarted,
  codexTurnStarted,
  ...middle,
  codexTerminal(usage),
];
afterEach(() => {
  if (savedBin === undefined) delete process.env.NORTH_CODEX_BIN;
  else process.env.NORTH_CODEX_BIN = savedBin;
  if (savedHome === undefined) delete process.env.HOME;
  else process.env.HOME = savedHome;
  if (savedPort === undefined) delete process.env.NORTH_PORT;
  else process.env.NORTH_PORT = savedPort;
  if (savedFramLog === undefined) delete process.env.FRAM_LOG;
  else process.env.FRAM_LOG = savedFramLog;
  if (savedLaws === undefined) delete process.env.AGENT_LAWS;
  else process.env.AGENT_LAWS = savedLaws;
  if (savedGaffer === undefined) delete process.env.GAFFER_HOME;
  else process.env.GAFFER_HOME = savedGaffer;
  for (const path of liveProcessPidFiles) killRecordedProcess(path);
  liveProcessPidFiles.clear();
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

async function resultFromScript(lines: string[]): Promise<any> {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-usage-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash\n${lines.map((line) => `printf '%s\\n' '${line}'`).join("\n")}\n`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const messages: any[] = [];
  for await (const message of openaiProvider.query({ prompt: "x", options: {} as any }) as AsyncIterable<any>) {
    messages.push(message);
  }
  return messages.at(-1);
}

async function resultFromScriptBody(body: string): Promise<any> {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-protocol-"));
  temporary.push(directory);
  const executable = join(directory, "fake-codex");
  writeFileSync(executable, `#!/usr/bin/env bash\nset -eu\n${body}\n`);
  chmodSync(executable, 0o700);
  process.env.NORTH_CODEX_BIN = executable;
  const messages: any[] = [];
  for await (const message of openaiProvider.query({
    prompt: "x",
    options: {} as any,
  }) as AsyncIterable<any>) messages.push(message);
  return messages.at(-1);
}

async function waitForFile(path: string, timeoutMs = 2_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!existsSync(path) && Date.now() < deadline) await Bun.sleep(10);
  if (!existsSync(path)) throw new Error(`timed out waiting for ${path}`);
}

async function expectProcessGone(pid: number, timeoutMs = 2_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let alive = true;
  while (alive && Date.now() < deadline) {
    try {
      process.kill(pid, 0);
      await Bun.sleep(10);
    } catch {
      alive = false;
    }
  }
  expect(alive).toBe(false);
}

function killRecordedProcess(path: string): void {
  if (!existsSync(path)) return;
  const pid = Number(readFileSync(path, "utf8"));
  if (!Number.isSafeInteger(pid) || pid <= 1) return;
  if (process.platform !== "win32") {
    try { process.kill(-pid, "SIGKILL"); } catch { /* already gone or not a leader */ }
  }
  try { process.kill(pid, "SIGKILL"); } catch { /* already gone */ }
}

async function within<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

test("Codex adapter owns the cumulative total and does not double-count subsets", async () => {
  const result = await resultFromScript(codexSuccess([], {
      input_tokens: 100, cached_input_tokens: 60,
      output_tokens: 20, reasoning_output_tokens: 7,
  }));
  expect(result.usage).toEqual({
    input_tokens: 100, cached_input_tokens: 60,
    output_tokens: 20, reasoning_output_tokens: 7,
  });
  expect(result._north_usage).toEqual({
    provider: "openai", terminal_count: 1,
    scope: "codex_fresh_invocation_thread_cumulative",
    total_status: "exact", total_tokens: 120,
  });
  expect(result).not.toHaveProperty("duration_ms");
});

test("Codex requires one complete terminal and never synthesizes exit-zero success", async () => {
  await expect(resultFromScript([
    codexThreadStarted,
    codexTurnStarted,
    codexTerminal({ input_tokens: 0 }),
  ])).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScript([])).rejects.toThrow("openai_provider_execution_failed");
});

test("repeated Codex terminals fail instead of selecting a cumulative snapshot", async () => {
  await expect(resultFromScript([
    ...codexSuccess([], {
      input_tokens: 5, cached_input_tokens: 0, output_tokens: 1, reasoning_output_tokens: 0,
    }),
    codexTerminal({
      input_tokens: 9, cached_input_tokens: 4, output_tokens: 2, reasoning_output_tokens: 1,
    }),
  ])).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex lifecycle events are closed, ordered, and terminal exactly once", async () => {
  const validUsage = {
    input_tokens: 1,
    cached_input_tokens: 0,
    output_tokens: 1,
    reasoning_output_tokens: 0,
  };
  const invalidStreams = [
    ["not json"],
    [codexTurnStarted, codexThreadStarted, codexTerminal(validUsage)],
    [codexThreadStarted, codexTerminal(validUsage)],
    [
      codexThreadStarted,
      codexTurnStarted,
      JSON.stringify({ type: "future.event", payload: true }),
      codexTerminal(validUsage),
    ],
    [
      JSON.stringify({
        type: "thread.started",
        thread_id: "67e55044-10b1-426f-9247-bb680e5fe0c8",
        extra: true,
      }),
      codexTurnStarted,
      codexTerminal(validUsage),
    ],
    [
      '{"type":"thread.started","thread_id":"one","thread_id":"two"}',
      codexTurnStarted,
      codexTerminal(validUsage),
    ],
    [
      codexThreadStarted,
      codexTurnStarted,
      codexTerminal(validUsage),
      JSON.stringify({
        type: "item.completed",
        item: { id: "item_0", type: "agent_message", text: "too late" },
      }),
    ],
    [
      codexThreadStarted,
      codexTurnStarted,
      JSON.stringify({ type: "turn.failed", error: { message: "private failure" } }),
    ],
  ];
  for (const events of invalidStreams) {
    await expect(resultFromScript(events)).rejects.toThrow(
      "openai_provider_execution_failed",
    );
  }
});

test("Codex terminal usage is an exact coherent four-counter contract", async () => {
  const invalidUsage = [
    { input_tokens: 1, output_tokens: 1 },
    {
      input_tokens: -1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1.5,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: Number.MAX_SAFE_INTEGER + 1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 2,
      output_tokens: 1,
      reasoning_output_tokens: 0,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 2,
    },
    {
      input_tokens: 1,
      cached_input_tokens: 0,
      output_tokens: 1,
      reasoning_output_tokens: 0,
      extra: 1,
    },
  ];
  for (const usage of invalidUsage) {
    await expect(resultFromScript([
      codexThreadStarted,
      codexTurnStarted,
      codexTerminal(usage),
    ])).rejects.toThrow("openai_provider_execution_failed");
  }
});

test("Codex item payloads stay opaque except for identity and final agent text", async () => {
  const result = await resultFromScript(codexSuccess([
    JSON.stringify({
      type: "item.updated",
      item: {
        id: "item_0",
        type: "future_provider_item",
        provider_may_evolve: { nested: [true, 1, "bounded"] },
      },
    }),
    JSON.stringify({
      type: "item.completed",
      item: {
        id: "item_1",
        type: "agent_message",
        text: "final answer",
        incidental_provider_field: true,
      },
    }),
  ]));
  expect(result.result).toBe("final answer");

  await expect(resultFromScript(codexSuccess([
    JSON.stringify({
      type: "item.completed",
      item: { type: "agent_message", text: "missing identity" },
    }),
  ]))).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScript(codexSuccess([
    JSON.stringify({
      type: "item.completed",
      item: { id: "item_0", type: "agent_message", text: 7 },
    }),
  ]))).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex raw JSONL framing rejects invalid UTF-8, partial frames, and line overflow", async () => {
  await expect(resultFromScriptBody("printf '\\377\\n'"))
    .rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScriptBody(
    "printf '%s' '{\"type\":\"thread.started\"'",
  )).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScriptBody(
    "head -c 1048577 /dev/zero | tr '\\000' x; printf '\\n'",
  )).rejects.toThrow("openai_provider_execution_failed");
  await expect(resultFromScriptBody([
    `printf '%s\\n' '${codexThreadStarted}'`,
    `printf '%s\\n' '${codexTurnStarted}'`,
    `printf '%s\\n' '${codexTerminal()}'`,
    "printf '%s' '{\"trailing\":\"partial\"'",
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex JSONL frame-count and cumulative-byte ceilings are enforced", async () => {
  const item = JSON.stringify({
    type: "item.updated",
    item: { id: "item_0", type: "progress" },
  });
  await expect(resultFromScriptBody([
    `printf '%s\\n' '${codexThreadStarted}'`,
    `printf '%s\\n' '${codexTurnStarted}'`,
    "i=0",
    "while [ \"$i\" -lt 10000 ]; do",
    `  printf '%s\\n' '${item}'`,
    "  i=$((i + 1))",
    "done",
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");

  await expect(resultFromScriptBody([
    `printf '%s\\n' '${codexThreadStarted}'`,
    `printf '%s\\n' '${codexTurnStarted}'`,
    "padding=\"$(head -c 2048 /dev/zero | tr '\\000' x)\"",
    "i=0",
    "while [ \"$i\" -lt 9000 ]; do",
    "  printf '{\"type\":\"item.updated\",\"item\":{\"id\":\"item_0\",\"type\":\"progress\",\"payload\":\"%s\"}}\\n' \"$padding\"",
    "  i=$((i + 1))",
    "done",
  ].join("\n"))).rejects.toThrow("openai_provider_execution_failed");
});

test("Codex error events terminate and reap the child before propagating", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-child-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const terminated = join(directory, "terminated");
  writeFileSync(command, `#!/usr/bin/env bash
trap 'printf terminated > "${terminated}"; exit 0' TERM
printf '%s\\n' '{"type":"error","message":"CODEX_EVENT_CANARY_DO_NOT_EXPOSE"}'
while true; do :; done
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  let caught: unknown;
  try { for await (const _ of query as AsyncIterable<any>) {} }
  catch (error) { caught = error; }
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  expect((caught as Error).message).not.toContain("CODEX_EVENT_CANARY_DO_NOT_EXPOSE");
  expect(existsSync(terminated)).toBe(true);
  expect(readFileSync(terminated, "utf8")).toBe("terminated");
});

test("Codex parent exit reaps a TERM-resistant inherited-pipe descendant", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-exited-parent-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const parentPath = join(directory, "parent-pid");
  const descendantPath = join(directory, "descendant-pid");
  liveProcessPidFiles.add(parentPath);
  liveProcessPidFiles.add(descendantPath);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
printf '%s' "$$" > "${parentPath}"
(
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
exit 0
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  try {
    const startedAt = Date.now();
    const caught = await within((async (): Promise<unknown> => {
      try {
        for await (const _ of openaiProvider.query({
          prompt: "x", options: {} as any,
        }) as AsyncIterable<any>) {}
        return undefined;
      } catch (error) {
        return error;
      }
    })(), 3_000, "exited-parent reaper");
    const elapsed = Date.now() - startedAt;
    const parentPid = Number(readFileSync(parentPath, "utf8"));
    const descendantPid = Number(readFileSync(descendantPath, "utf8"));
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
    expect(elapsed).toBeLessThan(2_500);
    await expectProcessGone(parentPid);
    await expectProcessGone(descendantPid);
  } finally {
    killRecordedProcess(parentPath);
    killRecordedProcess(descendantPath);
    liveProcessPidFiles.delete(parentPath);
    liveProcessPidFiles.delete(descendantPath);
  }
});

test("Codex completion reaps a TERM-resistant descendant with closed pipes", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-closed-pipe-child-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const parentPath = join(directory, "parent-pid");
  const descendantPath = join(directory, "descendant-pid");
  liveProcessPidFiles.add(parentPath);
  liveProcessPidFiles.add(descendantPath);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
printf '%s' "$$" > "${parentPath}"
(
  exec </dev/null >/dev/null 2>&1
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
printf '%s\\n' '${codexTerminal()}'
exit 0
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  try {
    const startedAt = Date.now();
    const messages = await within((async (): Promise<any[]> => {
      const result: any[] = [];
      for await (const message of openaiProvider.query({
        prompt: "x", options: {} as any,
      }) as AsyncIterable<any>) result.push(message);
      return result;
    })(), 3_000, "closed-pipe descendant reaper");
    const elapsed = Date.now() - startedAt;
    const parentPid = Number(readFileSync(parentPath, "utf8"));
    const descendantPid = Number(readFileSync(descendantPath, "utf8"));
    expect(messages.at(-1)).toMatchObject({ type: "result", subtype: "success" });
    expect(elapsed).toBeLessThan(2_500);
    await expectProcessGone(parentPid);
    await expectProcessGone(descendantPid);
  } finally {
    killRecordedProcess(parentPath);
    killRecordedProcess(descendantPath);
    liveProcessPidFiles.delete(parentPath);
    liveProcessPidFiles.delete(descendantPath);
  }
});

test("Codex interrupt is bounded and kills a TERM-resistant process group", async () => {
  if (process.platform === "win32") return;
  const directory = mkdtempSync(join(tmpdir(), "north-codex-interrupt-group-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const parentPath = join(directory, "parent-pid");
  const descendantPath = join(directory, "descendant-pid");
  liveProcessPidFiles.add(parentPath);
  liveProcessPidFiles.add(descendantPath);
  writeFileSync(command, `#!/usr/bin/env bash
set -eu
trap '' TERM
printf '%s' "$$" > "${parentPath}"
(
  trap '' TERM
  printf '%s' "$BASHPID" > "${descendantPath}"
  while true; do sleep 10; done
) &
while [ ! -s "${descendantPath}" ]; do :; done
printf '%s\\n' '${codexThreadStarted}'
printf '%s\\n' '${codexTurnStarted}'
while true; do sleep 10; done
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  const running = (async (): Promise<unknown> => {
    try {
      for await (const _ of query as AsyncIterable<any>) {}
      return undefined;
    } catch (error) {
      return error;
    }
  })();
  try {
    await Promise.all([waitForFile(parentPath), waitForFile(descendantPath)]);
    const parentPid = Number(readFileSync(parentPath, "utf8"));
    const descendantPid = Number(readFileSync(descendantPath, "utf8"));
    const startedAt = Date.now();
    await within(query.interrupt(), 2_500, "Codex process-group interrupt");
    const elapsed = Date.now() - startedAt;
    const caught = await within(running, 2_500, "interrupted query settlement");
    expect((caught as Error).message).toBe("openai_provider_execution_failed");
    expect(elapsed).toBeLessThan(2_500);
    await expectProcessGone(parentPid);
    await expectProcessGone(descendantPid);
  } finally {
    killRecordedProcess(parentPath);
    killRecordedProcess(descendantPath);
    liveProcessPidFiles.delete(parentPath);
    liveProcessPidFiles.delete(descendantPath);
  }
});

test("cleanup failure never replaces the real Codex provider error", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-cleanup-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s\\n' '{"type":"error","message":"CODEX_CLEANUP_CANARY_DO_NOT_EXPOSE"}'
exit 2
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  query.interrupt = async () => { throw new Error("cleanup failed"); };

  await expect(async () => { for await (const _ of query as AsyncIterable<any>) {} })
    .toThrow("openai_provider_execution_failed");
});

test("Codex nonzero exit redacts stderr and is never retry-safe", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-reject-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  writeFileSync(command, "#!/usr/bin/env bash\nprintf 'CODEX_STDERR_CANARY_DO_NOT_EXPOSE' >&2\nexit 2\n");
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  let caught: unknown;
  try { for await (const _ of query as AsyncIterable<any>) {} }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(Error);
  expect(caught).not.toBeInstanceOf(ProviderRetrySafeError);
  expect((caught as Error).message).toBe("openai_provider_execution_failed");
  expect((caught as Error).message).not.toContain("CODEX_STDERR_CANARY_DO_NOT_EXPOSE");
});

test("a genuinely missing Codex executable is handled and retry-safe", async () => {
  process.env.NORTH_CODEX_BIN = join(tmpdir(), `north-no-such-codex-${process.pid}`);
  const query = openaiProvider.query({ prompt: "x", options: {} as any });
  let caught: unknown;
  try { for await (const _ of query as AsyncIterable<any>) {} }
  catch (error) { caught = error; }
  expect(caught).toBeInstanceOf(ProviderRetrySafeError);
  expect((caught as Error).message).toBe("openai_provider_executable_unavailable_before_acceptance");
  expect((caught as Error).message).not.toContain(process.env.NORTH_CODEX_BIN!);
});

test("two same-provider targets execute concurrently in disjoint Codex homes", async () => {
  const home = mkdtempSync(join(tmpdir(), "north-codex-targets-"));
  temporary.push(home);
  process.env.HOME = home;
  mkdirSync(join(home, ".codex"), { recursive: true });
  const command = join(home, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s' "$CODEX_HOME" > "$CODEX_HOME/execution-root"
printf '%s\n' "$@" > "$CODEX_HOME/argv"
printf '%s\n' '{"type":"thread.started","thread_id":"67e55044-10b1-426f-9247-bb680e5fe0c8"}'
printf '%s\n' '{"type":"turn.started"}'
printf '{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"%s"}}\n' "$CODEX_HOME"
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}}'
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const targets = [
    { id: "codex-one", provider: "openai" as const, authMode: "isolated" as const, profile: "one" },
    { id: "codex-two", provider: "openai" as const, authMode: "isolated" as const, profile: "two" },
  ];
  const execute = async (target: typeof targets[number]) => {
    const messages: any[] = [];
    for await (const message of openaiProvider.query({ prompt: target.id, options: {} as any, target }) as AsyncIterable<any>)
      messages.push(message);
    return messages.at(-1);
  };
  const [first, second] = await Promise.all(targets.map(execute));
  const firstRoot = join(home, ".local/state/north/accounts/openai/one");
  const secondRoot = join(home, ".local/state/north/accounts/openai/two");
  expect(first.result).toBe(firstRoot);
  expect(second.result).toBe(secondRoot);
  expect(readFileSync(join(firstRoot, "execution-root"), "utf8")).toBe(firstRoot);
  expect(readFileSync(join(secondRoot, "execution-root"), "utf8")).toBe(secondRoot);
  for (const root of [firstRoot, secondRoot]) {
    const argv = readFileSync(join(root, "argv"), "utf8");
    expect(argv).toContain('cli_auth_credentials_store="file"');
    expect(argv).toContain('forced_login_method="chatgpt"');
    expect(argv).toContain('model_provider="openai"');
    expect(argv).toContain(`sqlite_home="${join(root, "sqlite")}"`);
  }
});

test("Codex capability flags are global-before-exec and fail closed before process spawn", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-capabilities-"));
  temporary.push(directory);
  const command = join(directory, "fake-codex");
  const argvPath = join(directory, "argv");
  const taskPath = join(directory, "task");
  writeFileSync(command, `#!/usr/bin/env bash
printf '%s\n' "$@" > "${argvPath}"
cat > "${taskPath}"
printf '%s\n' '{"type":"thread.started","thread_id":"67e55044-10b1-426f-9247-bb680e5fe0c8"}'
printf '%s\n' '{"type":"turn.started"}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0}}'
`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  const canonical = harnessOptions({
    self: "openai-authority-probe",
    provider: "openai",
    model: "gpt-5.6-terra",
    routingMetadata: applyGafferStaffing({ role: "implementer" }),
    presenceRegistrar: false,
  }) as any;
  // A direct adapter caller cannot widen authority by omitting Claude-shaped
  // deny metadata: Codex derives both hard restrictions from capabilities.
  const options = {
    ...canonical,
    disallowedTools: canonical.disallowedTools.filter(
      (toolName: string) => ![
        "Agent", "Task", "Workflow", "mcp__north__spawn", "mcp__north__dispatch",
      ].includes(toolName),
    ),
  };
  // This case exercises CLI authority compilation, not coordinator transport;
  // carry a one-use admission receipt so the unit test stays hermetic.
  markExecutionAdmission("openai", options);
  for await (const _ of openaiProvider.query({ prompt: "x", options }) as AsyncIterable<any>) {}
  const argv = readFileSync(argvPath, "utf8").trim().split("\n");
  expect(argv[0]).toBe("exec");
  expect(argv).toEqual(expect.arrayContaining([
    "--sandbox", "workspace-write", "--disable", "multi_agent",
  ]));
  expect(argv).toEqual(expect.arrayContaining([
    "--strict-config",
    "--ephemeral",
    "--ignore-user-config",
    "--ignore-rules",
    "--disable", "plugins",
    "--disable", "hooks",
    'project_root_markers=[".git"]',
    `projects.${JSON.stringify(northRoot)}.trust_level="untrusted"`,
    "project_doc_max_bytes=0",
    `mcp_servers.north.command=${JSON.stringify(canonical.mcpServers.north.command)}`,
    "mcp_servers.north.args=[]",
    "mcp_servers.north.enabled=true",
    "mcp_servers.north.required=true",
  ]));
  const developerInstructions = argv.find((argument) =>
    argument.startsWith("developer_instructions="))!;
  expect(developerInstructions).toContain("Gaffer role contract");
  expect(developerInstructions).toContain("Project instructions — Git root to cwd");
  expect(readFileSync(taskPath, "utf8")).toBe("x");
  expect(readFileSync(taskPath, "utf8")).not.toContain("Gaffer role contract");
  expect(argv).toContain(
    `mcp_servers.north.env={NORTH_BIN=${JSON.stringify(canonical.mcpServers.north.env.NORTH_BIN)},`
    + `AGENT_ID=${JSON.stringify(canonical.mcpServers.north.env.AGENT_ID)},`
    + `AGENT_TOPOLOGY=${JSON.stringify(canonical.mcpServers.north.env.AGENT_TOPOLOGY)},`
    + `NORTH_PORT=${JSON.stringify(canonical.mcpServers.north.env.NORTH_PORT)}}`,
  );
  expect(argv).toContain("mcp_servers.north.required=true");
  expect(argv.some((argument) => /mcp_servers\\.(linear|fram)/i.test(argument))).toBe(false);
  expect(argv).toContain('web_search="disabled"');
  const nestedCwd = join(northRoot, "sdk", "src");
  const nestedArgs = codexHarnessArguments({ ...canonical, cwd: nestedCwd });
  expect(nestedArgs).toContain(
    `projects.${JSON.stringify(northRoot)}.trust_level="untrusted"`,
  );
  expect(nestedArgs).not.toContain(
    `projects.${JSON.stringify(nestedCwd)}.trust_level="untrusted"`,
  );

  rmSync(argvPath, { force: true });
  const web = harnessOptions({
    self: "openai-web-admission-proof",
    provider: "openai",
    model: "gpt-5.6-terra",
    routingMetadata: applyGafferStaffing({ role: "scout" }),
    presenceRegistrar: false,
  }) as any;
  expect(() => codexHarnessArguments(web))
    .toThrow("openai_adapter_web_capability_unproven");
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "must not spawn for unproven web authority", options: web,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_adapter_web_capability_unproven");
  expect(existsSync(argvPath)).toBe(false);

  const unsupported = openaiProvider.query({
    prompt: "x",
    options: { ...canonical, northCapabilities: ["filesystem.read"] } as any,
  });
  await expect(async () => {
    for await (const _ of unsupported as AsyncIterable<any>) {}
  }).toThrow("openai_adapter_cannot_enforce_gaffer_capabilities");
  expect(existsSync(argvPath)).toBe(false);

  const ambientTopology = {
    ...canonical,
    env: { ...canonical.env, AGENT_TOPOLOGY: undefined },
  };
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "x", options: ambientTopology,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_managed_identity_topology_contract_missing");
  expect(existsSync(argvPath)).toBe(false);

  const missingDeveloperInstructions = { ...canonical, systemPrompt: "" };
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "x", options: missingDeveloperInstructions,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_developer_instructions_contract_missing");
  expect(existsSync(argvPath)).toBe(false);

  markExecutionAdmission("openai", canonical);
  const admitted = openaiProvider.query({
    prompt: "must not spawn", options: canonical,
  });
  canonical.env.AGENT_TOPOLOGY = undefined;
  await expect(async () => {
    for await (const _ of admitted as AsyncIterable<any>) {}
  }).toThrow("openai_managed_identity_topology_contract_missing");
  expect(existsSync(argvPath)).toBe(false);

  const missingGlobalHome = mkdtempSync(join(tmpdir(), "north-openai-missing-global-"));
  temporary.push(missingGlobalHome);
  const missingGlobal = {
    ...harnessOptions({
      self: "openai-missing-global-proof",
      provider: "openai",
      routingMetadata: applyGafferStaffing({ role: "implementer" }),
      presenceRegistrar: false,
    }) as any,
  };
  missingGlobal.env = { ...missingGlobal.env, HOME: missingGlobalHome };
  markExecutionAdmission("openai", missingGlobal);
  await expect(async () => {
    for await (const _ of openaiProvider.query({
      prompt: "must not spawn", options: missingGlobal,
    }) as AsyncIterable<any>) {}
  }).toThrow("openai_canonical_global_agents_unavailable");
  expect(existsSync(argvPath)).toBe(false);
});

test("the executable Codex adapter rejects orchestrator authority before starting a provider turn", async () => {
  const directory = mkdtempSync(join(tmpdir(), "north-codex-orchestrator-admission-"));
  temporary.push(directory);
  const marker = join(directory, "provider-started");
  const command = join(directory, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash\nprintf started > "${marker}"\n`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;
  process.env.NORTH_PORT = "65534";

  const options = harnessOptions({
    self: "openai-orchestrator-admission-proof",
    provider: "openai",
    cwd: northRoot,
    routingMetadata: applyGafferStaffing({ role: "director" }),
    presenceRegistrar: false,
  }) as any;
  let caught: unknown;
  try {
    for await (const _ of openaiProvider.query({
      prompt: "must not start a provider turn",
      options,
    }) as AsyncIterable<any>) {}
  } catch (error) {
    caught = error;
  }

  expect(caught).toMatchObject({
    code: "blocked_preflight",
    processOutcome: "blocked_preflight",
    retrySafeBeforeAcceptance: true,
  });
  expect((caught as Error).message).toBe(
    "openai_adapter_orchestrator_authority_unavailable",
  );
  expect(existsSync(marker)).toBe(false);
});

test("selected Codex account bootstrap fails during admission before onRoute or provider spawn", async () => {
  let coordinatorLog = "";
  const server = createServer((socket) => {
    socket.once("data", (chunk) => {
      if (chunk.toString("utf8").includes(":for-log")) {
        socket.end("{:version 23}\n");
      } else {
        socket.end(
          `{:reject ["fence required"] :code :log-fence-required :served-log ${JSON.stringify(coordinatorLog)}}\n`,
        );
      }
    });
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const home = mkdtempSync(join(tmpdir(), "north-openai-target-admission-"));
  temporary.push(home);
  process.env.HOME = home;
  process.env.AGENT_LAWS = "on";
  process.env.GAFFER_HOME = realpathSync(join(northRoot, "../gaffer"));
  process.env.NORTH_PORT = String((server.address() as AddressInfo).port);
  process.env.FRAM_LOG = join(home, "north target admission.log");
  coordinatorLog = process.env.FRAM_LOG;
  const codexHome = join(home, ".codex");
  mkdirSync(codexHome);
  writeFileSync(join(codexHome, "AGENTS.md"), "TARGET_ADMISSION_CANONICAL\n");

  const target = {
    id: "codex-broken-target",
    provider: "openai" as const,
    authMode: "isolated" as const,
    profile: "broken-target",
  };
  const targetRoot = join(home, ".local/state/north/accounts/openai/broken-target");
  mkdirSync(targetRoot, { recursive: true });
  writeFileSync(join(targetRoot, "AGENTS.md"), "TARGET_REPLACEMENT_MUST_FAIL\n");

  const marker = join(home, "provider-spawned");
  const command = join(home, "fake-codex");
  writeFileSync(command, `#!/usr/bin/env bash\nprintf spawned > "${marker}"\n`);
  chmodSync(command, 0o700);
  process.env.NORTH_CODEX_BIN = command;

  const options = harnessOptions({
    self: "openai-target-admission-proof",
    provider: "openai",
    cwd: northRoot,
    routingMetadata: applyGafferStaffing({ role: "implementer" }),
    presenceRegistrar: false,
  });
  await expect(openaiProvider.admit!({
    options: {
      ...options,
      env: { ...options.env, AGENT_LAWS: "off" },
    },
    target: { ...target, authMode: "ambient" },
  })).rejects.toThrow("openai_agent_laws_opt_out_unenforceable");
  expect(existsSync(marker)).toBe(false);

  const decision = selectProviderFromAvailability(
    { provider: "openai", target: target.id },
    [{ targetId: target.id, provider: "openai", available: true, reason: "ready" }],
    {
      mode: "balanced",
      targets: [target],
      targetOrder: [target.id],
      providerOrder: ["openai"],
      pressures: { openai: "normal" },
    },
    "economy",
    "target-admission-proof",
    "low",
  );
  let routePublished = false;
  const query = routedQuery(
    decision,
    { prompt: "must not run", options },
    "economy",
    undefined,
    undefined,
    () => { routePublished = true; },
  );
  try {
    await expect(async () => {
      for await (const _ of query as AsyncIterable<any>) {}
    }).toThrow("openai_target_environment_invalid");
    expect(routePublished).toBe(false);
    expect(existsSync(marker)).toBe(false);
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
