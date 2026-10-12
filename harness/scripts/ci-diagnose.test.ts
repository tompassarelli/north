import { test, expect } from "bun:test";
import { readFileSync } from "node:fs";
import { diagnose, type Io, type Run } from "./ci-diagnose";

const log = readFileSync(`${import.meta.dir}/fixtures/firn-run-38133514013.log`, "utf8");
const run = (id: number, conclusion: string, headSha: string): Run => ({ databaseId: id, workflowName: "Tests", conclusion, status: "completed", event: "push", headSha, url: `https://github.com/tompassarelli/firn/actions/runs/${id}`, createdAt: "" });

test("recorded firn run 38133514013 failed log through both stages posts one diagnosis (kind 4)", () => {
  const repo = "tompassarelli/firn";
  const step = "Tests (discovered): Discovered shell tests";
  const error = "system-policy-native: line 126 failed: jq -e '.hookSpecificOutput.hookEventName == \"PreToolUse\" and (.hookSpecificOutput.additionalContext | type) == \"string\"' \"$scratch/inject.out\" > /dev/null";
  const cause = "The inject hook no longer emits additionalContext as a string.";
  let stage1 = "```json\n" + JSON.stringify({ step, errors: [error, "##[error]native/system_policy_native.test.sh failed"], culprit: "native/system_policy_native.test.sh:126" }) + "\n```";
  const comments: string[] = [];
  const prompts: string[] = [];
  const io: Io = {
    issues: () => [{ number: 7, title: "main is red" }],
    comments: () => comments,
    runs: () => [run(38133514013, "failure", "bad"), run(38133004576, "success", "good"), { ...run(38133600000, "failure", "x"), workflowName: "Main is red" }],
    log: (_, id) => { expect(id).toBe(38133514013); return log; },
    compare: (_, base, head) => { expect([base, head]).toEqual(["good", "bad"]); return { files: [{ filename: "native/system_policy.bjs", additions: 4, deletions: 2 }] }; },
    claude: (model, prompt) => { prompts.push(prompt); return model === "haiku" ? stage1 : cause; },
    post: (r, n, body) => { expect([r, n]).toEqual([repo, 7]); comments.push(body); },
  };
  expect(diagnose(io, [repo])).toHaveLength(1);
  expect(comments).toHaveLength(1);
  for (const part of [step, error, cause, "<!-- ci-diagnose:38133514013 -->"]) expect(comments[0]).toContain(part);
  expect(prompts[1]).toContain("native/system_policy.bjs +4 -2");
  expect(diagnose(io, [repo])).toHaveLength(0);
  expect(comments).toHaveLength(1);
  comments.length = 0;
  stage1 = JSON.stringify({ step: "Invented step", errors: [error], culprit: null });
  expect(diagnose(io, [repo])).toHaveLength(0);
  stage1 = "The failing step is the shell tests.";
  expect(diagnose(io, [repo])).toHaveLength(0);
  expect(comments).toHaveLength(0);
});
