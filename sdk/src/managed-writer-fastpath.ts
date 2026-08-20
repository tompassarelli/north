// Managed identity publication uses only canonical Store RPC. It reports a
// commit only after exact marker readback under the subject's fenced lease.
import { createHash } from "node:crypto";
import {
  StoreRpcClient, StoreRpcServerError, StoreRpcTransportError,
} from "./store-rpc-client";
import type { BatchAction, Term } from "./store-rpc-codec";
import {
  StoreTriple, RPC_SUBJECT_ANY, rpcFence, termEquals, triple,
} from "./store-rpc-codec";
import type { ManagedWriteResult } from "./identity";

// ---------------------------------------------------------------------------
// Predicate vocabulary — MUST stay byte-identical to cli/agent-provenance.clj
// identity-predicates and cli/agent-fact-internal.clj publish/required sets. A
// drift here would let the fast path commit an identity the reader rejects.
// ---------------------------------------------------------------------------
const IDENTITY_PREDICATES = [
  "kind", "role", "model", "provider", "provider_target", "live_input",
  "live_input_state", "live_input_epoch", "effort",
  "composition_kind", "composition_id", "composition_overrides",
  "composition_override_reason", "nearest_template", "bespoke_reason",
  "promotion_candidate", "composition_contract_sha256",
  "composition_contract_fingerprint_version", "composition_contract_fingerprint_domain",
  "repo", "goal", "worktree", "branch", "coordinator", "spawned_at",
] as const;
const PROJECTION_PREDICATES = ["display_handle", "display_name"] as const;
const PUBLISH_PREDICATES = new Set<string>([...IDENTITY_PREDICATES, ...PROJECTION_PREDICATES]);
// required-identity-predicates in agent-fact-internal.clj is North's required
// set MINUS identity_manifest_sha256 (the marker is written last, not supplied).
const REQUIRED_PUBLISH_PREDICATES = [
  "kind", "role", "goal", "provider", "provider_target", "live_input",
  "live_input_state", "live_input_epoch", "model", "effort",
  "composition_kind", "composition_id", "repo", "spawned_at", "display_handle",
  "display_name",
];
const MARKER_PREDICATE = "identity_manifest_sha256";
const TERMINAL_MARKER_PREDICATE = "terminal_manifest_sha256";
// Terminal bodies must be absent for a clean fresh publish; presence forces fallback.
const TERMINAL_PREDICATES = [
  "process_outcome", "delivery_outcome", "delivery_reason",
  "delivery_evidence", "delivery_evidence_sha256",
  "delivery_attestation", "delivery_attestation_sha256",
];
const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const SHA256_HEX = /^[0-9a-f]{64}$/;
// Mirror cli/terminal-projection.clj valid-agent-entity? — the canonical entity
// string whose bytes feed both the write-lease digest and every te on the wire.
const AGENT_ENTITY = /^@agent:[A-Za-z0-9][A-Za-z0-9._:-]*$/;

function sha256Hex(value: string): string {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

/**
 * Port of cli/agent-fact-internal.clj `entity`: strip a leading `@agent:`/`agent:`
 * and re-prefix `@agent:`, then require the canonical shape. The subprocess
 * normalizes its subject argument this way, and BOTH the write-lease resource
 * and every `te` derive from the normalized form — so the fast path must use the
 * identical string or it would take a different lease and write to a different
 * entity than the reader/subprocess. Returns null (→ fallback) on any id the
 * subprocess would reject.
 */
export function normalizeAgentEntity(subject: string): string | null {
  const raw = subject.replace(/^@?agent:/, "");
  const canonical = `@agent:${raw}`;
  return AGENT_ENTITY.test(canonical) ? canonical : null;
}

/** Marker: present identity predicates, sorted, joined `pred\u0000value\n`,
 * sha256 — MUST match cli/agent-fact-internal.clj `canonical` over
 * (select-keys facts identity-predicates); NUL is load-bearing. */
export function identityMarker(projection: Record<string, string>): string {
  const canonical = IDENTITY_PREDICATES
    .filter((p) => projection[p] !== undefined && projection[p] !== "")
    .slice()
    .sort()
    .map((p) => `${p}\u0000${projection[p]}\n`)
    .join("");
  return sha256Hex(canonical);
}

/** managed-agent-write:<sha256(entity)> — the coordinator write-lease resource,
 * computed over the NORMALIZED @agent: entity exactly like write-lease-resource. */
export function writeLeaseResource(entity: string): string {
  return `managed-agent-write:${sha256Hex(entity)}`;
}

/**
 * Port of cli/agent-fact-internal.clj validate-publish!. Returns true only for a
 * projection the subprocess would accept, so the fast path never commits an
 * identity the reader would reject. Any false → the caller uses the subprocess,
 * which reproduces the canonical rejection error.
 */
export function validPublishProjection(projection: Record<string, string>): boolean {
  for (const key of Object.keys(projection)) {
    if (!PUBLISH_PREDICATES.has(key)) return false;
  }
  for (const req of REQUIRED_PUBLISH_PREDICATES) {
    const v = projection[req];
    if (v === undefined || v === "") return false;
  }
  if (projection.kind !== "lane") return false;
  if (!["streaming", "turn-messages", "unsupported"].includes(projection.live_input)) return false;
  if (!["pending", "armed", "frozen"].includes(projection.live_input_state)) return false;
  if (!UUID_V4.test(projection.live_input_epoch)) return false;
  if (projection.live_input === "unsupported" && projection.live_input_state !== "frozen") return false;
  if (projection.role !== projection.composition_id) return false;
  const bespokeOnly = [
    "bespoke_reason", "promotion_candidate", "composition_contract_sha256",
    "composition_contract_fingerprint_version", "composition_contract_fingerprint_domain",
  ];
  if (projection.composition_kind === "template") {
    if (!("composition_overrides" in projection)) return false;
    if (bespokeOnly.some((p) => p in projection)) return false;
  } else if (projection.composition_kind === "bespoke") {
    if (bespokeOnly.some((p) => !(p in projection))) return false;
    if (!["true", "false"].includes(projection.promotion_candidate)) return false;
    if (!SHA256_HEX.test(projection.composition_contract_sha256)) return false;
    if (projection.composition_contract_fingerprint_version !== "v1") return false;
    if (projection.composition_contract_fingerprint_domain !== "north:bespoke-contract:v1") return false;
  } else {
    return false;
  }
  return true;
}

export interface NativeFastPublishOptions {
  /** Hermetic transport seam. Production constructs the client from env. */
  client?: StoreRpcClient;
}

type NativeProjectionClass = "blank" | "exact_replay" | "decline";

interface NativeSnapshot {
  classification: NativeProjectionClass;
  servedVersion: number;
}

function nativeIndeterminate(operationId: string, reason: string): ManagedWriteResult {
  return { status: "indeterminate", operationId, reason };
}

function desiredNativeOccurrences(
  projection: Record<string, string>, marker: string,
): Map<string, Map<string, number>> {
  const desired = new Map<string, Map<string, number>>();
  for (const [predicate, value] of Object.entries(projection))
    desired.set(predicate, new Map([[value, 1]]));
  desired.set(MARKER_PREDICATE, new Map([[marker, 1]]));
  return desired;
}

function classifyNativeProjection(
  rows: readonly Term[], projection: Record<string, string>, marker: string,
): NativeProjectionClass {
  const checked = new Set<string>([
    ...IDENTITY_PREDICATES, ...Object.keys(projection), MARKER_PREDICATE,
    TERMINAL_MARKER_PREDICATE, ...TERMINAL_PREDICATES,
  ]);
  const actual = new Map<string, Map<string, number>>();
  for (const row of rows) {
    if (!(row instanceof StoreTriple) || typeof row.t2 !== "string") continue;
    if (!checked.has(row.t2)) continue;
    if (typeof row.t3 !== "string") return "decline";
    const values = actual.get(row.t2) ?? new Map<string, number>();
    values.set(row.t3, (values.get(row.t3) ?? 0) + 1);
    actual.set(row.t2, values);
  }
  if (actual.size === 0) return "blank";
  const desired = desiredNativeOccurrences(projection, marker);
  if (actual.size !== desired.size) return "decline";
  for (const [predicate, wanted] of desired) {
    const found = actual.get(predicate);
    if (found === undefined || found.size !== wanted.size) return "decline";
    for (const [value, count] of wanted) {
      if (found.get(value) !== count) return "decline";
    }
  }
  return "exact_replay";
}

async function nativeSnapshot(
  client: StoreRpcClient, entity: string, projection: Record<string, string>, marker: string,
): Promise<NativeSnapshot> {
  const scanned = await client.scanAll(entity, null, null);
  return {
    classification: classifyNativeProjection(scanned.rows, projection, marker),
    servedVersion: scanned.servedVersion,
  };
}

function nativeFreshActions(
  entity: string, projection: Record<string, string>, marker: string,
): BatchAction[] {
  const body = Object.entries(projection).sort(([leftPredicate, leftValue], [rightPredicate, rightValue]) => {
    if (leftPredicate !== rightPredicate) return leftPredicate < rightPredicate ? -1 : 1;
    if (leftValue === rightValue) return 0;
    return leftValue < rightValue ? -1 : 1;
  });
  return [
    ...body.map(([predicate, value]) => ({
      op: "assert" as const,
      proposition: triple(entity, predicate, value),
      policy: RPC_SUBJECT_ANY,
    })),
    {
      op: "assert" as const,
      proposition: triple(entity, MARKER_PREDICATE, marker),
      policy: RPC_SUBJECT_ANY,
    },
  ];
}

function expectedActionResults(actions: readonly BatchAction[], results: readonly { inputIndex: number }[]): boolean {
  return results.length === actions.length
    && results.every((result, index) => result.inputIndex === index);
}

function durabilityAmbiguous(error: unknown): boolean {
  return error instanceof StoreRpcServerError && error.code === "durability-ambiguous";
}

async function checkedCandidateFence(
  client: StoreRpcClient, candidate: Term,
): Promise<boolean> {
  try {
    return (await client.leaseCheck(candidate)).valid;
  } catch {
    return false;
  }
}

type NativeLeaseAttempt =
  | { kind: "acquired"; fence: Term }
  | { kind: "replan" }
  | { kind: "decline" }
  | { kind: "indeterminate"; result: ManagedWriteResult };

async function nativeLeaseAtVersion(
  client: StoreRpcClient,
  resource: string,
  holder: string,
  expectedVersion: number,
  operationId: string,
): Promise<NativeLeaseAttempt> {
  const candidate = rpcFence(resource, holder, expectedVersion + 1);
  const acquire = async () => client.leaseAcquire(
    resource, holder, 60_000, { expectedVersion },
  );
  for (let identicalAttempt = 0; identicalAttempt < 2; identicalAttempt += 1) {
    try {
      const grant = await acquire();
      if (!termEquals(grant.fence, candidate)) return { kind: "decline" };
      return { kind: "acquired", fence: grant.fence };
    } catch (error) {
      if (durabilityAmbiguous(error)) {
        return {
          kind: "indeterminate",
          result: nativeIndeterminate(operationId, "durability_ambiguous_restart_required"),
        };
      }
      if (error instanceof StoreRpcServerError) {
        if (error.code === "rpc/conflict") return { kind: "replan" };
        if (error.code === "rpc/lease-held") return { kind: "decline" };
        return { kind: "decline" };
      }
      if (error instanceof StoreRpcTransportError && !error.requestSent)
        return { kind: "decline" };
      if (await checkedCandidateFence(client, candidate))
        return { kind: "acquired", fence: candidate };
      if (!(error instanceof StoreRpcTransportError) || !error.requestSent)
        return { kind: "decline" };
    }
  }
  return { kind: "decline" };
}

async function nativeReadbackResult(
  client: StoreRpcClient,
  entity: string,
  projection: Record<string, string>,
  marker: string,
  operationId: string,
  mismatchReason: string,
): Promise<ManagedWriteResult> {
  try {
    const readback = await nativeSnapshot(client, entity, projection, marker);
    if (readback.classification === "exact_replay")
      return { status: "committed", operationId };
  } catch {
    // A read failure after a sent/acknowledged mutation cannot prove absence.
  }
  return nativeIndeterminate(operationId, mismatchReason);
}

async function nativeFastPublish(
  entity: string,
  projection: Record<string, string>,
  holder: string,
  operationId: string,
  timeoutMs: number,
  options: NativeFastPublishOptions,
): Promise<ManagedWriteResult | null> {
  const deadline = Date.now() + Math.max(1, Math.floor(timeoutMs));
  const client = options.client ?? StoreRpcClient.create({
    connectTimeoutMs: Math.min(2_000, Math.max(1, Math.floor(timeoutMs))),
    readTimeoutMs: Math.max(1, Math.floor(timeoutMs)),
    maxAttempts: 1,
    retryDelayMs: 0,
    jitterMs: 0,
  });
  const ownsClient = options.client === undefined;
  const marker = identityMarker(projection);
  const resource = writeLeaseResource(entity);
  let lease: Term | null = null;
  let releaseAllowed = true;
  try {
    while (Date.now() < deadline && lease === null) {
      let version: number;
      try {
        version = (await client.version()).servedVersion;
      } catch {
        return null;
      }
      const attempt = await nativeLeaseAtVersion(
        client, resource, holder, version, operationId,
      );
      if (attempt.kind === "acquired") lease = attempt.fence;
      else if (attempt.kind === "replan") continue;
      else if (attempt.kind === "indeterminate") {
        releaseAllowed = false;
        return attempt.result;
      } else return null;
    }
    if (lease === null) return null;

    const actions = nativeFreshActions(entity, projection, marker);
    while (Date.now() < deadline) {
      let snapshot: NativeSnapshot;
      try {
        snapshot = await nativeSnapshot(client, entity, projection, marker);
      } catch {
        return null;
      }
      if (snapshot.classification === "exact_replay")
        return { status: "committed", operationId, reason: "exact_replay" };
      if (snapshot.classification !== "blank") return null;

      try {
        const applied = await client.batch(actions, {
          expectedVersion: snapshot.servedVersion,
          fence: lease,
        });
        if (!expectedActionResults(actions, applied.results)) {
          return nativeIndeterminate(operationId, "unexpected_native_batch_result");
        }
        return nativeReadbackResult(
          client, entity, projection, marker, operationId,
          "native_batch_readback_mismatch",
        );
      } catch (error) {
        if (durabilityAmbiguous(error)) {
          releaseAllowed = false;
          return nativeIndeterminate(operationId, "durability_ambiguous_restart_required");
        }
        if (error instanceof StoreRpcServerError && error.code === "rpc/conflict")
          continue;
        if (error instanceof StoreRpcServerError
            && error.code === "rpc/lease-fence-mismatch") {
          try {
            const final = await nativeSnapshot(client, entity, projection, marker);
            return final.classification === "exact_replay"
              ? { status: "committed", operationId, reason: "exact_replay" }
              : null;
          } catch {
            return null;
          }
        }
        if (error instanceof StoreRpcTransportError && error.requestSent) {
          try {
            const retried = await client.batch(actions, {
              expectedVersion: snapshot.servedVersion,
              fence: lease,
            });
            if (!expectedActionResults(actions, retried.results)) {
              return nativeIndeterminate(operationId, "unexpected_native_batch_result");
            }
          } catch (retryError) {
            if (durabilityAmbiguous(retryError)) {
              releaseAllowed = false;
              return nativeIndeterminate(
                operationId, "durability_ambiguous_restart_required",
              );
            }
          }
          return nativeReadbackResult(
            client, entity, projection, marker, operationId,
            "native_batch_ambiguous_readback",
          );
        }
        return null;
      }
    }
    return null;
  } finally {
    if (lease !== null && releaseAllowed) {
      try { await client.leaseRelease(lease); } catch { /* expiry recovers the lease */ }
    }
    if (ownsClient) client.close();
  }
}

/** Attempt one canonical Store RPC identity publication. */
export async function fastPublish(
  subject: string,
  projection: Record<string, string>,
  holder: string,
  operationId: string,
  timeoutMs: number,
  nativeOptions: NativeFastPublishOptions = {},
): Promise<ManagedWriteResult | null> {
  if (process.env.NORTH_MANAGED_WRITER_FASTPATH === "0") return null;
  if (process.env.NORTH_IDENTITY_TEST_REDIRECT === "1") return null;
  if (!validPublishProjection(projection)) return null;
  const entity = normalizeAgentEntity(subject);
  if (entity === null) return null;

  return nativeFastPublish(
    entity, projection, holder, operationId, timeoutMs, nativeOptions,
  );
}
