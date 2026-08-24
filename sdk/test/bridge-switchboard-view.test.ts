import { expect, test } from "bun:test";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import { createTestRenderer } from "@opentui/core/testing";
import {
  "config-activation-of-json" as activationOfJson,
  "config-header-roles" as configHeaderRoles,
  "config-row-parts" as configRowParts,
  "config-section-title" as configSectionTitle,
  "config-state-text" as configStateText,
  "config-view-rows" as configViewRows,
  "render-config-panel!" as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";
import { resolved } from "./bridge-module-members.test";

Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });

test("switchboard renders Hooks, Modules, Skills with alphabetical entries", () => {
  const activation = activationOfJson(resolved());
  const entries = configViewRows(activation.units, "all");
  const roles = entries.map((entry: { kind: string }) => configHeaderRoles(entry.kind)[0]);
  expect([...new Set(roles)]).toEqual(["hook", "module", "skill"]);
  expect([...new Set(roles)].map(configSectionTitle)).toEqual(["HOOKS", "MODULES", "SKILLS"]);

  const orchestration = entries.find((entry: { name: string }) => entry.name === "orchestration");
  const state = configStateText(orchestration);
  expect(configRowParts(orchestration, state, 100)).toMatchObject({
    name: "orchestration",
    state: "on",
    detail: "3 members",
  });
});

test("the real headless Switchboard widget renders alphabetical binary rows", async () => {
  const activation = activationOfJson(resolved());
  const entries = configViewRows(activation.units, "all");
  const runtime = {
    configEntries: entries,
    configAllEntries: activation.units,
    configFilter: "all",
    configIndex: 0,
    configLoaded: true,
    configInspectId: "",
    detailView: "config",
    panelFocused: true,
  };
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 120, height: 24,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(runtime);
  await renderOnce();
  const snapshot = captureCharFrame();
  renderer.destroy();

  const ordered = [
    "HOOKS", "worktree-guard: on",
    "MODULES", "coordination: on", "operations: off", "orchestration: on",
    "planning: on", "workspace: on",
    "SKILLS", "assignments: on", "messages: on", "review: on", "threads: on",
  ];
  const positions = ordered.map((text) => {
    const position = snapshot.indexOf(text);
    expect(position).toBeGreaterThanOrEqual(0);
    return position;
  });
  expect(positions).toEqual([...positions].sort((left, right) => left - right));
  expect(snapshot).not.toContain("permission on");
  expect(snapshot).not.toContain("permission off");
});
