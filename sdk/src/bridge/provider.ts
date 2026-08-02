export interface NormalizedProviderEvent {
  kind: string;
  data: Record<string, unknown>;
  turnTerminal?: boolean;
}

export interface BridgeProviderSession {
  submitInput(input: string): Promise<void>;
  interruptTurn(): Promise<void>;
  terminateSession(): Promise<void>;
  events(): AsyncIterable<NormalizedProviderEvent>;
}

export interface BridgeProviderExecution {
  open(
    context: { executionId: string; prompt: string; cwd: string; signal: AbortSignal },
  ): Promise<BridgeProviderSession>;
}

class InputChannel implements AsyncIterable<string> {
  private values: string[] = [];
  private pulls: Array<(result: IteratorResult<string>) => void> = [];
  private ended = false;

  push(value: string): void {
    if (this.ended) throw new Error("provider input channel is closed");
    const pull = this.pulls.shift();
    if (pull) pull({ done: false, value });
    else this.values.push(value);
  }

  close(): void {
    if (this.ended) return;
    this.ended = true;
    for (const pull of this.pulls.splice(0)) pull({ done: true, value: undefined });
  }

  [Symbol.asyncIterator](): AsyncIterator<string> {
    return {
      next: async () => {
        const value = this.values.shift();
        if (value !== undefined) return { done: false, value };
        if (this.ended) return { done: true, value: undefined };
        return new Promise<IteratorResult<string>>((resolve) => this.pulls.push(resolve));
      },
      return: async () => {
        this.close();
        return { done: true, value: undefined };
      },
    };
  }
}

class EventChannel implements AsyncIterable<NormalizedProviderEvent> {
  private values: NormalizedProviderEvent[] = [];
  private wake?: () => void;
  private ended = false;
  private failure?: unknown;
  private consumed = false;

  push(value: NormalizedProviderEvent): void {
    if (this.ended) return;
    this.values.push(value);
    this.wake?.();
    this.wake = undefined;
  }

  close(): void {
    this.ended = true;
    this.wake?.();
    this.wake = undefined;
  }

  fail(error: unknown): void {
    this.failure = error;
    this.close();
  }

  async *[Symbol.asyncIterator](): AsyncIterator<NormalizedProviderEvent> {
    if (this.consumed) throw new Error("provider event stream is single-consumer");
    this.consumed = true;
    while (true) {
      const value = this.values.shift();
      if (value) {
        yield value;
        continue;
      }
      if (this.failure) throw this.failure;
      if (this.ended) return;
      await new Promise<void>((resolve) => { this.wake = resolve; });
    }
  }
}

function jsonData(value: unknown): Record<string, unknown> {
  const normalized = JSON.parse(JSON.stringify(value)) as unknown;
  return normalized && typeof normalized === "object" && !Array.isArray(normalized)
    ? normalized as Record<string, unknown>
    : { value: normalized };
}

export const codexBridgeProvider: BridgeProviderExecution = {
  async open(context): Promise<BridgeProviderSession> {
    const [{ harnessOptions }, { applyOrchestrationStaffing }, { openaiProvider }] = await Promise.all([
      import("../harness"),
      import("../orchestration-staffing"),
      import("../providers/openai"),
    ]);
    const routingMetadata = applyOrchestrationStaffing({ role: "implementer" });
    const abortController = new AbortController();
    const options = harnessOptions({
      self: `bridge-${context.executionId}`,
      provider: "openai",
      routingMetadata,
      role: routingMetadata.role,
      posture: routingMetadata.posture,
      cwd: context.cwd,
      model: process.env.NORTH_BRIDGE_MODEL,
      presenceRegistrar: false,
      presenceRenewer: false,
      systemPrompt: "You are a North Bridge execution. Complete the attached operator request.",
      abortController,
    });
    await openaiProvider.admit?.({ options });

    const input = new InputChannel();
    input.push(context.prompt);
    const query = openaiProvider.query({ prompt: input, options });
    const events = new EventChannel();
    let activitySequence = query.executionActivity?.snapshot().sequence ?? 0;
    const unsubscribe = query.executionActivity?.subscribe(() => {
      const snapshot = query.executionActivity!.snapshot();
      if (snapshot.sequence === activitySequence) return;
      activitySequence = snapshot.sequence;
      const activity = snapshot.lastProvider ?? snapshot.lastOuter;
      if (activity) events.push({ kind: "activity", data: jsonData(activity) });
    });
    let terminating = false;
    const terminate = () => { void terminateSession().catch(() => {}); };

    const pump = (async () => {
      try {
        for await (const frame of query) {
          const value = jsonData(frame);
          const type = typeof value.type === "string" && value.type ? value.type : "event";
          events.push({ kind: type, data: value, ...(type === "result" ? { turnTerminal: true } : {}) });
        }
      } catch (error) {
        if (!terminating) events.fail(error);
      } finally {
        unsubscribe?.();
        context.signal.removeEventListener("abort", terminate);
        input.close();
        try { await query.close?.(); }
        catch (error) { if (!terminating) events.fail(error); }
        finally { events.close(); }
      }
    })();

    let termination: Promise<void> | undefined;
    const terminateSession = (): Promise<void> => {
      if (termination) return termination;
      terminating = true;
      input.close();
      abortController.abort();
      termination = (async () => {
        await query.interrupt?.();
        await pump;
      })();
      return termination;
    };
    context.signal.addEventListener("abort", terminate, { once: true });
    if (context.signal.aborted) void terminateSession();

    return {
      async submitInput(value) { input.push(value); },
      async interruptTurn() {
        if (!query.interruptTurn) throw new Error("provider does not support turn interruption");
        await query.interruptTurn();
      },
      terminateSession,
      events: () => events,
    };
  },
};
