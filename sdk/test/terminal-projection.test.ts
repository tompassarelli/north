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

test("manifested process terminals remain valid", () => {
  const facts = committed([
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "unverified" },
    { predicate: "delivery_reason", value: "provider_terminal_success_without_external_verification" },
  ]);
  expect(terminalProcessOutcome(facts)).toBe("ran");
  expect(terminalDeliveryOutcome(facts)).toBe("unverified");
});

test("outcome-only historical terminals are no longer resolved", () => {
  expect(terminalProcessOutcome([
    { predicate: "outcome", value: "ran" },
  ])).toBeUndefined();
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

test("an unsupported delivery state cannot become a terminal", () => {
  const body: TerminalFact[] = [
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "complete" },
    { predicate: "delivery_reason", value: "unsupported" },
  ];
  expect(terminalProcessOutcome(committed(body))).toBeUndefined();
  expect(terminalDeliveryOutcome(committed(body))).toBeUndefined();
});

test("delivery state must agree with the provider process terminal", () => {
  const diedReported: TerminalFact[] = [
    { predicate: "process_outcome", value: "died" },
    { predicate: "delivery_outcome", value: "reported" },
    { predicate: "delivery_reason", value: "complete_run_scoped_done_bar_evidence_self_reported" },
  ];
  expect(terminalProcessOutcome(committed(diedReported))).toBeUndefined();

  const ranBlocked: TerminalFact[] = [
    { predicate: "process_outcome", value: "ran" },
    { predicate: "delivery_outcome", value: "blocked" },
    { predicate: "delivery_reason", value: "provider_process_died" },
  ];
  expect(terminalProcessOutcome(committed(ranBlocked))).toBeUndefined();
});
