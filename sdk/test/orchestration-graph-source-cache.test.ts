import { afterEach, beforeEach, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  catalogBundle,
  resetCatalogBundleCache,
} from "../src/orchestration-graph-source";

const SOURCE = resolve(import.meta.dir, "../src/orchestration-graph-source.ts");
const original = {
  NORTH_PEER_BB: process.env.NORTH_PEER_BB,
  NORTH_PORT: process.env.NORTH_PORT,
  NORTH_ORCHESTRATION_CATALOG_CACHE: process.env.NORTH_ORCHESTRATION_CATALOG_CACHE,
};

let scratch: string;
let fakeBb: string;
let statePath: string;
let logPath: string;
let cachePath: string;

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

function digest(staffing: unknown, providers: Record<string, unknown>): string {
  return createHash("sha256").update(JSON.stringify(canonical({ staffing, providers }))).digest("hex");
}

function setCatalog(marker: string, catalogVersion: number, bundleDelayMs = 0): string {
  const staffing = { version: 2, presets: [{ name: marker }] };
  const providers = {
    anthropic: { provider: "anthropic", marker },
    openai: { provider: "openai", marker },
  };
  const catalogDigestSha256 = digest(staffing, providers);
  writeFileSync(statePath, JSON.stringify({
    catalogVersion,
    coordinatorVersion: 1000 + catalogVersion,
    catalogDigestSha256,
    staffing,
    providers,
    bundleDelayMs,
  }));
  return catalogDigestSha256;
}

function calls(): string[] {
  if (!existsSync(logPath)) return [];
  return readFileSync(logPath, "utf8").trim().split("\n").filter(Boolean);
}

function marker(): string {
  return (catalogBundle().staffing as { presets: Array<{ name: string }> }).presets[0]!.name;
}

beforeEach(() => {
  scratch = mkdtempSync(join(tmpdir(), "north-catalog-cache-"));
  fakeBb = join(scratch, "fake-bb");
  statePath = join(scratch, "catalog-state.json");
  logPath = join(scratch, "projector.log");
  cachePath = join(scratch, "state", "north", "catalog-cache.json");
  writeFileSync(fakeBb, `#!/usr/bin/env bun
import { appendFileSync, readFileSync } from "node:fs";
const verb = process.argv.at(-1);
appendFileSync(${JSON.stringify(logPath)}, verb + "\\n");
const state = JSON.parse(readFileSync(${JSON.stringify(statePath)}, "utf8"));
if (verb === "catalog-pin") {
  console.log(JSON.stringify({
    catalogVersion: state.catalogVersion,
    coordinatorVersion: state.coordinatorVersion,
    catalogDigestSha256: state.catalogDigestSha256,
  }));
} else if (verb === "bundle") {
  if (state.bundleDelayMs) await Bun.sleep(state.bundleDelayMs);
  console.log(JSON.stringify({
    catalogVersion: state.catalogVersion,
    staffing: state.staffing,
    providers: state.providers,
  }));
} else {
  process.exit(2);
}
`);
  chmodSync(fakeBb, 0o755);
  process.env.NORTH_PEER_BB = fakeBb;
  process.env.NORTH_PORT = "17977";
  process.env.NORTH_ORCHESTRATION_CATALOG_CACHE = cachePath;
  resetCatalogBundleCache();
});

afterEach(() => {
  resetCatalogBundleCache();
  for (const [key, value] of Object.entries(original)) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  rmSync(scratch, { recursive: true, force: true });
});

test("cold cache performs the full projection and publishes a digest-pinned record", () => {
  const expectedDigest = setCatalog("cold", 1);
  expect(marker()).toBe("cold");
  expect(calls()).toEqual(["catalog-pin", "bundle"]);

  const record = JSON.parse(readFileSync(cachePath, "utf8"));
  expect(record).toMatchObject({
    version: 1,
    catalogDigestSha256: expectedDigest,
    coordinatorVersion: 1001,
    catalogVersion: 1,
  });
  expect(record.bundle.staffing.presets[0].name).toBe("cold");
});

test("a changed catalog digest invalidates the durable projection", () => {
  setCatalog("before", 1);
  expect(marker()).toBe("before");

  resetCatalogBundleCache();
  writeFileSync(logPath, "");
  expect(marker()).toBe("before");
  expect(calls()).toEqual(["catalog-pin"]);

  const changedDigest = setCatalog("after", 2);
  resetCatalogBundleCache();
  expect(marker()).toBe("after");
  expect(calls()).toEqual(["catalog-pin", "catalog-pin", "bundle"]);
  expect(JSON.parse(readFileSync(cachePath, "utf8")).catalogDigestSha256).toBe(changedDigest);
});

test("concurrent admissions atomically replace one whole parseable cache record", async () => {
  setCatalog("concurrent", 3, 200);
  const child = join(scratch, "admit.ts");
  writeFileSync(child, `import { catalogBundle } from ${JSON.stringify(SOURCE)};
const bundle = catalogBundle();
console.log(bundle.staffing.presets[0].name);
`);

  const children = Array.from({ length: 12 }, () => Bun.spawn(
    [process.execPath, child],
    { env: { ...process.env }, stdout: "pipe", stderr: "pipe" },
  ));
  const tornReads: string[] = [];
  const monitor = setInterval(() => {
    if (!existsSync(cachePath)) return;
    try {
      const record = JSON.parse(readFileSync(cachePath, "utf8"));
      if (record.bundle?.staffing?.presets?.[0]?.name !== "concurrent")
        tornReads.push("valid JSON with an incomplete projection");
    } catch (error) {
      tornReads.push(String(error));
    }
  }, 1);
  try {
    const results = await Promise.all(children.map(async (process) => ({
      exit: await process.exited,
      out: await new Response(process.stdout).text(),
      err: await new Response(process.stderr).text(),
    })));
    expect(results.map(({ exit }) => exit)).toEqual(Array(12).fill(0));
    expect(results.map(({ out }) => out.trim())).toEqual(Array(12).fill("concurrent"));
    expect(results.map(({ err }) => err)).toEqual(Array(12).fill(""));
  } finally {
    clearInterval(monitor);
  }
  expect(tornReads).toEqual([]);
  expect(JSON.parse(readFileSync(cachePath, "utf8")).bundle.staffing.presets[0].name)
    .toBe("concurrent");
  expect(readdirSync(resolve(cachePath, "..")).filter((name) => name.includes(".tmp-")))
    .toEqual([]);
  expect(calls().filter((call) => call === "bundle").length).toBeGreaterThan(1);
});

test("warm digest-matched admission completes under one second", () => {
  setCatalog("timed", 4, 1_150);
  const coldStarted = performance.now();
  expect(marker()).toBe("timed");
  const coldMs = performance.now() - coldStarted;

  resetCatalogBundleCache();
  const warmStarted = performance.now();
  expect(marker()).toBe("timed");
  const warmMs = performance.now() - warmStarted;

  expect(coldMs).toBeGreaterThanOrEqual(1_100);
  expect(warmMs).toBeLessThan(1_000);
  expect(calls()).toEqual(["catalog-pin", "bundle", "catalog-pin"]);
  console.log(JSON.stringify({ coldMs: Math.round(coldMs), warmMs: Math.round(warmMs) }));
});
