import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import {
  assessThreadDelivery,
  canonicalEvidenceText,
  DELIVERY_ATTESTATION_AUTHORITY,
  DELIVERY_ATTESTATION_VERSION,
  MAX_DELIVERY_BARS,
  MAX_DELIVERY_AGENT_ID_UTF8_BYTES,
  MAX_DELIVERY_ATTESTATION_UTF8_BYTES,
  MAX_DELIVERY_BAR_UTF8_BYTES,
  MAX_DELIVERY_ENVELOPE_UTF8_BYTES,
  MAX_DELIVERY_OBSERVED_UTF8_BYTES,
  MAX_DELIVERY_RUN_ID_UTF8_BYTES,
  MAX_DELIVERY_THREAD_ID_UTF8_BYTES,
  MAX_DELIVERY_WRITER_REQUEST_UTF8_BYTES,
  MAX_RUN_BAR_EVIDENCE_RECORD_UTF8_BYTES,
  MAX_RUN_RESERVATION_BASELINE_UTF8_BYTES,
  deliveryProofValid,
  parseDeliveryAttestation,
  parseDeliveryEvidence,
  parseRunBarEvidence,
  RUN_BAR_EVIDENCE_VERSION,
  sha256,
  validateRunBarEvidence,
  validAgentEntity,
  validInstant,
  validRunEntity,
  validThreadEntity,
  type RunBarEvidence,
} from "../src/delivery-verification";

const at = "2026-07-18T10:00:00.000Z";
const conformance = JSON.parse(readFileSync(
  new URL("./fixtures/delivery-conformance.json", import.meta.url),
  "utf8",
)) as {
  validInstants: string[];
  invalidInstants: string[];
  limits: {
    maxBars: number;
    maxBarUtf8Bytes: number;
    maxObservedUtf8Bytes: number;
    maxEnvelopeUtf8Bytes: number;
    maxRecordUtf8Bytes: number;
    maxReservationBaselineUtf8Bytes: number;
    maxWriterRequestUtf8Bytes: number;
    maxThreadIdUtf8Bytes: number;
    maxRunIdUtf8Bytes: number;
    maxAgentIdUtf8Bytes: number;
    maxAttestationUtf8Bytes: number;
  };
  textCases: Array<{ name: string; raw: string; canonical: string | null }>;
  validThreadEntities: string[];
  invalidThreadEntities: string[];
};

function evidence(
  run: string,
  reporter: string,
  bar = "tests pass",
  observed = "24/24, exit 0",
  thread = "thread-1",
): RunBarEvidence {
  return {
    bar,
    observed,
    recordedAt: at,
    reporter: `@agent:${reporter}`,
    run: `@${run}`,
    thread: `@${thread}`,
    version: RUN_BAR_EVIDENCE_VERSION,
  };
}

test("strict instant grammar rejects calendar normalization and matches java.time semantics", () => {
  for (const valid of conformance.validInstants) expect(validInstant(valid)).toBe(true);
  for (const invalid of conformance.invalidInstants) expect(validInstant(invalid)).toBe(false);
});

test("evidence limits are fixture-bound and count UTF-8 bytes, not code points", () => {
  expect({
    maxBars: MAX_DELIVERY_BARS,
    maxBarUtf8Bytes: MAX_DELIVERY_BAR_UTF8_BYTES,
    maxObservedUtf8Bytes: MAX_DELIVERY_OBSERVED_UTF8_BYTES,
    maxEnvelopeUtf8Bytes: MAX_DELIVERY_ENVELOPE_UTF8_BYTES,
    maxRecordUtf8Bytes: MAX_RUN_BAR_EVIDENCE_RECORD_UTF8_BYTES,
    maxReservationBaselineUtf8Bytes: MAX_RUN_RESERVATION_BASELINE_UTF8_BYTES,
    maxWriterRequestUtf8Bytes: MAX_DELIVERY_WRITER_REQUEST_UTF8_BYTES,
    maxThreadIdUtf8Bytes: MAX_DELIVERY_THREAD_ID_UTF8_BYTES,
    maxRunIdUtf8Bytes: MAX_DELIVERY_RUN_ID_UTF8_BYTES,
    maxAgentIdUtf8Bytes: MAX_DELIVERY_AGENT_ID_UTF8_BYTES,
    maxAttestationUtf8Bytes: MAX_DELIVERY_ATTESTATION_UTF8_BYTES,
  }).toEqual(conformance.limits);
  expect(validateRunBarEvidence({
    ...evidence("run-a", "worker"),
    bar: "🧪".repeat(MAX_DELIVERY_BAR_UTF8_BYTES / 4),
    observed: "o".repeat(MAX_DELIVERY_OBSERVED_UTF8_BYTES),
  })).toBeDefined();
  expect(validateRunBarEvidence({
    ...evidence("run-a", "worker"),
    bar: "🧪".repeat(MAX_DELIVERY_BAR_UTF8_BYTES / 4 + 1),
  })).toBeUndefined();
  expect(validateRunBarEvidence({
    ...evidence("run-a", "worker"),
    observed: "o".repeat(MAX_DELIVERY_OBSERVED_UTF8_BYTES + 1),
  })).toBeUndefined();
  const recordRaw = JSON.stringify(evidence("run-a", "worker"));
  const exactRecord = recordRaw + " ".repeat(
    MAX_RUN_BAR_EVIDENCE_RECORD_UTF8_BYTES - Buffer.byteLength(recordRaw, "utf8"),
  );
  expect(Buffer.byteLength(exactRecord, "utf8"))
    .toBe(MAX_RUN_BAR_EVIDENCE_RECORD_UTF8_BYTES);
  expect(parseRunBarEvidence(exactRecord)).toBeDefined();
  expect(parseRunBarEvidence(`${exactRecord} `)).toBeUndefined();
});

test("proof text and entity grammar are cross-runtime canonical", () => {
  for (const { raw, canonical } of conformance.textCases) {
    expect(canonicalEvidenceText(raw)).toBe(canonical ?? undefined);
  }
  for (const value of conformance.validThreadEntities) {
    expect(validThreadEntity(value)).toBe(true);
  }
  for (const value of conformance.invalidThreadEntities) {
    expect(validThreadEntity(value)).toBe(false);
  }
  expect(validRunEntity(`@run-${"a".repeat(507)}`)).toBe(true);
  expect(validRunEntity(`@run-${"a".repeat(508)}`)).toBe(false);
  expect(validAgentEntity(`@agent:${"a".repeat(249)}`)).toBe(true);
  expect(validAgentEntity(`@agent:${"a".repeat(250)}`)).toBe(false);
  expect(validThreadEntity(`@${"a".repeat(511)}`)).toBe(true);
  expect(validThreadEntity(`@${"a".repeat(512)}`)).toBe(false);
});

test("missing, bar-less, and incomplete run-scoped evidence fail closed", () => {
  expect(assessThreadDelivery("thread-1", "worker", [])).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_thread_unavailable_at_finalize",
  });
  expect(assessThreadDelivery("thread-1", "worker", [
    { predicate: "title", value: "bounded work" },
  ], [{ predicate: "title", value: "bounded work" }])).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_contract_missing",
  });
  expect(assessThreadDelivery(
    "thread-1",
    "worker",
    [{ predicate: "done_when", value: "tests pass" }],
    [{ predicate: "done_when", value: "tests pass" }],
    "run-worker",
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_bar_evidence_incomplete",
  });
});

test("accepted bars are immutable and worker-defined bars are explicit", () => {
  const accepted = [{ predicate: "done_when", value: "tests pass" }];
  expect(assessThreadDelivery(
    "thread-1", "worker", accepted, accepted, "run-worker",
    [evidence("run-worker", "worker")],
  ).deliveryOutcome).toBe("reported");

  expect(assessThreadDelivery(
    "thread-1",
    "worker",
    [{ predicate: "done_when", value: "trivial replacement" }],
    accepted,
    "run-worker",
    [evidence("run-worker", "worker", "trivial replacement")],
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_contract_changed_during_run",
  });

  const workerDefined = assessThreadDelivery(
    "thread-1",
    "worker",
    [{ predicate: "title", value: "work" }, { predicate: "done_when", value: "tests pass" }],
    [{ predicate: "title", value: "work" }],
    "run-worker",
    [evidence("run-worker", "worker")],
  );
  if (workerDefined.deliveryOutcome !== "reported") throw new Error("expected reported");
  expect(JSON.parse(workerDefined.proof.deliveryEvidence).contractOrigin).toBe("worker-defined");
});

test("run and reporter scope prevent concurrent lanes from cross-crediting evidence", () => {
  const bars = [{ predicate: "done_when", value: "tests pass" }];
  const forA = evidence("run-a", "worker-a");
  expect(assessThreadDelivery(
    "thread-1", "worker-a", bars, bars, "run-a", [forA],
  ).deliveryOutcome).toBe("reported");
  expect(assessThreadDelivery(
    "thread-1", "worker-b", bars, bars, "run-b", [forA],
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_bar_evidence_ambiguous",
  });
});

test("multiple records for one run/bar fail closed instead of creating parser-invalid reported proof", () => {
  const bars = [
    { predicate: "title", value: "bounded work" },
    { predicate: "done_when", value: "tests pass" },
  ];
  const first = evidence("run-a", "worker-a");
  const second = { ...first, recordedAt: "2026-07-18T10:00:01.000Z" };
  expect(assessThreadDelivery(
    "thread-1", "worker-a", bars, bars, "run-a", [first, second],
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_bar_evidence_ambiguous",
  });
});

test("invalid controls, duplicate-collapsing contract floods, and raw envelopes fail closed", () => {
  const bars = Array.from(
    { length: MAX_DELIVERY_BARS },
    (_, index) => `probe ${index.toString().padStart(2, "0")}`,
  );
  const facts = [
    { predicate: "title", value: "bounded proof" },
    ...bars.map((value) => ({ predicate: "done_when", value })),
  ];
  const records = bars.map((bar) =>
    evidence(
      "run-bounded", "worker", bar,
      "\u0001".repeat(MAX_DELIVERY_OBSERVED_UTF8_BYTES),
    ));
  expect(assessThreadDelivery(
    "thread-1", "worker", facts, facts, "run-bounded", records,
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_bar_evidence_ambiguous",
  });
  const collapsing = Array.from(
    { length: MAX_DELIVERY_BARS + 1 },
    () => ({ predicate: "done_when", value: " tests pass " }),
  );
  expect(assessThreadDelivery(
    "thread-1", "worker", collapsing, collapsing, "run-bounded", [],
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_contract_exceeds_evidence_limits",
  });
  expect(parseDeliveryEvidence(
    " ".repeat(MAX_DELIVERY_ENVELOPE_UTF8_BYTES + 1),
  )).toBeUndefined();
});

test("32 exact records round-trip while the 33rd invalidates the whole set", () => {
  const bars = Array.from(
    { length: MAX_DELIVERY_BARS },
    (_, index) => `probe ${index.toString().padStart(2, "0")}`,
  );
  const facts = bars.map((value) => ({ predicate: "done_when", value }));
  const records = bars.map((bar) =>
    evidence("run-bounded", "worker", bar, `probe ${bar} exit 0`));
  const assessment = assessThreadDelivery(
    "thread-1", "worker", facts, facts, "run-bounded", records,
  );
  if (assessment.deliveryOutcome !== "reported") throw new Error("expected reported");
  expect(parseDeliveryEvidence(assessment.proof.deliveryEvidence)).toBeDefined();
  expect(Buffer.byteLength(assessment.proof.deliveryEvidence, "utf8"))
    .toBeLessThanOrEqual(MAX_DELIVERY_ENVELOPE_UTF8_BYTES);
  expect(assessThreadDelivery(
    "thread-1", "worker", facts, facts, "run-bounded",
    [...records, evidence("run-bounded", "worker", "outside contract")],
  )).toEqual({
    deliveryOutcome: "unverified",
    deliveryReason: "delivery_bar_evidence_ambiguous",
  });
});

test("attestation parser bounds raw bytes and every authority identifier", () => {
  const raw = JSON.stringify({
    actor: "@agent:verifier",
    attestedAt: at,
    authority: DELIVERY_ATTESTATION_AUTHORITY,
    evidenceSha256: "a".repeat(64),
    role: "verifier",
    run: "@run-worker",
    target: "@agent:worker",
    thread: "@thread-1",
    version: DELIVERY_ATTESTATION_VERSION,
  });
  expect(parseDeliveryAttestation(raw)).toBeDefined();
  const exact = raw + " ".repeat(MAX_DELIVERY_ATTESTATION_UTF8_BYTES
    - Buffer.byteLength(raw, "utf8"));
  expect(Buffer.byteLength(exact, "utf8")).toBe(MAX_DELIVERY_ATTESTATION_UTF8_BYTES);
  expect(parseDeliveryAttestation(exact)).toBeDefined();
  expect(parseDeliveryAttestation(`${exact} `)).toBeUndefined();
  expect(parseDeliveryAttestation(raw.replace("@thread-1", "@thread 1")))
    .toBeUndefined();
});

test("v2 snapshot contains only mechanically bound proof fields", () => {
  const baseline = [
    { predicate: "done_when", value: "tests pass" },
    { predicate: "bar_evidence", value: "tests pass → old run" },
  ];
  const current = [
    ...baseline,
    { predicate: "outcome", value: "narrative thread outcome" },
  ];
  const assessment = assessThreadDelivery(
    "thread-1", "worker", current, baseline, "run-worker",
    [evidence("run-worker", "worker")],
  );
  if (assessment.deliveryOutcome !== "reported") throw new Error("expected reported");
  const snapshot = JSON.parse(assessment.proof.deliveryEvidence);
  expect(Object.keys(snapshot).sort()).toEqual([
    "baselineDoneWhen", "contractOrigin", "doneWhen", "matches",
    "reporter", "run", "thread", "version",
  ]);
  expect(snapshot.contractOrigin).toBe("accepted");
  expect(snapshot).not.toHaveProperty("baselineEvidence");
  expect(snapshot).not.toHaveProperty("baselineEvidenceSha256");
  expect(snapshot).not.toHaveProperty("capturedAt");
  expect(snapshot).not.toHaveProperty("threadOutcome");
  expect(parseDeliveryEvidence(assessment.proof.deliveryEvidence)).toBeDefined();
  const exactEnvelope = assessment.proof.deliveryEvidence + " ".repeat(
    MAX_DELIVERY_ENVELOPE_UTF8_BYTES
      - Buffer.byteLength(assessment.proof.deliveryEvidence, "utf8"),
  );
  expect(Buffer.byteLength(exactEnvelope, "utf8"))
    .toBe(MAX_DELIVERY_ENVELOPE_UTF8_BYTES);
  expect(parseDeliveryEvidence(exactEnvelope)).toBeDefined();
  expect(parseDeliveryEvidence(`${exactEnvelope} `)).toBeUndefined();
  expect(deliveryProofValid(
    assessment.deliveryOutcome, assessment.deliveryReason, assessment.proof,
  )).toBe(true);

  snapshot.threadOutcome = "fabricated narrative";
  const forged = JSON.stringify(snapshot);
  expect(parseDeliveryEvidence(forged)).toBeUndefined();
  expect(deliveryProofValid("reported", assessment.deliveryReason, {
    deliveryEvidence: forged,
    deliveryEvidenceSha256: sha256(forged),
  })).toBe(false);
});

test("shared-UID attestation never upgrades reported evidence to verified", () => {
  const bars = [{ predicate: "done_when", value: "tests pass" }];
  const assessment = assessThreadDelivery(
    "thread-1", "worker", bars, bars, "run-worker",
    [evidence("run-worker", "worker")],
  );
  if (assessment.deliveryOutcome !== "reported") throw new Error("expected reported");
  expect(deliveryProofValid(
    "verified",
    "independent_managed_verifier_attested",
    assessment.proof,
  )).toBe(false);
});
