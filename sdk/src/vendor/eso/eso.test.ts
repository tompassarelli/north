import { describe, test, expect } from "bun:test";
import { encode, decode, tryDecode, isRecord } from "./index.js";
import { crush, replay, strip, isSentinel, hashOf, evaluateCacheStability, DEFAULTS } from "./ccr.js";

function cache() {
  const values = new Map<string, unknown[]>();
  return {
    put(hash: string, original: unknown[]) { values.set(hash, original); },
    get(hash: string) { return values.get(hash); },
  };
}

// Deliberately sees every serialised element: a CCR marker costs tokens too.
const tokenCount = (value: readonly unknown[]) => JSON.stringify(value).length;

// ── ESO round-trip ───────────────────────────────────────────────────────────

describe("ESO encode/decode round-trip", () => {
  test("record array — basic", () => {
    const records = Array.from({ length: 12 }, (_, i) => ({
      id: String(i),
      path: `src/file${i}.ts`,
      line: i + 1,
    }));
    const doc = { findings: records };
    const rt = decode(encode(doc));
    expect(rt.findings).toEqual(records);
  });

  test("empty array", () => {
    const doc = { items: [] };
    const rt = decode(encode(doc));
    expect(rt.items).toEqual([]);
  });

  test("unicode values in cells", () => {
    const records = Array.from({ length: 10 }, (_, i) => ({
      msg: `héllo wörld ${i} — 日本語`,
      n: i,
    }));
    const doc = { rows: records };
    const rt = decode(encode(doc));
    expect(rt.rows).toEqual(records);
  });

  test("tabs and newlines in string values use JSON quoting", () => {
    const records = Array.from({ length: 10 }, (_, i) => ({
      text: `line\tone\ntwo\t${i}`,
      idx: i,
    }));
    const doc = { data: records };
    const rt = decode(encode(doc));
    expect(rt.data).toEqual(records);
  });

  test("null and boolean values", () => {
    const records = Array.from({ length: 10 }, (_, i) => ({
      ok: i % 2 === 0,
      val: i % 3 === 0 ? null : i,
    }));
    const doc = { rows: records };
    const rt = decode(encode(doc));
    expect(rt.rows).toEqual(records);
  });

  test("scalar fields survive round-trip", () => {
    const doc = {
      from: "agent-a",
      to: "agent-b",
      count: 42,
      done: true,
      nothing: null,
      items: [{ x: "1" }, { x: "2" }, { x: "3" }, { x: "4" }, { x: "5" },
               { x: "6" }, { x: "7" }, { x: "8" }, { x: "9" }, { x: "10" }],
    };
    const rt = decode(encode(doc));
    expect(rt.from).toBe("agent-a");
    expect(rt.to).toBe("agent-b");
    expect(rt.count).toBe(42);
    expect(rt.done).toBe(true);
    expect(rt.nothing).toBe(null);
    expect(rt.items).toEqual(doc.items);
  });

  test("count is a checksum — truncated rows fail", () => {
    // Manually craft a document with wrong count
    const bad = "!eso/1\nrows[5]{a,b}\nv1\tv2\n";
    const result = tryDecode(bad);
    expect(result.ok).toBe(false);
  });

  test("tryDecode returns ok:false on invalid input", () => {
    const result = tryDecode("not eso at all");
    expect(result.ok).toBe(false);
    expect(result.error).toBeDefined();
  });

  test("tryDecode returns ok:true on valid input", () => {
    const encoded = encode({ x: "hello" });
    const result = tryDecode(encoded);
    expect(result.ok).toBe(true);
    expect((result as any).value.x).toBe("hello");
  });

  test("isRecord utility", () => {
    expect(isRecord({})).toBe(true);
    expect(isRecord([])).toBe(false);
    expect(isRecord(null)).toBe(false);
    expect(isRecord("str")).toBe(false);
  });
});

// ── CCR crush/retrieve fidelity ──────────────────────────────────────────────

describe("CCR crush/strip", () => {
  test("passthrough below minItems", () => {
    const arr = [{ a: "1" }, { a: "2" }, { a: "3" }];
    const { view, hash, dropped } = crush(arr);
    expect(view).toBe(arr); // same reference
    expect(hash).toBe(null);
    expect(dropped).toBe(0);
  });

  test("crush large array: dropped > 0, sentinel present", () => {
    const arr = Array.from({ length: 50 }, (_, i) => ({ id: i, val: `v${i}` }));
    const { view, hash, dropped } = crush(arr, { cache: cache(), tokenCount });
    expect(dropped).toBeGreaterThan(0);
    expect(hash).not.toBe(null);
    expect(view.length).toBeLessThan(arr.length);
    // sentinel is last element
    const sentinel = view[view.length - 1];
    expect(isSentinel(sentinel)).toBe(true);
  });

  test("strip removes sentinel", () => {
    const arr = Array.from({ length: 20 }, (_, i) => ({ n: i }));
    const { view } = crush(arr, { cache: cache(), tokenCount });
    const stripped = strip(view);
    expect(stripped.every((x: any) => !isSentinel(x))).toBe(true);
    expect(stripped.length).toBeGreaterThan(0);
  });

  test("hash is deterministic", () => {
    const arr = [{ x: "a" }, { x: "b" }, { x: "c" }];
    expect(hashOf(arr)).toBe(hashOf(arr));
    expect(hashOf(arr)).not.toBe(hashOf([{ x: "b" }]));
  });

  test("crush keeps first and last element", () => {
    const arr = Array.from({ length: 30 }, (_, i) => ({ n: i }));
    const { view } = crush(arr, { cache: cache(), tokenCount });
    const stripped = strip(view);
    const ids = stripped.map((r: any) => r.n);
    expect(ids).toContain(0);   // first
    expect(ids).toContain(29);  // last
  });

  test("crush preserves change-points (level transitions)", () => {
    // 20 info rows, then 5 warn rows, then 5 info rows
    const arr = [
      ...Array.from({ length: 20 }, (_, i) => ({ level: "info", n: i })),
      ...Array.from({ length: 5 },  (_, i) => ({ level: "warn", n: 20 + i })),
      ...Array.from({ length: 5 },  (_, i) => ({ level: "info", n: 25 + i })),
    ];
    const { view } = crush(arr, { maxItems: 15, cache: cache(), tokenCount });
    const stripped = strip(view);
    const levels = stripped.map((r: any) => r.level);
    // Must include at least one warn
    expect(levels).toContain("warn");
  });

  test("isSentinel rejects non-sentinel objects", () => {
    expect(isSentinel({ _ccr: "random string" })).toBe(false);
    expect(isSentinel({ other: "field" })).toBe(false);
    expect(isSentinel(null)).toBe(false);
    expect(isSentinel("string")).toBe(false);
  });

  test("does not emit a marker when marker-inclusive token count inflates", () => {
    const arr = Array.from({ length: 20 }, (_, i) => ({ id: i }));
    const seen: unknown[][] = [];
    const result = crush(arr, {
      cache: cache(),
      tokenCount(value) { seen.push([...value]); return value.length === arr.length ? 1 : 2; },
    });
    expect(result).toEqual({ view: arr, hash: null, dropped: 0 });
    expect(seen.some((value) => isSentinel(value.at(-1)))).toBe(true);
  });

  test("lossy views require an exact retrievable original and replay it losslessly", () => {
    const arr = Array.from({ length: 40 }, (_, i) => ({ nested: { i }, text: `row ${i}` }));
    expect(crush(arr, { tokenCount }).hash).toBeNull();
    const unavailable = { put() {}, get() { return undefined; } };
    expect(crush(arr, { cache: unavailable, tokenCount }).hash).toBeNull();

    const stored = cache();
    const result = crush(arr, { cache: stored, tokenCount });
    expect(result.hash).not.toBeNull();
    arr[0].nested.i = 999;
    expect(replay(result, stored)).toEqual(Array.from({ length: 40 }, (_, i) => ({ nested: { i }, text: `row ${i}` })));
  });

  test("replay rejects cache corruption instead of returning a near match", () => {
    const arr = Array.from({ length: 30 }, (_, i) => ({ i }));
    const stored = cache();
    const result = crush(arr, { cache: stored, tokenCount });
    const corrupt = { put: stored.put, get: () => [{ i: "wrong" }] };
    expect(() => replay(result, corrupt)).toThrow("ccr cache miss or corruption");
  });

  test("protects errors, tags, explicit outliers, and whole tool exchanges", () => {
    const arr: Array<Record<string, unknown>> = Array.from({ length: 40 }, (_, i) => ({ level: "info", i }));
    arr[9] = { level: "error", i: 9, error: "failed" };
    arr[17] = { level: "info", i: 17, tags: ["keep"] };
    arr[23] = { level: "info", i: 23, outlier: true };
    arr[29] = { level: "info", i: 29, tool_call_id: "call-1", type: "tool_use" };
    arr[30] = { level: "info", i: 30, tool_call_id: "call-1", type: "tool_result" };
    const result = crush(arr, { maxItems: 5, cache: cache(), tokenCount });
    const kept = strip(result.view).map((row: any) => row.i);
    expect(kept).toEqual(expect.arrayContaining([9, 17, 23, 29, 30]));
  });

  test("selection and replay are deterministic", () => {
    const arr = Array.from({ length: 40 }, (_, i) => ({ level: i === 21 ? "error" : "info", i }));
    const one = crush(arr, { cache: cache(), tokenCount });
    const two = crush(arr, { cache: cache(), tokenCount });
    expect(one.hash).toBe(two.hash);
    expect(one.view).toEqual(two.view);
  });
});

describe("CCR passive cache evaluator", () => {
  test("reports drift only across an unchanged caller-supplied prefix", () => {
    expect(evaluateCacheStability([
      { prefix: "stable", cacheReadTokens: 90 },
      { prefix: "stable", cacheReadTokens: 84 },
    ])).toEqual({ stablePrefix: true, cacheTokenDrift: -6, sampleCount: 2 });
    expect(evaluateCacheStability([
      { prefix: "first", cachedInputTokens: 90 },
      { prefix: "second", cachedInputTokens: 84 },
    ])).toEqual({ stablePrefix: false, cacheTokenDrift: undefined, sampleCount: 2 });
  });
});
