import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import {
  config_entry_active_p as configEntryActive,
  config_skill_on_p as configSkillOn,
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

// Two axes. A skill row stores its activity (`on`/`off`). A hook row stores the
// user's permission (`enabled`/`disabled`) and, when it is bound, the skill it
// follows in the fourth field; its activity is derived from both.
const MANIFEST: Row[] = [
  row("item", "agents-md", "on"),
  row("item", "statusline"),
  row("hook", "firn-guard", "enabled", "firn"),
  row("hook", "webdev-guard", "enabled", "webdev"),
  row("hook", "worktree-guard", "enabled"),
  row("hook", "tripwire-guard", "disabled"),
  row("skill", "firn", "on"),
  row("skill", "webdev", "off"),
  row("plugin", "typescript-lsp@claude-plugins-official", "on"),
  row("dir", "north", "on", "/tmp/switchboard-fixture/north"),
];

const kinds = (rows: Row[]) => rows.map((r) => r.kind);

test("skills read before the hooks they move", () => {
  // The relationship runs skill -> hook, so the row you flip sits above the
  // rows it changes. This is the user's stated order.
  expect(kinds(configViewRows(MANIFEST.slice(), "all") as Row[])).toEqual([
    "item", "item",
    "dir",
    "skill", "skill",
    "hook", "hook", "hook", "hook",
    "plugin",
  ]);
  expect(kinds(configViewRows(MANIFEST.slice(), "globals") as Row[])).toEqual([
    "item", "item", "skill", "skill", "hook", "hook", "hook", "hook",
  ]);
});

test("a hook's activity is derived from permission and its companion skill", () => {
  const bound = (name: string) => MANIFEST.find((r) => r.name === name)!;

  // Permitted and following a skill that is on.
  expect(configEntryActive(bound("firn-guard"), MANIFEST)).toBe(true);
  // Permitted, but the skill it follows is off. Nothing on its own line says so.
  expect(configEntryActive(bound("webdev-guard"), MANIFEST)).toBe(false);
  // Unbound and permitted is active by definition.
  expect(configEntryActive(bound("worktree-guard"), MANIFEST)).toBe(true);
  // The user pin wins over everything, bound or not.
  expect(configEntryActive(bound("tripwire-guard"), MANIFEST)).toBe(false);
  expect(configEntryActive(row("hook", "pinned", "disabled", "firn"), MANIFEST)).toBe(false);

  // A companion the manifest does not carry is not an on skill.
  expect(configEntryActive(row("hook", "ghost", "enabled", "nosuch"), MANIFEST)).toBe(false);
  expect(configSkillOn(MANIFEST, "firn")).toBe(true);
  expect(configSkillOn(MANIFEST, "webdev")).toBe(false);
  expect(configSkillOn(MANIFEST, "nosuch")).toBe(false);

  // Every other kind stores its own activity.
  expect(configEntryActive(row("skill", "firn", "on"), MANIFEST)).toBe(true);
  expect(configEntryActive(row("item", "statusline", "off"), MANIFEST)).toBe(false);
  expect(configEntryActive(row("dir", "north", "on", "/tmp/x"), MANIFEST)).toBe(true);

  // `on` is the pre-two-axis spelling of the same permission, so a manifest
  // written by the older CLI still renders truthfully instead of reading as a
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
  // Permitted but inactive, and the row names the reason.
  expect(at("webdev-guard")).toBe("enabled · off · webdev");
  // A pin is not an off: no derived activity is reported for it at all.
  expect(at("tripwire-guard")).toBe("disabled");

  // Nothing else grows a second axis.
  expect(configStateText(row("skill", "firn", "on"), true)).toBe("on");
  expect(configStateText(row("item", "statusline", "off"), false)).toBe("off");
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
    // What it derives activity from. A narrowed view drops the skill rows that
    // decide it, so the whole manifest is kept alongside.
    configAllEntries: entries.slice(),
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
  };
}

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

test("the switchboard reads GLOBALS, DIRECTORY CONTEXT, SKILLS, HOOKS, PLUGINS", async () => {
  const frame = await frameOf("all");
  const order = ["GLOBALS", "DIRECTORY CONTEXT", "SKILLS", "HOOKS", "PLUGINS"];
  const seen = order.map((header) => {
    const at = frame.indexOf(header);
    expect(at).toBeGreaterThanOrEqual(0);
    return at;
  });
  expect(seen).toEqual([...seen].sort((a, b) => a - b));
});

test("each of the three hook states renders as itself", async () => {
  const frame = await frameOf("globals");
  // Active, unbound: permitted and running.
  expect(frame).toContain("enabled · on  worktree-guard");
  // Active and bound: the skill it follows is on screen above it.
  expect(frame).toContain("enabled · on · firn  firn-guard");
  // Permitted but inactive, with the reason on the row — the whole point of
  // the companion field.
  expect(frame).toContain("enabled · off · webdev  webdev-guard");
  // A user pin, which no skill flip will move. Distinct from an off.
  expect(frame).toContain("disabled      tripwire-guard");
  expect(frame).not.toContain("off  tripwire-guard");
});

test("a narrowed /hooks view still knows which of its hooks are running", async () => {
  // The view filters skill rows away entirely; activity is still derived from
  // the manifest the panel kept.
  const frame = await frameOf("hook");
  expect(frame).toContain("hooks");
  expect(frame).not.toContain("SKILLS");
  expect(frame).toContain("enabled · off · webdev  webdev-guard");
  expect(frame).toContain("enabled · on · firn  firn-guard");
});

test("one skill flip re-renders every hook bound to it, with no hook line changed", async () => {
  const before = await frameOf("globals");
  expect(before).toContain("enabled · on · firn  firn-guard");

  // The manifest the CLI writes back after `agents off firn`: the skill line
  // moved, the hook lines are byte-identical. This is why a toggle reloads
  // every row instead of patching the one it flipped — a panel that patched
  // firn-guard's line would show nothing at all, because its line did not
  // change.
  const after = MANIFEST.map((r) =>
    r.kind === "skill" && r.name === "firn" ? { ...r, state: "off" } : r);
  expect(after.filter((r) => r.kind === "hook")).toEqual(
    MANIFEST.filter((r) => r.kind === "hook"));

  const frame = await frameOf("globals", after);
  expect(frame).toContain("enabled · off · firn  firn-guard");
  expect(frame).not.toContain("enabled · on · firn  firn-guard");
  // The pin is immune to the cascade; the unbound hook never followed anything.
  expect(frame).toContain("disabled      tripwire-guard");
  expect(frame).toContain("enabled · on  worktree-guard");
});
