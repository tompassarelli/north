export const CODEX_SUPERVISOR_STATUS_PREFIX = "NORTH_CODEX_SUPERVISOR 1 " as const;
/**
 * Provider stderr forwarded to the host, one bounded line per packet, base64 so
 * a diagnostic can never forge a newline or another receipt. Opt-in: only a
 * supervisor launched with `--stderr-tail` emits these, because the one-shot
 * and Linear-broker status readers admit exactly three receipts and nothing
 * else. Adapted from hermes-agent (MIT, Copyright (c) 2025 Nous Research),
 * `agent/transports/codex_app_server.py:353-368`.
 */
export const CODEX_SUPERVISOR_STDERR_PREFIX = "STDERR " as const;
export const CODEX_SUPERVISOR_STDERR_FLAG = "--stderr-tail" as const;

export type CodexSupervisorStatus =
  | "STARTED"
  | "UNAVAILABLE"
  | `EXIT ${number}`
  | `STDERR ${string}`;

export function codexSupervisorStatusLine(status: CodexSupervisorStatus): string {
  return `${CODEX_SUPERVISOR_STATUS_PREFIX}${status}`;
}

/** Encode one already-redacted provider stderr line as a status receipt. */
export function codexSupervisorStderrStatus(line: string): CodexSupervisorStatus {
  return `${CODEX_SUPERVISOR_STDERR_PREFIX}${Buffer.from(line, "utf8").toString("base64")}`;
}

/**
 * Decode a `STDERR` receipt body, or `undefined` when the status is not one.
 * Fails closed on anything that is not canonical base64.
 */
export function codexSupervisorStderrLine(status: string): string | undefined {
  if (!status.startsWith(CODEX_SUPERVISOR_STDERR_PREFIX)) return undefined;
  const encoded = status.slice(CODEX_SUPERVISOR_STDERR_PREFIX.length);
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(encoded)) return undefined;
  const decoded = Buffer.from(encoded, "base64");
  if (decoded.toString("base64").replace(/=+$/, "") !== encoded.replace(/=+$/, "")) return undefined;
  return decoded.toString("utf8");
}
