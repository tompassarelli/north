import emptyResultRepairInput from "./empty-result-repair.md" with { type: "text" };
import type { WireQuery } from "./wire/query";

export type EmptyResultRepairMode = "streaming" | "turn-framed";

export function emptyResultRepairMode(query: WireQuery): EmptyResultRepairMode | undefined {
	if (query.executionTransport === "sdk-stream" && query.continueTurn) return "streaming";
	if (query.executionTransport === "managed-app-server") return "turn-framed";
	return undefined;
}

export function successfulEmptyResultRepairInput(): string {
	return emptyResultRepairInput.trim();
}
