import { expect, test } from "bun:test";
import {
  chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  DeliveryEvidenceProofTransportFailure, DeliveryReservationWriterProcessFailure,
  deliveryEvidenceWriterError, deliveryReservationFailureCause, deliveryRunEnvironment,
  deliveryWriterInvocation, loadDeliveryRunState, newDeliveryRunContext,
  parseEvidenceRecordArgv, recordRunBarEvidence, recordUnreservedBarEvidence,
  resolveDeliveryRunState, resolveThreadFacts, RUN_RESERVATION_VERSION,
  runReservationValid, DELIVERY_EVIDENCE_WRITER_TIMEOUT_MS,
  DELIVERY_RESERVATION_WRITER_TIMEOUT_MS,
} from "../src/delivery-evidence";
import { MANAGED_NORTH_MCP_ENV_KEYS } from "../src/execution-admission";
import { harnessOptions } from "../src/harness";
import {
  canonicalEvidenceText, MAX_DELIVERY_BARS,
  MAX_RUN_BAR_EVIDENCE_RECORD_UTF8_BYTES, MAX_UNRESERVED_BAR_UTF8_BYTES,
  parseRunBarEvidence, RUN_BAR_EVIDENCE_VERSION, sha256,
  UNRESERVED_BAR_EVIDENCE_VERSION, validateUnreservedBarEvidence,
} from "../src/delivery-verification";

const conformance = JSON.parse(readFileSync(
  new URL("./fixtures/delivery-conformance.json", import.meta.url),
  "utf8",
)) as {
  reservationBody: Array<[string, string]>;
  reservationManifestSha256: string;
};

test("delivery run context is explicit and never mutates ambient process env", () => {
  const before = {
    run: process.env.NORTH_RUN_ID,
    thread: process.env.NORTH_THREAD_ID,
    capability: process.env.NORTH_RUN_CAPABILITY,
  };
  const context = newDeliveryRunContext(
    "run-lane-123",
    "thread-123",
    "lane-123",
    "a".repeat(64),
  );
  expect(deliveryRunEnvironment(context)).toEqual({
    NORTH_RUN_ID: "run-lane-123",
    NORTH_THREAD_ID: "thread-123",
    NORTH_RUN_CAPABILITY: "a".repeat(64),
  });
  const options = harnessOptions({
    self: "lane-123",
    deliveryRun: context,
    presenceRegistrar: false,
    presenceRenewer: false,
  }) as any;
  expect(options.env.NORTH_RUN_ID).toBe("run-lane-123");
  expect(options.env.NORTH_THREAD_ID).toBe("thread-123");
  expect(options.env.NORTH_RUN_CAPABILITY).toBe("a".repeat(64));
  expect(options.mcpServers.north.env.NORTH_RUN_ID).toBe("run-lane-123");
  expect(options.mcpServers.north.env.NORTH_THREAD_ID).toBe("thread-123");
  expect(options.mcpServers.north.env.NORTH_RUN_CAPABILITY).toBe("a".repeat(64));
  const withoutReservation = harnessOptions({
    self: "lane-without-reservation",
    presenceRegistrar: false,
    presenceRenewer: false,
  }) as any;
  for (const key of ["NORTH_RUN_ID", "NORTH_THREAD_ID", "NORTH_RUN_CAPABILITY"]) {
    expect(withoutReservation.env[key]).toBeUndefined();
    expect(withoutReservation.mcpServers.north.env[key]).toBeUndefined();
  }
  expect({
    run: process.env.NORTH_RUN_ID,
    thread: process.env.NORTH_THREAD_ID,
    capability: process.env.NORTH_RUN_CAPABILITY,
  }).toEqual(before);
});

test("writer failures never echo the live capability in diagnostics", () => {
  const capability = "b".repeat(64);
  expect(() => recordRunBarEvidence("tests pass", "exit 0", {
    AGENT_ID: "lane-123",
    NORTH_RUN_ID: "run-lane-123",
    NORTH_THREAD_ID: "thread-123",
    NORTH_RUN_CAPABILITY: capability,
    NORTH_PORT: "1",
  })).toThrow("delivery evidence record rejected");
  try {
    recordRunBarEvidence("tests pass", "exit 0", {
      AGENT_ID: "lane-123",
      NORTH_RUN_ID: "run-lane-123",
      NORTH_THREAD_ID: "thread-123",
      NORTH_RUN_CAPABILITY: capability,
      NORTH_PORT: "1",
    });
  } catch (error) {
    expect(String(error)).not.toContain(capability);
  }
});

test("proof transport failure is distinct and never asks for task repetition", () => {
  const error = deliveryEvidenceWriterError(
    "record",
    "ExceptionInfo: transport\nMessage: PROOF_TRANSPORT_FAILURE:"
      + " run-bound proof publication was not acknowledged;"
      + " do not repeat the task\n",
  );
  expect(error).toBeInstanceOf(DeliveryEvidenceProofTransportFailure);
  expect(error.name).toBe("DeliveryEvidenceProofTransportFailure");
  expect(error.retryable).toBe(false);
  expect(error.message).toContain("proof publication was not acknowledged");
  expect(error.message).toContain("task result remains valid");
  expect(error.message).toContain("must not be repeated");
  const processFailure = deliveryEvidenceWriterError(
    "record", "", {}, { code: "ETIMEDOUT" },
  );
  expect(processFailure).toBeInstanceOf(DeliveryEvidenceProofTransportFailure);
  expect(processFailure.retryable).toBe(false);
  expect(DELIVERY_EVIDENCE_WRITER_TIMEOUT_MS).toBe(45_000);
});

test("reservation failure diagnostics expose only bounded semantic causes", () => {
  const secret = "live-capability-must-not-leak";
  expect(deliveryReservationFailureCause(new Error(
    `delivery evidence reserve rejected: delivery evidence publication deadline exceeded ${secret}`,
  ))).toBe("publication deadline exceeded");
  expect(deliveryReservationFailureCause(new Error(
    `delivery evidence reserve rejected: run reservation projection changed before commit ${secret}`,
  ))).toBe("reservation conflict");
  expect(deliveryReservationFailureCause(new Error(
    `delivery evidence reserve rejected: coordinator did not answer a delivery evidence read ${secret}`,
  ))).toBe("coordinator read unavailable");
  expect(deliveryReservationFailureCause(new Error(
    `unclassified writer failure ${secret}`,
  ))).toBe("writer rejected reservation");
  const timedOut = deliveryEvidenceWriterError("reserve", "", {
    run: "run-lane-123",
    thread: "thread-123",
    reporter: "agent:lane-123",
    capabilitySha256: secret,
  }, { code: "ETIMEDOUT", signal: "SIGTERM" });
  expect(timedOut.message).toBe(
    "delivery evidence reserve rejected: run reservation refused:"
    + " run=@run-lane-123 holder=@agent:lane-123"
    + " receipt=unavailable reason=writer-timeout",
  );
  expect(timedOut.message).not.toContain(secret);
  expect(timedOut.name).toBe("DeliveryEvidenceRetryableError");
  expect(timedOut.retryable).toBe(true);
  expect(deliveryReservationFailureCause(timedOut)).toBe("writer timed out");
  const publicationDeadline = deliveryEvidenceWriterError(
    "reserve",
    "Message: delivery evidence publication deadline exceeded after 5000ms\n",
    {
      run: "run-lane-789",
      thread: "thread-123",
      reporter: "agent:lane-789",
      capabilitySha256: secret,
    },
  );
  expect(publicationDeadline.name).toBe("DeliveryEvidenceRetryableError");
  expect(publicationDeadline.retryable).toBe(true);
  expect(deliveryReservationFailureCause(publicationDeadline))
    .toBe("publication deadline exceeded");
  expect(publicationDeadline.message).not.toContain(secret);
  const writerProcessFailure = deliveryEvidenceWriterError(
    "reserve",
    "",
    {
      run: "run-lane-123",
      thread: "thread-123",
      reporter: "agent:lane-123",
      capabilitySha256: secret,
    },
    { code: "EPIPE" },
  );
  expect(writerProcessFailure).toBeInstanceOf(
    DeliveryReservationWriterProcessFailure,
  );
  expect(writerProcessFailure.message).not.toContain(secret);
  expect(deliveryReservationFailureCause(writerProcessFailure))
    .toBe("writer process failed");
  expect(deliveryEvidenceWriterError("reserve", "Message: run subject is not fresh", {
    run: "run-lane-456",
    reporter: "agent:lane-456",
  }).message).toBe(
    "delivery evidence reserve rejected: run reservation refused:"
    + " run=@run-lane-456 holder=@agent:lane-456 receipt=unavailable"
    + " reason=writer-refusal detail=run subject is not fresh",
  );
  // The outer boundary must lose the race against the writer's inner windows
  // (15s read budget + 30s socket read + 30s publication + readback).
  expect(DELIVERY_RESERVATION_WRITER_TIMEOUT_MS).toBe(180_000);
  expect(DELIVERY_RESERVATION_WRITER_TIMEOUT_MS).toBeGreaterThan(
    2 * (15_000 + 30_000 + 30_000 + 2_000),
  );
});

test("live run capabilities travel on stdin and never enter writer argv", () => {
  const capability = "c".repeat(64);
  const invocation = deliveryWriterInvocation("record", {
    run: "run-lane-123",
    thread: "thread-123",
    reporter: "agent:lane-123",
    capability,
    bar: "tests pass",
    observed: "exit 0",
  }, "7977");
  expect(invocation.argv).toHaveLength(3);
  expect(invocation.argv.join("\0")).not.toContain(capability);
  expect(invocation.stdin).toContain(capability);
});

test("managed MCP environment explicitly carries all run evidence bindings", () => {
  for (const key of ["NORTH_RUN_ID", "NORTH_THREAD_ID", "NORTH_RUN_CAPABILITY"]) {
    expect((MANAGED_NORTH_MCP_ENV_KEYS as readonly string[]).includes(key)).toBe(true);
  }
});

test("evidence loading requires one digest-committed reservation projection", () => {
  const body = conformance.reservationBody;
  expect(body.find(([predicate]) => predicate === "run_reservation_version")?.[1])
    .toBe(RUN_RESERVATION_VERSION);
  const marker = sha256(body.map(([predicate, value]) =>
    `${predicate}\0${value}\n`).join(""));
  expect(marker).toBe(conformance.reservationManifestSha256);
  const facts = [
    ...body.map(([predicate, value]) => ({ predicate, value })),
    { predicate: "run_reservation_manifest_sha256", value: marker },
  ];
  expect(runReservationValid(facts)).toBe(true);
  const workerDefinedBody = body.map(([predicate, value]) => [
    predicate,
    predicate === "run_reservation_contract_origin"
      ? "worker-defined"
      : predicate === "run_reservation_done_when" ? "[]" : value,
  ] as [string, string]);
  const workerDefinedMarker = sha256(workerDefinedBody.map(([predicate, value]) =>
    `${predicate}\0${value}\n`).join(""));
  expect(runReservationValid([
    ...workerDefinedBody.map(([predicate, value]) => ({ predicate, value })),
    {
      predicate: "run_reservation_manifest_sha256",
      value: workerDefinedMarker,
    },
  ])).toBe(true);
  expect(runReservationValid([
    ...facts,
    { predicate: "run_reservation_agent", value: "@agent:competing-lane" },
  ])).toBe(false);
  expect(runReservationValid(facts.map((fact) =>
    fact.predicate === "run_reservation_contract_origin"
      ? { ...fact, value: "worker-defined" }
      : fact))).toBe(false);
  expect(runReservationValid(facts.map((fact) =>
    fact.predicate === "run_reservation_done_when"
      ? { ...fact, value: "[\" tests pass \"]" }
      : fact))).toBe(false);
  expect(runReservationValid(facts.map((fact) =>
    fact.predicate === "run_reserved_at"
      ? { ...fact, value: "2026-01-01T24:00:00Z" }
      : fact))).toBe(false);
});

test("evidence loading invalidates the entire malformed, cross-scoped, duplicate, or over-cap set", () => {
  const body = conformance.reservationBody;
  const marker = sha256(body.map(([predicate, value]) =>
    `${predicate}\0${value}\n`).join(""));
  const reservation = [
    ...body.map(([predicate, value]) => ({ predicate, value })),
    { predicate: "run_reservation_manifest_sha256", value: marker },
  ];
  const record = {
    bar: "smoke → old run",
    observed: "exit 0",
    recordedAt: "2026-07-18T10:00:00Z",
    reporter: "@agent:lane-123",
    run: "@run-load-state",
    thread: "@thread-123",
    version: RUN_BAR_EVIDENCE_VERSION,
  };
  const load = (evidenceValues: string[]) => {
    const dir = mkdtempSync(join(tmpdir(), "north-delivery-state-"));
    const command = join(dir, "facts");
    const facts = [
      ...reservation,
      ...evidenceValues.map((value) => ({ predicate: "run_bar_evidence", value })),
    ];
    writeFileSync(
      command,
      `#!/usr/bin/env node\nrequire("node:fs").writeFileSync(1, ${JSON.stringify(JSON.stringify(facts))});\n`,
    );
    chmodSync(command, 0o700);
    try {
      return loadDeliveryRunState("run-load-state", command);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  };
  expect(load([JSON.stringify(record)])).toEqual({
    reservationValid: true,
    evidence: [record],
  });
  expect(load(["{"])).toEqual({ reservationValid: false, evidence: [] });
  expect(load([JSON.stringify({ ...record, reporter: "@agent:other" })]))
    .toEqual({ reservationValid: false, evidence: [] });
  expect(load([
    JSON.stringify(record),
    JSON.stringify({ ...record, recordedAt: "2026-07-18T10:00:01Z" }),
  ])).toEqual({ reservationValid: false, evidence: [] });
  expect(load(Array.from(
    { length: MAX_DELIVERY_BARS + 1 },
    (_, index) => JSON.stringify({ ...record, bar: `probe-${index}` }),
  ))).toEqual({ reservationValid: false, evidence: [] });
  expect(load([" ".repeat(MAX_RUN_BAR_EVIDENCE_RECORD_UTF8_BYTES + 1)]))
    .toEqual({ reservationValid: false, evidence: [] });
});

// Thread 019f9cc1: a reader that never spoke used to be indistinguishable from
// a reader that spoke and found no valid reservation. Lanes ms1awg94 and
// ms1b7syb finalized delivery=unverified against reservations that are still
// provably valid on the graph, because `north json show` (2.5-3.5s idle) blew
// the old 5s ceiling while the coordinator was busy.
test("run-state loading separates a failed reader from an invalid reservation", () => {
  const script = (body: string) => {
    const dir = mkdtempSync(join(tmpdir(), "north-delivery-load-"));
    const command = join(dir, "facts");
    writeFileSync(command, `#!/usr/bin/env node\n${body}\n`);
    chmodSync(command, 0o700);
    try {
      return loadDeliveryRunState("run-load-failure", command, 750);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  };
  // Reader hangs past the per-attempt ceiling: transient, NOT a verdict.
  const timedOut = script('require("node:fs").readFileSync("/dev/stdin");setInterval(()=>{},1000);');
  expect(timedOut.reservationValid).toBe(false);
  expect(timedOut.loadFailure).toBe("reader timed out");
  // Reader fails outright: transient.
  expect(script("process.exit(3);").loadFailure).toBe("reader exited 3");
  // Reader emits something that is not a fact list: transient.
  expect(script('require("node:fs").writeFileSync(1,"coordinator busy");').loadFailure)
    .toBe("reader payload unparseable");
  expect(script('require("node:fs").writeFileSync(1,"{}");').loadFailure)
    .toBe("reader payload not a fact list");
  // Reader spoke and the facts carry no valid reservation: a CONTENT verdict,
  // which must stay fail-closed with no loadFailure marker.
  expect(script('require("node:fs").writeFileSync(1,"[]");'))
    .toEqual({ reservationValid: false, evidence: [] });
});

test("run-state resolution retries only the load and never a content verdict", () => {
  const invalid = { reservationValid: false, evidence: [] };
  const valid = { reservationValid: true, evidence: [] };
  const options = { attempts: 3, backoffMs: 10, budgetMs: 10_000 };

  // A content verdict is final on the first attempt: fail-closed posture intact.
  let calls = 0;
  const verdict = resolveDeliveryRunState("run-x", () => { calls++; return invalid; }, options);
  expect([verdict.attempts, verdict.transientFailure, calls]).toEqual([1, undefined, 1]);

  // A transient failure that clears is delivery, not a misclassification.
  calls = 0;
  const slept: number[] = [];
  const recovered = resolveDeliveryRunState("run-x", () => (++calls < 3
    ? { ...invalid, loadFailure: "reader timed out" }
    : valid), { ...options, sleep: (ms) => slept.push(ms) });
  expect(recovered).toEqual({ state: valid, attempts: 3 });
  expect(slept).toEqual([10, 20]);

  // A thrown load is transient too, and exhaustion names the cause.
  calls = 0;
  const exhausted = resolveDeliveryRunState("run-x", () => {
    calls++;
    throw new Error("torn rotated-run predicate row");
  }, { ...options, sleep: () => {} });
  expect(calls).toBe(3);
  expect(exhausted.attempts).toBe(3);
  expect(exhausted.transientFailure).toBe("torn rotated-run predicate row");

  // The retry window is bounded: a budget that cannot fit the next backoff stops.
  calls = 0;
  let clock = 0;
  const budgeted = resolveDeliveryRunState("run-x", () => {
    calls++;
    clock += 400;
    return { ...invalid, loadFailure: "reader timed out" };
  }, { attempts: 9, backoffMs: 100, budgetMs: 700, now: () => clock, sleep: () => {} });
  expect(calls).toBe(2);
  expect(budgeted.transientFailure).toBe("reader timed out");
});

test("evidence record argv carries an explicit unreserved thread", () => {
  expect(parseEvidenceRecordArgv(["record", "bar", "observed"]))
    .toEqual({ bar: "bar", observed: "observed", thread: undefined });
  expect(parseEvidenceRecordArgv(["record", "--thread", "t-1", "bar", "observed"]))
    .toEqual({ bar: "bar", observed: "observed", thread: "t-1" });
  expect(parseEvidenceRecordArgv(["record", "--thread=t-1", "bar", "observed"]))
    .toEqual({ bar: "bar", observed: "observed", thread: "t-1" });
  // A bar that looks like a flag is still a bar; a repeated or empty --thread,
  // a missing operand, and an extra positional are all refusals, never guesses.
  expect(parseEvidenceRecordArgv(["record", "--thread", "a", "--thread", "b", "x", "y"]))
    .toBeUndefined();
  expect(parseEvidenceRecordArgv(["record", "--thread"])).toBeUndefined();
  expect(parseEvidenceRecordArgv(["record", "bar"])).toBeUndefined();
  expect(parseEvidenceRecordArgv(["record", "bar", "observed", "extra"])).toBeUndefined();
  expect(parseEvidenceRecordArgv(["reserve", "bar", "observed"])).toBeUndefined();
});

test("unreserved acknowledgements can never be read as run-bound evidence", () => {
  const unreserved = {
    version: UNRESERVED_BAR_EVIDENCE_VERSION,
    scope: "unreserved",
    thread: "@thread-123",
    bar: "tests pass",
    observed: "exit 0",
    recordedAt: "2026-07-18T10:00:00Z",
    superseded: 0,
  };
  expect(validateUnreservedBarEvidence(unreserved)).toEqual(unreserved);
  // Neither validator accepts the other's shape: an unreserved record carries no
  // run, and a run record carries no unreserved scope.
  expect(parseRunBarEvidence(JSON.stringify(unreserved))).toBeUndefined();
  expect(validateUnreservedBarEvidence({
    version: RUN_BAR_EVIDENCE_VERSION,
    run: "@run-1", thread: "@thread-123", reporter: "@agent:lane-1",
    bar: "tests pass", observed: "exit 0", recordedAt: "2026-07-18T10:00:00Z",
  })).toBeUndefined();
  // Fabricating a run binding onto an unreserved payload is still refused.
  expect(validateUnreservedBarEvidence({ ...unreserved, run: "@run-1" })).toBeUndefined();
  expect(validateUnreservedBarEvidence({ ...unreserved, scope: "run" })).toBeUndefined();
  expect(validateUnreservedBarEvidence({ ...unreserved, superseded: -1 })).toBeUndefined();
  expect(validateUnreservedBarEvidence({ ...unreserved, thread: "not-an-entity" }))
    .toBeUndefined();
});

test("unreserved evidence accepts the long bars a reservation would reject", () => {
  const longBar = "b".repeat(MAX_UNRESERVED_BAR_UTF8_BYTES);
  expect(validateUnreservedBarEvidence({
    version: UNRESERVED_BAR_EVIDENCE_VERSION,
    scope: "unreserved",
    thread: "@thread-123",
    bar: longBar,
    observed: "exit 0",
    recordedAt: "2026-07-18T10:00:00Z",
    superseded: 1,
  })?.bar).toBe(longBar);
  expect(validateUnreservedBarEvidence({
    version: UNRESERVED_BAR_EVIDENCE_VERSION,
    scope: "unreserved",
    thread: "@thread-123",
    bar: `${longBar}b`,
    observed: "exit 0",
    recordedAt: "2026-07-18T10:00:00Z",
    superseded: 0,
  })).toBeUndefined();
});

test("unreserved recording refuses an invalid thread before any subprocess", () => {
  expect(() => recordUnreservedBarEvidence("thread with spaces", "bar", "exit 0", {}))
    .toThrow("invalid delivery thread id");
});

test("braces are ordinary content in bars and observations", () => {
  const observed = "{\"exit\":0,\"failures\":[]} · {nested {braces}}";
  expect(canonicalEvidenceText(observed)).toBe(observed);
  const record = {
    version: RUN_BAR_EVIDENCE_VERSION,
    run: "@run-1",
    thread: "@thread-123",
    reporter: "@agent:lane-1",
    bar: "Probe: bun test. Expected: {\"pass\": true}",
    observed,
    recordedAt: "2026-07-18T10:00:00Z",
  };
  expect(parseRunBarEvidence(JSON.stringify(record))).toEqual(record);
});

// Thread 019f9e0d: the deferred sibling of 019f9cc1 — the THREAD load at
// finalize has the same transient-read exposure as the reservation load and
// was not retried. `resolveThreadFacts` mirrors `resolveDeliveryRunState`
// exactly: a thrown load retries, a returned (even empty) content result is
// final on the first attempt.
test("thread-facts resolution retries only a load that never spoke, never a content result", () => {
  const options = { attempts: 3, backoffMs: 10, budgetMs: 10_000 };

  // A content read is final on the first attempt, even genuinely empty
  // (absent thread) — fail-closed posture for that case belongs to
  // assessThreadDelivery, not the loader.
  let calls = 0;
  const empty = resolveThreadFacts("thread-x", () => { calls++; return []; }, options);
  expect([empty.facts, empty.attempts, empty.transientFailure, calls]).toEqual([[], 1, undefined, 1]);

  // A transient failure that clears is delivery, not a misclassification.
  calls = 0;
  const slept: number[] = [];
  const facts = [{ predicate: "title", value: "recovered" }];
  const recovered = resolveThreadFacts("thread-x", () => {
    calls++;
    if (calls < 3) throw new Error("reader timed out");
    return facts;
  }, { ...options, sleep: (ms) => slept.push(ms) });
  expect(recovered).toEqual({ facts, attempts: 3 });
  expect(slept).toEqual([10, 20]);

  // Exhaustion names the cause and carries no facts.
  calls = 0;
  const exhausted = resolveThreadFacts("thread-x", () => {
    calls++;
    throw new Error("torn thread fact row");
  }, { ...options, sleep: () => {} });
  expect(calls).toBe(3);
  expect(exhausted).toEqual({ attempts: 3, transientFailure: "torn thread fact row" });

  // The retry window is bounded, same as the reservation loader.
  calls = 0;
  let clock = 0;
  const budgeted = resolveThreadFacts("thread-x", () => {
    calls++;
    clock += 400;
    throw new Error("reader timed out");
  }, { attempts: 9, backoffMs: 100, budgetMs: 700, now: () => clock, sleep: () => {} });
  expect(calls).toBe(2);
  expect(budgeted.transientFailure).toBe("reader timed out");
});
