import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import {
  BoxRenderable, ScrollBoxRenderable, TextRenderable, brightBlack, brightWhite,
} from "@opentui/core";
import {
  banner_box as bannerBox,
  banner_line_segments as bannerLineSegments,
  banner_permissions as bannerPermissions,
  banner_revision as bannerRevision,
  banner_rule_line_p as bannerRuleLine,
  render_conversation_bang as renderConversation,
  roster_row_suppressed_p as rosterRowSuppressed,
  roster_text as rosterText,
  session_banner as sessionBanner,
  session_banner_lines as sessionBannerLines,
  session_banner_runs as sessionBannerRuns,
  transcript_banner_p as transcriptBanner,
  transcript_placeholder as transcriptPlaceholder,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  Agent, make_model as makeModel, snapshot, upsert_agent as upsertAgent,
} from "../src/bridge/generated/north/bridge/model.js";

// The banner is sized against the frame, which is read off the terminal. Pin it
// so the box below is arithmetic rather than a property of whoever's tty ran
// the suite.
Object.defineProperty(process.stdout, "columns", { value: 120, configurable: true });
Object.defineProperty(process.stdout, "rows", { value: 40, configurable: true });

// The placeholder is a pure fold of (label, session status, transcript size,
// working flag). The bug it replaces: "empty and not working" alone, which kept
// claiming a boot in progress after the session reported ready.
test("transcript placeholder follows session state, not just emptiness", () => {
  // Nothing observed yet: still starting.
  expect(transcriptPlaceholder("Main", "", 0, false)).toBe("Starting Main…");
  expect(transcriptPlaceholder("Main", "starting", 0, false)).toBe("Starting Main…");

  // Ready says nothing here any more. "Main is ready." spent the emptiest
  // screen in the frame repeating what the roster header and the strip were
  // already saying; the banner is what that screen is for now.
  expect(transcriptPlaceholder("Main", "ready", 0, false)).toBe("");
  expect(transcriptPlaceholder("Main", "running", 0, false)).toBe("");

  // Working draws its own wave; the placeholder stays out of the way.
  expect(transcriptPlaceholder("Main", "starting", 0, true)).toBe("");
  expect(transcriptPlaceholder("Main", "ready", 0, true)).toBe("");

  // Any transcript content at all replaces the placeholder entirely.
  expect(transcriptPlaceholder("Main", "starting", 1, false)).toBe("");
  expect(transcriptPlaceholder("Main", "ready", 3, false)).toBe("");

  // A dead session must not read as booting.
  expect(transcriptPlaceholder("Main", "offline", 0, false)).toBe("Main is offline.");
  expect(transcriptPlaceholder("Codex Main", "failed", 0, false)).toBe("Codex Main is offline.");
});

test("the banner belongs to exactly one state: ready, empty, and idle", () => {
  expect(transcriptBanner("ready", 0, false)).toBe(true);
  expect(transcriptBanner("running", 0, false)).toBe(true);
  // The states that have something of their own to say keep saying it.
  expect(transcriptBanner("", 0, false)).toBe(false);
  expect(transcriptBanner("starting", 0, false)).toBe(false);
  expect(transcriptBanner("offline", 0, false)).toBe(false);
  expect(transcriptBanner("failed", 0, false)).toBe(false);
  expect(transcriptBanner("error", 0, false)).toBe(false);
  // The disappearance rule is the placeholder's, unchanged: a turn in flight or
  // one line of transcript and the empty screen is not empty any more.
  expect(transcriptBanner("ready", 0, true)).toBe(false);
  expect(transcriptBanner("ready", 1, false)).toBe(false);
});

test("the banner states the session's facts, and says pending for the ones it lacks", () => {
  const lines = sessionBannerLines(
    "1f3c2de78461ee37cbba2f49cfefa28d1d0a87fb", "claude-fable-5", "xhigh",
    "~/code", "acceptEdits") as string[];
  expect(lines).toEqual([
    ">_ North Bridge (1f3c2de7)",
    "",
    "model:       claude-fable-5 xhigh   NORTH_BRIDGE_MODEL to change",
    "directory:   ~/code",
    "permissions: acceptEdits",
  ]);

  // The identity is the staleness handshake's, shortened to what a human would
  // type at git.
  expect(bannerRevision("1f3c2de78461ee37cbba2f49cfefa28d1d0a87fb")).toBe("1f3c2de7");
  expect(bannerRevision("")).toBe("unknown");
  expect(bannerRevision(undefined)).toBe("unknown");

  // The mode where nothing is asked is called what the operator calls it; every
  // other mode is reported by its own name rather than a euphemism.
  expect(bannerPermissions("bypassPermissions")).toBe("YOLO mode");
  expect(bannerPermissions("acceptEdits")).toBe("acceptEdits");
  expect(bannerPermissions("default")).toBe("default");
  expect(bannerPermissions("")).toBe("pending");

  // Facts the session has not reported yet, in the word the rest of the frame
  // already uses for the same gap.
  const bare = sessionBannerLines("", "", "", "", "") as string[];
  expect(bare[0]).toBe(">_ North Bridge (unknown)");
  expect(bare[2]).toBe("model:       pending   NORTH_BRIDGE_MODEL to change");
  expect(bare[3]).toBe("directory:   pending");
  expect(bare[4]).toBe("permissions: pending");

  // There is no /model command in the bridge, so the hint names the thing that
  // actually changes the model rather than inventing one.
  expect(lines[2]).toContain("NORTH_BRIDGE_MODEL");
  expect(lines.join("\n")).not.toContain("/model");
});

test("the box is drawn to the content, and gives way rather than wrapping", () => {
  const boxed = bannerBox(["abc", "de"], 60) as string[];
  expect(boxed).toEqual([
    "╭─────╮",
    "│ abc │",
    "│ de  │",
    "╰─────╯",
  ]);

  // Wide enough to hold the widest line and its borders: the box is a card
  // sized to what it says, not a rule across the frame.
  const wide = sessionBanner("1f3c2de7", "claude-fable-5", "xhigh", "~/code",
                             "acceptEdits", 110) as string[];
  expect(wide).toHaveLength(7);
  expect(wide[0]!.startsWith("╭")).toBe(true);
  expect(wide[6]!.endsWith("╯")).toBe(true);
  const widths = new Set(wide.map((l) => [...l].length));
  expect(widths.size).toBe(1);
  expect([...widths][0]).toBeLessThanOrEqual(110);

  // Narrow: the borders cost four columns and a clipped path says less than the
  // plain line does, so the banner degrades to its own lines. A box that does
  // not fit is not a smaller box, it is a broken one.
  const narrow = sessionBanner("1f3c2de7", "claude-fable-5", "xhigh", "~/code",
                               "acceptEdits", 54) as string[];
  expect(narrow).toHaveLength(5);
  expect(narrow.some((l) => l.includes("╭") || l.includes("│"))).toBe(false);
  expect(narrow[0]).toBe(">_ North Bridge (1f3c2de7)");
  // And nothing it prints can wrap the frame open.
  for (const line of narrow) expect([...line].length).toBeLessThanOrEqual(54 - 4);

  // A single line longer than the whole frame is cut, not wrapped, box or no.
  const long = bannerBox([`~/code/${"deep/".repeat(40)}end`], 60) as string[];
  for (const line of long) expect([...line].length).toBeLessThanOrEqual(60);
  expect(long[1]).toContain("…");
});

function runtimeWith(status: string) {
  return {
    conversation: [] as unknown[],
    working: false,
    workingSince: Date.now(),
    spinnerIndex: 0,
    supervisorId: "exec-supervisor",
    sourceIdentity: "1f3c2de78461ee37cbba2f49cfefa28d1d0a87fb",
    sessionModel: "claude-fable-5",
    sessionEffort: "xhigh",
    sessionCwd: "/tmp/switchboard-fixture/code",
    sessionPermissions: "acceptEdits",
    model: upsertAgent(
      makeModel("list"),
      Agent("exec-supervisor", "Main", status, "Northbridge control session"),
    ),
  };
}

test("agents-pane transcript stops saying Starting, and shows the session instead", async () => {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 90, height: 12,
  });

  const pane = new BoxRenderable(renderer, { id: "agents-pane", flexGrow: 1 });
  const scroll = new ScrollBoxRenderable(renderer, { id: "transcript-scroll", flexGrow: 1 });
  const transcript = new TextRenderable(renderer, { id: "transcript-text", wrapMode: "word" });
  scroll.add(transcript);
  pane.add(scroll);
  renderer.root.add(pane);

  // Boot: the session row exists but has not reported ready yet. No banner —
  // there is nothing to state about a session that has not started.
  transcript.content = renderConversation(runtimeWith("starting"));
  await renderOnce();
  const booting = captureCharFrame();
  expect(booting).toContain("Starting Main");
  expect(booting).not.toContain("North Bridge");

  // The exact transition the screenshot never showed: session.idle -> ready.
  transcript.content = renderConversation(runtimeWith("ready"));
  await renderOnce();
  const ready = captureCharFrame();
  expect(ready).not.toContain("Starting Main");
  // The line the banner replaces, gone.
  expect(ready).not.toContain("Main is ready.");
  expect(ready).toContain("╭");
  expect(ready).toContain(">_ North Bridge (1f3c2de7)");
  expect(ready).toContain("model:       claude-fable-5 xhigh");
  expect(ready).toContain("NORTH_BRIDGE_MODEL to change");
  expect(ready).toContain("directory:   /tmp/switchboard-fixture/code");
  expect(ready).toContain("permissions: acceptEdits");
  expect(ready).toContain("╰");

  // First transcript item: the banner leaves with the placeholder it replaced.
  const withItem = runtimeWith("ready");
  withItem.conversation = [{
    id: "item-1", kind: "assistant", title: "", body: "hello", status: "done", data: null,
  }];
  transcript.content = renderConversation(withItem);
  await renderOnce();
  const answered = captureCharFrame();
  expect(answered).not.toContain("Starting Main");
  expect(answered).not.toContain("North Bridge");
  expect(answered).not.toContain("╭");
  expect(answered).toContain("hello");

  // A session that is gone says so, and states nothing about itself.
  transcript.content = renderConversation(runtimeWith("offline"));
  await renderOnce();
  const offline = captureCharFrame();
  expect(offline).toContain("Main is offline.");
  expect(offline).not.toContain("North Bridge");

  renderer.destroy();
});

// The screenshot bug: the title LINE was coloured bright and every other line
// dim, so the box characters took the colour of whatever line they sat on and
// the frame came out in two tones — bright pipes beside the title, dim
// everywhere else, which reads as a broken box rather than as emphasis.
test("the box is one tone: the border is frame, and only content is coloured", () => {
  expect(bannerRuleLine("╭─────╮")).toBe(true);
  expect(bannerRuleLine("╰─────╯")).toBe(true);
  expect(bannerRuleLine("│ abc │")).toBe(false);

  // A content line is cut where the box stops, so the sides are never part of
  // the run that carries the line's colour.
  expect(bannerLineSegments("│ abc │")).toEqual(["│ ", "abc", " │"]);
  expect(bannerLineSegments("╭─────╮")).toEqual(["╭─────╮", "", ""]);
  // The narrow banner draws no box at all: all content, no frame.
  expect(bannerLineSegments(">_ North Bridge (1f3c2de7)"))
    .toEqual(["", ">_ North Bridge (1f3c2de7)", ""]);

  const runs = sessionBannerRuns(
    sessionBanner("1f3c2de7", "claude-fable-5", "xhigh", "~/code", "acceptEdits", 110),
  ) as Array<{ text: string; tone: string }>;

  // Exactly one bright run, and it is the title's own text — not the line it
  // sits on, and never a border character.
  const title = runs.filter((run) => run.tone === "title");
  expect(title).toHaveLength(1);
  expect(title[0]!.text).toContain(">_ North Bridge (1f3c2de7)");
  expect(title[0]!.text).not.toContain("│");

  // Every box character in the whole banner belongs to a frame run.
  for (const run of runs)
    if (/[╭─╮│╰╯]/.test(run.text)) expect(run.tone).toBe("frame");

  // And the runs still spell the box exactly as it is drawn.
  expect(runs.map((run) => run.text).join(""))
    .toBe((sessionBanner("1f3c2de7", "claude-fable-5", "xhigh", "~/code",
                         "acceptEdits", 110) as string[]).join("\n"));
});

test("the drawn banner paints its border dim on every line, title included", () => {
  const chunks = (renderConversation(runtimeWith("ready")) as {
    chunks: Array<{ text: string; fg?: unknown }>;
  }).chunks;
  const frame = JSON.stringify(brightBlack("x").fg);
  const bright = JSON.stringify(brightWhite("x").fg);

  const bordered = chunks.filter((chunk) => /[╭─╮│╰╯]/.test(chunk.text));
  expect(bordered.length).toBeGreaterThan(0);
  // The line the bug was visible on: the title's row carries borders too.
  for (const chunk of bordered) expect(JSON.stringify(chunk.fg)).toBe(frame);

  const titled = chunks.filter((chunk) => chunk.text.includes("North Bridge"));
  expect(titled).toHaveLength(1);
  expect(JSON.stringify(titled[0]!.fg)).toBe(bright);
});

// The second screenshot complaint: `› Main (ready) — Northbridge control
// session` sat directly above a banner whose entire purpose is to state that
// same session. One of the two is a duplicate, and it is the row.
test("the roster stands down for the banner, and comes back with it", () => {
  expect(rosterRowSuppressed("exec-supervisor", "exec-supervisor", true)).toBe(true);
  // No banner, no suppression — the row is the only place the status appears.
  expect(rosterRowSuppressed("exec-supervisor", "exec-supervisor", false)).toBe(false);
  // Another agent's row is never the banner's duplicate.
  expect(rosterRowSuppressed("exec-worker", "exec-supervisor", true)).toBe(false);
  // Nothing is suppressed on behalf of a session that has no id yet.
  expect(rosterRowSuppressed("exec-supervisor", "", true)).toBe(false);

  const control = snapshot(runtimeWith("ready").model);
  expect(rosterText(control, 0, "exec-supervisor", false))
    .toBe("› Main (ready) — Northbridge control session");
  // With the banner up the pane says nothing rather than reporting "no agents"
  // about a session that is plainly on the screen underneath it.
  expect(rosterText(control, 0, "exec-supervisor", true)).toBe("");
  expect(rosterText(snapshot(makeModel("list")), 0, "exec-supervisor", true))
    .toBe("No agents attached");

  // A worker keeps its row, and the marker still tracks the runtime's index
  // into the full roster rather than into what survived the filter.
  const withWorker = snapshot(upsertAgent(
    runtimeWith("ready").model,
    Agent("exec-worker", "Worker", "running", "landing the fix"),
  ));
  expect(rosterText(withWorker, 1, "exec-supervisor", true))
    .toBe("› Worker (running) — landing the fix");
  expect(rosterText(withWorker, 1, "exec-supervisor", false))
    .toBe("  Main (ready) — Northbridge control session"
      + "\n› Worker (running) — landing the fix");
});

test("the roster row and the banner are never on screen together", async () => {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 90, height: 14,
  });
  const pane = new BoxRenderable(renderer, { id: "agents-pane", flexGrow: 1 });
  const roster = new TextRenderable(renderer, { id: "agents-text", height: 4, wrapMode: "word" });
  const scroll = new ScrollBoxRenderable(renderer, { id: "transcript-scroll", flexGrow: 1 });
  const transcript = new TextRenderable(renderer, { id: "transcript-text", wrapMode: "word" });
  scroll.add(transcript);
  pane.add(roster);
  pane.add(scroll);
  renderer.root.add(pane);

  // Banner state: ready, empty, idle.
  const ready = runtimeWith("ready");
  roster.content = rosterText(snapshot(ready.model), 0, "exec-supervisor", true);
  transcript.content = renderConversation(ready);
  await renderOnce();
  const banner = captureCharFrame();
  expect(banner).toContain(">_ North Bridge (1f3c2de7)");
  expect(banner).not.toContain("Northbridge control session");
  expect(banner).not.toContain("Main (ready)");

  // One line of transcript and the banner is gone, so the row is back — with
  // its status, which is the fact the banner was standing in for.
  const answered = runtimeWith("ready");
  answered.conversation = [{
    id: "item-1", kind: "assistant", title: "", body: "hello", status: "done", data: null,
  }];
  roster.content = rosterText(snapshot(answered.model), 0, "exec-supervisor", false);
  transcript.content = renderConversation(answered);
  await renderOnce();
  const conversing = captureCharFrame();
  expect(conversing).not.toContain("North Bridge (");
  expect(conversing).toContain("Main (ready)");
  expect(conversing).toContain("Northbridge control session");

  renderer.destroy();
});
