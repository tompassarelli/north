import { expect, test } from "bun:test";
import { BridgeWireSession } from "../src/bridge/provider";
import {
  WireEventWriter,
  wireModelCallId,
  wireRunId,
  type WireEvent,
  type WireQuery,
  type WireQueryInput,
} from "../src/wire";

class LiveContinuationQuery implements WireQuery {
  readonly inputs: string[] = [];
  readonly teardown: string[] = [];
  #events: readonly WireEvent[];
  #continued = Promise.withResolvers<void>();
  #closed = Promise.withResolvers<void>();

  constructor(events: readonly WireEvent[]) {
    this.#events = events;
  }

  async continueTurn(input: WireQueryInput): Promise<void> {
    if (typeof input !== "string") throw new Error("test query expects a text continuation");
    this.inputs.push(input);
    this.#continued.resolve();
  }

  async interrupt(): Promise<void> {
    this.teardown.push("interrupt");
  }

  async close(): Promise<void> {
    this.teardown.push("close");
    this.#closed.resolve();
  }

  async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
    yield this.#events[0]!;
    yield this.#events[1]!;
    await this.#continued.promise;
    yield this.#events[2]!;
    yield this.#events[3]!;
    await this.#closed.promise;
  }
}

class PerTurnContinuationQuery implements WireQuery {
  readonly inputs: string[] = [];
  iterations = 0;
  #events: readonly WireEvent[];
  #continued = false;

  constructor(events: readonly WireEvent[]) {
    this.#events = events;
  }

  async continueTurn(input: WireQueryInput): Promise<void> {
    if (typeof input !== "string") throw new Error("test query expects a text continuation");
    this.inputs.push(input);
    this.#continued = true;
  }

  async close(): Promise<void> {}

  async *[Symbol.asyncIterator](): AsyncIterator<WireEvent> {
    this.iterations += 1;
    if (this.iterations === 1) {
      yield this.#events[0]!;
      yield this.#events[1]!;
      return;
    }
    if (this.iterations === 2 && this.#continued) {
      yield this.#events[2]!;
      yield this.#events[3]!;
      return;
    }
    throw new Error("test query was iterated without an admitted turn");
  }
}

function wireTurns(label: string): readonly WireEvent[] {
  const writer = new WireEventWriter({ runId: wireRunId(`run:${label}`) });
  writer.append({ kind: "run.started", lifecycle: "running", owner: "bridge:test" });
  const first = wireModelCallId(`model-call:${label}:1`);
  const second = wireModelCallId(`model-call:${label}:2`);
  return writer.appendAll([
    {
      kind: "model-call.started",
      modelCallId: first,
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    },
    {
      kind: "model-call.completed",
      modelCallId: first,
      status: "succeeded",
      origin: "provider",
      usage: {
        lifetime: {
          inputTokens: 1,
          outputTokens: 1,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          modelCalls: 1,
        },
        context: { tokens: 2 },
      },
		usageCoverage: "exact",
    },
    {
      kind: "model-call.started",
      modelCallId: second,
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    },
    {
      kind: "model-call.completed",
      modelCallId: second,
      status: "succeeded",
      origin: "provider",
      usage: {
        lifetime: {
          inputTokens: 2,
          outputTokens: 2,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          modelCalls: 2,
        },
        context: { tokens: 4 },
      },
		usageCoverage: "exact",
    },
  ]);
}

test("a submitted turn reaches a query whose active iterator is waiting for continuation", async () => {
  const events = wireTurns("bridge-live-continuation");
  const query = new LiveContinuationQuery(events);
  const abort = new AbortController();
  const session = new BridgeWireSession(query, abort, new AbortController().signal);
  const iterator = session.events()[Symbol.asyncIterator]();

  expect((await iterator.next()).value?.kind).toBe("model-call.started");
  expect((await iterator.next()).value?.kind).toBe("model-call.completed");
  const waitingForContinuedTurn = iterator.next();
  await session.submitInput("continue on the same provider session");
  expect(query.inputs).toEqual(["continue on the same provider session"]);
  expect((await waitingForContinuedTurn).value?.kind).toBe("model-call.started");
  expect((await iterator.next()).value?.kind).toBe("model-call.completed");

  await session.terminateSession();
  expect((await iterator.next()).done).toBe(true);
  expect(query.teardown).toEqual(["interrupt", "close"]);
  expect(abort.signal.aborted).toBe(true);
});

test("a submitted turn wakes a provider whose prior turn iterator already returned", async () => {
  const query = new PerTurnContinuationQuery(wireTurns("bridge-per-turn-continuation"));
  const session = new BridgeWireSession(
    query,
    new AbortController(),
    new AbortController().signal,
  );
  const iterator = session.events()[Symbol.asyncIterator]();

  expect((await iterator.next()).value?.kind).toBe("model-call.started");
  expect((await iterator.next()).value?.kind).toBe("model-call.completed");
  const waitingForNextTurn = iterator.next();
  await Bun.sleep(5);
  expect(query.iterations).toBe(1);

  await session.submitInput("start a fresh per-turn iterator");
  expect((await waitingForNextTurn).value?.kind).toBe("model-call.started");
  expect((await iterator.next()).value?.kind).toBe("model-call.completed");
  const idle = iterator.next();
  await Bun.sleep(5);
  expect(query.iterations).toBe(2);

  await session.terminateSession();
  expect((await idle).done).toBe(true);
  expect(query.inputs).toEqual(["start a fresh per-turn iterator"]);
});
