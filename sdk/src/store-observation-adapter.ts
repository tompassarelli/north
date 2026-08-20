import { createHash } from "node:crypto";
import {
  RPC_SUBJECT_ANY, StoreTriple, triple,
  type BatchAction, type Term,
} from "./store-rpc-codec";
import { StoreRpcClient } from "./store-rpc-client";

export const STORE_OBSERVATION_RECEIPT_VERSION = "north:provider-observation:v1" as const;

export interface StoreObservationReceipt {
  readonly version: typeof STORE_OBSERVATION_RECEIPT_VERSION;
  readonly subject: string;
  readonly digest: string;
  readonly servedVersion: number;
}

export interface StoreObservationSnapshot<T> {
  readonly observation: T;
  readonly receipt: StoreObservationReceipt;
}

export interface StoreObservationClient {
  scanAll(t1: Term | null, t2: Term | null, t3: Term | null): Promise<{
    readonly rows: Term[];
    readonly servedVersion: number;
  }>;
  batch(actions: readonly BatchAction[], options: { expectedVersion: number }): Promise<{
    readonly results: readonly unknown[];
    readonly servedVersion: number;
  }>;
  close(): void;
}

export interface StoreObservationCodec<T> {
  readonly kind: string;
  parse(value: unknown): T;
  observedAt(observation: T): string;
}

export interface AdmitStoreObservationOptions<T> {
  readonly subject: string;
  readonly observation: T;
  readonly codec: StoreObservationCodec<T>;
  readonly client?: StoreObservationClient;
}

export interface LoadStoreObservationOptions<T> {
  readonly subject: string;
  readonly codec: StoreObservationCodec<T>;
  readonly client?: StoreObservationClient;
}

const KIND = "kind";
const DIGEST = "digest";
const PAYLOAD = "payload";

function canonicalJson(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  const record = value as Record<string, unknown>;
  return `{${Object.keys(record).sort().map((key) =>
    `${JSON.stringify(key)}:${canonicalJson(record[key])}`).join(",")}}`;
}

function digest(payload: string): string {
  return createHash("sha256").update(payload, "utf8").digest("hex");
}

function receipt(subject: string, payload: string, servedVersion: number): StoreObservationReceipt {
  return Object.freeze({
    version: STORE_OBSERVATION_RECEIPT_VERSION,
    subject,
    digest: digest(payload),
    servedVersion,
  });
}

function ownedClient(client: StoreObservationClient | undefined): {
  client: StoreObservationClient;
  owns: boolean;
} {
  return client
    ? { client, owns: false }
    : { client: StoreRpcClient.create({ maxAttempts: 1, retryDelayMs: 0, jitterMs: 0 }), owns: true };
}

function requireTimestamp(value: string): number {
  const parsed = Date.parse(value);
  if (!Number.isFinite(parsed)) throw new Error("Store observation timestamp is invalid");
  return parsed;
}

function readRows<T>(
  subject: string, rows: readonly Term[], servedVersion: number, codec: StoreObservationCodec<T>,
): StoreObservationSnapshot<T> | undefined {
  if (rows.length === 0) return undefined;
  const facts = new Map<string, string>();
  for (const row of rows) {
    if (!(row instanceof StoreTriple) || row.t1 !== subject
        || typeof row.t2 !== "string" || typeof row.t3 !== "string" || facts.has(row.t2))
      throw new Error("Store observation snapshot is malformed");
    facts.set(row.t2, row.t3);
  }
  if (facts.size !== 3 || facts.get(KIND) !== codec.kind
      || !facts.has(DIGEST) || !facts.has(PAYLOAD))
    throw new Error("Store observation snapshot is incomplete");
  const payload = facts.get(PAYLOAD)!;
  const payloadDigest = facts.get(DIGEST)!;
  if (!/^[a-f0-9]{64}$/.test(payloadDigest) || digest(payload) !== payloadDigest)
    throw new Error("Store observation digest is invalid");
  let decoded: unknown;
  try { decoded = JSON.parse(payload); }
  catch { throw new Error("Store observation payload is not JSON"); }
  const observation = codec.parse(decoded);
  if (canonicalJson(observation) !== payload)
    throw new Error("Store observation payload is not canonical");
  requireTimestamp(codec.observedAt(observation));
  return Object.freeze({ observation, receipt: receipt(subject, payload, servedVersion) });
}

/** Read one complete, canonical Store subject. Missing, torn, or contradictory facts fail closed. */
export async function loadStoreObservation<T>(
  options: LoadStoreObservationOptions<T>,
): Promise<StoreObservationSnapshot<T> | undefined> {
  const connection = ownedClient(options.client);
  try {
    const snapshot = await connection.client.scanAll(options.subject, null, null);
    return readRows(options.subject, snapshot.rows, snapshot.servedVersion, options.codec);
  } finally {
    if (connection.owns) connection.client.close();
  }
}

/**
 * Admit one observation at the Store boundary. The Store snapshot is the result:
 * stale samples retain the newer Store value, while a version conflict publishes
 * nothing and leaves the caller's JSON projection untouched.
 */
export async function admitStoreObservation<T>(
  options: AdmitStoreObservationOptions<T>,
): Promise<StoreObservationSnapshot<T>> {
  const normalized = options.codec.parse(options.observation);
  const candidatePayload = canonicalJson(normalized);
  const candidateAt = requireTimestamp(options.codec.observedAt(normalized));
  const connection = ownedClient(options.client);
  try {
    const before = await connection.client.scanAll(options.subject, null, null);
    const current = readRows(options.subject, before.rows, before.servedVersion, options.codec);
    if (current && requireTimestamp(options.codec.observedAt(current.observation)) > candidateAt) return current;
    if (current && current.receipt.digest === digest(candidatePayload)) return current;
    const actions: BatchAction[] = [
      ...before.rows.map((row) => {
        if (!(row instanceof StoreTriple)) throw new Error("Store observation snapshot is malformed");
        return { op: "retract" as const, proposition: row, policy: RPC_SUBJECT_ANY };
      }),
      { op: "assert", proposition: triple(options.subject, KIND, options.codec.kind), policy: RPC_SUBJECT_ANY },
      { op: "assert", proposition: triple(options.subject, DIGEST, digest(candidatePayload)), policy: RPC_SUBJECT_ANY },
      { op: "assert", proposition: triple(options.subject, PAYLOAD, candidatePayload), policy: RPC_SUBJECT_ANY },
    ];
    const committed = await connection.client.batch(actions, { expectedVersion: before.servedVersion });
    if (committed.results.length !== actions.length)
      throw new Error("Store observation batch result is incomplete");
    return Object.freeze({
      observation: normalized,
      receipt: receipt(options.subject, candidatePayload, committed.servedVersion),
    });
  } finally {
    if (connection.owns) connection.client.close();
  }
}

export function storeObservationSubject(kind: string, identity: readonly string[]): string {
  if (!/^[a-z][a-z0-9_-]{0,63}$/.test(kind) || identity.some((value) => !value.length))
    throw new Error("Store observation identity is invalid");
  return `@provider-observation:${kind}:${createHash("sha256")
    .update(canonicalJson(identity), "utf8").digest("hex")}`;
}
