import { expect, test } from "bun:test";
import {
  "handle-local-command!" as handleLocalCommand,
  "parse-bridge-stream!" as parseBridgeStream,
  "project-conversation" as projectConversation,
} from "../src/bridge/generated/north/bridge/app.js";
import { renderWireEvent } from "../src/bridge/cli";
import {
  WireEventWriter,
  wireMessageId,
  wireModelCallId,
  wireRunId,
  type WireEvent,
} from "../src/wire";

interface ConversationItem {
  id: string;
  body: string;
  execution_id: string;
  cursor: number;
  sequence: number;
}

function runtime() {
  return {
    conversation: [] as ConversationItem[],
    lastAssistantText: "",
    sessionCwd: "",
    renders: 0,
    render() { this.renders += 1; },
  };
}

function stream(executionId: string) {
  return {
    buffer: "",
    stderr: "",
    executionId,
    role: "worker",
    booting: false,
    soundLive: false,
  };
}

function control(
  sequence: number,
  kind: string,
  data: Record<string, unknown>,
  at: string,
): string {
  return `[${sequence}] ${kind} ${JSON.stringify({ ...data, bridgeRecordAt: at })}\n`;
}

function messageEvents(writer: WireEventWriter, suffix: string, body: string): WireEvent[] {
  const modelCallId = wireModelCallId(`model-call:${suffix}`);
  writer.append({
    kind: "model-call.started",
    modelCallId,
    model: { provider: "openai", capabilityClass: "authoring" },
    effort: "high",
    attempt: 1,
  });
  const messageId = wireMessageId(`message:${suffix}`);
  return writer.appendAll([
    { kind: "message.recorded", messageId, modelCallId, stage: "started", role: "assistant" },
    {
      kind: "message.recorded", messageId, modelCallId, stage: "delta",
      role: "assistant", content: body,
    },
    { kind: "message.recorded", messageId, modelCallId, stage: "completed", role: "assistant" },
  ]);
}

test("selected and aggregate execution projections are identical after live and replay arrival", () => {
  let tick = 0;
  const now = () => new Date(Date.UTC(2026, 7, 12, 0, 0, tick++)).toISOString();
  const writerA = new WireEventWriter({ runId: wireRunId("run:projection:a"), now });
  writerA.append({ kind: "run.started", lifecycle: "running", owner: "bridge:test" });
  const firstA = messageEvents(writerA, "projection:a:1", "answer one");
  const followupCursor = writerA.events().length;
  const secondA = messageEvents(writerA, "projection:a:2", "answer two");

  const writerB = new WireEventWriter({ runId: wireRunId("run:projection:b"), now });
  writerB.append({ kind: "run.started", lifecycle: "running", owner: "bridge:test" });
  const firstB = messageEvents(writerB, "projection:b:1", "other answer");

  const acceptedA = control(1, "execution.accepted", {
    prompt: "first request", role: "implementer", wireCursor: 1,
  }, "2026-08-12T00:00:00.500Z");
  const submittedA = control(2, "control.submit_input", {
    input: "second request", delivery: "queued-next-turn", wireCursor: followupCursor,
  }, "2026-08-12T00:00:04.500Z");
  const acceptedB = control(1, "execution.accepted", {
    prompt: "other request", role: "implementer", wireCursor: 1,
  }, "2026-08-12T00:00:09.500Z");
  const firstALines = firstA.map(renderWireEvent).join("\n") + "\n";
  const secondALines = secondA.map(renderWireEvent).join("\n") + "\n";
  const firstBLines = firstB.map(renderWireEvent).join("\n") + "\n";

  const live = runtime();
  parseBridgeStream(live, stream("exec-a"), acceptedA + firstALines + submittedA + secondALines);
  parseBridgeStream(live, stream("exec-b"), acceptedB + firstBLines);

  // Attach replay sends the complete control journal before the Wire journal.
  const replay = runtime();
  parseBridgeStream(replay, stream("exec-a"), acceptedA + submittedA + firstALines + secondALines);
  parseBridgeStream(replay, stream("exec-b"), acceptedB + firstBLines);

  const selectedLive = projectConversation(live.conversation, "exec-a", false) as ConversationItem[];
  const selectedReplay = projectConversation(
    replay.conversation, "exec-a", false,
  ) as ConversationItem[];
  expect(selectedReplay).toEqual(selectedLive);
  expect(selectedReplay.map((item) => item.body)).toEqual([
    "first request", "answer one", "second request", "answer two",
  ]);
  expect(selectedReplay.every((item) => item.execution_id === "exec-a")).toBe(true);

  const aggregateLive = projectConversation(live.conversation, "", true) as ConversationItem[];
  const aggregateReplay = projectConversation(replay.conversation, "", true) as ConversationItem[];
  expect(aggregateReplay).toEqual(aggregateLive);
  expect(new Set(aggregateReplay.map((item) => item.execution_id)))
    .toEqual(new Set(["exec-a", "exec-b"]));
  expect(projectConversation(replay.conversation, "exec-b", false))
    .toEqual(aggregateReplay.filter((item) => item.execution_id === "exec-b")
      .sort((left, right) => left.cursor - right.cursor || left.sequence - right.sequence));
});

test("aggregate transcript view is an explicit command state", () => {
  const target = { transcriptView: "selected", renders: 0, render() { this.renders += 1; } };
  expect(handleLocalCommand(target, {}, "/transcript all")).toBe(true);
  expect(target.transcriptView).toBe("all");
  expect(target.renders).toBe(1);
  expect(handleLocalCommand(target, {}, "/transcript selected")).toBe(true);
  expect(target.transcriptView).toBe("selected");
  expect(() => handleLocalCommand(target, {}, "/transcript merged")).toThrow(
    "transcript requires selected or all",
  );
});
