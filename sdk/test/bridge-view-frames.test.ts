import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  apply_frame_visibility_bang as applyFrameVisibility,
  boot_frame as bootFrame,
  composer_hint as composerHint,
  escape_rung as escapeRung,
  handle_local_command_bang as handleLocalCommand,
  palette_options as paletteOptions,
  palette_enter_action as paletteEnterAction,
  render_detail_panel_bang as renderDetailPanel,
  render_view_tabs as renderViewTabs,
  restore_submitted_text_bang as restoreSubmittedText,
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
    conversation: [] as Array<{ body: string }>,
    itemSequence: 0,
    renders: 0,
    destroyed: false,
    render() { this.renders += 1; },
  };
  return runtime;
}
// The panel takes the keyboard while it is open, so the stub has to be able to
// lose the cursor as well as take it.
const ui = {
  composerInput: {
    focused: 0, blurred: 0,
    focus() { this.focused += 1; },
    blur() { this.blurred += 1; },
  },
};

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

// One escape verb, four spellings, one rung per invocation. Quit lives on the
// last rung and nowhere else, so it is always reachable by repetition and never
// by accident.
test("the escape family climbs one rung at a time and only quits at the root", () => {
  const runtime = runtimeAt("threads");

  // From Threads with nothing open: back to Agents, not out.
  expect(handleLocalCommand(runtime, ui, "/q")).toBe(true);
  expect(runtime.frame).toBe("agents");
  expect(runtime.destroyed).toBe(false);

  // Every spelling is the same verb, and each one climbs the same rung.
  // (The root rung calls process.exit, so it is asserted on the pure ladder
  // below rather than driven through a live runtime here.)
  for (const spelling of ["/close", "/esc", "/exit"]) {
    const r = runtimeAt("threads");
    expect(handleLocalCommand(r, ui, spelling)).toBe(true);
    expect(r.frame).toBe("agents");
    expect(r.destroyed).toBe(false);
  }

  // A panel is an inner rung: it closes before the view does.
  const panelled = runtimeAt("threads");
  panelled.detailView = "config";
  expect(handleLocalCommand(panelled, ui, "/q")).toBe(true);
  expect(panelled.detailView).toBe("");
  expect(panelled.frame).toBe("threads");
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

// The bar sits directly under the composer and already says which frame is
// active, so the hint does not repeat it.
test("the composer hint says what to type and nothing about which frame", () => {
  expect(composerHint("agents", "Main")).toBe("Message Main…");
  expect(composerHint("threads", "Main"))
    .toBe("/list, /board, /graph, /capture, /filter, /assign");
  for (const frame of ["agents", "threads"]) {
    expect(composerHint(frame, "Main")).not.toContain("commands]");
  }
});

// The ladder, as a matrix. The physical key climbs rungs one to five and stops
// there: a key you hit reflexively must never be the key that ends the session.
test("the escape ladder is innermost-first, and the key stops short of quitting", () => {
  const rung = (
    palette: boolean, filtering: boolean, panel: boolean, strip: boolean,
    threads: boolean, working: boolean, fromKey: boolean,
  ) => escapeRung(palette, filtering, panel, strip, threads, working, fromKey);

  for (const fromKey of [true, false]) {
    // Rung 1 outranks everything below it, including all of it at once.
    expect(rung(true, true, true, true, true, true, fromKey)).toBe("close-palette");
    // Rung 2: a live filter is inside the panel that carries it, so the query
    // goes back before the panel does — clearing a search must not cost you the
    // switchboard you were searching.
    expect(rung(false, true, true, true, true, true, fromKey)).toBe("clear-filter");
    // Rung 3.
    expect(rung(false, false, true, true, true, true, fromKey)).toBe("close-detail");
    // Rung 4.
    expect(rung(false, false, false, true, true, true, fromKey)).toBe("focus-composer");
    // Rung 5.
    expect(rung(false, false, false, false, true, true, fromKey)).toBe("show-agents");
  }

  // Rung 6 is the command's alone: from the empty root the verb quits.
  expect(rung(false, false, false, false, false, false, false)).toBe("quit");
  expect(rung(false, false, false, false, false, true, false)).toBe("quit");

  // The key never reaches it. At the root it spends itself on the turn in
  // flight, or on nothing at all.
  expect(rung(false, false, false, false, false, true, true)).toBe("cancel-turn");
  expect(rung(false, false, false, false, false, false, true)).toBe("");
});

// Help used to print itself into the transcript with no way to dismiss it.
// It is a tenant of the docked panel now, so the ladder closes it and the panel
// says so on its own header.
test("/help opens the docked panel and escape closes it", async () => {
  const runtime = runtimeAt("agents");
  expect(handleLocalCommand(runtime, ui, "/help")).toBe(true);
  expect(runtime.detailView).toBe("help");
  // An open panel holds the keyboard: the composer keeps its content and loses
  // the cursor, exactly as it does when the agent strip takes focus.
  expect(runtime.panelFocused).toBe(true);

  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 20,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderDetailPanel(runtime);
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();

  expect(frame).toContain("Northbridge keys");
  expect(frame).toContain("esc closes");
  // The escape family reads as one entry, not two contradicting ones.
  expect(frame).toContain("/q /close /esc /exit");
  expect(frame).not.toContain("back, then quit");

  // Rung two: the panel it opened is the first thing escape takes back, and the
  // keyboard goes back to the composer with it.
  expect(handleLocalCommand(runtime, ui, "/esc")).toBe(true);
  expect(runtime.detailView).toBe("");
  expect(runtime.panelFocused).toBe(false);

  // Toggling it off from the command is the same switch.
  expect(handleLocalCommand(runtime, ui, "/help")).toBe(true);
  expect(runtime.detailView).toBe("help");
  expect(handleLocalCommand(runtime, ui, "/help")).toBe(true);
  expect(runtime.detailView).toBe("");
});

// One keystroke for a command that has nothing left to say. Arguments, and the
// emoji/glyph pickers whose entries are text rather than commands, still get
// completed into the composer so there is something to type next.
test("enter on the palette fires a finished command and completes an unfinished one", () => {
  const enter = (
    matches: number, takesArgs: boolean, insertOnly = false, completed = false,
  ) => paletteEnterAction(matches, takesArgs, insertOnly, completed);

  // Nothing matched: enter is not the palette's to claim.
  expect(enter(0, false)).toBe("");
  expect(enter(0, true)).toBe("");

  // One match, no arguments: run it now, not on the second enter.
  expect(enter(1, false)).toBe("fire");
  // One match that takes arguments: complete and keep typing.
  expect(enter(1, true)).toBe("complete");

  // Several matches: the same rule, applied to the highlighted entry.
  expect(enter(4, false)).toBe("fire");
  expect(enter(4, true)).toBe("complete");

  // Emoji and glyph entries are text to insert, never commands to run.
  expect(enter(1, false, true)).toBe("complete");
  expect(enter(6, false, true)).toBe("complete");

  // Once the composer already holds what completion would write, there is
  // nothing left to complete, so enter fires — this is how an argument command
  // is finally sent bare and how a picked glyph is submitted.
  expect(enter(1, true, false, true)).toBe("fire");
  expect(enter(1, false, true, true)).toBe("fire");
});

test("a cancelled turn hands its message back to the composer", () => {
  const composerInput = { value: "", focused: 0, focus() { this.focused += 1; } };
  const runtime = { lastSubmitted: "rewrite the staleness note" };
  restoreSubmittedText(runtime, { composerInput });
  expect(composerInput.value).toBe("rewrite the staleness note");
  expect(composerInput.focused).toBe(1);
  // Handed back once: a second cancel must not re-stuff a stale message.
  expect(runtime.lastSubmitted).toBe("");
  restoreSubmittedText(runtime, { composerInput });
  expect(composerInput.value).toBe("rewrite the staleness note");
  expect(composerInput.focused).toBe(1);
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

// A stand-in session: the identity the Agents tail spends its half of the bar
// on. A literal cwd keeps `short-directory` off whatever HOME ran the suite.
const SESSION = {
  sessionModel: "claude-fable-5",
  sessionEffort: "xhigh",
  sessionCwd: "/tmp/demo",
  sessionBranch: "not a Git worktree",
};

// The real root order, in miniature: content on top, then the bottom cluster —
// composer, view bar, agent strip. The rows come back so the order is an
// assertion and not a reading of the source.
async function frameOf(frame: string, viewId = "list") {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 90, height: 10,
  });
  const root = new BoxRenderable(renderer, {
    id: "root", flexDirection: "column", width: "100%", height: "100%",
    paddingTop: 1, paddingBottom: 0,
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
  const composer = new TextRenderable(renderer, { id: "composer", height: 1 });
  composer.content = "COMPOSERLINE";
  const tabs = new TextRenderable(renderer, { id: "view-tabs", height: 1 });
  const strip = new TextRenderable(renderer, { id: "agent-strip", height: 1 });
  strip.content = "STRIPLINE";
  workspace.add(agentsPane);
  workspace.add(workPane);
  root.add(workspace);
  root.add(composer);
  root.add(tabs);
  root.add(strip);
  renderer.root.add(root);

  const runtime = { frame };
  applyFrameVisibility(runtime, { agentsPane, workPane });
  tabs.content = renderViewTabs(frame, state(viewId), viewId, SESSION);
  await renderOnce();
  const captured = captureCharFrame();
  const lines = captured.split("\n");
  const rowOf = (needle: string) => lines.findIndex((line) => line.includes(needle));
  const rows = {
    body: rowOf(frame === "threads" ? "THREADBODY" : "AGENTBODY"),
    composer: rowOf("COMPOSERLINE"),
    tabs: rowOf("Agents | Threads"),
    strip: rowOf("STRIPLINE"),
  };
  // The hit box is columns off the bar's own origin, so this is the number the
  // click handler subtracts from the event.
  const origins = { tabs: tabs.x, composer: composer.x };
  const visibility = { agents: agentsPane.visible, threads: workPane.visible };
  renderer.destroy();
  return { frame: captured, visibility, rows, origins };
}

// Locality of information: everything you can act on is in one cluster at the
// bottom, and the bar that switches views sits with the composer that types
// into them, not a screen away at the top.
test("the view bar sits under the composer and over the strip", async () => {
  for (const frame of ["agents", "threads"]) {
    const { rows } = await frameOf(frame);
    expect(rows.body).toBeGreaterThanOrEqual(0);
    expect(rows.tabs).toBe(rows.composer + 1);
    expect(rows.strip).toBe(rows.tabs + 1);
    // Content is above the whole cluster.
    expect(rows.body).toBeLessThan(rows.composer);
  }
});

// Moving the bar changed its row, not its column origin, so the labels are
// still where a click lands: column zero of the bar is still `Agents`.
test("the bar's hit box moved rows without moving columns", async () => {
  const views = viewList(state());
  for (const frame of ["agents", "threads"]) {
    const { origins } = await frameOf(frame);
    expect(origins.tabs).toBe(origins.composer);
    expect(viewTabIdAt(frame, views, 0)).toBe("agents");
    expect(viewTabIdAt(frame, views, 9)).toBe("threads");
  }
});

test("the Agents view is alone on screen, and the bar's tail says who you are talking to", async () => {
  const agents = await frameOf(bootFrame());
  expect(agents.visibility).toEqual({ agents: true, threads: false });
  expect(agents.frame).toContain("Agents | Threads > claude-fable-5 xhigh");
  expect(agents.frame).toContain("/tmp/demo");
  expect(agents.frame).toContain("not a Git worktree");
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
  // The tail belongs to the active view: sub-tabs here, no session identity.
  expect(threads.frame).not.toContain("claude-fable-5");

  const list = await frameOf("threads", "list");
  expect(list.frame).toContain("[List]");
  expect(list.frame).not.toContain("[Board]");
});
