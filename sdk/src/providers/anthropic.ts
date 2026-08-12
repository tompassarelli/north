import {
	query,
	type Options,
	type Query,
	type SDKUserMessage,
} from "@anthropic-ai/claude-agent-sdk";
import {
	providerPreacceptError,
	type AgentProvider,
	type AgentProviderQuery,
	type ProviderAvailability,
	type RoutingTarget,
} from "./types";
import { probeAnthropic } from "../provider-routing";
import {
	observeAnthropicQuery,
	type AnthropicObservedStream,
} from "./anthropic-observations";
import { providerEnvironmentForTarget } from "../accounts";
import { resolve } from "node:path";
import { requireOrchestrationCapabilities } from "../orchestration-capabilities";
import type { OrchestrationCapability } from "../orchestration-capabilities";
import {
  admitExecution, admitPinnedProvider, consumeExecutionAdmission,
  validateManagedExecutionEnvelope,
} from "../execution-admission";
import {
  READONLY_SHELL_SERVER, READONLY_SHELL_TOOL,
} from "../readonly-shell";
import {
  canonicalHarnessModelAvailability,
  COORDINATION_TOOLS, hasCanonicalAuthoringHooks, hasCanonicalHarnessAuthority, managedToolPolicy,
  NATIVE_AGENT_TOOLS, ORCHESTRATION_TOOLS,
} from "../harness";
import { validateModelAdmissionReceipt } from "../provider-model-observation-store";
import {
	createAnthropicProcessLifecycle, settleAnthropicProcessOwner,
	type AnthropicProcessLifecycle,
} from "./anthropic-process";
import { resolveTier } from "./catalog";
import { AnthropicWireNormalizer } from "./anthropic-wire";
import { createExecutionActivityEmitter } from "../execution-activity";
import { McpActivityAccumulator } from "../tool-activity";
import {
	WireContractError,
	type WireEvent,
	type WireEventListener,
	type WireModelSelection,
	type WireQuery,
	type WireQueryFlagSettings,
	type WireQueryInput,
} from "../wire";

// Selection already proved a CLI-owned first-party Claude.ai session, and the
// target environment strips API-key, cloud, and alternate-endpoint transports.
// Claude Code Agent SDK 0.3.195 reports `none` for that subscription flow even
// though its current ApiKeySource declaration omits the runtime value.
const SUBSCRIPTION_SAFE_API_KEY_SOURCES = new Set(["oauth", "none"]);
export async function disposeAnthropicSdkQuery(
  rawQuery: Pick<Query, "return"> | undefined,
  lifecycle: AnthropicProcessLifecycle | undefined,
  abort: AbortController | undefined,
  graceMs?: number,
): Promise<void> {
  if (!lifecycle || !abort) {
    if (rawQuery) await rawQuery.return(undefined);
    return;
  }
  await settleAnthropicProcessOwner({
    lifecycle,
    abortController: abort,
    dispose: rawQuery ? () => rawQuery.return(undefined) : undefined,
    ...(graceMs === undefined ? {} : { disposalGraceMs: graceMs }),
  });
}

function exactStrings(actual: unknown, expected: readonly string[]): boolean {
  return Array.isArray(actual)
    && actual.length === expected.length
    && actual.every((value, index) => value === expected[index]);
}

type UnknownRecord = Record<string, unknown>;

function record(value: unknown): UnknownRecord | undefined {
	return value && typeof value === "object" && !Array.isArray(value)
		? value as UnknownRecord : undefined;
}

function normalizedAnthropicMessage(message: unknown): unknown {
	const source = record(message);
	if (!source) return message;
	if (source.type === "system" && source.subtype === "init"
			&& !SUBSCRIPTION_SAFE_API_KEY_SOURCES.has(String(source.apiKeySource))) {
		throw new Error("anthropic_subscription_authentication_required");
	}
	if (source.type === "result" && source.subtype !== "success") {
		return { ...source, errors: ["anthropic_provider_execution_failed"] };
	}
	const assistant = record(source.message);
	if (source.type === "assistant" && source.error && assistant) {
		return { ...source, message: { ...assistant, content: [] } };
	}
	if (source.type === "auth_status") {
		return {
			...source,
			output: [],
			...(source.error === undefined ? {} : { error: "anthropic_provider_authentication_failed" }),
		};
	}
	if (source.type === "system" && source.subtype === "mirror_error") {
		return { ...source, error: "anthropic_provider_execution_failed" };
	}
	if (source.type === "system" && source.subtype === "status" && source.compact_error !== undefined) {
		return { ...source, compact_error: "anthropic_provider_execution_failed" };
	}
	return source;
}

function normalizedAnthropicFrames(source: AsyncIterable<unknown>): AsyncIterable<unknown> {
	return {
		async *[Symbol.asyncIterator]() {
			for await (const message of source) yield normalizedAnthropicMessage(message);
		},
	};
}

function validateAnthropicHarness(options: Options): OrchestrationCapability[] | undefined {
	if (!("northCapabilities" in options)) return undefined;
  const managed = options as Options & {
    northCapabilities: unknown;
    northDataOnly?: boolean;
  };
  const capabilities = requireOrchestrationCapabilities(
    managed.northCapabilities, "northCapabilities",
  );
  if (!hasCanonicalHarnessAuthority(options, "anthropic"))
    throw providerPreacceptError("anthropic_harness_authority_seal_missing");
  validateManagedExecutionEnvelope("anthropic", capabilities, options);
  admitPinnedProvider("anthropic", capabilities);
  const policy = managedToolPolicy(capabilities);
  const dataOnly = managed.northDataOnly === true;
  if (!Array.isArray(options.settingSources) || options.settingSources.length !== 0)
    throw providerPreacceptError("anthropic_setting_sources_must_be_isolated");
  if (options.strictMcpConfig !== true)
    throw providerPreacceptError("anthropic_strict_mcp_config_required");
  const denied = new Set(options.disallowedTools ?? []);
  const allowed = new Set(options.allowedTools ?? []);
  const requireDenied = (tools: string[], capability: string) => {
    if (tools.some((toolName) => !denied.has(toolName)))
      throw providerPreacceptError(
        `anthropic_adapter_did_not_enforce_absent_${capability}_capability`,
      );
  };
  const requireAllowed = (tools: string[], capability: string) => {
    if (tools.some((toolName) => !allowed.has(toolName)))
      throw providerPreacceptError(
        `anthropic_adapter_did_not_apply_${capability}_capability`,
      );
  };
  requireDenied(NATIVE_AGENT_TOOLS, "native_agent");
  if (dataOnly) requireDenied(COORDINATION_TOOLS, "north");
  else requireAllowed(COORDINATION_TOOLS, "north");

  const exactCapability = (present: boolean, tools: string[], capability: string) => {
    if (present && !dataOnly) requireAllowed(tools, capability);
    else requireDenied(tools, capability);
  };
  exactCapability(capabilities.includes("filesystem.read"), ["Read"], "filesystem_read");
  exactCapability(capabilities.includes("filesystem.search"), ["Grep", "Glob"], "filesystem_search");
  exactCapability(
    capabilities.includes("filesystem.write"),
    ["Edit", "Write", "NotebookEdit"],
    "filesystem_write",
  );
  exactCapability(capabilities.includes("web"), ["WebSearch", "WebFetch"], "web");

  if (capabilities.includes("shell") && !dataOnly) {
    requireAllowed(["Bash"], "shell");
    requireDenied([READONLY_SHELL_TOOL], "readonly_shell");
  } else if (capabilities.includes("shell.readonly") && !dataOnly) {
    requireDenied(["Bash"], "shell");
    requireAllowed([READONLY_SHELL_TOOL], "readonly_shell");
  } else {
    requireDenied(["Bash", READONLY_SHELL_TOOL], "shell");
  }

  const expectedMcpServers = dataOnly ? [] : [
    "north",
    ...(capabilities.includes("coordination") ? ["north-peer"] : []),
    ...(capabilities.includes("shell.readonly") ? [READONLY_SHELL_SERVER] : []),
  ];
  if (capabilities.includes("coordination") && !dataOnly) {
    requireAllowed(ORCHESTRATION_TOOLS, "coordination");
    const peer = options.mcpServers?.["north-peer"];
    if (peer?.type !== "sdk" || peer.name !== "north-peer")
      throw providerPreacceptError("anthropic_coordination_server_contract_missing");
  } else {
    requireDenied(ORCHESTRATION_TOOLS, "coordination");
  }

  const permissionMode = capabilities.includes("filesystem.write") ? "acceptEdits" : "default";
  if (options.permissionMode !== permissionMode)
    throw providerPreacceptError("anthropic_permission_mode_contract_missing");
  if (capabilities.includes("shell.readonly") && !dataOnly) {
    const readonly = options.mcpServers?.[READONLY_SHELL_SERVER];
    if (readonly?.type !== "sdk" || readonly.name !== READONLY_SHELL_SERVER) {
      throw providerPreacceptError("anthropic_readonly_shell_contract_missing");
    }
  }
  const actualMcpServers = Object.keys(options.mcpServers ?? {});
  if (!exactStrings(actualMcpServers, expectedMcpServers))
    throw providerPreacceptError("anthropic_mcp_server_surface_contract_missing");
  if (!exactStrings(options.tools, dataOnly ? [] : policy.tools))
    throw providerPreacceptError("anthropic_builtin_tool_surface_contract_missing");
  if (!exactStrings(options.allowedTools, dataOnly ? [] : policy.allowedTools))
    throw providerPreacceptError("anthropic_auto_approval_contract_missing");
  const expectedDenied = dataOnly
    ? [...new Set([...policy.allowedTools, ...policy.disallowedTools])]
    : policy.disallowedTools;
  if (!exactStrings(options.disallowedTools, expectedDenied))
    throw providerPreacceptError("anthropic_denied_tool_contract_missing");
  if (!hasCanonicalAuthoringHooks(options))
    throw providerPreacceptError("anthropic_authoring_guard_contract_missing");
  return capabilities;
}

export async function admitAnthropic(options: Options, target?: RoutingTarget): Promise<void> {
  const capabilities = validateAnthropicHarness(options);
  if (!capabilities) return;
  const modelAvailability = canonicalHarnessModelAvailability(options, "anthropic");
  if (!modelAvailability)
    throw providerPreacceptError("anthropic_model_availability_authority_missing");
  if (modelAvailability.required) {
    if (!target || modelAvailability.targetId !== target.id
        || modelAvailability.model !== options.model
        || typeof options.model !== "string"
        || !await validateModelAdmissionReceipt(
          modelAvailability.receipt,
          target,
          options.model,
          modelAvailability.observationPath,
        )) {
      throw providerPreacceptError("anthropic_model_availability_unproven");
    }
  }
  await admitExecution("anthropic", capabilities, resolve(options.cwd ?? process.cwd()), options, target);
}

export interface AnthropicQueryRuntime {
	query: typeof query;
	observe: typeof observeAnthropicQuery;
	createLifecycle: typeof createAnthropicProcessLifecycle;
	admit?: typeof admitAnthropic;
}

interface AnthropicTurn {
	rawQuery: Query;
	observed: AnthropicObservedStream;
	lifecycle: AnthropicProcessLifecycle;
	abort: AbortController;
	detachCallerAbort?: () => void;
	sessionIds: Set<string>;
	sessionIdentityLost: boolean;
	consumed: boolean;
	cleanup?: Promise<void>;
}

function anthropicInput(input: WireQueryInput): string | AsyncIterable<SDKUserMessage> {
	if (typeof input === "string") return input;
	return {
		async *[Symbol.asyncIterator](): AsyncIterator<SDKUserMessage> {
			for await (const frame of input) {
				if (frame.kind !== "user.input" || typeof frame.text !== "string") {
					throw new TypeError("anthropic wire input frame is malformed");
				}
				yield {
					type: "user",
					message: { role: "user", content: frame.text },
					parent_tool_use_id: null,
				};
			}
		},
	};
}

function providerFailure(): Error {
	return new Error("anthropic_provider_execution_failed");
}

type AnthropicSessionContinuationOptions = Options & {
	continueConversation?: unknown;
};

function anthropicContinuationExtraArg(key: string): boolean {
	const flag = key.split("=", 1)[0]?.replace(/^--?/, "") ?? key;
	const normalized = flag.replace(/[-_]/g, "").toLowerCase();
	return normalized === "continue"
		|| normalized === "continueconversation"
		|| normalized === "resume"
		|| normalized === "forksession"
		|| normalized === "resumesessionat"
		|| normalized === "sessionid";
}

function withoutCallerSessionContinuation(options: Options): Options {
	const {
		resume: _resume,
		continue: _continue,
		continueConversation: _continueConversation,
		forkSession: _forkSession,
		resumeSessionAt: _resumeSessionAt,
		sessionId: _sessionId,
		extraArgs,
		...neutral
	} = options as AnthropicSessionContinuationOptions;
	if (extraArgs === undefined) return neutral;
	const neutralExtraArgs = Object.fromEntries(
		Object.entries(extraArgs).filter(([key]) => !anthropicContinuationExtraArg(key)),
	);
	return Object.keys(neutralExtraArgs).length === 0
		? neutral
		: { ...neutral, extraArgs: neutralExtraArgs };
}

export function createAnthropicQuery(
	args: AgentProviderQuery,
	admitted: boolean,
	runtime: AnthropicQueryRuntime = {
    query,
    observe: observeAnthropicQuery,
    createLifecycle: createAnthropicProcessLifecycle,
    admit: admitAnthropic,
	},
): WireQuery {
	const normalizer = new AnthropicWireNormalizer(
		args.context.writer,
		args.context.route,
		args.context.artifacts,
	);
	const activity = createExecutionActivityEmitter();
	const mcp = new McpActivityAccumulator("anthropic-agent-sdk:assistant-tool-use");
	const listeners = new Set<WireEventListener>();
	const callerSignal = args.options.abortController?.signal;
	let pendingInput: WireQueryInput | undefined = args.input;
	let pendingResume: string | undefined;
	let continuationSessionId: string | undefined;
	let activeTurn: AnthropicTurn | undefined;
	let initialization: Promise<AnthropicTurn> | undefined;
	let latestCleanup: Promise<void> | undefined;
	let closePromise: Promise<void> | undefined;
	let closed = false;
	let iterating = false;
	let turnCompleted = false;

	const ensureOpen = (): void => {
		if (closed || callerSignal?.aborted) throw new Error("anthropic_query_closed");
	};
	const publish = (event: WireEvent): void => {
		for (const listener of listeners) {
			try { listener(event); }
			catch { /* Observers cannot change provider execution. */ }
		}
	};
	const cleanup = (turn: AnthropicTurn): Promise<void> => {
		latestCleanup = turn.cleanup ??= (async () => {
			try {
				await disposeAnthropicSdkQuery(turn.rawQuery, turn.lifecycle, turn.abort);
			} finally {
				turn.detachCallerAbort?.();
			}
		})();
		return latestCleanup;
	};
	const initialize = async (): Promise<AnthropicTurn> => {
		ensureOpen();
		if (activeTurn) return activeTurn;
		if (pendingInput === undefined) throw new Error("anthropic_turn_input_unavailable");
		initialization ??= (async () => {
			if (admitted) validateAnthropicHarness(args.options);
			else await (runtime.admit ?? admitAnthropic)(args.options, args.target);
			ensureOpen();
			admitted = true;
			const input = pendingInput;
			if (input === undefined) throw new Error("anthropic_turn_input_unavailable");
			const resume = pendingResume;
			const abort = new AbortController();
			let detachCallerAbort: (() => void) | undefined;
			if (callerSignal) {
				const forward = () => abort.abort(callerSignal.reason);
				if (callerSignal.aborted) forward();
				else {
					callerSignal.addEventListener("abort", forward, { once: true });
					detachCallerAbort = () => callerSignal.removeEventListener("abort", forward);
				}
			}
			let lifecycle: AnthropicProcessLifecycle;
			try {
				lifecycle = runtime.createLifecycle();
			} catch {
				detachCallerAbort?.();
				throw providerFailure();
			}
			let rawQuery: Query | undefined;
			try {
				const options = {
					...withoutCallerSessionContinuation(args.options),
					abortController: abort,
					spawnClaudeCodeProcess: lifecycle.spawnClaudeCodeProcess,
					env: providerEnvironmentForTarget("anthropic", args.target, { env: args.options.env }),
					...(resume === undefined ? {} : { resume }),
				};
				rawQuery = runtime.query({ prompt: anthropicInput(input), options });
				const sessionIds = new Set<string>();
				let sessionIdentityLost = false;
				const observed = runtime.observe(normalizedAnthropicFrames(rawQuery), {
					targetId: () => args.target?.id ?? "anthropic",
					mcpAccumulator: mcp,
					onSessionId: (sessionId) => {
						if (sessionIds.has(sessionId)) return;
						if (sessionIds.size >= 2) sessionIdentityLost = true;
						else sessionIds.add(sessionId);
					},
				});
				const turn: AnthropicTurn = {
					rawQuery,
					observed,
					lifecycle,
					abort,
					...(detachCallerAbort === undefined ? {} : { detachCallerAbort }),
					sessionIds,
					get sessionIdentityLost() { return sessionIdentityLost; },
					consumed: false,
				};
				pendingInput = undefined;
				pendingResume = undefined;
				activeTurn = turn;
				return turn;
			} catch {
				try { await disposeAnthropicSdkQuery(rawQuery, lifecycle, abort); }
				catch { /* Provider construction failure remains redacted. */ }
				detachCallerAbort?.();
				throw providerFailure();
			}
		})();
		return initialization;
	};

	const wireQuery: WireQuery = {
		executionTransport: "sdk-stream",
		executionActivity: activity.source,
		subscribeProviderEvents(listener: WireEventListener): () => void {
			listeners.add(listener);
			return () => listeners.delete(listener);
		},
		mcpActivity: () => mcp.snapshot(),
		async continueTurn(input: WireQueryInput): Promise<void> {
			ensureOpen();
			if (!turnCompleted || !continuationSessionId || pendingInput !== undefined) {
				throw new Error("anthropic_continuation_unavailable");
			}
			normalizer.beginNextTurn();
			pendingInput = input;
			pendingResume = continuationSessionId;
			continuationSessionId = undefined;
			turnCompleted = false;
		},
		interruptTurn: async () => {
			try { await (await initialize()).rawQuery.interrupt(); }
			catch { throw providerFailure(); }
		},
		interrupt: async () => {
			try { await (await initialize()).rawQuery.interrupt(); }
			catch { throw providerFailure(); }
		},
		close: () => closePromise ??= (async () => {
			closed = true;
			if (!initialization && !activeTurn && !latestCleanup) return;
			try {
				if (activeTurn) await cleanup(activeTurn);
				else if (initialization) await cleanup(await initialization);
				else await latestCleanup;
			} catch (error) {
				if (!(error instanceof Error && error.message === "anthropic_query_closed")) {
					throw providerFailure();
				}
			}
		})(),
		forceClose: () => {
			closed = true;
			activeTurn?.lifecycle.forceKill();
		},
		setModel: async (selection: WireModelSelection) => {
			if (selection.provider !== "anthropic") throw providerFailure();
			const tier = selection.tier ?? args.context.route.model.tier;
			if (!tier) throw providerFailure();
			try {
				const model = resolveTier("anthropic", tier).model;
				if (!model) throw providerFailure();
				await (await initialize()).rawQuery.setModel(model);
				normalizer.setModel(selection);
			} catch {
				throw providerFailure();
			}
		},
		applyFlagSettings: async (settings: WireQueryFlagSettings) => {
			try {
				await (await initialize()).rawQuery.applyFlagSettings({
					...(settings.effortLevel === undefined
						? {} : { effortLevel: settings.effortLevel }),
				});
				normalizer.setEffort(settings.effortLevel ?? undefined);
			} catch {
				throw providerFailure();
			}
		},
		supportsInFlightEscalation: () => true,
		async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
			if (closed) return;
			if (iterating) throw new Error("anthropic_turn_already_consumed");
			iterating = true;
			try {
				while (!closed) {
					let turn: AnthropicTurn;
					try {
						turn = await initialize();
					} catch (error) {
						if (closed || callerSignal?.aborted) return;
						throw error;
					}
					if (turn.consumed) throw new Error("anthropic_turn_already_consumed");
					let terminalSeen = false;
					let failure: unknown;
					try {
						for await (const observed of turn.observed) {
							const accepted = normalizer.accept(observed.frame, {
								...(observed.providerJoin === undefined
									? {} : { providerJoin: observed.providerJoin }),
							});
							activity.record("provider", "provider.anthropic.frame.accepted");
							for (const event of accepted.events) {
								if (event.kind === "model-call.completed" && accepted.turnOutcome) {
									terminalSeen = true;
									turnCompleted = true;
									continuationSessionId = !turn.sessionIdentityLost
										&& turn.sessionIds.size === 1
										? turn.sessionIds.values().next().value
										: undefined;
								}
								publish(event);
								yield event;
							}
						}
						if (!terminalSeen) failure = providerFailure();
					} catch (error) {
						failure = error;
					}
					try {
						await cleanup(turn);
					} catch {
						failure ??= providerFailure();
					}
					turn.consumed = true;
					activeTurn = undefined;
					initialization = undefined;
					if (failure !== undefined) {
						pendingInput = undefined;
						pendingResume = undefined;
						continuationSessionId = undefined;
						const cancelled = closed || callerSignal?.aborted;
						const settled = normalizer.settleAbrupt(cancelled ? "cancelled" : "failed");
						for (const event of settled.events) {
							publish(event);
							yield event;
						}
						if (cancelled) return;
						if (failure instanceof WireContractError) throw failure;
						throw providerFailure();
					}
					if (pendingInput === undefined) return;
					turnCompleted = false;
				}
			} finally {
				iterating = false;
			}
		},
	};
	return wireQuery;
}

const canonicalAnthropicProvider: AgentProvider = {
  id: "anthropic",
  liveInput: "streaming",
  probe(target): ProviderAvailability {
    return probeAnthropic(target);
  },
  admit: ({ options, target }) => admitAnthropic(options, target),
  query(args) {
    const admitted = consumeExecutionAdmission("anthropic", args.options);
    // Direct adapter callers are admitted lazily before SDK query construction;
    // routedQuery carries a one-use receipt from the same full preflight.
    return createAnthropicQuery(args, admitted);
  },
};

export const anthropicProvider: AgentProvider = Object.freeze(
  canonicalAnthropicProvider,
);
