import { createHash } from "node:crypto";
import {
  deliveryProofValid,
  type DeliveryProof,
} from "./delivery-verification";

export interface TerminalFact {
  predicate: string;
  value: string;
}

const TERMINAL_PREDICATES = [
  "outcome",
  "process_outcome",
  "delivery_outcome",
  "delivery_reason",
] as const;
const DELIVERY_PROOF_PREDICATES = [
  "delivery_evidence",
  "delivery_evidence_sha256",
  "delivery_attestation",
  "delivery_attestation_sha256",
] as const;
const TERMINAL_PROJECTION_PREDICATES = [
  ...TERMINAL_PREDICATES,
  ...DELIVERY_PROOF_PREDICATES,
] as const;

const valuesOf = (facts: readonly TerminalFact[], predicate: string): string[] =>
  facts.filter((fact) => fact.predicate === predicate).map((fact) => fact.value);

const singletonValue = (
  facts: readonly TerminalFact[],
  predicate: string,
): string | undefined => {
  const values = valuesOf(facts, predicate);
  return values.length === 1 && values[0]?.trim() ? values[0] : undefined;
};

const factPresent = (facts: readonly TerminalFact[], predicate: string): boolean =>
  valuesOf(facts, predicate).length > 0;

const lexical = (left: string, right: string): number =>
  left < right ? -1 : left > right ? 1 : 0;

export function terminalManifestSha256(
  facts: readonly TerminalFact[],
): string | undefined {
  const required = TERMINAL_PREDICATES
    .map((predicate) => [predicate, singletonValue(facts, predicate)] as const);
  if (required.some(([, value]) => value === undefined)) return undefined;
  if (TERMINAL_PROJECTION_PREDICATES.some(
    (predicate) => valuesOf(facts, predicate).length > 1,
  )) return undefined;
  const projection: Array<readonly [string, string]> = [];
  for (const predicate of TERMINAL_PROJECTION_PREDICATES) {
    const value = singletonValue(facts, predicate);
    if (value !== undefined) projection.push([predicate, value]);
  }
  const canonical = projection
    .sort(([left], [right]) => lexical(left, right))
    .map(([predicate, value]) => `${predicate}\0${value}\n`)
    .join("");
  return createHash("sha256").update(canonical, "utf8").digest("hex");
}

export function terminalDeliveryProof(
  facts: readonly TerminalFact[],
): DeliveryProof | undefined {
  const deliveryEvidence = singletonValue(facts, "delivery_evidence");
  const deliveryEvidenceSha256 = singletonValue(facts, "delivery_evidence_sha256");
  const deliveryAttestation = singletonValue(facts, "delivery_attestation");
  const deliveryAttestationSha256 = singletonValue(facts, "delivery_attestation_sha256");
  if (!deliveryEvidence && !deliveryEvidenceSha256
    && !deliveryAttestation && !deliveryAttestationSha256) return undefined;
  if (!deliveryEvidence || !deliveryEvidenceSha256) return undefined;
  return {
    deliveryEvidence,
    deliveryEvidenceSha256,
    ...(deliveryAttestation ? { deliveryAttestation } : {}),
    ...(deliveryAttestationSha256 ? { deliveryAttestationSha256 } : {}),
  };
}

export function terminalDeliveryProjectionValid(
  facts: readonly TerminalFact[],
): boolean {
  const outcome = singletonValue(facts, "delivery_outcome");
  const reason = singletonValue(facts, "delivery_reason");
  const process = singletonValue(facts, "process_outcome");
  if (!outcome || !reason || !process) return false;
  if (process === "ran" ? outcome === "blocked" : outcome !== "blocked") return false;
  const proofFactCount = DELIVERY_PROOF_PREDICATES
    .filter((predicate) => factPresent(facts, predicate)).length;
  const proof = terminalDeliveryProof(facts);
  if (proofFactCount > 0 && !proof) return false;
  return deliveryProofValid(outcome, reason, proof);
}

export function terminalManifestValid(
  facts: readonly TerminalFact[],
): boolean {
  const process = singletonValue(facts, "process_outcome");
  const legacyAlias = singletonValue(facts, "outcome");
  const marker = singletonValue(facts, "terminal_manifest_sha256");
  const expected = terminalManifestSha256(facts);
  return Boolean(process && legacyAlias && process === legacyAlias
    && marker && expected && marker === expected
    && terminalDeliveryProjectionValid(facts));
}

export function terminalProcessOutcome(
  facts: readonly TerminalFact[],
): string | undefined {
  if (factPresent(facts, "process_outcome")) {
    return terminalManifestValid(facts)
      ? singletonValue(facts, "process_outcome")?.trim()
      : undefined;
  }
  return singletonValue(facts, "outcome")?.trim();
}

export function terminalDeliveryOutcome(
  facts: readonly TerminalFact[],
): string | undefined {
  return terminalManifestValid(facts)
    ? singletonValue(facts, "delivery_outcome")?.trim()
    : undefined;
}

export function committedRunProcessOutcome(
  facts: readonly TerminalFact[],
): string | undefined {
  if (singletonValue(facts, "kind") !== "run") return undefined;
  return factPresent(facts, "process_outcome")
    ? singletonValue(facts, "process_outcome")?.trim()
    : singletonValue(facts, "outcome")?.trim();
}

export function laneResolvedByFacts(
  laneFacts: readonly TerminalFact[],
  taggedRuns: readonly (readonly TerminalFact[])[],
): boolean {
  return Boolean(
    terminalProcessOutcome(laneFacts)
    || taggedRuns.some((facts) => committedRunProcessOutcome(facts)),
  );
}
