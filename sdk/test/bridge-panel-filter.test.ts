import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  clear_panel_filter_bang as clearPanelFilter,
  config_detail_lines as configDetailLines,
  config_fold_rows as configFoldRows,
  config_node_expanded_p as configNodeExpanded,
  config_panel_legend as configPanelLegend,
  config_panel_rows as configPanelRows,
  config_query_field as configQueryField,
  config_query_rows as configQueryRows,
  config_reference_text as configReferenceText,
  config_row_context_only_p as configRowContextOnly,
  config_row_matches_p as configRowMatches,
  config_row_search_text as configRowSearchText,
  config_view_rows as configViewRows,
  detail_height as detailHeight,
  escape_rung as escapeRung,
  filter_character as filterCharacter,
  filter_key_action as filterKeyAction,
  fold_key_action as foldKeyAction,
  active_focus as activeFocus,
  config_kind_tag as configKindTag,
  tab_action as tabAction,
  tab_fold_step_bang as tabFoldStep,
  tab_swap_frame as tabSwapFrame,
  help_query_rows as helpQueryRows,
  render_detail_panel_bang as renderDetailPanel,
  set_node_expanded_bang as setNodeExpanded,
  set_panel_query_bang as setPanelQuery,
} from "../src/bridge/generated/north/bridge/app.js";

// The panel reads the terminal for its window math. Pin both dimensions so the
// frames below are arithmetic rather than a property of whoever's tty ran the
// suite.
const ROWS = 40;
const COLUMNS = 120;
Object.defineProperty(process.stdout, "rows", { value: ROWS, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: COLUMNS, configurable: true });

const PANEL_BUDGET = ROWS - 4 - 4;

type Row = { kind: string; name: string; state: string; detail: string };

function row(kind: string, name: string, state = "off", detail = ""): Row {
  return { kind, name, state, detail };
}

const MANIFEST: Row[] = [
  row("dir", "global", "on", "~"),
  row("ins", "global", "on"),
  row("dir", "code", "on", "/tmp/switchboard-fixture/code"),
  row("ins", "code", "on"),
  row("memroot", "code", "on"),
  row("mem", "code/report-style-recommendations", "on"),
  row("mem", "code/agents-switchboard-architecture", "on"),
  row("dir", "north", "on", "/tmp/switchboard-fixture/north"),
  row("module", "orchestration", "on"),
  row("skill", "firn", "on"),
  row("skill", "webdev", "on"),
  row("hook", "firn-guard", "enabled", "firn"),
  row("hook", "tripwire-guard", "disabled"),
  row("other", "statusline-script", "on"),
];

const ids = (rows: Row[]) => rows.map((r) => `${r.kind}:${r.name}`);
const viewRows = (view = "all") => configViewRows(MANIFEST.slice(), view) as Row[];
const queried = (query: string, view = "all") =>
  configQueryRows(viewRows(view), query) as Row[];

test("a row is searchable by what it says, what it is called and what it carries", () => {
  // The label is what is on screen, the name is how the CLI spells it, the
  // detail is the path. Searching one of them and not the others makes rows
  // unfindable by the very text printed on them.
  expect(configRowSearchText(row("ins", "code", "on"))).toBe("agents.md code ");
  expect(configRowSearchText(row("mem", "code/style", "on")))
    .toBe("style code/style ");
  expect(configRowSearchText(row("dir", "code", "on", "/tmp/switchboard-fixture/code")))
    .toBe("code code /tmp/switchboard-fixture/code");

  // Case-insensitive substring, and an empty query matches everything —
  // entering filter mode must not blank the panel before a key is typed.
  expect(configRowMatches(row("skill", "firn", "on"), "FIR")).toBe(true);
  expect(configRowMatches(row("skill", "firn", "on"), "webdev")).toBe(false);
  expect(configRowMatches(row("skill", "firn", "on"), "")).toBe(true);
  expect(configRowMatches(row("skill", "firn", "on"), "   ")).toBe(true);
});

test("the query narrows the tree, and an empty one returns it whole", () => {
  expect(ids(queried(""))).toEqual(ids(viewRows()));
  expect(ids(queried("WEBDEV"))).toEqual(["dir:global", "skill:webdev"]);
  expect(ids(queried("statusline")))
    .toEqual(["dir:global", "other:statusline-script"]);
  expect(ids(queried("north"))).toEqual(["dir:north"]);
  expect(queried("no-such-thing")).toHaveLength(0);
});

test("a match brings its ancestors, and a match brings its children", () => {
  // A memory matched by its own name is unreachable without the directory it
  // is addressed through, so the rungs above it come with it.
  expect(ids(queried("report-style"))).toEqual([
    "dir:code", "memroot:code", "mem:code/report-style-recommendations",
  ]);
  const kept = queried("report-style");
  expect(configRowContextOnly(kept[0]!, "report-style")).toBe(true);
  expect(configRowContextOnly(kept[1]!, "report-style")).toBe(true);
  expect(configRowContextOnly(kept[2]!, "report-style")).toBe(false);

  // A matched node is a question about what it carries, so it answers with the
  // whole node.
  expect(ids(queried("fixture/code"))).toEqual([
    "dir:code", "ins:code", "memroot:code",
    "mem:code/report-style-recommendations",
    "mem:code/agents-switchboard-architecture",
  ]);
  // A matched memories gate opens the memories under it and keeps its node.
  expect(ids(queried("MEMORIES"))).toEqual([
    "dir:code", "memroot:code", "mem:code/report-style-recommendations",
    "mem:code/agents-switchboard-architecture",
  ]);
  // A matched hook keeps the skill it is drawn inside, and its node.
  expect(ids(queried("guard"))).toEqual([
    "dir:global", "skill:firn", "hook:firn-guard", "hook:tripwire-guard",
  ]);
  // A matched instruction file keeps its directory and nothing else of it.
  expect(ids(queried("agents.md")))
    .toEqual(["dir:global", "ins:global", "dir:code", "ins:code"]);
  expect(ids(queried("report-style"))).not.toContain("skill:firn");
});

test("filter mode is a state, and the ladder gives the query back before the panel", () => {
  const rung = (filtering: boolean, panel: boolean) =>
    escapeRung(false, filtering, panel, false, false, false);
  expect(rung(true, true)).toBe("clear-filter");
  expect(rung(false, true)).toBe("close-detail");
  // A filter with no panel under it cannot happen, and if it did the query is
  // still the innermost thing there is to take back.
  expect(rung(true, false)).toBe("clear-filter");
});

test("the keystrokes: / opens, characters type, backspace erases then leaves", () => {
  // `/` is a character like any other until a panel is open; inside one it is
  // the verb that starts the search.
  expect(filterKeyAction(false, "", "/", "/")).toBe("open");
  expect(filterKeyAction(false, "", "f", "f")).toBe("");
  expect(filterKeyAction(true, "", "f", "f")).toBe("type");
  expect(filterKeyAction(true, "fi", "r", "r")).toBe("type");
  // A slash typed into a live query is text, not a second filter — and so is
  // an `@`, which outside filter mode is the reference key.
  expect(filterKeyAction(true, "code", "/", "/")).toBe("type");
  expect(filterKeyAction(true, "code", "@", "@")).toBe("type");
  expect(filterKeyAction(true, "fir", "backspace", "")).toBe("erase");
  // The way out from the bottom: one more backspace than there are characters.
  expect(filterKeyAction(true, "", "backspace", "")).toBe("close");
  // Keys the panel already owns are not the filter's to take.
  expect(filterKeyAction(true, "fir", "up", "")).toBe("");
  expect(filterKeyAction(true, "fir", "space", "")).toBe("");
  expect(filterKeyAction(true, "fir", "return", "")).toBe("");

  // What counts as a character: printable, unmodified, and never the space that
  // flips the selected row.
  expect(filterCharacter("f", "f", false, false)).toBe("f");
  expect(filterCharacter("/", "/", false, false)).toBe("/");
  expect(filterCharacter("@", "@", false, false)).toBe("@");
  expect(filterCharacter("space", " ", false, false)).toBe("");
  expect(filterCharacter("f", "f", true, false)).toBe("");
  expect(filterCharacter("f", "f", false, true)).toBe("");
  expect(filterCharacter("up", "[A", false, false)).toBe("");
  expect(filterCharacter("return", "\r", false, false)).toBe("");
});

test("the fold keys are the tree convention, and nothing else", () => {
  // `l` opens the node you are on and has nothing to say anywhere else; `h`
  // shuts it, or climbs to it from inside.
  expect(foldKeyAction(true, false, true)).toBe("expand");
  expect(foldKeyAction(true, true, true)).toBe("");
  expect(foldKeyAction(true, true, false)).toBe("collapse");
  expect(foldKeyAction(true, false, false)).toBe("");
  expect(foldKeyAction(false, true, false)).toBe("climb");
  expect(foldKeyAction(false, true, true)).toBe("");
});

test("every node opens folded, and opening one shows only its own rows", () => {
  const tree = viewRows();
  expect(configNodeExpanded([], "global")).toBe(false);
  expect(configNodeExpanded(["global"], "global")).toBe(true);

  // Folded: which scopes exist, answered before what is in them.
  expect(ids(configFoldRows(tree, []) as Row[]))
    .toEqual(["dir:global", "dir:code", "dir:north"]);

  // One node open is one node's rows: a fold is per-directory and not a mode.
  expect(ids(configFoldRows(tree, ["code"]) as Row[])).toEqual([
    "dir:global", "dir:code", "ins:code", "memroot:code",
    "mem:code/report-style-recommendations",
    "mem:code/agents-switchboard-architecture", "dir:north",
  ]);
  expect(configFoldRows(tree, ["global", "code", "north"])).toHaveLength(tree.length);
});

function configRuntime(entries: Row[], view: string, index = 0,
                       query: string | null = null, expanded: string[] = [],
                       focused = true) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configAllEntries: entries.slice(),
    configMemberships: [],
    configFilter: view,
    configIndex: index,
    configLoaded: true,
    expandedDirs: expanded,
    panelFocused: focused,
    panelFiltering: query !== null,
    panelQuery: query ?? "",
  };
}

test("a query outranks the fold, and the fold survives the query", () => {
  const runtime = configRuntime(MANIFEST, "all");
  // Folded shut, the panel is three directories.
  expect(configPanelRows(runtime)).toHaveLength(3);

  // A match opens whatever it was buried under, without touching the fold.
  setPanelQuery(runtime, "report-style");
  expect(ids(configPanelRows(runtime) as Row[])).toEqual([
    "dir:code", "memroot:code", "mem:code/report-style-recommendations",
  ]);
  expect(runtime.expandedDirs).toEqual([]);

  // Clearing the query gives the fold state back exactly as it was.
  clearPanelFilter(runtime);
  expect(configPanelRows(runtime)).toHaveLength(3);

  // …and a node opened by hand stays open across a query.
  setNodeExpanded(runtime, "code", true);
  expect(configPanelRows(runtime)).toHaveLength(7);
  setPanelQuery(runtime, "webdev");
  expect(ids(configPanelRows(runtime) as Row[]))
    .toEqual(["dir:global", "skill:webdev"]);
  clearPanelFilter(runtime);
  expect(configPanelRows(runtime)).toHaveLength(7);
  setNodeExpanded(runtime, "code", false);
  expect(configPanelRows(runtime)).toHaveLength(3);
});

test("the cursor belongs to the visible rows, and comes back inside when they shrink", () => {
  const runtime = configRuntime(MANIFEST, "all", 2, null, ["global", "code"]);
  expect(configPanelRows(runtime)).toHaveLength(viewRows().length);

  // Typing narrows the list under a cursor that was pointing past its new end.
  runtime.configIndex = 11;
  setPanelQuery(runtime, "report-style");
  expect(configPanelRows(runtime)).toHaveLength(3);
  expect(runtime.configIndex).toBe(2);

  // A query matching nothing parks it at zero rather than at minus one.
  setPanelQuery(runtime, "no-such-thing");
  expect(configPanelRows(runtime)).toHaveLength(0);
  expect(runtime.configIndex).toBe(0);

  // Folding a node shut takes rows away under the cursor too.
  clearPanelFilter(runtime);
  runtime.configIndex = 12;
  setNodeExpanded(runtime, "code", false);
  expect(runtime.configIndex).toBeLessThan(configPanelRows(runtime).length);
});

test("the panel is as tall as what the filter left it", () => {
  const wide = configRuntime(MANIFEST, "all", 0, null, ["global"]);
  const narrow = configRuntime(MANIFEST, "all", 0, "guard");
  // Four rows, and the three headings they are under — SKILLS, its MODULES
  // subsection, HOOKS — plus the title line.
  expect(configDetailLines(narrow)).toBe(1 + 4 + 3);
  expect(configDetailLines(narrow)).toBeLessThan(configDetailLines(wide));
  expect(detailHeight(narrow)).toBeLessThanOrEqual(PANEL_BUDGET);

  // The header field and the legend are the header line, and it says which
  // rung escape is on.
  expect(configQueryField(true, "gua")).toBe("  /gua");
  expect(configQueryField(true, "")).toBe("  /");
  expect(configQueryField(false, "gua")).toBe("");
  expect(configPanelLegend(true)).toContain("esc clears filter");
  expect(configPanelLegend(false)).toContain("/ filter");
  expect(configPanelLegend(false)).toContain("esc close");
});

async function frameOf(runtime: unknown, height = 26) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderDetailPanel(runtime);
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();
  return frame.split("\n").map((l) => l.trimEnd()).filter((l) => l !== "");
}

test("a filtered switchboard shows the query, the matches, and the headings that survived", async () => {
  const before = await frameOf(configRuntime(MANIFEST, "all", 0, null, ["global"]));
  expect(before[0]).toBe(
    "context switchboard  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close");
  for (const heading of ["SETS", "SKILLS", "MODULES", "HOOKS", "OTHER"]) {
    expect(before.some((l) => l.trim() === heading)).toBe(true);
  }
  // Directories are the root level, under nothing.
  expect(before.some((l) => l.trim() === "DIRECTORY")).toBe(false);

  const after = await frameOf(configRuntime(MANIFEST, "all", 0, "guard"));
  expect(after).toEqual([
    "context switchboard  /guard  ↑/↓ move · tab fold · space toggle · enter edit · esc clears filter",
    "› ▾ GLOBAL: on",
    "    SKILLS",
    "      MODULES",
    "        firn: on",
    "          hook · firn-guard: on",
    "    HOOKS",
    "      tripwire-guard: disabled",
  ]);
  // Every heading with nothing left prints nothing: headings go with the rows
  // they head.
  for (const heading of ["SETS", "OTHER", "PLUGINS"]) {
    expect(after.some((l) => l.trim() === heading)).toBe(false);
  }
});

test("a matching memory keeps the directory it hangs off, nested as it was", async () => {
  const frame = await frameOf(configRuntime(MANIFEST, "all", 0, "report-style"));
  expect(frame).toEqual([
    "context switchboard  /report-style  ↑/↓ move · tab fold · space toggle · enter edit · esc clears filter",
    "› ▾ /tmp/switchboard-fixture/code: on",
    "    MEMORIES: on",
    "      report-style-recommendations: on",
  ]);

  // The other direction: a matched node answers with its whole subtree,
  // indentation and all, however folded it was.
  const parent = await frameOf(configRuntime(MANIFEST, "agentsmd", 0, "fixture/code"));
  expect(parent).toEqual([
    "directory context  /fixture/code  ↑/↓ move · tab fold · space toggle · enter edit · esc clears filter",
    "› ▾ /tmp/switchboard-fixture/code: on",
    "    AGENTS.md: on",
    "    MEMORIES: on",
    "      report-style-recommendations: on",
    "      agents-switchboard-architecture: on",
  ]);
});

test("a query that matches nothing says so, and keeps the field to back out of", async () => {
  const frame = await frameOf(configRuntime(MANIFEST, "all", 0, "zzz"));
  expect(frame).toEqual(["context switchboard  /zzz nothing matches"]);
  // Not "loading…", which is what an unloaded panel says and this one is not.
  expect(frame[0]).not.toContain("loading");
});

test("folding renders as folding: a marker, and the rows that came with it", async () => {
  const shut = await frameOf(configRuntime(MANIFEST, "all"));
  expect(shut).toEqual([
    "context switchboard  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close",
    "› ▸ GLOBAL: on",
    "  ▸ /tmp/switchboard-fixture/code: on",
    "  ▸ /tmp/switchboard-fixture/north: on",
  ]);

  const open = await frameOf(configRuntime(MANIFEST, "all", 1, null, ["code"]));
  expect(open).toEqual([
    "context switchboard  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close",
    "  ▸ GLOBAL: on",
    "› ▾ /tmp/switchboard-fixture/code: on",
    "    AGENTS.md: on",
    "    MEMORIES: on",
    "      report-style-recommendations: on",
    "      agents-switchboard-architecture: on",
    "  ▸ /tmp/switchboard-fixture/north: on",
  ]);
});

test("help is rows too, so the same slash narrows it", async () => {
  expect(helpQueryRows("").length).toBeGreaterThan(5);
  expect(helpQueryRows("sound")).toHaveLength(1);
  expect(helpQueryRows("ESC").length).toBeGreaterThanOrEqual(2);

  const open = await frameOf({ detailView: "help" });
  expect(open[0]).toBe("Northbridge keys · / filter · esc closes");

  const filtered = await frameOf({
    detailView: "help", panelFiltering: true, panelQuery: "sound",
  });
  expect(filtered).toEqual([
    "Northbridge keys  /sound · esc clears filter",
    "/sound on|off|pack    voice lines",
  ]);

  const empty = await frameOf({
    detailView: "help", panelFiltering: true, panelQuery: "zzz",
  });
  expect(empty).toEqual(["Northbridge keys  /zzz · esc clears filter", " nothing matches"]);
});

test("@ writes the selected row into the sentence, kind first", () => {
  // The kind comes first for the same reason it does in the provenance column:
  // `@repo-safety` alone is a name with no idea what it is.
  expect(configReferenceText("skill", "repo-safety")).toBe("@skill:repo-safety ");
  expect(configReferenceText("hook", "worktree-guard")).toBe("@hook:worktree-guard ");
  expect(configReferenceText("module", "dev-core")).toBe("@module:dev-core ");
  expect(configReferenceText("dir", "code")).toBe("@dir:code ");
  // The subtree rows use the word the panel uses for them and the address the
  // CLI answers to — which is the qualified one, through the directory gate.
  expect(configReferenceText("ins", "code")).toBe("@file:code/AGENTS.md ");
  expect(configReferenceText("memroot", "code")).toBe("@memories:code/memories ");
  expect(configReferenceText("mem", "code/report-style-recommendations.md"))
    .toBe("@memory:code/report-style-recommendations.md ");
  // The trailing space is part of it: the next thing typed is the rest of the
  // sentence, not more of the reference.
  expect(configReferenceText("skill", "firn").endsWith(" ")).toBe(true);
});

// Styles, not characters: the frame text cannot say which rows recede, and
// receding is the whole point of the change.
async function spansOf(runtime: unknown, height = 26) {
  const { renderer, renderOnce, captureSpans } = await createTestRenderer({
    width: 110, height,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderDetailPanel(runtime);
  await renderOnce();
  const frame = captureSpans();
  renderer.destroy();
  return frame.lines.map((line) => line.spans.map((span) => ({
    text: span.text,
    fg: [span.fg.r, span.fg.g, span.fg.b].map((c) => Math.round(c * 255)).join(","),
    dim: (span.attributes & 2) !== 0,
  })));
}

const spanWith = (lines: Awaited<ReturnType<typeof spansOf>>, needle: string) => {
  const line = lines.find((spans) => spans.some((s) => s.text.includes(needle)));
  expect(line).toBeDefined();
  return line!.filter((s) => s.text.trim() !== "");
};

const BRIGHT_BLACK = "102,102,102";
const BRIGHT_GREEN = "102,255,102";
const BRIGHT_CYAN = "102,255,255";

test("what you turned off recedes: a pinned hook is the dimmest row on screen", async () => {
  const lines = await spansOf(configRuntime(MANIFEST, "all", 0, null, ["global"]));

  // A running hook keeps its colour and its normal weight.
  const running = spanWith(lines, "firn-guard");
  expect(running.some((s) => s.fg === BRIGHT_GREEN && !s.dim)).toBe(true);
  // Everything but the kind tag, which is dim on every row that carries one:
  // the tag says what sort of thing this is, and the name stays the loud part.
  expect(running.filter((s) => !s.text.includes("hook · ")).every((s) => !s.dim))
    .toBe(true);
  expect(running.filter((s) => s.text.includes("hook · ")).every((s) => s.dim))
    .toBe(true);

  // A hook the user pinned off is dim end to end — the state column AND the
  // name — instead of wearing the warning colour it used to shout in.
  const pinned = spanWith(lines, "tripwire-guard");
  expect(pinned.every((s) => s.dim)).toBe(true);
  expect(pinned.every((s) => s.fg === BRIGHT_BLACK)).toBe(true);

  // Gated-off sits between them: dim tone, no dim attribute, and it still
  // carries the provenance that says why.
  const gated = spanWith(await spansOf(configRuntime(
    [...MANIFEST, row("dir", "held", "off", "/tmp/switchboard-fixture/held"),
     row("ins", "held", "on")],
    "agentsmd", 0)), "AGENTS.md");
  expect(gated.every((s) => !s.dim)).toBe(true);
});

test("a row kept only for context is dimmer than the match it is holding up", async () => {
  const lines = await spansOf(configRuntime(MANIFEST, "all", 0, "report-style"));

  // The matched row is the answer, at the panel's normal weight.
  const match = spanWith(lines, "report-style-recommendations");
  expect(match.every((s) => !s.dim)).toBe(true);

  // The memories gate is only there so the match has an address: its name
  // recedes, while its state column keeps the colour that says it is on.
  const context = spanWith(lines, "MEMORIES");
  expect(context.some((s) => s.text.includes("MEMORIES") && s.dim)).toBe(true);
  expect(context.some((s) => s.fg === BRIGHT_GREEN && !s.dim)).toBe(true);
});

test("an unfocused panel stops burning its cursor, and says so without moving", async () => {
  const focused = await spansOf(configRuntime(MANIFEST, "all", 0, null, [], true));
  const marker = focused[1]!.find((s) => s.text.includes("›"))!;
  expect(marker.fg).toBe(BRIGHT_CYAN);

  // `@` hands the keyboard back to the composer with the panel still open. The
  // rows do not move; the cursor stops claiming to be yours.
  const handed = await spansOf(configRuntime(MANIFEST, "all", 0, null, [], false));
  const quiet = handed[1]!.find((s) => s.text.includes("›"))!;
  expect(quiet.fg).toBe(BRIGHT_BLACK);
  expect(await frameOf(configRuntime(MANIFEST, "all", 0, null, [], false)))
    .toEqual(await frameOf(configRuntime(MANIFEST, "all", 0, null, [], true)));
});

test("the cursor still finds its row, and space still has a row to flip", async () => {
  // Cursor on the second surviving row rather than the second row of the view.
  const runtime = configRuntime(MANIFEST, "all", 3, "guard");
  const frame = await frameOf(runtime);
  expect(frame).toContain("›     tripwire-guard: disabled");
  expect((configPanelRows(runtime) as Row[])[3]!.name).toBe("tripwire-guard");

  // An index past the end of the filtered rows still renders a cursor, on the
  // last row there is.
  const overrun = await frameOf(configRuntime(MANIFEST, "all", 99, "guard"));
  expect(overrun).toContain("›     tripwire-guard: disabled");
});

// --- Focus decides what a key means -----------------------------------------
//
// The switchboard had no discoverable way to unfold: ←/→ and h/l are
// conventions you either arrive knowing or never find, and the legend named
// neither. Tab is the discoverable key, which means tab now carries two verbs —
// so the verb it carries has to be a function of focus and nothing else.

test("exactly one surface holds the keyboard, innermost first", () => {
  // palette, panel + filter, panel, strip, composer.
  expect(activeFocus(true, true, true, true, true)).toBe("palette");
  expect(activeFocus(false, true, true, true, true)).toBe("filter");
  expect(activeFocus(false, true, true, false, true)).toBe("panel");
  expect(activeFocus(false, false, false, false, true)).toBe("strip");
  expect(activeFocus(false, false, false, false, false)).toBe("composer");
  // An open panel nobody is focused on does not hold anything: you went back to
  // your sentence and the panel stayed on screen behind it.
  expect(activeFocus(false, true, false, false, false)).toBe("composer");
  // Nor does a filter flag on a panel that is not focused.
  expect(activeFocus(false, true, false, true, false)).toBe("composer");
});

test("tab means the fold in the panel and the view swap in the composer", () => {
  // Panel, on a node: the toggle, both directions.
  expect(tabAction("panel", true, false)).toBe("expand");
  expect(tabAction("panel", true, true)).toBe("collapse");
  // Panel, on a row inside a node: climb to the node, so the next press folds
  // it. A legend key must move something wherever the cursor stands.
  expect(tabAction("panel", false, false)).toBe("climb");
  expect(tabAction("panel", false, true)).toBe("climb");
  // A live filter keeps folding: tab is never a character, unlike the h and l
  // that have to stand down while a query is being typed.
  expect(tabAction("filter", true, false)).toBe("expand");
  expect(tabAction("filter", true, true)).toBe("collapse");
  expect(tabAction("filter", false, false)).toBe("climb");
  // Everywhere else it is what it has always been.
  expect(tabAction("composer", true, true)).toBe("swap-view");
  expect(tabAction("strip", true, true)).toBe("swap-view");
  expect(tabAction("palette", true, true)).toBe("complete");
  // One key, one meaning per context, and never two at once.
  for (const focus of ["palette", "filter", "panel", "strip", "composer"])
    expect(new Set([tabAction(focus, true, true)]).size).toBe(1);
});

test("the legend advertises the fold, and names tab for it", () => {
  expect(configPanelLegend(false))
    .toBe("  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close");
  expect(configPanelLegend(true))
    .toBe("  ↑/↓ move · tab fold · space toggle · enter edit · esc clears filter");
  // Named before the other things a row can do, because it is the affordance
  // that was invisible.
  for (const filtering of [false, true]) {
    const legend = configPanelLegend(filtering) as string;
    expect(legend).toContain("tab fold");
    expect(legend.indexOf("tab fold")).toBeLessThan(legend.indexOf("space toggle"));
  }
});

test("GLOBAL is open before you open anything, and shuts when you shut it", async () => {
  // A runtime that has never touched the fold: the profile every other node is
  // read on top of is already showing.
  const fresh = configRuntime(MANIFEST, "all");
  delete (fresh as { expandedDirs?: unknown }).expandedDirs;
  const rows = configPanelRows(fresh) as Row[];
  expect(rows.some((r) => r.kind === "dir" && r.name === "global")).toBe(true);
  expect(rows.length).toBeGreaterThan(3);

  const frame = await frameOf(fresh);
  expect(frame).toContain("› ▾ GLOBAL: on");
  expect(frame).not.toContain("▸ GLOBAL");

  // And it is a default, not a floor: shutting it sticks.
  setNodeExpanded(fresh, "global", false);
  expect(fresh.expandedDirs).toEqual([]);
  expect(configPanelRows(fresh)).toHaveLength(3);
});

// A row under a heading that already names its kind says nothing extra; a row
// nested inside another UNIT has to say what it is, because the heading over it
// names its parent's kind and not its own.
test("a row names its kind only where nothing above it already did", () => {
  expect(configKindTag("hook", "boundhook")).toBe("hook · ");
  expect(configKindTag("hook", "hook")).toBe("");
  expect(configKindTag("skill", "skill")).toBe("");
  // A module row IS a skill — one that brings hooks — and it reads under
  // MODULES, which is the heading its role names. No tag.
  expect(configKindTag("skill", "module")).toBe("");
  expect(configKindTag("module", "moduleset")).toBe("");
  expect(configKindTag("plugin", "plugin")).toBe("");
  // Directories head nothing at all: they are the panel's root level, and the
  // position says what they are.
  expect(configKindTag("dir", "dir")).toBe("");
  expect(configKindTag("ins", "ins")).toBe("");
  expect(configKindTag("mem", "mem")).toBe("");
});

test("tab folds the node under the cursor, on screen, both ways", async () => {
  const runtime = configRuntime(MANIFEST, "all", 1, null, []);
  // Cursor on the second directory, everything shut.
  expect(await frameOf(runtime)).toEqual([
    "context switchboard  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close",
    "  ▸ GLOBAL: on",
    "› ▸ /tmp/switchboard-fixture/code: on",
    "  ▸ /tmp/switchboard-fixture/north: on",
  ]);

  tabFoldStep(runtime);
  const opened = await frameOf(runtime);
  expect(opened).toContain("› ▾ /tmp/switchboard-fixture/code: on");
  expect(opened).toContain("    AGENTS.md: on");
  // The node next door did not move.
  expect(opened).toContain("  ▸ GLOBAL: on");

  // The same key shuts it again — one verb, both directions.
  tabFoldStep(runtime);
  expect(await frameOf(runtime)).toContain("› ▸ /tmp/switchboard-fixture/code: on");

  // From a row INSIDE a node, tab climbs to the node rather than doing nothing,
  // and the next press folds what it landed on.
  tabFoldStep(runtime);
  const inside = configRuntime(MANIFEST, "all", 3, null, ["code"]);
  expect((configPanelRows(inside) as Row[])[3]!.kind).not.toBe("dir");
  tabFoldStep(inside);
  expect(inside.configIndex).toBe(1);
  expect((configPanelRows(inside) as Row[])[1]!.name).toBe("code");
  tabFoldStep(inside);
  expect(inside.expandedDirs).toEqual([]);
});

test("tab in the composer swaps the view, and never folds anything", async () => {
  expect(tabSwapFrame("agents")).toBe("threads");
  expect(tabSwapFrame("threads")).toBe("agents");

  // The panel's fold state is untouched by the composer's tab: the two verbs
  // live on different surfaces and neither reaches the other.
  const runtime = configRuntime(MANIFEST, "all", 0, null, ["code"]);
  const before = await frameOf(runtime);
  tabSwapFrame("agents");
  expect(await frameOf(runtime)).toEqual(before);
  expect(runtime.expandedDirs).toEqual(["code"]);
});
