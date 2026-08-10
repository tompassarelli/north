import type { WireEvent } from "../wire/events";

function assistantMessageIsActivity(
	event: Extract<WireEvent, { essential: true; kind: "message.recorded" }>,
): boolean {
	if (event.role !== "assistant") return false;
	if (event.stage === "completed") return true;
	if (event.stage !== "delta" || event.content === undefined) return false;
	return typeof event.content !== "string" || event.content.length > 0;
}

/** Classify canonical execution liveness without consulting provider frames. */
export function outerExecutionActivityKind(event: WireEvent): string | undefined {
	if (!event.essential) return undefined;
	switch (event.kind) {
		case "message.recorded":
			return assistantMessageIsActivity(event)
				? `wire.message.assistant.${event.stage}` : undefined;
		case "model-call.completed":
			return `wire.model-call.${event.status}`;
		case "tool.admitted":
			return "wire.tool.admitted";
		case "tool.progress":
			return event.progress !== undefined || event.outputArtifactId !== undefined
				? "wire.tool.progress" : undefined;
		case "tool.terminal":
			return `wire.tool.${event.status}`;
		case "artifact.published":
			return "wire.artifact.published";
		case "run.progress":
			return event.progress.compactions === undefined ? undefined : "wire.run.compacted";
		case "run.started":
		case "resource.pressure":
		case "run.terminated":
			return undefined;
	}
}

export function isOuterExecutionActivity(event: WireEvent): boolean {
	return outerExecutionActivityKind(event) !== undefined;
}
