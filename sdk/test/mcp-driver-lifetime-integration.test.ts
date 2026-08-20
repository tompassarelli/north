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
import {
  beagleStoreBabashkaArguments,
  beagleStoreEnvironment,
} from "../src/beagle-store";
import { StoreRpcClient } from "../src/store-rpc-client";
import { triple } from "../src/store-rpc-codec";
import { presetRequest } from "./routing-fixtures";

const north = resolve(import.meta.dir, "../..");
const orchestration = resolve(north, "orchestration");
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
      BEAGLE_STORE_SERVER_RUNTIME: "jvm-dev",
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
        NORTH_ORCHESTRATION_HOME: orchestration,
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
