import { expect, test } from "bun:test";
import {
  DeliveryReservationWriterProcessFailure,
  DeliveryEvidenceRetryableError,
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
