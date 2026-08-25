import { expect, test } from "bun:test";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  beagleStoreBabashkaArguments,
  beagleStoreEnvironment,
} from "../src/beagle-store";
import { StoreRpcClient } from "../src/store-rpc-client";
import { triple } from "../src/store-rpc-codec";
import { StoreBridgeCommandReceipts } from "../src/bridge/command-receipts";
import { loadStoreSnapshot } from "../src/store-kernel-loader";
import { safeNext } from "../src/store-kernel";
import { presetRequest } from "./routing-fixtures";

const north = resolve(import.meta.dir, "../..");
const agentMachinery = process.env.AGENT_MACHINERY_HOME ?? "/home/tom/code/agent-machinery/main";
const agentRuntime = process.env.NORTH_AGENT_RUNTIME_HOME ?? resolve(north, "agent-runtime/orchestration");
const acquireCli = resolve(north, "cli/acquire-cli.clj");
const thread = "019fa4ec-d2e6-7f8f-b375-a4f2ea407a0c";
const frozenStoreHome = process.env.BEAGLE_STORE_TEST_CHECKOUT
  ?? process.env.BEAGLE_STORE_HOME
  ?? "/home/tom/code/beagle/main/store";

async function unusedPort(): Promise<number> {
  const server = createServer();
  await new Promise<void>((resolveListen, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolveListen);
  });
  const address = server.address();
  if (!address || typeof address === "string")
    throw new Error("failed to allocate test port");
  await new Promise<void>((resolveClose) => server.close(() => resolveClose()));
  return address.port;
}

async function waitForStoreServer(port: number, spaceId: string): Promise<StoreRpcClient> {
  for (let attempt = 0; attempt < 400; attempt += 1) {
    try {
      return await StoreRpcClient.connect({
        port, spaceId, connectTimeoutMs: 100, readTimeoutMs: 500,
        maxAttempts: 1, retryDelayMs: 0, jitterMs: 0,
      });
    } catch {}
    await Bun.sleep(25);
  }
  throw new Error("isolated Beagle Store server did not become Store RPC-ready");
}

function storeServerFixture(): {
  home: string; bin: string; out: string; server: string;
} {
  const home = frozenStoreHome;
  const bin = resolve(home, "bin");
  const out = resolve(home, "out");
  const server = resolve(bin, "beagle-store-server");
  if (!existsSync(server))
    throw new Error("frozen Beagle Store bin/beagle-store-server is unavailable for the MCP fixture");
  return { home, bin, out, server };
}

test("real MCP adapter retains its preclaim until the detached child verifies it", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "north-mcp-driver-lifetime-"));
  const log = join(scratch, "history.storelog");
  const spaceId = "north-mcp-driver-lifetime";
  const store = storeServerFixture();
  const fakeBun = join(scratch, "bun");
  const fakeNorth = join(scratch, "north");
  const verifyResult = join(scratch, "verify-result");
  const providerMarker = join(scratch, "provider-started");
  const childFixture = resolve(
    north,
    "sdk/test/fixtures/mcp-preclaimed-driver-child.ts",
  );
  const port = await unusedPort();
  const coordinationEnvironment = beagleStoreEnvironment({
    ...process.env,
    NORTH_PORT: String(port),
    BEAGLE_STORE_SERVER_PORT: String(port),
    BEAGLE_STORE_LOG: log,
    BEAGLE_STORE_SPACE_ID: spaceId,
    NORTH_TELEMETRY_SPACE_ID: "north-telemetry",
    NORTH_TELEMETRY_PORT: String(port === 65535 ? port - 1 : port + 1),
    NORTH_TELEMETRY_PARTITION: "0",
    BEAGLE_STORE_HOME: store.home,
    BEAGLE_STORE_BIN: store.bin,
    BEAGLE_STORE_OUT: store.out,
    BABASHKA_CLASSPATH: store.out,
    BEAGLE_STORE_SINGLE_VALUED: "title driver",
  });
  const serverProcess = Bun.spawn([
    store.server, "serve", String(port), log, spaceId,
  ], {
    cwd: store.home,
    env: {
      ...coordinationEnvironment,
      BEAGLE_STORE_SERVER_QUIET: "1",
      BEAGLE_STORE_SERVER_XMX: "1g",
      BEAGLE_STORE_SNAPSHOT_BOOT: "0",
    },
    stdout: "pipe",
    stderr: "pipe",
  });

  let serverClient: StoreRpcClient | undefined;
  try {
    serverClient = await waitForStoreServer(port, spaceId);
    const seeded = await serverClient.batch([{
      op: "assert",
      proposition: triple(
        `@${thread}`, "title", "MCP detached driver lifetime fixture",
      ),
    }]);
    expect(seeded.results).toHaveLength(1);

    writeFileSync(fakeNorth, "#!/usr/bin/env bash\nprintf '%s\\n' '[]'\n");
    chmodSync(fakeNorth, 0o755);
    writeFileSync(fakeBun, `#!/usr/bin/env bash
set -euo pipefail
thread="\${@: -1}"
exec "$NORTH_TEST_REAL_BUN" "$NORTH_TEST_CHILD_FIXTURE" "$thread"
`);
    chmodSync(fakeBun, 0o755);

    const request = `${JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: {
        name: "dispatch",
        arguments: { id: thread, ...presetRequest("verifier") },
      },
    })}\n`;
    const result = spawnSync("bb", beagleStoreBabashkaArguments([
      resolve(north, "bin/north-mcp"),
    ], coordinationEnvironment), {
      input: request,
      encoding: "utf8",
      env: {
        ...coordinationEnvironment,
        HOME: scratch,
        NORTH_BIN: fakeNorth,
        NORTH_MCP_BUN: fakeBun,
        NORTH_POLICY_BUN: process.execPath,
        AGENT_MACHINERY_HOME: agentMachinery,
        NORTH_AGENT_RUNTIME_HOME: agentRuntime,
        NORTH_SPAWN_STARTUP_TIMEOUT_MS: "2000",
        NORTH_TEST_REAL_BUN: process.execPath,
        NORTH_TEST_CHILD_FIXTURE: childFixture,
        NORTH_TEST_VERIFY_RESULT: verifyResult,
        NORTH_TEST_PROVIDER_MARKER: providerMarker,
        NO_COLOR: "1",
      },
    });
    expect(result.status, result.stderr).toBe(0);
    const response = JSON.parse(result.stdout.trim());
    expect(response.result.isError).toBe(true);
    expect(response.result.content[0].text).toContain(
      "child exited before startup acknowledgement (exit 23)",
    );
    expect(readFileSync(verifyResult, "utf8")).toContain(`VERIFIED @${thread} by sdk-`);
    expect(existsSync(providerMarker)).toBe(false);

    // The abnormal child deliberately does not release. The adapter must
    // retract its exact holder after observing the detached exit receipt.
    const status = spawnSync(
      "bb",
      beagleStoreBabashkaArguments(
        [acquireCli, String(port), "status", thread, "unused"],
        coordinationEnvironment,
      ),
      {
        encoding: "utf8",
        env: coordinationEnvironment,
      },
    );
    expect(status.status, status.stderr).toBe(0);
    expect(status.stdout).toContain(`@${thread} driver=(none)`);
  } finally {
    serverClient?.close();
    serverProcess.kill("SIGTERM");
    await serverProcess.exited;
    rmSync(scratch, { recursive: true, force: true });
  }
}, 30_000);

test("real Store restart reconstructs authoritative attempts without a duplicate launch or command", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "north-store-restart-case-"));
  const restartHome = mkdtempSync(join(tmpdir(), "north-store-restart-home-"));
  const secondRestartHome = mkdtempSync(join(tmpdir(), "north-store-restart-home-"));
  const log = join(scratch, "history.storelog");
  const spaceId = "north-store-restart-case";
  const store = storeServerFixture();
  const port = await unusedPort();
  const digest = (value: string) => createHash("sha256").update(value).digest("hex");
  const attemptA = `@attempt:${digest("attempt-a")}`;
  const attemptB = `@attempt:${digest("attempt-b")}`;
  const environment = (home: string) => beagleStoreEnvironment({
    ...process.env, HOME: home, NORTH_PORT: String(port), BEAGLE_STORE_SERVER_PORT: String(port),
    BEAGLE_STORE_LOG: log, BEAGLE_STORE_SPACE_ID: spaceId, BEAGLE_STORE_HOME: store.home,
    BEAGLE_STORE_BIN: store.bin, BEAGLE_STORE_OUT: store.out, BABASHKA_CLASSPATH: store.out,
  });
  const start = (home: string) => Bun.spawn([store.server, "serve", String(port), log, spaceId], {
    cwd: store.home, env: { ...environment(home), BEAGLE_STORE_SERVER_QUIET: "1", BEAGLE_STORE_SNAPSHOT_BOOT: "0" },
    stdout: "pipe", stderr: "pipe",
  });
  const facts = (attempt: string, account: string, threadId: string, run: string, state: "launching" | "started" | "unsent") => {
    const subject = `@account:${account}`;
    const common = [
      triple(subject, "kind", "provider_account"), triple(subject, "account_id", account),
      triple(subject, "provider", "openai"), triple(subject, "provider_profile", account),
      triple(subject, "account_role", "execution"),
      triple(subject, "execution_eligible", "true"), triple(attempt, "kind", "execution_attempt"),
      triple(attempt, "execution_attempt_version", "north:execution-attempt:v1"),
      triple(attempt, "execution_attempt_manifest_sha256", attempt.slice("@attempt:".length)),
      triple(attempt, "execution_attempt_run", run), triple(attempt, "execution_attempt_thread", threadId),
      triple(attempt, "execution_attempt_reporter", "@agent:restart-case"), triple(attempt, "execution_attempt_ordinal", "1"),
      triple(attempt, "execution_attempt_account", account), triple(attempt, "execution_attempt_provider", "openai"),
      triple(attempt, "execution_attempt_model", "gpt-5"), triple(attempt, "execution_attempt_account_authority_sha256", digest(`authority-${account}`)),
      triple(attempt, "execution_attempt_route_observation_sha256", digest(`route-${account}`)),
      triple(attempt, "execution_attempt_run_capability_sha256", digest(`capability-${run}`)),
      triple(attempt, "execution_attempt_run_contract_sha256", digest(`contract-${run}`)),
      triple(attempt, "execution_attempt_reserved_at", "2026-08-20T12:00:00Z"),
      triple(attempt, "execution_attempt_thread_lease", JSON.stringify({ resource: `thread:${threadId.slice("@thread:".length)}:dispatch`, holder: "restart-case", epoch: 1 })),
      triple(attempt, "execution_attempt_account_lease", JSON.stringify({ resource: `codex-account:${account}:slot:0`, holder: "restart-case", epoch: 1 })),
      triple(attempt, "execution_attempt_launch_intent_version", "north:execution-attempt-launch-intent:v1"),
      triple(attempt, "execution_attempt_launch_intent_at", "2026-08-20T12:00:01Z"), triple(attempt, "execution_attempt_launch_intent_sha256", digest(`launch-${attempt}`)),
    ];
    return state === "started" ? [...common,
      triple(attempt, "execution_attempt_provider_start_receipt_sha256", digest(`start-receipt-${attempt}`)),
      triple(attempt, "execution_attempt_provider_start_manifest_sha256", digest(`start-${attempt}`)),
      triple(attempt, "execution_attempt_provider_started_at", "2026-08-20T12:00:02Z"),
    ] : state === "unsent" ? [...common,
      triple(attempt, "execution_attempt_unsent_receipt_sha256", digest(`unsent-receipt-${attempt}`)),
      triple(attempt, "execution_attempt_unsent_manifest_sha256", digest(`unsent-${attempt}`)),
      triple(attempt, "execution_attempt_unsent_at", "2026-08-20T12:00:02Z"),
    ] : common;
  };
  let serverProcess = start(scratch);
  let client: StoreRpcClient | undefined;
  try {
    client = await waitForStoreServer(port, spaceId);
    const oversight = "@account:oversight";
    await client.batch([...facts(attemptA, "execution-a", "@thread:alpha", "run-alpha", "launching"),
      ...facts(attemptB, "execution-b", "@thread:beta", "run-beta", "unsent"),
      triple(oversight, "kind", "provider_account"), triple(oversight, "account_id", "oversight"),
      triple(oversight, "provider", "openai"), triple(oversight, "account_role", "oversight"),
      triple(oversight, "execution_eligible", "false"),
      triple("@run:wire-alpha", "kind", "wire_event"), triple("@run:wire-alpha", "wire_run_id", "run-alpha"),
      triple("@run:wire-alpha", "wire_event_sequence", "0"), triple("@run:wire-alpha", "wire_event_json", "{}"),
      triple("@run:wire-alpha", "wire_event_sha256", digest("wire-alpha")),
    ].map((proposition) => ({ op: "assert" as const, proposition })));
    const beforeCrash = await loadStoreSnapshot({ attemptId: attemptA, client });
    expect(safeNext(beforeCrash.snapshot)).toMatchObject({ kind: "reconcile-launch", attempt: { subject: attemptA }, replayPosition: 1 });
    client.close(); client = undefined;
    serverProcess.kill("SIGTERM"); await serverProcess.exited;

    serverProcess = start(restartHome); // Fresh HOME: only the Store log survives the coordinator restart.
    client = await waitForStoreServer(port, spaceId);
    const recoveredA = await loadStoreSnapshot({ attemptId: attemptA, client });
    expect(recoveredA).toEqual(beforeCrash);
    expect(recoveredA.snapshot.facts.find((fact) => fact.kind === "reserved")?.attempt.subject).toBe(attemptA);
    expect(recoveredA.snapshot.facts.find((fact) => fact.kind === "reserved")?.provenance).toContain("execution_attempt_manifest_sha256");
    expect(safeNext(recoveredA.snapshot)).toMatchObject({ kind: "reconcile-launch", attempt: { subject: attemptA }, replayPosition: 1 });
    await client.batch([
      triple(attemptA, "execution_attempt_provider_start_receipt_sha256", digest("start-receipt-a")),
      triple(attemptA, "execution_attempt_provider_start_manifest_sha256", digest("start-a")),
      triple(attemptA, "execution_attempt_provider_started_at", "2026-08-20T12:00:02Z"),
    ].map((proposition) => ({ op: "assert" as const, proposition })));
    const receipts = new StoreBridgeCommandReceipts(client);
    await receipts.bindExecution("execution-alpha", attemptA, {
      provider: "openai", model: "gpt-5",
    });
    const command = await receipts.admit({ executionId: "execution-alpha", attemptId: attemptA,
      kind: "submit-input", payloadDigest: digest("input"), payloadArtifact: "fake-provider:input",
      delivery: "queued-next-turn" });
    await receipts.commitIntent(command);
    client.close(); client = undefined;
    serverProcess.kill("SIGTERM"); await serverProcess.exited;

    serverProcess = start(secondRestartHome);
    client = await waitForStoreServer(port, spaceId);
    expect(safeNext((await loadStoreSnapshot({ attemptId: attemptA, client })).snapshot))
      .toMatchObject({ kind: "reconcile-command", attempt: { subject: attemptA }, replayPosition: 1 });
    await client.batch([
      triple(attemptA, "execution_attempt_terminal_receipt_sha256", digest("terminal-receipt")),
      triple(attemptA, "execution_attempt_terminal_manifest_sha256", digest("terminal")),
      triple(attemptA, "execution_attempt_terminal_at", "2026-08-20T12:00:03Z"),
    ].map((proposition) => ({ op: "assert" as const, proposition })));
    expect(safeNext((await loadStoreSnapshot({ attemptId: attemptA, client })).snapshot))
      .toMatchObject({ kind: "advance", attempt: { subject: attemptA }, replayPosition: 1 });
    expect(safeNext((await loadStoreSnapshot({ attemptId: attemptB, client })).snapshot)).toMatchObject({ kind: "advance", attempt: { subject: attemptB }, replayPosition: 0 });
    expect(safeNext((await loadStoreSnapshot({ accountId: "oversight", client })).snapshot)).toEqual({ kind: "no-op", reason: "oversight-account", replayPosition: 0 });
    expect(command.ordinal).toBe(1);
  } finally {
    client?.close(); serverProcess.kill("SIGTERM"); await serverProcess.exited;
    rmSync(scratch, { recursive: true, force: true }); rmSync(restartHome, { recursive: true, force: true });
    rmSync(secondRestartHome, { recursive: true, force: true });
  }
}, 60_000);
