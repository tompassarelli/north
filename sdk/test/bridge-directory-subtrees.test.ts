import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  config_cli_name as configCliName,
  config_detail_lines as configDetailLines,
  config_entry_active_p as configEntryActive,
  config_gate_modules as configGateModules,
  config_row_depth as configRowDepth,
  config_row_label as configRowLabel,
  config_section_kind as configSectionKind,
  config_section_rows as configSectionRows,
  config_state_text as configStateText,
  config_unit_active_p as configUnitActive,
  config_view_includes_p as configViewIncludes,
  config_view_rows as configViewRows,
  config_visible_count as configVisibleCount,
  detail_height as detailHeight,
  render_config_panel as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";

// The panel reads the terminal for its window math. Pin both dimensions so the
// frames below are arithmetic rather than a property of whoever's tty ran the
// suite.
const ROWS = 40;
const COLUMNS = 120;
Object.defineProperty(process.stdout, "rows", { value: ROWS, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: COLUMNS, configurable: true });

const CHROME_ROWS = 4;
const MIN_WORKSPACE_ROWS = 4;
const PANEL_BUDGET = ROWS - CHROME_ROWS - MIN_WORKSPACE_ROWS;

type Row = { kind: string; name: string; state: string; detail: string };
type Membership = { module: string; members: string[] };

function row(kind: string, name: string, state = "off", detail = ""): Row {
  return { kind, name, state, detail };
}

// A directory's context is a subtree, and the manifest says so in four kinds:
// `dir` is the gate over the whole thing and carries the path, `ins` is that
// directory's instruction file, `memroot` is the gate over its memories, `mem`
// is one memory. Every one of them is named for the directory's slug — a memory
// qualifies that with its own name — and the manifest is append-ordered, so the
// rows arrive interleaved and out of reading order.
const MANIFEST: Row[] = [
  row("dir", "code", "on", "/tmp/switchboard-fixture/code"),
  row("hook", "firn-guard", "enabled", "firn"),
  row("mem", "code/report-style-recommendations", "on"),
  row("skill", "firn", "on"),
  row("ins", "code", "on"),
  row("dir", "global", "on", "~"),
  row("memroot", "code", "on"),
  row("mem", "code/agents-switchboard-architecture", "on"),
  row("ins", "global", "on"),
  row("module", "orchestration", "on"),
  row("dir", "north", "on", "/tmp/switchboard-fixture/north"),
  row("plugin", "typescript-lsp@claude-plugins-official", "on"),
];

const names = (rows: Row[]) => rows.map((r) => r.name);
const kinds = (rows: Row[]) => rows.map((r) => r.kind);

const withState = (rows: Row[], kind: string, name: string, state: string) =>
  rows.map((r) => (r.kind === kind && r.name === name ? { ...r, state } : r));

const entryOf = (rows: Row[], kind: string, name: string) =>
  rows.find((r) => r.kind === kind && r.name === name)!;

const stateOf = (rows: Row[], kind: string, name: string,
                 rosters: Membership[] = []) => {
  const entry = entryOf(rows, kind, name);
  return configStateText(entry, rows, rosters,
                         configEntryActive(entry, rows, rosters));
};

test("a subtree row belongs to its directory's section, not one of its own", () => {
  // The nesting is rows, not sections: however deep a directory gets, the panel
  // still prints one DIRECTORY INSTRUCTIONS header and the six-section budget
  // is untouched.
  expect(configSectionKind("ins")).toBe("dir");
  expect(configSectionKind("memroot")).toBe("dir");
  expect(configSectionKind("mem")).toBe("dir");
  expect(configSectionKind("skill")).toBe("skill");
  expect(configSectionRows("all")).toBe(6);
  expect(configSectionRows("agentsmd")).toBe(1);
  expect(configSectionRows("globals")).toBe(5);

  // Depth reads as depth: the file and the memories group one step under the
  // directory, the memories one step under those.
  expect(configRowDepth("dir")).toBe(0);
  expect(configRowDepth("ins")).toBe(1);
  expect(configRowDepth("memroot")).toBe(1);
  expect(configRowDepth("mem")).toBe(2);
  expect(configRowDepth("skill")).toBe(0);
});

test("every subtree row follows the directory it belongs to", () => {
  const all = configViewRows(MANIFEST.slice(), "all") as Row[];
  // The global profile heads the section as it always has, now carrying its own
  // instruction file; each directory is followed by its file, its memories gate
  // and the memories, and the later sections are where they were.
  expect(names(all)).toEqual([
    "global", "global",
    "code", "code", "code",
    "code/report-style-recommendations", "code/agents-switchboard-architecture",
    "north",
    "orchestration",
    "firn",
    "firn-guard",
    "typescript-lsp@claude-plugins-official",
  ]);
  expect(kinds(all)).toEqual([
    "dir", "ins",
    "dir", "ins", "memroot", "mem", "mem",
    "dir",
    "module",
    "skill",
    "hook",
    "plugin",
  ]);
  // Memories keep the order the CLI wrote them in — the sort inside a subtree
  // has nothing to say between two rows of the same kind.
  expect(names(all).slice(5, 7)).toEqual([
    "code/report-style-recommendations",
    "code/agents-switchboard-architecture",
  ]);
});

test("/agentsmd is the whole subtree — what context exists, and what is active", () => {
  expect(configViewIncludes("agentsmd", "ins", "code")).toBe(true);
  expect(configViewIncludes("agentsmd", "memroot", "code")).toBe(true);
  expect(configViewIncludes("agentsmd", "mem", "code/report-style-recommendations"))
    .toBe(true);
  expect(configViewIncludes("agentsmd", "skill", "firn")).toBe(false);

  const view = configViewRows(MANIFEST.slice(), "agentsmd") as Row[];
  expect(names(view)).toEqual([
    "global", "global",
    "code", "code", "code",
    "code/report-style-recommendations", "code/agents-switchboard-architecture",
    "north",
  ]);

  // /globals is the root scope, so it carries the profile's own subtree and no
  // other directory's — a project's memories are not a global knob.
  expect(configViewIncludes("globals", "ins", "global")).toBe(true);
  expect(configViewIncludes("globals", "mem", "global/house-style")).toBe(true);
  expect(configViewIncludes("globals", "ins", "code")).toBe(false);
  expect(configViewIncludes("globals", "mem", "code/report-style-recommendations"))
    .toBe(false);
  const globals = configViewRows(MANIFEST.slice(), "globals") as Row[];
  expect(names(globals)).toEqual([
    "global", "global", "orchestration", "firn", "firn-guard",
  ]);
});

test("a row the panel cannot nest keeps its place instead of vanishing", () => {
  // A memory whose directory row the manifest does not carry is still a switch
  // that exists. It reads at the end of the section rather than disappearing.
  const orphaned = [...MANIFEST, row("mem", "ghost/leftover", "on")];
  const view = configViewRows(orphaned, "agentsmd") as Row[];
  expect(names(view)[names(view).length - 1]).toBe("ghost/leftover");
  expect(view).toHaveLength(9);
});

test("the directory is the gate, and one press closes the whole subtree", () => {
  // Everything on: the subtree composes.
  expect(configUnitActive(MANIFEST, [], "code")).toBe(true);
  expect(configEntryActive(entryOf(MANIFEST, "ins", "code"), MANIFEST, [])).toBe(true);
  expect(configEntryActive(entryOf(MANIFEST, "mem", "code/report-style-recommendations"),
                           MANIFEST, [])).toBe(true);

  // The dir row goes off. No other row in the manifest changed, and nothing in
  // the subtree is composing — the file one link down, the memories two.
  const dirOff = withState(MANIFEST, "dir", "code", "off");
  expect(dirOff.filter((r) => !(r.kind === "dir" && r.name === "code")))
    .toEqual(MANIFEST.filter((r) => !(r.kind === "dir" && r.name === "code")));
  expect(configEntryActive(entryOf(dirOff, "ins", "code"), dirOff, [])).toBe(false);
  expect(configEntryActive(entryOf(dirOff, "memroot", "code"), dirOff, [])).toBe(false);
  expect(configEntryActive(entryOf(dirOff, "mem", "code/agents-switchboard-architecture"),
                           dirOff, [])).toBe(false);
  // The directory next door is untouched — the gate is per-subtree.
  expect(configEntryActive(entryOf(dirOff, "ins", "global"), dirOff, [])).toBe(true);

  // The memories gate closes only the memories: the instruction file hangs off
  // the directory, not off the memories group.
  const memsOff = withState(MANIFEST, "memroot", "code", "off");
  expect(configEntryActive(entryOf(memsOff, "ins", "code"), memsOff, [])).toBe(true);
  expect(configEntryActive(entryOf(memsOff, "mem", "code/report-style-recommendations"),
                           memsOff, [])).toBe(false);

  // A memory's own switch still answers for itself, gate wide open.
  const memOff = withState(MANIFEST, "mem", "code/report-style-recommendations", "off");
  expect(configEntryActive(entryOf(memOff, "mem", "code/report-style-recommendations"),
                           memOff, [])).toBe(false);
  expect(configEntryActive(entryOf(memOff, "mem", "code/agents-switchboard-architecture"),
                           memOff, [])).toBe(true);
});

test("a gated child says which row is holding it, one hop per rung", () => {
  const dirOff = withState(MANIFEST, "dir", "code", "off");
  // The gate itself is off on its own account and says only that.
  expect(stateOf(dirOff, "dir", "code")).toBe("off");
  // Its children's own switches are still on, and they are not composing: the
  // provenance style a gated unit has always had, now naming a directory.
  expect(stateOf(dirOff, "ins", "code")).toBe("on · off (code off)");
  expect(stateOf(dirOff, "memroot", "code")).toBe("on · off (code off)");
  // A memory names the rung directly above it, which names the directory in
  // turn — the same ladder a skill inside a switched-off bundle reads as.
  expect(stateOf(dirOff, "mem", "code/report-style-recommendations"))
    .toBe("on · off (memories off)");
  expect(configGateModules(entryOf(dirOff, "ins", "code"), dirOff, []))
    .toEqual(["code"]);
  expect(configGateModules(entryOf(dirOff, "mem", "code/report-style-recommendations"),
                           dirOff, [])).toEqual(["memories"]);

  // Own switch off needs no gate to explain it.
  const memOff = withState(dirOff, "mem", "code/report-style-recommendations", "off");
  expect(stateOf(memOff, "mem", "code/report-style-recommendations")).toBe("off");
});

test("modules still gate a subtree row, by union, alongside the directory", () => {
  // Membership is orthogonal to nesting: a bundle can hold a memory, and the
  // memory then needs its directory open AND one holding bundle on.
  const rosters: Membership[] = [{ module: "orchestration",
                                   members: ["code/report-style-recommendations"] }];
  expect(configEntryActive(entryOf(MANIFEST, "mem", "code/report-style-recommendations"),
                           MANIFEST, rosters)).toBe(true);

  const bundleOff = withState(MANIFEST, "module", "orchestration", "off");
  expect(configEntryActive(entryOf(bundleOff, "mem", "code/report-style-recommendations"),
                           bundleOff, rosters)).toBe(false);
  expect(stateOf(bundleOff, "mem", "code/report-style-recommendations", rosters))
    .toBe("on · off (orchestration off)");

  // Both closed: the subtree gate reads first, because it is the coarser thing
  // and the one the panel nests the row under.
  const both = withState(bundleOff, "dir", "code", "off");
  expect(configGateModules(entryOf(both, "mem", "code/report-style-recommendations"),
                           both, rosters)).toEqual(["memories", "orchestration"]);
});

test("a memory with no memories row still answers to its directory", () => {
  // A half-written manifest must not derive a permanent off: the dir-gate half
  // of a memory's activity is not optional, so it applies with the middle row
  // missing.
  const noRoot = MANIFEST.filter((r) => r.kind !== "memroot");
  expect(configEntryActive(entryOf(noRoot, "mem", "code/report-style-recommendations"),
                           noRoot, [])).toBe(true);
  const dirOff = withState(noRoot, "dir", "code", "off");
  expect(configEntryActive(entryOf(dirOff, "mem", "code/report-style-recommendations"),
                           dirOff, [])).toBe(false);
  expect(stateOf(dirOff, "mem", "code/report-style-recommendations"))
    .toBe("on · off (code off)");

  // A subtree row whose directory is nowhere in the manifest is ungated rather
  // than dead: absence means the CLI never registered the scope, and the honest
  // answer is the row's own switch.
  const orphan = row("mem", "ghost/leftover", "on");
  expect(configEntryActive(orphan, [...MANIFEST, orphan], [])).toBe(true);
});

test("the slug names three rows, and each derives as itself", () => {
  // `dir code`, `ins code` and `memroot code` share a name. Keying the cycle
  // walk on the name alone would read the file gating on its directory as a
  // loop and derive every one of them off.
  const rows = MANIFEST.filter((r) => r.name === "code" || r.name.startsWith("code/"));
  expect(kinds(rows).sort()).toEqual(["dir", "ins", "mem", "mem", "memroot"]);
  for (const r of rows) expect(configEntryActive(r, MANIFEST, [])).toBe(true);
});

test("a row calls itself what it is, and answers to what the CLI knows", () => {
  // The panel prints what the row IS, because the directory is already the row
  // it is nested under.
  expect(configRowLabel("ins", "code")).toBe("AGENTS.md");
  expect(configRowLabel("memroot", "code")).toBe("memories");
  expect(configRowLabel("mem", "code/report-style-recommendations"))
    .toBe("report-style-recommendations");
  expect(configRowLabel("dir", "code")).toBe("code");
  expect(configRowLabel("skill", "firn")).toBe("firn");

  // Enter shells out to `agents path <target>`, and the target is the manifest
  // name: a memory's already carries its directory, and the instruction file
  // answers to the directory alone because that is what `agents path` resolves
  // to that file.
  expect(configCliName("mem", "code/report-style-recommendations"))
    .toBe("code/report-style-recommendations");
  expect(configCliName("mem", "global/house-style")).toBe("global/house-style");
  expect(configCliName("ins", "code")).toBe("code");
  expect(configCliName("dir", "code")).toBe("code");
  expect(configCliName("skill", "firn")).toBe("firn");
});

function configRuntime(entries: Row[], view: string, index = 0,
                       rosters: Membership[] = []) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configAllEntries: entries.slice(),
    configMemberships: rosters,
    configFilter: view,
    configIndex: index,
    configLoaded: true,
  };
}

async function frameOf(view: string, entries: Row[] = MANIFEST, index = 0,
                       height = 26, rosters: Membership[] = []) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(configRuntime(entries, view, index, rosters));
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();
  return frame;
}

// One rendered row with its indentation intact: only the two-cell cursor gutter
// comes off, because how far in the row sits is the thing under test.
function rowLine(frame: string, needle: string): string {
  const line = frame.split("\n").find((l) => l.includes(needle));
  expect(line).toBeDefined();
  return line!.replace(/^(› |  )/, "").trimEnd();
}

test("the subtree renders nested, at both depths", async () => {
  const frame = await frameOf("agentsmd");
  const lines = frame.split("\n").map((l) => l.trimEnd());
  const at = lines.indexOf("DIRECTORY INSTRUCTIONS");
  expect(at).toBeGreaterThanOrEqual(0);
  // Exactly the user's sketch: the directory at the section's margin with its
  // path, its file and its memories one step in, the memories one step further.
  expect(lines.slice(at + 1, at + 8)).toEqual([
    "› on  global  ~",
    "      on  AGENTS.md",
    "  on  code  /tmp/switchboard-fixture/code",
    "      on  AGENTS.md",
    "      on  memories",
    "          on  report-style-recommendations",
    "          on  agents-switchboard-architecture",
  ]);
  // One header for the section however deep it gets.
  expect(lines.filter((l) => l === "DIRECTORY INSTRUCTIONS")).toHaveLength(1);
  // A memory prints its own name, not the slug it is addressed by.
  expect(frame).not.toContain("code/report-style-recommendations");
});

test("a closed directory darkens its subtree on screen, and says which row did it", async () => {
  const frame = await frameOf("agentsmd", withState(MANIFEST, "dir", "code", "off"));
  expect(rowLine(frame, "code  /tmp")).toBe("off code  /tmp/switchboard-fixture/code");
  const lines = frame.split("\n").map((l) => l.trimEnd());
  const at = lines.indexOf("  off code  /tmp/switchboard-fixture/code");
  expect(lines.slice(at + 1, at + 5)).toEqual([
    "      on · off (code off)  AGENTS.md",
    "      on · off (code off)  memories",
    "          on · off (memories off)  report-style-recommendations",
    "          on · off (memories off)  agents-switchboard-architecture",
  ]);
  // The directory next door is untouched, and still nests.
  expect(lines).toContain("      on  AGENTS.md");
  expect(lines).toContain("› on  global  ~");
});

test("closing the memories keeps the instruction file, indentation and all", async () => {
  const frame = await frameOf("agentsmd", withState(MANIFEST, "memroot", "code", "off"));
  const lines = frame.split("\n").map((l) => l.trimEnd());
  expect(lines).toContain("      on  AGENTS.md");
  expect(lines).toContain("      off memories");
  expect(lines).toContain("          on · off (memories off)  report-style-recommendations");
});

test("/config carries the subtrees inside the directory section, sections intact", async () => {
  const frame = await frameOf("all");
  const order = [
    "DIRECTORY INSTRUCTIONS", "MODULES", "SKILLS", "HOOKS", "PLUGINS",
  ];
  const seen = order.map((header) => {
    const at = frame.indexOf(header);
    expect(at).toBeGreaterThanOrEqual(0);
    return at;
  });
  expect(seen).toEqual([...seen].sort((a, b) => a - b));
  // The subtree rows sit above MODULES, which is where the directory section
  // ends — they are rows in it, not a section after it.
  expect(frame.indexOf("memories")).toBeLessThan(frame.indexOf("MODULES"));
  expect(frame.split("\n").map((l) => l.trimEnd()))
    .toContain("          on  report-style-recommendations");
});

// A directory deep enough that the window is what limits the panel, so the
// scroll math is load-bearing rather than slack.
const DEEP: Row[] = [
  row("dir", "global", "on", "~"),
  row("dir", "code", "on", "/tmp/switchboard-fixture/code"),
  row("ins", "code", "on"),
  row("memroot", "code", "on"),
  ...Array.from({ length: 40 }, (_, i) =>
    row("mem", `code/memory-${String(i).padStart(2, "0")}`, "on")),
  row("skill", "firn", "on"),
];

test("the window follows the cursor into the subtree, and pays for one header", () => {
  const total = (configViewRows(DEEP.slice(), "agentsmd") as Row[]).length;
  const window = configVisibleCount(total, "agentsmd");
  expect(window).toBeLessThan(total);

  // The whole deep section is one header, however many rows it holds, so the
  // panel is exactly as tall as its content and never taller than the budget.
  for (const index of [0, 20, total - 1]) {
    const runtime = configRuntime(DEEP, "agentsmd", index);
    expect(configDetailLines(runtime)).toBe(1 + window + 1);
    expect(detailHeight(runtime)).toBeLessThanOrEqual(PANEL_BUDGET);
  }
  // The full switchboard sits under the same ceiling with the subtree in it.
  expect(detailHeight(configRuntime(DEEP, "all"))).toBeLessThanOrEqual(PANEL_BUDGET);
});

test("scrolling a deep section is kind-agnostic: it follows the row, not the row's kind", async () => {
  const top = (await frameOf("agentsmd", DEEP, 0, 40)).split("\n").map((l) => l.trimEnd());
  expect(top).toContain("› on  global  ~");
  expect(top).toContain("          on  memory-00");
  // The tail is outside the window at the top of the list.
  expect(top).not.toContain("          on  memory-39");

  const rows = configViewRows(DEEP.slice(), "agentsmd") as Row[];
  const bottom = (await frameOf("agentsmd", DEEP, rows.length - 1, 40))
    .split("\n").map((l) => l.trimEnd());
  // The cursor is on the last memory and the window came with it, still nested
  // and still under its one section header. The marker keeps the leftmost cell
  // on every row, whatever depth the row it is on sits at.
  expect(bottom).toContain("›         on  memory-39");
  expect(bottom).not.toContain("  on  global  ~");
  expect(bottom.filter((l) => l === "DIRECTORY INSTRUCTIONS")).toHaveLength(1);
  // The header prints for whatever row the window starts on, memory or not.
  expect(bottom.indexOf("DIRECTORY INSTRUCTIONS"))
    .toBeLessThan(bottom.findIndex((l) => l.includes("memory-")));

  // Mid-list the cursor is centred, with the rows either side of it on screen.
  const middle = (await frameOf("agentsmd", DEEP, 24, 40))
    .split("\n").map((l) => l.trimEnd());
  const cursor = middle.findIndex((l) => l.startsWith("›"));
  expect(middle[cursor]).toBe("›         on  memory-20");
  expect(middle[cursor - 1]).toBe("          on  memory-19");
  expect(middle[cursor + 1]).toBe("          on  memory-21");
});
