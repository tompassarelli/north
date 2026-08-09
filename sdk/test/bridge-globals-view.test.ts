import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  config_detail_lines as configDetailLines,
  config_section_rows as configSectionRows,
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
// workspace's minimum.
const CHROME_ROWS = 4;
const MIN_WORKSPACE_ROWS = 4;
const PANEL_BUDGET = ROWS - CHROME_ROWS - MIN_WORKSPACE_ROWS;

type Row = { kind: string; name: string; state: string; detail: string };

function row(kind: string, name: string, state = "off", detail = ""): Row {
  return { kind, name, state, detail };
}

// Manifest order, not reading order: `agents` appends, so `item orchestration`
// really does land after the skills block, and a `dir` row lands wherever it
// was registered.
const MANIFEST: Row[] = [
  row("item", "agents-md", "on"),
  row("item", "code-md"),
  row("item", "statusline"),
  row("hook", "firn-guard", "on"),
  row("hook", "worktree-guard"),
  row("skill", "webdev"),
  row("skill", "firn", "on"),
  row("item", "orchestration", "on"),
  row("plugin", "typescript-lsp@claude-plugins-official", "on"),
  row("dir", "north", "on", "/home/tom/code/north"),
  row("dir", "nixos-config", "off", "/home/tom/code/nixos-config"),
];

const kinds = (rows: Row[]) => rows.map((r) => r.kind);
const names = (rows: Row[]) => rows.map((r) => r.name);

test("view membership spans kinds where the view is a question, not a kind", () => {
  // Narrow views stay kind equality.
  expect(configViewIncludes("hook", "hook", "firn-guard")).toBe(true);
  expect(configViewIncludes("hook", "skill", "firn")).toBe(false);
  expect(configViewIncludes("all", "dir", "north")).toBe(true);

  // /globals: the global knobs plus the hooks and skills switches.
  expect(configViewIncludes("globals", "item", "statusline")).toBe(true);
  expect(configViewIncludes("globals", "hook", "firn-guard")).toBe(true);
  expect(configViewIncludes("globals", "skill", "firn")).toBe(true);
  expect(configViewIncludes("globals", "plugin", "typescript-lsp")).toBe(false);
  expect(configViewIncludes("globals", "dir", "north")).toBe(false);

  // /agentsmd answers "what context files exist and which are active": the one
  // global AGENTS.md row and every directory-triggered file. The other
  // singletons are not context files and must not appear.
  expect(configViewIncludes("agentsmd", "item", "agents-md")).toBe(true);
  expect(configViewIncludes("agentsmd", "dir", "north")).toBe(true);
  expect(configViewIncludes("agentsmd", "item", "statusline")).toBe(false);
  expect(configViewIncludes("agentsmd", "item", "orchestration")).toBe(false);
  expect(configViewIncludes("agentsmd", "hook", "firn-guard")).toBe(false);
});

test("sectioned views group by kind so each section prints one header", () => {
  const all = configViewRows(MANIFEST.slice(), "all") as Row[];
  expect(kinds(all)).toEqual([
    "item", "item", "item", "item",
    "dir", "dir",
    "hook", "hook",
    "skill", "skill",
    "plugin",
  ]);
  // Stable within a kind: orchestration keeps its manifest position relative to
  // the other items even though it was appended after the skills.
  expect(names(all).slice(0, 4)).toEqual([
    "agents-md", "code-md", "statusline", "orchestration",
  ]);

  const globals = configViewRows(MANIFEST.slice(), "globals") as Row[];
  expect(kinds(globals)).toEqual([
    "item", "item", "item", "item", "hook", "hook", "skill", "skill",
  ]);

  const agentsmd = configViewRows(MANIFEST.slice(), "agentsmd") as Row[];
  expect(names(agentsmd)).toEqual(["agents-md", "north", "nixos-config"]);
  expect(kinds(agentsmd)).toEqual(["item", "dir", "dir"]);

  // A narrow view is untouched: no grouping, no headers, manifest order.
  const plugins = configViewRows(MANIFEST.slice(), "plugin") as Row[];
  expect(names(plugins)).toEqual(["typescript-lsp@claude-plugins-official"]);

  // The kind may be absent entirely until the CLI registers a directory; the
  // empty-view path has to be reachable rather than an exception.
  const noDirs = MANIFEST.filter((r) => r.kind !== "dir");
  expect(configViewRows(noDirs.slice(), "agentsmd")).toHaveLength(1);
});

test("the header budget covers every kind the view can show", () => {
  // Five kinds now: item, dir, hook, skill, plugin. Four was the pre-`dir`
  // budget and is exactly one row short.
  expect(configSectionRows("all")).toBe(5);
  expect(configSectionRows("globals")).toBe(3);
  expect(configSectionRows("agentsmd")).toBe(2);
  expect(configSectionRows("hook")).toBe(0);

  // A narrowed view keeps the rows the full switchboard has to give up.
  const total = 100;
  expect(configVisibleCount(total, "hook")).toBeGreaterThan(configVisibleCount(total, "globals"));
  expect(configVisibleCount(total, "globals")).toBeGreaterThan(configVisibleCount(total, "all"));
  expect(configVisibleCount(total, "all")).toBe(ROWS - CHROME_ROWS - MIN_WORKSPACE_ROWS - 3 - 5);
});

function configRuntime(entries: Row[], view: string) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
  };
}

test("a manifest with a fifth kind still fits the docked panel", () => {
  // Long enough that the window is what limits the panel, so the header budget
  // is load-bearing rather than slack.
  const long: Row[] = [];
  for (let i = 0; i < 5; i += 1) long.push(row("item", `item-${i}`));
  for (let i = 0; i < 5; i += 1) long.push(row("dir", `dir-${i}`, "on", `/home/tom/code/d${i}`));
  for (let i = 0; i < 5; i += 1) long.push(row("hook", `hook-${i}`));
  for (let i = 0; i < 5; i += 1) long.push(row("skill", `skill-${i}`));
  for (let i = 0; i < 20; i += 1) long.push(row("plugin", `plugin-${i}`));

  const runtime = configRuntime(long, "all");
  const window = configVisibleCount(long.length, "all");
  expect(window).toBeLessThan(long.length);

  // Title line + windowed rows + one header per kind inside the window.
  expect(configDetailLines(runtime)).toBe(1 + window + 5);
  // The number the layout actually spends. Pre-bump this was PANEL_BUDGET + 1.
  expect(detailHeight(runtime)).toBe(PANEL_BUDGET);
  expect(detailHeight(runtime)).toBeLessThanOrEqual(PANEL_BUDGET);

  // The narrowed views sit under the same ceiling.
  expect(detailHeight(configRuntime(long, "globals"))).toBeLessThanOrEqual(PANEL_BUDGET);
  expect(detailHeight(configRuntime(long, "agentsmd"))).toBeLessThanOrEqual(PANEL_BUDGET);
});

async function frameOf(view: string, entries: Row[] = MANIFEST) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 22,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(configRuntime(entries, view));
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();
  return frame;
}

test("/globals renders the three global sections under the new header", async () => {
  const frame = await frameOf("globals");
  expect(frame).toContain("globals");
  expect(frame).toContain("GLOBALS");
  expect(frame).toContain("HOOKS");
  expect(frame).toContain("SKILLS");
  // The header the user renamed away from.
  expect(frame).not.toContain("AGENTS.MD & GLOBALS");
  // Neither plugins nor directory context belong to the global knobs.
  expect(frame).not.toContain("PLUGINS");
  expect(frame).not.toContain("DIRECTORY CONTEXT");
  expect(frame).toContain("orchestration");
  expect(frame).toContain("statusline");
  expect(frame).toContain("firn-guard");
});

test("/agentsmd answers which context files exist and which are on", async () => {
  const frame = await frameOf("agentsmd");
  expect(frame).toContain("agents.md & directory context");
  expect(frame).toContain("GLOBALS");
  expect(frame).toContain("DIRECTORY CONTEXT");
  expect(frame).toContain("agents-md");
  // Slug plus state plus the directory the CLI hands over, on one row.
  expect(frame).toContain("north  /home/tom/code/north");
  expect(frame).toContain("nixos-config  /home/tom/code/nixos-config");
  expect(frame).toContain("on ");
  expect(frame).toContain("off ");
  // Other singletons are switches, not context files.
  expect(frame).not.toContain("statusline");
  expect(frame).not.toContain("HOOKS");
});

test("/config carries the directory-context section alongside the rest", async () => {
  const frame = await frameOf("all");
  for (const header of ["GLOBALS", "DIRECTORY CONTEXT", "HOOKS", "SKILLS", "PLUGINS"]) {
    expect(frame).toContain(header);
  }
  expect(frame).toContain("context switchboard");
});

test("a view with no rows says so instead of claiming to be loading", async () => {
  const frame = await frameOf("agentsmd", [row("item", "statusline"), row("hook", "firn-guard")]);
  expect(frame).toContain("nothing to configure here");
  expect(frame).not.toContain("loading");
});
