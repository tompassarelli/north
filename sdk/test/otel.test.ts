import { afterEach, describe, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createTracer, configuredSeverity, formatTraceparent, parseTraceparent, SeverityNumber, traceparentEnv } from "../src/otel";

const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true }); });
function traceDir(): string { const root = mkdtempSync(join(tmpdir(), "north-otel-")); roots.push(root); return root; }

describe("offline OTLP span export", () => {
  test("emits valid OTLP/JSON with parent-child nesting and nanosecond strings", async () => {
    const dir = traceDir();
    const tracer = createTracer({ traceDir: dir, now: () => Date.UTC(2026, 6, 29, 12), retentionDays: 7 });
    const parent = tracer.startSpan("north.command", { attributes: { "north.thread.id": "t-1", retries: 2, cold: false } });
    const child = parent.startChild("north.lane.lease", { attributes: { "north.lane.id": "lane-1" } });
    child.end(Date.UTC(2026, 6, 29, 12, 0, 1));
    parent.end(Date.UTC(2026, 6, 29, 12, 0, 2));
    await tracer.flush();
    const rows = readFileSync(join(dir, "2026-07-29.otlp.jsonl"), "utf8").trim().split("\n").map(JSON.parse);
    const spans = rows.map((row) => row.resourceSpans[0].scopeSpans[0].spans[0]);
    const byName = new Map(spans.map((span) => [span.name, span]));
    expect(spans).toHaveLength(2);
    expect(byName.get("north.lane.lease")!.parentSpanId).toBe(parent.context.spanId);
    expect(byName.get("north.lane.lease")!.traceId).toBe(parent.context.traceId);
    expect(byName.get("north.command")!.parentSpanId).toBeUndefined();
    expect(byName.get("north.command")!.startTimeUnixNano).toBe("1785326400000000000");
    expect(typeof byName.get("north.lane.lease")!.endTimeUnixNano).toBe("string");
    expect(rows[0].resourceSpans[0].resource.attributes[0]).toEqual({ key: "service.name", value: { stringValue: "north" } });
  });

  test("round-trips real W3C TRACEPARENT and injects it into child env", () => {
    const original = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    const context = parseTraceparent(original)!;
    expect(formatTraceparent(context)).toBe(original);
    expect(traceparentEnv(context, { PATH: "/bin" })).toEqual({ PATH: "/bin", TRACEPARENT: original });
    expect(parseTraceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01")).toBeUndefined();
  });

  test("retention removes dated OTLP files older than the configured window", async () => {
    const dir = traceDir();
    writeFileSync(join(dir, "2026-07-20.otlp.jsonl"), "old\n");
    const tracer = createTracer({ traceDir: dir, retentionDays: 7, now: () => Date.UTC(2026, 6, 29) });
    tracer.startSpan("north.command").end();
    await tracer.flush();
    expect(existsSync(join(dir, "2026-07-20.otlp.jsonl"))).toBe(false);
  });

  test("NORTH_LOG_LEVEL overrides OTEL_LOG_LEVEL and defaults to INFO", () => {
    expect(configuredSeverity({})).toBe(SeverityNumber.INFO);
    expect(configuredSeverity({ OTEL_LOG_LEVEL: "debug" })).toBe(SeverityNumber.DEBUG);
    expect(configuredSeverity({ OTEL_LOG_LEVEL: "debug", NORTH_LOG_LEVEL: "ERROR" })).toBe(SeverityNumber.ERROR);
  });

  test("an unrecognised level falls back to INFO rather than throwing", () => {
    expect(configuredSeverity({ NORTH_LOG_LEVEL: "chatty" })).toBe(SeverityNumber.INFO);
    expect(configuredSeverity({ NORTH_LOG_LEVEL: "" })).toBe(SeverityNumber.INFO);
  });

  // The load-bearing safety property. Tracing sits on hot paths, so a broken
  // sink must degrade to silence — never to a failure in the operation being
  // traced. An unwritable directory is the realistic form of "broken".
  test("a failing sink never throws into the caller", async () => {
    const dir = join(traceDir(), "definitely", "not", "creatable\0bad");
    const tracer = createTracer({ traceDir: dir });
    const span = tracer.startSpan("north.command");
    expect(() => span.end()).not.toThrow();
    await expect(tracer.flush()).resolves.toBeUndefined();
  });

  test("ending a span twice is a no-op, not a duplicate export", async () => {
    const dir = traceDir();
    const tracer = createTracer({ traceDir: dir, now: () => Date.UTC(2026, 6, 29, 12) });
    const span = tracer.startSpan("north.command");
    span.end();
    span.end();
    await tracer.flush();
    const rows = readFileSync(join(dir, "2026-07-29.otlp.jsonl"), "utf8").trim().split("\n");
    expect(rows).toHaveLength(1);
  });

  // Retention used to run inside every append: a full readdir per span ended,
  // a filesystem scan on the hot path of the subsystem that measures hot paths.
  // Files are named by day, so re-scanning within a day can never find anything
  // the first scan missed.
  test("retention scans at most once per day, not once per span", async () => {
    const dir = traceDir();
    let scans = 0;
    const tracer = createTracer({ traceDir: dir, now: () => Date.UTC(2026, 6, 29, 12) });
    const realPrune = (tracer as unknown as { prune: (n: number) => Promise<void> }).prune;
    (tracer as unknown as { prune: (n: number) => Promise<void> }).prune = async (n: number) => {
      scans += 1;
      return realPrune.call(tracer, n);
    };
    for (let i = 0; i < 25; i += 1) tracer.startSpan(`span-${i}`).end();
    await tracer.flush();
    expect(scans).toBe(1);
    const rows = readFileSync(join(dir, "2026-07-29.otlp.jsonl"), "utf8").trim().split("\n");
    expect(rows).toHaveLength(25);
  });

  test("a span with no parent and no TRACEPARENT starts a fresh trace", async () => {
    const dir = traceDir();
    const tracer = createTracer({ traceDir: dir });
    const a = tracer.startSpan("a");
    const b = tracer.startSpan("b");
    expect(a.context.traceId).not.toBe(b.context.traceId);
    expect(a.context.traceId).toMatch(/^[\da-f]{32}$/);
    expect(a.context.spanId).toMatch(/^[\da-f]{16}$/);
  });
});
