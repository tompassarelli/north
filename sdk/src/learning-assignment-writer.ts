import { execFile } from "node:child_process";
import { resolve } from "node:path";
import {
  learningAssignmentFacts, type LearningAssignment,
} from "./learning-regime";

const REPO = resolve(import.meta.dir, "../..");
const WRITER = resolve(REPO, "cli/learning-assignment-internal.clj");

export const LEARNING_ASSIGNMENT_WRITE_TIMEOUT_MS = 30_000;

export type LearningAssignmentPublicationStatus = "recorded";

/**
 * Publish the episode assignment before any provider selection or construction.
 * Failure is deliberately throwing: executing without the durable assignment
 * would destroy randomization integrity and permit a retry to move arms.
 */
export function publishLearningAssignment(
  runId: string,
  assignment: LearningAssignment,
  timeoutMs = LEARNING_ASSIGNMENT_WRITE_TIMEOUT_MS,
): Promise<LearningAssignmentPublicationStatus> {
  const facts = learningAssignmentFacts(assignment);
  return new Promise((resolvePublication, rejectPublication) => {
    try {
      execFile("bb", [
        WRITER,
        process.env.NORTH_PORT ?? "7977",
        runId,
        JSON.stringify(facts),
      ], { timeout: Math.max(1, Math.floor(timeoutMs)) }, (error, _stdout, stderr) => {
        if (!error) {
          resolvePublication("recorded");
          return;
        }
        const cause = String(stderr ?? "").trim() || error.message;
        rejectPublication(new Error(
          `learning assignment publication failed before provider execution: ${cause}`,
          { cause: error },
        ));
      });
    } catch (error) {
      rejectPublication(new Error(
        `learning assignment writer unavailable before provider execution: ${(error as Error).message}`,
        { cause: error },
      ));
    }
  });
}
