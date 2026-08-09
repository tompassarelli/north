import { expect, test } from "bun:test";
import { createTestRenderer } from "@opentui/core/testing";
import { BoxRenderable, ScrollBoxRenderable, TextRenderable } from "@opentui/core";
import {
  render_conversation_bang as renderConversation,
  transcript_placeholder as transcriptPlaceholder,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  Agent, make_model as makeModel, upsert_agent as upsertAgent,
} from "../src/bridge/generated/north/bridge/model.js";

// The placeholder is a pure fold of (label, session status, transcript size,
// working flag). The bug it replaces: "empty and not working" alone, which kept
// claiming a boot in progress after the session reported ready.
test("transcript placeholder follows session state, not just emptiness", () => {
  // Nothing observed yet: still starting.
  expect(transcriptPlaceholder("Main", "", 0, false)).toBe("Starting Main…");
  expect(transcriptPlaceholder("Main", "starting", 0, false)).toBe("Starting Main…");

  // The state the roster header and strip already show: ready/running.
  expect(transcriptPlaceholder("Main", "ready", 0, false)).toBe("Main is ready.");
  expect(transcriptPlaceholder("Main", "running", 0, false)).toBe("Main is ready.");

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

function runtimeWith(status: string) {
  return {
    conversation: [] as unknown[],
    working: false,
    workingSince: Date.now(),
    spinnerIndex: 0,
    supervisorId: "exec-supervisor",
    model: upsertAgent(
      makeModel("list"),
      Agent("exec-supervisor", "Main", status, "Northbridge control session"),
    ),
  };
}

test("agents-pane transcript stops saying Starting once the session is ready", async () => {
  const { renderer, renderOnce, captureCharFrame } = await createTestRenderer({
    width: 60, height: 8,
  });

  const pane = new BoxRenderable(renderer, { id: "agents-pane", flexGrow: 1 });
  const scroll = new ScrollBoxRenderable(renderer, { id: "transcript-scroll", flexGrow: 1 });
  const transcript = new TextRenderable(renderer, { id: "transcript-text", wrapMode: "word" });
  scroll.add(transcript);
  pane.add(scroll);
  renderer.root.add(pane);

  // Boot: the session row exists but has not reported ready yet.
  transcript.content = renderConversation(runtimeWith("starting"));
  await renderOnce();
  expect(captureCharFrame()).toContain("Starting Main");

  // The exact transition the screenshot never showed: session.idle -> ready.
  transcript.content = renderConversation(runtimeWith("ready"));
  await renderOnce();
  const ready = captureCharFrame();
  expect(ready).not.toContain("Starting Main");
  expect(ready).toContain("Main is ready.");

  // First transcript item: the placeholder leaves entirely.
  const withItem = runtimeWith("ready");
  withItem.conversation = [{
    id: "item-1", kind: "assistant", title: "", body: "hello", status: "done", data: null,
  }];
  transcript.content = renderConversation(withItem);
  await renderOnce();
  const answered = captureCharFrame();
  expect(answered).not.toContain("Starting Main");
  expect(answered).not.toContain("Main is ready.");
  expect(answered).toContain("hello");

  renderer.destroy();
});
