import { expect, test } from "bun:test";
import { commentId, desiredState, parseBrief, parseStatusLine, sourceFieldDiff, type CoreInput, type DesiredIssue, type GitHubIssue, type IssueRef } from "./paperclip-core";

import { topologyState } from "./paperclip-topology";

const node = { id: "lead", role: "lead" as const, domain: "north", state: "active" as const, brief: "/brief", status_file: "/status" };
const snapshot = (ref: IssueRef, changes: Partial<GitHubIssue> = {}): GitHubIssue => ({
  ref, title: "Work", body: "Details", url: `https://example.test/${ref}`, state: "OPEN", labels: [], ...changes,
});

const base = (): CoreInput => ({
  projects: { north: { priority: 1, target: "2026-10-16", repos: ["north"] }, spare: { priority: 2, repos: [] } },
  nodes: [node], briefs: { "/brief": "Goal: finish north#3\n1. N2, north#2: finish it\nBackground: north#8" },
  github: [snapshot("north#1", { subissues: ["north#4"] }), snapshot("north#2"), snapshot("north#3"), snapshot("north#4", { parent: "north#1", subissues: ["north#5"], labels: ["priority:now"] }),
    snapshot("north#5", { parent: "north#4", subissues: ["north#6"] }), snapshot("north#6", { parent: "north#5", subissues: ["north#7"] }),
    snapshot("north#7", { parent: "north#6" }), snapshot("north#8"), snapshot("spare#9", { labels: ["priority:now"] })],
});

const fields = ({ key, ref, leadId, ...rest }: DesiredIssue) => rest;

test("property: repeated reconcile is empty, source changes one field, unchanged source preserves Tom edits", () => {
  let seed = 0x52b2;
  const random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed; };
  for (let i = 0; i < 300; i++) {
    const input = base();
    input.github = input.github.map(g => ({ ...g, title: `title ${random()}`, body: `body ${random()}` }));
    const first = desiredState(input);
    const issue = first.issues.find(g => g.ref === "north#2")!;
    const pushed = fields(issue);
    const remote = { ...pushed, title: `Tom ${random()}`, description: `Tom body ${random()}` };
    const second = desiredState({ ...input, previous: first });
    const desired = fields(second.issues.find(g => g.ref === "north#2")!);
    const patch = sourceFieldDiff(desired, pushed);
    expect(patch).toEqual({});
    expect({ ...remote, ...patch }).toEqual(remote);
    for (const project of second.projects) expect(sourceFieldDiff(project, first.projects.find(p => p.key === project.key)!)).toEqual({});
    input.github = input.github.map(g => g.ref === "north#2" ? { ...g, title: `changed ${random()}` } : g);
    const changed = fields(desiredState({ ...input, previous: first }).issues.find(g => g.ref === "north#2")!);
    const changedPatch = sourceFieldDiff(changed, pushed);
    expect(changedPatch).toEqual({ title: changed.title });
    const applied = { ...remote, ...changedPatch };
    expect(applied.title).toBe(changed.title);
    expect(applied.description).toBe(remote.description);
    expect(sourceFieldDiff(changed, { ...pushed, ...changedPatch })).toEqual({});
  }
});

test("recorded mapping scenario: key set, lifecycle, hierarchy, no unrelated brief refs, retained missing snapshots", () => {
  const input = base();
  const first = desiredState(input);
  expect(first.trackedRefs).toEqual(["north#1", "north#2", "north#3", "north#4", "north#5", "north#6", "north#7"]);
  expect(first.projects.map(p => [p.key, p.status, p.targetDate])).toEqual([
    ["north:project:north", "in_progress", "2026-10-16"], ["north:project:spare", "planned", null],
  ]);
  expect(first.issues.filter(i => i.ref).map(i => [i.ref, i.priority, i.status, i.parentKey])).toEqual([
    ["north#1", "medium", "todo", undefined], ["north#2", "high", "in_progress", undefined],
    ["north#3", "high", "in_progress", undefined], ["north#4", "medium", "todo", "gh:north#1"],
    ["north#5", "medium", "todo", "gh:north#4"], ["north#6", "medium", "todo", "gh:north#5"],
    ["north#7", "medium", "todo", "gh:north#6"],
  ]);
  const next = desiredState({ ...input, previous: first, nodes: [], projects: { north: { priority: 1, repos: ["north"] } },
    github: input.github.filter(i => i.ref !== "north#3").map(i => ({ ...i, labels: [],
      ...(i.ref === "north#2" ? { state: "CLOSED" as const, stateReason: "NOT_PLANNED" as const } : {}),
      ...(i.ref === "north#4" ? { state: "CLOSED" as const, stateReason: "COMPLETED" as const } : {}),
    })) });
  expect(next.issues.find(i => i.ref === "north#2")!.status).toBe("cancelled");
  expect(next.issues.find(i => i.ref === "north#4")!.status).toBe("done");
  expect(next.issues.find(i => i.ref === "north#3")).toEqual(first.issues.find(i => i.ref === "north#3"));
  expect(next.trackedRefs).toEqual(["north#1", "north#3", "north#5", "north#6", "north#7"]);
  expect(next.issues.find(i => i.leadId === "lead")!.status).toBe("done");
  for (const reason of ["COMPLETED", "NOT_PLANNED"] as const) {
    const closed = desiredState({ ...input, github: input.github.map(g => g.ref === "north#1" ? { ...g, state: "CLOSED", stateReason: reason } : g), finishedLeadIds: ["lead"] });
    expect(closed.issues.find(i => i.ref === "north#1")!.status).toBe(reason === "COMPLETED" ? "done" : "cancelled");
    expect(closed.issues.find(i => i.leadId)!.status).toBe("done");
  }
  const cyclic = desiredState({ ...input, github: input.github.map(g => g.ref === "north#7" ? { ...g, subissues: ["north#4", "north#9"] } : g) });
  expect(cyclic.trackedRefs).toEqual([...first.trackedRefs, "north#9"]);
  expect(cyclic.issues.find(i => i.ref === "north#4")!.parentKey).toBe("gh:north#1");
  expect(parseBrief(input.briefs["/brief"]).refs).toEqual(["north#3", "north#2"]);
  const overlayInput = { ...input, briefs: { "/brief": "Goal: epic north#1\n1. N4, north#4: descendant" }, statuses: {} };
  const overlay = topologyState(desiredState(overlayInput), overlayInput).desired;
  expect(overlay.issues.find(issue => issue.ref === "north#4")!.parentKey).toBe("gh:north#1");
  expect(overlay.issues.find(issue => issue.ref === "north#1")!.parentKey).toBe("north:lead:lead");
  expect(overlay.issues.filter(issue => issue.ref).every(issue => issue.assigneeNodeId === "lead")).toBe(true);
  const replay = topologyState(desiredState({ ...overlayInput, previous: overlay }), { ...overlayInput, previous: overlay }).desired;
  expect(replay).toEqual(overlay);
  const status = `DONE SOC221: Restore concise box titles; ${"native status detail ".repeat(20)}`.trim();
  const boxInput = { ...input, briefs: {}, statuses: { "/status": status } };
  const boxState = topologyState(desiredState(boxInput), boxInput).desired;
  const box = boxState.issues.find(issue => issue.boxId === "soc221")!;
  expect(box.title.startsWith("soc221: Restore concise box titles;")).toBe(true);
  expect(box.title.slice("soc221: ".length).length).toBeLessThanOrEqual(80);
  expect(box.description).toBe(status);
  const legacy = { ...boxState, issues: boxState.issues.map(issue => issue.key === box.key ? { ...issue, title: status } : issue) };
  const reconcile = (nodes = boxInput.nodes) => topologyState(desiredState({ ...boxInput, nodes, previous: legacy }), { ...boxInput, nodes, statuses: {}, previous: legacy }).desired;
  for (const nodes of [boxInput.nodes, []]) {
    const retained = reconcile(nodes).issues.find(issue => issue.key === box.key)!;
    expect(retained.title).toBe(box.title);
    expect(retained.description).toBe(status);
    expect(retained.key).toBe(box.key);
  }
  const restrictedLegacy = { ...legacy, issues: legacy.issues.map(issue => issue.key === box.key ? { ...issue, projectKey: "north:project:wisp", assigneeNodeId: "old-wisp" } : issue) };
  const restricted = topologyState(desiredState({ ...boxInput, nodes: [], previous: restrictedLegacy }), { ...boxInput, nodes: [], statuses: {}, previous: restrictedLegacy }).desired.issues.find(issue => issue.key === box.key)!;
  expect([restricted.title, restricted.description]).toEqual(["soc221", ""]);
  const replayBoxes = topologyState(desiredState({ ...boxInput, previous: boxState }), { ...boxInput, previous: boxState }).desired;
  expect(replayBoxes).toEqual(boxState);
  const launchInput: CoreInput = {
    projects: { smashcraft: { priority: 1, repos: ["smashcraft", "south"] } },
    nodes: [{ ...node, id: "launch", domain: "smashcraft", provider: "claude" }, { ...node, id: "sub", domain: "smashcraft", parent: "launch", role: "sub-lead", provider: "codex", boxes: [{ boxId: "epic", ref: "smashcraft#1" }] }],
    briefs: {}, additionalRefs: ["smashcraft#1", "south#3"], fullBodyRefs: ["smashcraft#1", "south#3"],
    github: [snapshot("smashcraft#1", { body: "Epic body".repeat(200), subissues: ["smashcraft#2"] }), snapshot("smashcraft#2", { parent: "smashcraft#1", body: "Held child body" }), snapshot("south#3", { body: "Private body".repeat(200) })],
  };
  const launch = topologyState(desiredState(launchInput), { ...launchInput, statuses: {} }).desired;
  expect(launch.issues.find(issue => issue.ref === "smashcraft#1")!.description).toBe(`${launchInput.github[0].url}\n\n${launchInput.github[0].body}`);
  expect(launch.issues.find(issue => issue.ref === "smashcraft#2")!.description).toBe(launchInput.github[1].url);
  expect(launch.issues.find(issue => issue.ref === "south#3")!.description).toBe(`${launchInput.github[2].url}\n\n${launchInput.github[2].body}`);
  expect(launch.issues.filter(issue => issue.ref).map(issue => [issue.ref, issue.assigneeNodeId, issue.parentKey])).toEqual([
    ["smashcraft#1", "launch", "north:lead:launch"], ["smashcraft#2", "launch", "gh:smashcraft#1"], ["south#3", "launch", "north:lead:launch"],
  ]);
});

const brief = "Goal: finish north#59\n1. N59c, north#59: isolation\n2. N61b, north#61: controller";
const shapes = [
  ["LANDED 7.3b abcdef1234567890 as abcdef1 (auto-land: rebased)", "LANDED", "north:lead:lead"],
  ["LANDED 7.11 abcdef1234567890 as abcdef1 (workspace passed)", "LANDED", "north:lead:lead"],
  ["LANDED 7.12 abcdef1234567890 as abcdef1 (resolved rebase conflict)", "LANDED", "north:lead:lead"],
  ["DONE 2026-10-13T09:30:55+08:00 P12 recovery: exact abcdef123 CI123 SUCCESS", "DONE", "north:lead:lead"],
  ["DONE 8.10: unlanded REVIEW already posted at abcdef1234567890", "DONE", "north:lead:lead"],
  ["QUESTION 00:24 wisp#125: may the lead staff box 3?", "QUESTION", "gh:wisp#125"],
  ["QUESTION 03:35 wisp#125: continue box 3 with one bundle?", "QUESTION", "gh:wisp#125"],
  ["QUESTION 16:46 smashcraft#546: which contact gate?", "QUESTION", "gh:smashcraft#546"],
  ["QUESTION N61b recovery design: please rule selector before integration", "QUESTION", "gh:north#61"],
  ["FINISHED smashcraft-lead", "FINISHED", "north:lead:lead"],
] as const;

test("external reference: ten authored status shapes observed in 2026-10-13 lead handoffs", () => {
  const tracked: IssueRef[] = ["north#59", "north#61", "wisp#125", "smashcraft#546"];
  for (const [line, kind, target] of shapes) {
    const result = parseStatusLine("lead", line, tracked, brief)!;
    expect([result.kind, result.targetKey, result.body]).toEqual([kind, target, line]);
    expect(result.finishedLead).toBe(kind === "FINISHED");
    expect(result.presentation.tone).toBe(kind === "LANDED" ? "success" : kind === "QUESTION" ? "warning" : "info");
    expect(result.clientRequestId).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-5[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    expect(parseStatusLine("lead", line, tracked, brief)!.clientRequestId).toBe(result.clientRequestId);
    expect(commentId("other", line)).not.toBe(result.clientRequestId);
  }
  expect(commentId("lead", "FINISHED smashcraft-lead")).toBe("233daf3d-62f9-5958-9541-211bbeb8ad52");
  expect(parseStatusLine("lead", "LANDED N59c abcdef1", tracked, brief)!.targetKey).toBe("gh:north#59");
  expect(parseStatusLine("lead", "DONE other#7 then north#61 north#59", tracked, brief)!.targetKey).toBe("gh:north#61");
  expect(parseStatusLine("lead", "QUESTION N59c", [], brief)!.targetKey).toBe("north:lead:lead");
  expect(parseStatusLine("lead", "REVIEW north#59 abcdef1", tracked, brief)).toBeNull();
});
