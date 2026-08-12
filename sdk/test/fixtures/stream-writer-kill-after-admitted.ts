import { SerializedWireEventCommitter, StreamWriter } from "../../src/stream-writer";
import {
	WireEventWriter,
	wireEventId,
	wireModelCallId,
	wireRunId,
	wireToolCallId,
} from "../../src/wire";

const agentId = process.argv[2];
if (!agentId) throw new Error("fixture requires an agent ID");

const writer = new WireEventWriter({
	runId: wireRunId("run:stream:crash-prefix"),
	eventId: (sequence) => wireEventId(`event:stream:crash-prefix:${sequence}`),
	now: () => "2026-08-12T00:00:00.000Z",
});
const stream = await StreamWriter.open(agentId);
const committer = new SerializedWireEventCommitter(writer, stream);
const started = writer.append({ kind: "run.started", lifecycle: "running" });
const modelCallId = wireModelCallId("model-call:stream:crash-prefix");
writer.append({
	kind: "model-call.started",
	modelCallId,
	model: { provider: "openai", capabilityClass: "authoring" },
	attempt: 1,
});
const admitted = writer.append({
	kind: "tool.admitted",
	toolCallId: wireToolCallId("tool:stream:crash-prefix"),
	modelCallId,
	name: "command",
	schema: { status: "unavailable", reason: "provider schema unavailable" },
});

await Promise.all([committer.commitThrough(admitted), committer.commitThrough(started)]);
process.stdout.write("ready\n");
for (;;) await Bun.sleep(60_000);
