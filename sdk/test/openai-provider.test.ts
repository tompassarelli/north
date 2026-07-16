import { afterEach, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openaiProvider } from "../src/providers/openai";
import { ProviderRetrySafeError } from "../src/providers";

const savedBin = process.env.NORTH_CODEX_BIN;
const savedHome = process.env.HOME;
const temporary: string[] = [];
afterEach(() => {
  if (savedBin === undefined) delete process.env.NORTH_CODEX_BIN;
  else process.env.NORTH_CODEX_BIN = savedBin;
  if (savedHome === undefined) delete process.env.HOME;
  else process.env.HOME = savedHome;
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

test("Codex adapter owns the cumulative total and does not double-count subsets", async () => {
  const result = await resultFromScript([
    JSON.stringify({ type: "turn.completed", usage: {
      input_tokens: 100, cached_input_tokens: 60,
      output_tokens: 20, reasoning_output_tokens: 7,
    } }),
  ]);
  expect(result.usage).toEqual({
    input_tokens: 100, cached_input_tokens: 60,
    output_tokens: 20, reasoning_output_tokens: 7,
  });
  expect(result._north_usage).toEqual({
    provider: "openai", terminal_count: 1,
    scope: "codex_fresh_invocation_thread_cumulative",
    total_status: "exact", total_tokens: 120,
  });
});

test("Codex preserves only present counters and zero completed terminals stays unknown", async () => {
  const incomplete = await resultFromScript([
    JSON.stringify({ type: "turn.completed", usage: { input_tokens: 0 } }),
  ]);
  expect(incomplete.usage).toEqual({ input_tokens: 0 });
  expect(incomplete._north_usage).toMatchObject({
    terminal_count: 1, total_status: "unknown_incomplete_terminal",
  });
  expect(incomplete._north_usage).not.toHaveProperty("total_tokens");

  const absent = await resultFromScript([]);
  expect(absent.usage).toEqual({});
  expect(absent._north_usage).toMatchObject({ terminal_count: 0, total_status: "unknown_no_terminal" });
});

test("repeated Codex completed events use the last cumulative snapshot explicitly", async () => {
  const result = await resultFromScript([
    JSON.stringify({ type: "turn.completed", usage: { input_tokens: 5, output_tokens: 1 } }),
    JSON.stringify({ type: "turn.completed", usage: {
      input_tokens: 9, cached_input_tokens: 4, output_tokens: 2, reasoning_output_tokens: 1,
    } }),
  ]);
  expect(result.usage).toEqual({
    input_tokens: 9, cached_input_tokens: 4, output_tokens: 2, reasoning_output_tokens: 1,
  });
  expect(result._north_usage).toMatchObject({ terminal_count: 2, total_status: "exact", total_tokens: 11 });
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
printf '{"type":"item.completed","item":{"type":"agent_message","text":"%s"}}\n' "$CODEX_HOME"
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}'
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
