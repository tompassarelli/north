import { execFileSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  closeSync,
  constants,
  fsyncSync,
  lstatSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import {
  framBabashkaArguments,
  framCoordinatorChildTimeout,
  framEngineEnvironment,
} from "./fram-engine";

/**
 * Dual-read seam for the Orchestration -> North Orchestration migration
 * (thread 019f8f5c). `NORTH_STAFFING_SOURCE` selects where the staffing catalog
 * and provider catalogs are read from:
 *
 *   graph (DEFAULT, Phase 2) — the imported @catalog:current subgraph,
 *                     reconstructed to the identical JSON shape by
 *                     orchestration-project-cli.clj; the graph is authoritative.
 *   file            — the Orchestration JSON files, byte-for-byte as today; the
 *                     retained rollback flag (retirement is Phase 4).
 *
 * The equality gate (cli/tests/orchestration-parity-test.clj) proves the two
 * sources are byte-equal after normalization, so switching the flag never
 * changes a routing decision. Phase 2 flips the default to GRAPH; only an
 * explicit NORTH_STAFFING_SOURCE=file falls back to the packaged files.
 */
export type StaffingSource = "file" | "graph";

export function staffingSource(): StaffingSource {
  return process.env.NORTH_STAFFING_SOURCE === "file" ? "file" : "graph";
}

const REPO = resolve(import.meta.dir, "..", "..");
const projectorCli = resolve(REPO, "cli/orchestration-project-cli.clj");
const MAX_CATALOG_CACHE_BYTES = 4 * 1024 * 1024;

function bb(): string {
  return process.env.NORTH_PEER_BB ?? "bb";
}

function port(): string {
  return process.env.NORTH_PORT ?? "7977";
}

function project(args: string[]): unknown {
  let out: string;
  try {
    out = execFileSync(bb(), framBabashkaArguments([projectorCli, port(), ...args]), {
      encoding: "utf8",
      env: framEngineEnvironment(),
      stdio: ["ignore", "pipe", "pipe"],
      timeout: framCoordinatorChildTimeout(30_000),
    });
  } catch (error) {
    // The projector fails LOUD on a timed-out query or a malformed graph row,
    // naming the exact model/field on its stderr (strict envelopes). Surface
    // that stderr in the thrown message so the packaged-JSON fallback upstream
    // can log precisely what the graph could not answer, never a bare
    // "Command failed".
    const err = error as { message?: string; stderr?: Buffer | string };
    const stderr = err.stderr ? String(err.stderr).trim() : "";
    const detail = [err.message, stderr].filter(Boolean).join(" :: ") || String(error);
    throw new Error(
      `NORTH_STAFFING_SOURCE=graph projection failed (${args.join(" ")}); `
      + `is @catalog:current imported on port ${port()}? ${detail}`,
    );
  }
  return JSON.parse(out);
}

/**
 * ONE catalog bundle — staffing + every provider — pinned to a single
 * @catalog:current version read by a single projector subprocess. The SDK's
 * admission path resolves tiers/reasoning/context/delta many times per spawn;
 * projecting each read independently shelled a fresh `bb` every call (an N+1
 * that repaid the coordinator's cold per-version scan warmup each time). This
 * bundle is projected once and cached for the process lifetime.
 */
export interface CatalogBundle {
  catalogVersion: number;
  staffing: unknown;
  providers: Record<string, unknown>;
}

interface CatalogProjectionCache {
  version: 1;
  catalogDigestSha256: string;
  coordinatorVersion: number;
  catalogVersion: number;
  bundle: CatalogBundle;
}

let cachedBundle: { port: string; bundle: CatalogBundle } | undefined;
let cachedCatalogGraphPin: { port: string; pin: CatalogGraphPin } | undefined;

function validateBundle(raw: unknown): CatalogBundle {
  if (!raw || typeof raw !== "object")
    throw new Error("NORTH_STAFFING_SOURCE=graph bundle projection returned an unexpected shape");
  const candidate = raw as Record<string, unknown>;
  if (typeof candidate.catalogVersion !== "number"
      || !candidate.staffing || typeof candidate.staffing !== "object"
      || !candidate.providers || typeof candidate.providers !== "object")
    throw new Error("NORTH_STAFFING_SOURCE=graph bundle projection returned an unexpected shape");
  return raw as CatalogBundle;
}

function projectBundleUncached(): CatalogBundle {
  const raw = project(["bundle"]) as Record<string, unknown>;
  return validateBundle(raw);
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonical);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)
        .map(([key, nested]) => [key, canonical(nested)]),
    );
  return value;
}

function bundleDigest(bundle: CatalogBundle): string {
  return createHash("sha256").update(JSON.stringify(canonical({
    staffing: bundle.staffing,
    providers: bundle.providers,
  }))).digest("hex");
}

/** Durable state shared by the fresh Bun/bb processes used for admissions. */
export function catalogProjectionCachePath(env: NodeJS.ProcessEnv = process.env): string {
  const explicit = env.NORTH_ORCHESTRATION_CATALOG_CACHE?.trim();
  if (explicit) return resolve(explicit);
  const home = env.HOME?.trim() || homedir();
  const state = env.XDG_STATE_HOME?.trim() || resolve(home, ".local/state");
  return resolve(state, "north/orchestration-catalog-projection-cache.json");
}

function readCachedBundle(path: string, pin: CatalogGraphPin): CatalogBundle | undefined {
  try {
    const state = lstatSync(path);
    if (!state.isFile() || state.isSymbolicLink()
        || state.size <= 0 || state.size > MAX_CATALOG_CACHE_BYTES)
      return undefined;
    const parsed = JSON.parse(readFileSync(path, "utf8")) as Partial<CatalogProjectionCache>;
    if (parsed.version !== 1
        || parsed.catalogDigestSha256 !== pin.catalogDigestSha256
        || !Number.isInteger(parsed.coordinatorVersion)
        || !Number.isInteger(parsed.catalogVersion))
      return undefined;
    const bundle = validateBundle(parsed.bundle);
    if (bundleDigest(bundle) !== pin.catalogDigestSha256) return undefined;
    // The digest may legitimately recur at a later catalog version. Preserve
    // the current projector shape: callers still observe the live pointer.
    return { ...bundle, catalogVersion: pin.catalogVersion };
  } catch {
    return undefined;
  }
}

/**
 * Best-effort atomic replace. A missing/unwritable cache is never an admission
 * refusal: the freshly projected bundle remains authoritative for this call.
 */
function writeCachedBundle(path: string, pin: CatalogGraphPin, bundle: CatalogBundle): void {
  const record: CatalogProjectionCache = {
    version: 1,
    catalogDigestSha256: pin.catalogDigestSha256,
    coordinatorVersion: pin.coordinatorVersion,
    catalogVersion: pin.catalogVersion,
    bundle,
  };
  const bytes = Buffer.from(`${JSON.stringify(record)}\n`);
  if (bytes.byteLength <= 0 || bytes.byteLength > MAX_CATALOG_CACHE_BYTES) return;
  const temporary = `${path}.tmp-${process.pid}-${randomUUID()}`;
  let fd: number | undefined;
  try {
    mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
    fd = openSync(
      temporary,
      constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW,
      0o600,
    );
    let offset = 0;
    while (offset < bytes.byteLength) offset += writeSync(fd, bytes, offset);
    fsyncSync(fd);
    closeSync(fd);
    fd = undefined;
    renameSync(temporary, path);
    const directory = openSync(dirname(path), constants.O_RDONLY | constants.O_DIRECTORY);
    try { fsyncSync(directory); } finally { closeSync(directory); }
  } catch {
    if (fd !== undefined) try { closeSync(fd); } catch {}
    try { unlinkSync(temporary); } catch {}
  }
}

function projectBundle(): CatalogBundle {
  // Admission always probes the cheap graph pin first. Its canonical digest is
  // both the cache key and the integrity check over the cached projection.
  const pin = catalogGraphPinForAdmission();
  const path = catalogProjectionCachePath();
  const hit = readCachedBundle(path, pin);
  if (hit) return hit;

  const bundle = projectBundleUncached();
  const projectedDigest = bundleDigest(bundle);
  if (projectedDigest !== pin.catalogDigestSha256)
    throw new Error(
      "NORTH_STAFFING_SOURCE=graph catalog changed between pin and bundle projection; retry admission",
    );
  writeCachedBundle(path, pin, bundle);
  return bundle;
}

/**
 * The process-cached catalog bundle, pinned to one version per admission
 * process. Only a SUCCESSFUL projection is cached; a failure re-projects (so a
 * transient timeout does not poison the whole process), letting the caller fall
 * back to the packaged JSON per read.
 */
export function catalogBundle(): CatalogBundle {
  const p = port();
  if (cachedBundle && cachedBundle.port === p) return cachedBundle.bundle;
  const bundle = projectBundle();
  cachedBundle = { port: p, bundle };
  return bundle;
}

/** @internal Test seam — drop the process bundle cache between fixtures. */
export function resetCatalogBundleCache(): void {
  cachedBundle = undefined;
  cachedCatalogGraphPin = undefined;
}

/**
 * Log a graph->packaged fallback. The projector already named the failing model
 * or query on its stderr (carried in `error.message`); echo it so a degraded
 * admission is visible in the logs, never silent.
 */
export function warnGraphCatalogFallback(what: string, error: unknown): void {
  const detail = error instanceof Error ? error.message : String(error);
  console.warn(
    `[north] graph catalog projection failed for ${what}; falling back to packaged JSON. ${detail}`,
  );
}

/** Graph projection of staffing/catalog.json (same shape the file loader parses). */
export function projectStaffingCatalog(): unknown {
  return catalogBundle().staffing;
}

/** Graph projection of providers/<provider>.json (same shape the file loader parses). */
export function projectProviderCatalog(provider: string): unknown {
  const catalog = catalogBundle().providers[provider];
  if (catalog === undefined)
    throw new Error(`NORTH_STAFFING_SOURCE=graph bundle has no provider catalog for ${provider}`);
  return catalog;
}

/**
 * §3.1 point 6 receipt evidence: the catalog subgraph digest plus the two
 * version watermarks that name the EXACT graph state an admission accepted.
 * Replaces the catalog-FILE sha256s the receipt carried in file mode, so a
 * graph-mode receipt never digests a file the graph may no longer mirror.
 */
export interface CatalogGraphPin {
  catalogVersion: number;
  coordinatorVersion: number;
  catalogDigestSha256: string;
}

const HEX64 = /^[0-9a-f]{64}$/;

export function projectCatalogGraphPin(): CatalogGraphPin {
  const raw = project(["catalog-pin"]) as Record<string, unknown>;
  const catalogVersion = Number(raw.catalogVersion);
  const coordinatorVersion = Number(raw.coordinatorVersion);
  const catalogDigestSha256 = raw.catalogDigestSha256;
  if (!Number.isInteger(catalogVersion) || catalogVersion < 1)
    throw new Error("catalog graph pin: catalogVersion must be a positive integer");
  if (!Number.isInteger(coordinatorVersion) || coordinatorVersion < 0)
    throw new Error("catalog graph pin: coordinatorVersion must be a non-negative integer");
  if (typeof catalogDigestSha256 !== "string" || !HEX64.test(catalogDigestSha256))
    throw new Error("catalog graph pin: catalogDigestSha256 must be a sha256 digest");
  return { catalogVersion, coordinatorVersion, catalogDigestSha256 };
}

/** One pin probe per fresh admission process, shared by bundle + receipt. */
export function catalogGraphPinForAdmission(): CatalogGraphPin {
  const p = port();
  if (cachedCatalogGraphPin?.port === p) return cachedCatalogGraphPin.pin;
  const pin = projectCatalogGraphPin();
  cachedCatalogGraphPin = { port: p, pin };
  return pin;
}
