import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  apply_frame_visibility_bang as applyFrameVisibility,
  boot_frame as bootFrame,
  composer_hint as composerHint,
  handle_local_command_bang as handleLocalCommand,
  palette_options as paletteOptions,
  quit_command_exits_p as quitCommandExits,
  render_view_tabs as renderViewTabs,
  thread_view_command_p as threadViewCommand,
  view_list as viewList,
  view_tab_id_at as viewTabIdAt,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  bridgesnapshot_active_view_id as activeViewId,
  make_model as makeModel, snapshot,
} from "../src/bridge/generated/north/bridge/model.js";

Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });

const state = (viewId = "list") => snapshot(makeModel(viewId));

// A runtime with only what the view switches touch. Nothing here is a pane:
// `frame` is which of the two views is on screen, full width.
function runtimeAt(frame: string, viewId = "list") {
  const runtime = {
    frame,
    model: makeModel(viewId),
    workIndex: 4,
    workScroll: { scrolledTo: -1, scrollTo(y: number) { this.scrolledTo = y; } },
    paletteIndex: 3,
    workspaceNotice: "stale",
    stripFocused: false,
    detailView: "",
    renders: 0,
    destroyed: false,
    render() { this.renders += 1; },
  };
  return runtime;
}
const ui = { composerInput: { focused: 0, focus() { this.focused += 1; } } };

test("Northbridge opens on Agents", () => {
  expect(bootFrame()).toBe("agents");
});

test("/threads and /agents swap which view is on screen", () => {
  const runtime = runtimeAt("agents");
  expect(handleLocalCommand(runtime, ui, "/threads")).toBe(true);
  expect(runtime.frame).toBe("threads");
  expect(runtime.renders).toBeGreaterThan(0);

  expect(handleLocalCommand(runtime, ui, "/agents")).toBe(true);
  expect(runtime.frame).toBe("agents");
});

test("naming a thread view shows the Threads view holding it", () => {
  expect(threadViewCommand("list")).toBe(true);
  expect(threadViewCommand("board")).toBe(true);
  expect(threadViewCommand("graph")).toBe(true);
  // /view is gone, and its arguments were never commands.
  expect(threadViewCommand("view")).toBe(false);
  expect(threadViewCommand("dag")).toBe(false);
  expect(threadViewCommand("kanban")).toBe(false);

  // From Agents: switch the sub-view AND come to look at it.
  const runtime = runtimeAt("agents", "list");
  expect(handleLocalCommand(runtime, ui, "/board")).toBe(true);
  expect(runtime.frame).toBe("threads");
  expect(activeViewId(snapshot(runtime.model))).toBe("board");
  // A new view starts at the top of its own list.
  expect(runtime.workIndex).toBe(0);
  expect(runtime.workScroll.scrolledTo).toBe(0);

  // From Threads: just the sub-view.
  expect(handleLocalCommand(runtime, ui, "/graph")).toBe(true);
  expect(runtime.frame).toBe("threads");
  expect(activeViewId(snapshot(runtime.model))).toBe("graph");
});

test("/q goes back before it quits", () => {
  // Pure decision first: Threads has somewhere to go back to, Agents does not.
  expect(quitCommandExits("threads")).toBe(false);
  expect(quitCommandExits("agents")).toBe(true);
  expect(quitCommandExits(bootFrame())).toBe(true);
  expect(quitCommandExits(undefined)).toBe(true);

  // From Threads it is a swap, not an exit.
  const runtime = runtimeAt("threads");
  expect(handleLocalCommand(runtime, ui, "/q")).toBe(true);
  expect(runtime.frame).toBe("agents");
});

test("both command sets are discoverable, and /view is in neither", () => {
  const named = (frame: string, query: string) =>
    (paletteOptions(frame, query) as Array<{ name: string }>).map((c) => c.name);

  expect(named("agents", "/threads")).toEqual(["/threads"]);
  // /agents shares a prefix with /agentsmd; both are real, neither is hidden.
  expect(named("threads", "/agents")).toEqual(["/agents", "/agentsmd"]);
  expect(named("threads", "/l")).toEqual(["/list"]);
  expect(named("threads", "/b")).toEqual(["/board"]);
  expect(named("threads", "/g")).toContain("/graph");
  // Capture is not the Threads view's privilege.
  expect(named("agents", "/capture")).toEqual(["/capture"]);
  expect(named("threads", "/capture")).toEqual(["/capture"]);
  // /q in both, /view in neither, /split gone with the panes.
  expect(named("agents", "/q")).toEqual(["/q"]);
  expect(named("threads", "/q")).toEqual(["/q"]);
  for (const frame of ["agents", "threads"]) {
    expect(named(frame, "/view")).toEqual([]);
    expect(named(frame, "/split")).toEqual([]);
  }
});

test("the composer hint names the command set you are looking at", () => {
  expect(composerHint("agents", "Main")).toBe("Message Main… [Agent commands]");
  expect(composerHint("threads", "Main"))
    .toBe("/list, /board, /graph, /capture, /filter, /assign [Thread commands]");
});

// The bar is a row of labels with fixed geometry, so a column maps back to the
// label drawn there. Derived from the same constants the renderer spends.
test("every label on the view bar is clickable, and only while it is drawn", () => {
  const views = viewList(state());
  const at = (frame: string, column: number) => viewTabIdAt(frame, views, column);

  expect(at("agents", 0)).toBe("agents");
  expect(at("agents", 5)).toBe("agents");
  expect(at("agents", 7)).toBe("");      // the " | " separator
  expect(at("agents", 9)).toBe("threads");
  expect(at("agents", 15)).toBe("threads");
  // The sub-view tabs are not drawn from Agents, so nothing there is clickable.
  expect(at("agents", 20)).toBe("");
  expect(at("agents", 30)).toBe("");

  // Showing Threads, the same columns keep their meaning and the sub-tabs
  // appear after " > ".
  expect(at("threads", 0)).toBe("agents");
  expect(at("threads", 9)).toBe("threads");
  expect(at("threads", 19)).toBe("list");
  expect(at("threads", 24)).toBe("list");
  expect(at("threads", 25)).toBe("");     // the gap between two tabs
  expect(at("threads", 27)).toBe("board");
  expect(at("threads", 36)).toBe("graph");
});

async function frameOf(frame: string, viewId = "list") {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 70, height: 8,
  });
  const workspace = new BoxRenderable(renderer, {
    id: "workspace", flexDirection: "row", width: "100%", flexGrow: 1,
  });
  const agentsPane = new BoxRenderable(renderer, {
    id: "agents-pane", flexDirection: "column", width: "100%",
  });
  const workPane = new BoxRenderable(renderer, {
    id: "work-pane", flexDirection: "column", width: "100%",
  });
  const agentsText = new TextRenderable(renderer, { id: "agents-text" });
  const workText = new TextRenderable(renderer, { id: "work-text" });
  agentsText.content = "AGENTBODY";
  workText.content = "THREADBODY";
  agentsPane.add(agentsText);
  workPane.add(workText);
  const tabs = new TextRenderable(renderer, { id: "view-tabs", height: 1 });
  workspace.add(agentsPane);
  workspace.add(workPane);
  renderer.root.add(tabs);
  renderer.root.add(workspace);

  const runtime = { frame };
  applyFrameVisibility(runtime, { agentsPane, workPane });
  tabs.content = renderViewTabs(frame, state(viewId), viewId);
  await renderOnce();
  const captured = captureCharFrame();
  const visibility = { agents: agentsPane.visible, threads: workPane.visible };
  renderer.destroy();
  return { frame: captured, visibility };
}

test("the Agents view is alone on screen under a two-label bar", async () => {
  const agents = await frameOf(bootFrame());
  expect(agents.visibility).toEqual({ agents: true, threads: false });
  expect(agents.frame).toContain("Agents | Threads");
  expect(agents.frame).toContain("AGENTBODY");
  expect(agents.frame).not.toContain("THREADBODY");
  // No sub-view tabs while Agents is showing, and no divider to drag.
  expect(agents.frame).not.toContain("List");
  expect(agents.frame).not.toContain("│");
});

test("showing Threads extends the bar with that view's own tabs", async () => {
  const threads = await frameOf("threads", "board");
  expect(threads.visibility).toEqual({ agents: false, threads: true });
  expect(threads.frame).toContain("Agents | Threads > ");
  expect(threads.frame).toContain("THREADBODY");
  expect(threads.frame).not.toContain("AGENTBODY");
  // The active sub-view is bracketed; the others are drawn but plain.
  expect(threads.frame).toContain("[Board]");
  expect(threads.frame).toContain("List");
  expect(threads.frame).toContain("Graph");
  expect(threads.frame).not.toContain("│");

  const list = await frameOf("threads", "list");
  expect(list.frame).toContain("[List]");
  expect(list.frame).not.toContain("[Board]");
});
