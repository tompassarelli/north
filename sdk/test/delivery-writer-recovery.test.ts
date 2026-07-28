import { expect, test } from "bun:test";
import { execFileSync } from "node:child_process";
import {
  DeliveryEvidenceRetryableError,
  deliveryEvidenceWriterError,
  deliveryReservationFailureCause,
  newDeliveryRunContext,
  reserveDeliveryRunWithRecovery,
} from "../src/delivery-evidence";

test("a writer killed before reservation is relaunched once and publishes exactly once", () => {
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
        try {
          execFileSync("bash", ["-c", "kill -KILL $$"], { stdio: "ignore" });
        } catch (error) {
          throw deliveryEvidenceWriterError("reserve", "", {
            run: context.runId,
            reporter: `agent:${context.reporterAgentId}`,
          }, error);
        }
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
        throw new DeliveryEvidenceRetryableError(
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
