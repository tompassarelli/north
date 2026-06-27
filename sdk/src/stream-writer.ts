import { appendFileSync, writeFileSync } from "fs";
import { join } from "path";

const STREAM_DIR =
  process.env.LODESTAR_STREAM_DIR ??
  join(process.env.HOME ?? "", "code/agent-data");

// Write SDK messages to .stream.jsonl in the same format the lodestar web tails.
// This bridges SDK dispatch into the existing lodestar web without changing the bridge.
export class StreamWriter {
  private path: string;

  constructor(agentId: string) {
    this.path = join(STREAM_DIR, `agent-${agentId}.stream.jsonl`);
    writeFileSync(this.path, "");
  }

  write(event: any) {
    appendFileSync(this.path, JSON.stringify(event) + "\n");
  }

  // Normalize an SDK message into the stream format the lodestar web expects.
  writeSDKMessage(message: any) {
    if (message.type === "assistant" && message.message?.content) {
      this.write({
        type: "assistant",
        content: message.message.content,
        ...(message.parent_tool_use_id
          ? { parent_tool_use_id: message.parent_tool_use_id }
          : {}),
      });
    } else if (message.type === "result") {
      this.write({ type: "result", result: message.result ?? "" });
    } else if (message.type === "system") {
      this.write({
        type: "system",
        subtype: message.subtype ?? "",
        ...(message.data ? { data: message.data } : {}),
      });
    }
  }
}
