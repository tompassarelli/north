import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  readSealedStoreRelease, runOfflineStorePersistenceProof,
  type OfflineProofChild, type OfflineProofLaunch,
} from "../src/store-offline-proof";

const temporary: string[] = [];
afterEach(() => { for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true }); });

async function releaseFixture() {
  const root = mkdtempSync(join(tmpdir(), "north-offline-proof-"));
  temporary.push(root);
  const artifact = join(root, "artifact");
  mkdirSync(join(artifact, "bin"), { recursive: true });
  const closure = "a".repeat(64);
  const server = join(artifact, "bin", "beagle-store-server-native");
  writeFileSync(server, "fake sealed server\n"); chmodSync(server, 0o755);
  const serverSha = createHash("sha256").update(readFileSync(server)).digest("hex");
  writeFileSync(join(artifact, "READY"), `beagle-store-native-build/v1 ${closure}\n`);
  const release = join(root, "release"); mkdirSync(release);
  writeFileSync(join(release, "RELEASE"), [
    "format=north-store-release/v1", "source=fake-source", "revision=fake-revision", "tree=fake-tree",
    `native_artifact_dir=${artifact}`, `native_closure_sha256=${closure}`,
    `server_artifact_sha256=${serverSha}`, "created=2026-08-21T00:00:00Z",
  ].join("\n") + "\n");
  return { root, release: await readSealedStoreRelease(release) };
}

function fakeChild(): { child: OfflineProofChild; killed: string[] } {
  const killed: string[] = [];
  const exited = Promise.withResolvers<number>();
  return { killed, child: { exited: exited.promise, kill(signal) { killed.push(signal); exited.resolve(0); } } };
}

test("offline proof binds one exact release across two launches and fully reaps", async () => {
  const fixture = await releaseFixture();
  const launches: OfflineProofLaunch[] = [];
  const releases: number[] = [];
  const children: ReturnType<typeof fakeChild>[] = [];
  const calls: string[] = [];
  const observation = {
    targetId: "store-release-proof", provider: "openai" as const,
    source: "codex-app-server:account-rate-limits", observedAt: "2026-08-21T00:00:00.000Z", state: "normal" as const,
  };
  const receipt = await runOfflineStorePersistenceProof({
    release: fixture.release, scratchRoot: fixture.root, observation,
    awaitReady: async ({ release, listener }) => { expect(release).toBe(fixture.release); expect(listener.fd).toBe(3); },
  }, {
    reserveListener: async () => ({ fd: 3, port: 49000 + launches.length, release: async () => { releases.push(1); } }),
    launch: async (launch) => { launches.push(launch); const child = fakeChild(); children.push(child); return child.child; },
    createClient: (options) => ({
      status: async () => { calls.push(`status:${options.maxAttempts}`); return { attempts: 1, servedVersion: 7, state: { name: "ready" }, engine: { name: "rpc/native" } }; },
      scan: async () => { calls.push(`scan:${options.maxAttempts}`); return { attempts: 1, servedVersion: 7, page: null, rows: [] }; },
      close: () => { calls.push("close"); },
    }) as any,
    persistObservation: async (value, path, client) => { calls.push(`persist:${path}`); expect(value).toEqual(observation); expect(client).toBeDefined(); },
    loadObservation: async (value) => ({ observation: value, receipt: {
      version: "north:provider-observation:v1", subject: "@provider-observation:usage:9720b148782292ef02a5b0921e430d51e0fa23e558d67a0d1394bd6b2c9416df",
      digest: "b".repeat(64), servedVersion: 9,
    } }),
    settleChild: async (child) => { child.kill("SIGTERM"); await child.exited; },
  });
  expect(launches.map(({ run, storeLog, spaceId, env }) => ({ run, storeLog, spaceId, fd: env.BEAGLE_STORE_LISTEN_FD }))).toEqual([
    { run: "initial", storeLog: join(fixture.root, "coordination.storelog"), spaceId: `north-store-release-proof-${"a".repeat(12)}`, fd: "3" },
    { run: "restart", storeLog: join(fixture.root, "coordination.storelog"), spaceId: `north-store-release-proof-${"a".repeat(12)}`, fd: "3" },
  ]);
  expect(calls).toEqual(["status:1", "scan:1", `persist:${join(fixture.root, "provider-usage-observations.json")}`, "close", "close"]);
  expect(children.map(({ killed }) => killed)).toEqual([["SIGTERM"], ["SIGTERM"]]);
  expect(releases).toHaveLength(2);
  expect(receipt.releaseSha256).toBe(fixture.release.releaseSha256);
});

test("offline proof failure reaps the launched child and listener", async () => {
  const fixture = await releaseFixture();
  const child = fakeChild(); let released = 0;
  await expect(runOfflineStorePersistenceProof({
    release: fixture.release, scratchRoot: fixture.root, awaitReady: async () => {},
  }, {
    reserveListener: async () => ({ fd: 3, port: 49001, release: async () => { released += 1; } }),
    launch: async () => child.child,
    createClient: () => ({ status: async () => { throw new Error("fixture status failure"); }, close: () => {} }) as any,
    settleChild: async (current) => { current.kill("SIGTERM"); await current.exited; },
  })).rejects.toThrow("fixture status failure");
  expect(child.killed).toEqual(["SIGTERM"]);
  expect(released).toBe(1);
});
