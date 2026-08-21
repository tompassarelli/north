import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  "config-cli-name" as configCliName,
  "config-detail-lines!" as configDetailLines,
  "config-entry-active?" as configEntryActive,
  "config-gate-modules" as configGateModules,
  "config-query-rows" as configQueryRows,
  "config-reference-text" as configReferenceText,
  "config-row-search-text" as configRowSearchText,
  "config-row-depth" as configRowDepth,
  "config-row-label" as configRowLabel,
  "config-row-role" as configRowRole,
  "config-row-scope" as configRowScope,
  "config-section-rows" as configSectionRows,
  "config-state-text" as configStateText,
  "config-unit-active?" as configUnitActive,
  "config-view-includes?" as configViewIncludes,
  "config-view-rows" as configViewRows,
  "config-visible-count" as configVisibleCount,
  "detail-height!" as detailHeight,
  "render-config-panel!" as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";

// The panel reads the terminal for its window math. Pin both dimensions so the
// snapshots below are arithmetic rather than a property of whoever's tty ran the
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
// `dir` is the gate over a directory's whole node and carries the path, `ins` is
// that directory's instruction file, `memroot` the gate over its memories, `mem`
// one memory. Every one of them is named for the directory's slug — a memory
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

test("a subtree row belongs to the directory node it names", () => {
  // Scope is the whole nesting rule: a row says which directory it is in force
  // under, and the panel puts it there.
  expect(configRowScope("dir", "code")).toBe("code");
  expect(configRowScope("ins", "code")).toBe("code");
  expect(configRowScope("memroot", "code")).toBe("code");
  expect(configRowScope("mem", "code/report-style-recommendations")).toBe("code");
  // Until the CLI can scope a skill to a project, everything else is the root's.
  expect(configRowScope("skill", "firn")).toBe("global");

  // Depth reads as depth: the directory at the margin, its own two files one
  // step in, its memories one step further.
  expect(configRowDepth("dir")).toBe(0);
  expect(configRowDepth("ins")).toBe(1);
  expect(configRowDepth("memroot")).toBe(1);
  expect(configRowDepth("mem")).toBe(2);

  // These rows head no section of their own: AGENTS.md and MEMORIES are rows
  // with a switch each, inside the DIRECTORY heading and under nothing else.
  for (const kind of ["ins", "memroot", "mem"]) {
    expect(configRowRole(row(kind, "code", "on"), MANIFEST)).toBe(kind);
  }
});

test("every subtree row follows the directory it belongs to", () => {
  const all = configViewRows(MANIFEST.slice(), "all") as Row[];
  // The root node reads first and carries what is scoped to it; each directory
  // is followed by its own two files and its memories.
  // What the directory SAYS reads before what it turns on: its own file and its
  // memories first, then the switches that happen to be in force there.
  expect(names(all)).toEqual([
    "global", "global",
    "orchestration", "firn", "firn-guard",
    "typescript-lsp@claude-plugins-official",
    "code", "code", "code",
    "code/report-style-recommendations", "code/agents-switchboard-architecture",
    "north",
  ]);
  expect(kinds(all).slice(0, 2)).toEqual(["dir", "ins"]);
  expect(kinds(all).slice(6)).toEqual([
    "dir", "ins", "memroot", "mem", "mem", "dir",
  ]);
  // Memories keep the order the CLI wrote them in — the sort inside a node has
  // nothing to say between two rows of the same kind.
  expect(names(all).slice(9, 11)).toEqual([
    "code/report-style-recommendations",
    "code/agents-switchboard-architecture",
  ]);
});

test("/agentsmd is every node's own files — what context exists, and what is active", () => {
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

  // /globals is the root node alone, and it carries the root's own files.
  expect(configViewIncludes("globals", "ins", "global")).toBe(true);
  expect(configViewIncludes("globals", "mem", "global/house-style")).toBe(true);
  expect(configViewIncludes("globals", "ins", "code")).toBe(false);
  expect(configViewIncludes("globals", "dir", "code")).toBe(false);
  const globals = configViewRows(MANIFEST.slice(), "globals") as Row[];
  expect(names(globals)).toEqual([
    "global", "global", "orchestration", "firn", "firn-guard",
  ]);
});

test("a row the panel cannot nest keeps its place instead of vanishing", () => {
  // A memory whose directory row the manifest does not carry is still a switch
  // that exists. It reads at the end rather than disappearing.
  const orphaned = [...MANIFEST, row("mem", "ghost/leftover", "on")];
  const view = configViewRows(orphaned, "agentsmd") as Row[];
  expect(names(view)[names(view).length - 1]).toBe("ghost/leftover");
  expect(view).toHaveLength(9);
});

test("the directory is the gate, and one press closes the whole subtree", () => {
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
  // The node next door is untouched — the gate is per-subtree.
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

test("a gated child says which row is holding it, kind and all, one hop per rung", () => {
  const dirOff = withState(MANIFEST, "dir", "code", "off");
  // The gate itself is off on its own account and says only that.
  expect(stateOf(dirOff, "dir", "code")).toBe("off");
  // Its children's own switches are still on, and they are not composing. The
  // kind comes with the name so `code` cannot be misread as a peer.
  expect(stateOf(dirOff, "ins", "code")).toBe("off (dir: code off)");
  expect(stateOf(dirOff, "memroot", "code")).toBe("off (dir: code off)");
  // A memory names the rung directly above it, which names the directory in
  // turn — the same ladder a skill inside a switched-off bundle reads as.
  expect(stateOf(dirOff, "mem", "code/report-style-recommendations"))
    .toBe("off (memories: code off)");
  expect(configGateModules(entryOf(dirOff, "ins", "code"), dirOff, []))
    .toEqual(["dir: code"]);
  expect(configGateModules(entryOf(dirOff, "mem", "code/report-style-recommendations"),
                           dirOff, [])).toEqual(["memories: code"]);

  // Own switch off needs no gate to explain it.
  const memOff = withState(dirOff, "mem", "code/report-style-recommendations", "off");
  expect(stateOf(memOff, "mem", "code/report-style-recommendations")).toBe("off");
});

test("modules still gate a subtree row, by union, alongside the directory", () => {
  const rosters: Membership[] = [{ module: "orchestration",
                                   members: ["code/report-style-recommendations"] }];
  expect(configEntryActive(entryOf(MANIFEST, "mem", "code/report-style-recommendations"),
                           MANIFEST, rosters)).toBe(true);

  const bundleOff = withState(MANIFEST, "module", "orchestration", "off");
  expect(configEntryActive(entryOf(bundleOff, "mem", "code/report-style-recommendations"),
                           bundleOff, rosters)).toBe(false);
  expect(stateOf(bundleOff, "mem", "code/report-style-recommendations", rosters))
    .toBe("off (module: orchestration off)");

  // Both closed: the subtree gate reads first, because it is the coarser thing
  // and the one the panel nests the row under.
  const both = withState(bundleOff, "dir", "code", "off");
  expect(configGateModules(entryOf(both, "mem", "code/report-style-recommendations"),
                           both, rosters))
    .toEqual(["memories: code", "module: orchestration"]);
});

test("a memory with no memories row still answers to its directory", () => {
  const noRoot = MANIFEST.filter((r) => r.kind !== "memroot");
  expect(configEntryActive(entryOf(noRoot, "mem", "code/report-style-recommendations"),
                           noRoot, [])).toBe(true);
  const dirOff = withState(noRoot, "dir", "code", "off");
  expect(configEntryActive(entryOf(dirOff, "mem", "code/report-style-recommendations"),
                           dirOff, [])).toBe(false);
  expect(stateOf(dirOff, "mem", "code/report-style-recommendations"))
    .toBe("off (dir: code off)");

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
  expect(configRowLabel("memroot", "code")).toBe("MEMORIES");
  expect(configRowLabel("mem", "code/report-style-recommendations"))
    .toBe("report-style-recommendations");
  expect(configRowLabel("dir", "code")).toBe("code");
  expect(configRowLabel("skill", "firn")).toBe("firn");

  // Enter shells out to `agents path <target>`, and the target is the manifest
  // name: a memory's already carries its directory, and the instruction file
  // answers to the directory alone because that is what `agents path` resolves
  // to that file.
  // Everything under a directory gate is addressed through it, extension and
  // all, which is what ends the collision the slug used to have with its own
  // subtree: `agents off code` closes the directory and nothing else.
  expect(configCliName("mem", "code/report-style-recommendations.md"))
    .toBe("code/report-style-recommendations.md");
  expect(configCliName("mem", "global/house-style.md")).toBe("global/house-style.md");
  expect(configCliName("ins", "code")).toBe("code/AGENTS.md");
  expect(configCliName("memroot", "code")).toBe("code/memories");
  expect(configCliName("dir", "code")).toBe("code");
  expect(configCliName("skill", "firn")).toBe("firn");
});

function configRuntime(entries: Row[], view: string, index = 0,
                       rosters: Membership[] = [], expanded: string[] = []) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configAllEntries: entries.slice(),
    configMemberships: rosters,
    configFilter: view,
    configIndex: index,
    configLoaded: true,
    expandedDirs: expanded,
  };
}

async function snapshotOf(view: string, entries: Row[] = MANIFEST, index = 0,
                       height = 26, rosters: Membership[] = [],
                       expanded: string[] = []) {
  const { renderer, renderOnce, captureCharSnapshot } = await createTestRenderer({
    width: 110, height,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(configRuntime(entries, view, index, rosters,
                                                 expanded));
  await renderOnce();
  const snapshot = captureCharSnapshot();
  renderer.destroy();
  return snapshot.split("\n").map((l) => l.trimEnd());
}

test("the subtree renders nested under its directory, at both depths", async () => {
  const lines = await snapshotOf("agentsmd");
  const at = lines.findIndex((l) => l.includes("GLOBAL"));
  expect(at).toBeGreaterThanOrEqual(0);
  // The directory at the margin, its own two files one step in, its memories
  // one step further — and the second node reads the same way under the same
  // one heading.
  expect(lines.slice(at, at + 8)).toEqual([
    "› ▾ GLOBAL: on",
    "    AGENTS.md: on",
    "  ▾ /tmp/switchboard-fixture/code: on",
    "    AGENTS.md: on",
    "    MEMORIES: on",
    "      report-style-recommendations: on",
    "      agents-switchboard-architecture: on",
    "  ▾ /tmp/switchboard-fixture/north: on",
  ]);
  // Directories are the panel's root level, not rows inside a section about
  // directories: there is no heading over them at all.
  expect(lines).not.toContain("DIRECTORY");
  // A memory prints its own name, not the slug it is addressed by.
  expect(lines.join("\n")).not.toContain("code/report-style-recommendations");
});

test("a directory row shows the place, and the slug stays behind the screen", async () => {
  const lines = await snapshotOf("agentsmd");
  const snapshot = lines.join("\n");
  // `code  /tmp/…/code` said the same thing twice. The path is the half that
  // answers "which directory is this" — two checkouts of one repo share a slug
  // and differ only here — so the path is the row and the slug is not printed.
  expect(lines).toContain("  ▾ /tmp/switchboard-fixture/code: on");
  expect(snapshot).not.toContain("code  /tmp");
  expect(snapshot).not.toContain("north  /tmp");
  // The root scope is the same rule with the root's own path.
  expect(lines).toContain("› ▾ GLOBAL: on");
  expect(snapshot).not.toContain("global  ~");

  // Everything the slug was is still the slug. It addresses the CLI…
  expect(configCliName("dir", "code")).toBe("code");
  // …it is what an `@` reference writes…
  expect(configReferenceText("dir", "code")).toBe("@dir:code ");
  // …it is what a gate note names, so provenance still reads as the CLI does…
  const dirOff = withState(MANIFEST, "dir", "code", "off");
  expect(stateOf(dirOff, "ins", "code")).toBe("off (dir: code off)");
  // …and it is still searchable, so `/code` finds the directory whose path is
  // on screen without the word `code` in it.
  const matched = configQueryRows(
    configViewRows(MANIFEST.slice(), "agentsmd"), "code") as Row[];
  expect(matched.some((r) => r.kind === "dir" && r.name === "code")).toBe(true);
  expect(configRowSearchText(entryOf(MANIFEST, "dir", "code")))
    .toContain("code");

  // A directory the CLI has registered without a path yet falls back to its
  // slug rather than rendering a blank row.
  const pathless = MANIFEST.map((r) =>
    r.kind === "dir" && r.name === "north" ? { ...r, detail: "" } : r);
  expect(await snapshotOf("agentsmd", pathless)).toContain("  ▾ north: on");
});

test("a closed directory darkens its subtree on screen, and says which row did it", async () => {
  const lines = await snapshotOf("agentsmd", withState(MANIFEST, "dir", "code", "off"));
  const at = lines.indexOf("  ▾ /tmp/switchboard-fixture/code: off");
  expect(at).toBeGreaterThanOrEqual(0);
  expect(lines.slice(at + 1, at + 5)).toEqual([
    "    AGENTS.md: off (dir: code off)",
    "    MEMORIES: off (dir: code off)",
    "      report-style-recommendations: off (memories: code off)",
    "      agents-switchboard-architecture: off (memories: code off)",
  ]);
  // The node next door is untouched, and still nests.
  expect(lines).toContain("    AGENTS.md: on");
  expect(lines).toContain("› ▾ GLOBAL: on");
});

test("closing the memories keeps the instruction file, indentation and all", async () => {
  const lines = await snapshotOf("agentsmd",
                              withState(MANIFEST, "memroot", "code", "off"));
  expect(lines).toContain("    AGENTS.md: on");
  expect(lines).toContain("    MEMORIES: off");
  expect(lines).toContain("      report-style-recommendations: off (memories: code off)");
});

test("/config carries the subtrees inside their nodes, sections intact", async () => {
  const lines = await snapshotOf("all", MANIFEST, 0, 26, [], ["global", "code"]);
  const snapshot = lines.join("\n");
  for (const header of ["SETS", "SKILLS", "PLUGINS"]) {
    expect(snapshot).toContain(header);
  }
  // The node's own files read FIRST inside it, before everything it turns on:
  // what the directory says, then what happens to be in force there.
  expect(snapshot.indexOf("AGENTS.md")).toBeLessThan(snapshot.indexOf("SETS"));
  expect(lines).toContain("      report-style-recommendations: on");
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

test("the window follows the cursor into the subtree, and pays for no heading", () => {
  const total = (configViewRows(DEEP.slice(), "agentsmd") as Row[]).length;
  const window = configVisibleCount(total, "agentsmd");
  expect(window).toBeLessThan(total);

  // A node's own files are rows under no heading whatsoever, so however many a
  // directory remembers, the panel is exactly as tall as its rows.
  for (const index of [0, 20, total - 1]) {
    const runtime = configRuntime(DEEP, "agentsmd", index);
    expect(configDetailLines(runtime)).toBe(1 + window);
    expect(detailHeight(runtime)).toBeLessThanOrEqual(PANEL_BUDGET);
  }
  expect(configSectionRows("agentsmd")).toBe(0);
  // The full switchboard sits under the same ceiling with the subtree in it.
  expect(detailHeight(configRuntime(DEEP, "all", 0, [], ["global", "code"])))
    .toBeLessThanOrEqual(PANEL_BUDGET);
});

test("scrolling a deep node is kind-agnostic: it follows the row, not the row's kind", async () => {
  const top = await snapshotOf("agentsmd", DEEP, 0, 40);
  expect(top).toContain("› ▾ GLOBAL: on");
  expect(top).toContain("      memory-00: on");
  // The tail is outside the window at the top of the list.
  expect(top).not.toContain("      memory-39: on");

  const rows = configViewRows(DEEP.slice(), "agentsmd") as Row[];
  const bottom = await snapshotOf("agentsmd", DEEP, rows.length - 1, 40);
  // The cursor is on the last memory and the window came with it, still nested.
  // The marker keeps the leftmost cell on every row, whatever depth the row it
  // is on sits at.
  expect(bottom).toContain("›     memory-39: on");
  expect(bottom).not.toContain("  on  ▾ GLOBAL");
  // Nothing is printed over the rows but the panel's own title line: the window
  // is memories all the way up.
  expect(bottom).not.toContain("DIRECTORY");
  expect(bottom[1]!.includes("memory-")).toBe(true);

  // Mid-list the cursor is centred, with the rows either side of it on screen.
  const middle = await snapshotOf("agentsmd", DEEP, 24, 40);
  const cursor = middle.findIndex((l) => l.startsWith("›"));
  expect(middle[cursor]).toBe("›     memory-20: on");
  expect(middle[cursor - 1]).toBe("      memory-19: on");
  expect(middle[cursor + 1]).toBe("      memory-21: on");
});
