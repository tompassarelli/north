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
// workspace's minimum. Chrome is five rows since the view bar moved to the top
// of the frame: padding, view bar, composer, context line, agent strip.
const CHROME_ROWS = 5;
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

test("view membership spans kinds where the view is a question, not a kind", () => {
  // Narrow views stay kind equality, including the new one.
  expect(configViewIncludes("hook", "hook", "firn-guard")).toBe(true);
  expect(configViewIncludes("hook", "skill", "firn")).toBe(false);
  expect(configViewIncludes("module", "module", "orchestration")).toBe(true);
  expect(configViewIncludes("module", "other", "statusline-script")).toBe(false);
  expect(configViewIncludes("all", "dir", "north")).toBe(true);

  // /globals is the global scope: the global profile row plus every knob that
  // is not per-directory and not somebody else's package.
  expect(configViewIncludes("globals", "dir", "global")).toBe(true);
  expect(configViewIncludes("globals", "hook", "firn-guard")).toBe(true);
  expect(configViewIncludes("globals", "skill", "firn")).toBe(true);
  expect(configViewIncludes("globals", "module", "orchestration")).toBe(true);
  expect(configViewIncludes("globals", "other", "statusline-script")).toBe(true);
  expect(configViewIncludes("globals", "plugin", "typescript-lsp")).toBe(false);
  expect(configViewIncludes("globals", "dir", "north")).toBe(false);

  // /agentsmd answers "what instruction files exist and which are active",
  // which is now exactly one section: the global profile and every
  // directory-triggered file, and nothing else.
  expect(configViewIncludes("agentsmd", "dir", "global")).toBe(true);
  expect(configViewIncludes("agentsmd", "dir", "north")).toBe(true);
  expect(configViewIncludes("agentsmd", "other", "statusline-script")).toBe(false);
  expect(configViewIncludes("agentsmd", "module", "orchestration")).toBe(false);
  expect(configViewIncludes("agentsmd", "hook", "firn-guard")).toBe(false);
});

test("sectioned views group by kind so each section prints one header", () => {
  const all = configViewRows(MANIFEST.slice(), "all") as Row[];
  expect(kinds(all)).toEqual([
    "dir", "dir", "dir", "dir",
    "skill", "skill",
    "hook", "hook",
    "module",
    "plugin",
    "other",
  ]);
  // The global profile reads first in its section wherever the CLI appended
  // it; the per-directory rows keep their manifest order behind it.
  expect(names(all).slice(0, 4)).toEqual(["global", "code", "north", "nixos-config"]);

  const globals = configViewRows(MANIFEST.slice(), "globals") as Row[];
  expect(kinds(globals)).toEqual([
    "dir", "skill", "skill", "hook", "hook", "module", "other",
  ]);
  expect(names(globals)[0]).toBe("global");

  const agentsmd = configViewRows(MANIFEST.slice(), "agentsmd") as Row[];
  expect(names(agentsmd)).toEqual(["global", "code", "north", "nixos-config"]);
  expect(kinds(agentsmd)).toEqual(["dir", "dir", "dir", "dir"]);

  // A narrow view is untouched: no grouping, no headers, manifest order.
  const plugins = configViewRows(MANIFEST.slice(), "plugin") as Row[];
  expect(names(plugins)).toEqual(["typescript-lsp@claude-plugins-official"]);
  const modules = configViewRows(MANIFEST.slice(), "module") as Row[];
  expect(names(modules)).toEqual(["orchestration"]);

  // A kind may be absent entirely until the CLI registers one; the empty-view
  // path has to be reachable rather than an exception.
  const noDirs = MANIFEST.filter((r) => r.kind !== "dir");
  expect(configViewRows(noDirs.slice(), "agentsmd")).toHaveLength(0);
});

test("the header budget covers every kind the view can show", () => {
  // Six kinds now: dir, skill, hook, module, plugin, other. `item` died and
  // module plus other were born, so the full switchboard pays one row more
  // than the five it used to.
  expect(configSectionRows("all")).toBe(6);
  // Everything but plugins, and only one row of the directory section.
  expect(configSectionRows("globals")).toBe(5);
  // Literally one section.
  expect(configSectionRows("agentsmd")).toBe(1);
  expect(configSectionRows("hook")).toBe(0);
  expect(configSectionRows("module")).toBe(0);

  // A narrowed view keeps the rows the full switchboard has to give up.
  const total = 100;
  expect(configVisibleCount(total, "hook")).toBeGreaterThan(configVisibleCount(total, "globals"));
  expect(configVisibleCount(total, "globals")).toBeGreaterThan(configVisibleCount(total, "all"));
  expect(configVisibleCount(total, "all")).toBe(ROWS - CHROME_ROWS - MIN_WORKSPACE_ROWS - 3 - 6);
});

function configRuntime(entries: Row[], view: string) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configAllEntries: entries.slice(),
    configMemberships: [],
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
  };
}

test("a manifest with all six kinds still fits the docked panel", () => {
  // Long enough that the window is what limits the panel, so the header budget
  // is load-bearing rather than slack.
  // Sized so the window still reaches the last section: every one of the six
  // headers is inside it, which is what makes the budget load-bearing.
  const long: Row[] = [row("dir", "global", "on", "~")];
  for (let i = 0; i < 2; i += 1) long.push(row("dir", `dir-${i}`, "on", `/tmp/switchboard-fixture/d${i}`));
  for (let i = 0; i < 3; i += 1) long.push(row("skill", `skill-${i}`));
  for (let i = 0; i < 3; i += 1) long.push(row("hook", `hook-${i}`));
  for (let i = 0; i < 3; i += 1) long.push(row("module", `module-${i}`));
  for (let i = 0; i < 8; i += 1) long.push(row("plugin", `plugin-${i}`));
  for (let i = 0; i < 5; i += 1) long.push(row("other", `other-${i}`));

  const runtime = configRuntime(long, "all");
  const window = configVisibleCount(long.length, "all");
  expect(window).toBeLessThan(long.length);

  // Title line + windowed rows + one header per kind inside the window.
  expect(configDetailLines(runtime)).toBe(1 + window + 6);
  // The number the layout actually spends: the extra header row is paid for by
  // one fewer list row, so the panel is exactly as tall as it always was.
  expect(detailHeight(runtime)).toBe(PANEL_BUDGET);
  expect(detailHeight(runtime)).toBeLessThanOrEqual(PANEL_BUDGET);

  // The narrowed views sit under the same ceiling.
  expect(detailHeight(configRuntime(long, "globals"))).toBeLessThanOrEqual(PANEL_BUDGET);
  expect(detailHeight(configRuntime(long, "agentsmd"))).toBeLessThanOrEqual(PANEL_BUDGET);
  expect(detailHeight(configRuntime(long, "module"))).toBeLessThanOrEqual(PANEL_BUDGET);
});

async function frameOf(view: string, entries: Row[] = MANIFEST) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 24,
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

test("/globals is the global scope: the profile, skills, hooks, modules, other", async () => {
  const frame = await frameOf("globals");
  expect(frame).toContain("globals");
  // The global profile is a directory row, so its section header comes with it.
  expect(frame).toContain("FILETREE-SCOPED INSTRUCTIONS");
  expect(frame).toContain("global  ~");
  expect(frame).toContain("SKILLS");
  expect(frame).toContain("HOOKS");
  expect(frame).toContain("MODULES");
  expect(frame).toContain("OTHER");
  expect(frame).toContain("orchestration");
  expect(frame).toContain("statusline-script");
  expect(frame).toContain("firn-guard");
  // Per-directory rows are not global knobs, and plugins are not ours.
  expect(frame).not.toContain("PLUGINS");
  expect(frame).not.toContain("nixos-config");
  expect(frame).not.toContain("/tmp/switchboard-fixture/north");
});

test("/agentsmd is the filetree section entire, root scope first", async () => {
  const frame = await frameOf("agentsmd");
  expect(frame).toContain("filetree-scoped instructions");
  expect(frame).toContain("FILETREE-SCOPED INSTRUCTIONS");
  // These rows are instruction files scoped to a subtree; the header says that
  // rather than naming the manifest token behind them.
  expect(frame).not.toContain("DIRECTORY CONTEXT");
  expect(frame).not.toContain("agents.md & directory context");
  // The root scope reads above the narrower scopes layered on it.
  expect(frame.indexOf("global  ~")).toBeGreaterThanOrEqual(0);
  expect(frame.indexOf("global  ~")).toBeLessThan(frame.indexOf("north  /tmp"));
  // Slug plus state plus the directory the CLI hands over, on one row.
  expect(frame).toContain("north  /tmp/switchboard-fixture/north");
  expect(frame).toContain("nixos-config  /tmp/switchboard-fixture/nixos-config");
  expect(frame).toContain("on ");
  expect(frame).toContain("off ");
  // One section means exactly one header and no other kind's rows.
  for (const header of ["SKILLS", "HOOKS", "MODULES", "PLUGINS", "OTHER"]) {
    expect(frame).not.toContain(header);
  }
  expect(frame).not.toContain("statusline-script");
});

test("/config carries all six sections in reading order", async () => {
  const frame = await frameOf("all");
  const order = [
    "FILETREE-SCOPED INSTRUCTIONS", "SKILLS", "HOOKS", "MODULES", "PLUGINS", "OTHER",
  ];
  const seen = order.map((header) => {
    const at = frame.indexOf(header);
    expect(at).toBeGreaterThanOrEqual(0);
    return at;
  });
  expect(seen).toEqual([...seen].sort((a, b) => a - b));
  expect(frame).toContain("context switchboard");
  // The headers the taxonomy and the rename retired.
  expect(frame).not.toContain("GLOBALS");
  expect(frame).not.toContain("DIRECTORY CONTEXT");
});

test("/modules narrows to the orchestration modules and says so", async () => {
  const frame = await frameOf("module");
  expect(frame).toContain("modules");
  expect(frame).toContain("orchestration");
  // A narrow view prints no headers and no other kind's rows.
  expect(frame).not.toContain("MODULES");
  expect(frame).not.toContain("firn-guard");
  expect(frame).not.toContain("statusline-script");
});

test("a view with no rows says so instead of claiming to be loading", async () => {
  const frame = await frameOf("agentsmd", [
    row("other", "statusline-script"), row("hook", "firn-guard"),
  ]);
  expect(frame).toContain("nothing to configure here");
  expect(frame).not.toContain("loading");
});
