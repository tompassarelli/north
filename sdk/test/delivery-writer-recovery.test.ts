import { expect, test } from "bun:test";
// Deliberately imports no new symbol: every assertion below is behavioural, so
// reverting the fix fails these tests instead of failing to load the module.
import {
  DeliveryReservationWriterProcessFailure,
  DeliveryEvidenceRetryableError,
  DELIVERY_RESERVATION_WRITER_TIMEOUT_MS,
  deliveryEvidenceWriterError,
  deliveryReservationFailureCause,
  newDeliveryRunContext,
  reserveDeliveryRunWithRecovery,
} from "../src/delivery-evidence";

test("a typed pre-reservation writer process failure is relaunched once", () => {
  const context = newDeliveryRunContext(
    "run-writer-recovery",
    "writer-recovery-thread",
    "writer-recovery-agent",
    "a".repeat(64),
  );
  let attempts = 0;
  let publishedReservations = 0;
  const slept: number[] = [];
  const reservation = reserveDeliveryRunWithRecovery(
    context,
    () => {
      attempts++;
      if (attempts === 1) {
        throw deliveryEvidenceWriterError("reserve", "", {
          run: context.runId,
          reporter: `agent:${context.reporterAgentId}`,
        }, { code: "EPIPE" });
      }
      publishedReservations++;
      return { contractOrigin: "worker-defined", baselineDoneWhen: [] };
    },
    { sleep: (ms) => slept.push(ms) },
  );

  expect(reservation).toEqual({
    contractOrigin: "worker-defined",
    baselineDoneWhen: [],
  });
  expect(attempts).toBe(2);
  expect(publishedReservations).toBe(1);
  expect(slept).toEqual([100]);
});

// The outer subprocess boundary must lose the race against the windows the
// writer itself opens, or a healthy writer is killed before it can refuse.
test("the reservation subprocess boundary outlasts the writer's inner windows", () => {
  const readRetryBudgetMs = 15_000; // delivery-evidence-internal read-retry-budget-ms
  const coordinatorReadMs = 30_000; // coord.clj NORTH_COORD_READ_TIMEOUT_MS
  const publicationDeadlineMs = 60_000; // reserve! reservation-publication-deadline-ms
  const readbackSleepMs = 2_000; // reserve! post-commit readback backoff
  const innerWindows = readRetryBudgetMs + coordinatorReadMs
    + publicationDeadlineMs + readbackSleepMs;
  expect(DELIVERY_RESERVATION_WRITER_TIMEOUT_MS).toBeGreaterThan(2 * innerWindows);
});

test("a coordinator transport death is replayable and relaunched with the same context", () => {
  for (const transport of [
    "coordinator response deadline exceeded",
    "coordinator closed before sending a response line",
    "coordinator closed during a response line",
  ]) {
    const context = newDeliveryRunContext(
      "run-writer-transport",
      "writer-transport-thread",
      "writer-transport-agent",
      "d".repeat(64),
    );
    let attempts = 0;
    const replayedContexts: string[] = [];
    const slept: number[] = [];
    const reservation = reserveDeliveryRunWithRecovery(
      context,
      (attemptContext) => {
        attempts++;
        replayedContexts.push(
          [
            attemptContext.runId, attemptContext.threadId,
            attemptContext.reporterAgentId, attemptContext.capability,
          ].join("|"),
        );
        if (attempts === 1) {
          throw deliveryEvidenceWriterError("reserve", `Message: ${transport}\n`, {
            run: context.runId,
            thread: context.threadId,
            reporter: `agent:${context.reporterAgentId}`,
            capabilitySha256: "e".repeat(64),
          });
        }
        return { contractOrigin: "accepted", baselineDoneWhen: ["bar → ok"] };
      },
      { sleep: (ms) => slept.push(ms) },
    );

    expect(reservation).toEqual({
      contractOrigin: "accepted",
      baselineDoneWhen: ["bar → ok"],
    });
    expect(attempts).toBe(2);
    expect(new Set(replayedContexts).size).toBe(1);
    expect(slept).toEqual([100]);
  }
});

test("a coordinator transport failure is typed, bounded, and named", () => {
  const secret = "live-capability-must-not-leak";
  const failure = deliveryEvidenceWriterError(
    "reserve",
    "Message: coordinator response deadline exceeded\n",
    {
      run: "run-lane-transport",
      thread: "thread-transport",
      reporter: "agent:lane-transport",
      capabilitySha256: secret,
    },
  );
  expect(failure).toBeInstanceOf(DeliveryEvidenceRetryableError);
  expect(failure.name).toBe("DeliveryReservationCoordinatorTransportFailure");
  expect(failure.retryable).toBe(true);
  expect(failure.message).toBe(
    "delivery evidence reserve rejected: run reservation refused:"
    + " run=@run-lane-transport holder=@agent:lane-transport"
    + " receipt=unavailable reason=coordinator-transport-failure"
    + " detail=coordinator response deadline exceeded",
  );
  expect(failure.message).not.toContain(secret);
  expect(deliveryReservationFailureCause(failure)).toBe("coordinator transport failed");
});

// A verdict the writer actually reached is never replayed, however transient it
// reads: replay safety rests on no reservation decision having been made.
test("a reservation verdict stays terminal even under recovery", () => {
  const context = newDeliveryRunContext(
    "run-writer-verdict",
    "writer-verdict-thread",
    "writer-verdict-agent",
    "f".repeat(64),
  );
  for (const stderr of [
    "Message: run subject is not fresh\n",
    "Message: run reservation refused: run=@run-writer-verdict"
    + " holder=@agent:other receipt=unavailable reason=existing-reservation\n",
  ]) {
    const failure = deliveryEvidenceWriterError("reserve", stderr, {
      run: context.runId,
      reporter: `agent:${context.reporterAgentId}`,
    });
    let attempts = 0;
    expect(() => reserveDeliveryRunWithRecovery(
      context,
      () => { attempts++; throw failure; },
      { sleep: () => { throw new Error("terminal verdict slept"); } },
    )).toThrow(failure.message);
    expect(attempts).toBe(1);
  }
});

test("writer timeout and publication deadline are terminal, not replayed", () => {
  const context = newDeliveryRunContext(
    "run-writer-terminal",
    "writer-terminal-thread",
    "writer-terminal-agent",
    "c".repeat(64),
  );
  for (const error of [
    deliveryEvidenceWriterError("reserve", "", {
      run: context.runId,
      reporter: `agent:${context.reporterAgentId}`,
    }, { code: "ETIMEDOUT", signal: "SIGTERM" }),
    new DeliveryEvidenceRetryableError(
      "delivery evidence reserve rejected: delivery evidence publication deadline exceeded",
    ),
  ]) {
    let attempts = 0;
    expect(() => reserveDeliveryRunWithRecovery(
      context,
      () => {
        attempts++;
        throw error;
      },
      { sleep: () => { throw new Error("terminal failure slept"); } },
    )).toThrow(error.message);
    expect(attempts).toBe(1);
  }
});

test("recovery exhaustion rethrows the final writer-process failure and never loops", () => {
  const context = newDeliveryRunContext(
    "run-writer-exhausted",
    "writer-exhausted-thread",
    "writer-exhausted-agent",
    "b".repeat(64),
  );
  let attempts = 0;
  const slept: number[] = [];
  let observed: unknown;
  try {
    reserveDeliveryRunWithRecovery(
      context,
      () => {
        attempts++;
        throw new DeliveryReservationWriterProcessFailure(
          "delivery evidence reserve rejected: run reservation refused: "
          + "receipt=unavailable reason=writer-process-failure",
        );
      },
      { sleep: (ms) => slept.push(ms) },
    );
  } catch (error) {
    observed = error;
  }
  expect(attempts).toBe(2);
  expect(slept).toEqual([100]);
  expect(deliveryReservationFailureCause(observed)).toBe("writer process failed");
});
