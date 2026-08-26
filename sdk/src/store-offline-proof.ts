/**
 * Scratch-only two-launch proof for a sealed Store release.  It deliberately
 * receives every effectful edge so its construction and cleanup can be proved
 * without starting a Store binary.
 */
import { createHash } from "node:crypto";
import { lstat, readFile, realpath } from "node:fs/promises";
import { createServer } from "node:net";
import { once } from "node:events";
import { spawn } from "node:child_process";
import { join, resolve } from "node:path";
import { settleBeagleStoreCoordinatorChild, type BeagleStoreCoordinatorChild } from "./beagle-store";
import { StoreRpcClient } from "./store-rpc-client";
import {
  loadStoreProviderUsageObservation, providerUsageObservationSubject,
  writeProviderUsageObservations,
} from "./provider-observation-store";
import type { ProviderUsageObservation } from "./providers/types";
import type { StoreObservationSnapshot } from "./store-observation-adapter";

const RELEASE_KEYS = [
  "format", "source", "revision", "tree", "native_artifact_dir",
  "native_closure_sha256", "server_artifact_sha256", "created",
] as const;
const SHA256 = /^[a-f0-9]{64}$/;

export interface SealedStoreRelease {
  readonly releasePath: string;
  readonly releaseSha256: string;
  readonly source: string;
  readonly revision: string;
  readonly tree: string;
  readonly artifactDir: string;
  readonly closureSha256: string;
  readonly serverPath: string;
  readonly serverSha256: string;
}

export interface OfflineProofListener {
  readonly fd: number;
  readonly port: number;
  release(): Promise<void>;
}

export interface OfflineProofChild extends BeagleStoreCoordinatorChild {}

export interface OfflineProofLaunch {
  readonly run: "initial" | "restart";
  readonly release: SealedStoreRelease;
  readonly listener: OfflineProofListener;
  readonly storeLog: string;
  readonly spaceId: string;
  readonly env: Readonly<Record<string, string>>;
}

export interface OfflineStorePersistenceProofOptions {
  readonly release: SealedStoreRelease;
  /** Existing private directory; no live North path is accepted implicitly. */
  readonly scratchRoot: string;
  /** A readiness receipt is required; retries are forbidden for the RPC checks. */
  readonly awaitReady: (launch: OfflineProofLaunch, child: OfflineProofChild) => Promise<void>;
  readonly observation?: ProviderUsageObservation;
}

export interface OfflineStorePersistenceProofRuntime {
  reserveListener?: () => Promise<OfflineProofListener>;
  launch?: (launch: OfflineProofLaunch) => Promise<OfflineProofChild>;
  createClient?: (options: Parameters<typeof StoreRpcClient.create>[0]) => StoreRpcClient;
  persistObservation?: (
    observation: ProviderUsageObservation, path: string, client: StoreRpcClient,
  ) => Promise<unknown>;
  loadObservation?: (
    observation: ProviderUsageObservation, client: StoreRpcClient,
  ) => Promise<StoreObservationSnapshot<ProviderUsageObservation> | undefined>;
  settleChild?: (child: OfflineProofChild) => Promise<unknown>;
  now?: () => Date;
}

export interface OfflineStorePersistenceProofReceipt {
  readonly releaseSha256: string;
  readonly closureSha256: string;
  readonly serverSha256: string;
  readonly initialPort: number;
  readonly restartPort: number;
  readonly providerReceiptSha256: string;
}

function sha256(value: Uint8Array | string): string {
  return createHash("sha256").update(value).digest("hex");
}

function releaseField(lines: readonly string[], key: string): string {
  const value = lines.find((line) => line.startsWith(`${key}=`))?.slice(key.length + 1);
  if (!value) throw new Error(`sealed Store release is missing ${key}`);
  return value;
}

/** Read the canonical release receipt and bind it to its READY native server. */
export async function readSealedStoreRelease(releasePath: string): Promise<SealedStoreRelease> {
  const releaseDirectory = await realpath(releasePath);
  const metadata = await lstat(releaseDirectory);
  if (!metadata.isDirectory() || metadata.isSymbolicLink())
    throw new Error("sealed Store release must be a real directory");
  const receiptPath = join(releaseDirectory, "RELEASE");
  const receipt = await readFile(receiptPath, "utf8");
  const lines = receipt.trimEnd().split("\n");
  if (lines.length !== RELEASE_KEYS.length
      || lines.map((line) => line.split("=", 1)[0]).join("\0") !== RELEASE_KEYS.join("\0"))
    throw new Error("sealed Store release has an invalid receipt shape");
  if (releaseField(lines, "format") !== "north-store-release/v1")
    throw new Error("sealed Store release has an unsupported format");
  const closureSha256 = releaseField(lines, "native_closure_sha256");
  const serverSha256 = releaseField(lines, "server_artifact_sha256");
  if (!SHA256.test(closureSha256) || !SHA256.test(serverSha256))
    throw new Error("sealed Store release has an invalid artifact digest");
  const artifactDir = await realpath(releaseField(lines, "native_artifact_dir"));
  const ready = await readFile(join(artifactDir, "READY"), "utf8");
  if (ready.trimEnd() !== `beagle-store-native-build/v1 ${closureSha256}`)
    throw new Error("sealed Store release READY identity does not bind its closure");
  const serverPath = join(artifactDir, "bin", "beagle-store-server-native");
  const server = await readFile(serverPath);
  if (sha256(server) !== serverSha256)
    throw new Error("sealed Store release server digest does not bind its receipt");
  return Object.freeze({
    releasePath: releaseDirectory, releaseSha256: sha256(receipt),
    source: releaseField(lines, "source"), revision: releaseField(lines, "revision"),
    tree: releaseField(lines, "tree"), artifactDir, closureSha256, serverPath, serverSha256,
  });
}

async function reserveLoopbackListener(): Promise<OfflineProofListener> {
  const server = createServer();
  server.listen({ host: "127.0.0.1", port: 0, exclusive: true });
  await once(server, "listening");
  const address = server.address();
  const fd = (server as unknown as { _handle?: { fd?: unknown } })._handle?.fd;
  if (!address || typeof address === "string" || typeof fd !== "number"
      || !Number.isSafeInteger(fd) || fd < 3)
    throw new Error("could not reserve an inherited Store listener");
  if (address.port === 7977 || address.port === 7978) {
    server.close();
    return reserveLoopbackListener();
  }
  return {
    fd, port: address.port,
    release: async () => { server.close(); await once(server, "close"); },
  };
}

async function launchNativeServer(launch: OfflineProofLaunch): Promise<OfflineProofChild> {
  const child = spawn(launch.release.serverPath, ["serve", String(launch.listener.port), launch.storeLog, launch.spaceId], {
    cwd: launch.release.releasePath,
    env: { ...process.env, ...launch.env },
    stdio: ["ignore", "pipe", "pipe", launch.listener.fd],
  });
  if (!child.pid) throw new Error("sealed Store server did not start");
  return {
    exited: once(child, "exit").then(([code]) => code ?? 1),
    kill: (signal) => child.kill(signal),
  };
}

function proofObservation(now: Date): ProviderUsageObservation {
  return {
    targetId: "store-release-proof", provider: "openai",
    source: "codex-app-server:account-rate-limits",
    observedAt: new Date(now.getTime() - 1_000).toISOString(), state: "normal",
  };
}

function checkInitial(
  status: Awaited<ReturnType<StoreRpcClient["status"]>>,
  scan: Awaited<ReturnType<StoreRpcClient["scan"]>>,
): void {
  if (status.attempts !== 1 || scan.attempts !== 1)
    throw new Error("offline Store proof retried an RPC");
  if (status.state.name !== "ready" || status.engine.name !== "native")
    throw new Error("offline Store proof did not reach a ready native Store");
  if (status.servedVersion !== scan.servedVersion || scan.page !== null || scan.rows.length !== 0)
    throw new Error("offline Store proof initial snapshot is not an empty unpaged read");
}

/**
 * Run the inherited-FD two-launch journey.  The runtime seam exists solely for
 * deterministic construction/cleanup tests; normal callers use its defaults.
 */
export async function runOfflineStorePersistenceProof(
  options: OfflineStorePersistenceProofOptions,
  runtime: OfflineStorePersistenceProofRuntime = {},
): Promise<OfflineStorePersistenceProofReceipt> {
  const scratchRoot = resolve(options.scratchRoot);
  const storeLog = join(scratchRoot, "coordination.storelog");
  const projectionPath = join(scratchRoot, "provider-usage-observations.json");
  const spaceId = `north-store-release-proof-${options.release.closureSha256.slice(0, 12)}`;
  const observation = options.observation ?? proofObservation((runtime.now ?? (() => new Date()))());
  const reserveListener = runtime.reserveListener ?? reserveLoopbackListener;
  const launchServer = runtime.launch ?? launchNativeServer;
  const createClient = runtime.createClient ?? StoreRpcClient.create;
  const persistObservation = runtime.persistObservation ?? ((value, path, client) =>
    writeProviderUsageObservations(value, path, { client }));
  const loadObservation = runtime.loadObservation ?? ((value, client) =>
    loadStoreProviderUsageObservation(value, client));
  const settleChild = runtime.settleChild ?? (async (child) => {
    child.kill("SIGTERM");
    await settleBeagleStoreCoordinatorChild(child);
    // A bounded escalation is not itself a receipt: do not release the FD or
    // restart the same log until this exact child has actually exited.
    await child.exited;
  });
  let listener: OfflineProofListener | undefined;
  let child: OfflineProofChild | undefined;
  let client: StoreRpcClient | undefined;
  const stop = async () => {
    const currentClient = client; client = undefined;
    currentClient?.close();
    const currentChild = child; child = undefined;
    if (currentChild) await settleChild(currentChild);
    const currentListener = listener; listener = undefined;
    if (currentListener) await currentListener.release();
  };
  const launch = async (run: "initial" | "restart") => {
    listener = await reserveListener();
    if (listener.port === 7977 || listener.port === 7978)
      throw new Error("offline Store proof reserved a protected port");
    const launch: OfflineProofLaunch = {
      run, release: options.release, listener, storeLog, spaceId,
      env: {
        BEAGLE_STORE_LISTEN_FD: "3", BEAGLE_STORE_BIND: "127.0.0.1",
        BEAGLE_STORE_SERVER_PORT: String(listener.port), BEAGLE_STORE_LOG: storeLog,
        BEAGLE_STORE_SPACE_ID: spaceId, BEAGLE_STORE_MAX_ACTIVE_CLIENTS: "4",
        BEAGLE_STORE_CLIENT_IO_TIMEOUT_MS: "15000",
      },
    };
    child = await launchServer(launch);
    await options.awaitReady(launch, child);
    return launch;
  };
  try {
    const initial = await launch("initial");
    client = createClient({ host: "127.0.0.1", port: initial.listener.port, spaceId,
      maxAttempts: 1, retryDelayMs: 0, jitterMs: 0 });
    const status = await client.status();
    const scan = await client.scan(null, null, null);
    checkInitial(status, scan);
    await persistObservation(observation, projectionPath, client);
    await stop();

    const restart = await launch("restart");
    client = createClient({ host: "127.0.0.1", port: restart.listener.port, spaceId,
      maxAttempts: 1, retryDelayMs: 0, jitterMs: 0 });
    const persisted = await loadObservation(observation, client);
    if (!persisted || JSON.stringify(persisted.observation) !== JSON.stringify(observation))
      throw new Error("offline Store proof lost the provider observation after restart");
    const expectedSubject = providerUsageObservationSubject(observation);
    if (persisted.receipt.subject !== expectedSubject || persisted.receipt.version !== "north:provider-observation:v1"
        || !SHA256.test(persisted.receipt.digest))
      throw new Error("offline Store proof persistence receipt is invalid");
    await stop();
    return Object.freeze({
      releaseSha256: options.release.releaseSha256, closureSha256: options.release.closureSha256,
      serverSha256: options.release.serverSha256, initialPort: initial.listener.port,
      restartPort: restart.listener.port, providerReceiptSha256: persisted.receipt.digest,
    });
  } finally {
    await stop();
  }
}
