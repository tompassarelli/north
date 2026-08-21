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

declare function Agent(
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
declare function makeModel(viewId: string): unknown;
declare function replaceProjection(
  model: unknown,
  agents: unknown[],
  listItems: unknown[],
  boardItems: unknown[],
): unknown;
declare function selectAgent(model: unknown, agentId: string): unknown;
declare function upsertAgent(model: unknown, agent: unknown): unknown;
declare function snapshot(model: unknown): BridgeModelSnapshot;
declare const bridgeSnapshotActiveViewId: (...args: any[]) => any;

export {
  Agent as "->Agent",
  bridgeSnapshotActiveViewId as "bridgesnapshot-active-view-id",
  makeModel as "make-model",
  replaceProjection as "replace-projection",
  selectAgent as "select-agent",
  snapshot as "snapshot",
  upsertAgent as "upsert-agent",
};
