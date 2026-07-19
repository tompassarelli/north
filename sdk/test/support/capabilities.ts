// Capability-honest test gating.
//
// Some SDK tests exercise real OS facilities the runner may not have:
//   - "loopback-bind": binding AND connecting a TCP socket on 127.0.0.1
//     (a usable loopback / network namespace — a bind can succeed while the
//     interface is DOWN, so a connect round-trip is the honest probe);
//   - "user-namespace": bubblewrap can build the read-only shell sandbox
//     (unprivileged user namespaces + seccomp — exactly what runReadonlyShell
//     needs).
//
// A gate fires ONLY when the probe (or a forced-false override) says the
// capability is genuinely absent. Every fired gate prints an explicit reason so
// a degraded run is never a silent hole. Strict mode
// (NORTH_TEST_STRICT_CAPABILITIES=1) turns any fired gate into a hard FAILURE
// instead of a skip: full-capability CI runs strict and must show zero skips.
import { test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer, connect } from "node:net";
import { join, resolve } from "node:path";
import { preflightReadonlyShell, ReadonlyShellUnavailableError } from "../../src/readonly-shell";

export type Capability = "loopback-bind" | "user-namespace";

const REPO_ROOT = resolve(import.meta.dir, "../../..");

const REASON: Record<Capability, string> = {
  "loopback-bind":
    "no usable 127.0.0.1 loopback (bind+connect round-trip failed — network namespace has no up loopback)",
  "user-namespace":
    "bubblewrap read-only shell sandbox unavailable (no unprivileged user namespaces / seccomp)",
};

const FORCE_ENV: Record<Capability, string> = {
  "loopback-bind": "NORTH_TEST_FORCE_NO_LOOPBACK",
  "user-namespace": "NORTH_TEST_FORCE_NO_USERNS",
};

export function strictCapabilities(): boolean {
  return process.env.NORTH_TEST_STRICT_CAPABILITIES === "1";
}

function forcedUnavailable(capability: Capability): boolean {
  return process.env[FORCE_ENV[capability]] === "1";
}

async function probeLoopbackBind(): Promise<boolean> {
  return await new Promise<boolean>((resolveProbe) => {
    const server = createServer((socket) => socket.end("north-loopback-probe"));
    let settled = false;
    const done = (usable: boolean) => {
      if (settled) return;
      settled = true;
      try { server.close(); } catch { /* already closing */ }
      resolveProbe(usable);
    };
    server.once("error", () => done(false));
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as { port: number };
      const client = connect(port, "127.0.0.1");
      client.setTimeout(1_500, () => { client.destroy(); done(false); });
      client.once("error", () => done(false));
      client.once("data", () => { client.end(); done(true); });
    });
  });
}

function probeUserNamespace(): boolean {
  let dir: string | undefined;
  try {
    dir = mkdtempSync(join(REPO_ROOT, ".north-cap-probe-"));
    preflightReadonlyShell(dir);
    return true;
  } catch (error) {
    if (error instanceof ReadonlyShellUnavailableError) return false;
    throw error;
  } finally {
    if (dir) { try { rmSync(dir, { recursive: true, force: true }); } catch { /* best effort */ } }
  }
}

async function detect(capability: Capability): Promise<boolean> {
  if (forcedUnavailable(capability)) return false;
  return capability === "loopback-bind" ? await probeLoopbackBind() : probeUserNamespace();
}

// Resolved once at module load (test files await this import via top-level await).
export const capabilityAvailable: Record<Capability, boolean> = {
  "loopback-bind": await detect("loopback-bind"),
  "user-namespace": await detect("user-namespace"),
};

/**
 * Define a test that requires an OS capability. When available it runs
 * normally. When genuinely unavailable it prints an explicit reason and either
 * skips (default) or, under strict mode, becomes a hard failure so CI can prove
 * zero silent skips.
 */
export function gatedTest(
  capability: Capability,
  name: string,
  fn: Parameters<typeof test>[1],
): void {
  if (capabilityAvailable[capability]) {
    test(name, fn);
    return;
  }
  const reason = `capability "${capability}" unavailable: ${REASON[capability]}`;
  if (strictCapabilities()) {
    test(name, () => {
      throw new Error(
        `strict capability mode: ${reason}. This environment cannot honestly run "${name}".`,
      );
    });
    return;
  }
  console.log(`[capability-gate] SKIP "${name}" — ${reason}`);
  test.skip(name, fn);
}
