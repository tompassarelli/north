import { expect, test } from "bun:test";
import {
  isOuterExecutionActivity, outerExecutionActivityKind,
} from "../src/providers/outer-activity";

test("normalized Anthropic/OpenAI execution frames are activity", () => {
  for (const [message, kind] of [
    [
      {
        type: "assistant",
        message: { content: [{ type: "text", text: "working" }] },
      },
      "outer.assistant.text",
    ],
    [
      {
        type: "user",
        message: { content: [{ type: "tool_result", tool_use_id: "tool-1" }] },
      },
      "outer.user.tool_result",
    ],
    [
      { type: "result", subtype: "success", result: "done" },
      "outer.result.success",
    ],
    [
      {
        type: "tool_progress", tool_use_id: "tool-1", tool_name: "Bash",
        parent_tool_use_id: null, elapsed_time_seconds: 2,
      },
      "outer.tool_progress",
    ],
  ] as const) {
    expect(outerExecutionActivityKind(message)).toBe(kind);
    expect(isOuterExecutionActivity(message)).toBe(true);
  }
});

test("status, lease, retry, auth, startup, hook, and bookkeeping cannot manufacture activity", () => {
  for (const message of [
    { type: "system", subtype: "status", status: "busy" },
    { type: "system", subtype: "init", session_id: "session-1" },
    { type: "system", subtype: "rate_limit", retry_after_ms: 5_000 },
    { type: "auth_status", isAuthenticating: true },
    { type: "system", subtype: "lease_renewed", lease_id: "lease-1" },
    { type: "system", subtype: "retry", attempt: 2 },
    { type: "system", subtype: "mcp_server_startup", server: "north" },
    { type: "system", subtype: "hook_started", hook_id: "hook-1" },
    { type: "system", subtype: "hook_response", hook_id: "hook-1" },
    { type: "system", subtype: "task_started", task_id: "background-1" },
    { type: "system", subtype: "task_notification", task_id: "background-1" },
    { type: "assistant", error: "provider_error", message: { content: [] } },
  ]) {
    expect(outerExecutionActivityKind(message)).toBeUndefined();
    expect(isOuterExecutionActivity(message)).toBe(false);
  }
});
