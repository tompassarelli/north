import { createHash, randomBytes } from "node:crypto";
import { resolve } from "node:path";
import {
	beagleStoreBabashkaArguments,
	beagleStoreEnvironment,
	settleBeagleStoreCoordinatorChild,
} from "./beagle-store";
import {
	shadowReviewerAgentId,
	type ShadowReviewerNote,
} from "./shadow-reviewer";
import { trustedNorthBabashkaExecutable } from "./trusted-runtime";

const REPO = resolve(import.meta.dir, "../..");
const MSG_CLI = resolve(REPO, "cli/msg-cli.clj");
const PUBLISH_TIMEOUT_MS = 30_000;
export const SHADOW_REVIEWER_NOTE_SUBJECT = "msg" as const;

export interface ShadowReviewerNoteCommand {
	readonly executable: string;
	readonly args: readonly string[];
	readonly env: NodeJS.ProcessEnv;
	readonly stdin: string;
}

export interface ShadowReviewerNoteCapability {
	readonly preimage: string;
	readonly sha256: string;
}

export interface ShadowReviewerNotePublisherRuntime {
	readonly publish?: (
		command: ShadowReviewerNoteCommand,
		signal: AbortSignal,
	) => Promise<"recorded" | "unavailable">;
	readonly env?: NodeJS.ProcessEnv;
}

function noteBody(note: ShadowReviewerNote): string {
	return `[${note.severity}] ${note.note}`;
}

export function mintShadowReviewerNoteCapability(): ShadowReviewerNoteCapability {
	const preimage = randomBytes(32).toString("hex");
	return Object.freeze({
		preimage,
		sha256: createHash("sha256").update(preimage).digest("hex"),
	});
}

export function shadowReviewerNoteCommand(
	sourceAgentId: string,
	note: ShadowReviewerNote,
	capability: ShadowReviewerNoteCapability,
	env: NodeJS.ProcessEnv = process.env,
): ShadowReviewerNoteCommand {
	return Object.freeze({
		executable: trustedNorthBabashkaExecutable(),
		args: Object.freeze(beagleStoreBabashkaArguments([
			MSG_CLI,
			env.NORTH_PORT ?? "7977",
			"review",
			shadowReviewerAgentId(sourceAgentId),
			sourceAgentId,
			noteBody(note),
		], env)),
		env: beagleStoreEnvironment({
			...env,
			AGENT_ID: sourceAgentId,
			AGENT_TOPOLOGY: "worker",
		}),
		stdin: capability.preimage,
	});
}

async function publishCommand(
	command: ShadowReviewerNoteCommand,
	signal: AbortSignal,
): Promise<"recorded" | "unavailable"> {
	if (signal.aborted) return "unavailable";
	const child = Bun.spawn([command.executable, ...command.args], {
		env: command.env,
		stdin: "pipe",
		stdout: "ignore",
		stderr: "ignore",
	});
	child.stdin.write(command.stdin);
	child.stdin.end();
	const outcome = await settleBeagleStoreCoordinatorChild(
		child,
		PUBLISH_TIMEOUT_MS,
		{ signal },
	);
	return !signal.aborted && !outcome.timedOut && outcome.exitCode === 0
		? "recorded" : "unavailable";
}

/**
 * Publish a validated reviewer note through the durable managed-message path.
 * The existing live input feed owns delivery and terminal-boundary replay; this publisher has no
 * interrupt, resume, or provider control capability.
 */
export async function publishShadowReviewerNote(
	sourceAgentId: string,
	note: ShadowReviewerNote,
	capability: ShadowReviewerNoteCapability,
	signal: AbortSignal,
	runtime: ShadowReviewerNotePublisherRuntime = {},
): Promise<void> {
	if (signal.aborted) throw signal.reason;
	const command = shadowReviewerNoteCommand(sourceAgentId, note, capability, runtime.env);
	const status = await (runtime.publish ?? publishCommand)(command, signal);
	if (signal.aborted) throw signal.reason;
	if (status !== "recorded") {
		throw new Error("shadow reviewer note publication was unavailable");
	}
}
