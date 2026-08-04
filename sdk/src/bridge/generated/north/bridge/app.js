import { BoxRenderable, ScrollBoxRenderable, StyledText, bg, brightBlack, brightCyan, brightGreen, brightRed, brightWhite, brightYellow, createCliRenderer, dim, InputRenderable, InputRenderableEvents, red, stripAnsiSequences, TextRenderable, white } from '@opentui/core';
import { registerEmacsBindings, registerEscapeClearsPendingSequence } from '@opentui/keymap/addons';
import { createDefaultOpenTuiKeymap } from '@opentui/keymap/opentui';
import { focus_view, make_agent, make_model, make_work, replace_projection, select_agent, select_thread, set_filter, set_layout, snapshot, upsert_agent } from './model.js';

const IntlSegmenter = Intl.Segmenter;

function SlashCommand(name, description, arguments$, completion, emoji) {
  return Object.freeze({_tag: "SlashCommand", name, description, arguments: arguments$, completion, emoji});
}

function slashcommand_name(r) { return r.name; }

function slashcommand_description(r) { return r.description; }

function slashcommand_arguments(r) { return r.arguments; }

function slashcommand_completion(r) { return r.completion; }

function slashcommand_emoji(r) { return r.emoji; }

const NORTH_BIN = (process.env.NORTH_BIN || "north");

const SUPERVISOR_BOOT_PROMPT = "You are the Northbridge supervisor. Reply only READY, then wait for operator input.";

const BOARD_LANES = [{id: "not-started", title: "Not Started"}, {id: "in-progress", title: "In Progress"}, {id: "done", title: "Done"}];

const DEFAULT_PROMPT_GLYPH = "❯";

const SOUND_COOLDOWN_MS = 700;

const SOUND_PACKS = {peon: {ready: ["PeonReady1.ogg"], done: ["PeonYes1.ogg", "PeonYes2.ogg", "PeonYes3.ogg", "PeonYes4.ogg"], interrupted: ["PeonWhat1.ogg", "PeonWhat3.ogg", "PeonWhat4.ogg"], failed: ["PeonAngry1.ogg", "PeonAngry4.ogg"]}, peasant: {ready: ["PeasantReady1.mp3"], done: ["PeasantYes2.mp3", "PeasantYes4.mp3", "PeasantYesAttack4.mp3"], interrupted: ["PeasantWhat1.mp3"], failed: ["PeasantAngry1.mp3"]}};

const EMOJI_COMMANDS = [SlashCommand("😀", "grinning face · happy smile", false, "😀", true), SlashCommand("😄", "smiling face · happy cheerful", false, "😄", true), SlashCommand("😂", "tears of joy · laugh funny", false, "😂", true), SlashCommand("😊", "warm smile · pleased blush", false, "😊", true), SlashCommand("😎", "cool face · sunglasses", false, "😎", true), SlashCommand("🤔", "thinking face · consider question", false, "🤔", true), SlashCommand("😅", "relieved smile · nervous sweat", false, "😅", true), SlashCommand("😭", "crying face · sad tears", false, "😭", true), SlashCommand("😡", "angry face · mad upset", false, "😡", true), SlashCommand("😤", "frustrated face · annoyed", false, "😤", true), SlashCommand("🥳", "celebration face · party", false, "🥳", true), SlashCommand("🤯", "mind blown · surprised", false, "🤯", true), SlashCommand("🫡", "salute · acknowledged", false, "🫡", true), SlashCommand("👋", "wave · hello goodbye", false, "👋", true), SlashCommand("👍", "thumbs up · approve yes", false, "👍", true), SlashCommand("👎", "thumbs down · reject no", false, "👎", true), SlashCommand("🙏", "thanks · please gratitude", false, "🙏", true), SlashCommand("💪", "strength · effort strong", false, "💪", true), SlashCommand("👀", "eyes · look review", false, "👀", true), SlashCommand("🎉", "party popper · celebrate success", false, "🎉", true), SlashCommand("❤️", "heart · love favorite", false, "❤️", true), SlashCommand("🔥", "fire · hot excellent", false, "🔥", true), SlashCommand("✨", "sparkles · magic polish", false, "✨", true), SlashCommand("🚀", "rocket · launch ship", false, "🚀", true), SlashCommand("💡", "light bulb · idea insight", false, "💡", true), SlashCommand("✅", "done · complete success check", false, "✅", true), SlashCommand("❌", "failed · error cross", false, "❌", true), SlashCommand("⚠️", "warning · caution attention", false, "⚠️", true), SlashCommand("🐛", "bug · defect debug", false, "🐛", true), SlashCommand("🔧", "wrench · fix repair tool", false, "🔧", true), SlashCommand("🧪", "test tube · test experiment", false, "🧪", true), SlashCommand("📌", "pin · important remember", false, "📌", true), SlashCommand("📝", "note · write document", false, "📝", true), SlashCommand("⏳", "waiting · hourglass pending", false, "⏳", true), SlashCommand("🔒", "lock · secure private", false, "🔒", true), SlashCommand("🔓", "unlock · open access", false, "🔓", true), SlashCommand("📦", "package · bundle release", false, "📦", true), SlashCommand("🧭", "compass · navigate direction", false, "🧭", true), SlashCommand("🔊", "speaker · sound volume", false, "🔊", true), SlashCommand("🔇", "muted speaker · quiet silence", false, "🔇", true), SlashCommand("❯", "prompt · leader chevron", false, "❯", true), SlashCommand("→", "right arrow · next forward", false, "→", true), SlashCommand("←", "left arrow · back previous", false, "←", true), SlashCommand("↑", "up arrow · increase", false, "↑", true), SlashCommand("↓", "down arrow · decrease", false, "↓", true), SlashCommand("★", "star · favorite important", false, "★", true), SlashCommand("•", "bullet · list point", false, "•", true), SlashCommand("✓", "check · yes done", false, "✓", true), SlashCommand("✗", "cross · no failed", false, "✗", true)];

const AGENT_COMMANDS = [SlashCommand("/launch", "start another Codex worker", true, "", false), SlashCommand("/steer", "steer the selected agent", true, "", false), SlashCommand("/interrupt", "interrupt the active agent turn", false, "", false), SlashCommand("/refresh", "refresh agents and work", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/exit", "close Northbridge", false, "", false), SlashCommand("/help", "show Northbridge controls", false, "", false)];

const WORK_COMMANDS = [SlashCommand("/capture", "capture a new work thread", true, "", false), SlashCommand("/filter", "filter visible work", true, "", false), SlashCommand("/assign", "reassign the selected work", true, "", false), SlashCommand("/outcome", "record a selected thread outcome", true, "", false), SlashCommand("/view", "switch List, Graph, or Board view", true, "", false), SlashCommand("/split", "switch horizontal or vertical layout", true, "", false), SlashCommand("/refresh", "refresh agents and work", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/exit", "close Northbridge", false, "", false), SlashCommand("/help", "show work commands", false, "", false)];

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

function conversation_item_with_data_bang(id, kind, title, body, status, data) {
  const item = conversation_item(id, kind, title, body, status);
  (item.data = data);
  return item;
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

function append_interrupted_bang(runtime) {
  return upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "interrupted"), "interrupted", "", "Conversation interrupted — tell the model what to do differently.", "interrupted"));
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

function exit_command_p(name) {
  return ((name === "exit") || (name === "close") || (name === "esc"));
}

function emoji_options(query) {
  const needle = query.trim().toLowerCase();
  return EMOJI_COMMANDS.filter((candidate) => ((needle === "") || ("".concat(slashcommand_name(candidate), " ", slashcommand_description(candidate))).toLowerCase().includes(needle))).slice(0, 8);
}

function palette_options(pane, input) {
  const query = input.trim().toLowerCase();
  const parsed = command(input);
  const name = text(parsed.name);
  const commands = ((pane === "agents") ? AGENT_COMMANDS : WORK_COMMANDS);
  return ((!query.startsWith("/"))) ? [] : ((name === "emoji")) ? emoji_options(text(parsed.rest)) : ((query.indexOf(" ") >= 0)) ? [] : commands.filter((candidate) => slashcommand_name(candidate).startsWith(query)).slice(0, 8);
}

function submit_key_p(name) {
  return ((name === "return") || (name === "enter") || (name === "kpenter") || (name === "linefeed"));
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

async function discover_session_branch_bang(runtime) {
  await (async () => { try {
    const directory = text(runtime.sessionCwd);
  const output = await run_command(["git", "-C", directory, "branch", "--show-current"]);
  const branch = text(output).trim();
  return (runtime.sessionBranch = ((branch === "") ? "detached HEAD" : branch));
  } catch (__) {
    return (runtime.sessionBranch = "not a Git worktree");
  } })();
  return runtime.render();
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

function dependencies_of(facts, id) {
  return ((!Array.isArray(facts)) ? [] : facts.filter((fact) => ((text(fact.predicate) === "depends_on") && (bare(fact.subject) === id))).map((fact) => bare(fact.value)));
}

function body_of(facts, id) {
  if ((!Array.isArray(facts))) {
    return "";
  } else {
    const row = facts.find((fact) => ((text(fact.predicate) === "body") && (bare(fact.subject) === id)));
    return (row ? text(row.value) : "");
  }
}

function normalize_work(board, facts) {
  return ((!Array.isArray(board)) ? [] : board.map((row) => { const id = bare(row.id);
const title = text((row.title || id));
const body = body_of(facts, id);
const condition = text((row.condition || "open"));
const driver = driver_of(facts, id);
const dependencies = dependencies_of(facts, id);
return make_work(id, title, body, condition, driver, dependencies); }));
}

function terminal_condition_p(condition) {
  return ((condition === "terminal") || (condition === "done") || (condition === "completed") || (condition === "failed"));
}

function board_lane_id(condition) {
  return ((condition === "active")) ? "in-progress" : (terminal_condition_p(condition)) ? "done" : "not-started";
}

function ordered_board_items(open_items, done_items) {
  const ready = open_items.filter((item) => (text(item.condition) === "ready"));
  const waiting = open_items.filter((item) => { const condition = text(item.condition);
return ((!(condition === "ready")) && (!(condition === "active"))); });
  const active = open_items.filter((item) => (text(item.condition) === "active"));
  return ready.concat(waiting, active, done_items);
}

function publish_line_bang(runtime, line) {
  if (((!runtime.disposed) && (!(line.trim() === "")))) {
    return append_system_bang(runtime, line);
  }
}

function grapheme_count(value) {
  return Array.from(new IntlSegmenter("en", {granularity: "grapheme"}).segment(value)).length;
}

function current_prompt_glyph(runtime) {
  const glyph = text(runtime.promptGlyph);
  return ((glyph === "") ? DEFAULT_PROMPT_GLYPH : glyph);
}

function render_prompt_bang(runtime, prompt) {
  const glyph = current_prompt_glyph(runtime);
  (prompt.width = (Bun.stringWidth(glyph) + 1));
  return (prompt.content = new StyledText([brightCyan(("".concat(glyph, " ")))]));
}

function set_prompt_glyph_bang(runtime, glyph) {
  if (((glyph.trim() === "") || (!(grapheme_count(glyph) === 1)))) {
    (() => { throw new Error("glyph requires exactly one grapheme, or use /glyph reset"); })();
  }
  (runtime.promptGlyph = glyph);
  publish_line_bang(runtime, ("".concat("prompt glyph set to ", glyph)));
  return runtime.render();
}

function sound_enabled_from_env(value) {
  const normalized = value.trim().toLowerCase();
  return (!((normalized === "0") || (normalized === "false") || (normalized === "off") || (normalized === "no")));
}

function sound_pack_from_env(value) {
  return ((value.trim().toLowerCase() === "peasant") ? "peasant" : "peon");
}

function sound_directory_from_env(value) {
  const directory = value.trim();
  const home = text(process.env.HOME).trim();
  return ((!(directory === ""))) ? directory : ((!(home === ""))) ? ("".concat(home, "/code/north/warcraft-sounds")) : "warcraft-sounds";
}

function discover_sound_player() {
  const mpv = Bun.which("mpv");
  const ffplay = Bun.which("ffplay");
  const pw_play = Bun.which("pw-play");
  return (mpv) ? {kind: "mpv", path: text(mpv)} : (ffplay) ? {kind: "ffplay", path: text(ffplay)} : (pw_play) ? {kind: "pw-play", path: text(pw_play)} : null;
}

function sound_warning_bang(runtime, message) {
  if (((!runtime.disposed) && (!runtime.soundWarningShown))) {
    (runtime.soundWarningShown = true);
    return publish_line_bang(runtime, ("".concat("sound warning: ", message)));
  }
}

function sound_path(directory, filename) {
  return ("".concat(directory, (directory.endsWith("/") ? "" : "/"), filename));
}

function sound_argv(player, path) {
  const executable = text(player.path);
  return ((text(player.kind) === "mpv")) ? [executable, "--no-video", "--really-quiet", path] : ((text(player.kind) === "ffplay")) ? [executable, "-nodisp", "-autoexit", "-loglevel", "quiet", path] : [executable, path];
}

function spawn_sound_bang(runtime, path) {
  return (() => { try {
    const player = runtime.soundPlayer;
  if ((player == null)) {
    return sound_warning_bang(runtime, "install mpv, ffplay, or pw-play to play local assets");
  } else {
    const child = Bun.spawn({cmd: sound_argv(player, path), stdin: "ignore", stdout: "ignore", stderr: "ignore"});
    runtime.soundChildren.add(child);
    child.unref();
    return child.exited.then((exit_code) => { runtime.soundChildren.delete(child);
if ((!(exit_code === 0))) {
  return sound_warning_bang(runtime, ("".concat("player exited ", exit_code, " for ", path)));
} }).catch((error) => { runtime.soundChildren.delete(child);
return sound_warning_bang(runtime, error_message(error)); });
  }
  } catch (error) {
    return sound_warning_bang(runtime, error_message(error));
  } })();
}

function play_sound_path_bang(runtime, path) {
  return (() => { try {
    return Bun.file(path).exists().then((exists) => { if ((exists && runtime.soundEnabled && (!runtime.disposed))) {
  return spawn_sound_bang(runtime, path);
} else {
  if (((!exists) && runtime.soundEnabled)) {
    return sound_warning_bang(runtime, ("".concat("missing local asset ", path)));
  }
} }).catch((error) => sound_warning_bang(runtime, error_message(error)));
  } catch (error) {
    return sound_warning_bang(runtime, error_message(error));
  } })();
}

function select_sound_path_bang(runtime, event) {
  const pack = ((text(runtime.soundPack) === "peasant") ? SOUND_PACKS.peasant : SOUND_PACKS.peon);
  const files = ((event === "ready")) ? pack.ready : ((event === "done")) ? pack.done : ((event === "interrupted")) ? pack.interrupted : ((event === "failed")) ? pack.failed : [];
  const count = files.length;
  if ((count === 0)) {
    return "";
  } else {
    const base_index = (runtime.soundSequence % count);
    const base_file = text(files[base_index]);
    const base_path = sound_path(text(runtime.soundDirectory), base_file);
    const index = (((count > 1) && (base_path === text(runtime.lastSoundPath))) ? ((base_index + 1) % count) : base_index);
    const path = sound_path(text(runtime.soundDirectory), text(files[index]));
    (runtime.soundSequence = (runtime.soundSequence + 1));
    (runtime.lastSoundPath = path);
    return path;
  }
}

function play_sound_event_bang(runtime, stream_state, event) {
  const now = Date.now();
  if ((runtime.soundEnabled && stream_state.soundLive && ((now - runtime.lastSoundAt) >= SOUND_COOLDOWN_MS))) {
    const path = select_sound_path_bang(runtime, event);
    if ((!(path === ""))) {
      (runtime.lastSoundAt = now);
      return play_sound_path_bang(runtime, path);
    }
  }
}

function sound_status(runtime) {
  const player = runtime.soundPlayer;
  return ("".concat("sound ", (runtime.soundEnabled ? "on" : "off"), " · pack ", text(runtime.soundPack), " · player ", (player ? text(player.kind) : "none"), " · ", text(runtime.soundDirectory)));
}

function handle_sound_command_bang(runtime, rest) {
  const request = rest.trim().toLowerCase();
  return (((request === "") || (request === "status"))) ? publish_line_bang(runtime, sound_status(runtime)) : ((request === "on")) ? (() => { (runtime.soundEnabled = true);
publish_line_bang(runtime, sound_status(runtime));
if ((runtime.soundPlayer == null)) {
  return sound_warning_bang(runtime, "install mpv, ffplay, or pw-play to play local assets");
} })() : ((request === "off")) ? (() => { (runtime.soundEnabled = false);
return publish_line_bang(runtime, sound_status(runtime)); })() : (request.startsWith("pack ")) ? (() => { const pack = request.slice(5).trim(); if ((!((pack === "peon") || (pack === "peasant")))) {
  (() => { throw new Error("sound pack must be peon or peasant"); })();
}
(runtime.soundPack = pack);
return publish_line_bang(runtime, sound_status(runtime)); })() : (() => { throw new Error("sound requires on, off, status, or pack peon|peasant"); })();
}

function handle_local_command_bang(runtime, ui, input) {
  const trimmed = input.trim();
  if ((!trimmed.startsWith("/"))) {
    return false;
  } else {
    const parsed = command(trimmed);
    const name = text(parsed.name);
    const rest = text(parsed.rest);
    return (((name === "glyph") || (name === "prompt"))) ? (() => { if ((rest.toLowerCase() === "reset")) {
  set_prompt_glyph_bang(runtime, DEFAULT_PROMPT_GLYPH);
} else {
  set_prompt_glyph_bang(runtime, rest);
}
return true; })() : ((name === "emoji")) ? (() => { const options = emoji_options(rest); if ((options.length === 0)) {
  (() => { throw new Error(("".concat("no emoji matches ", rest))); })();
}
const input_renderable = ((runtime.pane === "agents") ? ui.agentInput : ui.workInput);
(input_renderable.value = slashcommand_completion(options[0]));
input_renderable.focus();
runtime.render();
return true; })() : ((name === "sound")) ? (() => { handle_sound_command_bang(runtime, rest);
return true; })() : ((name === "mute")) ? (() => { (runtime.soundEnabled = false);
publish_line_bang(runtime, sound_status(runtime));
return true; })() : false;
  }
}

function destroy_bang(runtime) {
  if ((!runtime.disposed)) {
    (runtime.disposed = true);
    if (runtime.spinnerTimer) {
      clearInterval(runtime.spinnerTimer);
    }
    runtime.soundChildren.forEach((child) => (() => { try {
    return child.kill();
  } catch (__) {
    return null;
  } })());
    runtime.soundChildren.clear();
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
  const payloads = await Promise.all([run_json([NORTH_BIN, "agents", "--json"]).catch((__) => null), run_json([NORTH_BIN, "json", "board", "--all"]).catch((__) => null), run_json([NORTH_BIN, "json", "done"]).catch((__) => null)]);
  const agent_payload = payloads[0];
  const board = payloads[1];
  const done = payloads[2];
  const state = snapshot(runtime.model);
  const current_agents = (state.agents || []);
  const bridge_agents = current_agents.filter((agent) => runtime.bridgeExecutions.has(text(agent.id)));
  const remote_agents = (agent_payload ? normalize_agents(agent_payload) : []);
  const distinct_remote = remote_agents.filter((agent) => (!runtime.bridgeExecutions.has(text(agent.id))));
  const agents = (agent_payload ? bridge_agents.concat(distinct_remote) : current_agents);
  const open_rows = (Array.isArray(board) ? board : []);
  const done_rows = (Array.isArray(done) ? done : []);
  const ids = board_ids(open_rows).concat(board_ids(done_rows));
  const facts = ((ids.length > 0) ? await run_json([NORTH_BIN, "json", "show-many", ids.join(",")]).catch((__) => []) : []);
  const work = (Array.isArray(board) ? normalize_work(open_rows, facts) : (state.list || []));
  const prior_terminal = (state.kanban || []).filter((item) => terminal_condition_p(text(item.condition)));
  const terminal_work = (Array.isArray(done) ? normalize_work(done_rows, facts) : prior_terminal);
  const kanban = ordered_board_items(work, terminal_work);
  const next_model = replace_projection(runtime.model, agents, work, kanban);
  const selected_id = bare(state.selectedThread);
  const next_state = snapshot(next_model);
  const next_view = selected_view(next_state, runtime.activeView);
  const next_items = (next_view.items || []);
  const next_index = next_items.findIndex((item) => (bare(item.id) === selected_id));
  (runtime.model = next_model);
  if ((next_index >= 0)) {
    (runtime.workIndex = next_index);
  }
  return runtime.render();
}

function canonical_work_view(view_id) {
  return (((view_id === "graph") || (view_id === "dag"))) ? "graph" : (((view_id === "board") || (view_id === "kanban"))) ? "board" : "list";
}

function model_work_view(view_id) {
  const view = canonical_work_view(view_id);
  return ((view === "graph")) ? "dag" : ((view === "board")) ? "kanban" : "list";
}

function recognized_work_view_p(view_id) {
  return ((view_id === "list") || (view_id === "graph") || (view_id === "board") || (view_id === "dag") || (view_id === "kanban"));
}

function view_list(state) {
  return [{id: "list", title: "List", items: (state.list || [])}, {id: "graph", title: "Graph", items: (state.list || [])}, {id: "board", title: "Board", items: (state.kanban || [])}];
}

function selected_view(state, view_id) {
  const views = view_list(state);
  const canonical = canonical_work_view(view_id);
  const selected = views.find((view) => (text(view.id) === canonical));
  return (selected || views[0]);
}

function roster_text(state, selected) {
  const agents = (state.agents || []);
  return ((agents.length === 0) ? "No agents attached" : agents.map((agent, index) => ("".concat(((index === selected) ? "› " : "  "), (text(agent.name) || text(agent.id)), ((text(agent.status) === "") ? "" : ("".concat(" (", text(agent.status), ")"))), ((text(agent.task) === "") ? "" : ("".concat(" — ", text(agent.task))))))).join("\n"));
}

function push_chunk_bang(chunks, chunk) {
  return chunks.push(chunk);
}

function render_command_palette_bang(commands, selected) {
  const chunks = [];
  commands.forEach((candidate, index) => { push_chunk_bang(chunks, ((index === selected) ? brightCyan("› ") : brightBlack("  ")));
push_chunk_bang(chunks, (((index === selected) ? brightGreen : brightWhite))(slashcommand_name(candidate).padEnd(13, " ")));
push_chunk_bang(chunks, brightBlack(slashcommand_description(candidate)));
if ((index < (commands.length - 1))) {
  return push_chunk_bang(chunks, white("\n"));
} });
  return new StyledText(chunks);
}

function terminal_columns() {
  const stdout = process.stdout;
  const columns = (stdout ? stdout.columns : null);
  return Math.max(1, ((typeof columns === "number") ? columns : 120));
}

function terminal_rows() {
  const stdout = process.stdout;
  const rows = (stdout ? stdout.rows : null);
  return Math.max(1, ((typeof rows === "number") ? rows : 40));
}

function stacked_workspace_p(state) {
  return (text(state.layout) === "horizontal");
}

function pane_visible_p(runtime, pane) {
  return (!(text(runtime.closedPane) === pane));
}

function workspace_primary_cells(runtime, ui, state) {
  const stacked_p = stacked_workspace_p(state);
  const measured = (stacked_p ? ui.root.height : ui.root.width);
  const fallback = (stacked_p ? terminal_rows() : terminal_columns());
  const outer = (((typeof measured === "number") && Number.isFinite(measured) && (measured > 0)) ? measured : fallback);
  return Math.max(2, (Math.floor(outer) - 2));
}

function minimum_pane_cells(state, total) {
  return Math.max(1, Math.min((stacked_workspace_p(state) ? 5 : 20), Math.floor((total / 2))));
}

function clamp_pane_ratio_bang(runtime, ui, state) {
  const total = workspace_primary_cells(runtime, ui, state);
  const minimum = minimum_pane_cells(state, total);
  const raw = Number((runtime.paneRatio || 50));
  const ratio = (Number.isFinite(raw) ? raw : 50);
  const desired = Math.round((total * (ratio / 100)));
  const agents_cells = Math.max(minimum, Math.min((total - minimum), desired));
  const clamped = (100 * (agents_cells / total));
  (runtime.paneRatio = clamped);
  return clamped;
}

function apply_workspace_geometry_bang(runtime, ui, state) {
  const stacked_p = stacked_workspace_p(state);
  const both_p = (text(runtime.closedPane) === "");
  const ratio = (both_p ? clamp_pane_ratio_bang(runtime, ui, state) : 50);
  const agents_primary = (both_p ? ("".concat(ratio, "%")) : "100%");
  const work_primary = (both_p ? ("".concat((100 - ratio), "%")) : "100%");
  (ui.agentsPane.visible = pane_visible_p(runtime, "agents"));
  (ui.workPane.visible = pane_visible_p(runtime, "work"));
  (ui.root.flexDirection = (stacked_p ? "column" : "row"));
  if (stacked_p) {
    (ui.agentsPane.width = "100%");
    (ui.workPane.width = "100%");
    (ui.agentsPane.height = agents_primary);
    (ui.workPane.height = work_primary);
    return (ui.agentsPane.border = (both_p ? ["bottom"] : false));
  } else {
    (ui.agentsPane.width = agents_primary);
    (ui.workPane.width = work_primary);
    (ui.agentsPane.height = "100%");
    (ui.workPane.height = "100%");
    return (ui.agentsPane.border = (both_p ? ["right"] : false));
  }
}

function available_agent_width(runtime) {
  const columns = terminal_columns();
  const state = snapshot(runtime.model);
  const horizontal_p = stacked_workspace_p(state);
  const work_closed_p = (text(runtime.closedPane) === "work");
  const ratio = ((runtime.paneRatio || 50) / 100);
  return Math.max(24, ((horizontal_p || work_closed_p) ? (columns - 6) : (Math.floor(((columns - 5) * ratio)) - 3)));
}

function user_block_text(runtime, body) {
  const width = available_agent_width(runtime);
  const lines = body.split("\n");
  return lines.map((line, index) => { const prefix = ((index === 0) ? "❯ " : "  ");
return ("".concat(prefix, line)).padEnd(width, " "); }).join("\n");
}

function short_directory(directory) {
  const path = text(directory);
  const home = text(process.env.HOME);
  return (((!(home === "")) && path.startsWith(home)) ? ("".concat("~", path.slice(home.length))) : path);
}

function session_context_text(runtime) {
  return ("".concat((text(runtime.sessionModel) || "model pending"), " ", (text(runtime.sessionEffort) || "effort pending"), " · ", (short_directory(runtime.sessionCwd) || "directory pending"), " · ", (text(runtime.sessionBranch) || "branch pending")));
}

function command_parts(title) {
  const space = title.indexOf(" ");
  return ((space < 0) ? {executable: title, arguments: ""} : {executable: title.slice(0, space), arguments: title.slice((space + 1))});
}

function push_command_output_bang(chunks, output) {
  const trimmed = output.trimEnd();
  if ((!(trimmed === ""))) {
    const lines = trimmed.split("\n");
    const limit = 8;
    const overflow = Math.max(0, (lines.length - limit));
    const visible = lines.slice(0, ((overflow > 0) ? (limit - 1) : limit));
    visible.forEach((line, index) => { const last_p = ((overflow === 0) && (index === (visible.length - 1)));
return push_chunk_bang(chunks, dim(("".concat("\n  ", (last_p ? "└ " : "│ "), line)))); });
    if ((overflow > 0)) {
      return push_chunk_bang(chunks, dim(("".concat("\n  └ … +", (overflow + 1), " lines"))));
    }
  }
}

function push_command_card_bang(chunks, title, body, status) {
  const parts = command_parts(title);
  const running_p = (status === "running");
  push_chunk_bang(chunks, (((status === "failed")) ? brightRed : (running_p) ? brightYellow : brightGreen)("• "));
  push_chunk_bang(chunks, brightWhite((running_p ? "Running " : "Ran ")));
  push_chunk_bang(chunks, brightCyan(text(parts.executable)));
  if ((!(text(parts.arguments) === ""))) {
    push_chunk_bang(chunks, dim(("".concat(" ", text(parts.arguments)))));
  }
  push_command_output_bang(chunks, body);
  return push_chunk_bang(chunks, white("\n\n"));
}

function diff_start_line(token) {
  const value = text(token);
  const without_sign = ((value.length > 0) ? value.slice(1) : "0");
  const comma = without_sign.indexOf(",");
  const number_text = ((comma < 0) ? without_sign : without_sign.slice(0, comma));
  const parsed = Number(number_text);
  return (Number.isFinite(parsed) ? parsed : 0);
}

function diff_rows(diff) {
  const source = clean_text(diff);
  return ((source === "") ? {oldLine: 0, newLine: 0, additions: 0, deletions: 0, rows: []} : source.split("\n").reduce((state, line) => { if (line.startsWith("@@ ")) {
  const parts = line.split(" ");
  (state.oldLine = diff_start_line(parts[1]));
  (state.newLine = diff_start_line(parts[2]));
  state.rows.push({kind: "hunk", old: "", new: "", text: line});
} else if ((line.startsWith("diff --git ") || line.startsWith("index ") || line.startsWith("--- ") || line.startsWith("+++ "))) {
  null;
} else if (line.startsWith("+")) {
  state.rows.push({kind: "add", old: "", new: state.newLine, text: line});
  (state.newLine = (state.newLine + 1));
  (state.additions = (state.additions + 1));
} else if (line.startsWith("-")) {
  state.rows.push({kind: "delete", old: state.oldLine, new: "", text: line});
  (state.oldLine = (state.oldLine + 1));
  (state.deletions = (state.deletions + 1));
} else if (line.startsWith("\\ No newline")) {
  state.rows.push({kind: "meta", old: "", new: "", text: line});
} else {
  state.rows.push({kind: "context", old: state.oldLine, new: state.newLine, text: line});
  (state.oldLine = (state.oldLine + 1));
  (state.newLine = (state.newLine + 1));
}
return state; }, {oldLine: 0, newLine: 0, additions: 0, deletions: 0, rows: []}));
}

function file_change_details(change) {
  const parsed = diff_rows(change.diff);
  return {path: text(change.path), kind: text(change.kind), additions: parsed.additions, deletions: parsed.deletions, rows: parsed.rows};
}

function file_change_summary(changes) {
  return changes.reduce((summary, change) => { const details = file_change_details(change);
(summary.additions = (summary.additions + details.additions));
(summary.deletions = (summary.deletions + details.deletions));
summary.files.push(details);
return summary; }, {additions: 0, deletions: 0, files: []});
}

function diff_line_number(value) {
  const number_text = text(value);
  return number_text.padStart(4, " ");
}

function push_diff_rows_bang(chunks, rows, width) {
  const limit = 36;
  const visible = rows.slice(0, limit);
  const overflow = Math.max(0, (rows.length - limit));
  visible.forEach((row) => { const kind = text(row.kind);
const numbers = ("".concat(diff_line_number(row.old), " ", diff_line_number(row.new), " │ "));
const line = compact_text(("".concat(numbers, text(row.text))), width);
push_chunk_bang(chunks, white("\n"));
return push_chunk_bang(chunks, ((kind === "add")) ? (bg("#173326"))(brightGreen(line.padEnd(width, " "))) : ((kind === "delete")) ? (bg("#382127"))(brightRed(line.padEnd(width, " "))) : ((kind === "hunk")) ? brightCyan(line) : dim(line)); });
  if ((overflow > 0)) {
    return push_chunk_bang(chunks, dim(("".concat("\n          └ … +", overflow, " diff lines"))));
  }
}

function push_file_change_card_bang(chunks, item, status, runtime) {
  const data = item.data;
  const changes = ((data && Array.isArray(data.changes)) ? data.changes : []);
  const summary = file_change_summary(changes);
  const files = summary.files;
  const width = Math.max(24, (available_agent_width(runtime) - 2));
  push_chunk_bang(chunks, ((status === "failed") ? brightRed("• ") : brightGreen("• ")));
  push_chunk_bang(chunks, brightWhite(("".concat(((status === "running") ? "Editing " : "Edited "), files.length, ((files.length === 1) ? " file" : " files"), " (+", summary.additions, " -", summary.deletions, ")"))));
  files.forEach((file, index) => { const last_p = (index === (files.length - 1));
push_chunk_bang(chunks, brightBlack(("".concat("\n  ", (last_p ? "└ " : "├ "), text(file.path), " (+", file.additions, " -", file.deletions, ")"))));
if ((file.rows.length > 0)) {
  return push_diff_rows_bang(chunks, file.rows, width);
} });
  return push_chunk_bang(chunks, white("\n\n"));
}

function push_working_wave_bang(chunks, runtime) {
  const letters = "Working".split("");
  const cursor = (runtime.spinnerIndex % letters.length);
  push_chunk_bang(chunks, brightBlack("• "));
  return letters.forEach((letter, index) => { const phase = (((index - cursor) + letters.length) % letters.length);
return push_chunk_bang(chunks, ((phase === 0)) ? brightYellow(letter) : (((phase === 1) || (phase === 6))) ? brightWhite(letter) : (((phase === 2) || (phase === 5))) ? white(letter) : brightBlack(letter)); });
}

function render_conversation_bang(runtime) {
  const chunks = [];
  const items = runtime.conversation;
  items.forEach((item) => { const kind = text(item.kind);
const title = text(item.title);
const body = clipped(item.body, 6000);
const status = text(item.status);
return ((kind === "user")) ? (() => { push_chunk_bang(chunks, (bg("#292c32"))(brightWhite(user_block_text(runtime, body))));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "assistant")) ? (() => { push_chunk_bang(chunks, brightGreen("• "));
push_chunk_bang(chunks, brightWhite(body));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "command")) ? push_command_card_bang(chunks, title, body, status) : ((kind === "interrupted")) ? (() => { push_chunk_bang(chunks, brightRed("■ Conversation interrupted"));
push_chunk_bang(chunks, red(" — tell the model what to do differently."));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "change")) ? push_file_change_card_bang(chunks, item, status, runtime) : ((kind === "tool")) ? (() => { push_chunk_bang(chunks, ((status === "failed") ? brightRed("• ") : brightGreen("• ")));
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
    const elapsed = Math.floor(((Date.now() - runtime.workingSince) / 1000));
    push_working_wave_bang(chunks, runtime);
    push_chunk_bang(chunks, brightBlack(("".concat(" (", elapsed, "s · esc to interrupt)\n  "))));
    push_chunk_bang(chunks, brightBlack(session_context_text(runtime)));
  }
  if (((items.length === 0) && (!runtime.working))) {
    push_chunk_bang(chunks, brightBlack("Starting Codex supervisor…"));
  }
  return new StyledText(chunks);
}

function visible_notice(notice) {
  const value = text(notice);
  return ((value === "view dag")) ? "view graph" : ((value === "view kanban")) ? "view board" : value;
}

function render_status(runtime, state) {
  const count_text = text(runtime.windowCount);
  const workspace_notice = text(runtime.workspaceNotice);
  const notice = ((workspace_notice === "") ? visible_notice(state.notice) : workspace_notice);
  return (runtime.windowChord) ? new StyledText([brightGreen("Ctrl-w"), brightYellow(((count_text === "") ? "" : ("".concat(" · count ", count_text)))), brightBlack("  h/j/k/l neighbor · w cycle · v side-by-side · s stacked · c close · = equalize · >/< resize · Esc cancel")]) : (runtime.showHelp) ? new StyledText([brightYellow("Northbridge keys\n"), brightWhite("F1"), brightBlack(" close help · "), brightWhite("F2"), brightBlack(" switch pane · "), brightWhite("F3"), brightBlack(" switch work view\n"), brightWhite("F4"), brightBlack(" toggle split · "), brightWhite("F5"), brightBlack(" refresh · "), brightWhite("F6"), brightBlack(" pop out\n"), brightWhite("Tab"), brightBlack(" switch pane · "), brightWhite("Ctrl-w h/j/k/l"), brightBlack(" neighbor · "), brightWhite("w"), brightBlack(" cycle · "), brightWhite("v/s"), brightBlack(" side-by-side/stacked · "), brightWhite("c"), brightBlack(" close · "), brightWhite("="), brightBlack(" equalize · "), brightWhite("[count] >/<"), brightBlack(" resize\n"), brightWhite("Esc"), brightBlack(" interrupt active turn · "), brightWhite("/help"), brightBlack(" commands · "), brightWhite("/close|/esc|/exit"), brightBlack(" close\n"), brightWhite("/glyph <one>|reset"), brightBlack(" prompt · "), brightWhite("/emoji <query>"), brightBlack(" picker\n"), brightWhite("/sound on|off|pack"), brightBlack(" voice lines · "), brightWhite("/mute"), brightBlack(" quiet")]) : new StyledText([brightBlack(("".concat(notice, "\n"))), brightCyan("F1"), brightBlack(" help · "), brightCyan("F2"), brightBlack(" pane · "), brightCyan("F3"), brightBlack(" view · "), brightCyan("F4"), brightBlack(" split · "), brightCyan("F5"), brightBlack(" refresh · "), brightCyan("F6"), brightBlack(" pop out\n"), brightCyan("Ctrl-w"), brightBlack(" h/j/k/l neighbor · w cycle · v/s split · c close · = equalize · [count] >/< resize")]);
}

function render_pane_header(title, __focused_p) {
  return new StyledText([brightGreen(title)]);
}

function render_work_tabs_bang(state, view_id, __focused_p) {
  const chunks = [];
  const views = view_list(state);
  push_chunk_bang(chunks, brightGreen("Work  "));
  views.forEach((view, index) => { const selected_p = (text(view.id) === view_id);
const title = (text(view.title) || text(view.id));
push_chunk_bang(chunks, ((selected_p ? brightCyan : brightBlack))(("".concat((selected_p ? "[" : " "), title, (selected_p ? "]" : " ")))));
if ((index < (views.length - 1))) {
  return push_chunk_bang(chunks, white("  "));
} });
  return new StyledText(chunks);
}

function compact_text(value, width) {
  const source = text(value);
  const limit = Math.max(1, width);
  return ((source.length > limit) ? ("".concat(source.slice(0, Math.max(0, (limit - 1))), "…")) : source);
}

function cell_text(value, width) {
  return compact_text(value, width).padEnd(width, " ");
}

function short_thread_id(item) {
  return bare(item.id).slice(0, 8);
}

function push_condition_bang(chunks, condition, label) {
  return push_chunk_bang(chunks, (((condition === "active")) ? brightCyan : ((condition === "ready")) ? brightGreen : ((condition === "blocked")) ? brightRed : brightYellow)(label));
}

function available_work_width(runtime, state) {
  const columns = terminal_columns();
  const agents_closed_p = (text(runtime.closedPane) === "agents");
  const ratio = ((runtime.paneRatio || 50) / 100);
  return Math.max(24, ((stacked_workspace_p(state) || agents_closed_p) ? (columns - 6) : (Math.floor(((columns - 5) * (1 - ratio))) - 3)));
}

function render_list_view_bang(items, selected, width) {
  const chunks = [];
  items.forEach((item, index) => { const condition = text((item.condition || "open"));
const title_width = Math.max(10, (width - 23));
const title = compact_text(item.title, title_width);
const selected_p = (index === selected);
push_chunk_bang(chunks, (selected_p ? brightCyan("› ") : brightBlack("  ")));
push_condition_bang(chunks, condition, cell_text(condition.toUpperCase(), 9));
push_chunk_bang(chunks, ((selected_p ? brightWhite : white))(title));
push_chunk_bang(chunks, dim(("".concat("  @", short_thread_id(item)))));
if ((index < (items.length - 1))) {
  return push_chunk_bang(chunks, white("\n"));
} });
  return new StyledText(chunks);
}

function work_item_by_id(items, id) {
  return items.find((item) => (bare(item.id) === id));
}

function render_dag_view_bang(items, selected, width) {
  const chunks = [];
  items.forEach((item, index) => { const condition = text((item.condition || "open"));
const dependencies = (item.dependencies || []);
const selected_p = (index === selected);
push_chunk_bang(chunks, (selected_p ? brightCyan("› ") : brightBlack("  ")));
push_condition_bang(chunks, condition, "● ");
push_chunk_bang(chunks, ((selected_p ? brightWhite : white))(compact_text(item.title, Math.max(12, (width - 16)))));
push_chunk_bang(chunks, dim(("".concat("  @", short_thread_id(item), "\n"))));
if ((dependencies.length === 0)) {
  push_chunk_bang(chunks, brightBlack("    ╰─ root\n"));
} else {
  dependencies.forEach((dependency) => { const target = work_item_by_id(items, dependency);
push_chunk_bang(chunks, brightBlack("    ╰─ requires ← "));
push_chunk_bang(chunks, ((target ? brightCyan : brightBlack))(("".concat("@", dependency.slice(0, 8)))));
return push_chunk_bang(chunks, (target ? dim(("".concat("  ", compact_text(target.title, Math.max(8, (width - 28))), "\n"))) : brightBlack("  outside current board\n"))); });
}
if ((index < (items.length - 1))) {
  return push_chunk_bang(chunks, white("\n"));
} });
  return new StyledText(chunks);
}

function board_lane_items(items, lane_id) {
  return items.filter((item) => (board_lane_id(text(item.condition)) === lane_id));
}

function compact_body(value, width) {
  const lines = text(value).split("\n").map((line) => line.trim()).filter((line) => (!(line === "")));
  return compact_text(lines.join(" "), width);
}

function board_card_id(thread_id) {
  return ("".concat("board-card-", thread_id));
}

function board_signature(items, selected, width) {
  return ("".concat(width, "|", selected, "|", items.map((item) => ("".concat(bare(item.id), "\x01", text(item.title), "\x01", text(item.body), "\x01", text(item.condition)))).join("\x02")));
}

function board_card_node(source) {
  return (() => { let node = source; while (true) {
    if (((!node) || (!(text(node.northThreadId) === "")))) { return node; } else { const _recur_0 = node.parent; node = _recur_0; continue; }
  } })();
}

function set_board_notice_bang(runtime, notice) {
  (runtime.workspaceNotice = notice);
  return runtime.render();
}

function select_board_card_bang(runtime, ui, item, index) {
  (runtime.workIndex = index);
  (runtime.model = select_thread(runtime.model, bare(item.id)));
  focus_pane_surface_bang(runtime, ui, "work");
  return ui.workScroll.scrollChildIntoView(board_card_id(bare(item.id)));
}

function prefill_outcome_bang(runtime, ui, thread_id) {
  (ui.workInput.value = ("".concat("/outcome @", thread_id, " ")));
  focus_pane_bang(runtime, ui, "work");
  return set_board_notice_bang(runtime, ("".concat("Finish the outcome, then press Enter; Done is derived from north tell @", thread_id, " outcome <result>.")));
}

async function move_ready_thread_bang(runtime, thread_id, position, anchor_id) {
  const argv = ((anchor_id === "") ? [NORTH_BIN, "queue", "move", thread_id, position] : [NORTH_BIN, "queue", "move", thread_id, position, anchor_id]);
  const output = await run_command(argv);
  (runtime.workspaceNotice = text(output).trim());
  return await refresh_bang(runtime);
}

function unsupported_drop_notice(source_condition, target_lane) {
  return (terminal_condition_p(source_condition)) ? "Done is derived from an outcome; reopening requires explicitly retracting that outcome." : ((target_lane === "in-progress")) ? "In Progress is derived from a live run; dispatch or /launch the thread to start it." : ((source_condition === "active")) ? "Active work is derived from its live run; /interrupt and settle that run before moving it." : ((!(source_condition === "ready"))) ? "Only ready work has a durable queue order; resolve its prerequisites before reordering it." : "That lane is derived from lifecycle facts; use the corresponding North lifecycle action.";
}

function handle_board_drop_bang(runtime, ui, target_lane, target_card, event) {
  event.preventDefault();
  event.stopPropagation();
  ui.workScroll.stopAutoScroll();
  const source_card = board_card_node(event.source);
  const source_id = (source_card ? bare(source_card.northThreadId) : "");
  const state = snapshot(runtime.model);
  const items = (state.kanban || []);
  const source_item = work_item_by_id(items, source_id);
  const source_condition = (source_item ? text(source_item.condition) : "");
  const source_lane = board_lane_id(source_condition);
  const target_id = (target_card ? bare(target_card.northThreadId) : "");
  const target_item = ((target_id === "") ? null : work_item_by_id(items, target_id));
  const target_condition = (target_item ? text(target_item.condition) : "");
  return ((source_id === "")) ? set_board_notice_bang(runtime, "Drop ignored: the dragged source was not a North work card.") : (((target_lane === "done") && (!terminal_condition_p(source_condition)))) ? prefill_outcome_bang(runtime, ui, source_id) : (((source_lane === "not-started") && (target_lane === "not-started") && (source_condition === "ready") && ((target_id === "") || (target_condition === "ready")))) ? ((source_id === target_id) ? set_board_notice_bang(runtime, "Queue order unchanged: a card cannot be moved relative to itself.") : (() => { const position = ((target_id === "") ? ((event.x < (event.currentTarget.screenX + (event.currentTarget.width / 2))) ? "first" : "last") : ((event.x < (target_card.screenX + (target_card.width / 2))) ? "before" : "after")); return report_promise_bang(runtime, move_ready_thread_bang(runtime, source_id, position, target_id)); })()) : set_board_notice_bang(runtime, unsupported_drop_notice(source_condition, target_lane));
}

function card_content(item, width) {
  const body = compact_body(item.body, Math.max(8, (width - 4)));
  const fallback = ("".concat("@", short_thread_id(item)));
  return new StyledText([brightWhite(compact_text(item.title, Math.max(8, (width - 4)))), white("\n"), dim(((body === "") ? fallback : body))]);
}

function make_board_card_bang(runtime, ui, item, index, lane_index, width) {
  const renderer = runtime.renderer;
  const thread_id = bare(item.id);
  const condition = text(item.condition);
  const selected_p = (index === runtime.workIndex);
  const next_up_p = ((condition === "ready") && (lane_index === 0));
  const card = new BoxRenderable(renderer, {id: board_card_id(thread_id), width: width, height: 4, flexShrink: 0, paddingX: 1, border: true, borderColor: (selected_p ? "#22d3ee" : "#64748b"), title: (next_up_p ? "Next Up" : null), titleColor: (next_up_p ? "#4ade80" : "#94a3b8")});
  const content = new TextRenderable(renderer, {width: "100%", height: 2, selectable: false, wrapMode: "none", truncate: true, content: card_content(item, width)});
  (card.northThreadId = thread_id);
  (card.northCondition = condition);
  (card.onMouseDown = (event) => { if ((event.button === 0)) {
  event.preventDefault();
  event.stopPropagation();
  return select_board_card_bang(runtime, ui, item, index);
} });
  (card.onMouseDrag = (event) => { (runtime.dragThreadId = thread_id);
return ui.workScroll.updateAutoScroll(event.x, event.y); });
  (card.onMouseDragEnd = (__event) => { (runtime.dragThreadId = "");
return ui.workScroll.stopAutoScroll(); });
  (card.onMouseOver = (__event) => (card.borderColor = "#22d3ee"));
  (card.onMouseOut = (__event) => (card.borderColor = (selected_p ? "#22d3ee" : "#64748b")));
  (card.onMouseDrop = (event) => handle_board_drop_bang(runtime, ui, board_lane_id(condition), card, event));
  card.add(content);
  return card;
}

function make_board_lane_bang(runtime, ui, lane, items, card_width) {
  const renderer = runtime.renderer;
  const lane_id = text(lane.id);
  const title = text(lane.title);
  const lane_items = board_lane_items(items, lane_id);
  const lane_box = new BoxRenderable(renderer, {width: "100%", flexDirection: "column", flexShrink: 0, border: ["bottom"], borderColor: "#334155", paddingBottom: 1});
  const header = new TextRenderable(renderer, {width: "100%", height: 1, flexShrink: 0, selectable: false, wrapMode: "none", content: new StyledText([brightGreen(title), brightBlack(("".concat("  ", lane_items.length)))])});
  const cards = new BoxRenderable(renderer, {width: "100%", minHeight: 4, flexDirection: "row", flexWrap: "wrap", flexShrink: 0, gap: 1, rowGap: 1});
  (cards.northLaneId = lane_id);
  (cards.onMouseOver = (__event) => (lane_box.borderColor = "#4ade80"));
  (cards.onMouseOut = (__event) => (lane_box.borderColor = "#334155"));
  (cards.onMouseDrop = (event) => handle_board_drop_bang(runtime, ui, lane_id, null, event));
  lane_items.forEach((item, lane_index) => cards.add(make_board_card_bang(runtime, ui, item, items.indexOf(item), lane_index, card_width)));
  lane_box.add(header);
  lane_box.add(cards);
  return lane_box;
}

function clear_board_bang(root) {
  return root.getChildren().forEach((child) => child.destroyRecursively());
}

function sync_board_bang(runtime, ui, items, selected, width) {
  const signature = board_signature(items, selected, width);
  if ((!(signature === text(runtime.boardSignature)))) {
    (runtime.boardSignature = signature);
    clear_board_bang(ui.boardRoot);
    const card_width = ((width >= 54) ? 25 : Math.max(18, (width - 3)));
    BOARD_LANES.forEach((lane) => ui.boardRoot.add(make_board_lane_bang(runtime, ui, lane, items, card_width)));
    if (((selected >= 0) && (selected < items.length))) {
      return ui.workScroll.scrollChildIntoView(board_card_id(bare(items[selected].id)));
    }
  }
}

function work_content_bang(runtime, state, view, selected) {
  const items = (view.items || []);
  const width = available_work_width(runtime, state);
  return ((items.length === 0) ? new StyledText([brightBlack(("".concat("No ", (text(view.title) || "work"), " items")))]) : ((text(view.id) === "graph")) ? render_dag_view_bang(items, selected, width) : render_list_view_bang(items, selected, width));
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
  const agent_options = ((runtime.pane === "agents") ? palette_options("agents", text(ui.agentInput.value)) : []);
  const work_options = ((runtime.pane === "work") ? palette_options("work", text(ui.workInput.value)) : []);
  const active_options = ((runtime.pane === "agents") ? agent_options : work_options);
  const palette_max = Math.max(0, (active_options.length - 1));
  const board_p = (text(current.id) === "board");
  (runtime.agentIndex = Math.max(0, Math.min(runtime.agentIndex, agent_max)));
  (runtime.workIndex = Math.max(0, Math.min(runtime.workIndex, work_max)));
  (runtime.paletteIndex = Math.max(0, Math.min(runtime.paletteIndex, palette_max)));
  apply_workspace_geometry_bang(runtime, ui, state);
  render_prompt_bang(runtime, ui.agentPrompt);
  render_prompt_bang(runtime, ui.workPrompt);
  (ui.agentsHeader.content = render_pane_header("Agents", (runtime.pane === "agents")));
  (ui.agentsText.content = roster_text(state, runtime.agentIndex));
  (ui.transcriptText.content = render_conversation_bang(runtime));
  (ui.tabsText.content = render_work_tabs_bang(state, text(current.id), (runtime.pane === "work")));
  (ui.workText.visible = (!board_p));
  (ui.boardRoot.visible = board_p);
  if (board_p) {
    sync_board_bang(runtime, ui, items, runtime.workIndex, available_work_width(runtime, state));
  } else {
    (ui.workText.content = work_content_bang(runtime, state, current, runtime.workIndex));
  }
  (ui.statusText.content = render_status(runtime, state));
  (ui.agentStatusText.content = render_status(runtime, state));
  (ui.agentStatusText.visible = (text(runtime.closedPane) === "work"));
  (ui.agentPalette.visible = (agent_options.length > 0));
  (ui.agentPalette.height = Math.max(1, Math.min(8, agent_options.length)));
  (ui.agentPalette.content = ((agent_options.length > 0) ? render_command_palette_bang(agent_options, runtime.paletteIndex) : ""));
  (ui.workPalette.visible = (work_options.length > 0));
  (ui.workPalette.height = Math.max(1, Math.min(8, work_options.length)));
  (ui.workPalette.content = ((work_options.length > 0) ? render_command_palette_bang(work_options, runtime.paletteIndex) : ""));
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
  const actual_title = (existing ? text(existing.title) : title);
  return upsert_conversation_bang(runtime, conversation_item_with_data_bang(id, kind, actual_title, clipped(("".concat(prior, delta)), 6000), status, (existing ? existing.data : null)));
}

function assistant_message_text(data) {
  const message = data.message;
  const content = (message ? message.content : null);
  return (Array.isArray(content) ? content.map((part) => text(part.text)).filter((part) => (!(part === ""))).join("\n") : text((data.text || data.result)));
}

function adopt_session_metadata_bang(runtime, source) {
  if (source) {
    const model = text((source.model || source.modelName));
    const effort = text((source.reasoningEffort || source.effort));
    const cwd = text((source.cwd || source.workingDirectory));
    if ((!(model === ""))) {
      (runtime.sessionModel = model);
    }
    if ((!(effort === ""))) {
      (runtime.sessionEffort = effort);
    }
    if ((!(cwd === ""))) {
      return (runtime.sessionCwd = cwd);
    }
  }
}

function handle_codex_event_bang(runtime, stream_state, data) {
  const method = text(data.method);
  const params = (data.params || {});
  const execution_id = text(stream_state.executionId);
  return ((method === "thread/started")) ? adopt_session_metadata_bang(runtime, params.thread) : ((method === "model/safetyBuffering/updated")) ? adopt_session_metadata_bang(runtime, params) : ((method === "turn/started")) ? (() => { adopt_session_metadata_bang(runtime, (params.turn || params));
set_working_bang(runtime, true, "Codex is working");
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "working");
} })() : ((method === "item/started")) ? (() => { const item = params.item; const kind = (item ? text(item.type) : ""); const id = event_item_id(execution_id, (item ? item.id : "item")); return ((kind === "commandExecution")) ? upsert_conversation_bang(runtime, conversation_item_with_data_bang(id, "command", clean_text(item.command), "", "running", item)) : ((kind === "mcpToolCall")) ? upsert_conversation_bang(runtime, conversation_item(id, "tool", ("".concat("Called ", text(item.server), ".", text(item.tool))), safe_json(item.arguments), "running")) : ((kind === "fileChange")) ? upsert_conversation_bang(runtime, conversation_item_with_data_bang(id, "change", "Editing files", "", "running", item)) : null; })() : ((method === "item/commandExecution/outputDelta")) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "command", "command", text(params.delta), "running") : ((method === "item/agentMessage/delta")) ? ((!stream_state.booting) ? (() => { return append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "assistant", "", text(params.delta), "running"); })() : null) : (((method === "item/reasoning/summaryTextDelta") || (method === "item/plan/delta"))) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "thought", "", text(params.delta), "running") : ((method === "item/fileChange/outputDelta")) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "change", "Editing files", text(params.delta), "running") : ((method === "item/fileChange/patchUpdated")) ? (() => { const id = event_item_id(execution_id, params.itemId); const existing = conversation_item_by_id(runtime, id); return upsert_conversation_bang(runtime, conversation_item_with_data_bang(id, "change", "Editing files", (existing ? text(existing.body) : ""), "running", params)); })() : ((method === "item/mcpToolCall/progress")) ? append_item_delta_bang(runtime, event_item_id(execution_id, params.itemId), "tool", "Tool activity", text(params.message), "running") : ((method === "item/completed")) ? (() => { const item = params.item; const kind = (item ? text(item.type) : ""); const id = event_item_id(execution_id, (item ? item.id : "item")); return ((kind === "commandExecution")) ? upsert_conversation_bang(runtime, conversation_item_with_data_bang(id, "command", clean_text(item.command), clipped(item.aggregatedOutput, 6000), (((text(item.status) === "completed") && (item.exitCode === 0)) ? "done" : "failed"), item)) : ((kind === "agentMessage")) ? (() => { const body = clean_text(item.text); (runtime.lastAssistantText = body);
if ((!stream_state.booting)) {
  return upsert_conversation_bang(runtime, conversation_item(id, "assistant", "", body, "done"));
} })() : ((kind === "mcpToolCall")) ? upsert_conversation_bang(runtime, conversation_item(id, "tool", ("".concat("Called ", text(item.server), ".", text(item.tool))), clipped(safe_json(item.result), 6000), ((text(item.status) === "failed") ? "failed" : "done"))) : ((kind === "fileChange")) ? upsert_conversation_bang(runtime, conversation_item_with_data_bang(id, "change", "Edited files", "", ((text(item.status) === "failed") ? "failed" : "done"), item)) : (((kind === "webSearch") || (kind === "todoList"))) ? upsert_conversation_bang(runtime, conversation_item(id, "tool", kind, clipped(safe_json(item), 3000), "done")) : null; })() : ((method === "turn/completed")) ? (runtime.workingLabel = "Finishing") : null;
}

function handle_record_bang(runtime, stream_state, record) {
  const kind = text(record.kind);
  const data = (record.data || {});
  const execution_id = text(stream_state.executionId);
  return ((kind === "execution.accepted")) ? (() => { const cwd = text(data.cwd); if ((!(cwd === ""))) {
  return (runtime.sessionCwd = cwd);
} })() : ((kind === "provider.starting")) ? ((!(execution_id === "")) ? (() => { return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "starting"); })() : null) : ((kind === "provider.codex.event")) ? handle_codex_event_bang(runtime, stream_state, data) : ((kind === "provider.assistant")) ? (() => { const body = assistant_message_text(data); if (((!stream_state.booting) && (!(body === "")) && (!(body === runtime.lastAssistantText)))) {
  (runtime.lastAssistantText = body);
  return upsert_conversation_bang(runtime, conversation_item(next_item_id_bang(runtime, "assistant"), "assistant", "", body, "done"));
} })() : ((kind === "provider.result")) ? set_working_bang(runtime, false, "") : ((kind === "session.idle")) ? (() => { const disposition = text(data.disposition); const pending_inputs = Number((data.pendingInputs || 0)); const booting = stream_state.booting; set_working_bang(runtime, false, "");
if (booting) {
  play_sound_event_bang(runtime, stream_state, "ready");
} else if ((disposition === "interrupted")) {
  play_sound_event_bang(runtime, stream_state, "interrupted");
} else if (((disposition === "completed") && (pending_inputs <= 0))) {
  play_sound_event_bang(runtime, stream_state, "done");
} else {
  null;
}
(stream_state.booting = false);
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "ready");
} })() : ((kind === "execution.failed")) ? (() => { set_working_bang(runtime, false, "");
play_sound_event_bang(runtime, stream_state, "failed");
return append_error_bang(runtime, ("".concat(kind, ": ", safe_json(data)))); })() : ((kind.includes("failed") || kind.includes("error"))) ? (() => { set_working_bang(runtime, false, "");
return append_error_bang(runtime, ("".concat(kind, ": ", safe_json(data)))); })() : null;
}

function parse_bridge_stream_bang(runtime, stream_state, chunk) {
  const lines = ("".concat(stream_state.buffer, chunk)).split("\n");
  const remainder = lines.pop();
  (stream_state.buffer = remainder);
  return lines.forEach((raw_line) => { const line = raw_line.trim();
return (line.startsWith("execution ")) ? (() => { const execution_id = line.slice(10).trim(); (stream_state.executionId = execution_id);
(stream_state.soundLive = true);
return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "starting"); })() : (line.startsWith("attached ")) ? (stream_state.soundLive = true) : (line.startsWith("[")) ? (() => { const record = record_line(line); if (record) {
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
  const stream_state = {buffer: "", stderr: "", executionId: "", role: role, booting: (role === "supervisor"), soundLive: false};
  const exit_code = await stream_command([NORTH_BIN, "bridge", "--role", ((role === "supervisor") ? "director" : "implementer"), prompt], (chunk) => parse_bridge_stream_bang(runtime, stream_state, chunk), (chunk) => (stream_state.stderr = clipped(("".concat(stream_state.stderr, chunk)), 6000)));
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

async function submit_agent_bang(runtime, ui, input, selection) {
  const trimmed = input.trim();
  const slash_p = trimmed.startsWith("/");
  const parsed = command(input);
  const name = text(parsed.name);
  const rest = text(parsed.rest);
  const target = text((selection || runtime.supervisorId));
  return (handle_local_command_bang(runtime, ui, input) ? null : ((slash_p && exit_command_p(name))) ? destroy_bang(runtime) : ((slash_p && (name === "launch"))) ? await launch_agent_bang(runtime, rest, "worker") : ((slash_p && (name === "refresh"))) ? await refresh_bang(runtime) : ((slash_p && (name === "popout"))) ? popout_bang(runtime, (rest || runtime.activeView)) : ((slash_p && (name === "help"))) ? (() => { (runtime.showHelp = (!runtime.showHelp));
return runtime.render(); })() : (async () => { if ((target === "")) {
  (() => { throw new Error("select an agent before steering or interrupting"); })();
}
if ((slash_p && (name === "interrupt"))) {
  if ((!runtime.bridgeExecutions.has(target))) {
    (() => { throw new Error("interrupt is available for Bridge-launched executions"); })();
  }
  await run_command([NORTH_BIN, "bridge", "interrupt", target]);
  set_working_bang(runtime, false, "");
  return append_interrupted_bang(runtime);
} else {
  const message = ((slash_p && (name === "steer")) ? rest : trimmed);
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
} })());
}

async function submit_work_bang(runtime, ui, input, selection) {
  const trimmed = input.trim();
  const slash_p = trimmed.startsWith("/");
  const parsed = command(input);
  const name = text(parsed.name);
  const rest = text(parsed.rest);
  return ((!slash_p) ? await submit_agent_bang(runtime, ui, input, runtime.supervisorId) : (handle_local_command_bang(runtime, ui, input) ? null : (exit_command_p(name)) ? destroy_bang(runtime) : ((name === "filter")) ? (() => { (runtime.model = set_filter(runtime.model, rest));
return runtime.render(); })() : ((name === "view")) ? (() => { if ((!recognized_work_view_p(rest))) {
  (() => { throw new Error("view requires list, graph, or board"); })();
}
(runtime.model = focus_view(runtime.model, model_work_view(rest)));
(runtime.workIndex = 0);
runtime.workScroll.scrollTo(0);
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
return await refresh_bang(runtime); })() : ((name === "outcome")) ? (async () => { const split_at = rest.indexOf(" "); const thread_id = ((split_at < 0) ? "" : bare(rest.slice(0, split_at))); const result = ((split_at < 0) ? "" : rest.slice((split_at + 1)).trim()); if ((thread_id === "")) {
  (() => { throw new Error("outcome requires: /outcome <thread-id> <result>"); })();
}
if ((result === "")) {
  (() => { throw new Error("outcome requires a result"); })();
}
await run_command([NORTH_BIN, "tell", thread_id, "outcome", result]);
(runtime.workspaceNotice = ("".concat("Recorded outcome for @", thread_id, ".")));
return await refresh_bang(runtime); })() : ((name === "help")) ? publish_line_bang(runtime, "work commands: /capture <title>, /filter <text>, /assign <driver>, /outcome <id> <result>, /view list|graph|board, /split h|v, /refresh, /popout, /exit") : (() => { throw new Error("unknown work command; use /help"); })()));
}

function report_promise_bang(runtime, promise) {
  return promise.catch((error) => publish_line_bang(runtime, ("".concat("error: ", error_message(error)))));
}

function select_pane_bang(runtime, pane) {
  (runtime.pane = pane);
  (runtime.paletteIndex = 0);
  return (runtime.workspaceNotice = "");
}

function pane_box(ui, pane) {
  return ((pane === "agents") ? ui.agentsPane : ui.workPane);
}

function focus_pane_bang(runtime, ui, pane) {
  select_pane_bang(runtime, pane);
  ((pane === "agents") ? ui.agentInput : ui.workInput).focus();
  return runtime.render();
}

function focus_pane_surface_bang(runtime, ui, pane) {
  if (pane_visible_p(runtime, pane)) {
    select_pane_bang(runtime, pane);
    pane_box(ui, pane).focus();
    return runtime.render();
  }
}

function active_input(runtime, ui) {
  return ((runtime.pane === "agents") ? ui.agentInput : ui.workInput);
}

function active_palette_options(runtime, ui) {
  const pane = text(runtime.pane);
  const input = active_input(runtime, ui);
  return palette_options(pane, text(input.value));
}

function pane_for_direction(runtime, state, direction) {
  const pane = text(runtime.pane);
  const layout = text(state.layout);
  return ((direction === "w")) ? (() => { const next_pane = ((pane === "agents") ? "work" : "agents"); return (pane_visible_p(runtime, next_pane) ? next_pane : pane); })() : (((layout === "vertical") && (direction === "h") && (pane === "work") && pane_visible_p(runtime, "agents"))) ? "agents" : (((layout === "vertical") && (direction === "l") && (pane === "agents") && pane_visible_p(runtime, "work"))) ? "work" : (((layout === "horizontal") && (direction === "k") && (pane === "work") && pane_visible_p(runtime, "agents"))) ? "agents" : (((layout === "horizontal") && (direction === "j") && (pane === "agents") && pane_visible_p(runtime, "work"))) ? "work" : pane;
}

function restore_workspace_bang(runtime, ui, layout) {
  (runtime.closedPane = "");
  (runtime.model = set_layout(runtime.model, layout));
  return focus_pane_surface_bang(runtime, ui, text(runtime.pane));
}

function close_focused_pane_bang(runtime, ui) {
  if ((!(text(runtime.closedPane) === ""))) {
    (runtime.workspaceNotice = "cannot close the last pane");
    return runtime.render();
  } else {
    const pane = text(runtime.pane);
    const survivor = ((pane === "agents") ? "work" : "agents");
    (runtime.closedPane = pane);
    return focus_pane_surface_bang(runtime, ui, survivor);
  }
}

function equalize_panes_bang(runtime) {
  (runtime.workspaceNotice = "");
  (runtime.paneRatio = 50);
  return runtime.render();
}

function resize_focused_pane_bang(runtime, ui, amount) {
  if ((text(runtime.closedPane) === "")) {
    const state = snapshot(runtime.model);
    const total = workspace_primary_cells(runtime, ui, state);
    const minimum = minimum_pane_cells(state, total);
    const ratio = clamp_pane_ratio_bang(runtime, ui, state);
    const measured = (stacked_workspace_p(state) ? ui.agentsPane.height : ui.agentsPane.width);
    const current = (((typeof measured === "number") && Number.isFinite(measured) && (measured > 0)) ? Math.round(measured) : Math.round((total * (ratio / 100))));
    const signed = ((text(runtime.pane) === "agents") ? amount : (-amount));
    const next = Math.max(minimum, Math.min((total - minimum), (current + signed)));
    (runtime.workspaceNotice = "");
    (runtime.paneRatio = (100 * (next / total)));
    return runtime.render();
  }
}

function workspace_count(context) {
  const payload = context.payload;
  const raw = Number((payload ? payload.count : 1));
  return ((Number.isFinite(raw) && (raw > 0)) ? Math.floor(raw) : 1);
}

function workspace_action_bang(runtime, ui, action, count) {
  const state = snapshot(runtime.model);
  return (((action === "h") || (action === "j") || (action === "k") || (action === "l") || (action === "w"))) ? focus_pane_surface_bang(runtime, ui, pane_for_direction(runtime, state, action)) : ((action === "v")) ? restore_workspace_bang(runtime, ui, "vertical") : ((action === "s")) ? restore_workspace_bang(runtime, ui, "horizontal") : ((action === "c")) ? close_focused_pane_bang(runtime, ui) : ((action === "=")) ? equalize_panes_bang(runtime) : ((action === ">")) ? resize_focused_pane_bang(runtime, ui, count) : ((action === "<")) ? resize_focused_pane_bang(runtime, ui, (-count)) : null;
}

function workspace_handler_bang(runtime, ui, action) {
  return (context) => workspace_action_bang(runtime, ui, action, workspace_count(context));
}

function uncounted_workspace_bindings_bang(runtime, ui) {
  return [{key: "ctrl+w h", cmd: workspace_handler_bang(runtime, ui, "h")}, {key: "ctrl+w j", cmd: workspace_handler_bang(runtime, ui, "j")}, {key: "ctrl+w k", cmd: workspace_handler_bang(runtime, ui, "k")}, {key: "ctrl+w l", cmd: workspace_handler_bang(runtime, ui, "l")}, {key: "ctrl+w w", cmd: workspace_handler_bang(runtime, ui, "w")}, {key: "ctrl+w v", cmd: workspace_handler_bang(runtime, ui, "v")}, {key: "ctrl+w s", cmd: workspace_handler_bang(runtime, ui, "s")}, {key: "ctrl+w c", cmd: workspace_handler_bang(runtime, ui, "c")}, {key: "ctrl+w =", cmd: workspace_handler_bang(runtime, ui, "=")}, {key: "ctrl+w >", cmd: workspace_handler_bang(runtime, ui, ">")}, {key: "ctrl+w shift+.", cmd: workspace_handler_bang(runtime, ui, ">")}, {key: "ctrl+w shift+>", cmd: workspace_handler_bang(runtime, ui, ">")}, {key: "ctrl+w <", cmd: workspace_handler_bang(runtime, ui, "<")}, {key: "ctrl+w shift+,", cmd: workspace_handler_bang(runtime, ui, "<")}, {key: "ctrl+w shift+<", cmd: workspace_handler_bang(runtime, ui, "<")}];
}

function add_counted_resize_bindings_bang(bindings, runtime, ui, action, suffix) {
  bindings.push({key: ("".concat("{count}ctrl+w", suffix)), cmd: workspace_handler_bang(runtime, ui, action)});
  return bindings.push({key: ("".concat("ctrl+w{count}", suffix)), cmd: workspace_handler_bang(runtime, ui, action)});
}

function counted_workspace_bindings_bang(runtime, ui) {
  const bindings = [];
  [">", "shift+.", "shift+>"].forEach((suffix) => add_counted_resize_bindings_bang(bindings, runtime, ui, ">", suffix));
  ["<", "shift+,", "shift+<"].forEach((suffix) => add_counted_resize_bindings_bang(bindings, runtime, ui, "<", suffix));
  return bindings;
}

function count_key_match(key) {
  const name = text(key.name);
  if (((name.length === 1) && "0123456789".includes(name) && (!key.ctrl) && (!key.shift) && (!(key.meta || key.option)) && (!key.super) && (!key.hyper))) {
    return {value: name, display: name};
  }
}

function finalize_count(values) {
  return Number(values.join(""));
}

function pending_count_text(sequence) {
  return sequence.filter((part) => (text(part.patternName) === "count")).map((part) => text(part.display)).join("");
}

function install_workspace_keymap_bang(runtime, ui) {
  const keymap = createDefaultOpenTuiKeymap(runtime.renderer);
  const counted = counted_workspace_bindings_bang(runtime, ui);
  registerEmacsBindings(keymap);
  registerEscapeClearsPendingSequence(keymap);
  keymap.registerSequencePattern({name: "count", display: "[count]", payloadKey: "count", min: 1, max: 9, match: count_key_match, finalize: finalize_count});
  keymap.registerLayer({priority: 50, bindings: uncounted_workspace_bindings_bang(runtime, ui)});
  keymap.registerLayer({target: ui.agentsPane, targetMode: "focus", priority: 60, bindings: counted});
  keymap.registerLayer({target: ui.workPane, targetMode: "focus", priority: 60, bindings: counted});
  keymap.on("pendingSequence", (sequence) => { (runtime.windowChord = (sequence.length > 0));
(runtime.windowCount = pending_count_text(sequence));
return runtime.render(); });
  return (runtime.keymap = keymap);
}

function complete_palette_bang(runtime, ui, commands) {
  if ((commands.length > 0)) {
    const index = Math.max(0, Math.min(runtime.paletteIndex, (commands.length - 1)));
    const candidate = commands[index];
    const completion = slashcommand_completion(candidate);
    const suffix = (((completion === "") && slashcommand_arguments(candidate)) ? " " : "");
    const input = active_input(runtime, ui);
    (input.value = ("".concat(((completion === "") ? slashcommand_name(candidate) : completion), suffix)));
    (runtime.paletteIndex = 0);
    input.focus();
    return runtime.render();
  }
}

function install_input_bang(runtime, ui) {
  ui.agentInput.on(InputRenderableEvents.INPUT, (__value) => { (runtime.paletteIndex = 0);
return runtime.render(); });
  ui.workInput.on(InputRenderableEvents.INPUT, (__value) => { (runtime.paletteIndex = 0);
return runtime.render(); });
  ui.agentInput.on(InputRenderableEvents.ENTER, () => { const input = text(ui.agentInput.value).trim();
const state = snapshot(runtime.model);
const agents = (state.agents || []);
const selected = ((agents.length > 0) ? text(agents[runtime.agentIndex].id) : "");
if ((!(input === ""))) {
  (ui.agentInput.value = "");
  return report_promise_bang(runtime, submit_agent_bang(runtime, ui, input, selected));
} });
  return ui.workInput.on(InputRenderableEvents.ENTER, () => { const input = text(ui.workInput.value).trim();
if ((!(input === ""))) {
  (ui.workInput.value = "");
  return report_promise_bang(runtime, submit_work_bang(runtime, ui, input, {view: runtime.activeView, index: runtime.workIndex}));
} });
}

function work_tab_at(tabs, event) {
  const x = (event.x - tabs.x);
  return (((x >= 6) && (x < 12))) ? "list" : (((x >= 14) && (x < 21))) ? "graph" : (((x >= 23) && (x < 30))) ? "board" : "";
}

function complete_clicked_palette_bang(runtime, ui, pane, palette_renderable, event) {
  if ((event.button === 0)) {
    select_pane_bang(runtime, pane);
    const input = ((pane === "agents") ? ui.agentInput : ui.workInput);
    const options = palette_options(pane, text(input.value));
    const row = Math.floor((event.y - palette_renderable.y));
    if (((row >= 0) && (row < options.length))) {
      event.preventDefault();
      event.stopPropagation();
      (runtime.paletteIndex = row);
      return complete_palette_bang(runtime, ui, options);
    }
  }
}

function install_mouse_bang(runtime, ui) {
  (ui.agentsPane.onMouseDown = (event) => { if ((event.button === 0)) {
  return focus_pane_surface_bang(runtime, ui, "agents");
} });
  (ui.workPane.onMouseDown = (event) => { if ((event.button === 0)) {
  return focus_pane_surface_bang(runtime, ui, "work");
} });
  (ui.agentComposer.onMouseDown = (event) => { if ((event.button === 0)) {
  event.stopPropagation();
  return focus_pane_bang(runtime, ui, "agents");
} });
  (ui.workComposer.onMouseDown = (event) => { if ((event.button === 0)) {
  event.stopPropagation();
  return focus_pane_bang(runtime, ui, "work");
} });
  (ui.agentPalette.onMouseDown = (event) => complete_clicked_palette_bang(runtime, ui, "agents", ui.agentPalette, event));
  (ui.workPalette.onMouseDown = (event) => complete_clicked_palette_bang(runtime, ui, "work", ui.workPalette, event));
  return (ui.tabsText.onMouseDown = (event) => { if ((event.button === 0)) {
  const view_id = work_tab_at(ui.tabsText, event);
  if ((!(view_id === ""))) {
    event.preventDefault();
    event.stopPropagation();
    (runtime.model = focus_view(runtime.model, model_work_view(view_id)));
    (runtime.workIndex = 0);
    ui.workScroll.scrollTo(0);
    return focus_pane_surface_bang(runtime, ui, "work");
  }
} });
}

function install_keys_bang(runtime, ui) {
  return runtime.renderer.keyInput.on("keypress", (key) => { if (((!key.defaultPrevented) && (!key.propagationStopped))) {
  const name = text(key.name).toLowerCase();
  const state = snapshot(runtime.model);
  const meta = (key.meta || key.option);
  const palette = active_palette_options(runtime, ui);
  const palette_open = (palette.length > 0);
  if ((palette_open && ((name === "up") || (name === "down") || (key.ctrl && ((name === "j") || (name === "k")))))) {
    key.preventDefault();
    key.stopPropagation();
    (runtime.paletteIndex = ((runtime.paletteIndex + (((name === "up") || (key.ctrl && (name === "k"))) ? -1 : 1) + palette.length) % palette.length));
    active_input(runtime, ui).focus();
  } else if ((palette_open && ((name === "tab") || submit_key_p(name)))) {
    const index = Math.max(0, Math.min(runtime.paletteIndex, (palette.length - 1)));
    const candidate = palette[index];
    const current = text(active_input(runtime, ui).value).trim();
    const exact = (current === slashcommand_name(candidate));
    if (((name === "tab") || (!exact) || slashcommand_arguments(candidate))) {
      key.preventDefault();
      key.stopPropagation();
      complete_palette_bang(runtime, ui, palette);
    }
  } else if ((palette_open && ((name === "escape") || (name === "esc")))) {
    key.preventDefault();
    key.stopPropagation();
    (active_input(runtime, ui).value = "");
    runtime.render();
  } else if (((name === "tab") || (name === "f2"))) {
    key.preventDefault();
    key.stopPropagation();
    focus_pane_bang(runtime, ui, ((runtime.pane === "agents") ? "work" : "agents"));
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
    (runtime.model = focus_view(runtime.model, model_work_view(next_id)));
    (runtime.workIndex = 0);
    ui.workScroll.scrollTo(0);
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
      ui.workScroll.scrollBy((delta * ((text(view.id) === "board")) ? 2 : ((text(view.id) === "graph")) ? 3 : 1), "step");
    }
  } else if (((name === "escape") || (name === "esc"))) {
    const target = text(runtime.supervisorId);
    if ((runtime.working && (!(target === "")))) {
      key.preventDefault();
      key.stopPropagation();
      report_promise_bang(runtime, submit_agent_bang(runtime, ui, "/interrupt", target));
    }
  } else {
    null;
  }
  if ((!runtime.disposed)) {
    return runtime.render();
  }
} });
}

async function open_app_bang(view_id) {
  const view = canonical_work_view(view_id);
  const renderer = await createCliRenderer({exitOnCtrlC: false, clearOnShutdown: true});
  const runtime = {model: make_model(model_work_view(view)), renderer: renderer, disposed: false, pane: "agents", activeView: view, agentIndex: 0, workIndex: 0, workScroll: null, boardSignature: "", dragThreadId: "", bridgeExecutions: new Set(), supervisorId: "", conversation: [], itemSequence: 0, lastAssistantText: "", working: false, workingLabel: "", workingSince: 0, spinnerIndex: 0, spinnerTimer: null, showHelp: false, paletteIndex: 0, promptGlyph: DEFAULT_PROMPT_GLYPH, soundEnabled: sound_enabled_from_env(text(process.env.NORTH_BRIDGE_SOUND)), soundPack: sound_pack_from_env(text(process.env.NORTH_BRIDGE_SOUND_PACK)), soundDirectory: sound_directory_from_env(text(process.env.NORTH_BRIDGE_SOUND_DIR)), soundPlayer: discover_sound_player(), soundChildren: new Set(), soundWarningShown: false, soundSequence: 0, lastSoundPath: "", lastSoundAt: 0, windowChord: false, windowCount: "", workspaceNotice: "", closedPane: "", paneRatio: 50, keymap: null, sessionModel: text((process.env.NORTH_BRIDGE_MODEL || process.env.AGENT_MODEL)), sessionEffort: text((process.env.AGENT_REASONING || process.env.AGENT_EFFORT)), sessionCwd: text(process.cwd()), sessionBranch: "", render: () => null};
  const root = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", height: "100%", gap: 0, padding: 1, onSizeChange: () => runtime.render()});
  const agents_pane = new BoxRenderable(renderer, {flexDirection: "column", width: "50%", border: ["right"], borderColor: "#64748b", focusable: true});
  const work_pane = new BoxRenderable(renderer, {flexDirection: "column", width: "50%", focusable: true});
  const agents_header = new TextRenderable(renderer, {height: 1, flexShrink: 0, wrapMode: "none"});
  const agents_text = new TextRenderable(renderer, {height: 4, flexShrink: 0, wrapMode: "word"});
  const transcript_scroll = new ScrollBoxRenderable(renderer, {flexGrow: 1, scrollY: true, stickyScroll: true, stickyStart: "bottom", viewportCulling: true, verticalScrollbarOptions: {visible: false}});
  const transcript_text_view = new TextRenderable(renderer, {width: "100%", flexShrink: 0, wrapMode: "word"});
  const tabs_text_view = new TextRenderable(renderer, {height: 1, flexShrink: 0, wrapMode: "none"});
  const work_scroll = new ScrollBoxRenderable(renderer, {flexGrow: 1, scrollY: true, viewportCulling: true, verticalScrollbarOptions: {visible: false}});
  const work_text_view = new TextRenderable(renderer, {width: "100%", flexShrink: 0, wrapMode: "none", truncate: true});
  const board_root = new BoxRenderable(renderer, {visible: false, width: "100%", flexDirection: "column", flexShrink: 0, gap: 1});
  const status_text = new TextRenderable(renderer, {flexShrink: 0, wrapMode: "word"});
  const agent_status_text = new TextRenderable(renderer, {visible: false, flexShrink: 0, wrapMode: "word"});
  const agent_palette = new TextRenderable(renderer, {visible: false, height: 1, width: "100%", flexShrink: 0, wrapMode: "none", truncate: true, bg: "#25272d"});
  const work_palette = new TextRenderable(renderer, {visible: false, height: 1, width: "100%", flexShrink: 0, wrapMode: "none", truncate: true, bg: "#25272d"});
  const agent_composer = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", height: 1, flexShrink: 0, backgroundColor: "#25272d"});
  const work_composer = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", height: 1, flexShrink: 0, backgroundColor: "#25272d"});
  const agent_prompt = new TextRenderable(renderer, {width: 2, height: 1, flexShrink: 0, wrapMode: "none", content: new StyledText([brightCyan("❯ ")])});
  const work_prompt = new TextRenderable(renderer, {width: 2, height: 1, flexShrink: 0, wrapMode: "none", content: new StyledText([brightCyan("❯ ")])});
  const agent_input = new InputRenderable(renderer, {width: "100%", flexGrow: 1, backgroundColor: "#25272d", focusedBackgroundColor: "#25272d", textColor: "#e5e7eb", focusedTextColor: "#f8fafc", placeholderColor: "#6b7280", placeholder: "Message Codex supervisor…"});
  const work_input = new InputRenderable(renderer, {width: "100%", flexGrow: 1, backgroundColor: "#25272d", focusedBackgroundColor: "#25272d", textColor: "#e5e7eb", focusedTextColor: "#f8fafc", placeholderColor: "#6b7280", placeholder: "/view list|graph|board, /capture, /filter, /assign"});
  const ui = {root: root, agentsPane: agents_pane, workPane: work_pane, agentsHeader: agents_header, agentsText: agents_text, transcriptText: transcript_text_view, tabsText: tabs_text_view, workScroll: work_scroll, workText: work_text_view, boardRoot: board_root, statusText: status_text, agentStatusText: agent_status_text, agentPalette: agent_palette, workPalette: work_palette, agentComposer: agent_composer, workComposer: work_composer, agentPrompt: agent_prompt, workPrompt: work_prompt, agentInput: agent_input, workInput: work_input};
  agents_pane.add(agents_header);
  agents_pane.add(agents_text);
  transcript_scroll.add(transcript_text_view);
  agents_pane.add(transcript_scroll);
  agents_pane.add(agent_status_text);
  agents_pane.add(agent_palette);
  agent_composer.add(agent_prompt);
  agent_composer.add(agent_input);
  agents_pane.add(agent_composer);
  work_pane.add(tabs_text_view);
  work_scroll.add(work_text_view);
  work_scroll.add(board_root);
  work_pane.add(work_scroll);
  work_pane.add(status_text);
  work_pane.add(work_palette);
  work_composer.add(work_prompt);
  work_composer.add(work_input);
  work_pane.add(work_composer);
  root.add(agents_pane);
  root.add(work_pane);
  renderer.root.add(root);
  (transcript_scroll.verticalScrollBar.visible = false);
  (transcript_scroll.horizontalScrollBar.visible = false);
  (work_scroll.verticalScrollBar.visible = false);
  (work_scroll.horizontalScrollBar.visible = false);
  (runtime.workScroll = work_scroll);
  (runtime.render = () => render_ui_bang(runtime, ui));
  install_input_bang(runtime, ui);
  install_mouse_bang(runtime, ui);
  install_workspace_keymap_bang(runtime, ui);
  install_global_exit_bang(runtime);
  install_keys_bang(runtime, ui);
  runtime.render();
  agent_input.focus();
  renderer.start();
  report_promise_bang(runtime, discover_session_branch_bang(runtime));
  report_promise_bang(runtime, refresh_bang(runtime));
  report_promise_bang(runtime, launch_agent_bang(runtime, SUPERVISOR_BOOT_PROMPT, "supervisor"));
  return runtime;
}

export function run_northbridge_app_bang(options) {
  return open_app_bang(text((options.viewId || "list")));
}
