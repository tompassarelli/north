import { createHash, randomUUID } from "node:crypto";
import {
  chmodSync, closeSync, fsyncSync, lstatSync, mkdirSync, openSync,
  readFileSync, readdirSync, renameSync, unlinkSync, writeFileSync,
} from "node:fs";
import { appendFile, chmod, mkdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join, resolve } from "node:path";
import { withFileLease } from "./file-lease";
import { crush, DEFAULTS, hashOf } from "./vendor/eso/ccr.js";
import type { CcrCache, CcrOptions, CcrSentinel } from "./vendor/eso/ccr.js";

/**
 * North does not currently intercept live provider tool results before they are
 * accepted. CCR is therefore an explicit canary boundary, never global coverage.
 */
export const CCR_COVERAGE = "explicit_canary_only" as const;
export const DEFAULT_CCR_TTL_MS = 24 * 60 * 60 * 1_000;

export type CcrRetrievalStatus = "hit" | "miss" | "expired" | "corrupt";
export type CcrDecision = "disabled" | "ineligible" | "passthrough" | "compressed" | "fallback" | "retrieve";
export type CcrFallbackReason =
  | "below_minimum_items"
  | "invalid_json_array"
  | "not_smaller"
  | "provider_side_effects_started"
  | "store_verification_failed"
  | "compression_error"
  | "telemetry_unavailable"
  | "retrieval_miss"
  | "retrieval_expired"
  | "retrieval_corrupt";

export interface CcrTelemetryEvent {
  version: 1;
  observedAt: string;
  coverage: typeof CCR_COVERAGE;
  implementation: "north_eso_ccr";
  workspaceScope: string;
  managedRunScope: string;
  enabled: boolean;
  eligible: boolean;
  decision: CcrDecision;
  originalBytes: number | null;
  compressedBytes: number | null;
  originalTokens: number | null;
  compressedTokens: number | null;
  hash: string | null;
  markerOutcome: "not_attempted" | "not_emitted" | "emitted" | "lookup_requested";
  storeOutcome: "not_attempted" | "stored" | "failed";
  retrievalOutcome: "not_attempted" | CcrRetrievalStatus;
  fallbackReason: CcrFallbackReason | null;
}

export interface CcrScope {
  workspaceIdentity: string;
  managedRunId: string;
}

export interface CcrStateOptions {
  stateDir?: string;
  ttlMs?: number;
  now?: () => number;
}

interface StoredEnvelope {
  version: 1;
  hash: string;
  sha256: string;
  createdAtMs: number;
  expiresAtMs: number;
  original: unknown[];
}

export interface CcrRetrieval<T = unknown> {
  status: CcrRetrievalStatus;
  output?: T[];
}

function sha256(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

function requireIdentity(label: string, value: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 4_096)
    throw new TypeError(`CCR ${label} must be a non-empty string of at most 4096 characters`);
  return value;
}

function requireHash(hash: string): string {
  if (!/^[0-9a-f]{16}$/.test(hash)) throw new TypeError("CCR hash must be exactly 16 lowercase hex characters");
  return hash;
}

function stateRoot(override?: string): string {
  const configured = override ?? process.env.NORTH_CCR_STATE_DIR;
  if (configured) return resolve(configured);
  const xdg = process.env.XDG_STATE_HOME;
  return resolve(xdg || join(homedir(), ".local", "state"), "north", "ccr");
}

function canonicalBytes(output: readonly unknown[]): Buffer {
  // hashOf is the vendored CCR implementation's exact JSON-domain validator.
  hashOf(output);
  return Buffer.from(JSON.stringify(output), "utf8");
}

function safeByteLength(output: readonly unknown[]): number | null {
  try { return canonicalBytes(output).byteLength; }
  catch { return null; }
}

function scopeDigests(scope: CcrScope): { workspace: string; run: string } {
  const workspace = requireIdentity("workspace identity", scope.workspaceIdentity);
  const run = requireIdentity("managed run id", scope.managedRunId);
  return {
    workspace: sha256(`north-ccr-workspace\0${workspace}`),
    run: sha256(`north-ccr-run\0${run}`),
  };
}

function ensurePrivateDirectory(path: string): void {
  mkdirSync(path, { recursive: true, mode: 0o700 });
  chmodSync(path, 0o700);
}

function syncDirectory(path: string): void {
  const descriptor = openSync(path, "r");
  try { fsyncSync(descriptor); }
  finally { closeSync(descriptor); }
}

function atomicPrivateWrite(path: string, content: string): void {
  const directory = resolve(path, "..");
  ensurePrivateDirectory(directory);
  const temporary = join(directory, `.${randomUUID()}.tmp`);
  let descriptor: number | undefined;
  try {
    descriptor = openSync(temporary, "wx", 0o600);
    writeFileSync(descriptor, content, "utf8");
    fsyncSync(descriptor);
    closeSync(descriptor);
    descriptor = undefined;
    chmodSync(temporary, 0o600);
    renameSync(temporary, path);
    chmodSync(path, 0o600);
    syncDirectory(directory);
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
    try { unlinkSync(temporary); }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
  }
}

function isEnvelope(value: unknown): value is StoredEnvelope {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const candidate = value as Partial<StoredEnvelope>;
  return candidate.version === 1
    && typeof candidate.hash === "string"
    && typeof candidate.sha256 === "string"
    && Number.isSafeInteger(candidate.createdAtMs)
    && Number.isSafeInteger(candidate.expiresAtMs)
    && Array.isArray(candidate.original);
}

/** Persistent, workspace/run-scoped cache used directly by the vendored CCR verifier. */
export class PersistentCcrStore implements CcrCache {
  readonly root: string;
  readonly workspaceScope: string;
  readonly managedRunScope: string;
  readonly ttlMs: number;
  lastStoreOutcome: "not_attempted" | "stored" | "failed" = "not_attempted";
  lastRetrievalOutcome: "not_attempted" | CcrRetrievalStatus = "not_attempted";
  private readonly now: () => number;

  constructor(scope: CcrScope, options: CcrStateOptions = {}) {
    const digests = scopeDigests(scope);
    const ttlMs = options.ttlMs ?? DEFAULT_CCR_TTL_MS;
    if (!Number.isSafeInteger(ttlMs) || ttlMs <= 0) throw new TypeError("CCR ttlMs must be a positive safe integer");
    this.root = stateRoot(options.stateDir);
    this.workspaceScope = digests.workspace;
    this.managedRunScope = digests.run;
    this.ttlMs = ttlMs;
    this.now = options.now ?? Date.now;
  }

  private hashDirectory(hash: string): string {
    return join(this.root, "objects", this.workspaceScope, this.managedRunScope, requireHash(hash));
  }

  put(hash: string, original: unknown[]): void {
    this.lastStoreOutcome = "failed";
    const bytes = canonicalBytes(original);
    const full = sha256(bytes);
    if (full.slice(0, 16) !== requireHash(hash)) throw new Error("CCR store hash does not match original content");
    const now = this.now();
    if (!Number.isSafeInteger(now)) throw new Error("CCR clock must return integer milliseconds");
    const directory = this.hashDirectory(hash);
    ensurePrivateDirectory(directory);
    const envelope: StoredEnvelope = {
      version: 1,
      hash,
      sha256: full,
      createdAtMs: now,
      expiresAtMs: now + this.ttlMs,
      original: JSON.parse(bytes.toString("utf8")) as unknown[],
    };
    atomicPrivateWrite(join(directory, `${full}.json`), `${JSON.stringify(envelope)}\n`);
    this.lastStoreOutcome = "stored";
  }

  retrieve<T = unknown>(hash: string): CcrRetrieval<T> {
    requireHash(hash);
    const directory = this.hashDirectory(hash);
    let files: string[];
    try {
      files = readdirSync(directory).filter((name) => /^[0-9a-f]{64}\.json$/.test(name));
    } catch (error) {
      const status: CcrRetrievalStatus = (error as NodeJS.ErrnoException).code === "ENOENT" ? "miss" : "corrupt";
      this.lastRetrievalOutcome = status;
      return { status };
    }
    if (files.length === 0) {
      this.lastRetrievalOutcome = "miss";
      return { status: "miss" };
    }
    if (files.length !== 1) {
      this.lastRetrievalOutcome = "corrupt";
      return { status: "corrupt" };
    }
    try {
      const path = join(directory, files[0]);
      const info = lstatSync(path);
      if (!info.isFile() || (info.mode & 0o077) !== 0) throw new Error("CCR object is not a private regular file");
      const envelope: unknown = JSON.parse(readFileSync(path, "utf8"));
      if (!isEnvelope(envelope)) throw new Error("invalid CCR envelope");
      const bytes = canonicalBytes(envelope.original);
      const full = sha256(bytes);
      if (envelope.hash !== hash || envelope.sha256 !== full || files[0] !== `${full}.json`
        || full.slice(0, 16) !== hash || hashOf(envelope.original) !== hash)
        throw new Error("CCR content digest mismatch");
      if (this.now() >= envelope.expiresAtMs) {
        this.lastRetrievalOutcome = "expired";
        return { status: "expired" };
      }
      this.lastRetrievalOutcome = "hit";
      return { status: "hit", output: envelope.original as T[] };
    } catch {
      this.lastRetrievalOutcome = "corrupt";
      return { status: "corrupt" };
    }
  }

  get(hash: string): unknown[] | undefined {
    const result = this.retrieve(hash);
    return result.status === "hit" ? result.output : undefined;
  }
}

export interface CcrPrepareOptions<T, Prefix> extends CcrScope, CcrStateOptions {
  output: T[];
  stablePrefix: Prefix;
  enabled?: boolean;
  providerSideEffectsStarted?: boolean;
  tokenCount?: CcrOptions["tokenCount"];
  recordTelemetry?: (event: CcrTelemetryEvent) => Promise<void>;
}

export interface CcrPrepareResult<T, Prefix> {
  coverage: typeof CCR_COVERAGE;
  stablePrefix: Prefix;
  output: Array<T | CcrSentinel>;
  transformed: boolean;
  hash: string | null;
  dropped: number;
  telemetry: CcrTelemetryEvent;
  telemetryRecorded: boolean;
}

function telemetryBase(scope: CcrScope, now: number): Pick<
  CcrTelemetryEvent,
  "version" | "observedAt" | "coverage" | "implementation" | "workspaceScope" | "managedRunScope"
> {
  const digests = scopeDigests(scope);
  return {
    version: 1,
    observedAt: new Date(now).toISOString(),
    coverage: CCR_COVERAGE,
    implementation: "north_eso_ccr",
    workspaceScope: digests.workspace.slice(0, 16),
    managedRunScope: digests.run.slice(0, 16),
  };
}

export async function recordCcrTelemetry(event: CcrTelemetryEvent, overrideStateDir?: string): Promise<void> {
  const root = stateRoot(overrideStateDir);
  await mkdir(root, { recursive: true, mode: 0o700 });
  await chmod(root, 0o700);
  const path = join(root, "telemetry.jsonl");
  await withFileLease(`${path}.lock`, async () => {
    await appendFile(path, `${JSON.stringify(event)}\n`, { encoding: "utf8", mode: 0o600 });
    await chmod(path, 0o600);
  });
}

async function persistTelemetry(
  event: CcrTelemetryEvent,
  options: { stateDir?: string; recordTelemetry?: (event: CcrTelemetryEvent) => Promise<void> },
): Promise<boolean> {
  try {
    await (options.recordTelemetry ?? ((value) => recordCcrTelemetry(value, options.stateDir)))(event);
    return true;
  } catch { return false; }
}

function passthroughResult<T, Prefix>(
  options: CcrPrepareOptions<T, Prefix>,
  telemetry: CcrTelemetryEvent,
  telemetryRecorded: boolean,
): CcrPrepareResult<T, Prefix> {
  return {
    coverage: CCR_COVERAGE,
    stablePrefix: options.stablePrefix,
    output: options.output,
    transformed: false,
    hash: null,
    dropped: 0,
    telemetry,
    telemetryRecorded,
  };
}

/**
 * Default-off canary around an already-materialized JSON array. The caller's
 * stable prefix is returned by identity and is never serialized or rewritten.
 */
export async function prepareCcrOutput<T, Prefix>(
  options: CcrPrepareOptions<T, Prefix>,
): Promise<CcrPrepareResult<T, Prefix>> {
  const now = (options.now ?? Date.now)();
  const base = telemetryBase(options, now);
  const enabled = options.enabled === true;
  const originalBytes = safeByteLength(options.output);
  const eligible = originalBytes !== null && options.output.length >= DEFAULTS.minItems
    && options.providerSideEffectsStarted !== true;
  const untouched: Omit<CcrTelemetryEvent, keyof typeof base> = {
    enabled,
    eligible,
    decision: "disabled",
    originalBytes,
    compressedBytes: originalBytes,
    originalTokens: null,
    compressedTokens: null,
    hash: null,
    markerOutcome: "not_attempted",
    storeOutcome: "not_attempted",
    retrievalOutcome: "not_attempted",
    fallbackReason: null,
  };

  if (!enabled) {
    const event: CcrTelemetryEvent = { ...base, ...untouched };
    return passthroughResult(options, event, await persistTelemetry(event, options));
  }
  if (options.providerSideEffectsStarted) {
    const event: CcrTelemetryEvent = {
      ...base, ...untouched, decision: "fallback", fallbackReason: "provider_side_effects_started",
    };
    return passthroughResult(options, event, await persistTelemetry(event, options));
  }
  if (originalBytes === null) {
    const event: CcrTelemetryEvent = {
      ...base, ...untouched, decision: "fallback", fallbackReason: "invalid_json_array",
    };
    return passthroughResult(options, event, await persistTelemetry(event, options));
  }
  if (options.output.length < DEFAULTS.minItems) {
    const event: CcrTelemetryEvent = {
      ...base, ...untouched, decision: "ineligible", fallbackReason: "below_minimum_items",
    };
    return passthroughResult(options, event, await persistTelemetry(event, options));
  }

  const store = new PersistentCcrStore(options, options);
  const tokenObservations = new Map<readonly unknown[], number>();
  const count = options.tokenCount
    ? (value: readonly unknown[]) => {
        const observed = options.tokenCount!(value);
        tokenObservations.set(value, observed);
        return observed;
      }
    : (value: readonly unknown[]) => canonicalBytes(value).byteLength;
  try {
    const result = crush(options.output, { tokenCount: count, cache: store });
    if (result.hash === null) {
      const verificationFailed = store.lastStoreOutcome !== "not_attempted"
        && store.lastRetrievalOutcome !== "hit";
      const event: CcrTelemetryEvent = {
        ...base,
        enabled,
        eligible: true,
        decision: verificationFailed ? "fallback" : "passthrough",
        originalBytes,
        compressedBytes: originalBytes,
        originalTokens: tokenObservations.get(options.output) ?? null,
        compressedTokens: tokenObservations.get(result.view) ?? null,
        hash: null,
        markerOutcome: "not_emitted",
        storeOutcome: store.lastStoreOutcome,
        retrievalOutcome: store.lastRetrievalOutcome,
        fallbackReason: verificationFailed ? "store_verification_failed" : "not_smaller",
      };
      return passthroughResult(options, event, await persistTelemetry(event, options));
    }
    const event: CcrTelemetryEvent = {
      ...base,
      enabled,
      eligible: true,
      decision: "compressed",
      originalBytes,
      compressedBytes: canonicalBytes(result.view).byteLength,
      originalTokens: tokenObservations.get(options.output) ?? null,
      compressedTokens: tokenObservations.get(result.view) ?? null,
      hash: result.hash,
      markerOutcome: "emitted",
      storeOutcome: store.lastStoreOutcome,
      retrievalOutcome: store.lastRetrievalOutcome,
      fallbackReason: null,
    };
    const recorded = await persistTelemetry(event, options);
    if (!recorded) {
      return passthroughResult(options, {
        ...event,
        decision: "fallback",
        compressedBytes: originalBytes,
        hash: null,
        markerOutcome: "not_emitted",
        fallbackReason: "telemetry_unavailable",
      }, false);
    }
    return {
      coverage: CCR_COVERAGE,
      stablePrefix: options.stablePrefix,
      output: result.view,
      transformed: true,
      hash: result.hash,
      dropped: result.dropped,
      telemetry: event,
      telemetryRecorded: true,
    };
  } catch {
    const event: CcrTelemetryEvent = {
      ...base,
      enabled,
      eligible: true,
      decision: "fallback",
      originalBytes,
      compressedBytes: originalBytes,
      originalTokens: tokenObservations.get(options.output) ?? null,
      compressedTokens: null,
      hash: null,
      markerOutcome: "not_emitted",
      storeOutcome: store.lastStoreOutcome,
      retrievalOutcome: store.lastRetrievalOutcome,
      fallbackReason: "compression_error",
    };
    return passthroughResult(options, event, await persistTelemetry(event, options));
  }
}

export interface CcrRetrieveOptions extends CcrScope, CcrStateOptions {
  hash: string;
  recordTelemetry?: (event: CcrTelemetryEvent) => Promise<void>;
}

export interface CcrRetrieveResult<T = unknown> extends CcrRetrieval<T> {
  coverage: typeof CCR_COVERAGE;
  telemetry: CcrTelemetryEvent;
  telemetryRecorded: boolean;
}

export async function retrieveCcrOutput<T = unknown>(options: CcrRetrieveOptions): Promise<CcrRetrieveResult<T>> {
  const now = (options.now ?? Date.now)();
  const store = new PersistentCcrStore(options, options);
  const result = store.retrieve<T>(options.hash);
  const event: CcrTelemetryEvent = {
    ...telemetryBase(options, now),
    enabled: true,
    eligible: true,
    decision: "retrieve",
    originalBytes: result.output ? canonicalBytes(result.output).byteLength : null,
    compressedBytes: null,
    originalTokens: null,
    compressedTokens: null,
    hash: options.hash,
    markerOutcome: "lookup_requested",
    storeOutcome: "not_attempted",
    retrievalOutcome: result.status,
    fallbackReason: result.status === "miss" ? "retrieval_miss"
      : result.status === "expired" ? "retrieval_expired"
      : result.status === "corrupt" ? "retrieval_corrupt" : null,
  };
  return {
    ...result,
    coverage: CCR_COVERAGE,
    telemetry: event,
    telemetryRecorded: await persistTelemetry(event, options),
  };
}
