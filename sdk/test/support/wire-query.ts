import { providerLiveInput, type RoutedQueryArguments } from "../../src/providers";
import { providerSupportsModel } from "../../src/providers/catalog";
import { providerJoinEvidence } from "../../src/providers/provider-join";
import {
  wireMessageId,
  wireModelCallId,
  type WireEvent,
  type WireModelSelection,
  type WireQuery,
  type WireQueryInput,
  type WireUsageSnapshot,
  type WireUserInputFrame,
} from "../../src/wire";

export interface WireTurnFixture {
  output?: string;
  provider?: WireModelSelection["provider"];
  turns?: number;
  providerDurationMs?: number;
  status?: "succeeded" | "failed" | "cancelled";
  errorCode?: string;
  failureDetail?: string;
  terminal?: boolean;
  usage?: WireUsageSnapshot;
}

export interface WireTurnSequenceOptions {
  onInput?: (text: string, turn: number) => void;
  onContinue?: (input: WireQueryInput, turn: number) => void;
}

let fixtureSequence = 0;

function fixtureProvider(
  args: RoutedQueryArguments,
  provider: WireModelSelection["provider"] | undefined,
): WireModelSelection["provider"] {
  if (provider) return provider;
  const model = args.options.model;
  const anthropic = providerSupportsModel("anthropic", model);
  const openai = providerSupportsModel("openai", model);
  if (anthropic !== openai) return anthropic ? "anthropic" : "openai";
  throw new Error(`wire query fixture cannot infer provider from model ${String(model)}`);
}

/** Emit one writer-owned canonical model turn for outer-runtime tests. */
export async function* wireTurnEvents(
  args: RoutedQueryArguments,
  fixture: WireTurnFixture,
): AsyncGenerator<WireEvent> {
  const sequence = ++fixtureSequence;
  const modelCallId = wireModelCallId(`model-call:test-fixture:${sequence}`);
  yield args.writer.append({
    kind: "model-call.started",
    modelCallId,
    model: {
      provider: fixtureProvider(args, fixture.provider),
    },
    attempt: 1,
  });
  if (fixture.output !== undefined) {
    const messageId = wireMessageId(`message:test-fixture:${sequence}`);
    yield args.writer.append({
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "started",
      role: "assistant",
    });
    yield args.writer.append({
      kind: "message.recorded",
      messageId,
      modelCallId,
      stage: "completed",
      role: "assistant",
      content: fixture.output,
    });
  }
  if (fixture.terminal === false) return;
  const evidence = {
    ...(fixture.turns === undefined ? {} : {
      turns: { unit: "assistant-turn" as const, count: fixture.turns, comparable: true as const },
    }),
    ...(fixture.providerDurationMs === undefined ? {} : {
      providerDurationMs: fixture.providerDurationMs,
    }),
    ...(fixture.failureDetail === undefined ? {} : {
      failure: { detail: fixture.failureDetail },
    }),
  };
  yield args.writer.append({
    kind: "model-call.completed",
    modelCallId,
    status: fixture.status ?? "succeeded",
    origin: "provider",
    usage: fixture.usage ?? args.writer.snapshot()!.usage,
    usageCoverage: "exact",
    ...(fixture.errorCode === undefined ? {} : { errorCode: fixture.errorCode }),
    ...(Object.keys(evidence).length === 0 ? {} : { evidence }),
  });
}

export function wireTurnQuery(
  args: RoutedQueryArguments,
  fixture: WireTurnFixture,
): WireQuery {
  return {
    [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      return wireTurnEvents(args, fixture);
    },
  };
}

/** Emit the canonical dead-attempt settlement followed by its managed Codex replacement. */
export function wireManagedCodexRespawnQuery(
  args: RoutedQueryArguments,
  output: string,
): WireQuery {
  return {
    async *[Symbol.asyncIterator](): AsyncGenerator<WireEvent> {
      const sequence = ++fixtureSequence;
      const deadModelCallId = wireModelCallId(`model-call:test-respawn:${sequence}:dead`);
      yield args.writer.append({
        kind: "model-call.started",
        modelCallId: deadModelCallId,
        model: { provider: "openai" },
        attempt: 1,
      });
      yield args.writer.append({
        kind: "model-call.completed",
        modelCallId: deadModelCallId,
        status: "failed",
        origin: "north",
        usage: args.writer.snapshot()!.usage,
        usageCoverage: "unavailable",
			errorCode: "provider_session_replaced",
        evidence: {
          providerJoin: providerJoinEvidence("openai", {
            sessionId: `test-respawn-session-${sequence}-dead`,
            turnIds: [`test-respawn-turn-${sequence}-dead`],
            sessionPersistence: "ephemeral",
          }),
          turns: { unit: "provider-turn", count: 1, toolItems: 0, comparable: false },
          failure: {
				detail: "provider_session_replaced",
            landed: { completedTurns: 0, toolItems: 0 },
          },
        },
      });

      const replacementModelCallId = wireModelCallId(
        `model-call:test-respawn:${sequence}:replacement`,
      );
      const replacementMessageId = wireMessageId(`message:test-respawn:${sequence}:replacement`);
      yield args.writer.append({
        kind: "model-call.started",
        modelCallId: replacementModelCallId,
        model: { provider: "openai" },
        attempt: 2,
      });
      yield args.writer.append({
        kind: "message.recorded",
        messageId: replacementMessageId,
        modelCallId: replacementModelCallId,
        stage: "started",
        role: "assistant",
      });
      yield args.writer.append({
        kind: "message.recorded",
        messageId: replacementMessageId,
        modelCallId: replacementModelCallId,
        stage: "completed",
        role: "assistant",
        content: output,
      });
      yield args.writer.append({
        kind: "model-call.completed",
        modelCallId: replacementModelCallId,
        status: "succeeded",
        origin: "provider",
        usage: args.writer.snapshot()!.usage,
        usageCoverage: "exact",
        evidence: {
          providerJoin: providerJoinEvidence("openai", {
            sessionId: `test-respawn-session-${sequence}-replacement`,
            turnIds: [`test-respawn-turn-${sequence}-replacement`],
            sessionPersistence: "ephemeral",
          }),
          turns: { unit: "provider-turn", count: 1, toolItems: 0, comparable: false },
        },
      });
    },
  };
}

/** Emit one canonical model turn per retained-query iteration. */
export function wireTurnSequenceQuery(
  args: RoutedQueryArguments,
  fixtures: readonly WireTurnFixture[],
  options: WireTurnSequenceOptions = {},
): WireQuery {
  const firstFixture = fixtures[0];
  if (firstFixture === undefined) throw new Error("wire turn fixture sequence is empty");
  const liveInput = providerLiveInput(fixtureProvider(args, firstFixture.provider));
  if (liveInput !== "streaming") {
    let opened = false;
    return {
      executionTransport: "managed-app-server",
      [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
        return (async function*(): AsyncGenerator<WireEvent> {
          if (opened) throw new Error("turn-framed wire fixture iterator opened twice");
          opened = true;
          const input = wireInputIterator(args.input);
          for (const [turn, fixture] of fixtures.entries()) {
            const frame = await input.next();
            if (frame.done) throw new Error("wire turn fixture input ended before a user frame");
            options.onInput?.(frame.value.text, turn);
            yield* wireTurnEvents(args, fixture);
          }
        })();
      },
    };
  }
  let active = false;
  let pendingInput: WireQueryInput | undefined = args.input;
  let turn = 0;
  return {
    executionTransport: "sdk-stream",
    async continueTurn(input: WireQueryInput): Promise<void> {
      if (active) throw new Error("wire turn fixture continued before its iterator completed");
      if (pendingInput !== undefined) throw new Error("wire turn fixture continuation was not consumed");
      options.onContinue?.(input, turn);
      pendingInput = input;
    },
    [Symbol.asyncIterator](): AsyncIterator<WireEvent> {
      return (async function*(): AsyncGenerator<WireEvent> {
        if (active) throw new Error("wire turn fixture iterators overlapped");
        const input = pendingInput;
        if (input === undefined) throw new Error("wire turn fixture iterator opened without input");
        const fixture = fixtures[turn];
        if (fixture === undefined) throw new Error("wire turn fixture sequence exhausted");
        pendingInput = undefined;
        active = true;
        try {
          const frame = await wireInputIterator(input).next();
          if (frame.done) throw new Error("wire turn fixture input ended before a user frame");
          options.onInput?.(frame.value.text, turn);
          turn++;
          yield* wireTurnEvents(args, fixture);
        } finally {
          active = false;
        }
      })();
    },
  };
}

export function wireInputIterator(input: WireQueryInput): AsyncIterator<WireUserInputFrame> {
  if (typeof input !== "string") return input[Symbol.asyncIterator]();
  return (async function*(): AsyncGenerator<WireUserInputFrame> {
    yield { kind: "user.input", text: input };
  })();
}
