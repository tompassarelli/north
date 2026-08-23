import { expect, test } from "bun:test";
import {
  "config-activation-path-from" as activationPathFrom,
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
  owner: { repo: string; path: string };
  members: string[];
  supports: string[];
  distributions: unknown[];
  activationPaths: string[][];
};

export const units: Unit[] = [
  { id: "workspace", kind: "set", title: "Workspace", triggerDescription: "work together", permission: "on", active: true, owner: { repo: "north", path: "agent-catalog/sets/workspace.json" }, members: ["orchestration", "planning"], supports: [], distributions: [], activationPaths: [["workspace"]] },
  { id: "orchestration", kind: "set", title: "Orchestration", triggerDescription: "delegate work", permission: "on", active: true, owner: { repo: "north", path: "agent-catalog/sets/orchestration.json" }, members: ["coordination", "review"], supports: [], distributions: [], activationPaths: [["workspace", "orchestration"]] },
  { id: "planning", kind: "set", title: "Planning", triggerDescription: "plan work", permission: "on", active: true, owner: { repo: "north", path: "agent-catalog/sets/planning.json" }, members: ["coordination"], supports: [], distributions: [], activationPaths: [["workspace", "planning"]] },
  { id: "coordination", kind: "set", title: "Coordination", triggerDescription: "coordinate agents", permission: "on", active: true, owner: { repo: "north", path: "agent-catalog/sets/coordination.json" }, members: ["messages", "threads", "assignments"], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "coordination"], ["workspace", "planning", "coordination"]] },
  { id: "operations", kind: "set", title: "Operations", triggerDescription: "operate North", permission: "off", active: false, owner: { repo: "north", path: "agent-catalog/sets/operations.json" }, members: ["messages"], supports: [], distributions: [], activationPaths: [] },
  { id: "messages", kind: "skill", title: "Messages", triggerDescription: "send a message", permission: "on", active: true, owner: { repo: "north", path: "coordination/messages/SKILL.md" }, members: [], supports: [], distributions: ["codex"], activationPaths: [["workspace", "orchestration", "coordination", "messages"], ["workspace", "planning", "coordination", "messages"]] },
  { id: "threads", kind: "skill", title: "Threads", triggerDescription: "manage threads", permission: "on", active: true, owner: { repo: "north", path: "coordination/threads/SKILL.md" }, members: [], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "coordination", "threads"], ["workspace", "planning", "coordination", "threads"]] },
  { id: "assignments", kind: "skill", title: "Assignments", triggerDescription: "assign work", permission: "on", active: true, owner: { repo: "north", path: "coordination/assignments/SKILL.md" }, members: [], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "coordination", "assignments"], ["workspace", "planning", "coordination", "assignments"]] },
  { id: "review", kind: "skill", title: "Review", triggerDescription: "review work", permission: "on", active: true, owner: { repo: "north", path: "profiles/tom/skills/review/SKILL.md" }, members: [], supports: [], distributions: [], activationPaths: [["workspace", "orchestration", "review"]] },
  { id: "worktree-guard", kind: "hook", title: "Worktree guard", triggerDescription: "before writes", permission: "on", active: true, owner: { repo: "nixos-config", path: "dotfiles/agents/hooks/worktree-guard.sh" }, members: [], supports: ["repo-safety", "orchestration"], distributions: ["codex", "claude"], activationPaths: [["orchestration", "worktree-guard"], ["repo-safety", "worktree-guard"]] },
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
  expect(tree).toContain("set workspace: on · permission on · owner north:agent-catalog/sets/workspace.json");
  expect(tree).toContain("set orchestration: on · permission on · owner north:agent-catalog/sets/orchestration.json");
  expect(tree).toContain("set coordination: on · permission on · owner north:agent-catalog/sets/coordination.json");
  expect(tree.indexOf("skill messages")).toBeLessThan(tree.indexOf("skill threads"));
  expect(tree.indexOf("skill threads")).toBeLessThan(tree.indexOf("skill assignments"));
  expect(tree.match(/set coordination:/g)).toHaveLength(2);
  expect(tree).toContain("skill messages: on · permission on · owner north:coordination/messages/SKILL.md · also in operations");
  expect(tree).toContain("activation: workspace → orchestration → coordination → messages");
  expect(tree).toContain("activation: workspace → planning → coordination → messages");
});

test("unknown or missing schema fails closed", () => {
  expect(() => activationOfJson(resolved({ schema: "north.agent-activation/v2" })))
    .toThrow("unsupported activation schema: north.agent-activation/v2");
  expect(() => activationOfJson(JSON.stringify({ units })))
    .toThrow("unsupported activation schema: missing");
});

test("only set, skill, and hook reach Bridge presentation", () => {
  for (const kind of ["plugin", "unknown"]) {
    const payload = JSON.parse(resolved());
    payload.units = [{ ...payload.units[0], kind }];
    expect(() => activationOfJson(JSON.stringify(payload)))
      .toThrow(`activation generation has invalid unit kind: ${kind}`);
  }
});

test("activation path honors the configured North agent state root", () => {
  expect(activationPathFrom({}, {
    HOME: "/home/tester",
    NORTH_AGENT_STATE_ROOT: "/var/lib/north-agent-state",
  })).toBe("/var/lib/north-agent-state/current/activation.json");
  expect(activationPathFrom({}, { HOME: "/home/tester" }))
    .toBe("/home/tester/.local/state/north/agents/current/activation.json");
  expect(activationPathFrom({ configActivationPath: "/run/test/activation.json" }, {
    HOME: "/home/tester",
    NORTH_AGENT_STATE_ROOT: "/var/lib/north-agent-state",
  })).toBe("/run/test/activation.json");
});

test("owner formatting is safe at the dynamic activation boundary", () => {
  const payload = JSON.parse(resolved());
  payload.units = [
    { ...payload.units[0], owner: "north:agent-catalog/catalog.json" },
    { ...payload.units[1], owner: { repo: "north", path: "" } },
    { ...payload.units[2], owner: null },
  ];
  expect(activationOfJson(JSON.stringify(payload)).units.map(
    (unit: { owner: string }) => unit.owner,
  )).toEqual(["north:agent-catalog/catalog.json", "north", ""]);
});
