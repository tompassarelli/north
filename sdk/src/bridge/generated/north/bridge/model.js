
function Agent(id, name, status, task) {
  return Object.freeze({_tag: "Agent", id, name, status, task});
}

function agent_id(r) { return r.id; }

function agent_name(r) { return r.name; }

function agent_status(r) { return r.status; }

function agent_task(r) { return r.task; }

function WorkItem(id, title, body, condition, driver, dependencies) {
  return Object.freeze({_tag: "WorkItem", id, title, body, condition, driver, dependencies});
}

function workitem_id(r) { return r.id; }

function workitem_title(r) { return r.title; }

function workitem_body(r) { return r.body; }

function workitem_condition(r) { return r.condition; }

function workitem_driver(r) { return r.driver; }

function workitem_dependencies(r) { return r.dependencies; }

function BridgeModel(agents, transcript, list, kanban, layout, active_view_id, selected_agent, selected_thread, filter_text, notice) {
  return Object.freeze({_tag: "BridgeModel", agents, transcript, list, kanban, layout, active_view_id, selected_agent, selected_thread, filter_text, notice});
}

function bridgemodel_agents(r) { return r.agents; }

function bridgemodel_transcript(r) { return r.transcript; }

function bridgemodel_list(r) { return r.list; }

function bridgemodel_kanban(r) { return r.kanban; }

function bridgemodel_layout(r) { return r.layout; }

function bridgemodel_active_view_id(r) { return r.active_view_id; }

function bridgemodel_selected_agent(r) { return r.selected_agent; }

function bridgemodel_selected_thread(r) { return r.selected_thread; }

function bridgemodel_filter_text(r) { return r.filter_text; }

function bridgemodel_notice(r) { return r.notice; }

export function make_agent(id, name, status, task) {
  return Agent(id, name, status, task);
}

export function make_work(id, title, body, condition, driver, dependencies) {
  return WorkItem(id, title, body, condition, driver, dependencies);
}

function canonical_view(view_id) {
  return ((view_id === "kanban")) ? "kanban" : ((view_id === "dag")) ? "dag" : "list";
}

export function make_model(view_id) {
  const view = canonical_view(view_id);
  return BridgeModel([], [], [], [], "vertical", view, "", "", "", ("".concat("view ", view)));
}

export function replace_projection(model, agents, list_items, kanban) {
  return BridgeModel(agents, bridgemodel_transcript(model), list_items, kanban, bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_filter_text(model), bridgemodel_notice(model));
}

export function upsert_agent(model, agent) {
  const agents = bridgemodel_agents(model).filter((existing) => (!(agent_id(existing) === agent_id(agent))));
  return BridgeModel(agents.concat(agent), bridgemodel_transcript(model), bridgemodel_list(model), bridgemodel_kanban(model), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_filter_text(model), bridgemodel_notice(model));
}

function append_transcript(model, line) {
  return BridgeModel(bridgemodel_agents(model), bridgemodel_transcript(model).concat(line).slice(-500), bridgemodel_list(model), bridgemodel_kanban(model), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_filter_text(model), line);
}

export function focus_view(model, view_id) {
  const view = canonical_view(view_id);
  return BridgeModel(bridgemodel_agents(model), bridgemodel_transcript(model), bridgemodel_list(model), bridgemodel_kanban(model), bridgemodel_layout(model), view, bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_filter_text(model), ("".concat("view ", view)));
}

export function set_layout(model, layout) {
  return BridgeModel(bridgemodel_agents(model), bridgemodel_transcript(model), bridgemodel_list(model), bridgemodel_kanban(model), ((layout === "horizontal") ? "horizontal" : "vertical"), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), bridgemodel_filter_text(model), bridgemodel_notice(model));
}

export function select_agent(model, agent_id) {
  return BridgeModel(bridgemodel_agents(model), bridgemodel_transcript(model), bridgemodel_list(model), bridgemodel_kanban(model), bridgemodel_layout(model), bridgemodel_active_view_id(model), agent_id, bridgemodel_selected_thread(model), bridgemodel_filter_text(model), ((agent_id === "") ? "showing all work" : ("".concat("work assigned to ", agent_id))));
}

export function select_thread(model, thread_id) {
  return BridgeModel(bridgemodel_agents(model), bridgemodel_transcript(model), bridgemodel_list(model), bridgemodel_kanban(model), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), thread_id, bridgemodel_filter_text(model), ((thread_id === "") ? "no thread selected" : ("".concat("selected @", thread_id))));
}

export function set_filter(model, filter_text) {
  return BridgeModel(bridgemodel_agents(model), bridgemodel_transcript(model), bridgemodel_list(model), bridgemodel_kanban(model), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_thread(model), filter_text.trim().toLowerCase(), ((filter_text.trim() === "") ? "filter cleared" : ("".concat("filter: ", filter_text))));
}

function visible_work(model, source) {
  const agent_id = bridgemodel_selected_agent(model);
  const needle = bridgemodel_filter_text(model);
  return source.filter((item) => (((agent_id === "") || (workitem_driver(item) === agent_id)) && ((needle === "") || ("".concat(workitem_id(item), " ", workitem_title(item), " ", workitem_body(item), " ", workitem_condition(item), " ", workitem_driver(item))).toLowerCase().includes(needle))));
}

export function snapshot(model) {
  const list_items = visible_work(model, bridgemodel_list(model));
  const kanban = visible_work(model, bridgemodel_kanban(model));
  return {agents: bridgemodel_agents(model), transcript: bridgemodel_transcript(model), list: list_items, kanban: kanban, views: [{id: "list", title: "List", items: list_items}, {id: "dag", title: "DAG", items: list_items}, {id: "kanban", title: "Kanban", items: kanban}], layout: bridgemodel_layout(model), activeViewId: bridgemodel_active_view_id(model), selectedAgent: bridgemodel_selected_agent(model), selectedThread: bridgemodel_selected_thread(model), notice: bridgemodel_notice(model)};
}
