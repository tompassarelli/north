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
  framBabashkaArguments,
  framEngineEnvironment,
  framEngineSelection,
} from "../src/fram-engine";
import { FramRpcClient } from "../src/framrpc-client";
import { triple } from "../src/framrpc-codec";
import { presetRequest } from "./routing-fixtures";

const north = resolve(import.meta.dir, "../..");
const orchestration = resolve(north, "orchestration");
const acquireCli = resolve(north, "cli/acquire-cli.clj");
const thread = "019fa4ec-d2e6-7f8f-b375-a4f2ea407a0c";

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

async function waitForFramServer(port: number, spaceId: string): Promise<FramRpcClient> {
  for (let attempt = 0; attempt < 400; attempt += 1) {
    try {
      return await FramRpcClient.connect({
        port, spaceId, connectTimeoutMs: 100, readTimeoutMs: 500,
        maxAttempts: 1, retryDelayMs: 0, jitterMs: 0,
      });
    } catch {}
    await Bun.sleep(25);
  }
  throw new Error("isolated Fram server did not become FRAMRPC-ready");
}

function framServerFixture(): {
  home: string; bin: string; out: string; server: string;
} {
  const selected = framEngineSelection(process.env);
  const home = process.env.FRAM_TEST_CHECKOUT ?? selected.home;
  const bin = process.env.FRAM_TEST_CHECKOUT ? resolve(home, "bin") : selected.bin;
  const out = process.env.FRAM_TEST_CHECKOUT ? resolve(home, "out") : selected.out;
  const server = resolve(bin, "fram-server");
  if (!existsSync(server))
    throw new Error("current Fram bin/fram-server is unavailable for the MCP fixture");
  return { home, bin, out, server };
}

test("real MCP adapter retains its preclaim until the detached child verifies it", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "north-mcp-driver-lifetime-"));
  const log = join(scratch, "history.framlog");
  const spaceId = "north-mcp-driver-lifetime";
  const fram = framServerFixture();
  const fakeBun = join(scratch, "bun");
  const fakeNorth = join(scratch, "north");
  const verifyResult = join(scratch, "verify-result");
  const providerMarker = join(scratch, "provider-started");
  const childFixture = resolve(
    north,
    "sdk/test/fixtures/mcp-preclaimed-driver-child.ts",
  );
  const port = await unusedPort();
  const coordinationEnvironment = framEngineEnvironment({
    ...process.env,
    NORTH_PORT: String(port),
    FRAM_LOG: log,
    FRAM_SPACE_ID: spaceId,
    FRAM_HOME: fram.home,
    FRAM_BIN: fram.bin,
    FRAM_OUT: fram.out,
    BABASHKA_CLASSPATH: fram.out,
    FRAM_SINGLE_VALUED: "title driver",
  });
  const serverProcess = Bun.spawn([
    fram.server, "serve", String(port), log, spaceId,
  ], {
    cwd: fram.home,
    env: {
      ...coordinationEnvironment,
      FRAM_SERVER_RUNTIME: "jvm-dev",
      FRAM_SERVER_QUIET: "1",
      FRAM_SERVER_XMX: "1g",
      FRAM_SNAPSHOT_BOOT: "0",
    },
    stdout: "pipe",
    stderr: "pipe",
  });

  let serverClient: FramRpcClient | undefined;
  try {
    serverClient = await waitForFramServer(port, spaceId);
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
    const result = spawnSync("bb", framBabashkaArguments([
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
      framBabashkaArguments(
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
