export interface AgentSnapshot {
  id: string;
  name: string;
  status: string;
  task: string;
  provider: string;
  provider_target: string;
  provider_label: string;
  model: string;
  model_display: string;
  effort: string;
  orchestration_provenance: string;
  goal: string;
  state: string;
}

export interface BridgeModelSnapshot {
  agents: AgentSnapshot[];
  selected_agent: string;
}

export function Agent(
  id: string,
  name: string,
  status: string,
  task: string,
  provider: string,
  providerTarget: string,
  providerLabel: string,
  model: string,
  modelDisplay: string,
  effort: string,
  orchestrationProvenance: string,
  goal: string,
  state: string,
): unknown;
export function make_model(viewId: string): unknown;
export function replace_projection(
  model: unknown,
  agents: unknown[],
  listItems: unknown[],
  boardItems: unknown[],
): unknown;
export function select_agent(model: unknown, agentId: string): unknown;
export function upsert_agent(model: unknown, agent: unknown): unknown;
export function snapshot(model: unknown): BridgeModelSnapshot;
