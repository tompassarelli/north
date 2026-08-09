import type { BridgeLaunchProvider, BridgeLaunchRole } from "./protocol";

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
    context: {
      executionId: string;
      prompt: string;
      cwd: string;
      role: BridgeLaunchRole;
      provider: BridgeLaunchProvider;
      signal: AbortSignal;
    },
  ): Promise<BridgeProviderSession>;
}

export function bridgeSystemPrompt(role: BridgeLaunchRole): string {
  return role === "director"
    ? "You are the North Bridge supervisor. Use the host-provided North MCP spawn and dispatch tools to coordinate the attached operator request; do not run North coordination commands through the sandboxed shell."
    : "You are a North Bridge implementation worker. Complete the attached operator request in the assigned workspace and report the result; do not spawn or delegate other agents.";
}

// Codex takes a bare turn; the Claude Agent SDK's streaming input accepts only a
// user-message envelope and silently produces no turn for anything else.
type BridgeInput = string | {
  type: "user";
  message: { role: "user"; content: string };
  parent_tool_use_id: null;
};

function bridgeInput(provider: BridgeLaunchProvider, text: string): BridgeInput {
  return provider === "anthropic"
    ? { type: "user", message: { role: "user", content: text }, parent_tool_use_id: null }
    : text;
}

class InputChannel implements AsyncIterable<BridgeInput> {
  private values: BridgeInput[] = [];
  private pulls: Array<(result: IteratorResult<BridgeInput>) => void> = [];
  private ended = false;

  push(value: BridgeInput): void {
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

  [Symbol.asyncIterator](): AsyncIterator<BridgeInput> {
    return {
      next: async () => {
        const value = this.values.shift();
        if (value !== undefined) return { done: false, value };
        if (this.ended) return { done: true, value: undefined };
        return new Promise<IteratorResult<BridgeInput>>((resolve) => this.pulls.push(resolve));
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

export const bridgeProvider: BridgeProviderExecution = {
  async open(context): Promise<BridgeProviderSession> {
    const [
      { harnessOptions }, { applyOrchestrationStaffing }, { openaiProvider }, { anthropicProvider },
      { selectProviderForExecution }, { markCoordinationOptional },
    ] = await Promise.all([
      import("../harness"),
      import("../orchestration-staffing"),
      import("../providers/openai"),
      import("../providers/anthropic"),
      import("../provider-routing"),
      import("../execution-admission"),
    ]);
    const agentProvider = context.provider === "anthropic" ? anthropicProvider : openaiProvider;
    // Without a routed target the adapter falls back to ambient credentials, which
    // are nobody's account: the Claude SDK then fails the turn on an expired OAuth.
    const decision = await selectProviderForExecution({ provider: context.provider });
    const target = decision.routingTargets[decision.target];
    const routingMetadata = applyOrchestrationStaffing({ role: context.role });
    const abortController = new AbortController();
    const options = harnessOptions({
      self: `bridge-${context.executionId}`,
      provider: context.provider,
      routingMetadata,
      role: routingMetadata.role,
      posture: routingMetadata.posture,
      cwd: context.cwd,
      model: process.env.NORTH_BRIDGE_MODEL,
      presenceRegistrar: false,
      presenceRenewer: false,
      systemPrompt: bridgeSystemPrompt(context.role),
      abortController,
    });
    // Bridge sessions are interactive chat: provider + auth are the only
    // structural dependencies; the coordinator is telemetry when reachable.
    // Marked out-of-band — a property write here would invalidate the harness
    // authority seal, which covers the exact option key set.
    markCoordinationOptional(options);
    await agentProvider.admit?.({ options, ...(target ? { target } : {}) });

    const input = new InputChannel();
    input.push(bridgeInput(context.provider, context.prompt));
    const query = agentProvider.query({
      prompt: input, options, ...(target ? { target } : {}),
    });
    const events = new EventChannel();
    events.push({
      kind: "session.config",
      data: {
        model: options.model,
        effort: options.effort,
        cwd: options.cwd ?? context.cwd,
        target: decision.target,
      },
    });
    let activitySequence = query.executionActivity?.snapshot().sequence ?? 0;
    const unsubscribe = query.subscribeProviderEvents
      ? query.subscribeProviderEvents((event) => {
          events.push({ kind: "codex.event", data: jsonData(event) });
        })
      : query.executionActivity?.subscribe(() => {
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
      async submitInput(value) { input.push(bridgeInput(context.provider, value)); },
      async interruptTurn() {
        if (!query.interruptTurn) throw new Error("provider does not support turn interruption");
        await query.interruptTurn();
      },
      terminateSession,
      events: () => events,
    };
  },
};

const BRIDGE_PRESSURE_RANK: Record<string, number> = {
  plenty: 4, normal: 3, low: 2, unknown: 1, exhausted: 0,
};

/**
 * Unpinned launches follow capacity: an exhausted entitlement is the one failure
 * a supervisor cannot work around, and it surfaces only as a provider error mid-turn.
 * Falls back to openai so a probe failure never blocks a launch outright.
 */
export async function selectBridgeProvider(): Promise<BridgeLaunchProvider> {
  try {
    const { collectProvidersStatus } = await import("../providers-cli");
    const document = await collectProvidersStatus();
    let best: { provider: BridgeLaunchProvider; rank: number } | undefined;
    for (const entry of document.providers) {
      for (const target of entry.targets) {
        if (target.routing !== "eligible") continue;
        if (target.provider !== "anthropic" && target.provider !== "openai") continue;
        const rank = BRIDGE_PRESSURE_RANK[target.headroom] ?? 0;
        if (!best || rank > best.rank) best = { provider: target.provider, rank };
      }
    }
    return best?.provider ?? "openai";
  } catch {
    return "openai";
  }
}
