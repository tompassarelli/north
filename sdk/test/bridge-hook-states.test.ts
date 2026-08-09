import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  config_companion_on_p as configCompanionOn,
  config_entry_active_p as configEntryActive,
  config_state_text as configStateText,
  config_toggle_verb as configToggleVerb,
  config_view_rows as configViewRows,
  render_config_panel as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";

// Pin the terminal: the panel reads it for its window math, so the frames below
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

test("skills read before the hooks they move", () => {
  // The relationship runs unit -> hook, so the row you flip sits above the
  // rows it changes. This is the user's stated order.
  expect(kinds(configViewRows(MANIFEST.slice(), "all") as Row[])).toEqual([
    "dir", "dir",
    "skill", "skill",
    "hook", "hook", "hook", "hook", "hook", "hook",
    "module",
    "plugin",
    "other",
  ]);
  expect(kinds(configViewRows(MANIFEST.slice(), "globals") as Row[])).toEqual([
    "dir",
    "skill", "skill",
    "hook", "hook", "hook", "hook", "hook", "hook",
    "module",
    "other",
  ]);
});

test("a hook's activity is derived from permission and its companion unit", () => {
  const bound = (name: string) => MANIFEST.find((r) => r.name === name)!;

  // Permitted and following a skill that is on.
  expect(configEntryActive(bound("firn-guard"), MANIFEST)).toBe(true);
  // Permitted, but the skill it follows is off. Nothing on its own line says so.
  expect(configEntryActive(bound("webdev-guard"), MANIFEST)).toBe(false);
  // A companion is a unit, not a skill: a dir row and a module decide their
  // followers' activity exactly the way a skill row does.
  expect(configEntryActive(bound("profile-guard"), MANIFEST)).toBe(true);
  expect(configEntryActive(bound("routing-guard"), MANIFEST)).toBe(true);
  const profileOff = MANIFEST.map((r) =>
    r.kind === "dir" && r.name === "global" ? { ...r, state: "off" } : r);
  expect(configEntryActive(bound("profile-guard"), profileOff)).toBe(false);
  const moduleOff = MANIFEST.map((r) =>
    r.kind === "module" ? { ...r, state: "off" } : r);
  expect(configEntryActive(bound("routing-guard"), moduleOff)).toBe(false);
  // Unbound and permitted is active by definition.
  expect(configEntryActive(bound("worktree-guard"), MANIFEST)).toBe(true);
  // The user pin wins over everything, bound or not.
  expect(configEntryActive(bound("tripwire-guard"), MANIFEST)).toBe(false);
  expect(configEntryActive(row("hook", "pinned", "disabled", "firn"), MANIFEST)).toBe(false);

  // A companion the manifest does not carry is not an on unit.
  expect(configEntryActive(row("hook", "ghost", "enabled", "nosuch"), MANIFEST)).toBe(false);
  expect(configCompanionOn(MANIFEST, "firn")).toBe(true);
  expect(configCompanionOn(MANIFEST, "global")).toBe(true);
  expect(configCompanionOn(MANIFEST, "orchestration")).toBe(true);
  expect(configCompanionOn(MANIFEST, "webdev")).toBe(false);
  expect(configCompanionOn(MANIFEST, "statusline-script")).toBe(false);
  expect(configCompanionOn(MANIFEST, "nosuch")).toBe(false);
  // The lookup spans every kind but hooks, whose activity is itself derived
  // and never stored — a hook can never be another hook's answer.
  expect(configCompanionOn(MANIFEST, "worktree-guard")).toBe(false);

  // Every other kind stores its own activity.
  expect(configEntryActive(row("skill", "firn", "on"), MANIFEST)).toBe(true);
  expect(configEntryActive(row("other", "statusline-script", "off"), MANIFEST)).toBe(false);
  expect(configEntryActive(row("module", "orchestration", "on"), MANIFEST)).toBe(true);
  expect(configEntryActive(row("dir", "north", "on", "/tmp/x"), MANIFEST)).toBe(true);

  // `on` is the pre-two-axis spelling of the same permission, so a manifest
  // written by an older CLI still renders truthfully instead of reading as a
  // pin.
  expect(configEntryActive(row("hook", "legacy", "on"), MANIFEST)).toBe(true);
  expect(configEntryActive(row("hook", "legacy", "on", "webdev"), MANIFEST)).toBe(false);
});

test("the state column says both axes, and where an off came from", () => {
  const at = (name: string) => {
    const entry = MANIFEST.find((r) => r.name === name)!;
    return configStateText(entry, configEntryActive(entry, MANIFEST));
  };
  expect(at("worktree-guard")).toBe("enabled · on");
  expect(at("firn-guard")).toBe("enabled · on · firn");
  // Provenance is the companion's name whatever kind it is.
  expect(at("profile-guard")).toBe("enabled · on · global");
  expect(at("routing-guard")).toBe("enabled · on · orchestration");
  // Permitted but inactive, and the row names the reason.
  expect(at("webdev-guard")).toBe("enabled · off · webdev");
  // A pin is not an off: no derived activity is reported for it at all.
  expect(at("tripwire-guard")).toBe("disabled");

  // Nothing else grows a second axis.
  expect(configStateText(row("skill", "firn", "on"), true)).toBe("on");
  expect(configStateText(row("other", "statusline-script", "off"), false)).toBe("off");
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

function configRuntime(entries: Row[], view: string) {
  return {
    detailView: "config",
    // What the panel shows.
    configEntries: configViewRows(entries.slice(), view),
    // What it derives activity from. A narrowed view drops the rows that
    // decide it, so the whole manifest is kept alongside.
    configAllEntries: entries.slice(),
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
  };
}

async function frameOf(view: string, entries: Row[] = MANIFEST) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 26,
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

test("the switchboard reads DIRECTORY CONTEXT, SKILLS, HOOKS, MODULES, PLUGINS, OTHER", async () => {
  const frame = await frameOf("all");
  const order = [
    "DIRECTORY CONTEXT", "SKILLS", "HOOKS", "MODULES", "PLUGINS", "OTHER",
  ];
  const seen = order.map((header) => {
    const at = frame.indexOf(header);
    expect(at).toBeGreaterThanOrEqual(0);
    return at;
  });
  expect(seen).toEqual([...seen].sort((a, b) => a - b));
  // The global profile heads the directory section, above the per-directory
  // rows it is read underneath.
  expect(frame.indexOf("global  ~")).toBeGreaterThan(frame.indexOf("DIRECTORY CONTEXT"));
  expect(frame.indexOf("global  ~")).toBeLessThan(frame.indexOf("north  /tmp"));
});

test("each of the hook states renders as itself, whatever kind it follows", async () => {
  const frame = await frameOf("globals");
  // Active, unbound: permitted and running.
  expect(frame).toContain("enabled · on  worktree-guard");
  // Active and bound: the unit it follows is on screen above it.
  expect(frame).toContain("enabled · on · firn  firn-guard");
  // A hook following the global profile, and one following a module, render
  // and derive exactly like one following a skill.
  expect(frame).toContain("enabled · on · global  profile-guard");
  expect(frame).toContain("enabled · on · orchestration  routing-guard");
  // Permitted but inactive, with the reason on the row — the whole point of
  // the companion field.
  expect(frame).toContain("enabled · off · webdev  webdev-guard");
  // A user pin, which no flip will move. Distinct from an off.
  expect(frame).toContain("disabled      tripwire-guard");
  expect(frame).not.toContain("off  tripwire-guard");
});

test("a narrowed /hooks view still knows which of its hooks are running", async () => {
  // The view filters every companion row away entirely; activity is still
  // derived from the manifest the panel kept.
  const frame = await frameOf("hook");
  expect(frame).toContain("hooks");
  expect(frame).not.toContain("SKILLS");
  expect(frame).toContain("enabled · off · webdev  webdev-guard");
  expect(frame).toContain("enabled · on · firn  firn-guard");
  expect(frame).toContain("enabled · on · global  profile-guard");
});

test("one unit flip re-renders every hook bound to it, with no hook line changed", async () => {
  const before = await frameOf("globals");
  expect(before).toContain("enabled · on · global  profile-guard");

  // The manifest the CLI writes back after `agents off global`: the dir line
  // moved, the hook lines are byte-identical. This is why a toggle reloads
  // every row instead of patching the one it flipped — a panel that patched
  // profile-guard's line would show nothing at all, because its line did not
  // change.
  const after = MANIFEST.map((r) =>
    r.kind === "dir" && r.name === "global" ? { ...r, state: "off" } : r);
  expect(after.filter((r) => r.kind === "hook")).toEqual(
    MANIFEST.filter((r) => r.kind === "hook"));

  const frame = await frameOf("globals", after);
  expect(frame).toContain("enabled · off · global  profile-guard");
  expect(frame).not.toContain("enabled · on · global  profile-guard");
  // Hooks following other units are untouched by this one's cascade.
  expect(frame).toContain("enabled · on · firn  firn-guard");
  // The pin is immune to the cascade; the unbound hook never followed anything.
  expect(frame).toContain("disabled      tripwire-guard");
  expect(frame).toContain("enabled · on  worktree-guard");
});
