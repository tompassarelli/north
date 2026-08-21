import { expect, test } from "bun:test";
import { renderWireEvent } from "../src/bridge/cli";
import {
  "parse-bridge-stream!" as parseBridgeStream,
} from "../src/bridge/generated/north/bridge/app.js";
import {
  "make-model" as makeModel,
  "snapshot" as snapshot,
} from "../src/bridge/generated/north/bridge/model.js";
import {
  WireEventWriter,
  wireMessageId,
  wireModelCallId,
  wireRunId,
} from "../src/wire";

interface ConversationItem {
  _tag: "ConversationItem";
  id: string;
  kind: string;
  title: string;
  body: string;
  status: string;
  data: unknown;
  execution_id: string;
  at: string;
  cursor: number;
  sequence: number;
}

test("canonical wire message stages rebuild one assistant item in the Bridge transcript", () => {
  const writer = new WireEventWriter({ runId: wireRunId("run:bridge-wire-ui") });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:test" });
  const modelCallId = wireModelCallId("model-call:bridge-wire-ui");
  writer.append({
    kind: "model-call.started",
    modelCallId,
    model: { provider: "openai", capabilityClass: "authoring" },
    effort: "high",
    attempt: 1,
  });
  const messageId = wireMessageId("message:bridge-wire-ui");
  const events = writer.appendAll([
    {
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "started",
      role: "assistant",
    },
    {
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "delta",
      role: "assistant",
      content: "hello ",
    },
    {
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "delta",
      role: "assistant",
      content: "wire",
    },
    {
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "completed",
      role: "assistant",
    },
  ]);
  const runtime = {
    conversation: [] as ConversationItem[],
    lastAssistantText: "",
    renders: 0,
    render() { this.renders += 1; },
  };
  const streamState = {
    buffer: "",
    stderr: "",
    executionId: "execution-wire-ui",
    role: "worker",
    booting: false,
    soundLive: false,
  };
  const rendered = events.map(renderWireEvent).join("\n") + "\n";

  parseBridgeStream(runtime, streamState, rendered.slice(0, 19));
  expect(runtime.conversation).toEqual([]);
  parseBridgeStream(runtime, streamState, rendered.slice(19));

  expect(runtime.conversation).toEqual([{
    _tag: "ConversationItem",
    id: `execution-wire-ui:${messageId}`,
    kind: "assistant",
    title: "",
    body: "hello wire",
    status: "done",
    data: null,
    execution_id: "execution-wire-ui",
    at: events[0]!.at,
    cursor: events[0]!.sequence + 1,
    sequence: 0,
  }]);
  expect(runtime.lastAssistantText).toBe("hello wire");
  expect(runtime.renders).toBe(4);
  expect(streamState.buffer).toBe("");
});

test("session config keeps the concrete model ahead of provider-neutral Wire metadata", () => {
  const runtime = {
    conversation: [] as ConversationItem[],
    sessionModel: "",
    sessionEffort: "",
    sessionCwd: "",
    sessionPermissions: "",
    model: makeModel("list"),
    bridgeExecutions: new Set<string>(),
    supervisorId: "",
    agentIndex: 0,
    working: false,
    workingLabel: "",
    workingSince: 0,
    spinnerTimer: null,
    spinnerIndex: 0,
    disposed: false,
    render() {},
    renderConversation() {},
  };
  const streamState = {
    buffer: "", executionId: "execution-config", role: "supervisor",
    booting: true, soundLive: false,
  };
  parseBridgeStream(
    runtime,
    streamState,
    '[1] session.config {"model":"gpt-5.6-sol","effort":"max",'
      + '"cwd":"/home/tom/code/north/main","permissionMode":"bypassPermissions"}\n',
  );
  expect(runtime.sessionModel).toBe("gpt-5.6-sol");
  expect(runtime.sessionEffort).toBe("max");
  expect(runtime.sessionCwd).toBe("/home/tom/code/north/main");
  expect(runtime.sessionPermissions).toBe("bypassPermissions");

  const writer = new WireEventWriter({ runId: wireRunId("run:bridge-config") });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:test" });
  const started = writer.append({
    kind: "model-call.started",
    modelCallId: wireModelCallId("model-call:bridge-config"),
    model: { provider: "openai", capabilityClass: "orchestrator" },
    effort: "max",
    attempt: 1,
  });
  parseBridgeStream(runtime, streamState, `${renderWireEvent(started)}\n`);
  expect(runtime.sessionModel).toBe("gpt-5.6-sol");
});

test("provider-session replacement stays working across its stream chunk boundary", () => {
  const writer = new WireEventWriter({
    runId: wireRunId("run:bridge-wire-ui-session-replacement"),
  });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:test" });
  const deadModelCallId = wireModelCallId(
    "model-call:bridge-wire-ui-session-replacement:dead",
  );
  const deadStarted = writer.append({
    kind: "model-call.started",
    modelCallId: deadModelCallId,
    model: { provider: "openai", capabilityClass: "authoring" },
    effort: "high",
    attempt: 1,
  });
  const replacementSettlement = writer.append({
    kind: "model-call.completed",
    modelCallId: deadModelCallId,
    status: "failed",
    origin: "north",
    usage: writer.snapshot()!.usage,
    usageCoverage: "unavailable",
    errorCode: "provider_session_replaced",
  });
  const replacementModelCallId = wireModelCallId(
    "model-call:bridge-wire-ui-session-replacement:replacement",
  );
  const replacementStarted = writer.append({
    kind: "model-call.started",
    modelCallId: replacementModelCallId,
    model: { provider: "openai", capabilityClass: "authoring" },
    effort: "high",
    attempt: 2,
  });
  const replacementCompleted = writer.append({
    kind: "model-call.completed",
    modelCallId: replacementModelCallId,
    status: "succeeded",
    origin: "provider",
    usage: writer.snapshot()!.usage,
    usageCoverage: "exact",
  });
  const runtime = {
    model: makeModel("list"),
    bridgeExecutions: new Set<string>(),
    supervisorId: "",
    agentIndex: 0,
    sessionModel: "",
    sessionEffort: "",
    working: false,
    workingLabel: "",
    workingSince: 0,
    spinnerTimer: null,
    spinnerIndex: 0,
    disposed: false,
    soundEnabled: false,
    renderCount: 0,
    conversationRenderCount: 0,
    render() { this.renderCount += 1; },
    renderConversation() { this.conversationRenderCount += 1; },
  };
  const streamState = {
    buffer: "",
    executionId: "execution-wire-ui-session-replacement",
    role: "supervisor",
    booting: false,
    soundLive: false,
  };

  parseBridgeStream(runtime, streamState, `${renderWireEvent(deadStarted)}\n`);
  expect(runtime.working).toBe(true);
  expect(snapshot(runtime.model).agents[0]?.status).toBe("working");
  const rendersBeforeSettlement = runtime.conversationRenderCount;

  parseBridgeStream(runtime, streamState, `${renderWireEvent(replacementSettlement)}\n`);
  expect(streamState.buffer).toBe("");
  expect(runtime.working).toBe(true);
  expect(runtime.conversationRenderCount).toBe(rendersBeforeSettlement);
  expect(snapshot(runtime.model).agents[0]?.status).toBe("working");

  parseBridgeStream(runtime, streamState, `${renderWireEvent(replacementStarted)}\n`);
  expect(runtime.working).toBe(true);
  parseBridgeStream(runtime, streamState, `${renderWireEvent(replacementCompleted)}\n`);
  expect(runtime.working).toBe(false);
  expect(snapshot(runtime.model).agents[0]?.status).toBe("working");

  parseBridgeStream(
    runtime,
    streamState,
    "[1] session.idle {\"disposition\":\"completed\",\"pendingInputs\":0}\n",
  );
  expect(runtime.working).toBe(false);
  expect(snapshot(runtime.model).agents[0]?.status).toBe("ready");
});
