import { afterAll, expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, TextRenderable } from "@opentui/core";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  config_entry_active_p as configEntryActive,
  config_gate_modules as configGateModules,
  config_membership_of_json as configMembershipOfJson,
  config_module_members as configModuleMembers,
  config_state_text as configStateText,
  config_unit_active_p as configUnitActive,
  config_view_rows as configViewRows,
  load_config_memberships_bang as loadConfigMemberships,
  render_config_panel_bang as renderConfigPanel,
} from "../src/bridge/generated/north/bridge/app.js";

// Pin the terminal: the panel reads it for its window math, so the frames below
// are arithmetic rather than a property of whoever's tty ran the suite.
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });

type Row = { kind: string; name: string; state: string; detail: string };
type Membership = { module: string; members: string[] };

function row(kind: string, name: string, state = "off", detail = ""): Row {
  return { kind, name, state, detail };
}
function bundle(module: string, ...members: string[]): Membership {
  return { module, members };
}

// Membership is NOT in the manifest — these rows are exactly what they were
// before any bundle existed. `repo-safety` belongs to two modules and its row
// says nothing about either.
const MANIFEST: Row[] = [
  row("dir", "global", "on", "~"),
  row("skill", "firn", "on"),
  row("skill", "webdev", "on"),
  row("skill", "repo-safety", "on"),
  row("hook", "firn-guard", "enabled", "firn"),
  row("hook", "worktree-guard", "enabled", "repo-safety"),
  row("hook", "solo-guard", "enabled"),
  row("module", "dev-core", "on"),
  row("module", "tooling", "on"),
  row("module", "everything", "on"),
  row("module", "empty-bundle", "on"),
  row("other", "statusline-script", "on"),
];

// dev-core and tooling share `repo-safety`; `everything` holds dev-core, so the
// chain is module -> module -> unit.
const ROSTERS: Membership[] = [
  bundle("dev-core", "firn", "repo-safety"),
  bundle("tooling", "repo-safety", "webdev"),
  bundle("everything", "dev-core"),
  bundle("empty-bundle"),
];

const withState = (rows: Row[], name: string, state: string) =>
  rows.map((r) => (r.name === name ? { ...r, state } : r));

const active = (name: string, rows = MANIFEST, rosters = ROSTERS) =>
  configUnitActive(rows, rosters, name);

const stateOf = (name: string, rows = MANIFEST, rosters = ROSTERS) => {
  const entry = rows.find((r) => r.name === name)!;
  return configStateText(entry, rows, rosters,
                         configEntryActive(entry, rows, rosters), false);
};

test("a unit in no module is exactly what its own switch says", () => {
  // The formula has to leave the pre-bundle world alone: no roster, no gate.
  expect(active("statusline-script")).toBe(true);
  expect(active("global")).toBe(true);
  expect(active("solo-guard")).toBe(true);
  expect(stateOf("statusline-script")).toBe("on");
  expect(configEntryActive(row("skill", "loner", "on"), MANIFEST, [])).toBe(true);
  expect(configGateModules(row("skill", "loner", "on"), MANIFEST, ROSTERS))
    .toEqual([]);
});

test("union: one module on is enough, and every module off is the reason", () => {
  // repo-safety is in dev-core and tooling. Either one on composes it.
  expect(active("repo-safety")).toBe(true);
  const devOff = withState(MANIFEST, "dev-core", "off");
  expect(active("repo-safety", devOff)).toBe(true);
  expect(stateOf("repo-safety", devOff)).toBe("on");
  expect(configGateModules(devOff[3]!, devOff, ROSTERS)).toEqual([]);

  // Both holders off: its own switch is still on, and the row has to say what
  // is actually holding it — both bundles, because either would release it.
  const bothOff = withState(devOff, "tooling", "off");
  expect(active("repo-safety", bothOff)).toBe(false);
  expect(stateOf("repo-safety", bothOff)).toBe("off (module: dev-core, module: tooling off)");
  expect(configGateModules(bothOff[3]!, bothOff, ROSTERS))
    .toEqual(["module: dev-core", "module: tooling"]);

  // A member whose own switch is off needs no bundle to explain it.
  const ownOff = withState(bothOff, "repo-safety", "off");
  expect(stateOf("repo-safety", ownOff)).toBe("off");
});

test("the gate is the module's activity, not its switch — modules hold modules", () => {
  // dev-core is on AND held by `everything`, which is on: the chain is open.
  expect(active("dev-core")).toBe(true);
  expect(active("firn")).toBe(true);

  // Switch off the outer bundle. dev-core's own row still says on, and it is
  // not composing; nor is anything dev-core holds, one link further down.
  const outerOff = withState(MANIFEST, "everything", "off");
  expect(active("dev-core", outerOff)).toBe(false);
  expect(stateOf("dev-core", outerOff)).toBe("off (module: everything off)");
  expect(active("firn", outerOff)).toBe(false);
  expect(stateOf("firn", outerOff)).toBe("off (module: dev-core off)");
  // repo-safety survives it: dev-core is closed, but tooling still holds it.
  expect(active("repo-safety", outerOff)).toBe(true);
});

test("a hook follows its companion's activity, gate and all", () => {
  // firn-guard follows firn, which is on and composing.
  expect(active("firn-guard")).toBe(true);
  expect(stateOf("firn-guard")).toBe("on · skill: firn");

  // Close the bundle over firn. Nothing on firn-guard's line changed and it is
  // no longer running: the companion is not composing, so neither is it.
  const devOff = withState(MANIFEST, "dev-core", "off");
  expect(active("firn-guard", devOff)).toBe(false);
  expect(stateOf("firn-guard", devOff)).toBe("off (skill: firn off)");

  // A hook can be a member itself; then the bundle gates the hook directly and
  // the row names it rather than the companion.
  const hookRoster = [...ROSTERS, bundle("tooling-hooks", "solo-guard")];
  const rows = [...MANIFEST, row("module", "tooling-hooks", "off")];
  expect(configUnitActive(rows, hookRoster, "solo-guard")).toBe(false);
  expect(stateOf("solo-guard", rows, hookRoster))
    .toBe("off (module: tooling-hooks off)");
  // A pin still outranks every derivation: no bundle can un-pin a hook.
  const pinned = withState(rows, "solo-guard", "disabled");
  expect(stateOf("solo-guard", pinned, hookRoster)).toBe("disabled");
});

test("a cycle derives inactive instead of looping", () => {
  // Two bundles holding each other. Nothing in the cycle can be shown to be
  // composing, so nothing in it is — and the walk terminates.
  const rows = [
    row("module", "ouroboros-a", "on"),
    row("module", "ouroboros-b", "on"),
    row("skill", "caught", "on"),
  ];
  const rosters = [
    bundle("ouroboros-a", "ouroboros-b"),
    bundle("ouroboros-b", "ouroboros-a", "caught"),
  ];
  expect(configUnitActive(rows, rosters, "ouroboros-a")).toBe(false);
  expect(configUnitActive(rows, rosters, "ouroboros-b")).toBe(false);
  expect(configUnitActive(rows, rosters, "caught")).toBe(false);
  expect(stateOf("caught", rows, rosters)).toBe("off (module: ouroboros-b off)");
});

test("a roster file is read as data, and a broken one is not fatal", () => {
  const parsed = configMembershipOfJson("dev-core",
                                        '{"members": ["firn", "repo-safety"]}');
  expect(parsed.module).toBe("dev-core");
  expect(parsed.members).toEqual(["firn", "repo-safety"]);

  // Absent, malformed, and wrong-shaped all mean "this module holds nobody",
  // which is a fact the panel can render, unlike an exception.
  expect(configMembershipOfJson("empty", '{"members": []}').members).toEqual([]);
  expect(configMembershipOfJson("junk", "not json at all").members).toEqual([]);
  expect(configMembershipOfJson("shapeless", '{"members": 3}').members).toEqual([]);
  expect(configMembershipOfJson("silent", "{}").members).toEqual([]);

  // A file that exists and a file that does not are different answers.
  expect(configModuleMembers(ROSTERS, "dev-core")).toEqual(["firn", "repo-safety"]);
  expect(configModuleMembers(ROSTERS, "no-such-module")).toBe(null);
});

const fixtureRoot = mkdtempSync(join(tmpdir(), "switchboard-modules-"));
afterAll(() => rmSync(fixtureRoot, { recursive: true, force: true }));

test("the roster directory is read from disk, and its absence is not an error", async () => {
  const dir = join(fixtureRoot, "modules.d");
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "dev-core.json"),
                JSON.stringify({ members: ["firn", "repo-safety"] }));
  writeFileSync(join(dir, "tooling.json"), JSON.stringify({ members: [] }));
  // Not a roster: the reader takes the .json files and leaves the rest.
  writeFileSync(join(dir, "README.md"), "not a roster");

  const loaded = await loadConfigMemberships(dir) as Membership[];
  expect(loaded.map((m) => m.module).sort()).toEqual(["dev-core", "tooling"]);
  expect(loaded.find((m) => m.module === "dev-core")!.members)
    .toEqual(["firn", "repo-safety"]);
  expect(loaded.find((m) => m.module === "tooling")!.members).toEqual([]);

  // No modules.d at all — no module has ever been given members, or this
  // machine has no nixos-config checkout. Every unit then gates on its own
  // switch, exactly as it did before bundles existed.
  expect(await loadConfigMemberships(join(fixtureRoot, "nowhere"))).toEqual([]);
});

function configRuntime(entries: Row[], view: string, rosters: Membership[]) {
  return {
    detailView: "config",
    configEntries: configViewRows(entries.slice(), view),
    configAllEntries: entries.slice(),
    configMemberships: rosters,
    configFilter: view,
    configIndex: 0,
    configLoaded: true,
  };
}

async function frameOf(view: string, entries = MANIFEST, rosters = ROSTERS) {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 110, height: 26,
  });
  const panel = new BoxRenderable(renderer, { id: "detail-panel", flexGrow: 1 });
  const body = new TextRenderable(renderer, { id: "detail-text" });
  panel.add(body);
  renderer.root.add(panel);
  body.content = renderConfigPanel(configRuntime(entries, view, rosters));
  await renderOnce();
  const frame = captureCharFrame();
  renderer.destroy();
  return frame;
}

// One rendered row, without the cursor gutter or the frame's right padding.
function lineWith(frame: string, needle: string): string {
  const line = frame.split("\n").find((l) => l.includes(needle));
  expect(line).toBeDefined();
  return line!.replace(/^[›\s]+/, "").trimEnd();
}

test("a module row carries the size of its roster, when it has one", async () => {
  const frame = await frameOf("module",
                              [...MANIFEST, row("module", "unrostered", "on")]);
  expect(lineWith(frame, "dev-core")).toBe("dev-core: on  2 members");
  // Singular for one, because the row is a sentence about this bundle.
  expect(lineWith(frame, "everything")).toBe("everything: on  1 member");
  // A file with nobody in it says so; a module modules.d carries no file for
  // says nothing at all, because "no roster yet" is a different fact.
  expect(lineWith(frame, "empty-bundle")).toBe("empty-bundle: on  0 members");
  expect(lineWith(frame, "unrostered")).toBe("unrostered: on");
});

test("union on screen: a shared member survives one bundle going off", async () => {
  // tooling off, dev-core still on: repo-safety belongs to both, so its row
  // reads as the plain `on` it would have read before bundles existed. webdev
  // belongs to tooling alone and does not.
  const frame = await frameOf("globals", withState(MANIFEST, "tooling", "off"));
  expect(lineWith(frame, "repo-safety")).toBe("repo-safety: on");
  expect(lineWith(frame, "webdev")).toBe("webdev: off (module: tooling off)");
});

test("flipping a module re-renders its members' activity, with no member line changed", async () => {
  const before = await frameOf("globals");
  expect(before).toContain("firn: on");
  expect(before).toContain("repo-safety: on");
  expect(before).toContain("webdev: on");
  expect(before).toContain("hook · firn-guard: on");

  // The manifest the CLI writes back after `agents off dev-core`: one module
  // row moved and every member row is byte-identical — membership never lived
  // in the manifest, so a panel that patched only the flipped row would show
  // nothing at all.
  const after = withState(MANIFEST, "dev-core", "off");
  expect(after.filter((r) => r.name !== "dev-core"))
    .toEqual(MANIFEST.filter((r) => r.name !== "dev-core"));

  const frame = await frameOf("globals", after);
  // The member's activity column moved, and says which bundle did it.
  expect(lineWith(frame, "firn:")).toBe("firn: off (module: dev-core off)");
  // And the hook following that member went with it, one link further out.
  expect(frame).toContain("hook · firn-guard: off (skill: firn off)");
  // repo-safety is in dev-core too, and tooling still holds it: union, on
  // screen, in the same flip that darkened firn.
  expect(lineWith(frame, "repo-safety")).toBe("repo-safety: on");
  // Units in no bundle at all are untouched.
  expect(lineWith(frame, "statusline-script")).toBe("statusline-script: on");
  expect(frame).toContain("solo-guard: on");
});

test("the chain renders end to end: outer bundle off darkens two links", async () => {
  const frame = await frameOf("globals", withState(MANIFEST, "everything", "off"));
  expect(frame).toContain("dev-core: off (module: everything off)");
  expect(frame).toContain("firn: off (module: dev-core off)");
  expect(frame).toContain("hook · firn-guard: off (skill: firn off)");
  // The outer bundle itself is off on its own account and says only that.
  expect(frame).toContain("everything: off  1 member");
});
