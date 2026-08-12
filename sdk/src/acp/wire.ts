import type { SessionUpdate, ToolKind } from "@agentclientprotocol/sdk";
import type { WireEvent } from "../wire/events";

function toolKind(name: string): ToolKind {
  const normalized = name.toLowerCase();
  if (/read|view|inspect|cat/.test(normalized)) return "read";
  if (/edit|write|patch|replace/.test(normalized)) return "edit";
  if (/delete|remove|unlink/.test(normalized)) return "delete";
  if (/move|rename/.test(normalized)) return "move";
  if (/search|find|grep|glob/.test(normalized)) return "search";
  if (/fetch|http|web|download/.test(normalized)) return "fetch";
  if (/think|reason/.test(normalized)) return "think";
  if (/exec|command|shell|terminal|bash/.test(normalized)) return "execute";
  return "other";
}

function text(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
}

/** A stateless projection: replaying the same Wire prefix yields the same ACP updates. */
export function projectWireEvent(event: WireEvent): readonly SessionUpdate[] {
  if (event.essential !== true) return [];
  if (event.kind === "message.recorded") {
    if (event.role !== "assistant" || event.stage !== "delta" || event.content === undefined) {
      return [];
    }
    return [{
      sessionUpdate: "agent_message_chunk",
      messageId: event.messageId,
      content: { type: "text", text: text(event.content) },
    }];
  }
  if (event.kind === "tool.admitted") {
    return [{
      sessionUpdate: "tool_call",
      toolCallId: event.toolCallId,
      title: event.name,
      kind: toolKind(event.name),
      status: "pending",
      ...(event.argumentPreview === undefined
        ? {}
        : { rawInput: event.argumentPreview }),
    }];
  }
  if (event.kind === "tool.progress") {
    return [{
      sessionUpdate: "tool_call_update",
      toolCallId: event.toolCallId,
      status: "in_progress",
      rawOutput: {
        ...(event.progress === undefined ? {} : { progress: event.progress }),
        ...(event.outputArtifactId === undefined
          ? {}
          : { outputArtifactId: event.outputArtifactId }),
      },
    }];
  }
  if (event.kind === "tool.terminal") {
    return [{
      sessionUpdate: "tool_call_update",
      toolCallId: event.toolCallId,
      status: event.status === "succeeded" ? "completed" : "failed",
      ...(event.resultPreview === undefined ? {} : {
        content: [{
          type: "content",
          content: { type: "text", text: event.resultPreview },
        }],
      }),
      rawOutput: {
        status: event.status,
        ...(event.resultPreview === undefined ? {} : { preview: event.resultPreview }),
        ...(event.resultArtifactId === undefined
          ? {}
          : { resultArtifactId: event.resultArtifactId }),
        ...(event.errorCode === undefined ? {} : { errorCode: event.errorCode }),
      },
    }];
  }
  return [];
}
