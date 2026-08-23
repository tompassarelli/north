import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  "apply-view-visibility!" as applyViewVisibility,
  "active-focus" as activeFocus,
  "boot-view" as bootView,
  "composer-hint" as composerHint,
  "escape-rung" as escapeRung,
  "handle-local-command!" as handleLocalCommand,
  "install-keys!" as installKeys,
  "palette-options" as paletteOptions,
  "palette-enter-action" as paletteEnterAction,
  "quit-command?" as quitCommand,
  "render-detail-panel!" as renderDetailPanel,
  "render-view-tabs!" as renderViewTabs,
  "restore-submitted-text!" as restoreSubmittedText,
  "tab-swap-view" as tabSwapView,
  "thread-view-command?" as threadViewCommand,
  "view-list" as viewList,
  "view-tab-id-at!" as viewTabIdAt,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "bridgesnapshot-active-view-id" as activeViewId,
  "make-model" as makeModel,
  "snapshot" as snapshot,
} from "../src/bridge/generated/north/bridge/model.js";

Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });

const state = (viewId = "list") => snapshot(makeModel(viewId));

// A runtime with only what the view switches touch. Nothing here is a pane:
// `view` is which of the two views is on screen, full width.
function runtimeAt(view: string, viewId = "list") {
  const runtime = {
    view,
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
  expect(bootView()).toBe("agents");
});

test("/threads and /agents swap which view is on screen", () => {
  const runtime = runtimeAt("agents");
  expect(handleLocalCommand(runtime, ui, "/threads")).toBe(true);
  expect(runtime.view).toBe("threads");
  expect(runtime.renders).toBeGreaterThan(0);

  expect(handleLocalCommand(runtime, ui, "/agents")).toBe(true);
  expect(runtime.view).toBe("agents");
});

test("Ctrl-J enters expanded Agents and Ctrl-K returns to the composer", () => {
  let keypress: ((key: Record<string, unknown>) => unknown) | undefined;
  const runtime = {
    view: "agents",
    model: makeModel("list"),
    detailView: "agents",
    detailSegment: "all",
    detailIndex: 0,
    stripFocused: false,
    panelFocused: false,
    paletteIndex: 0,
    render() {},
  } as Record<string, unknown>;
  const composerInput = {
    value: "",
    focused: 1,
    blurred: 0,
    focus() { this.focused += 1; },
    blur() { this.blurred += 1; },
  };
  const ui = { composerInput };
  runtime.renderer = {
    keyInput: {
      on(_event: string, handler: (key: Record<string, unknown>) => unknown) {
        keypress = handler;
      },
    },
  };

  installKeys(runtime, ui);
  expect(keypress).toBeDefined();
  const press = (name: string, ctrl = false) => {
    const key = {
      name, ctrl, meta: false, option: false, sequence: "",
      defaultPrevented: false, propagationStopped: false,
      preventDefault() { this.defaultPrevented = true; },
      stopPropagation() { this.propagationStopped = true; },
    };
    keypress!(key);
    return key;
  };

  const down = press("j", true);
  expect(down.defaultPrevented).toBe(true);
  expect(runtime.detailView).toBe("agents");
  expect(runtime.panelFocused).toBe(true);
  expect(activeFocus(false, true, true, false, false)).toBe("panel");

  const up = press("k", true);
  expect(up.defaultPrevented).toBe(true);
  expect(runtime.detailView).toBe("");
  expect(runtime.panelFocused).toBe(false);
  expect(composerInput.focused).toBe(2);
  expect(composerInput.blurred).toBe(1);
  expect(activeFocus(false, false, false, false, false)).toBe("composer");
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
  expect(runtime.view).toBe("threads");
  expect(activeViewId(snapshot(runtime.model))).toBe("board");
  // A new view starts at the top of its own list.
  expect(runtime.workIndex).toBe(0);
  expect(runtime.workScroll.scrolledTo).toBe(0);

  // From Threads: just the sub-view.
  expect(handleLocalCommand(runtime, ui, "/graph")).toBe(true);
  expect(runtime.view).toBe("threads");
  expect(activeViewId(snapshot(runtime.model))).toBe("graph");
});

test("quit is unconditional while escape aliases navigate one rung", () => {
  expect(quitCommand("q")).toBe(true);
  expect(quitCommand("exit")).toBe(true);
  expect(quitCommand("close")).toBe(false);
  expect(quitCommand("esc")).toBe(false);

  const runtime = runtimeAt("threads");

  // Escape aliases navigate back from Threads instead of quitting.
  expect(handleLocalCommand(runtime, ui, "/close")).toBe(true);
  expect(runtime.view).toBe("agents");
  expect(runtime.destroyed).toBe(false);

  for (const spelling of ["/close", "/esc"]) {
    const r = runtimeAt("threads");
    expect(handleLocalCommand(r, ui, spelling)).toBe(true);
    expect(r.view).toBe("agents");
    expect(r.destroyed).toBe(false);
  }

  // A panel is an inner rung: it closes before the view does.
  const panelled = runtimeAt("threads");
  panelled.detailView = "config";
  expect(handleLocalCommand(panelled, ui, "/esc")).toBe(true);
  expect(panelled.detailView).toBe("");
  expect(panelled.view).toBe("threads");
});

test("both command sets are discoverable, and /view is in neither", () => {
  const named = (view: string, query: string) =>
    (paletteOptions(view, query) as Array<{ name: string }>).map((c) => c.name);

  expect(named("agents", "/threads")).toEqual(["/threads"]);
  expect(named("threads", "/agents")).toEqual(["/agents"]);
  expect(named("threads", "/l")).toEqual(["/list"]);
  expect(named("threads", "/b")).toEqual(["/board"]);
  expect(named("threads", "/g")).toContain("/graph");
  // Capture is not the Threads view's privilege.
  expect(named("agents", "/capture")).toEqual(["/capture"]);
  expect(named("threads", "/capture")).toEqual(["/capture"]);
  // /q in both, /view in neither, /split gone with the panes.
  expect(named("agents", "/q")).toEqual(["/q"]);
  expect(named("threads", "/q")).toEqual(["/q"]);
  expect(named("agents", "/exit")).toEqual([]);
  const quit = paletteOptions("agents", "/q") as Array<{
    name: string; description: string;
  }>;
  expect(quit).toMatchObject([{ name: "/q", description: "quit Northbridge" }]);
  for (const view of ["agents", "threads"]) {
    expect(named(view, "/view")).toEqual([]);
    expect(named(view, "/split")).toEqual([]);
  }
});

// The bar sits directly under the composer and already says which view is
// active, so the hint does not repeat it.
test("the composer hint says what to type and nothing about which view", () => {
  expect(composerHint("agents", "Main")).toBe("Message Main…");
  expect(composerHint("threads", "Main"))
    .toBe("/list, /board, /graph, /capture, /filter, /assign");
  for (const view of ["agents", "threads"]) {
    expect(composerHint(view, "Main")).not.toContain("commands]");
  }
});

// The ladder, as a matrix. The physical key climbs rungs one to five and stops
// there: a key you hit reflexively must never be the key that ends the session.
test("the escape ladder is innermost-first and never quits", () => {
  const rung = (
    palette: boolean, filtering: boolean, panel: boolean, strip: boolean,
    threads: boolean, working: boolean,
  ) => escapeRung(palette, filtering, panel, strip, threads, working);

  // Rung 1 outranks everything below it, including all of it at once.
  expect(rung(true, true, true, true, true, true)).toBe("close-palette");
    // Rung 2: a live filter is inside the panel that carries it, so the query
    // goes back before the panel does — clearing a search must not cost you the
    // switchboard you were searching.
  expect(rung(false, true, true, true, true, true)).toBe("clear-filter");
    // Rung 3.
  expect(rung(false, false, true, true, true, true)).toBe("close-detail");
    // Rung 4.
  expect(rung(false, false, false, true, true, true)).toBe("focus-composer");
    // Rung 5.
  expect(rung(false, false, false, false, true, true)).toBe("show-agents");

  // At the root escape spends itself on the turn in flight or does nothing.
  expect(rung(false, false, false, false, false, true)).toBe("cancel-turn");
  expect(rung(false, false, false, false, false, false)).toBe("");
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

  const { renderer, renderOnce, captureCharSnapshot } = await createTestRenderer({
    width: 110, height: 20,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderDetailPanel(runtime);
  await renderOnce();
  const snapshot = captureCharSnapshot();
  renderer.destroy();

  expect(snapshot).toContain("Northbridge keys");
  expect(snapshot).toContain("esc closes");
  expect(snapshot).toContain("Esc /close /esc");
  expect(snapshot).toContain("Ctrl-C /interrupt");
  expect(snapshot).toContain("/q /exit / Ctrl-Q");
  expect(snapshot).not.toContain("back, then quit");

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
  const at = (view: string, column: number) => viewTabIdAt(view, views, column);

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

test("the view-list switch hint names both directions", () => {
  const tabs = renderViewTabs("threads", state("list"), "list", SESSION);
  const text = tabs.chunks.map((chunk: { text: string }) => chunk.text).join("");
  expect(text).toContain("←/→ switch");
  expect(text).not.toContain("← switch →");
});

// The real root order, in miniature: content on top, then the bottom cluster —
// composer, view bar, agent strip. The rows come back so the order is an
// assertion and not a reading of the source.
async function renderView(view: string, viewId = "list") {
  const { renderer, renderOnce, captureCharSnapshot } = await createTestRenderer({
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

  const runtime = { view };
  applyViewVisibility(runtime, { agentsPane, workPane });
  tabs.content = renderViewTabs(view, state(viewId), viewId, SESSION);
  await renderOnce();
  const captured = captureCharSnapshot();
  const lines = captured.split("\n");
  const rowOf = (needle: string) => lines.findIndex((line) => line.includes(needle));
  const rows = {
    body: rowOf(view === "threads" ? "THREADBODY" : "AGENTBODY"),
    composer: rowOf("COMPOSERLINE"),
    tabs: rowOf("Agents | Threads"),
    strip: rowOf("STRIPLINE"),
  };
  // The hit box is columns off the bar's own origin, so this is the number the
  // click handler subtracts from the event.
  const origins = { tabs: tabs.x, composer: composer.x };
  const visibility = { agents: agentsPane.visible, threads: workPane.visible };
  renderer.destroy();
  return { snapshot: captured, visibility, rows, origins };
}

// Locality of information: everything you can act on is in one cluster at the
// bottom, and the bar that switches views sits with the composer that types
// into them, not a screen away at the top.
test("the view bar sits under the composer and over the strip", async () => {
  for (const view of ["agents", "threads"]) {
    const { rows } = await renderView(view);
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
  for (const view of ["agents", "threads"]) {
    const { origins } = await renderView(view);
    expect(origins.tabs).toBe(origins.composer);
    expect(viewTabIdAt(view, views, 0)).toBe("agents");
    expect(viewTabIdAt(view, views, 9)).toBe("threads");
  }
});

test("the Agents view is alone on screen, and the bar's tail says who you are talking to", async () => {
  const agents = await renderView(bootView());
  expect(agents.visibility).toEqual({ agents: true, threads: false });
  expect(agents.snapshot).toContain("Agents | Threads > claude-fable-5 xhigh");
  expect(agents.snapshot).toContain("/tmp/demo");
  expect(agents.snapshot).toContain("not a Git worktree");
  expect(agents.snapshot).toContain("AGENTBODY");
  expect(agents.snapshot).not.toContain("THREADBODY");
  // No sub-view tabs while Agents is showing, and no divider to drag.
  expect(agents.snapshot).not.toContain("List");
  expect(agents.snapshot).not.toContain("│");
});

test("showing Threads extends the bar with that view's own tabs", async () => {
  const threads = await renderView("threads", "board");
  expect(threads.visibility).toEqual({ agents: false, threads: true });
  expect(threads.snapshot).toContain("Agents | Threads > ");
  expect(threads.snapshot).toContain("THREADBODY");
  expect(threads.snapshot).not.toContain("AGENTBODY");
  // The active sub-view is bracketed; the others are drawn but plain.
  expect(threads.snapshot).toContain("[Board]");
  expect(threads.snapshot).toContain("List");
  expect(threads.snapshot).toContain("Graph");
  expect(threads.snapshot).toContain("←/→ switch");
  expect(threads.snapshot).not.toContain("← switch →");
  expect(threads.snapshot).not.toContain("│");
  // The tail belongs to the active view: sub-tabs here, no session identity.
  expect(threads.snapshot).not.toContain("claude-fable-5");

  const list = await renderView("threads", "list");
  expect(list.snapshot).toContain("[List]");
  expect(list.snapshot).not.toContain("[Board]");
});

// Tab's other meaning, the one it has always had: with the keyboard in the
// composer it swaps which view is on screen. The panel's fold is a different
// surface's verb and cannot reach here.
test("tab swaps the view on screen, and swaps it back", async () => {
  expect(tabSwapView("agents")).toBe("threads");
  expect(tabSwapView("threads")).toBe("agents");

  const agents = await renderView("agents");
  expect(agents.visibility).toEqual({ agents: true, threads: false });
  expect(agents.snapshot).toContain("AGENTBODY");

  const swapped = await renderView(tabSwapView("agents") as string);
  expect(swapped.visibility).toEqual({ agents: false, threads: true });
  expect(swapped.snapshot).toContain("THREADBODY");

  const back = await renderView(tabSwapView("threads") as string);
  expect(back.visibility).toEqual({ agents: true, threads: false });
});
