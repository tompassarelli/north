
export function Agent(id, name, status, task, provider, provider_target, provider_label, model, model_display, effort, orchestration_provenance, goal, state) {
  return Object.freeze({_tag: "Agent", id, name, status, task, provider, provider_target, provider_label, model, model_display, effort, orchestration_provenance, goal, state});
}

export function agent_id(r) { return r.id; }

export function agent_name(r) { return r.name; }

export function agent_status(r) { return r.status; }

export function agent_task(r) { return r.task; }

export function agent_provider(r) { return r.provider; }

export function agent_provider_target(r) { return r.provider_target; }

export function agent_provider_label(r) { return r.provider_label; }

export function agent_model(r) { return r.model; }

export function agent_model_display(r) { return r.model_display; }

export function agent_effort(r) { return r.effort; }

export function agent_orchestration_provenance(r) { return r.orchestration_provenance; }

export function agent_goal(r) { return r.goal; }

export function agent_state(r) { return r.state; }

export function WorkItem(id, title, body, condition, driver, dependencies) {
  return Object.freeze({_tag: "WorkItem", id, title, body, condition, driver, dependencies});
}

export function workitem_id(r) { return r.id; }

export function workitem_title(r) { return r.title; }

export function workitem_body(r) { return r.body; }

export function workitem_condition(r) { return r.condition; }

export function workitem_driver(r) { return r.driver; }

export function workitem_dependencies(r) { return r.dependencies; }

function BridgeModel(agents, list, board, layout, active_view_id, selected_agent, selected_thread, filter_text, notice) {
  return Object.freeze({_tag: "BridgeModel", agents, list, board, layout, active_view_id, selected_agent, selected_thread, filter_text, notice});
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

export function BridgeSnapshot(agents, list, board, layout, active_view_id, selected_agent, selected_thread, notice) {
  return Object.freeze({_tag: "BridgeSnapshot", agents, list, board, layout, active_view_id, selected_agent, selected_thread, notice});
}

export function bridgesnapshot_agents(r) { return r.agents; }

export function bridgesnapshot_list(r) { return r.list; }

export function bridgesnapshot_board(r) { return r.board; }

export function bridgesnapshot_layout(r) { return r.layout; }

export function bridgesnapshot_active_view_id(r) { return r.active_view_id; }

export function bridgesnapshot_selected_agent(r) { return r.selected_agent; }

export function bridgesnapshot_selected_thread(r) { return r.selected_thread; }

export function bridgesnapshot_notice(r) { return r.notice; }

function canonical_view(view_id) {
  return ((((_truthy) => _truthy !== false && _truthy != null)(((view_id === "board") || (view_id === "kanban")))) ? "board" : (((_truthy) => _truthy !== false && _truthy != null)(((view_id === "graph") || (view_id === "dag")))) ? "graph" : "list");
}

export function make_model(view_id) {
  const view = canonical_view(view_id);
  return BridgeModel([], [], [], "vertical", view, "", "", "", "");
}

export function replace_projection(model, agents, list_items, board_items) {
  return Object.freeze({...model, agents: agents, list: list_items, board: board_items});
}

export function upsert_agent(model, agent) {
  const agents = bridgemodel_agents(model).filter((existing) => (!(agent_id(existing) === agent_id(agent))));
  return Object.freeze({...model, agents: agents.concat(agent)});
}

export function remove_agent(model, removed_id) {
  return Object.freeze({...model, agents: bridgemodel_agents(model).filter((existing) => (!(agent_id(existing) === removed_id)))});
}

export function focus_view(model, view_id) {
  const view = canonical_view(view_id);
  return Object.freeze({...model, active_view_id: view, notice: ("".concat("view ", view))});
}

function set_layout(model, layout) {
  return Object.freeze({...model, layout: ((layout === "horizontal") ? "horizontal" : "vertical")});
}

export function select_agent(model, agent_id) {
  return Object.freeze({...model, selected_agent: agent_id, notice: ""});
}

export function select_thread(model, thread_id) {
  return Object.freeze({...model, selected_thread: thread_id, notice: ((thread_id === "") ? "no thread selected" : ("".concat("selected @", thread_id)))});
}

export function set_filter(model, filter_text) {
  const trimmed = filter_text.trim();
  return Object.freeze({...model, filter_text: trimmed.toLowerCase(), notice: ((trimmed === "") ? "filter cleared" : ("".concat("filter: ", filter_text)))});
}

function visible_work(model, source) {
  const agent_id = bridgemodel_selected_agent(model);
  const needle = bridgemodel_filter_text(model);
  return source.filter((item) => ((_logical) => (_logical !== false && _logical != null ? ((needle === "") || ("".concat(workitem_id(item), " ", workitem_title(item), " ", workitem_body(item), " ", workitem_condition(item), " ", workitem_driver(item))).toLowerCase().includes(needle)) : _logical))(((agent_id === "") || (workitem_driver(item) === agent_id))));
}

export function snapshot(model) {
  return BridgeSnapshot(bridgemodel_agents(model), visible_work(model, bridgemodel_list(model)), visible_work(model, bridgemodel_board(model)), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_notice(model));
}
