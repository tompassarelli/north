import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  config_detail_lines as configDetailLines,
  config_header_roles as configHeaderRoles,
  config_row_role as configRowRole,
  config_section_rows as configSectionRows,
  config_section_title as configSectionTitle,
  config_view_folds_p as configViewFolds,
  config_view_includes_p as configViewIncludes,
  config_view_rows as configViewRows,
  config_visible_count as configVisibleCount,
  detail_height as detailHeight,
  render_config_panel as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";

// The panel reads the terminal for its window math. Pin both dimensions so the
// height assertions below are arithmetic, not a property of whoever's tty ran
// the suite.
const ROWS = 40;
const COLUMNS = 120;
Object.defineProperty(process.stdout, "rows", { value: ROWS, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: COLUMNS, configurable: true });

// The layout reserves these before the docked panel gets a row: the frame
// chrome and the workspace floor. A panel taller than the difference eats the
// workspace's minimum. Chrome is four rows since the view bar moved down under
// the composer and absorbed the session context line: padding, composer, view
// bar, agent strip.
const CHROME_ROWS = 4;
const MIN_WORKSPACE_ROWS = 4;
const PANEL_BUDGET = ROWS - CHROME_ROWS - MIN_WORKSPACE_ROWS;

type Row = { kind: string; name: string; state: string; detail: string };

function row(kind: string, name: string, state = "off", detail = ""): Row {
  return { kind, name, state, detail };
}

// Manifest order, not reading order: `agents` appends, so a module really does
// land after the skills block, a `dir` row lands wherever it was registered,
// and the global profile — a `dir` row whose directory is the root scope `~` —
// is registered no earlier than any other.
const MANIFEST: Row[] = [
  row("dir", "code", "on", "/tmp/switchboard-fixture/code"),
  row("hook", "firn-guard", "enabled", "firn"),
  row("hook", "worktree-guard", "enabled"),
  row("skill", "webdev"),
  row("skill", "firn", "on"),
  row("module", "orchestration", "on"),
  row("other", "statusline-script"),
  row("plugin", "typescript-lsp@claude-plugins-official", "on"),
  row("dir", "global", "on", "~"),
  row("dir", "north", "on", "/tmp/switchboard-fixture/north"),
  row("dir", "nixos-config", "off", "/tmp/switchboard-fixture/nixos-config"),
];

const kinds = (rows: Row[]) => rows.map((r) => r.kind);
const names = (rows: Row[]) => rows.map((r) => r.name);

test("every view is a projection of one tree, narrowed by what it admits", () => {
  // A directory is admitted everywhere, because a row with no node over it has
  // nowhere to hang; the nodes that end up holding nothing are pruned later.
  expect(configViewIncludes("hook", "dir", "code")).toBe(true);
  expect(configViewIncludes("hook", "hook", "firn-guard")).toBe(true);
  expect(configViewIncludes("hook", "skill", "firn")).toBe(false);
  expect(configViewIncludes("module", "module", "orchestration")).toBe(true);
  expect(configViewIncludes("module", "other", "statusline-script")).toBe(false);
  expect(configViewIncludes("all", "dir", "north")).toBe(true);

  // /globals is the root node alone with everything scoped to it, minus the
  // packages that are somebody else's.
  expect(configViewIncludes("globals", "dir", "global")).toBe(true);
  expect(configViewIncludes("globals", "dir", "north")).toBe(false);
  expect(configViewIncludes("globals", "hook", "firn-guard")).toBe(true);
  expect(configViewIncludes("globals", "skill", "firn")).toBe(true);
  expect(configViewIncludes("globals", "module", "orchestration")).toBe(true);
  expect(configViewIncludes("globals", "other", "statusline-script")).toBe(true);
  expect(configViewIncludes("globals", "plugin", "typescript-lsp")).toBe(false);

  // /agentsmd answers "what context exists and which of it is active", which is
  // every node and the files it carries — and nothing that a node turns on.
  expect(configViewIncludes("agentsmd", "dir", "global")).toBe(true);
  expect(configViewIncludes("agentsmd", "dir", "north")).toBe(true);
  expect(configViewIncludes("agentsmd", "ins", "north")).toBe(true);
  expect(configViewIncludes("agentsmd", "other", "statusline-script")).toBe(false);
  expect(configViewIncludes("agentsmd", "module", "orchestration")).toBe(false);
  expect(configViewIncludes("agentsmd", "hook", "firn-guard")).toBe(false);
});

test("the tree is directories first, each carrying what is scoped to it", () => {
  const all = configViewRows(MANIFEST.slice(), "all") as Row[];
  expect(kinds(all)).toEqual([
    "dir",
    "module",
    "skill", "hook",
    "skill",
    "hook",
    "plugin",
    "other",
    "dir", "dir", "dir",
  ]);
  // The root node reads first wherever the CLI appended it, and the other
  // directories keep their manifest order behind it.
  expect(names(all).filter((_, i) => all[i]!.kind === "dir"))
    .toEqual(["global", "code", "north", "nixos-config"]);
  // A skill some hook follows is a module, and its hook is drawn inside it.
  expect(names(all).slice(2, 4)).toEqual(["firn", "firn-guard"]);
  expect(configRowRole(all[2]!, all)).toBe("module");
  expect(configRowRole(all[3]!, all)).toBe("boundhook");
  expect(configRowRole(all[4]!, all)).toBe("skill");
  expect(configRowRole(all[5]!, all)).toBe("hook");
  expect(configRowRole(all[1]!, all)).toBe("moduleset");

  const globals = configViewRows(MANIFEST.slice(), "globals") as Row[];
  expect(kinds(globals)).toEqual([
    "dir", "module", "skill", "hook", "skill", "hook", "other",
  ]);
  expect(names(globals)[0]).toBe("global");

  const agentsmd = configViewRows(MANIFEST.slice(), "agentsmd") as Row[];
  expect(names(agentsmd)).toEqual(["global", "code", "north", "nixos-config"]);

  // A narrow view is the same tree with one kind admitted, and it prints only
  // the nodes that answer: an empty heading is a row spent saying nothing.
  const plugins = configViewRows(MANIFEST.slice(), "plugin") as Row[];
  expect(names(plugins)).toEqual(["global", "typescript-lsp@claude-plugins-official"]);
  const modules = configViewRows(MANIFEST.slice(), "module") as Row[];
  expect(names(modules)).toEqual(["global", "orchestration"]);

  // A kind may be absent entirely until the CLI registers one; the empty-view
  // path has to be reachable rather than an exception.
  const noDirs = MANIFEST.filter((r) => r.kind !== "dir");
  expect(configViewRows(noDirs.slice(), "agentsmd")).toHaveLength(0);
});

test("MODULES is a subsection of SKILLS, and the headings say so", () => {
  // The stack inside a node, in the order it reads.
  expect(configSectionTitle("dir")).toBe("DIRECTORY");
  expect(configSectionTitle("moduleset")).toBe("MODULE SETS");
  expect(configSectionTitle("module")).toBe("MODULES");
  expect(configSectionTitle("skill")).toBe("SKILLS");
  expect(configSectionTitle("hook")).toBe("HOOKS");
  expect(configSectionTitle("plugin")).toBe("PLUGINS");
  expect(configSectionTitle("other")).toBe("OTHER");
  // A directory's own two files are rows with a switch each, under no heading
  // but DIRECTORY.
  for (const role of ["ins", "memroot", "mem"]) {
    expect(configSectionTitle(role)).toBe("");
    expect(configHeaderRoles(role)).toEqual(["dir"]);
  }
  // A module is a skill that brings hooks, so it reads inside SKILLS; its hooks
  // read inside it and head nothing of their own.
  expect(configHeaderRoles("module")).toEqual(["dir", "skill", "module"]);
  expect(configHeaderRoles("skill")).toEqual(["dir", "skill"]);
  expect(configHeaderRoles("boundhook")).toEqual(["dir", "skill", "module"]);
  expect(configHeaderRoles("hook")).toEqual(["dir", "hook"]);
});

test("the heading budget covers what a view can print", () => {
  // DIRECTORY plus the six headings a node can carry: MODULE SETS, SKILLS, its
  // MODULES subsection, HOOKS, PLUGINS, OTHER.
  expect(configSectionRows("all")).toBe(7);
  // /globals is the root node without plugins.
  expect(configSectionRows("globals")).toBe(6);
  // /agentsmd is DIRECTORY and the rows under it.
  expect(configSectionRows("agentsmd")).toBe(1);
  // A kind view is DIRECTORY plus that kind's own heading.
  expect(configSectionRows("hook")).toBe(2);
  expect(configSectionRows("module")).toBe(2);

  // A narrowed view keeps the rows the full switchboard has to give up.
  const total = 100;
  expect(configVisibleCount(total, "hook")).toBeGreaterThan(configVisibleCount(total, "globals"));
  expect(configVisibleCount(total, "globals")).toBeGreaterThan(configVisibleCount(total, "all"));
  expect(configVisibleCount(total, "all")).toBe(ROWS - CHROME_ROWS - MIN_WORKSPACE_ROWS - 3 - 7);

  // Folding is for the view that shows everything; a narrowed view is already
  // an answer and would answer nothing folded shut.
  expect(configViewFolds("all")).toBe(true);
  for (const view of ["globals", "agentsmd", "hook", "skill", "module", "plugin"]) {
    expect(configViewFolds(view)).toBe(false);
  }
});

function configRuntime(entries: Row[], view: string, expanded: string[] = []) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configAllEntries: entries.slice(),
    configMemberships: [],
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
    expandedDirs: expanded,
  };
}

test("a node with every heading in it still fits the docked panel", () => {
  // Long enough that the window is what limits the panel, so the heading budget
  // is load-bearing rather than slack.
  // Sized so the window still reaches the end of the root node's stack: every
  // one of the seven headings is inside it, and the directories past it are
  // what push the list beyond what the window can hold.
  const long: Row[] = [row("dir", "global", "on", "~")];
  for (let i = 0; i < 10; i += 1) long.push(row("dir", `dir-${i}`, "on", `/tmp/switchboard-fixture/d${i}`));
  for (let i = 0; i < 3; i += 1) long.push(row("skill", `skill-${i}`));
  long.push(row("skill", "carrier", "on"));
  for (let i = 0; i < 3; i += 1) long.push(row("hook", `hook-${i}`, "enabled", "carrier"));
  for (let i = 0; i < 3; i += 1) long.push(row("hook", `loose-${i}`, "enabled"));
  for (let i = 0; i < 3; i += 1) long.push(row("module", `module-${i}`));
  for (let i = 0; i < 3; i += 1) long.push(row("plugin", `plugin-${i}`));
  for (let i = 0; i < 2; i += 1) long.push(row("other", `other-${i}`));

  const runtime = configRuntime(long, "all", ["global"]);
  const rows = configViewRows(long.slice(), "all") as Row[];
  const window = configVisibleCount(rows.length, "all");
  expect(window).toBeLessThan(rows.length);

  // Title line + windowed rows + the headings inside the window. Every one of
  // the seven is in it, which is what makes the budget load-bearing.
  expect(configDetailLines(runtime)).toBe(1 + window + 7);
  // The number the layout actually spends: the extra heading row is paid for by
  // one fewer list row, so the panel is exactly as tall as it always was.
  expect(detailHeight(runtime)).toBe(PANEL_BUDGET);

  // The narrowed views sit under the same ceiling.
  for (const view of ["globals", "agentsmd", "module"]) {
    expect(detailHeight(configRuntime(long, view))).toBeLessThanOrEqual(PANEL_BUDGET);
  }
  // And so does the folded default, which is a handful of rows.
  expect(detailHeight(configRuntime(long, "all"))).toBeLessThanOrEqual(PANEL_BUDGET);
});

async function frameOf(view: string, entries: Row[] = MANIFEST,
                       expanded: string[] = []) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 24,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(configRuntime(entries, view, expanded));
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();
  return frame;
}

test("/globals is the root node, expanded, with everything scoped to it", async () => {
  const frame = await frameOf("globals");
  expect(frame).toContain("globals");
  expect(frame).toContain("DIRECTORY");
  expect(frame).toContain("global  ~");
  expect(frame).toContain("MODULE SETS");
  expect(frame).toContain("SKILLS");
  expect(frame).toContain("MODULES");
  expect(frame).toContain("HOOKS");
  expect(frame).toContain("OTHER");
  expect(frame).toContain("orchestration");
  expect(frame).toContain("statusline-script");
  expect(frame).toContain("firn-guard");
  // Per-directory nodes are not global knobs, and plugins are not ours.
  expect(frame).not.toContain("PLUGINS");
  expect(frame).not.toContain("nixos-config");
  expect(frame).not.toContain("/tmp/switchboard-fixture/north");
});

test("/agentsmd is every node and the files it carries", async () => {
  const frame = await frameOf("agentsmd");
  expect(frame).toContain("directory context");
  expect(frame).toContain("DIRECTORY");
  // The manifest token is not the heading, and neither is the old name.
  expect(frame).not.toContain("DIRECTORY INSTRUCTIONS");
  expect(frame).not.toContain("agents.md & directory context");
  // The root scope reads above the narrower scopes layered on it.
  expect(frame.indexOf("global  ~")).toBeGreaterThanOrEqual(0);
  expect(frame.indexOf("global  ~")).toBeLessThan(frame.indexOf("north  /tmp"));
  // Slug plus state plus the directory the CLI hands over, on one row.
  expect(frame).toContain("north  /tmp/switchboard-fixture/north");
  expect(frame).toContain("nixos-config  /tmp/switchboard-fixture/nixos-config");
  expect(frame).toContain("on ");
  expect(frame).toContain("off ");
  // Nothing a node turns on belongs in this view.
  for (const header of ["SKILLS", "HOOKS", "MODULE SETS", "PLUGINS", "OTHER"]) {
    expect(frame).not.toContain(header);
  }
  expect(frame).not.toContain("statusline-script");
});

test("/config opens as a list of directories, and expands into the stack", async () => {
  // Folded is the default: which scopes exist, answered before what is in them.
  const folded = await frameOf("all");
  expect(folded).toContain("context switchboard");
  expect(folded).toContain("DIRECTORY");
  expect(folded).toContain("▸ global  ~");
  expect(folded).toContain("▸ code  /tmp/switchboard-fixture/code");
  for (const header of ["MODULE SETS", "SKILLS", "HOOKS", "PLUGINS", "OTHER"]) {
    expect(folded).not.toContain(header);
  }

  const open = await frameOf("all", MANIFEST, ["global"]);
  const order = ["DIRECTORY", "MODULE SETS", "SKILLS", "MODULES", "HOOKS",
                 "PLUGINS", "OTHER"];
  const seen = order.map((header) => {
    const at = open.indexOf(header);
    expect(at).toBeGreaterThanOrEqual(0);
    return at;
  });
  expect(seen).toEqual([...seen].sort((a, b) => a - b));
  expect(open).toContain("▾ global  ~");
  // The headings the taxonomy and the rename retired.
  expect(open).not.toContain("GLOBALS");
  expect(open).not.toContain("DIRECTORY INSTRUCTIONS");
});

test("/modules narrows to the module sets, under the node that holds them", async () => {
  const frame = await frameOf("module");
  expect(frame).toContain("modules");
  expect(frame).toContain("orchestration");
  expect(frame).toContain("MODULE SETS");
  // A narrow view carries the node its rows are in and no other kind's rows.
  expect(frame).toContain("global  ~");
  expect(frame).not.toContain("firn-guard");
  expect(frame).not.toContain("statusline-script");
  // …and no node that has none of them.
  expect(frame).not.toContain("north");
});

test("a view with no rows says so instead of claiming to be loading", async () => {
  const frame = await frameOf("agentsmd", [
    row("other", "statusline-script"), row("hook", "firn-guard"),
  ]);
  expect(frame).toContain("nothing to configure here");
  expect(frame).not.toContain("loading");
});
