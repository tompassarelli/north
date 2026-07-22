import type { ObservationCoverage } from "./run-ledger";

export interface McpToolActivityCount {
  server: string;
  tool: string;
  count: number;
}

export interface McpActivityObservation {
  source: string;
  coverage: ObservationCoverage;
  totalCalls?: number;
  tools: ReadonlyArray<Readonly<McpToolActivityCount>>;
}

const COMPONENT = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$/;

function component(value: unknown): string | undefined {
  if (typeof value !== "string" || !COMPONENT.test(value)) return undefined;
  return value.toLowerCase();
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

  constructor(private readonly source: string) {}

  observe(idValue: unknown, identity: { server: string; tool: string } | undefined): void {
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
  }

  complete(): void { this.terminal = true; }

  reopen(): void { this.terminal = false; }

  snapshot(): McpActivityObservation {
    if (!this.terminal) return { source: this.source, coverage: "unknown", tools: [] };
    const tools = [...this.counts.values()].sort((left, right) =>
      left.server.localeCompare(right.server) || left.tool.localeCompare(right.tool));
    return Object.freeze({
      source: this.source,
      coverage: this.identityLoss ? "partial" as const : "exact" as const,
      totalCalls: this.calls.size,
      tools: Object.freeze(tools.map((entry) => Object.freeze({ ...entry }))),
    });
  }
}

export function unknownMcpActivity(source: string): McpActivityObservation {
  return { source, coverage: "unknown", tools: [] };
}
