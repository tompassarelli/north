import { expect, test } from "bun:test";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import { createTestRenderer } from "@opentui/core/testing";
import {
  "apply-view-visibility!" as applyViewVisibility,
  "boot-view" as bootView,
  "composer-hint" as composerHint,
  "escape-rung" as escapeRung,
  "handle-local-command!" as handleLocalCommand,
  "palette-options" as paletteOptions,
  "render-view-tabs!" as renderViewTabs,
  "semantic-view-text!" as semanticViewText,
  "tab-swap-view" as tabSwapView,
  "top-level-view?" as topLevelView,
  "view-list" as viewList,
  "view-tab-id-at!" as viewTabIdAt,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "->TrackedThing" as TrackedThing,
  "make-model" as makeModel,
  "replace-catalog" as replaceCatalog,
  "snapshot" as snapshot,
} from "../src/bridge/generated/north/bridge/model.js";

Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });

const THINGS = [
  TrackedThing("@tracked:01-tracker", "Tracker", null, true, false, false, false,
    null, null, "ready"),
  TrackedThing("@tracked:02-worker", "Worker", null, true, false, false, false,
    null, null, "ready"),
  TrackedThing(
    "@tracked:03-ship-bridge", "Ship bridge", "The bridge candidate is accepted",
    false, true, false, true, "@tracked:02-worker", "Worker", null,
  ),
  TrackedThing(
    "@tracked:04-release", "Release succeeds",
    "The release is available to its intended users",
    false, false, false, false, null, null, null,
  ),
  TrackedThing("@tracked:05-release-path", "Release path", null,
    false, true, false, false, null, null, null),
  TrackedThing("@tracked:06-note", "Plain tracked note", null,
    false, false, false, false, null, null, null),
];

function state(view = "agents") {
  return snapshot(replaceCatalog(
    makeModel(view), THINGS, "north-coordination", 42,
  ));
}

const SESSION = {
  sessionModel: "claude-fable-5",
  sessionEffort: "xhigh",
  sessionCwd: "/tmp/demo",
  sessionBranch: "not a Git worktree",
  transcriptView: "selected",
};

async function renderSemanticView(view: string): Promise<string> {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 12,
  });
  const root = new BoxRenderable(renderer, {
    flexDirection: "column", width: "100%", height: "100%",
  });
  const tabs = new TextRenderable(renderer, { height: 1, width: "100%" });
  const body = new TextRenderable(renderer, { flexGrow: 1, width: "100%" });
  tabs.content = renderViewTabs(view, state(view), view, SESSION);
  body.content = semanticViewText(state(view), view, 0, 106);
  root.add(tabs);
  root.add(body);
  renderer.root.add(root);
  await renderOnce();
  const captured = captureCharFrame();
  renderer.destroy();
  return captured;
}

test("the only top-level routes are Agents, Goals, and All", () => {
  expect(bootView()).toBe("agents");
  expect(["agents", "goals", "all"].map(topLevelView)).toEqual([true, true, true]);
  expect(["list", "board", "graph", "threads", "tasks"].map(topLevelView))
    .toEqual([false, false, false, false, false]);
  expect(viewList(state()).map((view: { id: string }) => view.id))
    .toEqual(["agents", "goals", "all"]);
  expect(tabSwapView("agents")).toBe("goals");
  expect(tabSwapView("goals")).toBe("all");
  expect(tabSwapView("all")).toBe("agents");
});

test("route commands switch the one full-width product surface", () => {
  const runtime = {
    view: "agents",
    activeView: "agents",
    model: makeModel("agents"),
    paletteIndex: 3,
    workspaceNotice: "stale",
    stripFocused: false,
    detailView: "",
    renders: 0,
    render() { this.renders += 1; },
  };
  const ui = { composerInput: { focus() {} } };

  expect(handleLocalCommand(runtime, ui, "/goals")).toBe(true);
  expect(runtime.view).toBe("goals");
  expect(handleLocalCommand(runtime, ui, "/all")).toBe(true);
  expect(runtime.view).toBe("all");
  expect(handleLocalCommand(runtime, ui, "/agents")).toBe(true);
  expect(runtime.view).toBe("agents");
  expect(handleLocalCommand(runtime, ui, "/threads")).toBe(false);

  const panes = { agentsPane: { visible: false }, workPane: { visible: false } };
  applyViewVisibility({ view: "agents" }, panes);
  expect(panes).toEqual({ agentsPane: { visible: true }, workPane: { visible: false } });
  applyViewVisibility({ view: "goals" }, panes);
  expect(panes).toEqual({ agentsPane: { visible: false }, workPane: { visible: true } });
  applyViewVisibility({ view: "all" }, panes);
  expect(panes).toEqual({ agentsPane: { visible: false }, workPane: { visible: true } });
  expect(runtime.renders).toBeGreaterThan(0);
});

test("the fixed view bar maps clicks to the three displayed labels", () => {
  const views = viewList(state());
  expect(viewTabIdAt("agents", views, 0)).toBe("agents");
  expect(viewTabIdAt("agents", views, 5)).toBe("agents");
  expect(viewTabIdAt("agents", views, 7)).toBe("");
  expect(viewTabIdAt("agents", views, 9)).toBe("goals");
  expect(viewTabIdAt("agents", views, 13)).toBe("goals");
  expect(viewTabIdAt("agents", views, 17)).toBe("all");
  expect(viewTabIdAt("agents", views, 19)).toBe("all");
  expect(viewTabIdAt("agents", views, 20)).toBe("");
});

test("the headless semantic surface matches the accepted product frame", async () => {
  const agents = await renderSemanticView("agents");
  expect(agents).toContain(
    "Agents | Goals | All > claude-fable-5 xhigh · /tmp/demo · not a Git worktree",
  );
  expect(agents).toContain("› Tracker (ready)");
  expect(agents).toContain("Worker (ready)");

  const goals = await renderSemanticView("goals");
  expect(goals).toContain("Agents | Goals | All > desired outcomes");
  expect(goals).toContain(
    "› [assigned: Worker] Ship bridge — The bridge candidate is accepted",
  );
  expect(goals).toContain(
    "[unassigned] Release succeeds — The release is available to its intended users",
  );
  expect(goals).not.toContain("Release path");
  expect(goals).not.toContain("Plain tracked note");

  const all = await renderSemanticView("all");
  expect(all).toContain("Agents | Goals | All > all tracked things");
  expect(all).toContain("› [Agent] Tracker");
  expect(all).toContain("[Agent] Worker");
  expect(all).toContain(
    "[Goal · Plan · Task] Ship bridge — The bridge candidate is accepted · assigned to Worker",
  );
  expect(all).toContain(
    "[Goal] Release succeeds — The release is available to its intended users · unassigned",
  );
  expect(all).toContain("[Plan] Release path");
  expect(all).toContain("[Tracked] Plain tracked note");
});

test("palette, chrome, empty states, and semantic rows use only product vocabulary", () => {
  const empty = snapshot(makeModel("agents"));
  const rendered = [
    semanticViewText(empty, "agents", 0, 100),
    semanticViewText(empty, "goals", 0, 100),
    semanticViewText(empty, "all", 0, 100),
    semanticViewText(state("agents"), "agents", 0, 100),
    semanticViewText(state("goals"), "goals", 0, 100),
    semanticViewText(state("all"), "all", 0, 100),
    composerHint("agents", "Main"),
    composerHint("goals", "Main"),
    composerHint("all", "Main"),
    ...["agents", "goals", "all"].flatMap((view) =>
      paletteOptions(view, "/").flatMap((option: { name: string; description: string }) =>
        [option.name, option.description]),
    ),
  ].join("\n");

  expect(rendered).not.toMatch(/referent|thread|mention/iu);
  expect(composerHint("agents", "Main")).toBe("Message Main…");
  expect(composerHint("goals", "Main")).toContain("/ownership");
  expect(composerHint("all", "Main")).toBe("/filter, /show, /history, /inbox");
  expect(semanticViewText(empty, "agents", 0, 100)).toBe("No Agents");
  expect(semanticViewText(empty, "goals", 0, 100)).toBe("No Goals");
  expect(semanticViewText(empty, "all", 0, 100)).toBe("No tracked things");
});

test("escape returns Goals and All to Agents before cancelling work", () => {
  expect(escapeRung(false, false, false, false, true, true)).toBe("show-agents");
  expect(escapeRung(false, false, false, false, false, true)).toBe("cancel-turn");
  expect(escapeRung(false, false, false, false, false, false)).toBe("");
});
