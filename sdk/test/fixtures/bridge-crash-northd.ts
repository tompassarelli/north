import { Northd } from "../../src/bridge/host";
import type { BridgeProviderExecution } from "../../src/bridge/provider";
import { wireToolCallId } from "../../src/wire";
import { BridgeWireTestSession } from "../support/bridge-wire-session";

const socketPath = process.argv[2];
const journalRoot = process.argv[3];
const root = process.argv[4];
if (!socketPath || !journalRoot || !root) {
  throw new Error("bridge crash fixture requires socket, journal, and root paths");
}

const provider: BridgeProviderExecution = {
  async open(context) {
    const session = new BridgeWireTestSession(context, { initialAssistant: "before SIGKILL" });
    const modelCall = Object.values(context.writer.snapshot()?.modelCalls ?? {})
      .find((candidate) => candidate.status === "running");
    if (!modelCall) throw new Error("bridge crash fixture has no active model call");
    session.publish({
      kind: "tool.admitted",
      toolCallId: wireToolCallId(`tool-call:bridge-crash:${context.executionId}`),
      modelCallId: modelCall.id,
      name: "crashed-background-tool",
      schema: { status: "unavailable", reason: "crash-recovery fixture" },
    });
    return session;
  },
};
const northd = new Northd({ socketPath, journalRoot, root, provider });
await northd.listen();
process.stdout.write("ready\n");
await Promise.withResolvers<void>().promise;
