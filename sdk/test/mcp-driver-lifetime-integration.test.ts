import { expect, test } from "bun:test";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer, connect } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import {
  framBabashkaArguments,
  framEngineEnvironment,
  framEngineSelection,
} from "../src/fram-engine";
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

async function waitForPort(port: number): Promise<void> {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    const open = await new Promise<boolean>((resolveProbe) => {
      const socket = connect({ host: "127.0.0.1", port });
      socket.once("connect", () => {
        socket.destroy();
        resolveProbe(true);
      });
      socket.once("error", () => resolveProbe(false));
    });
    if (open) return;
    await Bun.sleep(25);
  }
  throw new Error("isolated Fram coordinator did not start");
}

function framCheckout(): string {
  const candidates = [
    process.env.FRAM_TEST_CHECKOUT,
    framEngineSelection(process.env).home,
  ].filter((candidate): candidate is string => Boolean(candidate));
  const found = candidates.find((candidate) =>
    existsSync(resolve(candidate, "coord_daemon.clj"))
  );
  if (!found) throw new Error("Fram checkout unavailable for MCP driver integration test");
  return found;
}

test("real MCP adapter retains its preclaim until the detached child verifies it", async () => {
  const scratch = mkdtempSync(join(tmpdir(), "north-mcp-driver-lifetime-"));
  const log = join(scratch, "facts.log");
  const fakeBun = join(scratch, "bun");
  const fakeNorth = join(scratch, "north");
  const verifyResult = join(scratch, "verify-result");
  const providerMarker = join(scratch, "provider-started");
  const childFixture = resolve(
    north,
    "sdk/test/fixtures/mcp-preclaimed-driver-child.ts",
  );
  const port = await unusedPort();
  writeFileSync(log, "");

  const daemon = Bun.spawn(["bb", ...framBabashkaArguments([
    "coord_daemon.clj", "serve-flat", String(port), log,
  ])], {
    cwd: framCheckout(),
    env: framEngineEnvironment({
      ...process.env,
      FRAM_LOG: log,
      FRAM_REQUIRE_LOG_FENCE: "1",
      FRAM_SINGLE_VALUED: "title driver",
    }),
    stdout: "pipe",
    stderr: "pipe",
  });

  try {
    await waitForPort(port);
    const seed = spawnSync("bb", ["-e", `
      (load-file ${JSON.stringify(resolve(north, "cli/coord.clj"))})
      (let [port ${port}
            base (:version (north.coord/send-op port {:op :version}))
            result (north.coord/send-op
                    port
                    {:op :assert
                     :te "@${thread}"
                     :p "title"
                     :r "MCP detached driver lifetime fixture"
                     :base base})]
        (when (:reject result)
          (binding [*out* *err*] (prn result))
          (System/exit 1)))
    `], {
      encoding: "utf8",
      env: { ...process.env, FRAM_LOG: log, NORTH_PORT: String(port) },
    });
    expect(seed.status, seed.stderr).toBe(0);

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
    const result = spawnSync("bb", [resolve(north, "bin/north-mcp")], {
      input: request,
      encoding: "utf8",
      env: {
        ...process.env,
        HOME: scratch,
        FRAM_LOG: log,
        NORTH_PORT: String(port),
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
      [acquireCli, String(port), "status", thread, "unused"],
      {
        encoding: "utf8",
        env: { ...process.env, FRAM_LOG: log, NORTH_PORT: String(port) },
      },
    );
    expect(status.status, status.stderr).toBe(0);
    expect(status.stdout).toContain(`@${thread} driver=(none)`);
  } finally {
    daemon.kill("SIGTERM");
    await daemon.exited;
    rmSync(scratch, { recursive: true, force: true });
  }
}, 15_000);
