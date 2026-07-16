import { afterEach, expect, test } from "bun:test";
import {
  chmodSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  readlinkSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { codexConfigArguments, providerEnvironmentForTarget } from "../src/accounts";

const root = join(import.meta.dir, "..");
const cli = join(root, "src/account-cli.ts");
const temporaryHomes: string[] = [];

afterEach(() => {
  for (const home of temporaryHomes.splice(0)) rmSync(home, { recursive: true, force: true });
});

function fixture() {
  const home = mkdtempSync(join(tmpdir(), "north-account-test-"));
  temporaryHomes.push(home);
  const claude = join(home, ".claude");
  const codex = join(home, ".codex");
  const bin = join(home, "bin");
  mkdirSync(claude, { recursive: true });
  mkdirSync(codex, { recursive: true });
  mkdirSync(bin, { recursive: true });

  writeFileSync(join(claude, "CLAUDE.md"), "shared claude instructions\n");
  mkdirSync(join(claude, "skills"));
  writeFileSync(join(claude, ".credentials.json"), "never-link-this\n");
  mkdirSync(join(claude, "sessions"));
  writeFileSync(join(codex, "AGENTS.md"), "shared codex instructions\n");
  writeFileSync(join(codex, "config.toml"), "model = 'test'\n");
  writeFileSync(join(codex, "auth.json"), "never-link-this\n");
  mkdirSync(join(codex, "log"));
  writeFileSync(join(codex, "state.sqlite"), "never-link-this\n");

  const fake = join(bin, "fake-provider.cjs");
  writeFileSync(fake, `#!/usr/bin/env node
const fs = require("node:fs");
const path = require("node:path");
const isClaude = Boolean(process.env.CLAUDE_CONFIG_DIR);
const root = isClaude ? process.env.CLAUDE_CONFIG_DIR : process.env.CODEX_HOME;
const record = { argv: process.argv.slice(2), root, sqlite: process.env.CODEX_SQLITE_HOME,
  sensitiveEnvPresent: Boolean(process.env.CLAUDE_PRIVATE_CREDENTIAL || process.env.CODEX_PRIVATE_CREDENTIAL) };
fs.appendFileSync(path.join(process.env.HOME, "calls.jsonl"), JSON.stringify(record) + "\\n");
const login = isClaude ? process.argv[2] === "auth" && process.argv[3] === "login"
  : process.argv[2] === "login" && process.argv[3] !== "status";
if (login) { fs.writeFileSync(path.join(root, "logged-in"), "yes"); process.exit(0); }
const loggedIn = fs.existsSync(path.join(root, "logged-in"));
if (isClaude) console.log(JSON.stringify({ loggedIn, authMethod: loggedIn ? "claude.ai" : "none" }));
else if (loggedIn) console.log("Logged in using ChatGPT");
else console.error("Not logged in");
process.exit(loggedIn ? 0 : 1);
`);
  chmodSync(fake, 0o755);
  const fakeClaude = join(bin, "fake-claude");
  const fakeCodex = join(bin, "fake-codex");
  symlinkSync(fake, fakeClaude);
  symlinkSync(fake, fakeCodex);

  const policy = join(home, ".config/north/routing-policy.json");
  mkdirSync(join(home, ".config/north"), { recursive: true });
  writeFileSync(policy, `${JSON.stringify({
    version: 1,
    mode: "preferential",
    targets: [{ id: "ambient", provider: "anthropic", currentField: "keep-me" }],
    targetOrder: ["ambient"],
    providerOrder: ["anthropic", "openai"],
    futureTopLevel: { preserved: true },
  }, null, 2)}\n`);

  const env = {
    ...process.env,
    HOME: home,
    NORTH_ROUTING_POLICY: policy,
    NORTH_CLAUDE_BIN: fakeClaude,
    NORTH_CODEX_BIN: fakeCodex,
    CLAUDE_PRIVATE_CREDENTIAL: "must-not-propagate",
    CODEX_PRIVATE_CREDENTIAL: "must-not-propagate",
  };
  const run = (...args: string[]) => spawnSync("bun", ["run", cli, ...args], { env, encoding: "utf8" });
  return { home, policy, run };
}

test("add preserves routing fields, isolates roots, and links only allowlisted config", () => {
  const { home, policy, run } = fixture();
  expect(run("add", "claude-work", "anthropic").status).toBe(0);
  expect(run("add", "codex-personal", "openai").status).toBe(0);

  const document = JSON.parse(readFileSync(policy, "utf8"));
  expect(document.futureTopLevel).toEqual({ preserved: true });
  expect(document.targets[0]).toEqual({ id: "ambient", provider: "anthropic", currentField: "keep-me" });
  expect(document.targets.slice(1)).toEqual([
    { id: "claude-work", provider: "anthropic", profile: "claude-work", authMode: "isolated" },
    { id: "codex-personal", provider: "openai", profile: "codex-personal", authMode: "isolated" },
  ]);
  expect(document.targetOrder).toEqual(["ambient", "claude-work", "codex-personal"]);
  expect(statSync(policy).mode & 0o777).toBe(0o600);
  expect(readdirSync(join(home, ".config/north")).filter((name) => name.includes(".tmp") || name.endsWith(".lock"))).toEqual([]);

  const claudeRoot = join(home, ".local/state/north/accounts/anthropic/claude-work");
  const codexRoot = join(home, ".local/state/north/accounts/openai/codex-personal");
  expect(claudeRoot).not.toBe(codexRoot);
  expect(statSync(claudeRoot).mode & 0o777).toBe(0o700);
  expect(statSync(codexRoot).mode & 0o777).toBe(0o700);
  expect(lstatSync(join(claudeRoot, "CLAUDE.md")).isSymbolicLink()).toBe(true);
  expect(readlinkSync(join(claudeRoot, "CLAUDE.md"))).toBe(join(home, ".claude/CLAUDE.md"));
  expect(lstatSync(join(claudeRoot, "skills")).isSymbolicLink()).toBe(true);
  expect(lstatSync(join(codexRoot, "AGENTS.md")).isSymbolicLink()).toBe(true);
  expect(lstatSync(join(codexRoot, "config.toml")).isSymbolicLink()).toBe(true);
  for (const forbidden of [
    join(claudeRoot, ".credentials.json"), join(claudeRoot, "sessions"),
    join(codexRoot, "auth.json"), join(codexRoot, "log"), join(codexRoot, "state.sqlite"),
  ]) expect(() => lstatSync(forbidden)).toThrow();
});

test("concurrent adds serialize the read-append-replace transaction", async () => {
  const { home, policy } = fixture();
  const env = {
    ...process.env,
    HOME: home,
    NORTH_ROUTING_POLICY: policy,
  };
  const first = Bun.spawn(["bun", "run", cli, "add", "claude-one", "anthropic"], {
    env, stdout: "pipe", stderr: "pipe",
  });
  const second = Bun.spawn(["bun", "run", cli, "add", "codex-two", "openai"], {
    env, stdout: "pipe", stderr: "pipe",
  });
  expect(await first.exited).toBe(0);
  expect(await second.exited).toBe(0);
  const document = JSON.parse(readFileSync(policy, "utf8"));
  const ids = document.targets.map((target: { id: string }) => target.id);
  expect([...ids].sort()).toEqual(["ambient", "claude-one", "codex-two"]);
  expect(document.targetOrder).toEqual(ids);
});

test("same isolated target bootstraps safely across concurrent processes", async () => {
  const { home } = fixture();
  const accountRoot = join(home, ".local/state/north/accounts/anthropic/claude-shared");
  const ready = join(home, "bootstrap-ready");
  const start = join(home, "bootstrap-start");
  mkdirSync(ready);
  const workers = 16;
  const program = `
    import { bootstrapAccountConfig } from ${JSON.stringify(join(root, "src/accounts.ts"))};
    import { existsSync, writeFileSync } from "node:fs";
    import { join } from "node:path";
    writeFileSync(join(process.env.READY, String(process.pid)), "ready");
    while (!existsSync(process.env.START))
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 2);
    bootstrapAccountConfig(JSON.parse(process.env.ACCOUNT), { home: process.env.HOME });
  `;
  const env = {
    ...process.env,
    HOME: home,
    READY: ready,
    START: start,
    ACCOUNT: JSON.stringify({
      id: "claude-shared", provider: "anthropic", profile: "claude-shared",
      authMode: "isolated", root: accountRoot,
    }),
  };
  const children = Array.from({ length: workers }, () => Bun.spawn(["bun", "--eval", program], {
    env, stdout: "pipe", stderr: "pipe",
  }));
  for (let attempt = 0; readdirSync(ready).length < workers && attempt < 1000; attempt++) await Bun.sleep(5);
  expect(readdirSync(ready)).toHaveLength(workers);
  writeFileSync(start, "go");
  const exits = await Promise.all(children.map((child) => child.exited));
  const errors = await Promise.all(children.map((child) => new Response(child.stderr).text()));
  expect(exits, errors.join("\n")).toEqual(Array(workers).fill(0));
  for (const name of ["CLAUDE.md", "skills"]) {
    const destination = join(accountRoot, name);
    expect(lstatSync(destination).isSymbolicLink()).toBe(true);
    expect(readlinkSync(destination)).toBe(join(home, ".claude", name));
  }
});

test("unsafe ids are rejected without changing policy or escaping the account root", () => {
  const { home, policy, run } = fixture();
  const before = readFileSync(policy, "utf8");
  for (const id of ["../escape", "nested/account", ".", "..", "account.name", "Uppercase"]) {
    const result = run("add", id, "anthropic");
    expect(result.status).toBe(2);
    expect(result.stderr).toContain("lowercase portable slug");
    expect(readFileSync(policy, "utf8")).toBe(before);
  }
  expect(() => lstatSync(join(home, ".local/state/north/escape"))).toThrow();
});

test("login and status use disjoint provider homes and normalized account identity", () => {
  const { home, run } = fixture();
  expect(run("add", "claude-work", "anthropic").status).toBe(0);
  expect(run("add", "codex-personal", "openai").status).toBe(0);

  writeFileSync(join(home, ".claude/logged-in"), "ambient login must not count\n");
  writeFileSync(join(home, ".codex/logged-in"), "ambient login must not count\n");
  const empty = run("status");
  expect(empty.status).toBe(1);
  expect(empty.stdout).toContain("claude-work\tanthropic\tnot-logged-in");
  expect(empty.stdout).toContain("codex-personal\topenai\tnot-logged-in");

  expect(run("login", "claude-work").status).toBe(0);
  const split = run("status");
  expect(split.status).toBe(1);
  expect(split.stdout).toContain("claude-work\tanthropic\tlogged-in");
  expect(split.stdout).toContain("codex-personal\topenai\tnot-logged-in");
  expect(run("login", "codex-personal").status).toBe(0);
  expect(run("status").status).toBe(0);

  const listed = run("list");
  expect(listed.status).toBe(0);
  expect(listed.stdout).toContain(`claude-work\tanthropic\tclaude-work\t${join(home, ".local/state/north/accounts/anthropic/claude-work")}`);
  expect(listed.stdout).toContain(`codex-personal\topenai\tcodex-personal\t${join(home, ".local/state/north/accounts/openai/codex-personal")}`);

  const calls = readFileSync(join(home, "calls.jsonl"), "utf8").trim().split("\n").map((line) => JSON.parse(line));
  expect(calls.every((call) => call.sensitiveEnvPresent === false)).toBe(true);
  const codexCalls = calls.filter((call) => call.root?.includes("/accounts/openai/"));
  expect(codexCalls.length).toBeGreaterThan(0);
  expect(codexCalls.every((call) => call.sqlite === join(call.root, "sqlite"))).toBe(true);
  expect(codexCalls.every((call) => call.argv.includes('cli_auth_credentials_store="file"'))).toBe(true);
  expect(codexCalls.every((call) => call.argv.includes('forced_login_method="chatgpt"'))).toBe(true);
  expect(codexCalls.every((call) => call.argv.includes('model_provider="openai"'))).toBe(true);
});

test("subscription targets deny hostile provider transports while preserving ordinary environment", () => {
  const { home } = fixture();
  const hostile = {
    HOME: home,
    PATH: process.env.PATH,
    TERM: "xterm-256color",
    NORTH_TRACE: "keep",
    ANTHROPIC_LOG: "keep-anthropic-log",
    OPENAI_LOG: "keep-openai-log",
    AWS_MAX_ATTEMPTS: "7",
    ANTHROPIC_API_KEY: "canary",
    CLAUDE_CODE_OAUTH_TOKEN: "canary",
    ANTHROPIC_BASE_URL: "https://hostile.invalid",
    ANTHROPIC_CUSTOM_HEADERS: "Authorization: canary",
    CLAUDE_CODE_USE_BEDROCK: "1",
    CLAUDE_CODE_SKIP_VERTEX_AUTH: "1",
    ANTHROPIC_VERTEX_BASE_URL: "https://hostile.invalid",
    AWS_ACCESS_KEY_ID: "canary",
    AWS_SECRET_ACCESS_KEY: "canary",
    AWS_PROFILE: "hostile",
    AWS_REGION: "hostile-1",
    AWS_SHARED_CREDENTIALS_FILE: "/tmp/canary",
    GOOGLE_APPLICATION_CREDENTIALS: "/tmp/canary",
    GOOGLE_API_KEY: "canary",
    CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE: "/tmp/canary",
    CLOUD_ML_REGION: "hostile-1",
    AZURE_OPENAI_ENDPOINT: "https://hostile.invalid",
    AZURE_CLIENT_SECRET: "canary",
    AZURE_SUBSCRIPTION_ID: "canary",
    OPENAI_API_KEY: "canary",
    OPENAI_BASE_URL: "https://hostile.invalid",
    OPENAI_DEFAULT_HEADERS: "Authorization: canary",
    OPENAI_API_TYPE: "azure",
    CHATGPT_BASE_URL: "https://hostile.invalid",
    BOTO_CONFIG: "/tmp/canary",
    CODEX_PRIVATE_CREDENTIAL: "canary",
    CLAUDE_CONFIG_DIR: "/tmp/hostile-claude",
    CODEX_HOME: "/tmp/hostile-codex",
    CODEX_SQLITE_HOME: "/tmp/hostile-sqlite",
    CODEX_PROFILE: "hostile",
  } satisfies NodeJS.ProcessEnv;
  const forbidden = Object.keys(hostile).filter((key) => ![
    "HOME", "PATH", "TERM", "NORTH_TRACE", "ANTHROPIC_LOG", "OPENAI_LOG", "AWS_MAX_ATTEMPTS",
    "CLAUDE_CONFIG_DIR", "CODEX_HOME", "CODEX_SQLITE_HOME",
  ].includes(key));

  for (const [provider, profile] of [["anthropic", "claude-safe"], ["openai", "codex-safe"]] as const) {
    const env = providerEnvironmentForTarget(provider, {
      id: profile, provider, profile, authMode: "isolated",
    }, { home, env: hostile });
    for (const key of forbidden) expect(env[key]).toBeUndefined();
    expect(env).toMatchObject({
      HOME: home,
      TERM: "xterm-256color",
      NORTH_TRACE: "keep",
      ANTHROPIC_LOG: "keep-anthropic-log",
      OPENAI_LOG: "keep-openai-log",
      AWS_MAX_ATTEMPTS: "7",
    });
    const root = join(home, ".local/state/north/accounts", provider, profile);
    if (provider === "anthropic") {
      expect(env.CLAUDE_CONFIG_DIR).toBe(root);
      expect(env.CODEX_HOME).toBeUndefined();
    } else {
      expect(env.CODEX_HOME).toBe(root);
      expect(env.CODEX_SQLITE_HOME).toBe(join(root, "sqlite"));
      expect(env.CLAUDE_CONFIG_DIR).toBeUndefined();
      expect(codexConfigArguments(env)).toContain('model_provider="openai"');
    }
  }
});
