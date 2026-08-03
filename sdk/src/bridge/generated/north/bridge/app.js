import { BoxRenderable, createCliRenderer, InputRenderable, InputRenderableEvents, TextRenderable } from '@opentui/core';
import { append_transcript, focus_view, make_agent, make_model, make_work, replace_projection, select_agent, select_thread, set_filter, set_layout, snapshot, upsert_agent } from './model.js';

const NORTH_BIN = (process.env.NORTH_BIN || "north");

function text(value) {
  return ((typeof value === "string") ? value : "");
}

function bare(value) {
  const value_text = text(value);
  return (value_text.startsWith("@") ? value_text.slice(1) : value_text);
}

function error_message(error) {
  return ((error && (typeof error === "object") && error.message) ? text(error.message) : String(error));
}

function command(input) {
  const trimmed = input.trim();
  const normalized = (trimmed.startsWith("/") ? trimmed.slice(1) : trimmed);
  const split_at = normalized.indexOf(" ");
  return ((split_at < 0) ? {name: normalized.toLowerCase(), rest: ""} : {name: normalized.slice(0, split_at).toLowerCase(), rest: normalized.slice((split_at + 1)).trim()});
}

async function run_command(argv) {
  const child = Bun.spawn({cmd: argv, stdout: "pipe", stderr: "pipe"});
  const results = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
  const stdout = text(results[0]);
  const stderr = text(results[1]);
  const exit_code = results[2];
  return ((exit_code === 0) ? stdout : (() => { throw new Error(("".concat(argv.join(" "), " failed (", exit_code, "): ", (stderr.trim() || stdout.trim())))); })());
}

async function run_json(argv) {
  const output = await run_command(argv);
  return JSON.parse(output);
}

async function read_stream(stream, on_chunk) {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  return (async () => {  while (true) {
    const result = await reader.read(); if ((!result.done)) { on_chunk(decoder.decode(result.value, {stream: true}));  continue; } else { return null; }
  } })();
}

async function stream_command(argv, on_chunk) {
  const child = Bun.spawn({cmd: argv, stdout: "pipe", stderr: "pipe"});
  const results = await Promise.all([read_stream(child.stdout, on_chunk), read_stream(child.stderr, on_chunk), child.exited]);
  return results[2];
}

function normalize_agents(payload) {
  const rows = ((payload && Array.isArray(payload.agents)) ? payload.agents : []);
  return rows.map((row) => { const id = bare((row.control_id || row.uuid || row.id));
const name = text((row.display_handle || row.display_name || id));
const status = text((row.state_label || row.state || "unknown"));
const task = text((row.task || row.thread_title || ""));
return make_agent(id, name, status, task); });
}

function board_ids(board) {
  return (Array.isArray(board) ? board.map((row) => bare(row.id)).filter((id) => (!(id === ""))) : []);
}

function driver_of(facts, id) {
  if ((!Array.isArray(facts))) {
    return "";
  } else {
    const row = facts.find((fact) => ((text(fact.predicate) === "driver") && (bare(fact.subject) === id)));
    return (row ? bare(row.value) : "");
  }
}

function normalize_work(board, facts) {
  return ((!Array.isArray(board)) ? [] : board.map((row) => { const id = bare(row.id);
const title = text((row.title || id));
const condition = text((row.condition || "open"));
const emoji = text(row.emoji);
const driver = driver_of(facts, id);
const prefix = ((emoji === "") ? "" : ("".concat(emoji, " ")));
return make_work(id, title, condition, driver, ("".concat(prefix, "@", id, " ", title)).trim(), ("".concat(condition, " · ", prefix, "@", id, " ", title)).trim()); }));
}

function publish_line_bang(runtime, line) {
  if (((!runtime.disposed) && (!(line.trim() === "")))) {
    (runtime.model = append_transcript(runtime.model, line.trim()));
    return runtime.render();
  }
}

function destroy_bang(runtime) {
  if ((!runtime.disposed)) {
    (runtime.disposed = true);
    runtime.renderer.destroy();
    return process.exit(0);
  }
}

function install_global_exit_bang(runtime) {
  return runtime.renderer.keyInput.on("keypress", (key) => { const name = text(key.name);
if ((key.ctrl && ((name === "c") || (name === "q")))) {
  key.preventDefault();
  key.stopPropagation();
  return destroy_bang(runtime);
} });
}

async function refresh_bang(runtime) {
  return (async () => { try {
    const payloads = await Promise.all([run_json([NORTH_BIN, "agents", "--json"]), run_json([NORTH_BIN, "json", "board"])]);
  const agent_payload = payloads[0];
  const board = payloads[1];
  const ids = board_ids(board);
  const facts = ((ids.length > 0) ? await run_json([NORTH_BIN, "json", "show-many", ids.join(",")]) : []);
  const agents = normalize_agents(agent_payload);
  const work = normalize_work(board, facts);
  const kanban = work.slice().sort((left, right) => ("".concat(left.condition, ":", left.title)).localeCompare(("".concat(right.condition, ":", right.title))));
  (runtime.model = replace_projection(runtime.model, agents, work, kanban));
  return publish_line_bang(runtime, ("".concat("refreshed ", agents.length, " agents · ", work.length, " threads")));
  } catch (error) {
    return publish_line_bang(runtime, ("".concat("refresh failed: ", error_message(error))));
  } })();
}

function item_line(item) {
  return ((typeof item === "string") ? item : text(item.text));
}

function view_list(state) {
  return ((Array.isArray(state.views) && (state.views.length > 0)) ? state.views : [{id: "graph", title: "Threads DAG", items: (state.graph || [])}, {id: "kanban", title: "Kanban", items: (state.kanban || [])}]);
}

function selected_view(state, view_id) {
  const views = view_list(state);
  const selected = views.find((view) => (text(view.id) === view_id));
  return (selected || views[0]);
}

function roster_text(state, selected) {
  const agents = (state.agents || []);
  return ((agents.length === 0) ? "No agents attached" : agents.map((agent, index) => ("".concat(((index === selected) ? "› " : "  "), (text(agent.name) || text(agent.id)), ((text(agent.status) === "") ? "" : ("".concat(" (", text(agent.status), ")"))), ((text(agent.task) === "") ? "" : ("".concat(" — ", text(agent.task))))))).join("\n"));
}

function transcript_text(state) {
  const lines = (state.transcript || []);
  return ((lines.length === 0) ? "Waiting for agent output…" : lines.join("\n"));
}

function tabs_text(state, view_id) {
  return view_list(state).map((view) => { const selected = (text(view.id) === view_id);
return ("".concat((selected ? "[" : " "), (text(view.title) || text(view.id)), (selected ? "]" : " "))); }).join(" ");
}

function work_text(view, selected) {
  const items = (view.items || []);
  return ((items.length === 0) ? ("".concat("No ", (text(view.title) || "work"), " projection")) : items.map((item, index) => ("".concat(((index === selected) ? "› " : "  "), item_line(item)))).join("\n"));
}

function render_ui_bang(runtime, ui) {
  const state = snapshot(runtime.model);
  const agents = (state.agents || []);
  const views = view_list(state);
  const requested = text(state.activeViewId);
  const current = selected_view(state, requested);
  const items = (current.items || []);
  const agent_max = Math.max(0, (agents.length - 1));
  const work_max = Math.max(0, (items.length - 1));
  (runtime.agentIndex = Math.max(0, Math.min(runtime.agentIndex, agent_max)));
  (runtime.workIndex = Math.max(0, Math.min(runtime.workIndex, work_max)));
  (ui.root.flexDirection = ((text(state.layout) === "horizontal") ? "column" : "row"));
  (ui.agentsText.content = roster_text(state, runtime.agentIndex));
  (ui.transcriptText.content = transcript_text(state));
  (ui.tabsText.content = tabs_text(state, text(current.id)));
  (ui.workText.content = work_text(current, runtime.workIndex));
  (ui.statusText.content = ("".concat(text(state.notice), "\nTab pane · Ctrl+←/→ view · Ctrl+↑/↓ select · Ctrl+h/v split · Ctrl+o pop out · Ctrl+r refresh")));
  (ui.agentsPane.title = ((runtime.pane === "agents") ? "Agents · active" : "Agents"));
  (ui.workPane.title = ("".concat("Work", ((runtime.pane === "work") ? " · active · " : " · "), text(current.id))));
  (runtime.activeView = text(current.id));
  return views;
}

function parse_execution_bang(runtime, chunk) {
  const lines = ("".concat(runtime.streamBuffer, chunk)).split("\n");
  const remainder = lines.pop();
  (runtime.streamBuffer = remainder);
  return lines.forEach((raw_line) => { const line = raw_line.trim();
if (line.startsWith("execution ")) {
  const execution_id = line.slice(10).trim();
  runtime.bridgeExecutions.add(execution_id);
  return (runtime.model = upsert_agent(runtime.model, make_agent(execution_id, ("".concat("bridge ", execution_id.slice(0, 8))), "active", "Bridge execution")));
} });
}

async function launch_agent_bang(runtime, prompt) {
  if ((prompt.trim() === "")) {
    (() => { throw new Error("launch requires a prompt"); })();
  }
  publish_line_bang(runtime, ("".concat("launching: ", prompt)));
  const exit_code = await stream_command([NORTH_BIN, "bridge", prompt], (chunk) => { parse_execution_bang(runtime, chunk);
return publish_line_bang(runtime, chunk); });
  if ((!(exit_code === 0))) {
    publish_line_bang(runtime, ("".concat("bridge exited ", exit_code)));
  }
  return await refresh_bang(runtime);
}

function popout_bang(runtime, view_id) {
  const ghostty = Bun.which("ghostty");
  const kitty = Bun.which("kitty");
  const wezterm = Bun.which("wezterm");
  const foot = Bun.which("foot");
  const xterm = Bun.which("xterm");
  const argv = (ghostty) ? [ghostty, "-e", NORTH_BIN, "bridge", "app", "--view-id", view_id] : (kitty) ? [kitty, "--detach", NORTH_BIN, "bridge", "app", "--view-id", view_id] : (wezterm) ? [wezterm, "start", "--always-new-process", "--", NORTH_BIN, "bridge", "app", "--view-id", view_id] : (foot) ? [foot, NORTH_BIN, "bridge", "app", "--view-id", view_id] : (xterm) ? [xterm, "-e", NORTH_BIN, "bridge", "app", "--view-id", view_id] : null;
  if ((argv == null)) {
    (() => { throw new Error("no supported terminal found for pop-out"); })();
  }
  const child = Bun.spawn({cmd: argv, stdin: "ignore", stdout: "ignore", stderr: "ignore"});
  child.unref();
  return publish_line_bang(runtime, ("".concat("opened ", view_id, " in a separate terminal")));
}

function selected_work(runtime, selection) {
  const state = snapshot(runtime.model);
  const view_id = text((selection.view || runtime.activeView));
  const view = selected_view(state, view_id);
  const items = (view.items || []);
  const index = ((selection.index == null) ? -1 : selection.index);
  return (((index >= 0) && (index < items.length)) ? items[index] : null);
}

async function submit_agent_bang(runtime, input, selection) {
  const parsed = command(input);
  const name = text(parsed.name);
  const rest = text(parsed.rest);
  const target = text(selection);
  return ((name === "launch")) ? await launch_agent_bang(runtime, rest) : ((name === "refresh")) ? await refresh_bang(runtime) : ((name === "popout")) ? popout_bang(runtime, (rest || runtime.activeView)) : ((name === "help")) ? publish_line_bang(runtime, "agent commands: /launch <prompt>, /interrupt, /refresh, /popout; plain text steers the selected agent") : (async () => { if ((target === "")) {
  (() => { throw new Error("select an agent before steering or interrupting"); })();
}
if ((name === "interrupt")) {
  if ((!runtime.bridgeExecutions.has(target))) {
    (() => { throw new Error("interrupt is available for Bridge-launched executions"); })();
  }
  await run_command([NORTH_BIN, "bridge", "interrupt", target]);
  return publish_line_bang(runtime, ("".concat("interrupted ", target)));
} else {
  const message = ((name === "steer") ? rest : input.trim());
  if ((message === "")) {
    (() => { throw new Error("steer requires input"); })();
  }
  if (runtime.bridgeExecutions.has(target)) {
    await run_command([NORTH_BIN, "bridge", "steer", target, message]);
  } else {
    await run_command([NORTH_BIN, "steer", target, message]);
  }
  return publish_line_bang(runtime, ("".concat("steered ", target)));
} })();
}

async function submit_work_bang(runtime, input, selection) {
  const parsed = command(input);
  const name = text(parsed.name);
  const rest = text(parsed.rest);
  return ((name === "filter")) ? (() => { (runtime.model = set_filter(runtime.model, rest));
return runtime.render(); })() : ((name === "view")) ? (() => { (runtime.model = focus_view(runtime.model, rest));
return runtime.render(); })() : ((name === "split")) ? (() => { (runtime.model = set_layout(runtime.model, (((rest === "h") || (rest === "horizontal")) ? "horizontal" : "vertical")));
return runtime.render(); })() : ((name === "refresh")) ? await refresh_bang(runtime) : ((name === "popout")) ? popout_bang(runtime, ((rest === "") ? runtime.activeView : rest)) : ((name === "capture")) ? (async () => { if ((rest === "")) {
  (() => { throw new Error("capture requires a title"); })();
}
const output = await run_command([NORTH_BIN, "capture", rest]);
publish_line_bang(runtime, text(output).trim());
return await refresh_bang(runtime); })() : ((name === "assign")) ? (async () => { const item = selected_work(runtime, selection); const words = rest.split(" ").filter((word) => (!(word.trim() === ""))); const current = (item ? text(item.driver) : ""); const prior = ((words.length > 1) ? words[0] : current); const next_driver = ((words.length > 1) ? words[1] : (words[0] || "")); const thread_id = (item ? bare(item.id) : ""); if ((thread_id === "")) {
  (() => { throw new Error("select work before assigning"); })();
}
if ((prior === "")) {
  (() => { throw new Error("unassigned work requires: /assign <prior-driver> <next-driver>"); })();
}
if ((next_driver === "")) {
  (() => { throw new Error("assign requires a new driver"); })();
}
publish_line_bang(runtime, "driver reassignment is a retract-then-tell operation");
await run_command([NORTH_BIN, "retract", thread_id, "driver", prior]);
await run_command([NORTH_BIN, "tell", thread_id, "driver", next_driver]);
publish_line_bang(runtime, ("".concat("assigned @", thread_id, " to ", next_driver)));
return await refresh_bang(runtime); })() : ((name === "help")) ? publish_line_bang(runtime, "work commands: /capture <title>, /filter <text>, /assign <driver>, /view graph|kanban, /split h|v, /refresh, /popout") : (() => { throw new Error("unknown work command; use /help"); })();
}

function report_promise_bang(runtime, promise) {
  return promise.catch((error) => publish_line_bang(runtime, ("".concat("error: ", error_message(error)))));
}

function install_input_bang(runtime, ui) {
  ui.agentInput.on(InputRenderableEvents.ENTER, () => { const input = text(ui.agentInput.value).trim();
const state = snapshot(runtime.model);
const agents = (state.agents || []);
const selected = ((agents.length > 0) ? text(agents[runtime.agentIndex].id) : "");
if ((!(input === ""))) {
  (ui.agentInput.value = "");
  return report_promise_bang(runtime, submit_agent_bang(runtime, input, selected));
} });
  return ui.workInput.on(InputRenderableEvents.ENTER, () => { const input = text(ui.workInput.value).trim();
if ((!(input === ""))) {
  (ui.workInput.value = "");
  return report_promise_bang(runtime, submit_work_bang(runtime, input, {view: runtime.activeView, index: runtime.workIndex}));
} });
}

function install_keys_bang(runtime, ui) {
  return (ui.root.onKeyDown = (key) => { const name = text(key.name);
const ctrl = key.ctrl;
const state = snapshot(runtime.model);
if ((name === "tab")) {
  key.preventDefault();
  (runtime.pane = ((runtime.pane === "agents") ? "work" : "agents"));
  ((runtime.pane === "agents") ? ui.agentInput : ui.workInput).focus();
} else if ((ctrl && ((name === "left") || (name === "right")))) {
  const views = view_list(state);
  const current = selected_view(state, runtime.activeView);
  const index = views.findIndex((view) => (view.id === current.id));
  const delta = ((name === "left") ? -1 : 1);
  const next_index = ((index + delta + views.length) % views.length);
  const next_id = text(views[next_index].id);
  key.preventDefault();
  (runtime.model = focus_view(runtime.model, next_id));
  (runtime.workIndex = 0);
} else if ((ctrl && ((name === "up") || (name === "down")))) {
  const delta = ((name === "up") ? -1 : 1);
  key.preventDefault();
  if ((runtime.pane === "agents")) {
    const agents = (state.agents || []);
    const max_index = Math.max(0, (agents.length - 1));
    const next_index = Math.max(0, Math.min(max_index, (runtime.agentIndex + delta)));
    const agent_id = ((agents.length > 0) ? text(agents[next_index].id) : "");
    (runtime.agentIndex = next_index);
    (runtime.model = select_agent(runtime.model, agent_id));
  } else {
    const view = selected_view(state, runtime.activeView);
    const items = (view.items || []);
    const max_index = Math.max(0, (items.length - 1));
    const next_index = Math.max(0, Math.min(max_index, (runtime.workIndex + delta)));
    const thread_id = ((items.length > 0) ? bare(items[next_index].id) : "");
    (runtime.workIndex = next_index);
    (runtime.model = select_thread(runtime.model, thread_id));
  }
} else if ((ctrl && (name === "a"))) {
  key.preventDefault();
  (runtime.model = select_agent(runtime.model, ""));
} else if ((ctrl && ((name === "h") || (name === "v")))) {
  key.preventDefault();
  (runtime.model = set_layout(runtime.model, ((name === "h") ? "horizontal" : "vertical")));
} else if ((ctrl && (name === "o"))) {
  key.preventDefault();
  popout_bang(runtime, runtime.activeView);
} else if ((ctrl && (name === "r"))) {
  key.preventDefault();
  report_promise_bang(runtime, refresh_bang(runtime));
} else if ((ctrl && ((name === "c") || (name === "q")))) {
  key.preventDefault();
  destroy_bang(runtime);
} else {
  null;
}
if ((!runtime.disposed)) {
  return runtime.render();
} });
}

async function open_app_bang(view_id) {
  const renderer = await createCliRenderer({exitOnCtrlC: false, clearOnShutdown: true});
  const runtime = {model: make_model(view_id), renderer: renderer, disposed: false, pane: "agents", activeView: ((view_id === "kanban") ? "kanban" : "graph"), agentIndex: 0, workIndex: 0, bridgeExecutions: new Set(), streamBuffer: "", render: () => null};
  const root = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", height: "100%", gap: 1, padding: 1});
  const agents_pane = new BoxRenderable(renderer, {flexDirection: "column", flexGrow: 1, border: true, title: "Agents"});
  const work_pane = new BoxRenderable(renderer, {flexDirection: "column", flexGrow: 1, border: true, title: "Work · graph"});
  const agents_text = new TextRenderable(renderer, {flexGrow: 1, wrapMode: "word"});
  const transcript_text_view = new TextRenderable(renderer, {flexGrow: 3, wrapMode: "word"});
  const tabs_text_view = new TextRenderable(renderer, {wrapMode: "word"});
  const work_text_view = new TextRenderable(renderer, {flexGrow: 1, wrapMode: "word"});
  const status_text = new TextRenderable(renderer, {wrapMode: "word"});
  const agent_input = new InputRenderable(renderer, {placeholder: "/launch <prompt> or steer selected agent"});
  const work_input = new InputRenderable(renderer, {placeholder: "/capture, /filter, /assign, /view, /split, /popout"});
  const ui = {root: root, agentsPane: agents_pane, workPane: work_pane, agentsText: agents_text, transcriptText: transcript_text_view, tabsText: tabs_text_view, workText: work_text_view, statusText: status_text, agentInput: agent_input, workInput: work_input};
  agents_pane.add(agents_text);
  agents_pane.add(transcript_text_view);
  agents_pane.add(agent_input);
  work_pane.add(tabs_text_view);
  work_pane.add(work_text_view);
  work_pane.add(status_text);
  work_pane.add(work_input);
  root.add(agents_pane);
  root.add(work_pane);
  renderer.root.add(root);
  (runtime.render = () => render_ui_bang(runtime, ui));
  install_input_bang(runtime, ui);
  install_global_exit_bang(runtime);
  install_keys_bang(runtime, ui);
  runtime.render();
  agent_input.focus();
  renderer.start();
  report_promise_bang(runtime, refresh_bang(runtime));
  return runtime;
}

export function run_northbridge_app_bang(options) {
  return open_app_bang(text((options.viewId || "graph")));
}
