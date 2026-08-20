import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  config_unit_active_p as configUnitActive,
  config_entry_active_p as configEntryActive,
  config_state_text as configStateText,
  config_toggle_verb as configToggleVerb,
  config_view_rows as configViewRows,
  render_config_panel_bang as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";

// Pin the terminal: the panel reads it for its window math, so the snapshots below
// are arithmetic rather than a property of whoever's tty ran the suite.
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });

type Row = { kind: string; name: string; state: string; detail: string };

function row(kind: string, name: string, state = "off", detail = ""): Row {
  return { kind, name, state, detail };
}

// Two axes. Every kind but `hook` stores its own activity (`on`/`off`). A hook
// row stores the user's permission (`enabled`/`disabled`) and, when it is
// bound, the unit it follows in the fourth field; its activity is derived from
// both. A companion names a unit of any kind: `firn` is a skill, `global` is
// the profile's dir row, `orchestration` is a module.
const MANIFEST: Row[] = [
  row("dir", "global", "on", "~"),
  row("hook", "firn-guard", "enabled", "firn"),
  row("hook", "webdev-guard", "enabled", "webdev"),
  row("hook", "profile-guard", "enabled", "global"),
  row("hook", "routing-guard", "enabled", "orchestration"),
  row("hook", "worktree-guard", "enabled"),
  row("hook", "tripwire-guard", "disabled"),
  row("skill", "firn", "on"),
  row("skill", "webdev", "off"),
  row("module", "orchestration", "on"),
  row("other", "statusline-script"),
  row("plugin", "typescript-lsp@claude-plugins-official", "on"),
  row("dir", "north", "on", "/tmp/switchboard-fixture/north"),
];

const kinds = (rows: Row[]) => rows.map((r) => r.kind);
const names = (rows: Row[]) => rows.map((r) => r.name);

test("a skill reads with the hooks it declared, and the loose ones read after", () => {
  // The relationship runs set -> skill -> hook, so the row you flip sits above
  // the rows it changes — and a hook that follows a skill is drawn INSIDE it
  // rather than in a block of its own. Only the hooks nobody claimed need a
  // heading of their own.
  expect(kinds(configViewRows(MANIFEST.slice(), "all") as Row[])).toEqual([
    "dir",
    "module",
    "skill", "hook",
    "skill", "hook",
    "hook", "hook", "hook", "hook",
    "plugin",
    "other",
    "dir",
  ]);
  const all = configViewRows(MANIFEST.slice(), "all") as Row[];
  expect(names(all).slice(2, 6)).toEqual([
    "firn", "firn-guard", "webdev", "webdev-guard",
  ]);
  expect(kinds(configViewRows(MANIFEST.slice(), "globals") as Row[])).toEqual([
    "dir",
    "module",
    "skill", "hook",
    "skill", "hook",
    "hook", "hook", "hook", "hook",
    "other",
  ]);
});

test("a hook's activity is derived from permission and its companion unit", () => {
  const bound = (name: string) => MANIFEST.find((r) => r.name === name)!;

  // Permitted and following a skill that is on.
  expect(configEntryActive(bound("firn-guard"), MANIFEST, [])).toBe(true);
  // Permitted, but the skill it follows is off. Nothing on its own line says so.
  expect(configEntryActive(bound("webdev-guard"), MANIFEST, [])).toBe(false);
  // A companion is a unit, not a skill: a dir row and a module decide their
  // followers' activity exactly the way a skill row does.
  expect(configEntryActive(bound("profile-guard"), MANIFEST, [])).toBe(true);
  expect(configEntryActive(bound("routing-guard"), MANIFEST, [])).toBe(true);
  const profileOff = MANIFEST.map((r) =>
    r.kind === "dir" && r.name === "global" ? { ...r, state: "off" } : r);
  expect(configEntryActive(bound("profile-guard"), profileOff, [])).toBe(false);
  const moduleOff = MANIFEST.map((r) =>
    r.kind === "module" ? { ...r, state: "off" } : r);
  expect(configEntryActive(bound("routing-guard"), moduleOff, [])).toBe(false);
  // Unbound and permitted is active by definition.
  expect(configEntryActive(bound("worktree-guard"), MANIFEST, [])).toBe(true);
  // The user pin wins over everything, bound or not.
  expect(configEntryActive(bound("tripwire-guard"), MANIFEST, [])).toBe(false);
  expect(configEntryActive(row("hook", "pinned", "disabled", "firn"), MANIFEST, [])).toBe(false);

  // A companion the manifest does not carry is not an active unit.
  expect(configEntryActive(row("hook", "ghost", "enabled", "nosuch"), MANIFEST, [])).toBe(false);
  expect(configUnitActive(MANIFEST, [], "firn")).toBe(true);
  expect(configUnitActive(MANIFEST, [], "global")).toBe(true);
  expect(configUnitActive(MANIFEST, [], "orchestration")).toBe(true);
  expect(configUnitActive(MANIFEST, [], "webdev")).toBe(false);
  expect(configUnitActive(MANIFEST, [], "statusline-script")).toBe(false);
  expect(configUnitActive(MANIFEST, [], "nosuch")).toBe(false);
  // The companion lookup spans every kind but hooks, whose activity is itself
  // derived — a hook can never be another hook's answer, even though asking
  // after that hook by name has a perfectly good answer.
  expect(configUnitActive(MANIFEST, [], "worktree-guard")).toBe(true);
  expect(configEntryActive(row("hook", "follower", "enabled", "worktree-guard"),
                           MANIFEST, [])).toBe(false);

  // Every other kind stores its own activity.
  expect(configEntryActive(row("skill", "firn", "on"), MANIFEST, [])).toBe(true);
  expect(configEntryActive(row("other", "statusline-script", "off"), MANIFEST, [])).toBe(false);
  expect(configEntryActive(row("module", "orchestration", "on"), MANIFEST, [])).toBe(true);
  expect(configEntryActive(row("dir", "north", "on", "/tmp/x"), MANIFEST, [])).toBe(true);

  // `on` was the pre-two-axis spelling of a hook's permission and the panel
  // used to read it as one. The CLI normalises every hook row it touches and
  // the migration has landed, so the shape cannot occur; a row still spelled
  // that way is not permission, and the panel no longer guesses that it is.
  expect(configEntryActive(row("hook", "legacy", "on"), MANIFEST, [])).toBe(false);
  expect(configStateText(row("hook", "legacy", "on"), MANIFEST, [], false)).toBe("disabled");
  // The two spellings that do occur are untouched by the removal.
  expect(configEntryActive(row("hook", "live", "enabled"), MANIFEST, [])).toBe(true);
  expect(configEntryActive(row("hook", "pinned", "disabled"), MANIFEST, [])).toBe(false);
});

// One state token per row, and a qualifier only where the row diverges from
// what that token would let you assume. The word `enabled` is gone: it was
// spent on the axis that always agreed with the answer.
test("a row states one thing, and explains itself only when it diverges", () => {
  const at = (name: string, nested = false) => {
    const entry = MANIFEST.find((r) => r.name === name)!;
    return configStateText(
      entry, MANIFEST, [], configEntryActive(entry, MANIFEST, []), nested);
  };
  expect(at("worktree-guard")).toBe("on");
  // Provenance is the companion's kind AND name, so the unit a hook follows
  // cannot be misread as another hook — but only where the row is not already
  // drawn underneath that unit.
  expect(at("firn-guard")).toBe("on · skill: firn");
  expect(at("firn-guard", true)).toBe("on");
  expect(at("profile-guard")).toBe("on · dir: global");
  expect(at("routing-guard")).toBe("on · module: orchestration");
  // Permitted but not running: the one case that owes an explanation, and it
  // owes it whether or not it is nested, because the reason is not provenance.
  expect(at("webdev-guard")).toBe("off (skill: webdev off)");
  expect(at("webdev-guard", true)).toBe("off (skill: webdev off)");
  // A pin is not an off: no derived activity is reported for it at all.
  expect(at("tripwire-guard")).toBe("disabled");

  // Nothing anywhere prints the word the two-axis model used to lead with.
  for (const entry of MANIFEST)
    expect(configStateText(entry, MANIFEST, [], configEntryActive(entry, MANIFEST, []), false))
      .not.toContain("enabled");

  // Nothing else grows a second axis.
  expect(configStateText(row("skill", "firn", "on"), MANIFEST, [], true, false)).toBe("on");
  expect(configStateText(row("other", "statusline-script", "off"), MANIFEST, [], false, false))
    .toBe("off");
});

test("space flips the axis the row stores, through the same two verbs", () => {
  // A hook: the user axis only. The CLI turns `off` into a pin and `on` into
  // clearing one; the cascade math is its business, not the panel's.
  expect(configToggleVerb("enabled")).toBe("off");
  expect(configToggleVerb("disabled")).toBe("on");
  // A skill (and every other kind): its own activity.
  expect(configToggleVerb("on")).toBe("off");
  expect(configToggleVerb("off")).toBe("on");
});

function configRuntime(entries: Row[], view: string, expanded: string[] = []) {
  return {
    detailView: "config",
    // What the panel shows.
    configEntries: configViewRows(entries.slice(), view),
    // What it derives activity from. A narrowed view drops the rows that
    // decide it, so the whole manifest is kept alongside.
    configAllEntries: entries.slice(),
    configMemberships: [],
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
    expandedDirs: expanded,
  };
}

async function snapshotOf(view: string, entries: Row[] = MANIFEST,
                       expanded: string[] = []) {
  const { renderer, renderOnce, captureCharSnapshot } = await createTestRenderer({
    width: 110, height: 26,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(configRuntime(entries, view, expanded));
  await renderOnce();
  const snapshot = captureCharSnapshot();
  renderer.destroy();
  return snapshot;
}

test("the stack inside a node reads SETS, SKILLS > MODULES, HOOKS, PLUGINS, OTHER", async () => {
  const snapshot = await snapshotOf("all", MANIFEST, ["global"]);
  const order = [
    "SETS", "SKILLS", "MODULES", "HOOKS", "PLUGINS", "OTHER",
  ];
  const seen = order.map((header) => {
    const at = snapshot.indexOf(header);
    expect(at).toBeGreaterThanOrEqual(0);
    return at;
  });
  expect(seen).toEqual([...seen].sort((a, b) => a - b));
  // The root node heads the tree, above the per-directory nodes read on top of
  // it, and its stack is inside it rather than beside it. Nothing is printed
  // over the directories themselves.
  expect(snapshot).not.toContain("DIRECTORY");
  expect(snapshot.indexOf("▾ GLOBAL")).toBeLessThan(snapshot.indexOf("/tmp/switchboard-fixture/north"));
  expect(snapshot.indexOf("SETS")).toBeLessThan(snapshot.indexOf("/tmp/switchboard-fixture/north"));
});

test("each of the hook states renders as itself, whatever kind it follows", async () => {
  const snapshot = await snapshotOf("globals");
  // The row reads name first and state after: the name is what you are looking
  // for, and the state answers the question you ask once you have found it.
  expect(snapshot).toContain("worktree-guard: on");
  // Drawn under the skill that claims it: the parent row is the provenance, so
  // the child says one token and no more. Its KIND is said instead, because the
  // heading over it names its parent's kind and not its own.
  expect(snapshot).toContain("hook · firn-guard: on");
  // A hook following the global profile, and one following a module, render
  // and derive exactly like one following a skill — and neither is nested under
  // its claimant here, so both still name it.
  expect(snapshot).toContain("profile-guard: on · dir: global");
  expect(snapshot).toContain("routing-guard: on · module: orchestration");
  // Permitted but inactive: the reason, because the row diverges from what `on`
  // would have let you assume.
  expect(snapshot).toContain("hook · webdev-guard: off (skill: webdev off)");
  // A user pin, which no flip will move. Distinct from an off.
  expect(snapshot).toContain("tripwire-guard: disabled");
  expect(snapshot).not.toContain("tripwire-guard: off");
  // The word the two-axis model used to lead every hook row with is gone.
  expect(snapshot).not.toContain("enabled");
});

test("a narrowed /hooks view still knows which of its hooks are running", async () => {
  // The view filters every companion row away entirely; activity is still
  // derived from the manifest the panel kept.
  const snapshot = await snapshotOf("hook");
  expect(snapshot).toContain("hooks");
  // No skill row is admitted, so no hook is claimed by one: they all read under
  // the node's HOOKS heading, which is the truth about what is on screen.
  expect(snapshot).not.toContain("SKILLS");
  expect(snapshot).toContain("HOOKS");
  // Nothing is nested here, so provenance is the row's own to state — and the
  // HOOKS heading already says what kind they are, so no row tags itself.
  expect(snapshot).toContain("webdev-guard: off (skill: webdev off)");
  expect(snapshot).toContain("firn-guard: on · skill: firn");
  expect(snapshot).toContain("profile-guard: on · dir: global");
  expect(snapshot).not.toContain("hook · ");
});

test("one unit flip re-renders every hook bound to it, with no hook line changed", async () => {
  const before = await snapshotOf("globals");
  expect(before).toContain("profile-guard: on · dir: global");

  // The manifest the CLI writes back after `agents off global`: the dir line
  // moved, the hook lines are byte-identical. This is why a toggle reloads
  // every row instead of patching the one it flipped — a panel that patched
  // profile-guard's line would show nothing at all, because its line did not
  // change.
  const after = MANIFEST.map((r) =>
    r.kind === "dir" && r.name === "global" ? { ...r, state: "off" } : r);
  expect(after.filter((r) => r.kind === "hook")).toEqual(
    MANIFEST.filter((r) => r.kind === "hook"));

  const snapshot = await snapshotOf("globals", after);
  // The claimant went off, so the hook diverges and names the claimant as the
  // reason rather than as provenance.
  expect(snapshot).toContain("profile-guard: off (dir: global off)");
  expect(snapshot).not.toContain("profile-guard: on");
  // Hooks following other units are untouched by this one's cascade.
  expect(snapshot).toContain("hook · firn-guard: on");
  // The pin is immune to the cascade; the unbound hook never followed anything.
  expect(snapshot).toContain("tripwire-guard: disabled");
  expect(snapshot).toContain("worktree-guard: on");
});
