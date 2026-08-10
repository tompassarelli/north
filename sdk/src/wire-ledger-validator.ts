import { decodeWireJsonl } from "./wire/jsonl";

try {
	const replay = decodeWireJsonl(await Bun.stdin.text());
	const first = replay.events[0];
	const last = replay.events.at(-1);
	if (first?.kind !== "run.started" || last?.kind !== "run.terminated"
		|| replay.snapshot === undefined
		|| !["completed", "failed", "cancelled", "blocked"].includes(replay.snapshot.lifecycle)) {
		throw new TypeError("wire ledger validator requires one complete terminal run");
	}
} catch {
	process.exitCode = 1;
}
