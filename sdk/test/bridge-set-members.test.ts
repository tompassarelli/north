import { expect, test } from "bun:test";
import {
  "config-activation-of-json" as activationOfJson,
  "config-set-inspection-text!" as setInspectionText,
  "config-view-rows" as configViewRows,
} from "../src/bridge/generated/north/bridge/app.js";

export type Unit = {
  id: string;
  kind: "set" | "skill" | "hook";
  title: string;
  triggerDescription: string;
  permission: string;
  active: boolean;
  owner: string;
  members: string[];
  supports: string[];
  distributions: unknown[];
  activationPaths: string[][];
};

export const units: Unit[] = [
  { id: "workspace", kind: "set", title: "Workspace", triggerDescription: "work together", permission: "on", active: true, owner: "north", members: ["orchestration", "planning"], supports: [], distributions: [], activationPaths: [["workspace"]] },
  { id: "orchestration", kind: "set", title: "Orchestration", triggerDescription: "delegate work", permission: "on", active: true, owner: "north", members: ["coordination", "review"], supports: [], distributions: [], activationPaths: [["workspace", "orchestration"]] },
  { id: "planning", kind: "set", title: "Planning", triggerDescription: "plan work", permission: "on", active: true, owner: "north", members: ["coordination"], supports: [], distributions: [], activationPaths: [["workspace", "planning"]] },
  { id: "coordination", kind: "set", title: "Coordination", triggerDescription: "coordinate agents", permission: "on", active: true, owner: "north", members: ["messages", "threads", "assignments"], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "coordination"], ["workspace", "planning", "coordination"]] },
  { id: "operations", kind: "set", title: "Operations", triggerDescription: "operate North", permission: "off", active: false, owner: "north", members: ["messages"], supports: [], distributions: [], activationPaths: [] },
  { id: "messages", kind: "skill", title: "Messages", triggerDescription: "send a message", permission: "on", active: true, owner: "north", members: [], supports: [], distributions: ["codex"], activationPaths: [["workspace", "orchestration", "coordination", "messages"], ["workspace", "planning", "coordination", "messages"]] },
  { id: "threads", kind: "skill", title: "Threads", triggerDescription: "manage threads", permission: "on", active: true, owner: "north", members: [], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "coordination", "threads"], ["workspace", "planning", "coordination", "threads"]] },
  { id: "assignments", kind: "skill", title: "Assignments", triggerDescription: "assign work", permission: "on", active: true, owner: "north", members: [], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "coordination", "assignments"], ["workspace", "planning", "coordination", "assignments"]] },
  { id: "review", kind: "skill", title: "Review", triggerDescription: "review work", permission: "on", active: true, owner: "north", members: [], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "review"]] },
  { id: "worktree-guard", kind: "hook", title: "Worktree guard", triggerDescription: "before writes", permission: "on", active: true, owner: "firn", members: [], supports: ["repo-safety", "orchestration"], distributions: ["codex", "claude"], activationPaths: [["orchestration", "worktree-guard"], ["repo-safety", "worktree-guard"]] },
];

export function resolved(overrides: Partial<{ schema: string }> = {}) {
  return JSON.stringify({
    schema: "north.agent-activation/v1",
    catalogDigest: "sha256:test",
    generationId: "gen-test",
    units,
    ...overrides,
  });
}

test("resolved generation preserves ordered recursive set edges and provenance", () => {
  const activation = activationOfJson(resolved());
  const rows = configViewRows(activation.units, "all");
  expect(rows.map((row: { kind: string }) => row.kind)).toEqual([
    "set", "set", "set", "set", "set", "skill", "skill", "skill", "skill", "hook",
  ]);

  const tree = setInspectionText(activation.units, "workspace");
  expect(tree).toContain("set workspace: on · permission on · owner north");
  expect(tree).toContain("set orchestration: on · permission on · owner north");
  expect(tree).toContain("set coordination: on · permission on · owner north");
  expect(tree.indexOf("skill messages")).toBeLessThan(tree.indexOf("skill threads"));
  expect(tree.indexOf("skill threads")).toBeLessThan(tree.indexOf("skill assignments"));
  expect(tree.match(/set coordination:/g)).toHaveLength(2);
  expect(tree).toContain("skill messages: on · permission on · owner north · also in operations");
  expect(tree).toContain("activation: workspace → orchestration → coordination → messages");
  expect(tree).toContain("activation: workspace → planning → coordination → messages");
});

test("unknown or missing schema fails closed instead of using a legacy reader", () => {
  expect(() => activationOfJson(resolved({ schema: "north.agent-activation/v2" })))
    .toThrow("unsupported activation schema: north.agent-activation/v2");
  expect(() => activationOfJson(JSON.stringify({ units })))
    .toThrow("unsupported activation schema: missing");
});
