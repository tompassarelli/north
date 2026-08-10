import type { ObservationCoverage } from "./observation";

export interface McpToolActivityCount {
  server: string;
  tool: string;
  count: number;
}

export interface McpOperationReceipt {
  tool: string;
  operation: string;
  durationMs: number;
  batchSize?: number;
  outcome: "ok" | string;
  resultSize: number;
}

export interface McpOperationAggregate {
  operation: string;
  count: number;
  totalDurationMs: number;
  meanDurationMs: number;
  failureCount: number;
}

export interface McpActivityObservation {
  source: string;
  coverage: ObservationCoverage;
  totalCalls?: number;
  tools: ReadonlyArray<Readonly<McpToolActivityCount>>;
  operationReceipts: ReadonlyArray<Readonly<McpOperationReceipt>>;
  operationAggregates: ReadonlyArray<Readonly<McpOperationAggregate>>;
}

const COMPONENT = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;
const MAX_OPERATION_RECEIPTS = 512;

function component(value: unknown): string | undefined {
  if (typeof value !== "string" || !COMPONENT.test(value)) return undefined;
  return value.toLowerCase();
}

/** Derive comparison metadata without retaining MCP arguments or results. */
export function mcpReceiptMetadata(
  payload: unknown, result: unknown, durationMs: unknown,
): Partial<McpOperationReceipt> {
  const object = result && typeof result === "object" && !Array.isArray(result)
    ? result as Record<string, unknown> : undefined;
  const tag = object && (object.operationTag ?? object.operation_tag ?? object.taxonomyTag);
  const failure = object && (object.failureClass ?? object.failure_class
    ?? (object.error && typeof object.error === "object"
      ? (object.error as Record<string, unknown>).type : undefined));
  const payloadRecord = payload && typeof payload === "object" && !Array.isArray(payload)
    ? payload as Record<string, unknown> : undefined;
  const batch = payloadRecord?.batch;
  const batchSize = Array.isArray(payload) ? payload.length : Array.isArray(batch) ? batch.length : undefined;
  let resultSize = 0;
  try { resultSize = JSON.stringify(result)?.length ?? 0; } catch { return {}; }
  return {
    operation: component(tag), outcome: component(failure) ?? "ok", durationMs: durationMs as number,
    resultSize, ...(batchSize === undefined ? {} : { batchSize }),
  };
}

export function parseAnthropicMcpName(value: unknown): { server: string; tool: string } | undefined {
  if (typeof value !== "string" || !value.startsWith("mcp__")) return undefined;
  const parts = value.slice(5).split("__");
  if (parts.length !== 2) return undefined;
  const server = component(parts[0]);
  const tool = component(parts[1]);
  return server && tool ? { server, tool } : undefined;
}

export function normalizeCodexMcpIdentity(
  serverValue: unknown,
  toolValue: unknown,
): { server: string; tool: string } | undefined {
  const server = component(serverValue);
  const tool = component(toolValue);
  return server && tool ? { server, tool } : undefined;
}

export class McpActivityAccumulator {
  private readonly calls = new Set<string>();
  private readonly counts = new Map<string, McpToolActivityCount>();
  private terminal = false;
  private identityLoss = false;
  private readonly receipts: McpOperationReceipt[] = [];
  private receiptLoss = false;

  constructor(private readonly source: string) {}

  observe(idValue: unknown, identity: { server: string; tool: string } | undefined,
    receipt?: Partial<McpOperationReceipt>): void {
    if (typeof idValue !== "string" || !idValue || idValue.length > 256) {
      this.identityLoss = true;
      return;
    }
    if (this.calls.has(idValue)) return;
    this.calls.add(idValue);
    if (!identity) {
      this.identityLoss = true;
      return;
    }
    const key = `${identity.server}\u0000${identity.tool}`;
    const prior = this.counts.get(key);
    this.counts.set(key, prior
      ? { ...prior, count: prior.count + 1 }
      : { ...identity, count: 1 });
    if (receipt === undefined) return;
    const durationMs = receipt.durationMs;
    const resultSize = receipt?.resultSize;
    const operation = component(receipt?.operation) ?? identity.tool;
    const outcome = component(receipt?.outcome) ?? "ok";
    const batchSize = receipt?.batchSize;
    if (typeof durationMs !== "number" || !Number.isSafeInteger(durationMs) || durationMs < 0
        || typeof resultSize !== "number" || !Number.isSafeInteger(resultSize) || resultSize < 0
        || (batchSize !== undefined && (!Number.isSafeInteger(batchSize) || batchSize < 0))) {
      this.receiptLoss = true;
      return;
    }
    if (this.receipts.length < MAX_OPERATION_RECEIPTS) this.receipts.push(Object.freeze({
      tool: `${identity.server}/${identity.tool}`, operation, durationMs, resultSize,
      ...(batchSize === undefined ? {} : { batchSize }), outcome,
    }));
    else this.receiptLoss = true;
  }

  private operationEvidence() {
    const operationReceipts = Object.freeze(this.receipts.map((receipt) => Object.freeze({ ...receipt })));
    const aggregates = new Map<string, { count: number; totalDurationMs: number; failureCount: number }>();
    for (const receipt of operationReceipts) {
      const prior = aggregates.get(receipt.operation) ?? { count: 0, totalDurationMs: 0, failureCount: 0 };
      prior.count++; prior.totalDurationMs += receipt.durationMs;
      if (receipt.outcome !== "ok") prior.failureCount++;
      aggregates.set(receipt.operation, prior);
    }
    const operationAggregates = Object.freeze([...aggregates.entries()]
      .sort(([left], [right]) => left.localeCompare(right)).map(([operation, aggregate]) => Object.freeze({
        operation, ...aggregate, meanDurationMs: aggregate.totalDurationMs / aggregate.count,
      })));
    return { operationReceipts, operationAggregates };
  }

  complete(): void { this.terminal = true; }

  reopen(): void { this.terminal = false; }

  /** A dead provider session makes exact lane-wide MCP coverage impossible. */
  retireSession(): void {
    this.identityLoss = true;
    this.terminal = false;
  }

  /**
   * What was observed when a turn DIED mid-flight. Coverage is always "partial":
   * the counts are real calls, but an interrupted turn can never claim to have
   * seen all of them. Used only to harvest a failed managed run — a clean turn
   * still goes through complete() + snapshot() and keeps exact coverage.
   */
  harvest(): McpActivityObservation {
    if (this.terminal) return this.snapshot();
    const tools = [...this.counts.values()].sort((left, right) =>
      left.server.localeCompare(right.server) || left.tool.localeCompare(right.tool));
    return Object.freeze({
      source: this.source,
      coverage: this.receiptLoss ? "partial" as const : "partial" as const,
      totalCalls: this.calls.size,
      tools: Object.freeze(tools.map((entry) => Object.freeze({ ...entry }))),
      ...this.operationEvidence(),
    });
  }

  snapshot(): McpActivityObservation {
    if (!this.terminal) return { source: this.source, coverage: "unknown", tools: [], operationReceipts: [], operationAggregates: [] };
    const tools = [...this.counts.values()].sort((left, right) =>
      left.server.localeCompare(right.server) || left.tool.localeCompare(right.tool));
    return Object.freeze({
      source: this.source,
      coverage: this.identityLoss || this.receiptLoss ? "partial" as const : "exact" as const,
      totalCalls: this.calls.size,
      tools: Object.freeze(tools.map((entry) => Object.freeze({ ...entry }))),
      ...this.operationEvidence(),
    });
  }
}

export function unknownMcpActivity(source: string): McpActivityObservation {
  return { source, coverage: "unknown", tools: [], operationReceipts: [], operationAggregates: [] };
}
