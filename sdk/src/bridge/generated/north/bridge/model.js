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

function TrackedThing(id, title, desired_outcome, agent, plan, project, task, assignee, assignee_title, status) {
  return $$bc$record_value("north.bridge.model/TrackedThing", {_tag: "TrackedThing", id, title, desired_outcome, agent, plan, project, task, assignee, assignee_title, status});
}

function trackedthing_id(r) { return r.id; }

function trackedthing_title(r) { return r.title; }

function trackedthing_desired_outcome(r) { return r.desired_outcome; }

function trackedthing_agent(r) { return r.agent; }

function trackedthing_plan(r) { return r.plan; }

function trackedthing_project(r) { return r.project; }

function trackedthing_task(r) { return r.task; }

function trackedthing_assignee(r) { return r.assignee; }

function trackedthing_assignee_title(r) { return r.assignee_title; }

function trackedthing_status(r) { return r.status; }

function BridgeModel(agents, list, board, tracked_things, store_space, store_version, layout, active_view_id, selected_agent, selected_tracked_thing, filter_text, notice) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {_tag: "BridgeModel", agents, list, board, tracked_things, store_space, store_version, layout, active_view_id, selected_agent, selected_tracked_thing, filter_text, notice});
}

function bridgemodel_agents(r) { return r.agents; }

function bridgemodel_list(r) { return r.list; }

function bridgemodel_board(r) { return r.board; }

function bridgemodel_tracked_things(r) { return r.tracked_things; }

function bridgemodel_store_space(r) { return r.store_space; }

function bridgemodel_store_version(r) { return r.store_version; }

function bridgemodel_layout(r) { return r.layout; }

function bridgemodel_active_view_id(r) { return r.active_view_id; }

function bridgemodel_selected_agent(r) { return r.selected_agent; }

function bridgemodel_selected_tracked_thing(r) { return r.selected_tracked_thing; }

function bridgemodel_filter_text(r) { return r.filter_text; }

function bridgemodel_notice(r) { return r.notice; }

function BridgeSnapshot(agents, list, board, tracked_things, semantic_agents, goals, all, store_space, store_version, layout, active_view_id, selected_agent, selected_tracked_thing, notice) {
  return $$bc$record_value("north.bridge.model/BridgeSnapshot", {_tag: "BridgeSnapshot", agents, list, board, tracked_things, semantic_agents, goals, all, store_space, store_version, layout, active_view_id, selected_agent, selected_tracked_thing, notice});
}

function bridgesnapshot_agents(r) { return r.agents; }

function bridgesnapshot_list(r) { return r.list; }

function bridgesnapshot_board(r) { return r.board; }

function bridgesnapshot_tracked_things(r) { return r.tracked_things; }

function bridgesnapshot_semantic_agents(r) { return r.semantic_agents; }

function bridgesnapshot_goals(r) { return r.goals; }

function bridgesnapshot_all(r) { return r.all; }

function bridgesnapshot_store_space(r) { return r.store_space; }

function bridgesnapshot_store_version(r) { return r.store_version; }

function bridgesnapshot_layout(r) { return r.layout; }

function bridgesnapshot_active_view_id(r) { return r.active_view_id; }

function bridgesnapshot_selected_agent(r) { return r.selected_agent; }

function bridgesnapshot_selected_tracked_thing(r) { return r.selected_tracked_thing; }

function bridgesnapshot_notice(r) { return r.notice; }

const TOP_LEVEL_VIEWS = ["agents", "goals", "all"];

function canonical_view(view_id) {
  return (((_truthy) => _truthy !== false && _truthy != null)(TOP_LEVEL_VIEWS.includes(view_id)) ? view_id : "agents");
}

function make_model(view_id) {
  const view = canonical_view(view_id);
  return BridgeModel([], [], [], [], "", 0, "vertical", view, "", "", "", "");
}

function replace_projection(model, agents, list_items, board_items) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, agents: agents, list: list_items, board: board_items});
}

function replace_catalog(model, tracked_things, store_space, store_version) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, tracked_things: tracked_things, store_space: store_space, store_version: store_version});
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

function select_tracked_thing(model, tracked_thing_id) {
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, selected_tracked_thing: tracked_thing_id, notice: ((tracked_thing_id === "") ? "no tracked thing selected" : $$bc$str("selected ", tracked_thing_id))});
}

function set_filter(model, filter_text) {
  const trimmed = filter_text.trim();
  return $$bc$record_value("north.bridge.model/BridgeModel", {...model, filter_text: trimmed.toLowerCase(), notice: ((trimmed === "") ? "filter cleared" : $$bc$str("filter: ", filter_text))});
}

function visible_work(model, source) {
  const needle = bridgemodel_filter_text(model);
  return source.filter((item) => ((needle === "") || $$bc$str(workitem_id(item), " ", workitem_title(item), " ", workitem_body(item), " ", workitem_condition(item), " ", workitem_driver(item)).toLowerCase().includes(needle)));
}

function optional_search_text(value) {
  return (((_truthy) => _truthy !== false && _truthy != null)(value) ? value : "");
}

function tracked_thing_search_text(item) {
  return $$bc$str(trackedthing_id(item), " ", trackedthing_title(item), " ", optional_search_text(trackedthing_desired_outcome(item)), " ", optional_search_text(trackedthing_assignee(item)), " ", optional_search_text(trackedthing_assignee_title(item)), " ", optional_search_text(trackedthing_status(item)));
}

function visible_tracked_things(model, source) {
  const needle = bridgemodel_filter_text(model);
  return source.filter((item) => ((needle === "") || tracked_thing_search_text(item).toLowerCase().includes(needle)));
}

function semantic_agent_p(item) {
  return trackedthing_agent(item);
}

function semantic_goal_p(item) {
  const outcome = trackedthing_desired_outcome(item);
  return (((_truthy) => _truthy !== false && _truthy != null)(outcome) ? true : false);
}

function snapshot(model) {
  const tracked_things = bridgemodel_tracked_things(model);
  const visible = visible_tracked_things(model, tracked_things);
  const semantic_agents = visible.filter(semantic_agent_p);
  const goals = visible.filter(semantic_goal_p);
  return BridgeSnapshot(bridgemodel_agents(model), visible_work(model, bridgemodel_list(model)), visible_work(model, bridgemodel_board(model)), tracked_things, semantic_agents, goals, visible, bridgemodel_store_space(model), bridgemodel_store_version(model), bridgemodel_layout(model), bridgemodel_active_view_id(model), bridgemodel_selected_agent(model), bridgemodel_selected_tracked_thing(model), bridgemodel_notice(model));
}

export { Agent as "->Agent" };
export { BridgeSnapshot as "->BridgeSnapshot" };
export { TrackedThing as "->TrackedThing" };
export { WorkItem as "->WorkItem" };
export { Agent as "Agent" };
export { BridgeSnapshot as "BridgeSnapshot" };
export { TrackedThing as "TrackedThing" };
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
export { bridgesnapshot_all as "bridgesnapshot-all" };
export { bridgesnapshot_board as "bridgesnapshot-board" };
export { bridgesnapshot_goals as "bridgesnapshot-goals" };
export { bridgesnapshot_layout as "bridgesnapshot-layout" };
export { bridgesnapshot_list as "bridgesnapshot-list" };
export { bridgesnapshot_notice as "bridgesnapshot-notice" };
export { bridgesnapshot_selected_agent as "bridgesnapshot-selected-agent" };
export { bridgesnapshot_selected_tracked_thing as "bridgesnapshot-selected-tracked-thing" };
export { bridgesnapshot_semantic_agents as "bridgesnapshot-semantic-agents" };
export { bridgesnapshot_store_space as "bridgesnapshot-store-space" };
export { bridgesnapshot_store_version as "bridgesnapshot-store-version" };
export { bridgesnapshot_tracked_things as "bridgesnapshot-tracked-things" };
export { focus_view as "focus-view" };
export { make_model as "make-model" };
export { remove_agent as "remove-agent" };
export { replace_catalog as "replace-catalog" };
export { replace_projection as "replace-projection" };
export { select_agent as "select-agent" };
export { select_tracked_thing as "select-tracked-thing" };
export { set_filter as "set-filter" };
export { snapshot as "snapshot" };
export { trackedthing_agent as "trackedthing-agent" };
export { trackedthing_assignee as "trackedthing-assignee" };
export { trackedthing_assignee_title as "trackedthing-assignee-title" };
export { trackedthing_desired_outcome as "trackedthing-desired-outcome" };
export { trackedthing_id as "trackedthing-id" };
export { trackedthing_plan as "trackedthing-plan" };
export { trackedthing_project as "trackedthing-project" };
export { trackedthing_status as "trackedthing-status" };
export { trackedthing_task as "trackedthing-task" };
export { trackedthing_title as "trackedthing-title" };
export { upsert_agent as "upsert-agent" };
export { workitem_body as "workitem-body" };
export { workitem_condition as "workitem-condition" };
export { workitem_dependencies as "workitem-dependencies" };
export { workitem_driver as "workitem-driver" };
export { workitem_id as "workitem-id" };
export { workitem_title as "workitem-title" };
