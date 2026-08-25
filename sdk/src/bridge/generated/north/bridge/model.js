import { record_value as $$bc$record_value, str as $$bc$str } from '../../beagle/core.js';

function Agent(id, name, status, task, provider, provider_target, provider_label, model, model_display, effort, orchestration_provenance, goal, state) {
  return $$bc$record_value("north.bridge.model/Agent", {_tag: "Agent", id, name, status, task, provider, provider_target, provider_label, model, model_display, effort, orchestration_provenance, goal, state});
}

function agent_id(r) { return r.id; }

function agent_name(r) { return r.name; }

function agent_status(r) { return r.status; }

function agent_task(r) { return r.task; }

function agent_provider(r) { return r.provider; }

function agent_provider_target(r) { return r.provider_target; }

function agent_provider_label(r) { return r.provider_label; }

function agent_model(r) { return r.model; }

function agent_model_display(r) { return r.model_display; }

function agent_effort(r) { return r.effort; }

function agent_orchestration_provenance(r) { return r.orchestration_provenance; }

function agent_goal(r) { return r.goal; }

function agent_state(r) { return r.state; }

function WorkItem(id, title, body, condition, driver, dependencies) {
  return $$bc$record_value("north.bridge.model/WorkItem", {_tag: "WorkItem", id, title, body, condition, driver, dependencies});
}

function workitem_id(r) { return r.id; }

function workitem_title(r) { return r.title; }

function workitem_body(r) { return r.body; }

function workitem_condition(r) { return r.condition; }

function workitem_driver(r) { return r.driver; }

function workitem_dependencies(r) { return r.dependencies; }

function BridgeModel(agents, list, board, layout, active_view_id, selected_agent, selected_thread, filter_text, notice) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {_tag: "BridgeModel", agents, list, board, layout, active_view_id, selected_agent, selected_thread, filter_text, notice});
}

function bridgemodel_agents(r) { return r.agents; }

function bridgemodel_list(r) { return r.list; }

function bridgemodel_board(r) { return r.board; }

function bridgemodel_layout(r) { return r.layout; }

function bridgemodel_active_view_id(r) { return r.active_view_id; }

function bridgemodel_selected_agent(r) { return r.selected_agent; }

function bridgemodel_selected_thread(r) { return r.selected_thread; }

function bridgemodel_filter_text(r) { return r.filter_text; }

function bridgemodel_notice(r) { return r.notice; }

function BridgeSnapshot(agents, list, board, layout, active_view_id, selected_agent, selected_thread, notice) {
  return $$bc$record_value("north.bridge.model/BridgeSnapshot", {_tag: "BridgeSnapshot", agents, list, board, layout, active_view_id, selected_agent, selected_thread, notice});
}

function bridgesnapshot_agents(r) { return r.agents; }

function bridgesnapshot_list(r) { return r.list; }

function bridgesnapshot_board(r) { return r.board; }

function bridgesnapshot_layout(r) { return r.layout; }

function bridgesnapshot_active_view_id(r) { return r.active_view_id; }

function bridgesnapshot_selected_agent(r) { return r.selected_agent; }

function bridgesnapshot_selected_thread(r) { return r.selected_thread; }

function bridgesnapshot_notice(r) { return r.notice; }

function canonical_view(view_id) {
  return ((((_truthy) => _truthy !== false && _truthy != null)(((view_id === "board") || (view_id === "kanban")))) ? "board" : (((_truthy) => _truthy !== false && _truthy != null)(((view_id === "graph") || (view_id === "dag")))) ? "graph" : "list");
}

function make_model(view_id) {
  const view = canonical_view(view_id);
  return BridgeModel([], [], [], "vertical", view, "", "", "", "");
}

function replace_projection(model, agents, list_items, board_items) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, agents: agents, list: list_items, board: board_items});
}

function upsert_agent(model, agent) {
  const agents = bridgemodel_agents(model).filter((existing) => (!(agent_id(existing) === agent_id(agent))));
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, agents: agents.concat(agent)});
}

function remove_agent(model, removed_id) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, agents: bridgemodel_agents(model).filter((existing) => (!(agent_id(existing) === removed_id)))});
}

function focus_view(model, view_id) {
  const view = canonical_view(view_id);
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, active_view_id: view, notice: $$bc$str("view ", view)});
}

function set_layout(model, layout) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, layout: ((layout === "horizontal") ? "horizontal" : "vertical")});
}

function select_agent(model, agent_id) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, selected_agent: agent_id, notice: ""});
}

function select_thread(model, thread_id) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, selected_thread: thread_id, notice: ((thread_id === "") ? "no thread selected" : $$bc$str("selected @", thread_id))});
}

function set_filter(model, filter_text) {
  const trimmed = filter_text.trim();
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, filter_text: trimmed.toLowerCase(), notice: ((trimmed === "") ? "filter cleared" : $$bc$str("filter: ", filter_text))});
}

function visible_work(model, source) {
  const agent_id = bridgemodel_selected_agent(model);
  const needle = bridgemodel_filter_text(model);
  return source.filter((item) => ((_logical) => (_logical !== false && _logical != null ? ((needle === "") || $$bc$str(workitem_id(item), " ", workitem_title(item), " ", workitem_body(item), " ", workitem_condition(item), " ", workitem_driver(item)).toLowerCase().includes(needle)) : _logical))(((agent_id === "") || (workitem_driver(item) === agent_id))));
}

function snapshot(model) {
  return BridgeSnapshot(bridgemodel_agents(model), visible_work(model, bridgemodel_list(model)), visible_work(model, bridgemodel_board(model)), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_notice(model));
}

export { Agent as "->Agent" };
export { BridgeSnapshot as "->BridgeSnapshot" };
export { WorkItem as "->WorkItem" };
export { Agent as "Agent" };
export { BridgeSnapshot as "BridgeSnapshot" };
export { WorkItem as "WorkItem" };
export { agent_effort as "agent-effort" };
export { agent_goal as "agent-goal" };
export { agent_id as "agent-id" };
export { agent_model as "agent-model" };
export { agent_model_display as "agent-model-display" };
export { agent_name as "agent-name" };
export { agent_orchestration_provenance as "agent-orchestration-provenance" };
export { agent_provider as "agent-provider" };
export { agent_provider_label as "agent-provider-label" };
export { agent_provider_target as "agent-provider-target" };
export { agent_state as "agent-state" };
export { agent_status as "agent-status" };
export { agent_task as "agent-task" };
export { bridgesnapshot_active_view_id as "bridgesnapshot-active-view-id" };
export { bridgesnapshot_agents as "bridgesnapshot-agents" };
export { bridgesnapshot_board as "bridgesnapshot-board" };
export { bridgesnapshot_layout as "bridgesnapshot-layout" };
export { bridgesnapshot_list as "bridgesnapshot-list" };
export { bridgesnapshot_notice as "bridgesnapshot-notice" };
export { bridgesnapshot_selected_agent as "bridgesnapshot-selected-agent" };
export { bridgesnapshot_selected_thread as "bridgesnapshot-selected-thread" };
export { focus_view as "focus-view" };
export { make_model as "make-model" };
export { remove_agent as "remove-agent" };
export { replace_projection as "replace-projection" };
export { select_agent as "select-agent" };
export { select_thread as "select-thread" };
export { set_filter as "set-filter" };
export { snapshot as "snapshot" };
export { upsert_agent as "upsert-agent" };
export { workitem_body as "workitem-body" };
export { workitem_condition as "workitem-condition" };
export { workitem_dependencies as "workitem-dependencies" };
export { workitem_driver as "workitem-driver" };
export { workitem_id as "workitem-id" };
export { workitem_title as "workitem-title" };
