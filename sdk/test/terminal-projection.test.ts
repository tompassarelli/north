import { expect, test } from "bun:test";
import {
  assessThreadDelivery, RUN_BAR_EVIDENCE_VERSION, sha256,
} from "../src/delivery-verification";
import {
  terminalDeliveryOutcome,
  laneResolvedByFacts,
  terminalManifestSha256,
  terminalProcessOutcome,
  type TerminalFact,
} from "../src/terminal-projection";

function committed(facts: TerminalFact[]): TerminalFact[] {
  return [...facts, {
    predicate: "terminal_manifest_sha256",
    value: terminalManifestSha256(facts)!,
  }];
}

test("old four-field unverified terminals remain valid", () => {
  const facts = committed([
    { predicate: "outcome", value: "ran" },
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "unverified" },
    { predicate: "delivery_reason", value: "provider_terminal_success_without_external_verification" },
  ]);
  expect(terminalProcessOutcome(facts)).toBe("ran");
  expect(terminalDeliveryOutcome(facts)).toBe("unverified");
});

test("reported terminal is accepted only with digest-bound evidence", () => {
  const assessment = assessThreadDelivery("thread", "worker", [
    { predicate: "done_when", value: "tests pass" },
  ], [
    { predicate: "done_when", value: "tests pass" },
  ], "run-worker", [{
    version: RUN_BAR_EVIDENCE_VERSION,
    run: "@run-worker",
    thread: "@thread",
    reporter: "@agent:worker",
    bar: "tests pass",
    observed: "exit 0",
    recordedAt: "2026-07-18T10:00:00.000Z",
  }]);
  if (assessment.deliveryOutcome !== "reported") throw new Error("expected reported");
  const body: TerminalFact[] = [
    { predicate: "outcome", value: "ran" },
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "reported" },
    { predicate: "delivery_reason", value: assessment.deliveryReason },
    { predicate: "delivery_evidence", value: assessment.proof.deliveryEvidence },
    { predicate: "delivery_evidence_sha256", value: assessment.proof.deliveryEvidenceSha256 },
  ];
  expect(terminalDeliveryOutcome(committed(body))).toBe("reported");
  expect(laneResolvedByFacts(committed(body), [])).toBe(true);

  const forgedBody = body.map((fact) => fact.predicate === "delivery_evidence_sha256"
    ? { ...fact, value: sha256("different") }
    : fact);
  expect(terminalManifestSha256(forgedBody)).toBeDefined();
  expect(terminalDeliveryOutcome(committed(forgedBody))).toBeUndefined();
  expect(laneResolvedByFacts(committed(forgedBody), [])).toBe(false);
});

test("status text cannot manufacture verified without an attestation", () => {
  const body: TerminalFact[] = [
    { predicate: "outcome", value: "ran" },
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "verified" },
    { predicate: "delivery_reason", value: "independent_managed_verifier_attested" },
  ];
  expect(terminalProcessOutcome(committed(body))).toBeUndefined();
  expect(terminalDeliveryOutcome(committed(body))).toBeUndefined();
});

test("delivery state must agree with the provider process terminal", () => {
  const diedReported: TerminalFact[] = [
    { predicate: "outcome", value: "died" },
    { predicate: "process_outcome", value: "died" },
    { predicate: "delivery_outcome", value: "reported" },
    { predicate: "delivery_reason", value: "complete_run_scoped_done_bar_evidence_self_reported" },
  ];
  expect(terminalProcessOutcome(committed(diedReported))).toBeUndefined();

  const ranBlocked: TerminalFact[] = [
    { predicate: "outcome", value: "ran" },
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "blocked" },
    { predicate: "delivery_reason", value: "provider_process_died" },
  ];
  expect(terminalProcessOutcome(committed(ranBlocked))).toBeUndefined();
});
