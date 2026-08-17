import { BoxRenderable, ScrollBoxRenderable, StyledText, bg, brightBlack, brightCyan, brightGreen, brightRed, brightWhite, brightYellow, createCliRenderer, dim, InputRenderable, InputRenderableEvents, red, stripAnsiSequences, TextRenderable, white } from '@opentui/core';
import { registerEmacsBindings, registerEscapeClearsPendingSequence } from '@opentui/keymap/addons';
import { createDefaultOpenTuiKeymap } from '@opentui/keymap/opentui';
import { Agent, WorkItem, agent_effort, agent_goal, agent_id, agent_model, agent_model_display, agent_name, agent_orchestration_provenance, agent_provider, agent_provider_label, agent_provider_target, agent_state, agent_status, agent_task, bridgesnapshot_active_view_id, bridgesnapshot_agents, bridgesnapshot_board, bridgesnapshot_list, bridgesnapshot_notice, bridgesnapshot_selected_agent, bridgesnapshot_selected_thread, focus_view, make_model, remove_agent, replace_projection, select_agent, select_thread, set_filter, snapshot, upsert_agent, workitem_body, workitem_condition, workitem_dependencies, workitem_driver, workitem_id, workitem_title } from './model.js';

const IntlSegmenter = Intl.Segmenter;

function SlashCommand(name, description, arguments$, completion, emoji) {
  return Object.freeze({_tag: "SlashCommand", name, description, arguments: arguments$, completion, emoji});
}

function slashcommand_name(r) { return r.name; }

function slashcommand_description(r) { return r.description; }

function slashcommand_arguments(r) { return r.arguments; }

function slashcommand_completion(r) { return r.completion; }

function slashcommand_emoji(r) { return r.emoji; }

function ParsedCommand(name, rest) {
  return Object.freeze({_tag: "ParsedCommand", name, rest});
}

function parsedcommand_name(r) { return r.name; }

function parsedcommand_rest(r) { return r.rest; }

function ConversationItem(id, kind, title, body, status, data, execution_id, at, cursor, sequence) {
  return Object.freeze({_tag: "ConversationItem", id, kind, title, body, status, data, execution_id, at, cursor, sequence});
}

function conversationitem_id(r) { return r.id; }

function conversationitem_kind(r) { return r.kind; }

function conversationitem_title(r) { return r.title; }

function conversationitem_body(r) { return r.body; }

function conversationitem_status(r) { return r.status; }

function conversationitem_data(r) { return r.data; }

function conversationitem_execution_id(r) { return r.execution_id; }

function conversationitem_at(r) { return r.at; }

function conversationitem_cursor(r) { return r.cursor; }

function conversationitem_sequence(r) { return r.sequence; }

function WorkView(id, title, items) {
  return Object.freeze({_tag: "WorkView", id, title, items});
}

function workview_id(r) { return r.id; }

function workview_title(r) { return r.title; }

function workview_items(r) { return r.items; }

function ListSection(id, title) {
  return Object.freeze({_tag: "ListSection", id, title});
}

function listsection_id(r) { return r.id; }

function listsection_title(r) { return r.title; }

function ListRow(kind, condition, index, count) {
  return Object.freeze({_tag: "ListRow", kind, condition, index, count});
}

function listrow_kind(r) { return r.kind; }

function listrow_condition(r) { return r.condition; }

function listrow_index(r) { return r.index; }

function listrow_count(r) { return r.count; }

function WorkSelection(view, index) {
  return Object.freeze({_tag: "WorkSelection", view, index});
}

function workselection_view(r) { return r.view; }

function workselection_index(r) { return r.index; }

function BoardLane(id, title) {
  return Object.freeze({_tag: "BoardLane", id, title});
}

function boardlane_id(r) { return r.id; }

function boardlane_title(r) { return r.title; }

function SoundPack(ready, done, interrupted, failed) {
  return Object.freeze({_tag: "SoundPack", ready, done, interrupted, failed});
}

function soundpack_ready(r) { return r.ready; }

function soundpack_done(r) { return r.done; }

function soundpack_interrupted(r) { return r.interrupted; }

function soundpack_failed(r) { return r.failed; }

function SoundPlayer(kind, path) {
  return Object.freeze({_tag: "SoundPlayer", kind, path});
}

function soundplayer_kind(r) { return r.kind; }

function soundplayer_path(r) { return r.path; }

function ParsedRecord(sequence, kind, data) {
  return Object.freeze({_tag: "ParsedRecord", sequence, kind, data});
}

function parsedrecord_sequence(r) { return r.sequence; }

function parsedrecord_kind(r) { return r.kind; }

function parsedrecord_data(r) { return r.data; }

function CommandParts(executable, arguments$) {
  return Object.freeze({_tag: "CommandParts", executable, arguments: arguments$});
}

function commandparts_executable(r) { return r.executable; }

function commandparts_arguments(r) { return r.arguments; }

function DiffRow(kind, old, new$, text) {
  return Object.freeze({_tag: "DiffRow", kind, old, new: new$, text});
}

function diffrow_kind(r) { return r.kind; }

function diffrow_old(r) { return r.old; }

function diffrow_new(r) { return r.new; }

function diffrow_text(r) { return r.text; }

function DiffState(old_line, new_line, additions, deletions, rows) {
  return Object.freeze({_tag: "DiffState", old_line, new_line, additions, deletions, rows});
}

function diffstate_old_line(r) { return r.old_line; }

function diffstate_new_line(r) { return r.new_line; }

function diffstate_additions(r) { return r.additions; }

function diffstate_deletions(r) { return r.deletions; }

function diffstate_rows(r) { return r.rows; }

function FileChangeDetails(path, kind, additions, deletions, rows) {
  return Object.freeze({_tag: "FileChangeDetails", path, kind, additions, deletions, rows});
}

function filechangedetails_path(r) { return r.path; }

function filechangedetails_kind(r) { return r.kind; }

function filechangedetails_additions(r) { return r.additions; }

function filechangedetails_deletions(r) { return r.deletions; }

function filechangedetails_rows(r) { return r.rows; }

function FileChangeSummary(additions, deletions, files) {
  return Object.freeze({_tag: "FileChangeSummary", additions, deletions, files});
}

function filechangesummary_additions(r) { return r.additions; }

function filechangesummary_deletions(r) { return r.deletions; }

function filechangesummary_files(r) { return r.files; }

function StripBucket(id, glyph) {
  return Object.freeze({_tag: "StripBucket", id, glyph});
}

function stripbucket_id(r) { return r.id; }

function stripbucket_glyph(r) { return r.glyph; }

function AgentSegment(id, label, count) {
  return Object.freeze({_tag: "AgentSegment", id, label, count});
}

function agentsegment_id(r) { return r.id; }

function agentsegment_label(r) { return r.label; }

function agentsegment_count(r) { return r.count; }

function north_bin() {
  const configured = text(process.env.NORTH_BIN);
  return ((configured === "") ? "north" : configured);
}

const AGENTS_BIN = (() => { const configured = text(process.env.AGENTS_BIN); return ((configured === "") ? "agents" : configured); })();

const SUPERVISOR_BOOT_PROMPT = "You are the Northbridge supervisor. Reply only READY, then wait for operator input.";

const BOARD_LANES = [BoardLane("not-started", "Not Started"), BoardLane("in-progress", "In Progress"), BoardLane("done", "Done")];

const LIST_SECTIONS = [ListSection("active", "In Progress"), ListSection("ready", "Next Up"), ListSection("blocked", "Blocked"), ListSection("dormant", "Backlog"), ListSection("draft", "Draft"), ListSection("terminal", "Done"), ListSection("other", "Todo")];

const STRIP_BUCKETS = [StripBucket("running", ""), StripBucket("blocked", "! "), StripBucket("failed", "✕ ")];

const STRIP_SEPARATOR = " · ";

const STRIP_INDENT = 2;

const CHROME_ROWS = 4;

const MIN_WORKSPACE_ROWS = 4;

function fitted_window(total, rows, reserved) {
  return Math.max(1, Math.min(total, (rows - reserved)));
}

function window_start(index, total, visible) {
  return Math.max(0, Math.min((index - Math.floor((visible / 2))), (total - visible)));
}

const DEFAULT_PROMPT_GLYPH = "❯";

const SOUND_COOLDOWN_MS = 700;

const PEON_SOUND_PACK = SoundPack(["PeonReady1.ogg"], ["PeonYes1.ogg", "PeonYes2.ogg", "PeonYes3.ogg", "PeonYes4.ogg"], ["PeonWhat1.ogg", "PeonWhat3.ogg", "PeonWhat4.ogg"], ["PeonAngry1.ogg", "PeonAngry4.ogg"]);

const PEASANT_SOUND_PACK = SoundPack(["PeasantReady1.mp3"], ["PeasantYes2.mp3", "PeasantYes4.mp3", "PeasantYesAttack4.mp3"], ["PeasantWhat1.mp3"], ["PeasantAngry1.mp3"]);

const EMOJI_COMMANDS = [SlashCommand("😀", "grinning face · happy smile", false, "😀", true), SlashCommand("😄", "smiling face · happy cheerful", false, "😄", true), SlashCommand("😂", "tears of joy · laugh funny", false, "😂", true), SlashCommand("😊", "warm smile · pleased blush", false, "😊", true), SlashCommand("😎", "cool face · sunglasses", false, "😎", true), SlashCommand("🤔", "thinking face · consider question", false, "🤔", true), SlashCommand("😅", "relieved smile · nervous sweat", false, "😅", true), SlashCommand("😭", "crying face · sad tears", false, "😭", true), SlashCommand("😡", "angry face · mad upset", false, "😡", true), SlashCommand("😤", "frustrated face · annoyed", false, "😤", true), SlashCommand("🥳", "celebration face · party", false, "🥳", true), SlashCommand("🤯", "mind blown · surprised", false, "🤯", true), SlashCommand("🫡", "salute · acknowledged", false, "🫡", true), SlashCommand("👋", "wave · hello goodbye", false, "👋", true), SlashCommand("👍", "thumbs up · approve yes", false, "👍", true), SlashCommand("👎", "thumbs down · reject no", false, "👎", true), SlashCommand("🙏", "thanks · please gratitude", false, "🙏", true), SlashCommand("💪", "strength · effort strong", false, "💪", true), SlashCommand("👀", "eyes · look review", false, "👀", true), SlashCommand("🎉", "party popper · celebrate success", false, "🎉", true), SlashCommand("❤️", "heart · love favorite", false, "❤️", true), SlashCommand("🔥", "fire · hot excellent", false, "🔥", true), SlashCommand("✨", "sparkles · magic polish", false, "✨", true), SlashCommand("🚀", "rocket · launch ship", false, "🚀", true), SlashCommand("💡", "light bulb · idea insight", false, "💡", true), SlashCommand("✅", "done · complete success check", false, "✅", true), SlashCommand("❌", "failed · error cross", false, "❌", true), SlashCommand("⚠️", "warning · caution attention", false, "⚠️", true), SlashCommand("🐛", "bug · defect debug", false, "🐛", true), SlashCommand("🔧", "wrench · fix repair tool", false, "🔧", true), SlashCommand("🧪", "test tube · test experiment", false, "🧪", true), SlashCommand("📌", "pin · important remember", false, "📌", true), SlashCommand("📝", "note · write document", false, "📝", true), SlashCommand("⏳", "waiting · hourglass pending", false, "⏳", true), SlashCommand("🔒", "lock · secure private", false, "🔒", true), SlashCommand("🔓", "unlock · open access", false, "🔓", true), SlashCommand("📦", "package · bundle release", false, "📦", true), SlashCommand("🧭", "compass · navigate direction", false, "🧭", true), SlashCommand("🔊", "speaker · sound volume", false, "🔊", true), SlashCommand("🔇", "muted speaker · quiet silence", false, "🔇", true), SlashCommand("❯", "prompt · leader chevron", false, "❯", true), SlashCommand("→", "right arrow · next forward", false, "→", true), SlashCommand("←", "left arrow · back previous", false, "←", true), SlashCommand("↑", "up arrow · increase", false, "↑", true), SlashCommand("↓", "down arrow · decrease", false, "↓", true), SlashCommand("★", "star · favorite important", false, "★", true), SlashCommand("•", "bullet · list point", false, "•", true), SlashCommand("✓", "check · yes done", false, "✓", true), SlashCommand("✗", "cross · no failed", false, "✗", true)];

const GLYPH_COMMANDS = [SlashCommand("❯", "heavy chevron", false, "/glyph ❯", true), SlashCommand("›", "single chevron", false, "/glyph ›", true), SlashCommand("»", "double chevron", false, "/glyph »", true), SlashCommand("→", "right arrow", false, "/glyph →", true), SlashCommand("λ", "lambda", false, "/glyph λ", true), SlashCommand("◆", "diamond", false, "/glyph ◆", true), SlashCommand("•", "bullet", false, "/glyph •", true), SlashCommand("$", "shell dollar", false, "/glyph $", true)];

const AGENT_COMMANDS = [SlashCommand("/launch", "start another Codex worker", true, "", false), SlashCommand("/provider", "set next launch: anthropic, openai, or auto", true, "", false), SlashCommand("/model", "set next launch: tier, exact model, or auto", true, "", false), SlashCommand("/effort", "set next launch: low, medium, high, xhigh, max, or auto", true, "", false), SlashCommand("/interrupt", "interrupt the active agent turn", false, "", false), SlashCommand("/transcript", "show selected or all execution transcripts", true, "", false), SlashCommand("/capture", "capture a new thread", true, "", false), SlashCommand("/threads", "show Threads (or `popout`)", true, "", false), SlashCommand("/refresh", "refresh agents and threads", false, "", false), SlashCommand("/restart", "retire the control daemon now", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/config", "toggle the context switchboard", false, "", false), SlashCommand("/hooks", "switchboard: hooks only", false, "", false), SlashCommand("/skills", "switchboard: skills only", false, "", false), SlashCommand("/mcp", "share MCP: add <name> <url|-- command> | remove <name> | list", true, "", false), SlashCommand("/plugins", "switchboard: plugins only", false, "", false), SlashCommand("/modules", "switchboard: orchestration modules only", false, "", false), SlashCommand("/globals", "switchboard: global knobs, skills, hooks", false, "", false), SlashCommand("/agentsmd", "switchboard: AGENTS.md and directory context", false, "", false), SlashCommand("/q", "quit Northbridge", false, "", false), SlashCommand("/help", "show Northbridge controls", false, "", false)];

const THREAD_COMMANDS = [SlashCommand("/capture", "capture a new thread", true, "", false), SlashCommand("/provider", "set next launch: anthropic, openai, or auto", true, "", false), SlashCommand("/model", "set next launch: tier, exact model, or auto", true, "", false), SlashCommand("/effort", "set next launch: low, medium, high, xhigh, max, or auto", true, "", false), SlashCommand("/filter", "filter visible threads", true, "", false), SlashCommand("/assign", "reassign the selected thread", true, "", false), SlashCommand("/outcome", "record a selected thread outcome", true, "", false), SlashCommand("/list", "threads as a list", false, "", false), SlashCommand("/board", "threads as a board", false, "", false), SlashCommand("/graph", "threads as a graph", false, "", false), SlashCommand("/agents", "back to Agents", false, "", false), SlashCommand("/refresh", "refresh agents and threads", false, "", false), SlashCommand("/restart", "retire the control daemon now", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/config", "toggle the context switchboard", false, "", false), SlashCommand("/hooks", "switchboard: hooks only", false, "", false), SlashCommand("/skills", "switchboard: skills only", false, "", false), SlashCommand("/mcp", "share MCP: add <name> <url|-- command> | remove <name> | list", true, "", false), SlashCommand("/plugins", "switchboard: plugins only", false, "", false), SlashCommand("/modules", "switchboard: orchestration modules only", false, "", false), SlashCommand("/globals", "switchboard: global knobs, skills, hooks", false, "", false), SlashCommand("/agentsmd", "switchboard: AGENTS.md and directory context", false, "", false), SlashCommand("/q", "quit Northbridge", false, "", false), SlashCommand("/help", "show thread commands", false, "", false)];

function text(value) {
  return ((typeof value === "string") ? value : "");
}

function text_or(value, fallback) {
  const candidate = text(value);
  return ((candidate === "") ? fallback : candidate);
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

function stale_daemon_summary(live) {
  return ((live > 0) ? ("".concat("control daemon is stale (", live, " live session", ((live === 1) ? "" : "s"), ") — /restart to replace it now, or it retires when they drain")) : "control daemon is stale — /restart to replace it now");
}

function failure_summary(data) {
  const message = text_or(data.message, text_or(data.detail, text_or(data.classification, text(data.code))));
  const causes = data.causes;
  const cause = ((causes && (causes.length > 0)) ? text(causes[0]) : "");
  return (((message.includes("north_coordinator_preflight") || cause.includes("ECONNREFUSED 127.0.0.1:7977"))) ? "coordinator offline — supervision unavailable; /config and /help still work" : ((message === "bridge_daemon_source_stale")) ? stale_daemon_summary(Number((data.live || 0))) : ((message === "")) ? safe_json(data) : ((cause === "")) ? message : ("".concat(message, " — ", cause)));
}

function clean_text(value) {
  return stripAnsiSequences(text(value));
}

export function agent_field_text(value) {
  return Array.from(clean_text(value)).map((character) => { const code = character.charCodeAt(0);
return ((((code === 9) || (code === 10) || (code === 13))) ? " " : (((code < 32) || ((code >= 127) && (code <= 159)))) ? "" : character); }).join("");
}

export function agent_cell_text_bang(value, width) {
  const source = agent_field_text(value);
  const limit = Math.max(1, width);
  if ((Bun.stringWidth(source) <= limit)) {
    return source;
  } else {
    const room = Math.max(0, (limit - Bun.stringWidth("…")));
    const kept = [];
    const state = {width: 0, done: false};
    Array.from(new IntlSegmenter("en", {granularity: "grapheme"}).segment(source)).forEach((part) => { const segment = text(part.segment);
const segment_width = Bun.stringWidth(segment);
if ((!state.done)) {
  if (((state.width + segment_width) <= room)) {
    kept.push(segment);
    return (state.width = (state.width + segment_width));
  } else {
    return (state.done = true);
  }
} });
    return ("".concat(kept.join(""), "…"));
  }
}

function clipped(value, limit) {
  const clean = clean_text(value);
  return ((clean.length > limit) ? ("".concat("…", clean.slice((clean.length - limit)))) : clean);
}

function next_item_id_bang(runtime, prefix) {
  (runtime.itemSequence = (runtime.itemSequence + 1));
  return ("".concat(prefix, ":", runtime.itemSequence));
}

function owned_conversation_item(id, kind, title, body, status, data, execution_id, at, cursor, sequence) {
  return ConversationItem(id, kind, title, body, status, data, execution_id, at, cursor, sequence);
}

function compare_text(left, right) {
  return left.localeCompare(right);
}

function compare_conversation_items(left, right) {
  const at_order = compare_text(conversationitem_at(left), conversationitem_at(right));
  const cursor_order = (conversationitem_cursor(left) - conversationitem_cursor(right));
  const owner_order = compare_text(conversationitem_execution_id(left), conversationitem_execution_id(right));
  const sequence_order = (conversationitem_sequence(left) - conversationitem_sequence(right));
  return (((!(at_order === 0))) ? at_order : ((!(cursor_order === 0))) ? cursor_order : ((!(owner_order === 0))) ? owner_order : ((!(sequence_order === 0))) ? sequence_order : compare_text(conversationitem_id(left), conversationitem_id(right)));
}

function compare_execution_items(left, right) {
  const cursor_order = (conversationitem_cursor(left) - conversationitem_cursor(right));
  const sequence_order = (conversationitem_sequence(left) - conversationitem_sequence(right));
  return (((!(cursor_order === 0))) ? cursor_order : ((!(sequence_order === 0))) ? sequence_order : compare_text(conversationitem_id(left), conversationitem_id(right)));
}

export function project_conversation(items, execution_id, aggregate) {
  const visible = (aggregate ? items.slice() : items.filter((item) => (conversationitem_execution_id(item) === execution_id)));
  return visible.sort((aggregate ? compare_conversation_items : compare_execution_items));
}

function runtime_selected_agent_id(runtime) {
  const agents = bridgesnapshot_agents(snapshot(runtime.model));
  const total = agents.length;
  const selected = Number((runtime.agentIndex || 0));
  return ((total > 0) ? agent_id(agents[Math.max(0, Math.min(selected, (total - 1)))]) : "");
}

function aggregate_transcript_p(runtime) {
  return (text(runtime.transcriptView) === "all");
}

function projected_conversation(runtime) {
  return project_conversation(runtime.conversation, runtime_selected_agent_id(runtime), aggregate_transcript_p(runtime));
}

function transcript_working_p(runtime) {
  const executions = runtime.workingExecutions;
  return (executions ? (aggregate_transcript_p(runtime) ? (executions.size > 0) : executions.has(runtime_selected_agent_id(runtime))) : (runtime.working ? true : false));
}

function mark_execution_working_bang(runtime, execution_id, working) {
  const executions = runtime.workingExecutions;
  if ((executions && (!(execution_id === "")))) {
    return (working ? executions.add(execution_id) : executions.delete(execution_id));
  }
}

function conversation_index(runtime, id) {
  return runtime.conversation.findIndex((item) => (conversationitem_id(item) === id));
}

function conversation_item_by_id(runtime, id) {
  const index = conversation_index(runtime, id);
  return ((index >= 0) ? runtime.conversation[index] : null);
}

function upsert_conversation_bang(runtime, item) {
  const index = conversation_index(runtime, conversationitem_id(item));
  if ((index >= 0)) {
    (runtime.conversation[index] = item);
  } else {
    runtime.conversation.push(item);
  }
  if ((runtime.conversation.length > 240)) {
    (runtime.conversation = project_conversation(runtime.conversation, "", true).slice(-240));
  }
  return runtime.render();
}

function append_system_bang(runtime, body) {
  if ((!(body.trim() === ""))) {
    const id = next_item_id_bang(runtime, "system");
    return upsert_conversation_bang(runtime, owned_conversation_item(id, "system", "", body.trim(), "done", null, runtime_selected_agent_id(runtime), new Date().toISOString(), runtime.itemSequence, 1));
  }
}

function append_error_bang(runtime, body) {
  if ((!(body.trim() === ""))) {
    const id = next_item_id_bang(runtime, "error");
    return upsert_conversation_bang(runtime, owned_conversation_item(id, "error", "Error", body.trim(), "failed", null, runtime_selected_agent_id(runtime), new Date().toISOString(), runtime.itemSequence, 1));
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
  return runtime.renderConversation();
} }, 180);
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
  return runtime.renderConversation();
}

function set_execution_working_bang(runtime, execution_id, working, label) {
  const executions = runtime.workingExecutions;
  mark_execution_working_bang(runtime, execution_id, working);
  if (executions) {
    const any_working_p = (executions.size > 0);
    return set_working_bang(runtime, any_working_p, (any_working_p ? text_or(label, runtime.workingLabel) : ""));
  } else {
    return set_working_bang(runtime, working, label);
  }
}

function command(input) {
  const trimmed = input.trim();
  const normalized = (trimmed.startsWith("/") ? trimmed.slice(1) : trimmed);
  const split_at = normalized.indexOf(" ");
  return ((split_at < 0) ? ParsedCommand(normalized.toLowerCase(), "") : ParsedCommand(normalized.slice(0, split_at).toLowerCase(), normalized.slice((split_at + 1)).trim()));
}

export function quit_command_p(name) {
  return ((name === "q") || (name === "exit"));
}

function escape_command_p(name) {
  return ((name === "close") || (name === "esc"));
}

const BOOT_FRAME = "agents";

export function boot_frame() {
  return BOOT_FRAME;
}

export function threads_frame_p(frame) {
  return (text(frame) === "threads");
}

export function escape_rung(palette_open_p, filtering_p, detail_open_p, strip_focused_p, threads_p, working_p) {
  return ((palette_open_p) ? "close-palette" : (filtering_p) ? "clear-filter" : (detail_open_p) ? "close-detail" : (strip_focused_p) ? "focus-composer" : (threads_p) ? "show-agents" : (working_p) ? "cancel-turn" : "");
}

export function active_focus(palette_open_p, panel_open_p, panel_focused_p, filtering_p, strip_focused_p) {
  return ((palette_open_p) ? "palette" : ((panel_open_p && panel_focused_p && filtering_p)) ? "filter" : ((panel_open_p && panel_focused_p)) ? "panel" : (strip_focused_p) ? "strip" : "composer");
}

export function tab_action(focus, dir_row_p, expanded_p) {
  return (((focus === "palette")) ? "complete" : (((focus === "panel") || (focus === "filter"))) ? (dir_row_p ? (expanded_p ? "collapse" : "expand") : "climb") : "swap-view");
}

export function tab_swap_frame(frame) {
  return (threads_frame_p(frame) ? "agents" : "threads");
}

function emoji_options(query) {
  const needle = query.trim().toLowerCase();
  return EMOJI_COMMANDS.filter((candidate) => ((needle === "") || ("".concat(slashcommand_name(candidate), " ", slashcommand_description(candidate))).toLowerCase().includes(needle)));
}

function glyph_options(query) {
  const needle = query.trim().toLowerCase();
  return GLYPH_COMMANDS.filter((candidate) => ((needle === "") || ("".concat(slashcommand_name(candidate), " ", slashcommand_description(candidate))).toLowerCase().includes(needle)));
}

export function palette_options(frame, input) {
  const query = input.trim().toLowerCase();
  const parsed = command(input);
  const name = parsedcommand_name(parsed);
  const commands = (threads_frame_p(frame) ? THREAD_COMMANDS : AGENT_COMMANDS);
  return (((!query.startsWith("/"))) ? [] : ((name === "emoji")) ? emoji_options(parsedcommand_rest(parsed)) : (((name === "glyph") || (name === "prompt"))) ? glyph_options(parsedcommand_rest(parsed)) : ((query.indexOf(" ") >= 0)) ? [] : commands.filter((candidate) => slashcommand_name(candidate).startsWith(query)));
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
  return ((exit_code === 0) ? stdout : (() => { throw new Error(("".concat(argv.join(" "), " failed (", exit_code, "): ", text_or(stderr.trim(), stdout.trim())))); })());
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

export function normalize_agents(payload) {
  const rows = ((payload && Array.isArray(payload.agents)) ? payload.agents : []);
  return rows.map((row) => { const id = bare(text_or(row.control_id, text_or(row.uuid, text(row.id))));
const name = text_or(row.display_handle, text_or(row.display_name, id));
const status = text_or(row.state_label, text_or(row.state, "unknown"));
const task = text_or(row.task, text(row.thread_title));
return Agent(id, name, status, task, text(row.provider), text(row.provider_target), text(row.provider_label), text(row.model), text(row.model_display), text(row.effort), text(row.orchestration_provenance), text(row.goal), text(row.state)); });
}

function agent_with_route(agent, route) {
  return Agent(agent_id(agent), agent_name(agent), agent_status(agent), agent_task(agent), agent_provider(route), agent_provider_target(route), agent_provider_label(route), agent_model(route), agent_model_display(route), agent_effort(route), agent_orchestration_provenance(route), agent_goal(route), agent_state(route));
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
const title = text_or(row.title, id);
const body = body_of(facts, id);
const condition = text_or(row.condition, "open");
const driver = driver_of(facts, id);
const dependencies = dependencies_of(facts, id);
return WorkItem(id, title, body, condition, driver, dependencies); }));
}

function terminal_condition_p(condition) {
  return ((condition === "terminal") || (condition === "done") || (condition === "completed") || (condition === "failed"));
}

function list_section_id(condition) {
  return ((terminal_condition_p(condition)) ? "terminal" : (["active", "ready", "blocked", "dormant", "draft"].includes(condition)) ? condition : "other");
}

function list_section_title(section_id) {
  const section = LIST_SECTIONS.find((candidate) => (listsection_id(candidate) === section_id));
  return (section ? listsection_title(section) : section_id);
}

function list_section_count(items, section_id) {
  return items.filter((item) => (list_section_id(workitem_condition(item)) === section_id)).length;
}

function list_rows(runtime, items) {
  const rows = [];
  const collapsed = runtime.collapsedListConditions;
  LIST_SECTIONS.forEach((section) => { const section_id = listsection_id(section);
const count = list_section_count(items, section_id);
if ((count > 0)) {
  rows.push(ListRow("header", section_id, -1, count));
  if ((!collapsed.has(section_id))) {
    return items.forEach((item, index) => { if ((list_section_id(workitem_condition(item)) === section_id)) {
  return rows.push(ListRow("item", section_id, index, 0));
} });
  }
} });
  return rows;
}

function board_lane_id(condition) {
  return (((condition === "active")) ? "in-progress" : (terminal_condition_p(condition)) ? "done" : "not-started");
}

function ordered_board_items(open_items, done_items) {
  const ready = open_items.filter((item) => (workitem_condition(item) === "ready"));
  const waiting = open_items.filter((item) => { const condition = workitem_condition(item);
return ((!(condition === "ready")) && (!(condition === "active"))); });
  const active = open_items.filter((item) => (workitem_condition(item) === "active"));
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
  const north_home = text(process.env.NORTH_HOME).trim();
  return (((!(directory === ""))) ? directory : ((!(north_home === ""))) ? ("".concat(north_home, "/../warcraft-sounds")) : "warcraft-sounds");
}

function discover_sound_player() {
  const mpv = Bun.which("mpv");
  const ffplay = Bun.which("ffplay");
  const pw_play = Bun.which("pw-play");
  return ((mpv) ? SoundPlayer("mpv", text(mpv)) : (ffplay) ? SoundPlayer("ffplay", text(ffplay)) : (pw_play) ? SoundPlayer("pw-play", text(pw_play)) : null);
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
  const executable = soundplayer_path(player);
  return (((soundplayer_kind(player) === "mpv")) ? [executable, "--no-video", "--really-quiet", path] : ((soundplayer_kind(player) === "ffplay")) ? [executable, "-nodisp", "-autoexit", "-loglevel", "quiet", path] : [executable, path]);
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
  const pack = ((text(runtime.soundPack) === "peasant") ? PEASANT_SOUND_PACK : PEON_SOUND_PACK);
  const files = (((event === "ready")) ? soundpack_ready(pack) : ((event === "done")) ? soundpack_done(pack) : ((event === "interrupted")) ? soundpack_interrupted(pack) : ((event === "failed")) ? soundpack_failed(pack) : []);
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
  return ("".concat("sound ", (runtime.soundEnabled ? "on" : "off"), " · pack ", text(runtime.soundPack), " · player ", (player ? soundplayer_kind(player) : "none"), " · ", text(runtime.soundDirectory)));
}

function ConfigEntry(kind, name, state, detail) {
  return Object.freeze({_tag: "ConfigEntry", kind, name, state, detail});
}

function configentry_kind(r) { return r.kind; }

function configentry_name(r) { return r.name; }

function configentry_state(r) { return r.state; }

function configentry_detail(r) { return r.detail; }

function config_hook_enabled_p(state) {
  return (state === "enabled");
}

function ModuleMembership(module, members) {
  return Object.freeze({_tag: "ModuleMembership", module, members});
}

function modulemembership_module(r) { return r.module; }

function modulemembership_members(r) { return r.members; }

function config_owner_modules(memberships, name) {
  return memberships.filter((membership) => modulemembership_members(membership).includes(name)).map((membership) => modulemembership_module(membership));
}

export function config_module_members(memberships, name) {
  const found = memberships.find((membership) => (modulemembership_module(membership) === name));
  return (found ? modulemembership_members(found) : null);
}

function config_subtree_kind_p(kind) {
  return ((kind === "ins") || (kind === "memroot") || (kind === "mem"));
}

function config_row_slug(kind, name) {
  if ((kind === "mem")) {
    const cut = name.indexOf("/");
    return ((cut < 0) ? name : name.slice(0, cut));
  } else {
    return name;
  }
}

function config_mem_name(name) {
  const cut = name.indexOf("/");
  return ((cut < 0) ? name : name.slice((cut + 1)));
}

export function config_kind_word(kind) {
  return (((kind === "ins")) ? "file" : ((kind === "memroot")) ? "memories" : ((kind === "mem")) ? "memory" : kind);
}

export function config_provenance_name(kind, name) {
  return ((kind === "") ? name : ("".concat(config_kind_word(kind), ": ", name)));
}

export function config_row_label(kind, name) {
  return (((kind === "ins")) ? "AGENTS.md" : ((kind === "memroot")) ? "MEMORIES" : ((kind === "mem")) ? config_mem_name(name) : name);
}

export function config_cli_name(kind, name) {
  return (((kind === "ins")) ? ("".concat(name, "/AGENTS.md")) : ((kind === "memroot")) ? ("".concat(name, "/memories")) : name);
}

export function config_reference_text(kind, name) {
  return ("".concat("@", config_kind_word(kind), ":", config_cli_name(kind, name), " "));
}

function config_find_entry(manifest, name) {
  return manifest.find((entry) => (configentry_name(entry) === name));
}

function config_find_kind(manifest, kind, name) {
  return manifest.find((entry) => ((configentry_kind(entry) === kind) && (configentry_name(entry) === name)));
}

function config_find_companion(manifest, name) {
  return manifest.find((entry) => ((!(configentry_kind(entry) === "hook")) && (configentry_name(entry) === name)));
}

function config_subtree_gate(manifest, kind, name) {
  const slug = config_row_slug(kind, name);
  return ((((kind === "ins") || (kind === "memroot"))) ? config_find_kind(manifest, "dir", slug) : ((kind === "mem")) ? (() => { const root = config_find_kind(manifest, "memroot", slug); return (root ? root : config_find_kind(manifest, "dir", slug)); })() : null);
}

function config_active_along_p(entry, manifest, memberships, trail) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const key = ("".concat(kind, " ", name));
  if (trail.includes(key)) {
    return false;
  } else {
    const walked = trail.concat([key]);
    const hook_p = (kind === "hook");
    const state = configentry_state(entry);
    const own_p = (hook_p ? config_hook_enabled_p(state) : (state === "on"));
    const owners = config_owner_modules(memberships, name);
    const gated_p = ((owners.length === 0) || owners.some((owner) => { const row = config_find_kind(manifest, "module", owner);
return (row ? config_active_along_p(row, manifest, memberships, walked) : false); }));
    const companion = (hook_p ? text(configentry_detail(entry)) : "");
    const followed_p = ((companion === "") ? true : (() => { const row = config_find_companion(manifest, companion); return (row ? config_active_along_p(row, manifest, memberships, walked) : false); })());
    const gate = config_subtree_gate(manifest, kind, name);
    const scoped_p = (gate ? config_active_along_p(gate, manifest, memberships, walked) : true);
    return (own_p && gated_p && followed_p && scoped_p);
  }
}

export function config_entry_active_p(entry, manifest, memberships) {
  return config_active_along_p(entry, manifest, memberships, []);
}

export function config_unit_active_p(manifest, memberships, name) {
  const entry = config_find_entry(manifest, name);
  return (entry ? config_entry_active_p(entry, manifest, memberships) : false);
}

export function config_gate_modules(entry, manifest, memberships) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const gate = config_subtree_gate(manifest, kind, name);
  const scope = ((gate && (!config_entry_active_p(gate, manifest, memberships))) ? [config_provenance_name(configentry_kind(gate), configentry_name(gate))] : []);
  const owners = config_owner_modules(memberships, name);
  const open_p = ((owners.length === 0) || owners.some((owner) => { const row = config_find_kind(manifest, "module", owner);
return (row ? config_entry_active_p(row, manifest, memberships) : false); }));
  return scope.concat((open_p ? [] : owners.map((owner) => config_provenance_name("module", owner))));
}

export function config_state_text(entry, manifest, memberships, active_p, nested_p) {
  const state = configentry_state(entry);
  const hook_p = (configentry_kind(entry) === "hook");
  const own_p = (hook_p ? config_hook_enabled_p(state) : (state === "on"));
  const gates = config_gate_modules(entry, manifest, memberships);
  const gate_note = ((gates.length === 0) ? "" : ("".concat(" (", gates.join(", "), " off)")));
  const companion = (hook_p ? text(configentry_detail(entry)) : "");
  const followed = config_find_companion(manifest, companion);
  const provenance = ((nested_p || (companion === "")) ? "" : ("".concat(" · ", config_provenance_name((followed ? configentry_kind(followed) : ""), companion))));
  const claimant_off_p = (followed && (!config_entry_active_p(followed, manifest, memberships)));
  const reason = (((!(gate_note === ""))) ? gate_note : (claimant_off_p) ? ("".concat(" (", config_provenance_name(configentry_kind(followed), companion), " off)")) : "");
  return (((hook_p && (!own_p))) ? "disabled" : ((!own_p)) ? "off" : (active_p) ? ("".concat("on", provenance)) : ("".concat("off", reason)));
}

export function config_toggle_verb(state) {
  return (((state === "on") || (state === "enabled")) ? "off" : "on");
}

const GLOBAL_DIR_NAME = "global";

function config_global_row_p(kind, name) {
  return ((kind === "dir") && (name === GLOBAL_DIR_NAME));
}

export function config_row_scope(kind, name) {
  return (((kind === "dir")) ? name : (config_subtree_kind_p(kind)) ? config_row_slug(kind, name) : GLOBAL_DIR_NAME);
}

export function config_view_includes_p(view, kind, name) {
  return (((view === "all")) ? true : ((kind === "dir")) ? ((view === "globals") ? (name === GLOBAL_DIR_NAME) : true) : ((view === "globals")) ? ((config_row_scope(kind, name) === GLOBAL_DIR_NAME) && (!(kind === "plugin"))) : ((view === "agentsmd")) ? config_subtree_kind_p(kind) : (kind === view));
}

function config_view_prunes_p(view) {
  return (!((view === "all") || (view === "globals") || (view === "agentsmd")));
}

export function config_view_folds_p(view) {
  return (view === "all");
}

function config_hook_companion(entry) {
  return ((configentry_kind(entry) === "hook") ? text(configentry_detail(entry)) : "");
}

export function config_skill_hooks(rows, name) {
  return rows.filter((entry) => ((configentry_kind(entry) === "hook") && (config_hook_companion(entry) === name)));
}

export function config_row_role(entry, rows) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  return (((kind === "module")) ? "moduleset" : ((kind === "skill")) ? ((config_skill_hooks(rows, name).length > 0) ? "module" : "skill") : ((kind === "hook")) ? (() => { const companion = config_hook_companion(entry); return (((!(companion === "")) && rows.some((row) => ((configentry_kind(row) === "skill") && (configentry_name(row) === companion)))) ? "boundhook" : "hook"); })() : kind);
}

function config_section_rank(role) {
  return (((role === "ins")) ? 0 : ((role === "memroot")) ? 1 : ((role === "mem")) ? 2 : ((role === "moduleset")) ? 3 : ((role === "module")) ? 4 : ((role === "boundhook")) ? 5 : ((role === "skill")) ? 6 : ((role === "hook")) ? 7 : ((role === "plugin")) ? 8 : ((role === "other")) ? 9 : 10);
}

function config_node_rank(entry) {
  return (config_global_row_p(configentry_kind(entry), configentry_name(entry)) ? 0 : 1);
}

function config_tree_rows(entries) {
  const dirs = entries.filter((entry) => (configentry_kind(entry) === "dir"));
  const kids = entries.filter((entry) => (!(configentry_kind(entry) === "dir")));
  const ordered = dirs.slice().sort((a, b) => (config_node_rank(a) - config_node_rank(b)));
  const tree = [];
  ordered.forEach((node) => { tree.push(node);
const slug = configentry_name(node);
const owned = kids.filter((child) => (config_row_scope(configentry_kind(child), configentry_name(child)) === slug));
const sorted = owned.sort((a, b) => (config_section_rank(config_row_role(a, entries)) - config_section_rank(config_row_role(b, entries))));
return sorted.forEach((child) => { if ((!(config_row_role(child, entries) === "boundhook"))) {
  tree.push(child);
  if ((config_row_role(child, entries) === "module")) {
    return config_skill_hooks(sorted, configentry_name(child)).forEach((hook) => tree.push(hook));
  }
} }); });
  return tree.concat(kids.filter((child) => (!tree.includes(child))));
}

export function config_view_rows(entries, view) {
  const kept = entries.filter((entry) => config_view_includes_p(view, configentry_kind(entry), configentry_name(entry)));
  const held = kept.filter((entry) => (((!config_view_prunes_p(view)) || (!(configentry_kind(entry) === "dir"))) ? true : kept.some((child) => ((!(configentry_kind(child) === "dir")) && (config_row_scope(configentry_kind(child), configentry_name(child)) === configentry_name(entry))))));
  return config_tree_rows(held);
}

export function config_node_expanded_p(expanded, slug) {
  return (expanded ? expanded.includes(slug) : false);
}

export function config_fold_rows(entries, expanded) {
  const nodes = entries.filter((entry) => (configentry_kind(entry) === "dir")).map((entry) => configentry_name(entry));
  return entries.filter((entry) => { const kind = configentry_kind(entry);
const scope = config_row_scope(kind, configentry_name(entry));
return ((kind === "dir") || (!nodes.includes(scope)) || config_node_expanded_p(expanded, scope)); });
}

export function config_row_search_text(entry) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  return ("".concat(config_row_label(kind, name), " ", name, " ", text(configentry_detail(entry)))).toLowerCase();
}

export function config_row_matches_p(entry, query) {
  const needle = query.trim().toLowerCase();
  return ((needle === "") || config_row_search_text(entry).includes(needle));
}

function config_matched_slugs(entries, query, kind) {
  return entries.filter((entry) => ((configentry_kind(entry) === kind) && config_row_matches_p(entry, query))).map((entry) => config_row_scope(configentry_kind(entry), configentry_name(entry)));
}

export function config_query_rows(entries, query) {
  if ((query.trim() === "")) {
    return entries;
  } else {
    const open_nodes = config_matched_slugs(entries, query, "dir");
    const open_mems = config_matched_slugs(entries, query, "memroot");
    const held = entries.filter((entry) => config_row_matches_p(entry, query)).map((entry) => config_row_scope(configentry_kind(entry), configentry_name(entry)));
    const held_mems = entries.filter((entry) => ((configentry_kind(entry) === "mem") && config_row_matches_p(entry, query))).map((entry) => config_row_scope(configentry_kind(entry), configentry_name(entry)));
    const held_skills = entries.filter((entry) => ((configentry_kind(entry) === "hook") && config_row_matches_p(entry, query))).map((entry) => config_hook_companion(entry));
    const open_skills = entries.filter((entry) => ((configentry_kind(entry) === "skill") && config_row_matches_p(entry, query))).map((entry) => configentry_name(entry));
    return entries.filter((entry) => { const kind = configentry_kind(entry);
const scope = config_row_scope(kind, configentry_name(entry));
return (config_row_matches_p(entry, query) || open_nodes.includes(scope) || ((kind === "mem") && open_mems.includes(scope)) || ((kind === "dir") && held.includes(scope)) || ((kind === "memroot") && held_mems.includes(scope)) || ((kind === "skill") && held_skills.includes(configentry_name(entry))) || ((kind === "hook") && open_skills.includes(config_hook_companion(entry)))); });
  }
}

export function config_row_context_only_p(entry, query) {
  return ((!(query.trim() === "")) && (!config_row_matches_p(entry, query)));
}

function config_manifest_path() {
  return ("".concat(text(process.env.HOME), "/.config/agents/manifest.conf"));
}

function config_modules_dir(runtime) {
  return text_or(text(runtime.configModulesDir), ("".concat(text(process.env.HOME), "/code/nixos-config/main/dotfiles/agents/modules.d")));
}

export function config_membership_of_json(module, content) {
  return (() => { try {
    const parsed = JSON.parse(content);
  const members = ((parsed && Array.isArray(parsed.members)) ? parsed.members : []);
  return ModuleMembership(module, members.map((member) => text(member)));
  } catch (__) {
    return ModuleMembership(module, []);
  } })();
}

async function list_module_files_bang(directory) {
  return (async () => { try {
    const listing = await run_command(["ls", directory]);
  return listing.trim().split("\n").filter((name) => name.endsWith(".json"));
  } catch (__) {
    return [];
  } })();
}

async function read_module_file_bang(path) {
  return (async () => { try {
    return await run_command(["cat", path]);
  } catch (__) {
    return "";
  } })();
}

export async function load_config_memberships_bang(directory) {
  const files = await list_module_files_bang(directory);
  const contents = await Promise.all(files.map((file) => read_module_file_bang(("".concat(directory, "/", file)))));
  return files.map((file, index) => config_membership_of_json(file.slice(0, (file.length - 5)), text(contents[index])));
}

async function ensure_config_manifest_bang() {
  return (async () => { try {
    return await run_command(["test", "-f", config_manifest_path()]);
  } catch (__) {
    return await run_command([AGENTS_BIN, "status"]);
  } })();
}

function panel_filtering_p(runtime) {
  return (runtime.panelFiltering ? true : false);
}

function panel_query(runtime) {
  return (panel_filtering_p(runtime) ? text(runtime.panelQuery) : "");
}

const PANEL_DEFAULT_EXPANDED = [GLOBAL_DIR_NAME];

function panel_expanded(runtime) {
  const stored = runtime.expandedDirs;
  return (stored ? stored : PANEL_DEFAULT_EXPANDED);
}

export function config_panel_rows(runtime) {
  const stored = runtime.configEntries;
  const entries = (stored ? stored : []);
  const query = panel_query(runtime);
  const view = text_or(text(runtime.configFilter), "all");
  return ((!(query.trim() === "")) ? config_query_rows(entries, query) : (config_view_folds_p(view) ? config_fold_rows(entries, panel_expanded(runtime)) : entries));
}

export function config_row_node(entry) {
  return config_row_scope(configentry_kind(entry), configentry_name(entry));
}

export function clamp_panel_cursor_bang(runtime) {
  const total = config_panel_rows(runtime).length;
  const raw = runtime.configIndex;
  const current = (raw ? raw : 0);
  return (runtime.configIndex = ((total > 0) ? Math.max(0, Math.min(current, (total - 1))) : 0));
}

export function set_panel_query_bang(runtime, query) {
  (runtime.panelFiltering = true);
  (runtime.panelQuery = query);
  return clamp_panel_cursor_bang(runtime);
}

export function clear_panel_filter_bang(runtime) {
  (runtime.panelFiltering = false);
  (runtime.panelQuery = "");
  return clamp_panel_cursor_bang(runtime);
}

export function set_node_expanded_bang(runtime, slug, open_p) {
  const current = panel_expanded(runtime);
  const without = current.filter((held) => (!(held === slug)));
  (runtime.expandedDirs = (open_p ? without.concat([slug]) : without));
  return clamp_panel_cursor_bang(runtime);
}

export function fold_key_action(dir_row_p, expanded_p, open_key_p) {
  return (((open_key_p && dir_row_p)) ? (expanded_p ? "" : "expand") : (open_key_p) ? "" : (dir_row_p) ? (expanded_p ? "collapse" : "") : "climb");
}

async function load_config_entries_bang(runtime) {
  await ensure_config_manifest_bang();
  const content = await run_command(["cat", config_manifest_path()]);
  const memberships = await load_config_memberships_bang(config_modules_dir(runtime));
  const config_filter = text_or(text(runtime.configFilter), "all");
  const lines = content.split("\n").filter((line) => (!(line.trim() === "")));
  const all_entries = lines.map((line) => { const parts = line.trim().split(" ").filter((part) => (!(part === "")));
return ConfigEntry(text(parts[0]), text(parts[1]), text(parts[2]), parts.slice(3).join(" ")); });
  const entries = config_view_rows(all_entries, config_filter);
  (runtime.configAllEntries = all_entries);
  (runtime.configMemberships = memberships);
  (runtime.configEntries = entries);
  clamp_panel_cursor_bang(runtime);
  (runtime.configLoaded = true);
  return runtime.render();
}

function focus_panel_bang(runtime, ui) {
  (runtime.panelFocused = true);
  return ui.composerInput.blur();
}

function focus_composer_bang(runtime, ui) {
  (runtime.panelFocused = false);
  return ui.composerInput.focus();
}

function panel_focused_p(runtime) {
  return (detail_open_p(runtime) && (!(runtime.panelFocused === false)));
}

function open_config_panel_bang(runtime, ui, config_filter) {
  const already = (detail_showing_p(runtime, "config") && (text(runtime.configFilter) === config_filter));
  if (already) {
    close_detail_bang(runtime);
    focus_composer_bang(runtime, ui);
  } else {
    focus_panel_bang(runtime, ui);
    (runtime.configFilter = config_filter);
    (runtime.configIndex = 0);
    (runtime.configLoaded = false);
    clear_panel_filter_bang(runtime);
    open_detail_bang(runtime, "config");
    report_promise_bang(runtime, load_config_entries_bang(runtime));
  }
  return runtime.render();
}

async function edit_config_entry_bang(runtime) {
  const entries = config_panel_rows(runtime);
  if ((entries.length > 0)) {
    const raw = runtime.configIndex;
    const index = Math.max(0, Math.min((raw ? raw : 0), (entries.length - 1)));
    const entry = entries[index];
    const raw_path = await run_command([AGENTS_BIN, "path", config_cli_name(configentry_kind(entry), configentry_name(entry))]);
    const path = raw_path.trim();
    const editor = text_or(text(process.env.EDITOR), "vi");
    const ghostty = Bun.which("ghostty");
    const kitty = Bun.which("kitty");
    const foot = Bun.which("foot");
    const xterm = Bun.which("xterm");
    const argv = ((ghostty) ? [ghostty, "-e", editor, path] : (kitty) ? [kitty, "--detach", editor, path] : (foot) ? [foot, editor, path] : (xterm) ? [xterm, "-e", editor, path] : null);
    if ((argv == null)) {
      (() => { throw new Error("no supported terminal found for edit"); })();
    }
    const child = Bun.spawn({cmd: argv, stdin: "ignore", stdout: "ignore", stderr: "ignore"});
    child.unref();
    return publish_line_bang(runtime, ("".concat("editing ", path)));
  }
}

async function toggle_config_entry_bang(runtime) {
  const entries = config_panel_rows(runtime);
  if ((entries.length > 0)) {
    const raw = runtime.configIndex;
    const index = Math.max(0, Math.min((raw ? raw : 0), (entries.length - 1)));
    const entry = entries[index];
    const verb = config_toggle_verb(configentry_state(entry));
    await run_command([AGENTS_BIN, verb, config_cli_name(configentry_kind(entry), configentry_name(entry))]);
    await load_config_entries_bang(runtime);
    return runtime.render();
  }
}

function handle_sound_command_bang(runtime, rest) {
  const request = rest.trim().toLowerCase();
  return ((((request === "") || (request === "status"))) ? publish_line_bang(runtime, sound_status(runtime)) : ((request === "on")) ? (() => { (runtime.soundEnabled = true);
publish_line_bang(runtime, sound_status(runtime));
if ((runtime.soundPlayer == null)) {
  return sound_warning_bang(runtime, "install mpv, ffplay, or pw-play to play local assets");
} })() : ((request === "off")) ? (() => { (runtime.soundEnabled = false);
return publish_line_bang(runtime, sound_status(runtime)); })() : (request.startsWith("pack ")) ? (() => { const pack = request.slice(5).trim(); if ((!((pack === "peon") || (pack === "peasant")))) {
  (() => { throw new Error("sound pack must be peon or peasant"); })();
}
(runtime.soundPack = pack);
return publish_line_bang(runtime, sound_status(runtime)); })() : (() => { throw new Error("sound requires on, off, status, or pack peon|peasant"); })());
}

function forget_control_session_bang(runtime) {
  const id = text(runtime.supervisorId);
  if ((!(id === ""))) {
    runtime.bridgeExecutions.delete(id);
    (runtime.model = remove_agent(runtime.model, id));
    (runtime.supervisorId = "");
    (runtime.agentIndex = 0);
    return runtime.render();
  }
}

async function restart_daemon_bang(runtime) {
  return (async () => { try {
    await run_command([north_bin(), "bridge", "restart"]);
  forget_control_session_bang(runtime);
  publish_line_bang(runtime, "control daemon replaced; session restored");
  return await launch_agent_bang(runtime, SUPERVISOR_BOOT_PROMPT, "supervisor");
  } catch (error) {
    return append_error_bang(runtime, error_message(error));
  } })();
}

export function handle_local_command_bang(runtime, ui, input) {
  const trimmed = input.trim();
  if ((!trimmed.startsWith("/"))) {
    return false;
  } else {
    const parsed = command(trimmed);
    const name = parsedcommand_name(parsed);
    const rest = parsedcommand_rest(parsed);
    return ((((name === "glyph") || (name === "prompt"))) ? (() => { if ((rest.toLowerCase() === "reset")) {
  set_prompt_glyph_bang(runtime, DEFAULT_PROMPT_GLYPH);
} else {
  set_prompt_glyph_bang(runtime, rest);
}
return true; })() : ((name === "emoji")) ? (() => { const options = emoji_options(rest); if ((options.length === 0)) {
  (() => { throw new Error(("".concat("no emoji matches ", rest))); })();
}
const input_renderable = ui.composerInput;
(input_renderable.value = slashcommand_completion(options[0]));
input_renderable.focus();
runtime.render();
return true; })() : ((name === "sound")) ? (() => { handle_sound_command_bang(runtime, rest);
return true; })() : ((name === "mute")) ? (() => { (runtime.soundEnabled = false);
publish_line_bang(runtime, sound_status(runtime));
return true; })() : ((name === "transcript")) ? (() => { const requested = rest.trim().toLowerCase(); if ((!((requested === "selected") || (requested === "all")))) {
  (() => { throw new Error("transcript requires selected or all"); })();
}
(runtime.transcriptView = requested);
runtime.render();
return true; })() : (((name === "provider") || (name === "model") || (name === "effort"))) ? (() => { set_launch_route_bang(runtime, name, rest);
return true; })() : ((name === "config")) ? (() => { open_config_panel_bang(runtime, ui, "all");
return true; })() : ((name === "hooks")) ? (() => { open_config_panel_bang(runtime, ui, "hook");
return true; })() : ((name === "skills")) ? (() => { open_config_panel_bang(runtime, ui, "skill");
return true; })() : ((name === "mcp")) ? (() => { const parts = rest.trim().split(/\\s+/).filter((part) => (!(part === "")));
run_command([north_bin(), "config", "mcp"].concat(parts)).then((output) => publish_line_bang(runtime, output.trim())).catch((error) => publish_line_bang(runtime, ("".concat("error: ", error_message(error)))));
return true; })() : ((name === "plugins")) ? (() => { open_config_panel_bang(runtime, ui, "plugin");
return true; })() : ((name === "modules")) ? (() => { open_config_panel_bang(runtime, ui, "module");
return true; })() : ((name === "globals")) ? (() => { open_config_panel_bang(runtime, ui, "globals");
return true; })() : ((name === "agentsmd")) ? (() => { open_config_panel_bang(runtime, ui, "agentsmd");
return true; })() : ((name === "restart")) ? (() => { restart_daemon_bang(runtime);
return true; })() : ((name === "agents")) ? (() => { show_frame_bang(runtime, ui, "agents");
return true; })() : ((name === "threads")) ? (() => { if ((rest.trim().toLowerCase() === "popout")) {
  popout_bang(runtime, text(runtime.activeView));
} else {
  show_frame_bang(runtime, ui, "threads");
}
return true; })() : (thread_view_command_p(name)) ? (() => { show_thread_view_bang(runtime, ui, name);
return true; })() : ((name === "help")) ? (() => { toggle_help_bang(runtime, ui);
return true; })() : (quit_command_p(name)) ? (() => { destroy_bang(runtime);
return true; })() : (escape_command_p(name)) ? (() => { escape_step_bang(runtime, ui);
return true; })() : false);
  }
}

function render_after_suspend_bang(runtime) {
  if (((!runtime.disposed) && runtime.render)) {
    return runtime.render();
  }
}

export function cleanup_suspend_bang(runtime, process_api) {
  const handler = runtime.suspendResume;
  const suspended_p = (runtime.rendererSuspended ? true : false);
  const active_p = (handler || suspended_p);
  if (handler) {
    process_api.removeListener("SIGCONT", handler);
  }
  (runtime.suspendResume = null);
  (runtime.rendererSuspended = false);
  if (suspended_p) {
    (() => { try {
    return runtime.renderer.resume();
  } catch (error) {
    return (runtime.suspendError = error_message(error));
  } })();
  }
  render_after_suspend_bang(runtime);
  return (active_p ? true : false);
}

export function suspend_runtime_bang(runtime, platform, process_api) {
  if (((platform === "win32") || runtime.suspendResume)) {
    return false;
  } else {
    const state = {handler: null};
    const resume = () => { const handler = state.handler;
if ((runtime.suspendResume === handler)) {
  return cleanup_suspend_bang(runtime, process_api);
} };
    (state.handler = resume);
    (runtime.suspendResume = resume);
    (runtime.suspendError = "");
    return (() => { try {
    process_api.once("SIGCONT", resume);
  (runtime.rendererSuspended = true);
  runtime.renderer.suspend();
  process_api.kill(0, "SIGSTOP");
  return true;
  } catch (error) {
    (runtime.suspendError = error_message(error));
    cleanup_suspend_bang(runtime, process_api);
    return false;
  } })();
  }
}

function quiesce_bang(runtime) {
  if ((!runtime.disposed)) {
    (runtime.disposed = true);
    cleanup_suspend_bang(runtime, process);
    if (runtime.spinnerTimer) {
      clearInterval(runtime.spinnerTimer);
    }
    (runtime.spinnerTimer = null);
    runtime.soundChildren.forEach((child) => (() => { try {
    return child.kill();
  } catch (__) {
    return null;
  } })());
    return runtime.soundChildren.clear();
  }
}

function destroy_bang(runtime) {
  if ((!runtime.disposed)) {
    quiesce_bang(runtime);
    runtime.renderer.destroy();
    return process.exit(0);
  }
}

function install_process_cleanup_bang(runtime) {
  ["SIGINT", "SIGTERM", "SIGHUP"].forEach((signal) => {
  process.prependOnceListener(signal, () => quiesce_bang(runtime));
});
}

function install_global_exit_bang(runtime) {
  return runtime.renderer.keyInput.on("keypress", (key) => { const name = text(key.name);
return (((key.ctrl && (name === "z"))) ? (() => { key.preventDefault();
key.stopPropagation();
return suspend_runtime_bang(runtime, text(process.platform), process); })() : ((key.ctrl && (name === "q"))) ? (() => { key.preventDefault();
key.stopPropagation();
return destroy_bang(runtime); })() : null); });
}

export function selected_agent_id(state, selected) {
  const agents = bridgesnapshot_agents(state);
  const total = agents.length;
  return ((total > 0) ? agent_id(agents[Math.max(0, Math.min(selected, (total - 1)))]) : "");
}

export function reconcile_agent_selection_bang(runtime, prior_id) {
  const state = snapshot(runtime.model);
  const agents = bridgesnapshot_agents(state);
  const total = agents.length;
  const found = agents.findIndex((agent) => (agent_id(agent) === prior_id));
  const fallback = Math.max(0, Math.min(runtime.agentIndex, Math.max(0, (total - 1))));
  const index = ((found >= 0) ? found : fallback);
  const selected = ((total > 0) ? agent_id(agents[index]) : "");
  (runtime.agentIndex = index);
  if ((!(bridgesnapshot_selected_agent(state) === selected))) {
    (runtime.model = select_agent(runtime.model, selected));
  }
  return selected;
}

export async function refresh_bang(runtime) {
  const payloads = await Promise.all([run_json([north_bin(), "agents", "--json"]).catch((__) => null), run_json([north_bin(), "json", "board", "--all"]).catch((__) => null), run_json([north_bin(), "json", "done"]).catch((__) => null)]);
  const agent_payload = payloads[0];
  const board = payloads[1];
  const done = payloads[2];
  const state = snapshot(runtime.model);
  const current_agents = bridgesnapshot_agents(state);
  const remote_agents = (agent_payload ? normalize_agents(agent_payload) : []);
  const bridge_agents = current_agents.filter((agent) => runtime.bridgeExecutions.has(agent_id(agent))).map((agent) => { const remote = remote_agents.find((candidate) => (agent_id(candidate) === agent_id(agent)));
return (remote ? agent_with_route(agent, remote) : agent); });
  const distinct_remote = remote_agents.filter((agent) => (!runtime.bridgeExecutions.has(agent_id(agent))));
  const agents = (agent_payload ? bridge_agents.concat(distinct_remote) : current_agents);
  const open_rows = (Array.isArray(board) ? board : []);
  const done_rows = (Array.isArray(done) ? done : []);
  const ids = board_ids(open_rows).concat(board_ids(done_rows));
  const facts = ((ids.length > 0) ? await run_json([north_bin(), "json", "show-many", ids.join(",")]).catch((__) => []) : []);
  const work = (Array.isArray(board) ? normalize_work(open_rows, facts) : bridgesnapshot_list(state));
  const prior_terminal = bridgesnapshot_board(state).filter((item) => terminal_condition_p(workitem_condition(item)));
  const terminal_work = (Array.isArray(done) ? normalize_work(done_rows, facts) : prior_terminal);
  const list_work = work.concat(terminal_work);
  const kanban = ordered_board_items(work, terminal_work);
  const next_model = replace_projection(runtime.model, agents, list_work, kanban);
  const prior_agent_id = selected_agent_id(state, runtime.agentIndex);
  const selected_id = bridgesnapshot_selected_thread(state);
  const next_state = snapshot(next_model);
  const next_view = selected_view(next_state, runtime.activeView);
  const next_items = workview_items(next_view);
  const next_index = next_items.findIndex((item) => (workitem_id(item) === selected_id));
  (runtime.model = next_model);
  reconcile_agent_selection_bang(runtime, prior_agent_id);
  if ((next_index >= 0)) {
    (runtime.workIndex = next_index);
    if ((workview_id(next_view) === "list")) {
      runtime.collapsedListConditions.delete(list_section_id(workitem_condition(next_items[next_index])));
    }
  }
  return runtime.render();
}

function canonical_work_view(view_id) {
  return ((((view_id === "graph") || (view_id === "dag"))) ? "graph" : (((view_id === "board") || (view_id === "kanban"))) ? "board" : "list");
}

export function thread_view_command_p(name) {
  return ((name === "list") || (name === "board") || (name === "graph"));
}

export function view_list(state) {
  return [WorkView("list", "List", bridgesnapshot_list(state)), WorkView("board", "Board", bridgesnapshot_board(state)), WorkView("graph", "Graph", bridgesnapshot_list(state))];
}

function selected_view(state, view_id) {
  const views = view_list(state);
  const canonical = canonical_work_view(view_id);
  const selected = views.find((view) => (workview_id(view) === canonical));
  return (selected || views[0]);
}

export function roster_row_suppressed_p(agent_id, supervisor_id, banner_p) {
  return (banner_p && (!(supervisor_id === "")) && (agent_id === supervisor_id));
}

export function roster_text_bang(state, selected, supervisor_id, banner_p) {
  const agents = bridgesnapshot_agents(state);
  const width = Math.max(1, (terminal_columns() - 6));
  const rows = agents.map((agent, index) => ({id: agent_id(agent), text: agent_row_text_bang(agent, (index === selected), width)})).filter((row) => (!roster_row_suppressed_p(row.id, supervisor_id, banner_p)));
  return (((agents.length === 0)) ? "No agents attached" : ((rows.length === 0)) ? "" : rows.map((row) => row.text).join("\n"));
}

export function roster_visible_rows(content) {
  const value = text(content).trim();
  return ((value === "") ? 0 : Math.min(4, value.split("\n").length));
}

function agent_display_name(agent) {
  const name = agent_field_text(agent_name(agent));
  return ((name === "") ? agent_field_text(agent_id(agent)) : name);
}

function agent_summary(agent) {
  const status = agent_field_text(agent_status(agent));
  const task = agent_field_text(agent_task(agent));
  return ("".concat(agent_display_name(agent), ((status === "") ? "" : ("".concat(" (", status, ")"))), ((task === "") ? "" : ("".concat(" — ", task)))));
}

export function agent_row_text_bang(agent, selected_p, width) {
  const prefix = (selected_p ? "› " : "  ");
  return ("".concat(prefix, agent_cell_text_bang(agent_summary(agent), Math.max(1, (width - Bun.stringWidth(prefix))))));
}

function route_provider(agent) {
  const label = agent_field_text(agent_provider_label(agent));
  const provider = agent_field_text(agent_provider(agent));
  const target = agent_field_text(agent_provider_target(agent));
  return (((!(label === ""))) ? label : (((!(provider === "")) && (!(target === "")))) ? ("".concat(provider, ":", target)) : text_or(provider, target));
}

function route_model(agent) {
  return text_or(agent_field_text(agent_model_display(agent)), agent_field_text(agent_model(agent)));
}

export function agent_route_text_bang(agent, width) {
  const provider = route_provider(agent);
  const model = route_model(agent);
  const effort = agent_field_text(agent_effort(agent));
  const provenance = agent_field_text(agent_orchestration_provenance(agent));
  const state = agent_field_text(agent_state(agent));
  const goal = agent_field_text(agent_goal(agent));
  const parts = [((provider === "") ? "" : ("".concat("provider ", provider))), ((model === "") ? "" : ("".concat("model ", model))), ((effort === "") ? "" : ("".concat("effort ", effort))), ((provenance === "") ? "" : provenance), ((state === "") ? "" : ("".concat("state ", state))), ((goal === "") ? "" : ("".concat("goal ", goal)))].filter((part) => (!(part === "")));
  return agent_cell_text_bang(parts.join(" · "), width);
}

function agent_bucket(status) {
  const value = status.trim().toLowerCase();
  return (((value === "")) ? "other" : (value.startsWith("finished(")) ? ((value.includes("process:failed") || value.includes("process:error")) ? "failed" : "other") : (value.startsWith("inconsistent")) ? "blocked" : (((value === "stalled") || (value === "blocked") || (value === "waiting") || (value === "paused") || (value === "queued"))) ? "blocked" : (((value === "failed") || (value === "error") || (value === "crashed"))) ? "failed" : (((value === "working") || (value === "running") || (value === "starting") || (value === "ready") || (value === "active") || (value === "online"))) ? "running" : "other");
}

function agents_in_bucket(agents, bucket) {
  return agents.filter((agent) => (agent_bucket(agent_status(agent)) === bucket));
}

function segment_agents(agents, segment_id) {
  return ((segment_id === "all") ? agents : agents_in_bucket(agents, segment_id));
}

function agent_total_label(total) {
  return ("".concat(total, ((total === 1) ? " agent" : " agents")));
}

function agent_segments(agents) {
  const total = agents.length;
  const segments = [AgentSegment("all", agent_total_label(total), total)];
  STRIP_BUCKETS.forEach((bucket) => { const id = stripbucket_id(bucket);
const members = agents_in_bucket(agents, id);
const count = members.length;
if ((count > 0)) {
  return segments.push(AgentSegment(id, ("".concat(stripbucket_glyph(bucket), count, " ", id)), count));
} });
  return segments;
}

function segment_columns(segments) {
  const starts = [];
  segments.forEach((__segment, index) => starts.push(((index === 0) ? STRIP_INDENT : (starts[(index - 1)] + agentsegment_label(segments[(index - 1)]).length + STRIP_SEPARATOR.length))));
  return starts;
}

function segment_at_column(segments, column) {
  const starts = segment_columns(segments);
  return starts.findIndex((start, index) => ((column >= start) && (column < (start + agentsegment_label(segments[index]).length))));
}

function push_chunk_bang(chunks, chunk) {
  return chunks.push(chunk);
}

function render_command_palette_bang(commands, selected, more) {
  const chunks = [];
  commands.forEach((candidate, index) => { push_chunk_bang(chunks, ((index === selected) ? brightCyan("› ") : brightBlack("  ")));
push_chunk_bang(chunks, (((index === selected) ? brightGreen : brightWhite))(slashcommand_name(candidate).padEnd(13, " ")));
push_chunk_bang(chunks, brightBlack(slashcommand_description(candidate)));
if ((index < (commands.length - 1))) {
  return push_chunk_bang(chunks, white("\n"));
} });
  if ((more > 0)) {
    push_chunk_bang(chunks, white("\n"));
    push_chunk_bang(chunks, brightBlack(("".concat("  … ", more, " more"))));
  }
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

export function apply_frame_visibility_bang(runtime, ui) {
  const threads_p = threads_frame_p(runtime.frame);
  (ui.agentsPane.visible = (!threads_p));
  return (ui.workPane.visible = threads_p);
}

function available_frame_width() {
  return Math.max(24, (terminal_columns() - 6));
}

function available_agent_width(__runtime) {
  return available_frame_width();
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
  return ("".concat(text_or(runtime.sessionModel, "model pending"), " ", text_or(runtime.sessionEffort, "effort pending"), " · ", text_or(short_directory(runtime.sessionCwd), "directory pending"), " · ", text_or(runtime.sessionBranch, "branch pending")));
}

function transcript_context_text(runtime) {
  const selected = runtime_selected_agent_id(runtime);
  return ((aggregate_transcript_p(runtime)) ? "all Bridge executions" : ((selected === text(runtime.supervisorId))) ? session_context_text(runtime) : ((!(selected === ""))) ? ("".concat("Bridge execution ", selected.slice(0, 8))) : "Bridge execution pending");
}

function command_parts(title) {
  const space = title.indexOf(" ");
  return ((space < 0) ? CommandParts(title, "") : CommandParts(title.slice(0, space), title.slice((space + 1))));
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
  push_chunk_bang(chunks, ((((status === "failed")) ? brightRed : (running_p) ? brightYellow : brightGreen))("• "));
  push_chunk_bang(chunks, brightWhite((running_p ? "Running " : "Ran ")));
  push_chunk_bang(chunks, brightCyan(commandparts_executable(parts)));
  if ((!(commandparts_arguments(parts) === ""))) {
    push_chunk_bang(chunks, dim(("".concat(" ", commandparts_arguments(parts)))));
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

function append_diff_row(state, row) {
  return Object.freeze({...state, rows: diffstate_rows(state).concat(row)});
}

function diff_rows(diff) {
  const source = clean_text(diff);
  return ((source === "") ? DiffState(0, 0, 0, 0, []) : source.split("\n").reduce((state, line) => ((line.startsWith("@@ ")) ? (() => { const parts = line.split(" "); return append_diff_row(Object.freeze({...state, old_line: diff_start_line(parts[1]), new_line: diff_start_line(parts[2])}), DiffRow("hunk", "", "", line)); })() : ((line.startsWith("diff --git ") || line.startsWith("index ") || line.startsWith("--- ") || line.startsWith("+++ "))) ? state : (line.startsWith("+")) ? append_diff_row(Object.freeze({...state, new_line: (diffstate_new_line(state) + 1), additions: (diffstate_additions(state) + 1)}), DiffRow("add", "", ("".concat(diffstate_new_line(state))), line)) : (line.startsWith("-")) ? append_diff_row(Object.freeze({...state, old_line: (diffstate_old_line(state) + 1), deletions: (diffstate_deletions(state) + 1)}), DiffRow("delete", ("".concat(diffstate_old_line(state))), "", line)) : (line.startsWith("\\ No newline")) ? append_diff_row(state, DiffRow("meta", "", "", line)) : append_diff_row(Object.freeze({...state, old_line: (diffstate_old_line(state) + 1), new_line: (diffstate_new_line(state) + 1)}), DiffRow("context", ("".concat(diffstate_old_line(state))), ("".concat(diffstate_new_line(state))), line))), DiffState(0, 0, 0, 0, [])));
}

function file_change_details(change) {
  const parsed = diff_rows(change.diff);
  return FileChangeDetails(text(change.path), text(change.kind), diffstate_additions(parsed), diffstate_deletions(parsed), diffstate_rows(parsed));
}

function file_change_summary(changes) {
  return changes.reduce((summary, change) => { const details = file_change_details(change);
return Object.freeze({...summary, additions: (filechangesummary_additions(summary) + filechangedetails_additions(details)), deletions: (filechangesummary_deletions(summary) + filechangedetails_deletions(details)), files: filechangesummary_files(summary).concat(details)}); }, FileChangeSummary(0, 0, []));
}

function diff_line_number(value) {
  const number_text = text(value);
  return number_text.padStart(4, " ");
}

function push_diff_rows_bang(chunks, rows, width) {
  const limit = 36;
  const visible = rows.slice(0, limit);
  const overflow = Math.max(0, (rows.length - limit));
  visible.forEach((row) => { const kind = diffrow_kind(row);
const numbers = ("".concat(diff_line_number(diffrow_old(row)), " ", diff_line_number(diffrow_new(row)), " │ "));
const line = compact_text(("".concat(numbers, diffrow_text(row))), width);
push_chunk_bang(chunks, white("\n"));
return push_chunk_bang(chunks, (((kind === "add")) ? (bg("#173326"))(brightGreen(line.padEnd(width, " "))) : ((kind === "delete")) ? (bg("#382127"))(brightRed(line.padEnd(width, " "))) : ((kind === "hunk")) ? brightCyan(line) : dim(line))); });
  if ((overflow > 0)) {
    return push_chunk_bang(chunks, dim(("".concat("\n          └ … +", overflow, " diff lines"))));
  }
}

function push_file_change_card_bang(chunks, item, status, runtime) {
  const data = conversationitem_data(item);
  const changes = ((data && Array.isArray(data.changes)) ? data.changes : []);
  const summary = file_change_summary(changes);
  const files = filechangesummary_files(summary);
  const width = Math.max(24, (available_agent_width(runtime) - 2));
  push_chunk_bang(chunks, ((status === "failed") ? brightRed("• ") : brightGreen("• ")));
  push_chunk_bang(chunks, brightWhite(("".concat(((status === "running") ? "Editing " : "Edited "), files.length, ((files.length === 1) ? " file" : " files"), " (+", filechangesummary_additions(summary), " -", filechangesummary_deletions(summary), ")"))));
  files.forEach((file, index) => { const last_p = (index === (files.length - 1));
push_chunk_bang(chunks, brightBlack(("".concat("\n  ", (last_p ? "└ " : "├ "), filechangedetails_path(file), " (+", filechangedetails_additions(file), " -", filechangedetails_deletions(file), ")"))));
if ((filechangedetails_rows(file).length > 0)) {
  return push_diff_rows_bang(chunks, filechangedetails_rows(file), width);
} });
  return push_chunk_bang(chunks, white("\n\n"));
}

function push_working_wave_bang(chunks, runtime) {
  const letters = text_or(runtime.workingLabel, "Working").split("");
  const cursor = (runtime.spinnerIndex % letters.length);
  push_chunk_bang(chunks, brightBlack("• "));
  return letters.forEach((letter, index) => push_chunk_bang(chunks, ((index === cursor) ? brightBlack(letter) : brightWhite(letter))));
}

function supervisor_status(runtime) {
  const id = text(runtime.supervisorId);
  const agents = bridgesnapshot_agents(snapshot(runtime.model));
  const matches = agents.filter((agent) => ((!(id === "")) && (agent_id(agent) === id)));
  return ((matches.length === 0) ? "" : agent_status(matches[0]));
}

export function transcript_placeholder(label, status, item_count, working_p) {
  const value = status.trim().toLowerCase();
  return (((item_count > 0)) ? "" : (working_p) ? "" : (((value === "") || (value === "starting"))) ? ("".concat("Starting ", label, "…")) : (((value === "offline") || (value === "failed") || (value === "error"))) ? ("".concat(label, " is offline.")) : "");
}

export function transcript_banner_p(status, item_count, working_p) {
  const value = status.trim().toLowerCase();
  return ((item_count === 0) && (!working_p) && (!(value === "")) && (!(value === "starting")) && (!(value === "offline")) && (!(value === "failed")) && (!(value === "error")));
}

function banner_visible_p(runtime) {
  return ((!aggregate_transcript_p(runtime)) && (runtime_selected_agent_id(runtime) === text(runtime.supervisorId)) && transcript_banner_p(supervisor_status(runtime), projected_conversation(runtime).length, transcript_working_p(runtime)));
}

const BANNER_LABEL_WIDTH = 13;

const BANNER_MIN_COLUMNS = 55;

const BANNER_MODEL_HINT = "/model changes the next launch";

export function banner_revision(identity) {
  const value = text(identity).trim();
  return ((value === "") ? "unknown" : value.slice(0, 8));
}

export function banner_permissions(mode) {
  const value = text(mode).trim();
  return (((value === "")) ? "pending" : ((value === "bypassPermissions")) ? "YOLO mode" : value);
}

function banner_field(label, value) {
  return ("".concat(("".concat(label, ":")).padEnd(BANNER_LABEL_WIDTH, " "), value));
}

export function session_banner_lines(identity, model, effort, directory, permissions) {
  const named = text_or(text(model), "pending");
  const graded = text(effort);
  const model_text = ((graded === "") ? named : ("".concat(named, " ", graded)));
  return [("".concat(">_ North Bridge (", banner_revision(identity), ")")), "", banner_field("model", ("".concat(model_text, "   ", BANNER_MODEL_HINT))), banner_field("directory", text_or(text(directory), "pending")), banner_field("permissions", banner_permissions(permissions))];
}

function banner_clip(line, width) {
  const limit = Math.max(1, width);
  return ((grapheme_count(line) > limit) ? ("".concat(line.slice(0, Math.max(0, (limit - 1))), "…")) : line);
}

function widest_line_bang(lines) {
  const widest = {n: 0};
  lines.forEach((line) => { if ((grapheme_count(line) > widest.n)) {
  return (widest.n = grapheme_count(line));
} });
  return widest.n;
}

export function banner_box_bang(lines, width) {
  const inner = Math.max(1, (width - 4));
  const clipped = lines.map((line) => banner_clip(line, inner));
  if ((width < BANNER_MIN_COLUMNS)) {
    return clipped;
  } else {
    const span = Math.min(inner, widest_line_bang(clipped));
    const rule = "─".repeat((span + 2));
    return [("".concat("╭", rule, "╮"))].concat(clipped.map((line) => ("".concat("│ ", line.padEnd(span, " "), " │"))), [("".concat("╰", rule, "╯"))]);
  }
}

export function session_banner_bang(identity, model, effort, directory, permissions, width) {
  return banner_box_bang(session_banner_lines(identity, model, effort, directory, permissions), width);
}

const BANNER_BOX_PREFIX = "│ ";

const BANNER_BOX_SUFFIX = " │";

export function banner_rule_line_p(line) {
  return (line.startsWith("╭") || line.startsWith("╰"));
}

export function banner_line_segments(line) {
  return ((banner_rule_line_p(line)) ? [line, "", ""] : ((line.startsWith(BANNER_BOX_PREFIX) && line.endsWith(BANNER_BOX_SUFFIX) && (line.length >= 4))) ? [BANNER_BOX_PREFIX, line.slice(2, (line.length - 2)), BANNER_BOX_SUFFIX] : ["", line, ""]);
}

export function session_banner_runs(lines) {
  const runs = [];
  lines.forEach((line, index) => { const segments = banner_line_segments(line);
const prefix = segments[0];
const content = segments[1];
const suffix = segments[2];
if ((index > 0)) {
  runs.push({text: "\n", tone: "frame"});
}
if ((!(prefix === ""))) {
  runs.push({text: prefix, tone: "frame"});
}
if ((!(content === ""))) {
  runs.push({text: content, tone: (content.includes("North Bridge") ? "title" : "field")});
}
if ((!(suffix === ""))) {
  return runs.push({text: suffix, tone: "frame"});
} });
  return runs;
}

export function render_conversation_bang(runtime) {
  const chunks = [];
  const items = projected_conversation(runtime);
  items.forEach((item) => { const kind = conversationitem_kind(item);
const title = conversationitem_title(item);
const body = clipped(conversationitem_body(item), 6000);
const status = conversationitem_status(item);
return (((kind === "user")) ? (() => { push_chunk_bang(chunks, (bg("#292c32"))(brightWhite(user_block_text(runtime, body))));
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
return push_chunk_bang(chunks, white("\n\n")); })()); });
  if (transcript_working_p(runtime)) {
    const elapsed = Math.floor(((Date.now() - runtime.workingSince) / 1000));
    push_working_wave_bang(chunks, runtime);
    push_chunk_bang(chunks, brightBlack(("".concat(" (", elapsed, "s · esc or ctrl-c to cancel)\n  "))));
    push_chunk_bang(chunks, brightBlack(transcript_context_text(runtime)));
  }
  const status = supervisor_status(runtime);
  const working_p = transcript_working_p(runtime);
  const supervisor_view_p = ((!aggregate_transcript_p(runtime)) && (runtime_selected_agent_id(runtime) === text(runtime.supervisorId)));
  const placeholder = (supervisor_view_p ? transcript_placeholder(main_agent_label(runtime), status, items.length, working_p) : "");
  if ((!(placeholder === ""))) {
    push_chunk_bang(chunks, brightBlack(placeholder));
  }
  if (banner_visible_p(runtime)) {
    session_banner_runs(session_banner_bang(runtime.sourceIdentity, runtime.sessionModel, runtime.sessionEffort, short_directory(runtime.sessionCwd), runtime.sessionPermissions, available_frame_width())).forEach((run) => push_chunk_bang(chunks, (((run.tone === "title") ? brightWhite : brightBlack))(run.text)));
  }
  return new StyledText(chunks);
}

function visible_notice(notice) {
  const value = text(notice);
  return (((value === "view dag")) ? "view graph" : ((value === "view kanban")) ? "view board" : value);
}

export function config_section_title(role) {
  return (((role === "moduleset")) ? "SETS" : ((role === "module")) ? "MODULES" : ((role === "skill")) ? "SKILLS" : ((role === "hook")) ? "HOOKS" : ((role === "plugin")) ? "PLUGINS" : ((role === "other")) ? "OTHER" : "");
}

export function config_header_roles(role) {
  return (((role === "moduleset")) ? ["moduleset"] : ((role === "module")) ? ["skill", "module"] : ((role === "boundhook")) ? ["skill", "module"] : ((role === "skill")) ? ["skill"] : ((role === "hook")) ? ["hook"] : ((role === "plugin")) ? ["plugin"] : ((role === "other")) ? ["other"] : []);
}

export function config_header_keys(entry, rows) {
  const role = config_row_role(entry, rows);
  const scope = config_row_scope(configentry_kind(entry), configentry_name(entry));
  return config_header_roles(role).map((heading) => ("".concat(scope, " ", heading)));
}

export function config_header_shared_bang(prior, current) {
  const count = {n: 0};
  current.forEach((heading, index) => { if (((count.n === index) && (index < prior.length) && (prior[index] === heading))) {
  return (count.n = (index + 1));
} });
  return count.n;
}

function config_panel_title(config_filter) {
  return (((config_filter === "hook")) ? "hooks" : ((config_filter === "skill")) ? "skills" : ((config_filter === "plugin")) ? "plugins" : ((config_filter === "module")) ? "modules" : ((config_filter === "globals")) ? "globals" : ((config_filter === "agentsmd")) ? "directory context" : "context switchboard");
}

export function config_empty_note(loaded_p, filtering_p) {
  return (((!loaded_p)) ? " loading…" : (filtering_p) ? " nothing matches" : " nothing to configure here");
}

export function config_query_field(filtering_p, query) {
  return (filtering_p ? ("".concat("  /", query)) : "");
}

export function config_panel_legend(filtering_p) {
  return (filtering_p ? "  ↑/↓ move · tab fold · space toggle · enter edit · esc clears filter" : "  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close");
}

function dimmest(value) {
  return dim(brightBlack(value));
}

function config_member_count_text(count) {
  return ("".concat(count, ((count === 1) ? " member" : " members")));
}

function config_fold_glyph(dir_row_p, expanded_p) {
  return ((!dir_row_p) ? "" : (expanded_p ? "▾ " : "▸ "));
}

function config_dir_label(entry) {
  if (config_global_row_p(configentry_kind(entry), configentry_name(entry))) {
    return "GLOBAL";
  } else {
    const path = short_directory(text(configentry_detail(entry)));
    return text_or(path, configentry_name(entry));
  }
}

export function config_kind_tag(kind, role) {
  const headings = config_header_roles(role);
  const depth = headings.length;
  const innermost = ((depth > 0) ? headings[(depth - 1)] : "");
  return (((innermost === "") || (innermost === role)) ? "" : ("".concat(kind, " · ")));
}

export function config_row_parts(entry, memberships, expanded_p, role, state_text, width) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const dir_p = (kind === "dir");
  const members = config_module_members(memberships, name);
  const detail = (((kind === "hook")) ? "" : (config_subtree_kind_p(kind)) ? "" : (dir_p) ? "" : ((kind === "module")) ? ((members == null) ? "" : config_member_count_text(members.length)) : text(configentry_detail(entry)));
  const indent = config_row_indent(role);
  const glyph = config_fold_glyph(dir_p, expanded_p);
  const tag = config_kind_tag(kind, role);
  const label = (dir_p ? config_dir_label(entry) : config_row_label(kind, name));
  const room = Math.max(8, (width - indent.length - glyph.length - tag.length - state_text.length - detail.length - 4));
  return {indent: indent, glyph: glyph, tag: tag, name: compact_text(label, room), state: state_text, detail: detail};
}

const CONFIG_INDENT_WIDTH = 2;

export function config_row_depth(role) {
  return (((role === "dir")) ? 0 : ((role === "ins")) ? 1 : ((role === "memroot")) ? 1 : ((role === "module")) ? 3 : ((role === "boundhook")) ? 4 : 2);
}

function config_row_indent(role) {
  return " ".repeat((CONFIG_INDENT_WIDTH * config_row_depth(role)));
}

function config_header_indent(index) {
  return " ".repeat((CONFIG_INDENT_WIDTH * (index + 2)));
}

export function render_config_panel_bang(runtime) {
  const entries = config_panel_rows(runtime);
  const total = entries.length;
  const stored_entries = runtime.configEntries;
  const stored_manifest = runtime.configAllEntries;
  const manifest = (stored_manifest ? stored_manifest : (stored_entries ? stored_entries : entries));
  const stored_memberships = runtime.configMemberships;
  const memberships = (stored_memberships ? stored_memberships : []);
  const basis = (stored_entries ? stored_entries : entries);
  const expanded = panel_expanded(runtime);
  const config_filter = text_or(text(runtime.configFilter), "all");
  const filtering_p = panel_filtering_p(runtime);
  const focused_p = panel_focused_p(runtime);
  const query = panel_query(runtime);
  if ((total === 0)) {
    return new StyledText([brightYellow(config_panel_title(config_filter)), brightCyan(config_query_field(filtering_p, query)), brightBlack(config_empty_note((runtime.configLoaded ? true : false), filtering_p))]);
  } else {
    const index = clamped_index(runtime.configIndex, total);
    const window = config_visible_count(total, config_filter);
    const start = window_start(index, total, window);
    const stop = Math.min(total, (start + window));
    const width = Math.max(12, (terminal_columns() - 12));
    const parts = [brightYellow(config_panel_title(config_filter)), brightCyan(config_query_field(filtering_p, query)), brightBlack(("".concat(config_panel_legend(filtering_p), "\n")))];
    entries.slice(start, stop).forEach((entry, offset) => { const i = (start + offset);
const cursor_p = (i === index);
const kind = configentry_kind(entry);
const active_p = config_entry_active_p(entry, manifest, memberships);
const pinned_p = ((kind === "hook") && (!config_hook_enabled_p(configentry_state(entry))));
const context_p = config_row_context_only_p(entry, query);
const role = config_row_role(entry, basis);
const open_p = ((kind === "dir") && ((!(query.trim() === "")) || (!config_view_folds_p(config_filter)) || config_node_expanded_p(expanded, configentry_name(entry))));
const nested_p = (role === "boundhook");
const state_text = config_state_text(entry, manifest, memberships, active_p, nested_p);
const row = config_row_parts(entry, memberships, open_p, role, state_text, width);
const headings = config_header_keys(entry, basis);
const prior = ((i === start) ? [] : config_header_keys(entries[(i - 1)], basis));
const shared = config_header_shared_bang(prior, headings);
const tail = (((i + 1) === stop) ? "" : "\n");
config_header_roles(role).forEach((heading, at) => { if ((at >= shared)) {
  return parts.push(brightYellow(("".concat(config_header_indent(at), config_section_title(heading), "\n"))));
} });
parts.push(((cursor_p && focused_p) ? brightCyan("› ") : (cursor_p ? brightBlack("› ") : brightBlack("  "))));
const name_tone = ((pinned_p) ? dimmest : ((cursor_p && focused_p)) ? brightWhite : (context_p) ? dimmest : brightBlack);
const state_tone = ((pinned_p) ? dimmest : (active_p) ? brightGreen : brightBlack);
parts.push(name_tone(("".concat(row.indent, row.glyph))));
if ((!(row.tag === ""))) {
  parts.push(dimmest(row.tag));
}
parts.push(name_tone(row.name));
parts.push(name_tone(": "));
parts.push(state_tone(row.state));
return parts.push(name_tone(("".concat(((row.detail === "") ? "" : ("".concat("  ", row.detail))), tail)))); });
    return new StyledText(parts);
  }
}

function detail_open_p(runtime) {
  return (!(text(runtime.detailView) === ""));
}

function detail_showing_p(runtime, view) {
  return (text(runtime.detailView) === view);
}

function open_detail_bang(runtime, view) {
  return (runtime.detailView = view);
}

function close_detail_bang(runtime) {
  clear_panel_filter_bang(runtime);
  return (runtime.detailView = "");
}

function toggle_help_bang(runtime, ui) {
  if (detail_showing_p(runtime, "help")) {
    close_detail_bang(runtime);
    focus_composer_bang(runtime, ui);
  } else {
    open_detail_bang(runtime, "help");
    focus_panel_bang(runtime, ui);
  }
  return runtime.render();
}

function detail_agents(runtime) {
  const state = snapshot(runtime.model);
  return segment_agents(bridgesnapshot_agents(state), text_or(text(runtime.detailSegment), "all"));
}

const DETAIL_CHROME_ROWS = 3;

const CONFIG_SECTION_ROWS = 6;

function detail_visible_count(total, extra) {
  return fitted_window(total, terminal_rows(), (CHROME_ROWS + MIN_WORKSPACE_ROWS + DETAIL_CHROME_ROWS + extra));
}

export function config_section_rows(view) {
  return (((view === "all")) ? CONFIG_SECTION_ROWS : ((view === "globals")) ? 5 : ((view === "agentsmd")) ? 0 : 1);
}

export function config_visible_count(total, view) {
  return detail_visible_count(total, config_section_rows(view));
}

function clamped_index(raw, total) {
  return Math.max(0, Math.min((raw ? raw : 0), (total - 1)));
}

function selected_detail_agent(runtime) {
  const agents = detail_agents(runtime);
  const total = agents.length;
  return ((total > 0) ? agents[clamped_index(runtime.detailIndex, total)] : null);
}

function config_header_lines_bang(entries, basis, start, stop) {
  const count = {n: 0};
  entries.slice(start, stop).forEach((entry, offset) => { const i = (start + offset);
const headings = config_header_keys(entry, basis);
const prior = ((i === start) ? [] : config_header_keys(entries[(i - 1)], basis));
return (count.n = (count.n + (headings.length - config_header_shared_bang(prior, headings)))); });
  return count.n;
}

export function config_detail_lines_bang(runtime) {
  const entries = config_panel_rows(runtime);
  const total = entries.length;
  const stored_entries = runtime.configEntries;
  const basis = (stored_entries ? stored_entries : entries);
  const view = text_or(text(runtime.configFilter), "all");
  if ((total === 0)) {
    return 1;
  } else {
    const index = clamped_index(runtime.configIndex, total);
    const window = config_visible_count(total, view);
    const start = window_start(index, total, window);
    const stop = Math.min(total, (start + window));
    return (1 + (stop - start) + config_header_lines_bang(entries, basis, start, stop));
  }
}

function detail_body_lines_bang(runtime) {
  return ((detail_showing_p(runtime, "config")) ? config_detail_lines_bang(runtime) : (detail_showing_p(runtime, "agents")) ? (() => { const total = detail_agents(runtime).length; const agent = selected_detail_agent(runtime); const metadata = (agent ? agent_route_text_bang(agent, Math.max(12, (terminal_columns() - 8))) : ""); const extra = ((metadata === "") ? 0 : 1); return (1 + Math.max(1, Math.min(total, detail_visible_count(total, extra))) + extra); })() : (detail_showing_p(runtime, "help")) ? (1 + help_visible_rows(panel_query(runtime))) : 0);
}

export function detail_height_bang(runtime) {
  return (2 + detail_body_lines_bang(runtime));
}

function agent_detail_title(segment_id) {
  return ((segment_id === "all") ? "agents" : ("".concat("agents · ", segment_id)));
}

function agent_detail_row_bang(agent, width) {
  return agent_cell_text_bang(agent_summary(agent), width);
}

function render_agent_detail_bang(runtime) {
  const agents = detail_agents(runtime);
  const total = agents.length;
  const title = agent_detail_title(text_or(text(runtime.detailSegment), "all"));
  const parts = [brightYellow(title), brightBlack("  ↑/↓ move · ←/→ segment · enter or esc close\n")];
  if ((total === 0)) {
    return new StyledText(parts.concat([brightBlack("  no agents in this state")]));
  } else {
    const index = clamped_index(runtime.detailIndex, total);
    const width = Math.max(12, (terminal_columns() - 8));
    const selected_agent = selected_detail_agent(runtime);
    const metadata = (selected_agent ? agent_route_text_bang(selected_agent, width) : "");
    const extra = ((metadata === "") ? 0 : 1);
    const window = detail_visible_count(total, extra);
    const start = window_start(index, total, window);
    const stop = Math.min(total, (start + window));
    agents.slice(start, stop).forEach((agent, offset) => { const i = (start + offset);
const cursor_p = (i === index);
const tail = ((((i + 1) === stop) && (metadata === "")) ? "" : "\n");
parts.push((cursor_p ? brightCyan("› ") : brightBlack("  ")));
return parts.push(((cursor_p ? brightWhite : brightBlack))(("".concat(agent_detail_row_bang(agent, width), tail)))); });
    if ((!(metadata === ""))) {
      parts.push(brightBlack(("".concat("  ", metadata))));
    }
    return new StyledText(parts);
  }
}

function HelpRow(keys, meaning) {
  return Object.freeze({_tag: "HelpRow", keys, meaning});
}

function helprow_keys(r) { return r.keys; }

function helprow_meaning(r) { return r.meaning; }

const HELP_ROWS = [HelpRow("Tab", "swap Agents/Threads; folds in the switchboard"), HelpRow("←/→", "switch thread view"), HelpRow("Ctrl-J / ↓", "into the agent strip, esc back out"), HelpRow("Esc /close /esc", "back or dismiss; cancels a turn at root"), HelpRow("Ctrl-C /interrupt", "cancel the turn; the message comes back"), HelpRow("/q /exit / Ctrl-Q", "quit Northbridge"), HelpRow("/help", "this panel"), HelpRow("/glyph <one>|reset", "prompt glyph"), HelpRow("/emoji <query>", "picker"), HelpRow("/sound on|off|pack", "voice lines"), HelpRow("/mute", "quiet")];

const HELP_KEY_WIDTH = 22;

export function help_query_rows(query) {
  const needle = query.trim().toLowerCase();
  return ((needle === "") ? HELP_ROWS : HELP_ROWS.filter((row) => ("".concat(helprow_keys(row), " ", helprow_meaning(row))).toLowerCase().includes(needle)));
}

function help_visible_rows(query) {
  const total = help_query_rows(query).length;
  return Math.max(1, Math.min(total, detail_visible_count(total, 0)));
}

function render_help_panel_bang(runtime) {
  const filtering_p = panel_filtering_p(runtime);
  const query = panel_query(runtime);
  const matched = help_query_rows(query);
  const chunks = [brightYellow("Northbridge keys"), brightCyan(config_query_field(filtering_p, query)), brightBlack((filtering_p ? " · esc clears filter\n" : " · / filter · esc closes\n"))];
  const rows = matched.slice(0, help_visible_rows(query));
  if ((rows.length === 0)) {
    push_chunk_bang(chunks, brightBlack(" nothing matches"));
  } else {
    rows.forEach((row, index) => { push_chunk_bang(chunks, brightWhite(helprow_keys(row).padEnd(HELP_KEY_WIDTH, " ")));
push_chunk_bang(chunks, brightBlack(helprow_meaning(row)));
if ((index < (rows.length - 1))) {
  return push_chunk_bang(chunks, brightBlack("\n"));
} });
  }
  return new StyledText(chunks);
}

export function render_detail_panel_bang(runtime) {
  return ((detail_showing_p(runtime, "config")) ? render_config_panel_bang(runtime) : (detail_showing_p(runtime, "agents")) ? render_agent_detail_bang(runtime) : (detail_showing_p(runtime, "help")) ? render_help_panel_bang(runtime) : new StyledText([brightBlack("")]));
}

function segment_chunk(segment, highlighted_p) {
  const label = agentsegment_label(segment);
  const id = agentsegment_id(segment);
  return ((highlighted_p) ? brightCyan(label) : ((id === "failed")) ? brightRed(label) : ((id === "blocked")) ? brightYellow(label) : ((id === "running")) ? brightGreen(label) : brightWhite(label));
}

function render_agent_strip(state, focused_p, selected) {
  const segments = agent_segments(bridgesnapshot_agents(state));
  const chunks = [(focused_p ? brightCyan("› ") : brightBlack("  "))];
  segments.forEach((segment, index) => { if ((index > 0)) {
  chunks.push(brightBlack(STRIP_SEPARATOR));
}
return chunks.push(segment_chunk(segment, (focused_p && (index === selected)))); });
  if (focused_p) {
    chunks.push(brightBlack("   h/l segment · enter open · esc input"));
  }
  return new StyledText(chunks);
}

function status_notice(runtime, state) {
  const workspace_notice = text(runtime.workspaceNotice);
  const notice = ((workspace_notice === "") ? visible_notice(bridgesnapshot_notice(state)) : workspace_notice);
  return notice;
}

function render_status(runtime, state) {
  return new StyledText([brightBlack(status_notice(runtime, state))]);
}

const AGENTS_TAB_LABEL = "Agents";

const THREADS_TAB_LABEL = "Threads";

const FRAME_TAB_SEPARATOR = " | ";

const SUBVIEW_TAB_SEPARATOR = " > ";

const THREADS_TAB_START = (AGENTS_TAB_LABEL.length + FRAME_TAB_SEPARATOR.length);

const SUBVIEW_TAB_ORIGIN = (THREADS_TAB_START + THREADS_TAB_LABEL.length + SUBVIEW_TAB_SEPARATOR.length);

const SUBVIEW_TAB_GAP = 2;

function push_session_identity_bang(chunks, session) {
  push_chunk_bang(chunks, brightYellow(("".concat(text_or(session.sessionModel, "model pending"), " ", text_or(session.sessionEffort, "effort pending")))));
  push_chunk_bang(chunks, brightBlack(" · "));
  push_chunk_bang(chunks, brightGreen(text_or(short_directory(session.sessionCwd), "directory pending")));
  push_chunk_bang(chunks, brightBlack(" · "));
  return push_chunk_bang(chunks, dim(text_or(session.sessionBranch, "branch pending")));
}

export function render_view_tabs_bang(frame, state, view_id, session) {
  const chunks = [];
  const threads_p = threads_frame_p(frame);
  const views = view_list(state);
  push_chunk_bang(chunks, ((threads_p ? brightBlack : brightGreen))(AGENTS_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(FRAME_TAB_SEPARATOR));
  push_chunk_bang(chunks, ((threads_p ? brightGreen : brightBlack))(THREADS_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(SUBVIEW_TAB_SEPARATOR));
  if (threads_p) {
    views.forEach((view, index) => { const selected_p = (workview_id(view) === view_id);
const title = workview_title(view);
push_chunk_bang(chunks, ((selected_p ? brightGreen : brightBlack))(("".concat((selected_p ? "[" : " "), title, (selected_p ? "]" : " ")))));
if ((index < (views.length - 1))) {
  return push_chunk_bang(chunks, white("  "));
} });
    push_chunk_bang(chunks, brightBlack("  ← switch →"));
  } else {
    push_session_identity_bang(chunks, session);
  }
  if (((!threads_p) && aggregate_transcript_p(session))) {
    push_chunk_bang(chunks, brightYellow(" · all transcripts"));
  }
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
  return workitem_id(item).slice(0, 8);
}

function push_condition_bang(chunks, condition, label) {
  return push_chunk_bang(chunks, ((((condition === "active")) ? brightCyan : ((condition === "ready")) ? brightGreen : ((condition === "blocked")) ? brightRed : brightYellow))(label));
}

function available_work_width(__runtime, __state) {
  return available_frame_width();
}

function render_list_view_bang(runtime, items, selected, width) {
  const chunks = [];
  const rows = list_rows(runtime, items);
  const collapsed = runtime.collapsedListConditions;
  rows.forEach((row, visual_index) => { const kind = listrow_kind(row);
const condition = listrow_condition(row);
if ((kind === "header")) {
  const collapsed_p = collapsed.has(condition);
  const header = ("".concat(" ", (collapsed_p ? "▸" : "▾"), "  ", list_section_title(condition), "  ", listrow_count(row)));
  push_chunk_bang(chunks, (bg("#292c32"))(brightWhite(header.padEnd(width, " "))));
} else {
  const index = listrow_index(row);
  const item = items[index];
  const title_width = Math.max(10, (width - 25));
  const title = compact_text(workitem_title(item), title_width);
  const selected_p = (index === selected);
  push_chunk_bang(chunks, brightBlack("  "));
  push_chunk_bang(chunks, (selected_p ? brightCyan("› ") : brightBlack("  ")));
  push_condition_bang(chunks, workitem_condition(item), cell_text(workitem_condition(item).toUpperCase(), 9));
  push_chunk_bang(chunks, ((selected_p ? brightWhite : white))(title));
  push_chunk_bang(chunks, dim(("".concat("  @", short_thread_id(item)))));
}
if ((visual_index < (rows.length - 1))) {
  return push_chunk_bang(chunks, white("\n"));
} });
  return new StyledText(chunks);
}

function work_item_by_id(items, id) {
  return items.find((item) => (workitem_id(item) === id));
}

function work_item_condition_for(items, id) {
  return items.reduce((condition, item) => ((workitem_id(item) === id) ? workitem_condition(item) : condition), "");
}

function render_dag_view_bang(items, selected, width) {
  const chunks = [];
  items.forEach((item, index) => { const condition = workitem_condition(item);
const dependencies = workitem_dependencies(item);
const selected_p = (index === selected);
push_chunk_bang(chunks, (selected_p ? brightCyan("› ") : brightBlack("  ")));
push_condition_bang(chunks, condition, "● ");
push_chunk_bang(chunks, ((selected_p ? brightWhite : white))(compact_text(workitem_title(item), Math.max(12, (width - 16)))));
push_chunk_bang(chunks, dim(("".concat("  @", short_thread_id(item), "\n"))));
if ((dependencies.length === 0)) {
  push_chunk_bang(chunks, brightBlack("    ╰─ root\n"));
} else {
  dependencies.forEach((dependency) => { const target = work_item_by_id(items, dependency);
push_chunk_bang(chunks, brightBlack("    ╰─ requires ← "));
push_chunk_bang(chunks, ((target ? brightCyan : brightBlack))(("".concat("@", dependency.slice(0, 8)))));
return push_chunk_bang(chunks, (target ? dim(("".concat("  ", compact_text(workitem_title(target), Math.max(8, (width - 28))), "\n"))) : brightBlack("  outside current board\n"))); });
}
if ((index < (items.length - 1))) {
  return push_chunk_bang(chunks, white("\n"));
} });
  return new StyledText(chunks);
}

function board_lane_items(items, lane_id) {
  return items.filter((item) => (board_lane_id(workitem_condition(item)) === lane_id));
}

function compact_body(value, width) {
  const lines = text(value).split("\n").map((line) => line.trim()).filter((line) => (!(line === "")));
  return compact_text(lines.join(" "), width);
}

function board_card_id(thread_id) {
  return ("".concat("board-card-", thread_id));
}

function board_signature(items, selected, width) {
  return ("".concat(width, "|", selected, "|", items.map((item) => ("".concat(workitem_id(item), "\x01", workitem_title(item), "\x01", workitem_body(item), "\x01", workitem_condition(item)))).join("\u0002")));
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
  (runtime.model = select_thread(runtime.model, workitem_id(item)));
  show_frame_bang(runtime, ui, "threads");
  return ui.workScroll.scrollChildIntoView(board_card_id(workitem_id(item)));
}

function prefill_outcome_bang(runtime, ui, thread_id) {
  (ui.composerInput.value = ("".concat("/outcome @", thread_id, " ")));
  show_frame_bang(runtime, ui, "threads");
  return set_board_notice_bang(runtime, ("".concat("Finish the outcome, then press Enter; Done is derived from north tell @", thread_id, " outcome <result>.")));
}

async function move_ready_thread_bang(runtime, thread_id, position, anchor_id) {
  const argv = ((anchor_id === "") ? [north_bin(), "queue", "move", thread_id, position] : [north_bin(), "queue", "move", thread_id, position, anchor_id]);
  const output = await run_command(argv);
  (runtime.workspaceNotice = text(output).trim());
  return await refresh_bang(runtime);
}

function unsupported_drop_notice(source_condition, target_lane) {
  return ((terminal_condition_p(source_condition)) ? "Done is derived from an outcome; reopening requires explicitly retracting that outcome." : ((target_lane === "in-progress")) ? "In Progress is derived from a live run; dispatch or /launch the thread to start it." : ((source_condition === "active")) ? "Active work is derived from its live run; /interrupt and settle that run before moving it." : ((!(source_condition === "ready"))) ? "Only ready work has a durable queue order; resolve its prerequisites before reordering it." : "That lane is derived from lifecycle facts; use the corresponding North lifecycle action.");
}

function handle_board_drop_bang(runtime, ui, target_lane, target_card, event) {
  event.preventDefault();
  event.stopPropagation();
  ui.workScroll.stopAutoScroll();
  const source_card = board_card_node(event.source);
  const source_id = (source_card ? bare(source_card.northThreadId) : "");
  const state = snapshot(runtime.model);
  const items = bridgesnapshot_board(state);
  const source_condition = work_item_condition_for(items, source_id);
  const source_lane = board_lane_id(source_condition);
  const target_id = (target_card ? bare(target_card.northThreadId) : "");
  const target_condition = work_item_condition_for(items, target_id);
  return (((source_id === "")) ? set_board_notice_bang(runtime, "Drop ignored: the dragged source was not a North work card.") : (((target_lane === "done") && (!terminal_condition_p(source_condition)))) ? prefill_outcome_bang(runtime, ui, source_id) : (((source_lane === "not-started") && (target_lane === "not-started") && (source_condition === "ready") && ((target_id === "") || (target_condition === "ready")))) ? ((source_id === target_id) ? set_board_notice_bang(runtime, "Queue order unchanged: a card cannot be moved relative to itself.") : (() => { const position = ((target_id === "") ? ((event.x < (event.currentTarget.screenX + (event.currentTarget.width / 2))) ? "first" : "last") : ((event.x < (target_card.screenX + (target_card.width / 2))) ? "before" : "after")); return report_promise_bang(runtime, move_ready_thread_bang(runtime, source_id, position, target_id)); })()) : set_board_notice_bang(runtime, unsupported_drop_notice(source_condition, target_lane)));
}

function card_content(item, width) {
  const body = compact_body(workitem_body(item), Math.max(8, (width - 4)));
  const fallback = ("".concat("@", short_thread_id(item)));
  return new StyledText([brightWhite(compact_text(workitem_title(item), Math.max(8, (width - 4)))), white("\n"), dim(((body === "") ? fallback : body))]);
}

function make_board_card_bang(runtime, ui, item, index, lane_index, width) {
  const renderer = runtime.renderer;
  const thread_id = workitem_id(item);
  const condition = workitem_condition(item);
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
  const lane_id = boardlane_id(lane);
  const title = boardlane_title(lane);
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
      return ui.workScroll.scrollChildIntoView(board_card_id(workitem_id(items[selected])));
    }
  }
}

function work_content_bang(runtime, state, view, selected) {
  const items = workview_items(view);
  const width = available_work_width(runtime, state);
  return ((items.length === 0) ? new StyledText([brightBlack(("".concat("No ", workview_title(view), " items")))]) : (((workview_id(view) === "graph")) ? render_dag_view_bang(items, selected, width) : render_list_view_bang(runtime, items, selected, width)));
}

export function composer_hint(pane, label) {
  return ((pane === "agents") ? ("".concat("Message ", label, "…")) : "/list, /board, /graph, /capture, /filter, /assign");
}

function minibuffer_placeholder(runtime) {
  return composer_hint(text(runtime.frame), main_agent_label(runtime));
}

function palette_visible_count(total, rows, docked) {
  return fitted_window(total, rows, (CHROME_ROWS + MIN_WORKSPACE_ROWS + docked));
}

function palette_option_rows(total, window) {
  return (((total > window) && (window > 1)) ? (window - 1) : window);
}

function palette_overflow(total, window, rows) {
  return (((total > rows) && (window > rows)) ? (total - rows) : 0);
}

function render_minibuffer_bang(runtime, ui) {
  const frame = text(runtime.frame);
  const input = ui.composerInput;
  const options = palette_options(frame, text(input.value));
  const total = options.length;
  const index = Math.max(0, Math.min(runtime.paletteIndex, Math.max(0, (total - 1))));
  const docked = (detail_open_p(runtime) ? detail_height_bang(runtime) : 0);
  const window = palette_visible_count(total, terminal_rows(), docked);
  const rows = palette_option_rows(total, window);
  const start = window_start(index, total, rows);
  (runtime.paletteIndex = index);
  (runtime.paletteStart = start);
  (runtime.paletteRows = rows);
  (input.placeholder = minibuffer_placeholder(runtime));
  (ui.composerPalette.visible = (total > 0));
  (ui.composerPalette.height = window);
  (ui.composerPalette.content = ((total > 0) ? render_command_palette_bang(options.slice(start, (start + rows)), (index - start), palette_overflow(total, window, rows)) : ""));
  return options;
}

function render_ui_bang(runtime, ui) {
  if ((!runtime.disposed)) {
    const state = snapshot(runtime.model);
    const agents = bridgesnapshot_agents(state);
    const views = view_list(state);
    const requested = bridgesnapshot_active_view_id(state);
    const current = selected_view(state, requested);
    const items = workview_items(current);
    const agent_max = Math.max(0, (agents.length - 1));
    const work_max = Math.max(0, (items.length - 1));
    const board_p = (workview_id(current) === "board");
    const banner_p = banner_visible_p(runtime);
    const roster = roster_text_bang(state, runtime.agentIndex, text(runtime.supervisorId), banner_p);
    const roster_rows = roster_visible_rows(roster);
    const notice = status_notice(runtime, state);
    const notice_p = (!(notice === ""));
    (runtime.agentIndex = Math.max(0, Math.min(runtime.agentIndex, agent_max)));
    (runtime.workIndex = Math.max(0, Math.min(runtime.workIndex, work_max)));
    apply_frame_visibility_bang(runtime, ui);
    (ui.viewTabsText.content = render_view_tabs_bang(runtime.frame, state, workview_id(current), runtime));
    (ui.agentsText.visible = (roster_rows > 0));
    (ui.agentsText.height = Math.max(1, roster_rows));
    (ui.agentsText.content = roster);
    (ui.transcriptText.content = render_conversation_bang(runtime));
    (ui.workText.visible = (!board_p));
    (ui.boardRoot.visible = board_p);
    if (board_p) {
      sync_board_bang(runtime, ui, items, runtime.workIndex, available_work_width(runtime, state));
    } else {
      (ui.workText.content = work_content_bang(runtime, state, current, runtime.workIndex));
    }
    (ui.statusText.content = render_status(runtime, state));
    (ui.agentStatusText.content = render_status(runtime, state));
    (ui.statusText.visible = (threads_frame_p(runtime.frame) && notice_p));
    (ui.agentStatusText.visible = ((!threads_frame_p(runtime.frame)) && notice_p));
    render_prompt_bang(runtime, ui.composerPrompt);
    const segments = agent_segments(agents);
    const strip_max = Math.max(0, (segments.length - 1));
    (runtime.stripIndex = Math.max(0, Math.min(runtime.stripIndex, strip_max)));
    (ui.agentStripText.content = render_agent_strip(state, runtime.stripFocused, runtime.stripIndex));
    const open_p = detail_open_p(runtime);
    (ui.detailPanel.visible = open_p);
    if (open_p) {
      (ui.detailPanel.height = detail_height_bang(runtime));
      (ui.detailText.content = render_detail_panel_bang(runtime));
    }
    render_minibuffer_bang(runtime, ui);
    (runtime.activeView = workview_id(current));
    return views;
  }
}

function bridge_agent_bang(runtime, execution_id, role, status) {
  const state = snapshot(runtime.model);
  const agents = bridgesnapshot_agents(state);
  const prior_id = selected_agent_id(state, runtime.agentIndex);
  const existing = agents.find((agent) => (agent_id(agent) === execution_id));
  const updated = Agent(execution_id, ((role === "supervisor") ? main_agent_label(runtime) : ("".concat("Codex ", execution_id.slice(0, 8)))), status, ((role === "supervisor") ? "Northbridge control session" : "Bridge execution"), "", "", "", "", "", "", "", "", "");
  runtime.bridgeExecutions.add(execution_id);
  if ((role === "supervisor")) {
    (runtime.supervisorId = execution_id);
  }
  (runtime.model = upsert_agent(runtime.model, (existing ? agent_with_route(updated, existing) : updated)));
  reconcile_agent_selection_bang(runtime, text_or(prior_id, ((role === "supervisor") ? execution_id : "")));
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
    return ParsedRecord(Number(line.slice(1, close)), kind, ((payload === "") ? {} : JSON.parse(payload)));
  } catch (__) {
    return null;
  } })();
  }
}

function event_item_id(execution_id, item_id) {
  return ("".concat(execution_id, ":", text(item_id)));
}

function safe_sequence(value) {
  const candidate = Number(value);
  return (Number.isSafeInteger(candidate) ? candidate : 0);
}

function wire_conversation_item(existing, id, kind, title, body, status, item_data, execution_id, event) {
  return owned_conversation_item(id, kind, title, body, status, item_data, execution_id, (existing ? conversationitem_at(existing) : text(event.at)), (existing ? conversationitem_cursor(existing) : (safe_sequence(event.sequence) + 1)), (existing ? conversationitem_sequence(existing) : 0));
}

function control_conversation_item(execution_id, record, kind, title, body, status) {
  const data = (parsedrecord_data(record) || {});
  const record_sequence = parsedrecord_sequence(record);
  const cursor_candidate = Number(data.wireCursor);
  const cursor = (Number.isSafeInteger(cursor_candidate) ? cursor_candidate : 9007199254740991);
  return owned_conversation_item(event_item_id(execution_id, ("".concat("control:", record_sequence))), kind, title, body, status, null, execution_id, text(data.bridgeRecordAt), cursor, (record_sequence + 1));
}

function append_item_delta_bang(runtime, stream_state, event, id, kind, title, delta, status) {
  const existing = conversation_item_by_id(runtime, id);
  const prior = (existing ? conversationitem_body(existing) : "");
  const actual_title = (existing ? conversationitem_title(existing) : title);
  return upsert_conversation_bang(runtime, wire_conversation_item(existing, id, kind, actual_title, clipped(("".concat(prior, delta)), 6000), status, (existing ? conversationitem_data(existing) : null), text(stream_state.executionId), event));
}

function wire_content_text(value) {
  return (((value == null)) ? "" : ((typeof value === "string")) ? value : (Array.isArray(value)) ? value.map((part) => wire_content_text(part)).filter((part) => (!(part === ""))).join("\n") : safe_json(value));
}

function adopt_wire_model_bang(runtime, model, effort) {
  if (model) {
    const provider = text(model.provider);
    const tier = text(model.tier);
    const label = ((((!(provider === "")) && (!(tier === "")))) ? ("".concat(provider, "/", tier)) : ((!(provider === ""))) ? provider : "");
    if (((!(label === "")) && (text(runtime.sessionModel).trim() === ""))) {
      (runtime.sessionModel = label);
    }
  }
  const effort_label = text(effort);
  if ((!(effort_label === ""))) {
    return (runtime.sessionEffort = effort_label);
  }
}

function intermediate_provider_session_replacement_p(data) {
  return ((text(data.status) === "failed") && (text(data.origin) === "north") && (text(data.errorCode) === "provider_session_replaced"));
}

function handle_wire_message_bang(runtime, stream_state, data) {
  const role = text(data.role);
  const stage = text(data.stage);
  const execution_id = text(stream_state.executionId);
  const id = event_item_id(execution_id, data.messageId);
  const body = clean_text(wire_content_text(data.content));
  const existing = conversation_item_by_id(runtime, id);
  if (((role === "assistant") && (!stream_state.booting))) {
    return (((stage === "started")) ? upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "assistant", "", "", "running", null, execution_id, data)) : ((stage === "delta")) ? append_item_delta_bang(runtime, stream_state, data, id, "assistant", "", body, "running") : ((stage === "completed")) ? (() => { const completed_body = ((body === "") ? (existing ? conversationitem_body(existing) : "") : body); (runtime.lastAssistantText = completed_body);
return upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "assistant", "", completed_body, "done", null, execution_id, data)); })() : null);
  }
}

function handle_wire_tool_bang(runtime, stream_state, data, kind) {
  const id = event_item_id(text(stream_state.executionId), data.toolCallId);
  const existing = conversation_item_by_id(runtime, id);
  return (((kind === "tool.admitted")) ? upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "tool", text(data.name), clean_text(data.argumentPreview), "running", null, text(stream_state.executionId), data)) : ((kind === "tool.progress")) ? append_item_delta_bang(runtime, stream_state, data, id, "tool", (existing ? conversationitem_title(existing) : "Tool activity"), wire_content_text(data.progress), "running") : ((kind === "tool.terminal")) ? upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "tool", (existing ? conversationitem_title(existing) : "Tool activity"), text_or(data.resultPreview, (existing ? conversationitem_body(existing) : "")), ((text(data.status) === "succeeded") ? "done" : "failed"), (existing ? conversationitem_data(existing) : null), text(stream_state.executionId), data)) : null);
}

function handle_record_bang(runtime, stream_state, record) {
  const kind = parsedrecord_kind(record);
  const data = (parsedrecord_data(record) || {});
  const execution_id = text(stream_state.executionId);
  return (((kind === "execution.accepted")) ? (() => { const cwd = text(data.cwd); const prompt = text(data.prompt).trim(); if ((!(cwd === ""))) {
  (runtime.sessionCwd = cwd);
}
if (((!stream_state.booting) && (!(prompt === "")))) {
  return upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "user", "", prompt, "done"));
} })() : ((kind === "session.config")) ? (() => { const model = text(data.model).trim(); const effort = text(data.effort).trim(); const cwd = text(data.cwd).trim(); const permissions = text(data.permissionMode).trim(); if ((!(model === ""))) {
  (runtime.sessionModel = model);
}
if ((!(effort === ""))) {
  (runtime.sessionEffort = effort);
}
if ((!(cwd === ""))) {
  (runtime.sessionCwd = cwd);
}
if ((!(permissions === ""))) {
  return (runtime.sessionPermissions = permissions);
} })() : (((kind === "control.submit_input") || (kind === "control.redirect_now"))) ? (() => { const input = text(data.input).trim(); if ((!(input === ""))) {
  return upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "user", "", input, "done"));
} })() : ((kind === "control.interrupt_turn")) ? upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "interrupted", "", "Conversation interrupted — tell the model what to do differently.", "interrupted")) : ((kind === "model-call.started")) ? (() => { const booting = stream_state.booting; adopt_wire_model_bang(runtime, data.model, data.effort);
set_execution_working_bang(runtime, execution_id, true, (booting ? ("".concat("Starting ", main_agent_label(runtime), "…")) : "Agent is working"));
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), (booting ? "starting" : "working"));
} })() : ((kind === "message.recorded")) ? handle_wire_message_bang(runtime, stream_state, data) : (((kind === "tool.admitted") || (kind === "tool.progress") || (kind === "tool.terminal"))) ? handle_wire_tool_bang(runtime, stream_state, data, kind) : ((kind === "run.progress")) ? (() => { const progress = (data.progress || {}); const action = text(progress.currentAction); const lifecycle = text(data.lifecycle); adopt_wire_model_bang(runtime, progress.model, progress.effort);
if (progress.branch) {
  (runtime.sessionBranch = text(progress.branch.name));
}
if ((lifecycle === "waiting")) {
  return set_working_bang(runtime, false, "");
} else {
  if ((!(action === ""))) {
    return set_working_bang(runtime, true, action);
  }
} })() : ((kind === "artifact.published")) ? (() => { const id = event_item_id(execution_id, data.artifactId); const existing = conversation_item_by_id(runtime, id); return upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "change", text_or(data.label, "Published artifact"), text(data.mediaType), "done", null, execution_id, data)); })() : ((kind === "model-call.completed")) ? ((!intermediate_provider_session_replacement_p(data)) ? (() => { return set_execution_working_bang(runtime, execution_id, false, ""); })() : null) : ((kind === "session.idle")) ? (() => { const disposition = text(data.disposition); const pending_inputs = Number((data.pendingInputs || 0)); const booting = stream_state.booting; set_execution_working_bang(runtime, execution_id, false, "");
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
} })() : ((kind === "run.terminated")) ? (() => { set_execution_working_bang(runtime, execution_id, false, "");
if (((text(data.lifecycle) === "failed") || (text(data.lifecycle) === "blocked"))) {
  play_sound_event_bang(runtime, stream_state, "failed");
  const id = event_item_id(execution_id, ("".concat("terminal:", data.sequence)));
  const existing = conversation_item_by_id(runtime, id);
  upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "error", "Error", failure_summary(data.reason), "failed", null, execution_id, data));
}
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "offline");
} })() : ((kind === "execution.failure")) ? (() => { set_execution_working_bang(runtime, execution_id, false, "");
return upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "error", "Error", ("".concat(kind, ": ", failure_summary(data))), "failed")); })() : null);
}

export function parse_bridge_stream_bang(runtime, stream_state, chunk) {
  const lines = ("".concat(stream_state.buffer, chunk)).split("\n");
  const remainder = lines.pop();
  (stream_state.buffer = remainder);
  return lines.forEach((raw_line) => { const line = raw_line.trim();
return ((line.startsWith("execution ")) ? (() => { const execution_id = line.slice(10).trim(); (stream_state.executionId = execution_id);
(stream_state.soundLive = true);
return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "starting"); })() : (line.startsWith("attached ")) ? (stream_state.soundLive = true) : (line.startsWith("[")) ? (() => { const record = record_line(line); if (record) {
  return handle_record_bang(runtime, stream_state, record);
} })() : (line.startsWith("northd: ")) ? publish_line_bang(runtime, line.slice(8)) : (line.startsWith("north bridge:")) ? append_error_bang(runtime, line) : null); });
}

export function launch_route_flags(provider, tier, model, effort) {
  const provider_value = text(provider).trim();
  const tier_value = text(tier).trim();
  const model_value = text(model).trim();
  const effort_value = text(effort).trim();
  return [].concat(((provider_value === "") ? [] : ["--provider", provider_value]), ((tier_value === "") ? [] : ["--tier", tier_value]), ((model_value === "") ? [] : ["--model", model_value]), ((effort_value === "") ? [] : ["--effort", effort_value]));
}

function launch_route_summary(runtime) {
  return ("".concat("next launch: provider ", text_or(runtime.launchProvider, "auto"), ", model ", text_or(text_or(runtime.launchModel, text(runtime.launchTier)), "auto"), ", effort ", text_or(runtime.launchEffort, "auto")));
}

export function set_launch_route_bang(runtime, name, value) {
  const trimmed = value.trim();
  const choice = trimmed.toLowerCase();
  if ((choice === "")) {
    (() => { throw new Error(("".concat(name, " requires a value or auto"))); })();
  }
  if ((name === "provider")) {
    if ((!((choice === "auto") || (choice === "anthropic") || (choice === "openai")))) {
      (() => { throw new Error("provider requires anthropic, openai, or auto"); })();
    }
    (runtime.launchProvider = ((choice === "auto") ? "" : choice));
  } else if ((name === "model")) {
    if ((choice === "auto")) {
      (runtime.launchTier = "");
      (runtime.launchModel = "");
    } else {
      if (["economy", "standard", "senior", "frontier"].includes(choice)) {
        (runtime.launchTier = choice);
        (runtime.launchModel = "");
      } else {
        (runtime.launchTier = "");
        (runtime.launchModel = trimmed);
      }
    }
  } else {
    if ((!((choice === "auto") || ["low", "medium", "high", "xhigh", "max"].includes(choice)))) {
      (() => { throw new Error("effort requires low, medium, high, xhigh, max, or auto"); })();
    }
    (runtime.launchEffort = ((choice === "auto") ? "" : choice));
  }
  return publish_line_bang(runtime, launch_route_summary(runtime));
}

export function take_launch_route_flags_bang(runtime) {
  const flags = launch_route_flags(runtime.launchProvider, runtime.launchTier, runtime.launchModel, runtime.launchEffort);
  (runtime.launchProvider = "");
  (runtime.launchTier = "");
  (runtime.launchModel = "");
  (runtime.launchEffort = "");
  return flags;
}

function main_agent_label(runtime) {
  const value = text(runtime.launchProvider).trim();
  return (((value === "anthropic")) ? "Claude Main" : ((value === "openai")) ? "Codex Main" : "Main");
}

async function launch_agent_bang(runtime, prompt, role) {
  if ((prompt.trim() === "")) {
    (() => { throw new Error("launch requires a prompt"); })();
  }
  set_working_bang(runtime, true, ("".concat("Starting ", main_agent_label(runtime), "…")));
  const stream_state = {buffer: "", stderr: "", executionId: "", role: role, booting: (role === "supervisor"), soundLive: false};
  const exit_code = await stream_command([north_bin(), "bridge", "--role", ((role === "supervisor") ? "director" : "implementer")].concat(take_launch_route_flags_bang(runtime), [prompt]), (chunk) => parse_bridge_stream_bang(runtime, stream_state, chunk), (chunk) => (stream_state.stderr = clipped(("".concat(stream_state.stderr, chunk)), 6000)));
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
  const argv = ((ghostty) ? [ghostty, "-e", north_bin(), "bridge", "app", "--view-id", view_id] : (kitty) ? [kitty, "--detach", north_bin(), "bridge", "app", "--view-id", view_id] : (wezterm) ? [wezterm, "start", "--always-new-process", "--", north_bin(), "bridge", "app", "--view-id", view_id] : (foot) ? [foot, north_bin(), "bridge", "app", "--view-id", view_id] : (xterm) ? [xterm, "-e", north_bin(), "bridge", "app", "--view-id", view_id] : null);
  if ((argv == null)) {
    (() => { throw new Error("no supported terminal found for pop-out"); })();
  }
  const child = Bun.spawn({cmd: argv, stdin: "ignore", stdout: "ignore", stderr: "ignore"});
  child.unref();
  return publish_line_bang(runtime, ("".concat("opened ", view_id, " in a separate terminal")));
}

function selected_work(runtime, selection) {
  const state = snapshot(runtime.model);
  const view_id = text_or(workselection_view(selection), text(runtime.activeView));
  const view = selected_view(state, view_id);
  const items = workview_items(view);
  const index = workselection_index(selection);
  if (((index >= 0) && (index < items.length))) {
    const item = items[index];
    return (((!(workview_id(view) === "list")) || (!runtime.collapsedListConditions.has(list_section_id(workitem_condition(item))))) ? item : null);
  } else {
    return null;
  }
}

async function capture_thread_bang(runtime, title) {
  if ((title === "")) {
    (() => { throw new Error("capture requires a title"); })();
  }
  const output = await run_command([north_bin(), "capture", title]);
  publish_line_bang(runtime, text(output).trim());
  return await refresh_bang(runtime);
}

export function restore_submitted_text_bang(runtime, ui) {
  const pending = text(runtime.lastSubmitted);
  const input = ui.composerInput;
  if (((!(pending === "")) && input)) {
    (runtime.lastSubmitted = "");
    (input.value = pending);
    return input.focus();
  }
}

async function cancel_turn_bang(runtime, ui, target) {
  if ((!runtime.bridgeExecutions.has(target))) {
    (() => { throw new Error("interrupt is available for Bridge-launched executions"); })();
  }
  await run_command([north_bin(), "bridge", "interrupt", target]);
  set_execution_working_bang(runtime, target, false, "");
  return restore_submitted_text_bang(runtime, ui);
}

async function submit_agent_bang(runtime, ui, input, selection) {
  const trimmed = input.trim();
  const slash_p = trimmed.startsWith("/");
  const parsed = command(input);
  const name = parsedcommand_name(parsed);
  const rest = parsedcommand_rest(parsed);
  const target = text_or(selection, text(runtime.supervisorId));
  return (handle_local_command_bang(runtime, ui, input) ? null : (((slash_p && (name === "launch"))) ? await launch_agent_bang(runtime, rest, "worker") : ((slash_p && (name === "capture"))) ? await capture_thread_bang(runtime, rest) : ((slash_p && (name === "refresh"))) ? await refresh_bang(runtime) : ((slash_p && (name === "popout"))) ? popout_bang(runtime, text_or(rest, text(runtime.activeView))) : (async () => { if ((target === "")) {
  (() => { throw new Error("select an agent before messaging or interrupting"); })();
}
if ((slash_p && (name === "interrupt"))) {
  return await cancel_turn_bang(runtime, ui, target);
} else {
  const message = trimmed;
  if ((message === "")) {
    (() => { throw new Error("nothing to send"); })();
  }
  if ((!runtime.bridgeExecutions.has(target))) {
    const id = next_item_id_bang(runtime, "user");
    upsert_conversation_bang(runtime, owned_conversation_item(id, "user", "", message, "done", null, target, new Date().toISOString(), runtime.itemSequence, 1));
  }
  (runtime.lastSubmitted = message);
  if (runtime.bridgeExecutions.has(target)) {
    set_execution_working_bang(runtime, target, true, "Codex is working");
  } else {
    set_working_bang(runtime, true, "Codex is working");
  }
  if (runtime.bridgeExecutions.has(target)) {
    await run_command([north_bin(), "bridge", "msg", target, message]);
  } else {
    await run_command([north_bin(), "msg", target, message]);
  }
  return runtime.render();
} })()));
}

async function submit_work_bang(runtime, ui, input, selection) {
  const trimmed = input.trim();
  const slash_p = trimmed.startsWith("/");
  const parsed = command(input);
  const name = parsedcommand_name(parsed);
  const rest = parsedcommand_rest(parsed);
  return ((!slash_p) ? await submit_agent_bang(runtime, ui, input, runtime.supervisorId) : (handle_local_command_bang(runtime, ui, input) ? null : (((name === "filter")) ? (() => { (runtime.model = set_filter(runtime.model, rest));
return runtime.render(); })() : ((name === "refresh")) ? await refresh_bang(runtime) : ((name === "popout")) ? popout_bang(runtime, ((rest === "") ? runtime.activeView : rest)) : ((name === "capture")) ? await capture_thread_bang(runtime, rest) : ((name === "assign")) ? (async () => { const item = selected_work(runtime, selection); const words = rest.split(" ").filter((word) => (!(word.trim() === ""))); const current = (item ? workitem_driver(item) : ""); const prior = ((words.length > 1) ? words[0] : current); const next_driver = ((words.length > 1) ? words[1] : text(words[0])); const thread_id = (item ? workitem_id(item) : ""); if ((thread_id === "")) {
  (() => { throw new Error("select a thread before assigning"); })();
}
if ((prior === "")) {
  (() => { throw new Error("an unassigned thread requires: /assign <prior-driver> <next-driver>"); })();
}
if ((next_driver === "")) {
  (() => { throw new Error("assign requires a new driver"); })();
}
publish_line_bang(runtime, "driver reassignment is a retract-then-tell operation");
await run_command([north_bin(), "retract", thread_id, "driver", prior]);
await run_command([north_bin(), "tell", thread_id, "driver", next_driver]);
publish_line_bang(runtime, ("".concat("assigned @", thread_id, " to ", next_driver)));
return await refresh_bang(runtime); })() : ((name === "outcome")) ? (async () => { const split_at = rest.indexOf(" "); const thread_id = ((split_at < 0) ? "" : bare(rest.slice(0, split_at))); const result = ((split_at < 0) ? "" : rest.slice((split_at + 1)).trim()); if ((thread_id === "")) {
  (() => { throw new Error("outcome requires: /outcome <thread-id> <result>"); })();
}
if ((result === "")) {
  (() => { throw new Error("outcome requires a result"); })();
}
await run_command([north_bin(), "tell", thread_id, "outcome", result]);
(runtime.workspaceNotice = ("".concat("Recorded outcome for @", thread_id, ".")));
return await refresh_bang(runtime); })() : (() => { throw new Error("unknown thread command; use /help"); })())));
}

function report_promise_bang(runtime, promise) {
  return promise.catch((error) => publish_line_bang(runtime, ("".concat("error: ", error_message(error)))));
}

function select_frame_bang(runtime, frame) {
  (runtime.frame = frame);
  (runtime.paletteIndex = 0);
  (runtime.workspaceNotice = "");
  return clear_strip_focus_bang(runtime);
}

function show_frame_bang(runtime, ui, frame) {
  select_frame_bang(runtime, frame);
  ui.composerInput.focus();
  return runtime.render();
}

function show_thread_view_bang(runtime, ui, view_id) {
  (runtime.model = focus_view(runtime.model, canonical_work_view(view_id)));
  (runtime.workIndex = 0);
  runtime.workScroll.scrollTo(0);
  return show_frame_bang(runtime, ui, "threads");
}

function strip_segments(runtime) {
  return agent_segments(bridgesnapshot_agents(snapshot(runtime.model)));
}

function focus_strip_bang(runtime, ui) {
  (runtime.stripFocused = true);
  ui.composerInput.blur();
  return runtime.render();
}

function leave_strip_bang(runtime, ui) {
  (runtime.stripFocused = false);
  if (detail_showing_p(runtime, "agents")) {
    close_detail_bang(runtime);
  }
  ui.composerInput.focus();
  return runtime.render();
}

function clear_strip_focus_bang(runtime) {
  if (runtime.stripFocused) {
    (runtime.stripFocused = false);
    if (detail_showing_p(runtime, "agents")) {
      return close_detail_bang(runtime);
    }
  }
}

function show_segment_detail_bang(runtime, index) {
  const segments = strip_segments(runtime);
  if ((segments.length > 0)) {
    const selected = clamped_index(index, segments.length);
    (runtime.stripIndex = selected);
    (runtime.detailSegment = agentsegment_id(segments[selected]));
    (runtime.detailIndex = 0);
    return open_detail_bang(runtime, "agents");
  }
}

function toggle_segment_detail_bang(runtime) {
  return (detail_showing_p(runtime, "agents") ? close_detail_bang(runtime) : show_segment_detail_bang(runtime, runtime.stripIndex));
}

function move_strip_segment_bang(runtime, delta) {
  const segments = strip_segments(runtime);
  const total = segments.length;
  if ((total > 0)) {
    const next_index = ((runtime.stripIndex + delta + total) % total);
    (runtime.stripIndex = next_index);
    (runtime.detailIndex = 0);
    if (detail_showing_p(runtime, "agents")) {
      return (runtime.detailSegment = agentsegment_id(segments[next_index]));
    }
  }
}

function move_detail_cursor_bang(runtime, delta) {
  const total = detail_agents(runtime).length;
  if ((total > 0)) {
    return (runtime.detailIndex = ((clamped_index(runtime.detailIndex, total) + delta + total) % total));
  }
}

function active_input(__runtime, ui) {
  return ui.composerInput;
}

function active_palette_options(runtime, ui) {
  const frame = text(runtime.frame);
  const input = active_input(runtime, ui);
  return palette_options(frame, text(input.value));
}

function install_composer_keymap_bang(runtime) {
  const keymap = createDefaultOpenTuiKeymap(runtime.renderer);
  registerEmacsBindings(keymap);
  registerEscapeClearsPendingSequence(keymap);
  return (runtime.keymap = keymap);
}

function palette_completion_text(candidate) {
  const completion = slashcommand_completion(candidate);
  return ((completion === "") ? ("".concat(slashcommand_name(candidate), (slashcommand_arguments(candidate) ? " " : ""))) : completion);
}

function palette_candidate(runtime, commands) {
  return commands[Math.max(0, Math.min(runtime.paletteIndex, (commands.length - 1)))];
}

function complete_palette_bang(runtime, ui, commands) {
  if ((commands.length > 0)) {
    const candidate = palette_candidate(runtime, commands);
    const input = active_input(runtime, ui);
    (input.value = palette_completion_text(candidate));
    (runtime.paletteIndex = 0);
    input.focus();
    return render_minibuffer_bang(runtime, ui);
  }
}

export function submit_input_bang(runtime, ui, input) {
  if ((!(input === ""))) {
    (ui.composerInput.value = "");
    (runtime.paletteIndex = 0);
    if ((!threads_frame_p(runtime.frame))) {
      const state = snapshot(runtime.model);
      const selected = selected_agent_id(state, runtime.agentIndex);
      return report_promise_bang(runtime, submit_agent_bang(runtime, ui, input, selected));
    } else {
      return report_promise_bang(runtime, submit_work_bang(runtime, ui, input, WorkSelection(runtime.activeView, runtime.workIndex)));
    }
  }
}

export function palette_enter_action(matches, takes_arguments_p, insert_only_p, completed_p) {
  return (((matches < 1)) ? "" : (completed_p) ? "fire" : ((insert_only_p || takes_arguments_p)) ? "complete" : "fire");
}

function install_input_bang(runtime, ui) {
  ui.composerInput.on(InputRenderableEvents.INPUT, (__value) => { (runtime.paletteIndex = 0);
return render_minibuffer_bang(runtime, ui); });
  return ui.composerInput.on(InputRenderableEvents.ENTER, () => submit_input_bang(runtime, ui, text(ui.composerInput.value).trim()));
}

function subview_tab_id_at_bang(views, column) {
  const cursor = {x: SUBVIEW_TAB_ORIGIN, id: ""};
  views.forEach((view) => { const start = cursor.x;
const width = (workview_title(view).length + 2);
if (((column >= start) && (column < (start + width)))) {
  (cursor.id = workview_id(view));
}
return (cursor.x = (start + width + SUBVIEW_TAB_GAP)); });
  return text(cursor.id);
}

export function view_tab_id_at_bang(frame, views, column) {
  return ((((column >= 0) && (column < AGENTS_TAB_LABEL.length))) ? "agents" : (((column >= THREADS_TAB_START) && (column < (THREADS_TAB_START + THREADS_TAB_LABEL.length)))) ? "threads" : ((threads_frame_p(frame) && (column >= SUBVIEW_TAB_ORIGIN))) ? subview_tab_id_at_bang(views, column) : "");
}

function view_tab_at_bang(runtime, tabs, event, views) {
  return view_tab_id_at_bang(runtime.frame, views, Math.floor((event.x - tabs.x)));
}

function complete_clicked_palette_bang(runtime, ui, frame, palette_renderable, event) {
  if ((event.button === 0)) {
    select_frame_bang(runtime, frame);
    const input = ui.composerInput;
    const options = palette_options(frame, text(input.value));
    const row = Math.floor((event.y - palette_renderable.y));
    const scrolled = runtime.paletteStart;
    const start = (scrolled ? scrolled : 0);
    const drawn = runtime.paletteRows;
    const rows = (drawn ? drawn : 0);
    const picked = (start + row);
    if (((row >= 0) && (row < rows) && (picked < options.length))) {
      event.preventDefault();
      event.stopPropagation();
      (runtime.paletteIndex = picked);
      return complete_palette_bang(runtime, ui, options);
    }
  }
}

function visible_list_indices(runtime, items) {
  return list_rows(runtime, items).filter((row) => (listrow_kind(row) === "item")).map((row) => listrow_index(row));
}

function next_visible_list_index(runtime, items, current, delta) {
  const indices = visible_list_indices(runtime, items);
  const position = indices.indexOf(current);
  const start = ((position >= 0) ? position : ((delta > 0) ? -1 : indices.length));
  const next = Math.max(0, Math.min((indices.length - 1), (start + delta)));
  return ((indices.length > 0) ? indices[next] : current);
}

function select_visible_list_fallback_bang(runtime, items) {
  const indices = visible_list_indices(runtime, items);
  const current = runtime.workIndex;
  if (((indices.length > 0) && (!indices.includes(current)))) {
    const index = indices[0];
    const item = items[index];
    (runtime.workIndex = index);
    return (runtime.model = select_thread(runtime.model, workitem_id(item)));
  }
}

function handle_list_click_bang(runtime, ui, event) {
  if ((event.button === 0)) {
    const state = snapshot(runtime.model);
    const view = selected_view(state, runtime.activeView);
    if ((workview_id(view) === "list")) {
      const items = workview_items(view);
      const rows = list_rows(runtime, items);
      const row_index = Math.floor((event.y - ui.workText.screenY));
      if (((row_index >= 0) && (row_index < rows.length))) {
        const row = rows[row_index];
        event.preventDefault();
        event.stopPropagation();
        if ((listrow_kind(row) === "header")) {
          const condition = listrow_condition(row);
          const collapsed = runtime.collapsedListConditions;
          if (collapsed.has(condition)) {
            collapsed.delete(condition);
          } else {
            collapsed.add(condition);
          }
          select_visible_list_fallback_bang(runtime, items);
        } else {
          const index = listrow_index(row);
          const item = items[index];
          (runtime.workIndex = index);
          (runtime.model = select_thread(runtime.model, workitem_id(item)));
        }
        return runtime.render();
      }
    }
  }
}

function install_mouse_bang(runtime, ui) {
  (ui.composer.onMouseDown = (event) => { if ((event.button === 0)) {
  event.stopPropagation();
  clear_strip_focus_bang(runtime);
  ui.composerInput.focus();
  return runtime.render();
} });
  (ui.agentStripText.onMouseDown = (event) => { if ((event.button === 0)) {
  event.preventDefault();
  event.stopPropagation();
  const segments = strip_segments(runtime);
  const column = Math.floor((event.x - ui.agentStripText.screenX));
  const index = segment_at_column(segments, column);
  if ((index >= 0)) {
    show_segment_detail_bang(runtime, index);
  }
  return focus_strip_bang(runtime, ui);
} });
  (ui.composerPalette.onMouseDown = (event) => complete_clicked_palette_bang(runtime, ui, text(runtime.frame), ui.composerPalette, event));
  (ui.workText.onMouseDown = (event) => handle_list_click_bang(runtime, ui, event));
  return (ui.viewTabsText.onMouseDown = (event) => { if ((event.button === 0)) {
  const tab = view_tab_at_bang(runtime, ui.viewTabsText, event, view_list(snapshot(runtime.model)));
  if ((!(tab === ""))) {
    event.preventDefault();
    event.stopPropagation();
    return ((tab === "agents") ? show_frame_bang(runtime, ui, "agents") : ((tab === "threads") ? show_frame_bang(runtime, ui, "threads") : show_thread_view_bang(runtime, ui, tab)));
  }
} });
}

function escape_step_bang(runtime, ui) {
  const palette = active_palette_options(runtime, ui);
  const action = escape_rung((palette.length > 0), panel_filtering_p(runtime), detail_open_p(runtime), (runtime.stripFocused ? true : false), threads_frame_p(runtime.frame), (runtime.working ? true : false));
  return (((action === "close-palette")) ? (() => { (active_input(runtime, ui).value = "");
render_minibuffer_bang(runtime, ui);
return true; })() : ((action === "clear-filter")) ? (() => { clear_panel_filter_bang(runtime);
runtime.render();
return true; })() : ((action === "close-detail")) ? (() => { close_detail_bang(runtime);
focus_composer_bang(runtime, ui);
runtime.render();
return true; })() : ((action === "focus-composer")) ? (() => { leave_strip_bang(runtime, ui);
runtime.render();
return true; })() : ((action === "show-agents")) ? (() => { show_frame_bang(runtime, ui, "agents");
return true; })() : ((action === "cancel-turn")) ? (() => { const target = text(runtime.supervisorId); if ((!(target === ""))) {
  report_promise_bang(runtime, cancel_turn_bang(runtime, ui, target));
}
return true; })() : false);
}

function panel_filterable_p(runtime) {
  return (detail_showing_p(runtime, "config") || detail_showing_p(runtime, "help"));
}

export function filter_character(name, sequence, ctrl_p, meta_p) {
  return ((ctrl_p || meta_p || (name === "space") || (!(sequence.length === 1)) || (sequence.charCodeAt(0) < 32)) ? "" : sequence);
}

export function filter_key_action(filtering_p, query, name, character) {
  return ((((!filtering_p) && (character === "/"))) ? "open" : ((!filtering_p)) ? "" : ((name === "backspace")) ? ((query === "") ? "close" : "erase") : ((!(character === ""))) ? "type" : "");
}

function ctrl_down_key_p(name, key) {
  return (key.ctrl && (name === "j"));
}

function bare_letter_p(name, key, letter) {
  return ((name === letter) && (!key.ctrl) && (!(key.meta || key.option)));
}

function fold_key_p(runtime, name, key) {
  return ((name === "left") || (name === "right") || ((!panel_filtering_p(runtime)) && (bare_letter_p(name, key, "h") || bare_letter_p(name, key, "l"))));
}

function apply_fold_action_bang(runtime, rows, node, action) {
  return (((action === "expand")) ? set_node_expanded_bang(runtime, node, true) : ((action === "collapse")) ? set_node_expanded_bang(runtime, node, false) : ((action === "climb")) ? (() => { const at = rows.findIndex((row) => ((configentry_kind(row) === "dir") && (configentry_name(row) === node))); if ((at >= 0)) {
  return (runtime.configIndex = at);
} })() : null);
}

function fold_step_bang(runtime, open_key_p) {
  const rows = config_panel_rows(runtime);
  const total = rows.length;
  if ((total > 0)) {
    const index = clamped_index(runtime.configIndex, total);
    const entry = rows[index];
    const dir_p = (configentry_kind(entry) === "dir");
    const node = config_row_node(entry);
    return apply_fold_action_bang(runtime, rows, node, fold_key_action(dir_p, config_node_expanded_p(panel_expanded(runtime), node), open_key_p));
  }
}

export function tab_fold_step_bang(runtime) {
  const rows = config_panel_rows(runtime);
  const total = rows.length;
  if ((total > 0)) {
    const index = clamped_index(runtime.configIndex, total);
    const entry = rows[index];
    const dir_p = (configentry_kind(entry) === "dir");
    const node = config_row_node(entry);
    return apply_fold_action_bang(runtime, rows, node, tab_action("panel", dir_p, config_node_expanded_p(panel_expanded(runtime), node)));
  }
}

function ctrl_up_key_p(name, key) {
  return (key.ctrl && (name === "k"));
}

function bare_key_p(name, key, letter) {
  return ((name === letter) && (!key.ctrl) && (!(key.meta || key.option)));
}

function strip_key_p(name, key) {
  return ((name === "left") || (name === "right") || (name === "up") || (name === "down") || submit_key_p(name) || ctrl_down_key_p(name, key) || ctrl_up_key_p(name, key) || bare_key_p(name, key, "h") || bare_key_p(name, key, "l") || bare_key_p(name, key, "j") || bare_key_p(name, key, "k"));
}

function install_keys_bang(runtime, ui) {
  return runtime.renderer.keyInput.on("keypress", (key) => { if (((!key.defaultPrevented) && (!key.propagationStopped))) {
  const name = text(key.name).toLowerCase();
  const meta = (key.meta || key.option);
  const palette = active_palette_options(runtime, ui);
  const palette_open = (palette.length > 0);
  const plain_view_arrow = (threads_frame_p(runtime.frame) && (text(ui.composerInput.value).trim() === "") && (!key.ctrl) && (!meta) && ((name === "left") || (name === "right")));
  return ((((name === "escape") || (name === "esc"))) ? (escape_step_bang(runtime, ui) ? (() => { key.preventDefault();
return key.stopPropagation(); })() : null) : ((detail_open_p(runtime) && (!panel_focused_p(runtime)) && (!palette_open) && ctrl_down_key_p(name, key))) ? (() => { key.preventDefault();
key.stopPropagation();
focus_panel_bang(runtime, ui);
return runtime.render(); })() : ((detail_showing_p(runtime, "config") && panel_focused_p(runtime) && (!palette_open) && ((name === "up") || (name === "down") || ctrl_up_key_p(name, key) || ctrl_down_key_p(name, key) || (name === "space") || submit_key_p(name) || (name === "tab") || fold_key_p(runtime, name, key)))) ? (() => { const up_p = ((name === "up") || ctrl_up_key_p(name, key)); const down_p = ((name === "down") || ctrl_down_key_p(name, key)); key.preventDefault();
key.stopPropagation();
if ((up_p || down_p)) {
  const total = config_panel_rows(runtime).length;
  if ((total > 0)) {
    const raw = runtime.configIndex;
    const current = (raw ? raw : 0);
    const delta = (up_p ? -1 : 1);
    (runtime.configIndex = ((current + delta + total) % total));
  }
} else if ((name === "tab")) {
  tab_fold_step_bang(runtime);
} else if (fold_key_p(runtime, name, key)) {
  fold_step_bang(runtime, ((name === "right") || bare_key_p(name, key, "l")));
} else if ((name === "space")) {
  report_promise_bang(runtime, toggle_config_entry_bang(runtime));
} else {
  report_promise_bang(runtime, edit_config_entry_bang(runtime));
}
return runtime.render(); })() : ((detail_showing_p(runtime, "config") && panel_focused_p(runtime) && (!panel_filtering_p(runtime)) && (!palette_open) && (filter_character(name, text(key.sequence), (key.ctrl ? true : false), (meta ? true : false)) === "@"))) ? (() => { const rows = config_panel_rows(runtime); const total = rows.length; key.preventDefault();
key.stopPropagation();
if ((total > 0)) {
  const entry = rows[clamped_index(runtime.configIndex, total)];
  const input = active_input(runtime, ui);
  (input.value = ("".concat(text(input.value), config_reference_text(configentry_kind(entry), configentry_name(entry)))));
  focus_composer_bang(runtime, ui);
  render_minibuffer_bang(runtime, ui);
}
return runtime.render(); })() : ((panel_filterable_p(runtime) && panel_focused_p(runtime) && (!palette_open) && (!(filter_key_action(panel_filtering_p(runtime), panel_query(runtime), name, filter_character(name, text(key.sequence), (key.ctrl ? true : false), (meta ? true : false))) === "")))) ? (() => { const character = filter_character(name, text(key.sequence), (key.ctrl ? true : false), (meta ? true : false)); const query = panel_query(runtime); const action = filter_key_action(panel_filtering_p(runtime), query, name, character); key.preventDefault();
key.stopPropagation();
if ((action === "open")) {
  set_panel_query_bang(runtime, "");
} else if ((action === "type")) {
  set_panel_query_bang(runtime, ("".concat(query, character)));
} else if ((action === "erase")) {
  set_panel_query_bang(runtime, query.slice(0, (query.length - 1)));
} else {
  clear_panel_filter_bang(runtime);
}
return runtime.render(); })() : ((panel_focused_p(runtime) && (!palette_open) && (!(filter_character(name, text(key.sequence), (key.ctrl ? true : false), (meta ? true : false)) === "")))) ? (() => { key.preventDefault();
return key.stopPropagation(); })() : ((runtime.stripFocused && strip_key_p(name, key))) ? (() => { const expanded_p = detail_showing_p(runtime, "agents"); const up_p = ((name === "up") || ctrl_up_key_p(name, key) || bare_key_p(name, key, "k")); const down_p = ((name === "down") || ctrl_down_key_p(name, key) || bare_key_p(name, key, "j")); const left_p = ((name === "left") || bare_key_p(name, key, "h")); const right_p = ((name === "right") || bare_key_p(name, key, "l")); key.preventDefault();
key.stopPropagation();
if (submit_key_p(name)) {
  toggle_segment_detail_bang(runtime);
} else if (left_p) {
  move_strip_segment_bang(runtime, -1);
} else if (right_p) {
  move_strip_segment_bang(runtime, 1);
} else if (up_p) {
  if (expanded_p) {
    move_detail_cursor_bang(runtime, -1);
  } else {
    leave_strip_bang(runtime, ui);
  }
} else if (down_p) {
  if (expanded_p) {
    move_detail_cursor_bang(runtime, 1);
  } else {
    toggle_segment_detail_bang(runtime);
  }
} else {
  null;
}
return runtime.render(); })() : ((palette_open && ((name === "up") || (name === "down") || (key.ctrl && ((name === "j") || (name === "k")))))) ? (() => { key.preventDefault();
key.stopPropagation();
(runtime.paletteIndex = ((runtime.paletteIndex + (((name === "up") || (key.ctrl && (name === "k"))) ? -1 : 1) + palette.length) % palette.length));
active_input(runtime, ui).focus();
return render_minibuffer_bang(runtime, ui); })() : ((palette_open && (name === "tab"))) ? (() => { key.preventDefault();
key.stopPropagation();
return complete_palette_bang(runtime, ui, palette); })() : ((palette_open && submit_key_p(name))) ? (() => { const candidate = palette_candidate(runtime, palette); const completed = palette_completion_text(candidate); const current = text(active_input(runtime, ui).value); const action = palette_enter_action(palette.length, slashcommand_arguments(candidate), slashcommand_emoji(candidate), (current === completed)); key.preventDefault();
key.stopPropagation();
return ((action === "fire") ? submit_input_bang(runtime, ui, completed.trim()) : complete_palette_bang(runtime, ui, palette)); })() : (((!runtime.stripFocused) && (!palette_open) && (ctrl_down_key_p(name, key) || ((name === "down") && (!key.ctrl) && (!meta) && (text(ui.composerInput.value).trim() === ""))))) ? (() => { key.preventDefault();
key.stopPropagation();
return focus_strip_bang(runtime, ui); })() : (((name === "tab") || (name === "f2"))) ? (() => { key.preventDefault();
key.stopPropagation();
return show_frame_bang(runtime, ui, tab_swap_frame(text(runtime.frame))); })() : ((name === "f1")) ? (() => { key.preventDefault();
key.stopPropagation();
return toggle_help_bang(runtime, ui); })() : (((name === "f3") || plain_view_arrow || (meta && ((name === "h") || (name === "l"))))) ? (() => { const state = snapshot(runtime.model); const views = view_list(state); const current = selected_view(state, runtime.activeView); const index = views.findIndex((view) => (workview_id(view) === workview_id(current))); const delta = (((name === "left") || (meta && (name === "h"))) ? -1 : 1); const next_index = ((index + delta + views.length) % views.length); const next_id = text(views[next_index].id); key.preventDefault();
key.stopPropagation();
return show_thread_view_bang(runtime, ui, next_id); })() : (((name === "f5") || (key.ctrl && (name === "r")))) ? (() => { key.preventDefault();
key.stopPropagation();
return report_promise_bang(runtime, refresh_bang(runtime)); })() : (((name === "f6") || (key.ctrl && (name === "o")))) ? (() => { key.preventDefault();
key.stopPropagation();
return popout_bang(runtime, runtime.activeView); })() : ((meta && ((name === "j") || (name === "k")))) ? (() => { const state = snapshot(runtime.model); const delta = ((name === "k") ? -1 : 1); key.preventDefault();
key.stopPropagation();
if ((!threads_frame_p(runtime.frame))) {
  const agents = bridgesnapshot_agents(state);
  const max_index = Math.max(0, (agents.length - 1));
  const next_index = Math.max(0, Math.min(max_index, (runtime.agentIndex + delta)));
  const selected_agent_id = ((agents.length > 0) ? agent_id(agents[next_index]) : "");
  (runtime.agentIndex = next_index);
  (runtime.model = select_agent(runtime.model, selected_agent_id));
} else {
  const view = selected_view(state, runtime.activeView);
  const items = workview_items(view);
  const max_index = Math.max(0, (items.length - 1));
  const next_index = ((workview_id(view) === "list") ? next_visible_list_index(runtime, items, runtime.workIndex, delta) : Math.max(0, Math.min(max_index, (runtime.workIndex + delta))));
  const thread_id = ((items.length > 0) ? workitem_id(items[next_index]) : "");
  (runtime.workIndex = next_index);
  (runtime.model = select_thread(runtime.model, thread_id));
  ui.workScroll.scrollBy((delta * (((workview_id(view) === "board")) ? 2 : ((workview_id(view) === "graph")) ? 3 : 1)), "step");
}
return runtime.render(); })() : ((key.ctrl && (name === "c"))) ? (() => { const target = text(runtime.supervisorId); key.preventDefault();
key.stopPropagation();
if ((runtime.working && (!(target === "")))) {
  return report_promise_bang(runtime, cancel_turn_bang(runtime, ui, target));
} })() : null);
} });
}

async function open_app_bang(view_id, source_identity) {
  const view = canonical_work_view(view_id);
  const renderer = await createCliRenderer({exitOnCtrlC: false, clearOnShutdown: true});
  const runtime = {model: make_model(view), renderer: renderer, disposed: false, rendererSuspended: false, suspendResume: null, suspendError: "", frame: BOOT_FRAME, activeView: view, agentIndex: 0, workIndex: 0, collapsedListConditions: new Set(["blocked", "dormant", "draft", "terminal", "other"]), workScroll: null, boardSignature: "", dragThreadId: "", bridgeExecutions: new Set(), supervisorId: "", conversation: [], transcriptView: "selected", itemSequence: 0, lastAssistantText: "", lastSubmitted: "", working: false, workingExecutions: new Set(), workingLabel: "", workingSince: 0, spinnerIndex: 0, spinnerTimer: null, stripFocused: false, stripIndex: 0, detailView: "", detailSegment: "all", detailIndex: 0, paletteIndex: 0, paletteStart: 0, paletteRows: 0, promptGlyph: DEFAULT_PROMPT_GLYPH, soundEnabled: sound_enabled_from_env(text(process.env.NORTH_BRIDGE_SOUND)), soundPack: sound_pack_from_env(text(process.env.NORTH_BRIDGE_SOUND_PACK)), soundDirectory: sound_directory_from_env(text(process.env.NORTH_BRIDGE_SOUND_DIR)), soundPlayer: discover_sound_player(), soundChildren: new Set(), soundWarningShown: false, soundSequence: 0, lastSoundPath: "", lastSoundAt: 0, workspaceNotice: "", keymap: null, sessionModel: text_or(process.env.NORTH_BRIDGE_MODEL, text(process.env.AGENT_MODEL)), sessionEffort: text(process.env.AGENT_REASONING), launchProvider: text(process.env.NORTH_BRIDGE_PROVIDER), launchTier: text(process.env.NORTH_BRIDGE_TIER), launchModel: text(process.env.NORTH_BRIDGE_MODEL), launchEffort: text(process.env.NORTH_BRIDGE_EFFORT), sessionCwd: text(process.cwd()), sessionBranch: "", sessionPermissions: "", sourceIdentity: source_identity, renderConversation: () => null, render: () => null};
  const root = new BoxRenderable(renderer, {flexDirection: "column", width: "100%", height: "100%", gap: 0, paddingTop: 1, paddingBottom: 0, paddingLeft: 1, paddingRight: 1, onSizeChange: () => runtime.render()});
  const workspace = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", flexGrow: 1, gap: 0});
  const view_tabs_text = new TextRenderable(renderer, {height: 1, width: "100%", flexShrink: 0, wrapMode: "none", truncate: true});
  const agents_pane = new BoxRenderable(renderer, {flexDirection: "column", width: "100%"});
  const work_pane = new BoxRenderable(renderer, {flexDirection: "column", width: "100%"});
  const agents_text = new TextRenderable(renderer, {height: 4, flexShrink: 0, wrapMode: "word"});
  const transcript_scroll = new ScrollBoxRenderable(renderer, {flexGrow: 1, scrollY: true, stickyScroll: true, stickyStart: "bottom", viewportCulling: true, verticalScrollbarOptions: {visible: false}});
  const transcript_text_view = new TextRenderable(renderer, {width: "100%", flexShrink: 0, wrapMode: "word"});
  const work_scroll = new ScrollBoxRenderable(renderer, {flexGrow: 1, scrollY: true, viewportCulling: true, verticalScrollbarOptions: {visible: false}});
  const work_text_view = new TextRenderable(renderer, {width: "100%", flexShrink: 0, wrapMode: "none", truncate: true});
  const board_root = new BoxRenderable(renderer, {visible: false, width: "100%", flexDirection: "column", flexShrink: 0, gap: 1});
  const status_text = new TextRenderable(renderer, {flexShrink: 0, wrapMode: "word"});
  const agent_status_text = new TextRenderable(renderer, {visible: false, flexShrink: 0, wrapMode: "word"});
  const composer_palette = new TextRenderable(renderer, {visible: false, height: 1, width: "100%", flexShrink: 0, wrapMode: "none", truncate: true, bg: "#25272d"});
  const composer = new BoxRenderable(renderer, {flexDirection: "row", width: "100%", height: 1, paddingLeft: 1, paddingRight: 1, flexShrink: 0, backgroundColor: "#25272d"});
  const agent_strip_text = new TextRenderable(renderer, {height: 1, width: "100%", flexShrink: 0, wrapMode: "none", truncate: true});
  const detail_panel = new BoxRenderable(renderer, {visible: false, width: "100%", height: 5, flexDirection: "column", flexShrink: 0, border: true, borderColor: "#64748b", paddingLeft: 1, paddingRight: 1});
  const detail_text = new TextRenderable(renderer, {width: "100%", flexGrow: 1, wrapMode: "none"});
  const composer_prompt = new TextRenderable(renderer, {width: 2, height: 1, flexShrink: 0, wrapMode: "none", content: new StyledText([brightCyan("❯ ")])});
  const composer_input = new InputRenderable(renderer, {flexGrow: 1, flexShrink: 1, flexBasis: 0, minWidth: 0, backgroundColor: "#25272d", focusedBackgroundColor: "#25272d", textColor: "#e5e7eb", focusedTextColor: "#f8fafc", placeholderColor: "#6b7280", placeholder: composer_hint("agents", main_agent_label(runtime))});
  const ui = {root: root, agentsPane: agents_pane, workPane: work_pane, viewTabsText: view_tabs_text, agentsText: agents_text, transcriptText: transcript_text_view, workScroll: work_scroll, workText: work_text_view, boardRoot: board_root, statusText: status_text, agentStatusText: agent_status_text, composerPalette: composer_palette, composer: composer, composerPrompt: composer_prompt, composerInput: composer_input, agentStripText: agent_strip_text, detailPanel: detail_panel, detailText: detail_text};
  agents_pane.add(agents_text);
  transcript_scroll.add(transcript_text_view);
  agents_pane.add(transcript_scroll);
  agents_pane.add(agent_status_text);
  work_scroll.add(work_text_view);
  work_scroll.add(board_root);
  work_pane.add(work_scroll);
  work_pane.add(status_text);
  composer.add(composer_prompt);
  composer.add(composer_input);
  workspace.add(agents_pane);
  workspace.add(work_pane);
  root.add(workspace);
  root.add(composer_palette);
  root.add(composer);
  root.add(view_tabs_text);
  root.add(agent_strip_text);
  detail_panel.add(detail_text);
  root.add(detail_panel);
  renderer.root.add(root);
  (transcript_scroll.verticalScrollBar.visible = false);
  (transcript_scroll.horizontalScrollBar.visible = false);
  (work_scroll.verticalScrollBar.visible = false);
  (work_scroll.horizontalScrollBar.visible = false);
  (runtime.workScroll = work_scroll);
  (runtime.renderConversation = () => (transcript_text_view.content = render_conversation_bang(runtime)));
  (runtime.render = () => render_ui_bang(runtime, ui));
  install_input_bang(runtime, ui);
  install_mouse_bang(runtime, ui);
  install_composer_keymap_bang(runtime);
  install_process_cleanup_bang(runtime);
  install_global_exit_bang(runtime);
  install_keys_bang(runtime, ui);
  runtime.render();
  composer_input.focus();
  report_promise_bang(runtime, discover_session_branch_bang(runtime));
  report_promise_bang(runtime, refresh_bang(runtime));
  report_promise_bang(runtime, launch_agent_bang(runtime, SUPERVISOR_BOOT_PROMPT, "supervisor"));
  return runtime;
}

export function run_northbridge_app_bang(options) {
  return open_app_bang(text_or(options.viewId, "list"), text(options.sourceIdentity));
}
