// Offline OpenTelemetry span core. This intentionally exports only to local OTLP/JSON:
// observations must never become North facts or introduce a network dependency.
import { appendFile, mkdir, readdir, rm } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { homedir } from "node:os";
import { join } from "node:path";

export type SpanAttributeValue = string | number | boolean | readonly SpanAttributeValue[];
export type SpanAttributes = Readonly<Record<string, SpanAttributeValue>>;

export interface SpanContext {
  traceId: string;
  spanId: string;
  traceFlags: string;
}

export interface SpanOptions {
  parent?: Span | SpanContext;
  attributes?: SpanAttributes;
  startTime?: number | bigint;
}

export interface TracerOptions {
  /** Defaults to ~/.local/state/north/traces; use a temporary directory in tests. */
  traceDir?: string;
  serviceName?: string;
  scopeName?: string;
  retentionDays?: number;
  now?: () => number;
}

/** OTel severity-number boundaries (the named level is the lowest number in its range). */
export const SeverityNumber = {
  TRACE: 1,
  DEBUG: 5,
  INFO: 9,
  WARN: 13,
  ERROR: 17,
  FATAL: 21,
} as const;

export type SeverityName = keyof typeof SeverityNumber;

const TRACEPARENT_RE = /^([\da-f]{2})-([\da-f]{32})-([\da-f]{16})-([\da-f]{2})(?:-[\da-f-]+)?$/i;
const ZERO_TRACE_ID = "0".repeat(32);
const ZERO_SPAN_ID = "0".repeat(16);

export function parseTraceparent(value: string | undefined): SpanContext | undefined {
  if (!value) return undefined;
  const raw = value.trim();
  const match = TRACEPARENT_RE.exec(raw);
  if (!match) return undefined;
  const [, version, traceId, spanId, traceFlags] = match;
  // Version ff is forbidden by Trace Context; all-zero IDs are invalid too.
  if (version.toLowerCase() === "ff" || (version === "00" && raw.split("-").length !== 4)
    || traceId === ZERO_TRACE_ID || spanId === ZERO_SPAN_ID) return undefined;
  return { traceId: traceId.toLowerCase(), spanId: spanId.toLowerCase(), traceFlags: traceFlags.toLowerCase() };
}

/** Emit the W3C version-00 form North currently supports. */
export function formatTraceparent(context: SpanContext): string {
  const parsed = parseTraceparent(`00-${context.traceId}-${context.spanId}-${context.traceFlags}`);
  if (!parsed) throw new TypeError("invalid W3C trace context");
  return `00-${parsed.traceId}-${parsed.spanId}-${parsed.traceFlags}`;
}

/** A fresh child-process environment with the current span context in standard W3C form. */
export function traceparentEnv(context: SpanContext, env: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  return { ...env, TRACEPARENT: formatTraceparent(context) };
}

export function configuredSeverity(env: NodeJS.ProcessEnv = process.env): number {
  const supplied = env.NORTH_LOG_LEVEL ?? env.OTEL_LOG_LEVEL ?? "INFO";
  const name = supplied.trim().toUpperCase() as SeverityName;
  return SeverityNumber[name] ?? SeverityNumber.INFO;
}

function unixNano(time: number | bigint): string {
  return (typeof time === "bigint" ? time : BigInt(Math.floor(time)) * 1_000_000n).toString();
}

function dateKey(time: number): string {
  return new Date(time).toISOString().slice(0, 10);
}

function randomId(bytes: number): string {
  return randomBytes(bytes).toString("hex");
}

function attributeValue(value: SpanAttributeValue): Record<string, unknown> {
  if (typeof value === "string") return { stringValue: value };
  if (typeof value === "boolean") return { boolValue: value };
  if (typeof value === "number") {
    if (Number.isInteger(value)) return { intValue: String(value) };
    return { doubleValue: value };
  }
  return { arrayValue: { values: value.map(attributeValue) } };
}

function attributes(attributes: SpanAttributes): Array<Record<string, unknown>> {
  return Object.entries(attributes).map(([key, value]) => ({ key, value: attributeValue(value) }));
}

export class Span {
  readonly context: SpanContext;
  readonly parentContext?: SpanContext;
  readonly name: string;
  readonly attributes: SpanAttributes;
  readonly startTime: number | bigint;
  private ended = false;

  constructor(private readonly tracer: Tracer, name: string, parent: SpanContext | undefined, options: SpanOptions) {
    this.name = name;
    this.parentContext = parent;
    this.context = { traceId: parent?.traceId ?? randomId(16), spanId: randomId(8), traceFlags: parent?.traceFlags ?? "01" };
    this.attributes = options.attributes ?? {};
    this.startTime = options.startTime ?? tracer.now();
  }

  startChild(name: string, options: Omit<SpanOptions, "parent"> = {}): Span {
    return this.tracer.startSpan(name, { ...options, parent: this });
  }

  childProcessEnv(env?: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
    return traceparentEnv(this.context, env);
  }

  /** Scheduling the append is best-effort and deliberately never awaits or throws. */
  end(endTime: number | bigint = this.tracer.now()): void {
    if (this.ended) return;
    this.ended = true;
    this.tracer.emit(this, endTime);
  }
}

export class Tracer {
  readonly traceDir: string;
  readonly serviceName: string;
  readonly scopeName: string;
  readonly retentionDays: number;
  readonly now: () => number;
  private pending = new Set<Promise<void>>();

  constructor(options: TracerOptions = {}) {
    this.traceDir = options.traceDir ?? join(homedir(), ".local", "state", "north", "traces");
    this.serviceName = options.serviceName ?? "north";
    this.scopeName = options.scopeName ?? "north";
    const configuredRetention = Number(process.env.NORTH_TRACE_RETENTION_DAYS ?? 7);
    this.retentionDays = Math.max(0, Math.floor(options.retentionDays ?? (Number.isFinite(configuredRetention) ? configuredRetention : 7)));
    this.now = options.now ?? Date.now;
  }

  startSpan(name: string, options: SpanOptions = {}): Span {
    const parent = options.parent instanceof Span ? options.parent.context : options.parent ?? parseTraceparent(process.env.TRACEPARENT);
    return new Span(this, name, parent, options);
  }

  emit(span: Span, endTime: number | bigint): void {
    try {
      const task = this.append(span, endTime).catch(() => undefined).then(() => undefined);
      this.pending.add(task);
      void task.finally(() => this.pending.delete(task));
    } catch {
      // A tracing failure is never allowed to affect the operation being traced.
    }
  }

  /** Wait for already scheduled writes; intended for orderly shutdown and tests, never hot paths. */
  async flush(): Promise<void> {
    while (this.pending.size) await Promise.all([...this.pending]);
  }

  private async append(span: Span, endTime: number | bigint): Promise<void> {
    const endMillis = typeof endTime === "bigint" ? Number(endTime / 1_000_000n) : endTime;
    const file = join(this.traceDir, `${dateKey(endMillis)}.otlp.jsonl`);
    const otlp = {
      resourceSpans: [{
        resource: { attributes: [{ key: "service.name", value: { stringValue: this.serviceName } }] },
        scopeSpans: [{
          scope: { name: this.scopeName },
          spans: [{
            traceId: span.context.traceId,
            spanId: span.context.spanId,
            ...(span.parentContext ? { parentSpanId: span.parentContext.spanId } : {}),
            name: span.name,
            kind: 1,
            startTimeUnixNano: unixNano(span.startTime),
            endTimeUnixNano: unixNano(endTime),
            attributes: attributes(span.attributes),
          }],
        }],
      }],
    };
    await mkdir(this.traceDir, { recursive: true, mode: 0o700 });
    await appendFile(file, `${JSON.stringify(otlp)}\n`, { encoding: "utf8", mode: 0o600 });
    await this.prune(endMillis);
  }

  private async prune(now: number): Promise<void> {
    const earliest = new Date(now - this.retentionDays * 86_400_000).toISOString().slice(0, 10);
    const entries = await readdir(this.traceDir, { withFileTypes: true });
    await Promise.all(entries.filter((entry) => entry.isFile() && /^\d{4}-\d{2}-\d{2}\.otlp\.jsonl$/.test(entry.name)
      && entry.name.slice(0, 10) < earliest).map((entry) => rm(join(this.traceDir, entry.name), { force: true })));
  }
}

export function createTracer(options: TracerOptions = {}): Tracer {
  return new Tracer(options);
}
