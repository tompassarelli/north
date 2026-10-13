import { expect, test } from "bun:test";
import { labelledSnapshot } from "./paperclip-labelled";

test("labelled snapshots preserve private closure text and enforce source authorization", () => {
  const projects = { work: { priority: 1, repos: ["private-work", "smashcraft"] } };
  const sources = { "private-work": ["launch"] };
  const issue = { ref: "private-work#2", title: "Launch", body: "Private fixture text", url: "https://github.com/example/private-work/issues/2", state: "CLOSED", stateReason: "COMPLETED", labels: ["launch"], parent: "smashcraft#50", subissues: [] };
  expect(labelledSnapshot(JSON.parse(JSON.stringify([issue])), projects, sources, [])).toEqual([issue]);
  expect(() => labelledSnapshot([{ ...issue, labels: ["other"] }], projects, sources, [])).toThrow("outside labelled source scope");
  expect(() => labelledSnapshot([{ ...issue, ref: "other#2" }], projects, sources, [])).toThrow("outside labelled source scope");
  expect(() => labelledSnapshot([issue, issue], projects, sources, [])).toThrow("outside labelled source scope");
  expect(() => labelledSnapshot([{ ...issue, state: "UNKNOWN" }], projects, sources, [])).toThrow("Invalid labelled GitHub issue");
  const restricted = { ...issue, ref: "smashcraft#50" };
  expect(() => labelledSnapshot([restricted], projects, { smashcraft: ["launch"] }, [])).toThrow("Restricted body");
  expect(labelledSnapshot([restricted], projects, { smashcraft: ["launch"] }, ["smashcraft#50"])[0].body).toBe(issue.body);
  expect(labelledSnapshot([{ ...restricted, body: "" }], projects, { smashcraft: ["launch"] }, [])[0].body).toBe("");
});
