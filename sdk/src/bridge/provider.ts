export interface NormalizedProviderEvent {
  kind: string;
  data: Record<string, unknown>;
}

export interface BridgeProviderExecution {
  execute(
    context: { executionId: string; prompt: string; cwd: string; signal: AbortSignal },
    onEvent: (event: NormalizedProviderEvent) => void,
  ): Promise<void>;
}

function jsonData(value: unknown): Record<string, unknown> {
  const normalized = JSON.parse(JSON.stringify(value)) as unknown;
  return normalized && typeof normalized === "object" && !Array.isArray(normalized)
    ? normalized as Record<string, unknown>
    : { value: normalized };
}

export const codexBridgeProvider: BridgeProviderExecution = {
  async execute(context, onEvent): Promise<void> {
    const [{ harnessOptions }, { applyOrchestrationStaffing }, { openaiProvider }] = await Promise.all([
      import("../harness"),
      import("../orchestration-staffing"),
      import("../providers/openai"),
    ]);
    const routingMetadata = applyOrchestrationStaffing({ role: "implementer" });
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
      abortController: new AbortController(),
    });
    await openaiProvider.admit?.({ options });
    const query = openaiProvider.query({ prompt: context.prompt, options });
    const interrupt = () => { void query.interrupt?.(); };
    context.signal.addEventListener("abort", interrupt, { once: true });
    let activitySequence = query.executionActivity?.snapshot().sequence ?? 0;
    const unsubscribe = query.executionActivity?.subscribe(() => {
      const snapshot = query.executionActivity!.snapshot();
      if (snapshot.sequence === activitySequence) return;
      activitySequence = snapshot.sequence;
      const activity = snapshot.lastProvider ?? snapshot.lastOuter;
      if (activity) onEvent({ kind: "activity", data: jsonData(activity) });
    });
    try {
      for await (const frame of query) {
        const value = jsonData(frame);
        const type = typeof value.type === "string" && value.type ? value.type : "event";
        onEvent({ kind: type, data: value });
      }
    } finally {
      unsubscribe?.();
      context.signal.removeEventListener("abort", interrupt);
      await query.close?.();
    }
  },
};
