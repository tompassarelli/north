import { BoxRenderable, ScrollBoxRenderable, StyledText, brightBlack, brightCyan, brightGreen, brightRed, brightWhite, brightYellow, createCliRenderer, dim, InputRenderable, InputRenderableEvents, red, stripAnsiSequences, TextRenderable, white } from '@opentui/core';
import { focus_view, make_agent, make_model, make_work, replace_projection, select_agent, select_thread, set_filter, set_layout, snapshot, upsert_agent } from './model.js';

const NORTH_BIN = (process.env.NORTH_BIN || "north");

const SUPERVISOR_BOOT_PROMPT = "You are the Northbridge supervisor. Reply only READY, then wait for operator input.";

const SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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

function safe_json(value) {
  return (() => { try {
    return JSON.stringify(value, null, 2);
  } catch (__) {
    return String(value);
  } })();
}

function clean_text(value) {
  return stripAnsiSequences(text(value));
}

function clipped(value, limit) {
  const clean = clean_text(value);
  return ((clean.length > limit) ? ("".concat("…", clean.slice((clean.length - limit)))) : clean);
}

function next_item_id_bang(runtime, prefix) {
  (runtime.itemSequence = (runtime.itemSequence + 1));
  return ("".concat(prefix, ":", runtime.itemSequence));
}

function conversation_item(id, kind, title, body, status) {
  return {id: id, kind: kind, title: title, body: body, status: status};
}

function conversation_index(runtime, id) {
  return runtime.conversation.findIndex((item) => (text(item.id) === id));
}

function conversation_item_by_id(runtime, id) {
  const index = conversation_index(runtime, id);
  return ((index >= 0) ? runtime.conversation[index] : null);
}

function upsert_conversation_bang(runtime, item) {
  const index = conversation_index(runtime, text(item.id));
  if ((index >= 0)) {
    (runtime.conversation[index] = item);
  } else {
    runtime.conversation.push(item);
  }
  if ((runtime.conversation.length > 240)) {
    (runtime.conversation = runtime.conversation.slice(-240));
  }
  return runtime.render();
}

function append_system_bang(runtime, body) {
  if ((!(body.trim() === ""))) {
    return upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "system"), "system", "", body.trim(), "done"));
  }
}

function append_error_bang(runtime, body) {
  if ((!(body.trim() === ""))) {
    return upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "error"), "error", "Error", body.trim(), "failed"));
  }
}

function set_working_bang(runtime, working, label) {
  (runtime.working = working);
  (runtime.workingLabel = label);
  if (working) {
    if ((runtime.workingSince === 0)) {
      (runtime.workingSince = Date.now());
    }
    if ((runtime.spinnerTimer == null)) {
      const timer = setInterval(() => { if ((!runtime.disposed)) {
  (runtime.spinnerIndex = (runtime.spinnerIndex + 1));
  return runtime.render();
} }, 90);
      (runtime.spinnerTimer = timer);
      if (timer.unref) {
        timer.unref();
      }
    }
  } else {
    if (runtime.spinnerTimer) {
      clearInterval(runtime.spinnerTimer);
    }
    (runtime.spinnerTimer = null);
    (runtime.workingSince = 0);
  }
  return runtime.render();
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

async function stream_command(argv, on_stdout, on_stderr) {
  const child = Bun.spawn({cmd: argv, stdout: "pipe", stderr: "pipe"});
  const results = await Promise.all([read_stream(child.stdout, on_stdout), read_stream(child.stderr, on_stderr), child.exited]);
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
    return append_system_bang(runtime, line);
  }
}

function destroy_bang(runtime) {
  if ((!runtime.disposed)) {
    (runtime.disposed = true);
    if (runtime.spinnerTimer) {
      clearInterval(runtime.spinnerTimer);
    }
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
  const payloads = await Promise.all([run_json([NORTH_BIN, "agents", "--json"]).catch((__) => null), run_json([NORTH_BIN, "json", "board"]).catch((__) => null)]);
  const agent_payload = payloads[0];
  const board = payloads[1];
  const state = snapshot(runtime.model);
  const current_agents = (state.agents || []);
  const bridge_agents = current_agents.filter((agent) => runtime.bridgeExecutions.has(text(agent.id)));
  const remote_agents = (agent_payload ? normalize_agents(agent_payload) : []);
  const distinct_remote = remote_agents.filter((agent) => (!runtime.bridgeExecutions.has(text(agent.id))));
  const agents = (agent_payload ? bridge_agents.concat(distinct_remote) : current_agents);
  const ids = (board ? board_ids(board) : []);
  const facts = ((ids.length > 0) ? await run_json([NORTH_BIN, "json", "show-many", ids.join(",")]).catch((__) => []) : []);
  const work = (board ? normalize_work(board, facts) : (state.graph || []));
  const kanban = (board ? work.slice().sort((left, right) => ("".concat(left.condition, ":", left.title)).localeCompare(("".concat(right.condition, ":", right.title)))) : (state.kanban || []));
  (runtime.model = replace_projection(runtime.model, agents, work, kanban));
  return runtime.render();
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

function push_chunk_bang(chunks, chunk) {
  return chunks.push(chunk);
}

function render_conversation(runtime) {
  const chunks = [];
  const items = runtime.conversation;
  items.forEach((item) => { const kind = text(item.kind);
const title = text(item.title);
const body = clipped(item.body, 6000);
const status = text(item.status);
return ((kind === "user")) ? (() => { push_chunk_bang(chunks, brightCyan("❯ "));
push_chunk_bang(chunks, brightWhite(body));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "assistant")) ? (() => { push_chunk_bang(chunks, brightGreen("• "));
push_chunk_bang(chunks, brightWhite(body));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "command")) ? (() => { push_chunk_bang(chunks, (((status === "failed")) ? brightRed : ((status === "running")) ? brightYellow : brightGreen)("• "));
push_chunk_bang(chunks, brightWhite(("".concat(((status === "running") ? "Running " : "Ran "), title))));
if ((!(body === ""))) {
  push_chunk_bang(chunks, dim(("".concat("\n  └ ", body.replaceAll("\n", "\n    ")))));
}
return push_chunk_bang(chunks, white("\n\n")); })() : (((kind === "tool") || (kind === "change"))) ? (() => { push_chunk_bang(chunks, ((status === "failed") ? brightRed("• ") : brightGreen("• ")));
push_chunk_bang(chunks, brightWhite(title));
if ((!(body === ""))) {
  push_chunk_bang(chunks, dim(("".concat("\n  └ ", body.replaceAll("\n", "\n    ")))));
}
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "thought")) ? (() => { push_chunk_bang(chunks, brightBlack("• Explored "));
push_chunk_bang(chunks, dim(body));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "error")) ? (() => { push_chunk_bang(chunks, brightRed("• Error\n  "));
push_chunk_bang(chunks, red(body.replaceAll("\n", "\n  ")));
return push_chunk_bang(chunks, white("\n\n")); })() : (() => { push_chunk_bang(chunks, brightBlack(("".concat("• ", body))));
return push_chunk_bang(chunks, white("\n\n")); })(); });
  if (runtime.working) {
    const frame = SPINNER_FRAMES[(runtime.spinnerIndex % SPINNER_FRAMES.length)];
    const elapsed = Math.floor(((Date.now() - runtime.workingSince) / 1000));
    push_chunk_bang(chunks, brightYellow(("".concat(frame, " Working"))));
    push_chunk_bang(chunks, brightBlack(("".concat(" (", elapsed, "s · esc to interrupt)"))));
  }
  if (((items.length === 0) && (!runtime.working))) {
    push_chunk_bang(chunks, brightBlack("Starting Codex supervisor…"));
  }
  return new StyledText(chunks);
}

function render_status(runtime, state) {
  return (runtime.showHelp ? new StyledText([brightYellow("Northbridge keys\n"), brightWhite("F1"), brightBlack(" close help · "), brightWhite("F2"), brightBlack(" switch pane · "), brightWhite("F3"), brightBlack(" switch work view\n"), brightWhite("F4"), brightBlack(" toggle split · "), brightWhite("F5"), brightBlack(" refresh · "), brightWhite("F6"), brightBlack(" pop out\n"), brightWhite("Tab"), brightBlack(" switch pane · "), brightWhite("Esc"), brightBlack(" interrupt active turn · "), brightWhite("/help"), brightBlack(" commands")]) : new StyledText([brightBlack(("".concat(text(state.notice), "\n"))), brightCyan("F1"), brightBlack(" help · "), brightCyan("F2"), brightBlack(" pane · "), brightCyan("F3"), brightBlack(" view · "), brightCyan("F4"), brightBlack(" split · "), brightCyan("F5"), brightBlack(" refresh · "), brightCyan("F6"), brightBlack(" pop out")]));
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
  if ((text(state.layout) === "horizontal")) {
    (ui.agentsPane.width = "100%");
    (ui.workPane.width = "100%");
    (ui.agentsPane.height = "50%");
    (ui.workPane.height = "50%");
  } else {
    (ui.agentsPane.width = "38%");
    (ui.workPane.width = "62%");
    (ui.agentsPane.height = "100%");
    (ui.workPane.height = "100%");
  }
  (ui.agentsText.content = roster_text(state, runtime.agentIndex));
  (ui.transcriptText.content = render_conversation(runtime));
  (ui.tabsText.content = tabs_text(state, text(current.id)));
  (ui.workText.content = work_text(current, runtime.workIndex));
  (ui.statusText.content = render_status(runtime, state));
  (ui.agentsPane.title = ((runtime.pane === "agents") ? "Agents · active" : "Agents"));
  (ui.workPane.title = ("".concat("Work", ((runtime.pane === "work") ? " · active · " : " · "), text(current.id))));
  (ui.agentsPane.borderColor = ((runtime.pane === "agents") ? "#4ade80" : "#64748b"));
  (ui.workPane.borderColor = ((runtime.pane === "work") ? "#d97706" : "#64748b"));
  (runtime.activeView = text(current.id));
  return views;
}

function bridge_agent_bang(runtime, execution_id, role, status) {
  runtime.bridgeExecutions.add(execution_id);
  if ((role === "supervisor")) {
    (runtime.supervisorId = execution_id);
    (runtime.agentIndex = 0);
  }
  (runtime.model = upsert_agent(runtime.model, make_agent(execution_id, ((role === "supervisor") ? "Codex supervisor" : ("".concat("Codex ", execution_id.slice(0, 8)))), status, ((role === "supervisor") ? "Northbridge control session" : "Bridge execution"))));
  return runtime.render();
}

function record_line(line) {
  const close = line.indexOf("] ");
  if (((!line.startsWith("[")) || (close < 0))) {
    return null;
  } else {
    const rest = line.slice((close + 2));
    const space = rest.indexOf(" ");
    const kind = ((space < 0) ? rest : rest.slice(0, space));
    const payload = ((space < 0) ? "" : rest.slice((space + 1)).trim());
    return (() => { try {
    return {kind: kind, data: ((payload === "") ? {} : JSON.parse(payload))};
  } catch (__) {
    return null;
  } })();
  }
}

function event_item_id(execution_id, item_id) {
  return ("".concat(execution_id, ":", text(item_id)));
}

function append_item_delta_bang(runtime, id, kind, title, delta, status) {
  const existing = conversation_item_by_id(runtime, id);
  const prior = (existing ? text(existing.body) : "");
  return upsert_conversation_bang(runtime, conversation_item(id, kind, title, clipped(("".concat(prior, delta)), 6000), status));
}

function assistant_message_text(data) {
  const message = data.message;
  const content = (message ? message.content : null);
  return (Array.isArray(content) ? content.map((part) => text(part.text)).filter((part) => (!(part === ""))).join("\n") : text((data.text || data.result)));
}

function handle_codex_event_bang(runtime, stream_state, data) {
  const method = text(data.method);
  const params = (data.params || {});
  const execution_id = text(stream_state.executionId);
  return ((method === "turn/started")) ? (() => { set_working_bang(runtime, true, "Codex is working");
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "working");
} })() : ((method === "item/started")) ? (() => { const item = params.item; const kind = (item ? text(item.type) : ""); const id = event_item_id(execution_id, (item ? item.id : "item")); return ((kind === "commandExecution")) ? upsert_conversation_bang(runtime, conversation_item(id, "command", clean_text(item.command), "", "running")) : ((kind === "mcpToolCall")) ? upsert_conversation_bang(runtime, conversation_item(id, "tool", ("".concat("Called ", text(item.server), ".", text(item.tool))), safe_json(item.arguments), "running")) : ((kind === "fileChange")) ? upsert_conversation_bang(runtime, conversation_item(id, "change", "Editing files", "", "running")) : null; })() : ((method === "item/commandExecution/outputDelta")) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "command", "command", text(params.delta), "running") : ((method === "item/agentMessage/delta")) ? ((!stream_state.booting) ? (() => { return append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "assistant", "", text(params.delta), "running"); })() : null) : (((method === "item/reasoning/summaryTextDelta") || (method === "item/plan/delta"))) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "thought", "", text(params.delta), "running") : ((method === "item/fileChange/outputDelta")) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "change", "Editing files", text(params.delta), "running") : ((method === "item/mcpToolCall/progress")) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "tool", "Tool activity", text(params.message), "running") : ((method === "item/completed")) ? (() => { const item = params.item; const kind = (item ? text(item.type) : ""); const id = event_item_id(execution_id, (item ? item.id : "item")); return ((kind === "commandExecution")) ? upsert_conversation_bang(runtime, conversation_item(id, "command", clean_text(item.command), clipped(item.aggregatedOutput, 6000), (((text(item.status) === "completed") && (item.exitCode === 0)) ? "done" : "failed"))) : ((kind === "agentMessage")) ? (() => { const body = clean_text(item.text); (runtime.lastAssistantText = body);
if ((!stream_state.booting)) {
  return upsert_conversation_bang(runtime, conversation_item(id, "assistant", "", body, "done"));
} })() : ((kind === "mcpToolCall")) ? upsert_conversation_bang(runtime, conversation_item(id, "tool", ("".concat("Called ", text(item.server), ".", text(item.tool))), clipped(safe_json(item.result), 6000), ((text(item.status) === "failed") ? "failed" : "done"))) : ((kind === "fileChange")) ? upsert_conversation_bang(runtime, conversation_item(id, "change", "Edited files", clipped(safe_json(item.changes), 6000), ((text(item.status) === "failed") ? "failed" : "done"))) : (((kind === "webSearch") || (kind === "todoList"))) ? upsert_conversation_bang(runtime, conversation_item(id, "tool", kind, clipped(safe_json(item), 3000), "done")) : null; })() : ((method === "turn/completed")) ? (runtime.workingLabel = "Finishing") : null;
}

function handle_record_bang(runtime, stream_state, record) {
  const kind = text(record.kind);
  const data = (record.data || {});
  const execution_id = text(stream_state.executionId);
  return ((kind === "provider.starting")) ? ((!(execution_id === "")) ? (() => { return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "starting"); })() : null) : ((kind === "provider.codex.event")) ? handle_codex_event_bang(runtime, stream_state, data) : ((kind === "provider.assistant")) ? (() => { const body = assistant_message_text(data); if (((!stream_state.booting) && (!(body === "")) && (!(body === runtime.lastAssistantText)))) {
  (runtime.lastAssistantText = body);
  return upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "assistant"), "assistant", "", body, "done"));
} })() : ((kind === "provider.result")) ? set_working_bang(runtime, false, "") : ((kind === "session.idle")) ? (() => { set_working_bang(runtime, false, "");
(stream_state.booting = false);
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "ready");
} })() : ((kind.includes("failed") || kind.includes("error"))) ? (() => { set_working_bang(runtime, false, "");
return append_error_bang(runtime, ("".concat(kind, ": ", safe_json(data)))); })() : null;
}

function parse_bridge_stream_bang(runtime, stream_state, chunk) {
  const lines = ("".concat(stream_state.buffer, chunk)).split("\n");
  const remainder = lines.pop();
  (stream_state.buffer = remainder);
  return lines.forEach((raw_line) => { const line = raw_line.trim();
return (line.startsWith("execution ")) ? (() => { const execution_id = line.slice(10).trim(); (stream_state.executionId = execution_id);
return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "starting"); })() : (line.startsWith("[")) ? (() => { const record = record_line(line); if (record) {
  return handle_record_bang(runtime, stream_state, record);
} })() : (line.startsWith("north bridge:")) ? append_error_bang(runtime, line) : null; });
}

async function launch_agent_bang(runtime, prompt, role) {
  if ((prompt.trim() === "")) {
    (() => { throw new Error("launch requires a prompt"); })();
  }
  if ((!(role === "supervisor"))) {
    upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "user"), "user", "", prompt.trim(), "done"));
  }
  set_working_bang(runtime, true, "Starting Codex");
  const stream_state = {buffer: "", stderr: "", executionId: "", role: role, booting: (role === "supervisor")};
  const exit_code = await stream_command([NORTH_BIN, "bridge", prompt], (chunk) => parse_bridge_stream_bang(runtime, stream_state, chunk), (chunk) => (stream_state.stderr = clipped(("".concat(stream_state.stderr, chunk)), 6000)));
  if ((!(exit_code === 0))) {
    set_working_bang(runtime, false, "");
    return append_error_bang(runtime, ("".concat("Bridge exited ", exit_code, ((text(stream_state.stderr).trim() === "") ? "" : ("".concat("\n", text(stream_state.stderr).trim()))))));
  }
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
  const target = text((selection || runtime.supervisorId));
  return ((name === "launch")) ? await launch_agent_bang(runtime, rest, "worker") : ((name === "refresh")) ? await refresh_bang(runtime) : ((name === "popout")) ? popout_bang(runtime, (rest || runtime.activeView)) : ((name === "help")) ? (() => { (runtime.showHelp = (!runtime.showHelp));
return runtime.render(); })() : (async () => { if ((target === "")) {
  (() => { throw new Error("select an agent before steering or interrupting"); })();
}
if ((name === "interrupt")) {
  if ((!runtime.bridgeExecutions.has(target))) {
    (() => { throw new Error("interrupt is available for Bridge-launched executions"); })();
  }
  await run_command([NORTH_BIN, "bridge", "interrupt", target]);
  set_working_bang(runtime, false, "");
  return append_system_bang(runtime, "Turn interrupted");
} else {
  const message = ((name === "steer") ? rest : input.trim());
  if ((message === "")) {
    (() => { throw new Error("steer requires input"); })();
  }
  upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "user"), "user", "", message, "done"));
  set_working_bang(runtime, true, "Codex is working");
  if (runtime.bridgeExecutions.has(target)) {
    await run_command([NORTH_BIN, "bridge", "steer", target, message]);
  } else {
    await run_command([NORTH_BIN, "steer", target, message]);
  }
  return runtime.render();
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
  return runtime.renderer.keyInput.on("keypress", (key) => { const name = text(key.name).toLowerCase();
const state = snapshot(runtime.model);
const meta = (key.meta || key.option);
if (((name === "tab") || (name === "f2"))) {
  key.preventDefault();
  key.stopPropagation();
  (runtime.pane = ((runtime.pane === "agents") ? "work" : "agents"));
  ((runtime.pane === "agents") ? ui.agentInput : ui.workInput).focus();
} else if ((name === "f1")) {
  key.preventDefault();
  key.stopPropagation();
  (runtime.showHelp = (!runtime.showHelp));
} else if (((name === "f3") || (meta && ((name === "h") || (name === "l"))))) {
  const views = view_list(state);
  const current = selected_view(state, runtime.activeView);
  const index = views.findIndex((view) => (view.id === current.id));
  const delta = ((meta && (name === "h")) ? -1 : 1);
  const next_index = ((index + delta + views.length) % views.length);
  const next_id = text(views[next_index].id);
  key.preventDefault();
  key.stopPropagation();
  (runtime.model = focus_view(runtime.model, next_id));
  (runtime.workIndex = 0);
} else if (((name === "f4") || (meta && (name === "s")))) {
  key.preventDefault();
  key.stopPropagation();
  (runtime.model = set_layout(runtime.model, ((text(state.layout) === "horizontal") ? "vertical" : "horizontal")));
} else if (((name === "f5") || (key.ctrl && (name === "r")))) {
  key.preventDefault();
  key.stopPropagation();
  report_promise_bang(runtime, refresh_bang(runtime));
} else if (((name === "f6") || (key.ctrl && (name === "o")))) {
  key.preventDefault();
  key.stopPropagation();
  popout_bang(runtime, runtime.activeView);
} else if ((meta && ((name === "j") || (name === "k")))) {
  const delta = ((name === "k") ? -1 : 1);
  key.preventDefault();
  key.stopPropagation();
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
} else if (((name === "escape") || (name === "esc"))) {
  const target = text(runtime.supervisorId);
  if ((runtime.working && (!(target === "")))) {
    key.preventDefault();
    key.stopPropagation();
    report_promise_bang(runtime, submit_agent_bang(runtime, "/interrupt", target));
  }
} else {
  null;
}
if ((!runtime.disposed)) {
  return runtime.render();
} });
}

async function open_app_bang(view_id) {
  const renderer = await createCliRenderer({exitOnCtrlC: false, clearOnShutdown: true});
  const runtime = {model: make_model(view_id), renderer: renderer, disposed: false, pane: "agents", activeView: ((view_id === "kanban") ? "kanban" : "graph"), agentIndex: 0, workIndex: 0, bridgeExecutions: new Set(), supervisorId: "", conversation: [], itemSequence: 0, lastAssistantText: "", working: false, workingLabel: "", workingSince: 0, spinnerIndex: 0, spinnerTimer: null, showHelp: false, render: () => null};
  const root = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", height: "100%", gap: 1, padding: 1});
  const agents_pane = new BoxRenderable(renderer, {flexDirection: "column", width: "38%", border: true, title: "Agents"});
  const work_pane = new BoxRenderable(renderer, {flexDirection: "column", flexGrow: 1, border: true, title: "Work · graph"});
  const agents_text = new TextRenderable(renderer, {height: 4, flexShrink: 0, wrapMode: "word"});
  const transcript_scroll = new ScrollBoxRenderable(renderer, {flexGrow: 1, scrollY: true, stickyScroll: true, stickyStart: "bottom", viewportCulling: true});
  const transcript_text_view = new TextRenderable(renderer, {width: "100%", flexShrink: 0, wrapMode: "word"});
  const tabs_text_view = new TextRenderable(renderer, {wrapMode: "word"});
  const work_text_view = new TextRenderable(renderer, {flexGrow: 1, wrapMode: "word"});
  const status_text = new TextRenderable(renderer, {wrapMode: "word"});
  const agent_input = new InputRenderable(renderer, {placeholder: "Message Codex supervisor…"});
  const work_input = new InputRenderable(renderer, {placeholder: "/capture, /filter, /assign, /view, /split, /popout"});
  const ui = {root: root, agentsPane: agents_pane, workPane: work_pane, agentsText: agents_text, transcriptText: transcript_text_view, tabsText: tabs_text_view, workText: work_text_view, statusText: status_text, agentInput: agent_input, workInput: work_input};
  agents_pane.add(agents_text);
  transcript_scroll.add(transcript_text_view);
  agents_pane.add(transcript_scroll);
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
  report_promise_bang(runtime, launch_agent_bang(runtime, SUPERVISOR_BOOT_PROMPT, "supervisor"));
  return runtime;
}

export function run_northbridge_app_bang(options) {
  return open_app_bang(text((options.viewId || "graph")));
}
