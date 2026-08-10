export interface AgentSnapshot {
  id: string;
  name: string;
  status: string;
  task: string;
}

export interface BridgeModelSnapshot {
  agents: AgentSnapshot[];
}

export function Agent(id: string, name: string, status: string, task: string): unknown;
export function make_model(viewId: string): unknown;
export function upsert_agent(model: unknown, agent: unknown): unknown;
export function snapshot(model: unknown): BridgeModelSnapshot;
