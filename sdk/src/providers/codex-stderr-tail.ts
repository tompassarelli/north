import { redactObviousSecrets } from "../privacy-filter";

/**
 * Bounded, redacted provider stderr — the diagnostics a Codex failure carries.
 *
 * Adapted from hermes-agent (MIT, Copyright (c) 2025 Nous Research):
 * `agent/transports/codex_app_server.py:353-368` keeps the last 500 stderr
 * lines in a lock-guarded ring, and
 * `agent/transports/codex_app_server_session.py:327-362` appends a redacted
 * last-N-line tail to every user-facing codex error, because the CLI's own
 * error text ("Internal error", "turn/start failed: ...") is opaque on its own.
 * North's shape differs in one way that matters: the raw stderr lives in a
 * SEPARATE process (the parent-death supervisor), so the tail is pushed to the
 * host as it is produced rather than pulled at failure time. Both ends keep a
 * ring; both redact on insert.
 */

/** Bound the memory a wedged provider's stderr can cost: last N lines only. */
export const STDERR_RING_LINES = 500;
/** How much of the ring rides a failure message. */
export const STDERR_TAIL_LINES = 40;
/** One diagnostic line is a diagnostic, not a payload channel. */
export const STDERR_LINE_BYTES = 512;

/**
 * Strip control characters, redact obvious credentials, and bound the length.
 * Applied on INSERT so neither ring ever holds an unredacted secret.
 */
export function redactProviderStderrLine(line: string): string {
  let value = redactObviousSecrets(line)
    .replace(/\r/g, "").replace(/[\u0000-\u0008\u000b-\u001f\u007f]/g, "");
  value = value.trimEnd();
  if (Buffer.byteLength(value, "utf8") <= STDERR_LINE_BYTES) return value;
  // Truncate on a byte bound without splitting a code point.
  const truncated = Buffer.from(value, "utf8").subarray(0, STDERR_LINE_BYTES).toString("utf8");
  return `${truncated.replace(/\ufffd$/, "")}…`;
}

/**
 * A bounded ring of provider stderr lines. Lines are redacted and truncated as
 * they arrive; only the last {@link STDERR_RING_LINES} survive.
 */
export class ProviderStderrRing {
  private lines: string[] = [];
  private partial = "";

  constructor(private readonly capacity = STDERR_RING_LINES) {}

  /** Record one already-split line. Returns the redacted form that was kept. */
  add(line: string): string {
    const value = redactProviderStderrLine(line);
    this.lines.push(value);
    if (this.lines.length > this.capacity)
      this.lines = this.lines.slice(this.lines.length - this.capacity);
    return value;
  }

  /** Feed raw bytes; returns the lines completed by this chunk, in order. */
  push(chunk: Buffer | string): string[] {
    this.partial += typeof chunk === "string" ? chunk : chunk.toString("utf8");
    const produced: string[] = [];
    while (true) {
      const newline = this.partial.indexOf("\n");
      if (newline < 0) break;
      const line = this.partial.slice(0, newline);
      this.partial = this.partial.slice(newline + 1);
      produced.push(this.add(line));
    }
    // A provider that never emits a newline must not grow the buffer without
    // bound: fold an oversized fragment into the ring as its own line.
    if (Buffer.byteLength(this.partial, "utf8") > STDERR_LINE_BYTES) {
      produced.push(this.add(this.partial));
      this.partial = "";
    }
    return produced;
  }

  /** Flush a trailing partial line at stream end. */
  finish(): string[] {
    if (!this.partial) return [];
    const line = this.add(this.partial);
    this.partial = "";
    return [line];
  }

  /** The most recent `count` lines, oldest first. */
  tail(count = STDERR_TAIL_LINES): string[] {
    return count >= this.lines.length ? [...this.lines] : this.lines.slice(this.lines.length - count);
  }

  get size(): number { return this.lines.length; }
}

/**
 * Render a tail for a human-readable failure message, or `undefined` when the
 * provider said nothing. Mirrors hermes' `_format_error_with_stderr` footer.
 */
export function formatProviderStderrTail(tail: readonly string[]): string | undefined {
  const lines = tail.filter((line) => line.trim() !== "");
  if (!lines.length) return undefined;
  return `codex stderr (last ${lines.length} line${lines.length === 1 ? "" : "s"}):\n${lines.join("\n")}`;
}
