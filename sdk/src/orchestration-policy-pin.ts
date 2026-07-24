import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

/**
 * §3.2 digest pin (thread 019f8f5c, Phase 2). When routing reads from the graph
 * (the Phase 2 default), the canonical selection-policy floors live in the graph
 * for query but the versioned validator stays the admission authority. This is
 * the fail-closed consumer of `policy_sha256`: at admission North recomputes the
 * three digests over the canonical selection-rule table and refuses unless all
 * three agree, so a routing floor can move only by a policy version bump, never
 * by a bare graph write.
 *
 *   storedSha256     — the `policy_sha256` fact the importer wrote.
 *   projectionSha256 — recomputed from the live rule subjects; a bare graph
 *                      write to a floor changes this but not the stored fact.
 *   validatorSha256  — enumerated from the canonical validator's baked table; a
 *                      coordinated forge of stored+projection still cannot move
 *                      this without a validator/policy version bump + rebuild.
 *
 * Full shape enforcement (default-deny kind shapes, the dispatch fast path) is
 * Phase 3; Phase 2 delivers only this admission-time digest pin.
 */
export interface PolicyDigestPin {
  policyVersion: string;
  catalogVersion: number;
  storedSha256: string;
  projectionSha256: string;
  validatorSha256: string;
}

const REPO = resolve(import.meta.dir, "..", "..");
const projectorCli = resolve(REPO, "cli/orchestration-project-cli.clj");
const HEX64 = /^[0-9a-f]{64}$/;

function bb(): string {
  return process.env.NORTH_PEER_BB ?? "bb";
}

function port(): string {
  return process.env.NORTH_PORT ?? "7977";
}

function digestField(value: unknown, label: string): string {
  if (typeof value !== "string" || !HEX64.test(value))
    throw new Error(`orchestration policy pin: ${label} is not a sha256 digest`);
  return value;
}

/** Read the three canonical selection-rule digests from the graph projection. */
export function readPolicyDigestPin(): PolicyDigestPin {
  let out: string;
  try {
    out = execFileSync(bb(), [projectorCli, port(), "policy-pin"], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 30_000,
    });
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    // Fail closed: a missing projection, dead coordinator, or absent validator
    // is a refusal to admit, never a silent file fallback.
    throw new Error(
      `orchestration policy pin projection failed on port ${port()} `
      + `(is @catalog:current imported and the canonical validator reachable?): ${detail}`,
    );
  }
  let parsed: Record<string, unknown>;
  try { parsed = JSON.parse(out) as Record<string, unknown>; }
  catch { throw new Error("orchestration policy pin projection returned invalid JSON"); }
  return {
    policyVersion: String(parsed.policyVersion ?? ""),
    catalogVersion: Number(parsed.catalogVersion),
    storedSha256: digestField(parsed.storedSha256, "storedSha256"),
    projectionSha256: digestField(parsed.projectionSha256, "projectionSha256"),
    validatorSha256: digestField(parsed.validatorSha256, "validatorSha256"),
  };
}

/**
 * Verify the three-way digest equality and return the pinned digest, throwing
 * (fail closed) on any mismatch. The messages name the specific breach so an
 * operator sees whether a floor moved by bare write (projection ≠ validator) or
 * a stale/forged pin (stored ≠ validator).
 */
export function verifyPolicyDigestPin(
  pin: PolicyDigestPin = readPolicyDigestPin(),
  surface = "orchestration policy pin",
): string {
  const { storedSha256, projectionSha256, validatorSha256 } = pin;
  if (storedSha256 !== validatorSha256)
    throw new Error(
      `${surface}: graph policy_sha256 (${storedSha256.slice(0, 12)}) does not match the `
      + `canonical validator baked table (${validatorSha256.slice(0, 12)}) — a routing floor `
      + `changed without a policy version bump; admission refused`,
    );
  if (projectionSha256 !== validatorSha256)
    throw new Error(
      `${surface}: live rule projection (${projectionSha256.slice(0, 12)}) diverges from the `
      + `canonical validator (${validatorSha256.slice(0, 12)}) — a bare graph write altered a `
      + `routing floor; admission refused`,
    );
  return validatorSha256;
}
