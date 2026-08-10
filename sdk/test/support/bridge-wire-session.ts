import {
  wireMessageId,
  wireModelCallId,
  type WireEvent,
  type WireEventDraft,
  type WireEventWriter,
  type WireModelCallId,
} from "../../src/wire";
import type {
  BridgeProviderOpenContext,
  BridgeProviderSession,
} from "../../src/bridge/provider";

export interface BridgeWireTestSessionOptions {
  initialAssistant?: string;
  onEffect?: (effect: string) => void;
  interruptFailure?: Error;
  terminationFailure?: Error;
  terminationNeverSettles?: boolean;
}

export class BridgeWireTestSession implements BridgeProviderSession {
  readonly effects: string[] = [];
  #writer: WireEventWriter;
  #label: string;
  #options: BridgeWireTestSessionOptions;
  #queue: WireEvent[] = [];
  #waiting?: PromiseWithResolvers<void>;
  #ended = false;
  #turn = 0;
  #message = 0;
  #activeModelCall?: WireModelCallId;

  constructor(
    context: Pick<BridgeProviderOpenContext, "executionId" | "prompt" | "writer">,
    options: BridgeWireTestSessionOptions = {},
  ) {
    this.#writer = context.writer;
    this.#label = context.executionId;
    this.#options = options;
    this.startTurn();
    if (options.initialAssistant !== undefined) this.assistant(options.initialAssistant);
  }

  #effect(value: string): void {
    this.effects.push(value);
    this.#options.onEffect?.(value);
  }

  #publish(events: readonly WireEvent[]): void {
    this.#queue.push(...events);
    this.#waiting?.resolve();
    this.#waiting = undefined;
  }

  publish(draft: WireEventDraft): WireEvent {
    const event = this.#writer.append(draft);
    this.#publish([event]);
    return event;
  }

  publishAll(drafts: readonly WireEventDraft[]): readonly WireEvent[] {
    const events = this.#writer.appendAll(drafts);
    this.#publish(events);
    return events;
  }

  startTurn(): WireModelCallId {
    if (this.#activeModelCall) throw new Error("test session already has an active model call");
    this.#turn += 1;
    const modelCallId = wireModelCallId(`model-call:bridge-test:${this.#label}:${this.#turn}`);
    this.#activeModelCall = modelCallId;
    this.publish({
      kind: "model-call.started",
      modelCallId,
      model: { provider: "openai", capabilityClass: "authoring" },
      effort: "high",
      attempt: 1,
    });
    return modelCallId;
  }

  assistant(text: string): void {
    const modelCallId = this.#activeModelCall;
    if (!modelCallId) throw new Error("test session has no active model call");
    this.#message += 1;
    const messageId = wireMessageId(`message:bridge-test:${this.#label}:${this.#message}`);
    this.publishAll([
      {
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "started",
        role: "assistant",
      },
      {
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "delta",
        role: "assistant",
        content: text,
      },
      {
        kind: "message.recorded",
        messageId,
        modelCallId,
        stage: "completed",
        role: "assistant",
      },
    ]);
  }

  complete(
    result?: string,
    status: "succeeded" | "failed" | "cancelled" = "succeeded",
  ): void {
    const modelCallId = this.#activeModelCall;
    if (!modelCallId) throw new Error("test session has no active model call");
    if (result !== undefined) this.assistant(result);
    this.publish({
      kind: "model-call.completed",
      modelCallId,
      status,
      origin: "provider",
      usage: {
        lifetime: {
          inputTokens: this.#turn,
          outputTokens: this.#turn,
          cacheReadTokens: 0,
          cacheWriteTokens: 0,
          reasoningTokens: 0,
          modelCalls: this.#turn,
        },
        context: { tokens: this.#turn * 2 },
      },
		usageCoverage: "exact",
    });
    this.#activeModelCall = undefined;
  }

  replaceProviderSession(): WireModelCallId {
    const deadModelCallId = this.#activeModelCall;
    if (!deadModelCallId) throw new Error("test session has no active model call");
    this.#turn += 1;
    const replacementModelCallId = wireModelCallId(
      `model-call:bridge-test:${this.#label}:${this.#turn}:replacement`,
    );
    this.publishAll([
      {
        kind: "model-call.completed",
        modelCallId: deadModelCallId,
        status: "failed",
        origin: "north",
        usage: this.#writer.snapshot()!.usage,
        usageCoverage: "unavailable",
        errorCode: "provider_session_replaced",
      },
      {
        kind: "model-call.started",
        modelCallId: replacementModelCallId,
        model: { provider: "openai", capabilityClass: "authoring" },
        effort: "high",
        attempt: 2,
      },
    ]);
    this.#activeModelCall = replacementModelCallId;
    return replacementModelCallId;
  }

  async submitInput(input: string): Promise<void> {
    this.#effect(`submit:${input}`);
    this.startTurn();
  }

  async interruptTurn(): Promise<void> {
    this.#effect("interrupt");
    if (this.#options.interruptFailure) throw this.#options.interruptFailure;
  }

  async terminateSession(): Promise<void> {
    if (this.#ended) return;
    this.#effect("terminate");
    this.#ended = true;
    this.#waiting?.resolve();
    this.#waiting = undefined;
    if (this.#options.terminationNeverSettles) {
      await Promise.withResolvers<void>().promise;
    }
    if (this.#options.terminationFailure) throw this.#options.terminationFailure;
  }

  forceTerminateSession(): void {
    this.#effect("force-terminate");
    this.#ended = true;
    this.#waiting?.resolve();
    this.#waiting = undefined;
  }

  async *events(): AsyncGenerator<WireEvent, void, unknown> {
    while (true) {
      const event = this.#queue.shift();
      if (event) {
        yield event;
        continue;
      }
      if (this.#ended) return;
      this.#waiting = Promise.withResolvers<void>();
      await this.#waiting.promise;
      this.#waiting = undefined;
    }
  }
}
