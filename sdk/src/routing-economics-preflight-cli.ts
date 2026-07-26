import { admitRoutingRequest } from "./routing-admission";
import { admitRoutingEconomics } from "./routing-economics";

interface PreflightEnvelope {
  routingMetadata?: unknown;
  routingAssessment?: unknown;
  pinEvidence?: unknown;
  provider?: string;
  target?: string;
  model?: string;
}

async function main(): Promise<void> {
  let payload: PreflightEnvelope;
  try {
    payload = JSON.parse(await Bun.stdin.text()) as PreflightEnvelope;
  } catch {
    throw new Error("routing economics preflight expects one JSON object on stdin");
  }
  if (!payload || typeof payload !== "object" || Array.isArray(payload))
    throw new Error("routing economics preflight expects one JSON object on stdin");
  const request = admitRoutingRequest(
    payload.routingMetadata ?? {}, "managed North routing preflight",
  );
  const admitted = admitRoutingEconomics({
    request,
    routingAssessment: payload.routingAssessment,
    pinEvidence: payload.pinEvidence,
    provider: payload.provider,
    target: payload.target,
    model: payload.model,
    surface: "managed North routing preflight",
  });
  process.stdout.write(JSON.stringify(admitted.receipt));
}

/**
 * A refusal is only useful if it NAMES itself. The old catch printed
 * `error.message` and otherwise a bare "routing economics preflight failed" —
 * a string the Clojure caller ALSO emitted as its own empty-output fallback, so
 * two unrelated failures (a non-Error throw here vs. the subprocess dying
 * unheard upstream) were indistinguishable on the terminal, and an Error thrown
 * with an empty message printed a blank line. Every exit through here now
 * carries the underlying rejection text plus the thrown value's type, so the
 * operator never has to guess which layer refused.
 */
export function rejectionText(error: unknown): string {
  if (error instanceof Error) {
    const message = error.message.trim();
    if (message) return message;
    return `routing economics preflight threw ${error.name || "Error"} with no message`;
  }
  const rendered = typeof error === "string" ? error.trim() : safeRender(error);
  return `routing economics preflight threw a non-Error ${typeof error}: `
    + (rendered || "(unrenderable value)");
}

function safeRender(value: unknown): string {
  try { return JSON.stringify(value) ?? String(value); }
  catch { return String(value); }
}

// Entry-point guard so a test can import `rejectionText` and drive the catch
// with the exact throws that used to print nothing useful, without this module
// consuming stdin as a side effect of being imported.
if (import.meta.main) {
  main().catch((error) => {
    console.error(rejectionText(error));
    process.exitCode = 1;
  });
}
