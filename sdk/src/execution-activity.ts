export type ExecutionActivityOrigin = "outer" | "provider";

export interface ExecutionActivityEvidence {
  origin: ExecutionActivityOrigin;
  kind: string;
  observedAt: string;
}

export interface ExecutionActivitySnapshot {
  lastOuter?: ExecutionActivityEvidence;
  lastProvider?: ExecutionActivityEvidence;
}

export interface ExecutionActivitySource {
  snapshot(): ExecutionActivitySnapshot & { sequence: number };
  subscribe(listener: () => void): () => void;
}

export interface ExecutionActivityEmitter {
  readonly source: ExecutionActivitySource;
  record(origin: ExecutionActivityOrigin, kind: string): void;
}

const ACTIVITY_KIND = /^[a-z][a-z0-9._/-]{0,127}$/;

/**
 * One query-local authenticated activity channel. Consumers receive only the
 * read side; the provider adapter retains the emitter and records a pulse only
 * after it has validated a provider-native frame against the active execution.
 */
export function createExecutionActivityEmitter(
  now: () => Date = () => new Date(),
): ExecutionActivityEmitter {
  let sequence = 0;
  let lastOuter: ExecutionActivityEvidence | undefined;
  let lastProvider: ExecutionActivityEvidence | undefined;
  const listeners = new Set<() => void>();
  const source: ExecutionActivitySource = Object.freeze({
    snapshot: () => ({
      sequence,
      ...(lastOuter ? { lastOuter } : {}),
      ...(lastProvider ? { lastProvider } : {}),
    }),
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  });
  return Object.freeze({
    source,
    record(origin: ExecutionActivityOrigin, kind: string) {
      if ((origin !== "outer" && origin !== "provider") || !ACTIVITY_KIND.test(kind))
        throw new Error("invalid execution activity evidence");
      const evidence = Object.freeze({
        origin,
        kind,
        observedAt: now().toISOString(),
      });
      if (origin === "outer") lastOuter = evidence;
      else lastProvider = evidence;
      sequence++;
      for (const listener of [...listeners]) listener();
    },
  });
}

/** Forward authenticated pulses while keeping the destination emitter private. */
export function forwardExecutionActivity(
  source: ExecutionActivitySource | undefined,
  destination: ExecutionActivityEmitter,
): () => void {
  if (!source) return () => {};
  let sequence = source.snapshot().sequence;
  return source.subscribe(() => {
    const snapshot = source.snapshot();
    if (snapshot.sequence === sequence) return;
    sequence = snapshot.sequence;
    if (snapshot.lastProvider)
      destination.record("provider", snapshot.lastProvider.kind);
    else if (snapshot.lastOuter)
      destination.record("outer", snapshot.lastOuter.kind);
  });
}
