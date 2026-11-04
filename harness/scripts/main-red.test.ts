import { test, expect } from "bun:test";
import { decide, revertRequest, type Commit, type Situation } from "./main-red";

test("GitHub workflow-run confirmation protocol (REST actions/workflow-jobs, kind 4)", () => {
  const job = { id: 7, name: "test", conclusion: "failure", steps: [{ name: "Test", number: 2, conclusion: "failure" }] };
  const culprit: Commit = { sha: "culprit", parents: ["parent"], message: "land", files: ["src/main.rs"] };
  const recorded: Situation = { attempt: 1, conclusion: "failure", original: [job], confirmed: [], previous: { sha: "parent", conclusion: "success" }, commits: [culprit], onMain: true, head: "culprit", tip: "culprit" };
  const confirmation = { ...recorded, attempt: 2, confirmed: [job] }, done = { ...job, conclusion: "cancelled", steps: [] }, only = (commit: Partial<Commit>) => ({ ...confirmation, commits: [{ ...culprit, ...commit }] });
  expect(decide(recorded).kind).toBe("confirm");
  expect(decide(confirmation)).toMatchObject({ kind: "request", shas: ["culprit"], parent: "parent", job: 7 }); expect(revertRequest("culprit", decide(confirmation), ["test suite::x ... FAILED"], "https://example/runs/9")).toBe("Main red at culprit\n\ntest: Test\ntest suite::x ... FAILED\n\nGreen-parent: parent\nMain-red-run: https://example/runs/9");
  for (const [status, conclusion, said] of [["completed", "success", "passed"], ["in_progress", null, "is still in_progress"], ["queued", null, "is still queued"]] as const) expect(decide({ ...confirmation, tip: "6b0baa426", successor: { status, conclusion } })).toMatchObject({ kind: "report", reason: expect.stringContaining(`6b0baa426, whose run ${said}`) });
  expect(decide({ ...confirmation, head: "tip", tip: "tip", commits: [{ sha: "tip", parents: ["culprit"], message: "second", files: ["src/lib.rs"] }, culprit] })).toMatchObject({ kind: "request", shas: ["tip", "culprit"], parent: "parent" });
  for (const s of [{ ...recorded, conclusion: "cancelled", original: [done] }, { ...recorded, conclusion: "cancelled" }, { ...confirmation, conclusion: "cancelled", confirmed: [done] }, { ...confirmation, conclusion: "cancelled" }]) expect(decide(s).kind).toBe("none");
  for (const s of [{ ...recorded, tip: "fix-forward" }, { ...confirmation, tip: "fix-forward" }, { ...confirmation, previous: { sha: "unrelated", conclusion: "success" } }, { ...confirmation, previous: { sha: "parent", conclusion: "failure" } }, { ...confirmation, attempt: 3 }, { ...confirmation, conclusion: "success", confirmed: [{ ...job, conclusion: "success", steps: [] }] }, { ...confirmation, confirmed: [{ ...job, steps: [{ name: "Other", number: 2, conclusion: "failure" }] }] }, only({ files: [".github/workflows/tests.yml"] }), only({ message: "Revert bad landing\n\nSheriff-Reverts: prior" }), { ...confirmation, onMain: false }]) expect(decide(s).kind).toBe("report");
});
