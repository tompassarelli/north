import { BoxRenderable, ScrollBoxRenderable, StyledText, bg, brightBlack, brightCyan, brightGreen, brightRed, brightWhite, brightYellow, createCliRenderer, dim, InputRenderable, InputRenderableEvents, red, stripAnsiSequences, TextRenderable, white } from '@opentui/core';
import { registerEmacsBindings, registerEscapeClearsPendingSequence } from '@opentui/keymap/addons';
import { createDefaultOpenTuiKeymap } from '@opentui/keymap/opentui';
import { "->Agent" as Agent, "->WorkItem" as WorkItem, "agent-effort" as agent_effort, "agent-goal" as agent_goal, "agent-id" as agent_id, "agent-model" as agent_model, "agent-model-display" as agent_model_display, "agent-name" as agent_name, "agent-orchestration-provenance" as agent_orchestration_provenance, "agent-provider" as agent_provider, "agent-provider-label" as agent_provider_label, "agent-provider-target" as agent_provider_target, "agent-state" as agent_state, "agent-status" as agent_status, "agent-task" as agent_task, "bridgesnapshot-active-view-id" as bridgesnapshot_active_view_id, "bridgesnapshot-agents" as bridgesnapshot_agents, "bridgesnapshot-board" as bridgesnapshot_board, "bridgesnapshot-list" as bridgesnapshot_list, "bridgesnapshot-notice" as bridgesnapshot_notice, "bridgesnapshot-selected-agent" as bridgesnapshot_selected_agent, "bridgesnapshot-selected-thread" as bridgesnapshot_selected_thread, "focus-view" as focus_view, "make-model" as make_model, "remove-agent" as remove_agent, "replace-projection" as replace_projection, "select-agent" as select_agent, "select-thread" as select_thread, "set-filter" as set_filter, "snapshot" as snapshot, "upsert-agent" as upsert_agent, "workitem-body" as workitem_body, "workitem-condition" as workitem_condition, "workitem-dependencies" as workitem_dependencies, "workitem-driver" as workitem_driver, "workitem-id" as workitem_id, "workitem-title" as workitem_title } from './model.js';
import { keyword as $$bc$keyword, property_key as $$bc$property_key, record_value as $$bc$record_value, str as $$bc$str } from '../../beagle/core.js';
import { catch_dispatch as $$bd$catch_dispatch } from '../../beagle/exception-dispatch.js';

function ResponseValue(text) {
  return $$bc$record_value("north.bridge.app/ResponseValue", {_tag: "ResponseValue", text});
}

function responsevalue_text(r) { return r.text; }

function StreamRead(done, value) {
  return $$bc$record_value("north.bridge.app/StreamRead", {_tag: "StreamRead", done, value});
}

function streamread_done(r) { return r.done; }

function streamread_value(r) { return r.value; }

function StreamReader(read) {
  return $$bc$record_value("north.bridge.app/StreamReader", {_tag: "StreamReader", read});
}

function streamreader_read(r) { return r.read; }

function ReadableStream(getReader) {
  return $$bc$record_value("north.bridge.app/ReadableStream", {_tag: "ReadableStream", getReader});
}

function readablestream_getReader(r) { return r.getReader; }

function CommandChild(stdout, stderr, exited) {
  return $$bc$record_value("north.bridge.app/CommandChild", {_tag: "CommandChild", stdout, stderr, exited});
}

function commandchild_stdout(r) { return r.stdout; }

function commandchild_stderr(r) { return r.stderr; }

function commandchild_exited(r) { return r.exited; }

function PromiseConstructor(all) {
  return $$bc$record_value("north.bridge.app/PromiseConstructor", {_tag: "PromiseConstructor", all});
}

function promiseconstructor_all(r) { return r.all; }

const IntlSegmenter = Intl.Segmenter;

function SlashCommand(name, description, arguments$, completion, emoji) {
  return $$bc$record_value("north.bridge.app/SlashCommand", {_tag: "SlashCommand", name, description, arguments: arguments$, completion, emoji});
}

function slashcommand_name(r) { return r.name; }

function slashcommand_description(r) { return r.description; }

function slashcommand_arguments(r) { return r.arguments; }

function slashcommand_completion(r) { return r.completion; }

function slashcommand_emoji(r) { return r.emoji; }

function ParsedCommand(name, rest) {
  return $$bc$record_value("north.bridge.app/ParsedCommand", {_tag: "ParsedCommand", name, rest});
}

function parsedcommand_name(r) { return r.name; }

function parsedcommand_rest(r) { return r.rest; }

function ConversationItem(id, kind, title, body, status, data, execution_id, at, cursor, sequence) {
  return $$bc$record_value("north.bridge.app/ConversationItem", {_tag: "ConversationItem", id, kind, title, body, status, data, execution_id, at, cursor, sequence});
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
  return $$bc$record_value("north.bridge.app/WorkView", {_tag: "WorkView", id, title, items});
}

function workview_id(r) { return r.id; }

function workview_title(r) { return r.title; }

function workview_items(r) { return r.items; }

function ListSection(id, title) {
  return $$bc$record_value("north.bridge.app/ListSection", {_tag: "ListSection", id, title});
}

function listsection_id(r) { return r.id; }

function listsection_title(r) { return r.title; }

function ListRow(kind, condition, index, count) {
  return $$bc$record_value("north.bridge.app/ListRow", {_tag: "ListRow", kind, condition, index, count});
}

function listrow_kind(r) { return r.kind; }

function listrow_condition(r) { return r.condition; }

function listrow_index(r) { return r.index; }

function listrow_count(r) { return r.count; }

function WorkSelection(view, index) {
  return $$bc$record_value("north.bridge.app/WorkSelection", {_tag: "WorkSelection", view, index});
}

function workselection_view(r) { return r.view; }

function workselection_index(r) { return r.index; }

function BoardLane(id, title) {
  return $$bc$record_value("north.bridge.app/BoardLane", {_tag: "BoardLane", id, title});
}

function boardlane_id(r) { return r.id; }

function boardlane_title(r) { return r.title; }

function SoundPack(ready, done, interrupted, failed) {
  return $$bc$record_value("north.bridge.app/SoundPack", {_tag: "SoundPack", ready, done, interrupted, failed});
}

function soundpack_ready(r) { return r.ready; }

function soundpack_done(r) { return r.done; }

function soundpack_interrupted(r) { return r.interrupted; }

function soundpack_failed(r) { return r.failed; }

function SoundPlayer(kind, path) {
  return $$bc$record_value("north.bridge.app/SoundPlayer", {_tag: "SoundPlayer", kind, path});
}

function soundplayer_kind(r) { return r.kind; }

function soundplayer_path(r) { return r.path; }

function ParsedRecord(sequence, kind, data) {
  return $$bc$record_value("north.bridge.app/ParsedRecord", {_tag: "ParsedRecord", sequence, kind, data});
}

function parsedrecord_sequence(r) { return r.sequence; }

function parsedrecord_kind(r) { return r.kind; }

function parsedrecord_data(r) { return r.data; }

function CommandParts(executable, arguments$) {
  return $$bc$record_value("north.bridge.app/CommandParts", {_tag: "CommandParts", executable, arguments: arguments$});
}

function commandparts_executable(r) { return r.executable; }

function commandparts_arguments(r) { return r.arguments; }

function DiffRow(kind, old, new$, text) {
  return $$bc$record_value("north.bridge.app/DiffRow", {_tag: "DiffRow", kind, old, new: new$, text});
}

function diffrow_kind(r) { return r.kind; }

function diffrow_old(r) { return r.old; }

function diffrow_new(r) { return r.new; }

function diffrow_text(r) { return r.text; }

function DiffState(old_line, new_line, additions, deletions, rows) {
  return $$bc$record_value("north.bridge.app/DiffState", {_tag: "DiffState", old_line, new_line, additions, deletions, rows});
}

function diffstate_old_line(r) { return r.old_line; }

function diffstate_new_line(r) { return r.new_line; }

function diffstate_additions(r) { return r.additions; }

function diffstate_deletions(r) { return r.deletions; }

function diffstate_rows(r) { return r.rows; }

function FileChangeDetails(path, kind, additions, deletions, rows) {
  return $$bc$record_value("north.bridge.app/FileChangeDetails", {_tag: "FileChangeDetails", path, kind, additions, deletions, rows});
}

function filechangedetails_path(r) { return r.path; }

function filechangedetails_kind(r) { return r.kind; }

function filechangedetails_additions(r) { return r.additions; }

function filechangedetails_deletions(r) { return r.deletions; }

function filechangedetails_rows(r) { return r.rows; }

function FileChangeSummary(additions, deletions, files) {
  return $$bc$record_value("north.bridge.app/FileChangeSummary", {_tag: "FileChangeSummary", additions, deletions, files});
}

function filechangesummary_additions(r) { return r.additions; }

function filechangesummary_deletions(r) { return r.deletions; }

function filechangesummary_files(r) { return r.files; }

function StripBucket(id, glyph) {
  return $$bc$record_value("north.bridge.app/StripBucket", {_tag: "StripBucket", id, glyph});
}

function stripbucket_id(r) { return r.id; }

function stripbucket_glyph(r) { return r.glyph; }

function AgentSegment(id, label, count) {
  return $$bc$record_value("north.bridge.app/AgentSegment", {_tag: "AgentSegment", id, label, count});
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
  return (((_truthy) => _truthy !== false && _truthy != null)(value_text.startsWith("@")) ? value_text.slice(1) : value_text);
}

function error_message(error) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((typeof error === "object") && error.message) : _logical))(error)) ? text(error.message) : String(error));
}

function safe_json(value) {
  return (() => { try {
    return JSON.stringify(value, null, 2);
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __ = _catch_0;
        return String(value);
      }
    }
  } })();
}

function stale_daemon_summary(live) {
  return ((live > 0) ? $$bc$str("control daemon is stale (", live, " live session", ((live === 1) ? "" : "s"), ") — /restart to replace it now, or it retires when they drain") : "control daemon is stale — /restart to replace it now");
}

function failure_summary(data) {
  const message = text_or(data.message, text_or(data.detail, text_or(data.classification, text(data.code))));
  const causes = data.causes;
  const cause = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (causes.length > 0) : _logical))(causes)) ? text(causes[0]) : "");
  return ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : cause.includes("ECONNREFUSED 127.0.0.1:7977")))(message.includes("north_coordinator_preflight")))) ? "coordinator offline — supervision unavailable; /config and /help still work" : ((message === "bridge_daemon_source_stale")) ? stale_daemon_summary(Number(((_logical) => (_logical !== false && _logical != null ? _logical : 0))(data.live))) : ((message === "")) ? safe_json(data) : ((cause === "")) ? message : $$bc$str(message, " — ", cause));
}

function clean_text(value) {
  return stripAnsiSequences(text(value));
}

function agent_field_text(value) {
  return Array.from(clean_text(value)).map((character) => { const code = character.charCodeAt(0);
return ((((_truthy) => _truthy !== false && _truthy != null)(((code === 9) || ((code === 10) || (code === 13))))) ? " " : (((_truthy) => _truthy !== false && _truthy != null)(((code < 32) || ((code >= 127) && (code <= 159))))) ? "" : character); }).join("");
}

function agent_cell_text_bang(value, width) {
  const source = agent_field_text(value);
  const limit = Math.max(1, width);
  if ((Bun.stringWidth(source) <= limit)) {
    return source;
  } else {
    const ellipsis_width = Bun.stringWidth("…");
    const room = Math.max(0, (limit - ellipsis_width));
    const kept = [];
    const state = {[$$bc$property_key($$bc$keyword("width"))]: 0, [$$bc$property_key($$bc$keyword("done"))]: false};
    Array.from(new IntlSegmenter("en", {[$$bc$property_key($$bc$keyword("granularity"))]: "grapheme"}).segment(source)).forEach((part) => { const segment = text(part.segment);
const segment_width = Bun.stringWidth(segment);
const state_width = state.width;
if ((!((_truthy) => _truthy !== false && _truthy != null)(state.done))) {
  if (((state_width + segment_width) <= room)) {
    kept.push(segment);
    return (state.width = (state_width + segment_width));
  } else {
    return (state.done = true);
  }
} });
    return $$bc$str(kept.join(""), "…");
  }
}

function clipped(value, limit) {
  const clean = clean_text(value);
  const clean_length = clean.length;
  return ((clean_length > limit) ? $$bc$str("…", clean.slice((clean_length - limit))) : clean);
}

function next_item_id_bang(runtime, prefix) {
  const sequence = runtime.itemSequence;
  (runtime.itemSequence = (sequence + 1));
  return $$bc$str(prefix, ":", runtime.itemSequence);
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

function project_conversation(items, execution_id, aggregate) {
  const visible = (aggregate ? items.slice() : items.filter((item) => (conversationitem_execution_id(item) === execution_id)));
  return visible.sort((aggregate ? compare_conversation_items : compare_execution_items));
}

function runtime_selected_agent_id(runtime) {
  const agents = bridgesnapshot_agents(snapshot(runtime.model));
  const total = agents.length;
  const selected = Number(((_logical) => (_logical !== false && _logical != null ? _logical : 0))(runtime.agentIndex));
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
  return (((_truthy) => _truthy !== false && _truthy != null)(executions) ? (aggregate_transcript_p(runtime) ? (executions.size > 0) : executions.has(runtime_selected_agent_id(runtime))) : (((_truthy) => _truthy !== false && _truthy != null)(runtime.working) ? true : false));
}

function mark_execution_working_bang(runtime, execution_id, working) {
  const executions = runtime.workingExecutions;
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(execution_id === "")) : _logical))(executions))) {
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
    runtime.conversation.splice(index, 1, item);
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
      const timer = setInterval(() => { if ((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed))) {
  const spinner_index = runtime.spinnerIndex;
  (runtime.spinnerIndex = (spinner_index + 1));
  return runtime.renderConversation();
} }, 180);
      (runtime.spinnerTimer = timer);
      if (((_truthy) => _truthy !== false && _truthy != null)(timer.unref)) {
        timer.unref();
      }
    }
  } else {
    if (((_truthy) => _truthy !== false && _truthy != null)(runtime.spinnerTimer)) {
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
  if (((_truthy) => _truthy !== false && _truthy != null)(executions)) {
    const any_working_p = (executions.size > 0);
    return set_working_bang(runtime, any_working_p, (any_working_p ? text_or(label, runtime.workingLabel) : ""));
  } else {
    return set_working_bang(runtime, working, label);
  }
}

function command(input) {
  const trimmed = input.trim();
  const normalized = (((_truthy) => _truthy !== false && _truthy != null)(trimmed.startsWith("/")) ? trimmed.slice(1) : trimmed);
  const split_at = normalized.indexOf(" ");
  return ((split_at < 0) ? ParsedCommand(normalized.toLowerCase(), "") : ParsedCommand(normalized.slice(0, split_at).toLowerCase(), normalized.slice((split_at + 1)).trim()));
}

function quit_command_p(name) {
  return ((name === "q") || (name === "exit"));
}

function escape_command_p(name) {
  return ((name === "close") || (name === "esc"));
}

const BOOT_VIEW = "agents";

function boot_view() {
  return BOOT_VIEW;
}

function threads_view_p(view) {
  return (text(view) === "threads");
}

function escape_rung(palette_open_p, filtering_p, detail_open_p, strip_focused_p, threads_p, working_p) {
  return ((palette_open_p) ? "close-palette" : (filtering_p) ? "clear-filter" : (detail_open_p) ? "close-detail" : (strip_focused_p) ? "focus-composer" : (threads_p) ? "show-agents" : (working_p) ? "cancel-turn" : "");
}

function active_focus(palette_open_p, panel_open_p, panel_focused_p, filtering_p, strip_focused_p) {
  return ((palette_open_p) ? "palette" : (((_truthy) => _truthy !== false && _truthy != null)((panel_open_p && (panel_focused_p && filtering_p)))) ? "filter" : (((_truthy) => _truthy !== false && _truthy != null)((panel_open_p && panel_focused_p))) ? "panel" : (strip_focused_p) ? "strip" : "composer");
}

function tab_action(focus, dir_row_p, expanded_p) {
  return (((focus === "palette")) ? "complete" : (((_truthy) => _truthy !== false && _truthy != null)(((focus === "panel") || (focus === "filter")))) ? (dir_row_p ? (expanded_p ? "collapse" : "expand") : "climb") : "swap-view");
}

function tab_swap_view(view) {
  return (threads_view_p(view) ? "agents" : "threads");
}

function emoji_options(query) {
  const needle = query.trim().toLowerCase();
  return EMOJI_COMMANDS.filter((candidate) => ((needle === "") || $$bc$str(slashcommand_name(candidate), " ", slashcommand_description(candidate)).toLowerCase().includes(needle)));
}

function glyph_options(query) {
  const needle = query.trim().toLowerCase();
  return GLYPH_COMMANDS.filter((candidate) => ((needle === "") || $$bc$str(slashcommand_name(candidate), " ", slashcommand_description(candidate)).toLowerCase().includes(needle)));
}

function palette_options(view, input) {
  const query = input.trim().toLowerCase();
  const parsed = command(input);
  const name = parsedcommand_name(parsed);
  const commands = (threads_view_p(view) ? THREAD_COMMANDS : AGENT_COMMANDS);
  return (((!((_truthy) => _truthy !== false && _truthy != null)(query.startsWith("/")))) ? [] : ((name === "emoji")) ? emoji_options(parsedcommand_rest(parsed)) : (((_truthy) => _truthy !== false && _truthy != null)(((name === "glyph") || (name === "prompt")))) ? glyph_options(parsedcommand_rest(parsed)) : ((query.indexOf(" ") >= 0)) ? [] : commands.filter((candidate) => slashcommand_name(candidate).startsWith(query)));
}

function submit_key_p(name) {
  return ((name === "return") || ((name === "enter") || ((name === "kpenter") || (name === "linefeed"))));
}

async function run_command(argv) {
  const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: argv, [$$bc$property_key($$bc$keyword("stdout"))]: "pipe", [$$bc$property_key($$bc$keyword("stderr"))]: "pipe"});
  const results = await Promise.all([new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited]);
  const stdout = text(results[0]);
  const stderr = text(results[1]);
  const exit_code = results[2];
  return ((exit_code === 0) ? stdout : (() => { throw new Error($$bc$str(argv.join(" "), " failed (", exit_code, "): ", text_or(stderr.trim(), stdout.trim()))); })());
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
  } catch (_catch_1) {
    switch ($$bd$catch_dispatch(_catch_1, [Error])) {
      case 0: {
        const __ = _catch_1;
        return (runtime.sessionBranch = "not a Git worktree");
      }
    }
  } })();
  return runtime.render();
}

async function read_stream(stream, on_chunk) {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  return (async () => {  while (true) {
    const result = await reader.read(); if ((!((_truthy) => _truthy !== false && _truthy != null)(result.done))) { on_chunk(decoder.decode(result.value, {[$$bc$property_key($$bc$keyword("stream"))]: true}));  continue; } else { return null; }
  } })();
}

async function stream_command(argv, on_stdout, on_stderr) {
  const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: argv, [$$bc$property_key($$bc$keyword("stdout"))]: "pipe", [$$bc$property_key($$bc$keyword("stderr"))]: "pipe"});
  const results = await Promise.all([read_stream(child.stdout, on_stdout), read_stream(child.stderr, on_stderr), child.exited]);
  return results[2];
}

function normalize_agents(payload) {
  const rows = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? Array.isArray(payload.agents) : _logical))(payload)) ? payload.agents : []);
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
    return (((_truthy) => _truthy !== false && _truthy != null)(row) ? bare(row.value) : "");
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
    return (((_truthy) => _truthy !== false && _truthy != null)(row) ? text(row.value) : "");
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
  return ((condition === "terminal") || ((condition === "done") || ((condition === "completed") || (condition === "failed"))));
}

function list_section_id(condition) {
  return ((terminal_condition_p(condition)) ? "terminal" : (((_truthy) => _truthy !== false && _truthy != null)(["active", "ready", "blocked", "dormant", "draft"].includes(condition))) ? condition : "other");
}

function list_section_title(section_id) {
  const section = LIST_SECTIONS.find((candidate) => (listsection_id(candidate) === section_id));
  return (((_truthy) => _truthy !== false && _truthy != null)(section) ? listsection_title(section) : section_id);
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
  if ((!((_truthy) => _truthy !== false && _truthy != null)(collapsed.has(section_id)))) {
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
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed)) && (!(line.trim() === ""))))) {
    return append_system_bang(runtime, line);
  }
}

function grapheme_count(value) {
  return Array.from(new IntlSegmenter("en", {[$$bc$property_key($$bc$keyword("granularity"))]: "grapheme"}).segment(value)).length;
}

function current_prompt_glyph(runtime) {
  const glyph = text(runtime.promptGlyph);
  return ((glyph === "") ? DEFAULT_PROMPT_GLYPH : glyph);
}

function render_prompt_bang(runtime, prompt) {
  const glyph = current_prompt_glyph(runtime);
  const glyph_width = Bun.stringWidth(glyph);
  (prompt.width = (glyph_width + 1));
  return (prompt.content = new StyledText([brightCyan($$bc$str(glyph, " "))]));
}

function set_prompt_glyph_bang(runtime, glyph) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((glyph.trim() === "") || (!(grapheme_count(glyph) === 1))))) {
    (() => { throw new Error("glyph requires exactly one grapheme, or use /glyph reset"); })();
  }
  (runtime.promptGlyph = glyph);
  publish_line_bang(runtime, $$bc$str("prompt glyph set to ", glyph));
  return runtime.render();
}

function sound_enabled_from_env(value) {
  const normalized = value.trim().toLowerCase();
  return (!((_truthy) => _truthy !== false && _truthy != null)(((normalized === "0") || ((normalized === "false") || ((normalized === "off") || (normalized === "no"))))));
}

function sound_pack_from_env(value) {
  return ((value.trim().toLowerCase() === "peasant") ? "peasant" : "peon");
}

function sound_directory_from_env(value) {
  const directory = value.trim();
  const north_home = text(process.env.NORTH_HOME).trim();
  return (((!(directory === ""))) ? directory : ((!(north_home === ""))) ? $$bc$str(north_home, "/../warcraft-sounds") : "warcraft-sounds");
}

function discover_sound_player() {
  const mpv = Bun.which("mpv");
  const ffplay = Bun.which("ffplay");
  const pw_play = Bun.which("pw-play");
  return ((((_truthy) => _truthy !== false && _truthy != null)(mpv)) ? SoundPlayer("mpv", text(mpv)) : (((_truthy) => _truthy !== false && _truthy != null)(ffplay)) ? SoundPlayer("ffplay", text(ffplay)) : (((_truthy) => _truthy !== false && _truthy != null)(pw_play)) ? SoundPlayer("pw-play", text(pw_play)) : null);
}

function sound_warning_bang(runtime, message) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed)) && (!((_truthy) => _truthy !== false && _truthy != null)(runtime.soundWarningShown))))) {
    (runtime.soundWarningShown = true);
    return publish_line_bang(runtime, $$bc$str("sound warning: ", message));
  }
}

function sound_path(directory, filename) {
  return $$bc$str(directory, (((_truthy) => _truthy !== false && _truthy != null)(directory.endsWith("/")) ? "" : "/"), filename);
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
    const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: sound_argv(player, path), [$$bc$property_key($$bc$keyword("stdin"))]: "ignore", [$$bc$property_key($$bc$keyword("stdout"))]: "ignore", [$$bc$property_key($$bc$keyword("stderr"))]: "ignore"});
    runtime.soundChildren.add(child);
    child.unref();
    return child.exited.then((exit_code) => { runtime.soundChildren.delete(child);
if ((!(exit_code === 0))) {
  return sound_warning_bang(runtime, $$bc$str("player exited ", exit_code, " for ", path));
} }).catch((error) => { runtime.soundChildren.delete(child);
return sound_warning_bang(runtime, error_message(error)); });
  }
  } catch (_catch_2) {
    switch ($$bd$catch_dispatch(_catch_2, [Error])) {
      case 0: {
        const error = _catch_2;
        return sound_warning_bang(runtime, error_message(error));
      }
    }
  } })();
}

function play_sound_path_bang(runtime, path) {
  return (() => { try {
    return Bun.file(path).exists().then((exists) => { if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed)) : _logical))(runtime.soundEnabled) : _logical))(exists))) {
  return spawn_sound_bang(runtime, path);
} else {
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(exists)) && runtime.soundEnabled))) {
    return sound_warning_bang(runtime, $$bc$str("missing local asset ", path));
  }
} }).catch((error) => sound_warning_bang(runtime, error_message(error)));
  } catch (_catch_3) {
    switch ($$bd$catch_dispatch(_catch_3, [Error])) {
      case 0: {
        const error = _catch_3;
        return sound_warning_bang(runtime, error_message(error));
      }
    }
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
    const index = (((_truthy) => _truthy !== false && _truthy != null)(((count > 1) && (base_path === text(runtime.lastSoundPath)))) ? ((base_index + 1) % count) : base_index);
    const path = sound_path(text(runtime.soundDirectory), text(files[index]));
    const sound_sequence = runtime.soundSequence;
    (runtime.soundSequence = (sound_sequence + 1));
    (runtime.lastSoundPath = path);
    return path;
  }
}

function play_sound_event_bang(runtime, stream_state, event) {
  const now = Date.now();
  const last_sound_at = runtime.lastSoundAt;
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? ((now - last_sound_at) >= SOUND_COOLDOWN_MS) : _logical))(stream_state.soundLive) : _logical))(runtime.soundEnabled))) {
    const path = select_sound_path_bang(runtime, event);
    if ((!(path === ""))) {
      (runtime.lastSoundAt = now);
      return play_sound_path_bang(runtime, path);
    }
  }
}

function sound_status(runtime) {
  const player = runtime.soundPlayer;
  return $$bc$str("sound ", (((_truthy) => _truthy !== false && _truthy != null)(runtime.soundEnabled) ? "on" : "off"), " · pack ", text(runtime.soundPack), " · player ", (((_truthy) => _truthy !== false && _truthy != null)(player) ? soundplayer_kind(player) : "none"), " · ", text(runtime.soundDirectory));
}

function ConfigEntry(kind, name, state, detail) {
  return $$bc$record_value("north.bridge.app/ConfigEntry", {_tag: "ConfigEntry", kind, name, state, detail});
}

function configentry_kind(r) { return r.kind; }

function configentry_name(r) { return r.name; }

function configentry_state(r) { return r.state; }

function configentry_detail(r) { return r.detail; }

function config_hook_enabled_p(state) {
  return (state === "enabled");
}

function ModuleMembership(module, members) {
  return $$bc$record_value("north.bridge.app/ModuleMembership", {_tag: "ModuleMembership", module, members});
}

function modulemembership_module(r) { return r.module; }

function modulemembership_members(r) { return r.members; }

function config_owner_modules(memberships, name) {
  return memberships.filter((membership) => modulemembership_members(membership).includes(name)).map((membership) => modulemembership_module(membership));
}

function config_module_members(memberships, name) {
  const found = memberships.find((membership) => (modulemembership_module(membership) === name));
  return (((_truthy) => _truthy !== false && _truthy != null)(found) ? modulemembership_members(found) : null);
}

function config_subtree_kind_p(kind) {
  return ((kind === "ins") || ((kind === "memroot") || (kind === "mem")));
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

function config_kind_word(kind) {
  return (((kind === "ins")) ? "file" : ((kind === "memroot")) ? "memories" : ((kind === "mem")) ? "memory" : kind);
}

function config_provenance_name(kind, name) {
  return ((kind === "") ? name : $$bc$str(config_kind_word(kind), ": ", name));
}

function config_row_label(kind, name) {
  return (((kind === "ins")) ? "AGENTS.md" : ((kind === "memroot")) ? "MEMORIES" : ((kind === "mem")) ? config_mem_name(name) : name);
}

function config_cli_name(kind, name) {
  return (((kind === "ins")) ? $$bc$str(name, "/AGENTS.md") : ((kind === "memroot")) ? $$bc$str(name, "/memories") : name);
}

function config_reference_text(kind, name) {
  return $$bc$str("@", config_kind_word(kind), ":", config_cli_name(kind, name), " ");
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
  return ((((_truthy) => _truthy !== false && _truthy != null)(((kind === "ins") || (kind === "memroot")))) ? config_find_kind(manifest, "dir", slug) : ((kind === "mem")) ? (() => { const root = config_find_kind(manifest, "memroot", slug); return (((_truthy) => _truthy !== false && _truthy != null)(root) ? root : config_find_kind(manifest, "dir", slug)); })() : null);
}

function config_active_along_p(entry, manifest, memberships, trail) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const key = $$bc$str(kind, " ", name);
  if (((_truthy) => _truthy !== false && _truthy != null)(trail.includes(key))) {
    return false;
  } else {
    const walked = trail.concat([key]);
    const hook_p = (kind === "hook");
    const state = configentry_state(entry);
    const own_p = (hook_p ? config_hook_enabled_p(state) : (state === "on"));
    const owners = config_owner_modules(memberships, name);
    const gated_p = ((owners.length === 0) || owners.some((owner) => { const row = config_find_kind(manifest, "module", owner);
return (((_truthy) => _truthy !== false && _truthy != null)(row) ? config_active_along_p(row, manifest, memberships, walked) : false); }));
    const companion = (hook_p ? text(configentry_detail(entry)) : "");
    const followed_p = ((companion === "") ? true : (() => { const row = config_find_companion(manifest, companion); return (((_truthy) => _truthy !== false && _truthy != null)(row) ? config_active_along_p(row, manifest, memberships, walked) : false); })());
    const gate = config_subtree_gate(manifest, kind, name);
    const scoped_p = (((_truthy) => _truthy !== false && _truthy != null)(gate) ? config_active_along_p(gate, manifest, memberships, walked) : true);
    return (own_p && ((_logical) => (_logical !== false && _logical != null ? (followed_p && scoped_p) : _logical))(gated_p));
  }
}

function config_entry_active_p(entry, manifest, memberships) {
  return config_active_along_p(entry, manifest, memberships, []);
}

function config_unit_active_p(manifest, memberships, name) {
  const entry = config_find_entry(manifest, name);
  return (((_truthy) => _truthy !== false && _truthy != null)(entry) ? config_entry_active_p(entry, manifest, memberships) : false);
}

function config_gate_modules(entry, manifest, memberships) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const gate = config_subtree_gate(manifest, kind, name);
  const scope = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!config_entry_active_p(gate, manifest, memberships)) : _logical))(gate)) ? [config_provenance_name(configentry_kind(gate), configentry_name(gate))] : []);
  const owners = config_owner_modules(memberships, name);
  const open_p = ((owners.length === 0) || owners.some((owner) => { const row = config_find_kind(manifest, "module", owner);
return (((_truthy) => _truthy !== false && _truthy != null)(row) ? config_entry_active_p(row, manifest, memberships) : false); }));
  return scope.concat((((_truthy) => _truthy !== false && _truthy != null)(open_p) ? [] : owners.map((owner) => config_provenance_name("module", owner))));
}

function config_state_text(entry, manifest, memberships, active_p, nested_p) {
  const state = configentry_state(entry);
  const hook_p = (configentry_kind(entry) === "hook");
  const own_p = (hook_p ? config_hook_enabled_p(state) : (state === "on"));
  const gates = config_gate_modules(entry, manifest, memberships);
  const gate_note = ((gates.length === 0) ? "" : $$bc$str(" (", gates.join(", "), " off)"));
  const companion = (hook_p ? text(configentry_detail(entry)) : "");
  const followed = config_find_companion(manifest, companion);
  const provenance = (((_truthy) => _truthy !== false && _truthy != null)((nested_p || (companion === ""))) ? "" : $$bc$str(" · ", config_provenance_name((((_truthy) => _truthy !== false && _truthy != null)(followed) ? configentry_kind(followed) : ""), companion)));
  const claimant_off_p = ((_logical) => (_logical !== false && _logical != null ? (!config_entry_active_p(followed, manifest, memberships)) : _logical))(followed);
  const reason = (((!(gate_note === ""))) ? gate_note : (((_truthy) => _truthy !== false && _truthy != null)(claimant_off_p)) ? $$bc$str(" (", config_provenance_name(configentry_kind(followed), companion), " off)") : "");
  return ((((_truthy) => _truthy !== false && _truthy != null)((hook_p && (!own_p)))) ? "disabled" : ((!own_p)) ? "off" : (active_p) ? $$bc$str("on", provenance) : $$bc$str("off", reason));
}

function config_toggle_verb(state) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((state === "on") || (state === "enabled"))) ? "off" : "on");
}

const GLOBAL_DIR_NAME = "global";

function config_global_row_p(kind, name) {
  return ((kind === "dir") && (name === GLOBAL_DIR_NAME));
}

function config_row_scope(kind, name) {
  return (((kind === "dir")) ? name : (config_subtree_kind_p(kind)) ? config_row_slug(kind, name) : GLOBAL_DIR_NAME);
}

function config_view_includes_p(view, kind, name) {
  return (((view === "all")) ? true : ((kind === "dir")) ? ((view === "globals") ? (name === GLOBAL_DIR_NAME) : true) : ((view === "globals")) ? ((config_row_scope(kind, name) === GLOBAL_DIR_NAME) && (!(kind === "plugin"))) : ((view === "agentsmd")) ? config_subtree_kind_p(kind) : (kind === view));
}

function config_view_prunes_p(view) {
  return (!((_truthy) => _truthy !== false && _truthy != null)(((view === "all") || ((view === "globals") || (view === "agentsmd")))));
}

function config_view_folds_p(view) {
  return (view === "all");
}

function config_hook_companion(entry) {
  return ((configentry_kind(entry) === "hook") ? text(configentry_detail(entry)) : "");
}

function config_skill_hooks(rows, name) {
  return rows.filter((entry) => ((configentry_kind(entry) === "hook") && (config_hook_companion(entry) === name)));
}

function config_row_role(entry, rows) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  return (((kind === "module")) ? "moduleset" : ((kind === "skill")) ? ((config_skill_hooks(rows, name).length > 0) ? "module" : "skill") : ((kind === "hook")) ? (() => { const companion = config_hook_companion(entry); return (((_truthy) => _truthy !== false && _truthy != null)(((!(companion === "")) && rows.some((row) => ((configentry_kind(row) === "skill") && (configentry_name(row) === companion))))) ? "boundhook" : "hook"); })() : kind);
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
  return tree.concat(kids.filter((child) => (!((_truthy) => _truthy !== false && _truthy != null)(tree.includes(child)))));
}

function config_view_rows(entries, view) {
  const kept = entries.filter((entry) => config_view_includes_p(view, configentry_kind(entry), configentry_name(entry)));
  const held = kept.filter((entry) => (((_truthy) => _truthy !== false && _truthy != null)(((!config_view_prunes_p(view)) || (!(configentry_kind(entry) === "dir")))) ? true : kept.some((child) => ((!(configentry_kind(child) === "dir")) && (config_row_scope(configentry_kind(child), configentry_name(child)) === configentry_name(entry))))));
  return config_tree_rows(held);
}

function config_node_expanded_p(expanded, slug) {
  return (((_truthy) => _truthy !== false && _truthy != null)(expanded) ? expanded.includes(slug) : false);
}

function config_fold_rows(entries, expanded) {
  const nodes = entries.filter((entry) => (configentry_kind(entry) === "dir")).map((entry) => configentry_name(entry));
  return entries.filter((entry) => { const kind = configentry_kind(entry);
const scope = config_row_scope(kind, configentry_name(entry));
return ((kind === "dir") || ((!((_truthy) => _truthy !== false && _truthy != null)(nodes.includes(scope))) || config_node_expanded_p(expanded, scope))); });
}

function config_row_search_text(entry) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  return $$bc$str(config_row_label(kind, name), " ", name, " ", text(configentry_detail(entry))).toLowerCase();
}

function config_row_matches_p(entry, query) {
  const needle = query.trim().toLowerCase();
  return ((needle === "") || config_row_search_text(entry).includes(needle));
}

function config_matched_slugs(entries, query, kind) {
  return entries.filter((entry) => ((configentry_kind(entry) === kind) && config_row_matches_p(entry, query))).map((entry) => config_row_scope(configentry_kind(entry), configentry_name(entry)));
}

function config_query_rows(entries, query) {
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
return (config_row_matches_p(entry, query) || ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((kind === "hook") && open_skills.includes(config_hook_companion(entry)))))(((kind === "skill") && held_skills.includes(configentry_name(entry))))))(((kind === "memroot") && held_mems.includes(scope)))))(((kind === "dir") && held.includes(scope)))))(((kind === "mem") && open_mems.includes(scope)))))(open_nodes.includes(scope))); });
  }
}

function config_row_context_only_p(entry, query) {
  return ((!(query.trim() === "")) && (!config_row_matches_p(entry, query)));
}

function config_manifest_path() {
  return $$bc$str(text(process.env.HOME), "/.config/agents/manifest.conf");
}

function config_modules_dir(runtime) {
  return text_or(text(runtime.configModulesDir), $$bc$str(text(process.env.HOME), "/code/nixos-config/main/dotfiles/agents/modules.d"));
}

function config_membership_of_json(module, content) {
  return (() => { try {
    const parsed = JSON.parse(content);
  const members = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? Array.isArray(parsed.members) : _logical))(parsed)) ? parsed.members : []);
  return ModuleMembership(module, members.map((member) => text(member)));
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const __ = _catch_4;
        return ModuleMembership(module, []);
      }
    }
  } })();
}

async function list_module_files_bang(directory) {
  return (async () => { try {
    const listing = await run_command(["ls", directory]);
  return listing.trim().split("\n").filter((name) => name.endsWith(".json"));
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const __ = _catch_5;
        return [];
      }
    }
  } })();
}

async function read_module_file_bang(path) {
  return (async () => { try {
    return await run_command(["cat", path]);
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const __ = _catch_6;
        return "";
      }
    }
  } })();
}

async function load_config_memberships_bang(directory) {
  const files = await list_module_files_bang(directory);
  const contents = await Promise.all(files.map((file) => read_module_file_bang($$bc$str(directory, "/", file))));
  return files.map((file, index) => { const file_length = file.length;
return config_membership_of_json(file.slice(0, (file_length - 5)), text(contents[index])); });
}

async function ensure_config_manifest_bang() {
  return (async () => { try {
    return await run_command(["test", "-f", config_manifest_path()]);
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const __ = _catch_7;
        return await run_command([AGENTS_BIN, "status"]);
      }
    }
  } })();
}

function panel_filtering_p(runtime) {
  return (((_truthy) => _truthy !== false && _truthy != null)(runtime.panelFiltering) ? true : false);
}

function panel_query(runtime) {
  return (panel_filtering_p(runtime) ? text(runtime.panelQuery) : "");
}

const PANEL_DEFAULT_EXPANDED = [GLOBAL_DIR_NAME];

function panel_expanded(runtime) {
  const stored = runtime.expandedDirs;
  return (((_truthy) => _truthy !== false && _truthy != null)(stored) ? stored : PANEL_DEFAULT_EXPANDED);
}

function config_panel_rows(runtime) {
  const stored = runtime.configEntries;
  const entries = (((_truthy) => _truthy !== false && _truthy != null)(stored) ? stored : []);
  const query = panel_query(runtime);
  const view = text_or(text(runtime.configFilter), "all");
  return ((!(query.trim() === "")) ? config_query_rows(entries, query) : (config_view_folds_p(view) ? config_fold_rows(entries, panel_expanded(runtime)) : entries));
}

function config_row_node(entry) {
  return config_row_scope(configentry_kind(entry), configentry_name(entry));
}

function clamp_panel_cursor_bang(runtime) {
  const total = config_panel_rows(runtime).length;
  const raw = runtime.configIndex;
  const current = (((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0);
  return (runtime.configIndex = ((total > 0) ? Math.max(0, Math.min(current, (total - 1))) : 0));
}

function set_panel_query_bang(runtime, query) {
  (runtime.panelFiltering = true);
  (runtime.panelQuery = query);
  return clamp_panel_cursor_bang(runtime);
}

function clear_panel_filter_bang(runtime) {
  (runtime.panelFiltering = false);
  (runtime.panelQuery = "");
  return clamp_panel_cursor_bang(runtime);
}

function set_node_expanded_bang(runtime, slug, open_p) {
  const current = panel_expanded(runtime);
  const without = current.filter((held) => (!(held === slug)));
  (runtime.expandedDirs = (open_p ? without.concat([slug]) : without));
  return clamp_panel_cursor_bang(runtime);
}

function fold_key_action(dir_row_p, expanded_p, open_key_p) {
  return ((((_truthy) => _truthy !== false && _truthy != null)((open_key_p && dir_row_p))) ? (expanded_p ? "" : "expand") : (open_key_p) ? "" : (dir_row_p) ? (expanded_p ? "collapse" : "") : "climb");
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
  if (((_truthy) => _truthy !== false && _truthy != null)(already)) {
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
    const entry_count = entries.length;
    const raw = runtime.configIndex;
    const index = Math.max(0, Math.min((((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0), (entry_count - 1)));
    const entry = entries[index];
    const raw_path = await run_command([AGENTS_BIN, "path", config_cli_name(configentry_kind(entry), configentry_name(entry))]);
    const path = raw_path.trim();
    const editor = text_or(text(process.env.EDITOR), "vi");
    const ghostty = Bun.which("ghostty");
    const kitty = Bun.which("kitty");
    const foot = Bun.which("foot");
    const xterm = Bun.which("xterm");
    const argv = ((((_truthy) => _truthy !== false && _truthy != null)(ghostty)) ? [ghostty, "-e", editor, path] : (((_truthy) => _truthy !== false && _truthy != null)(kitty)) ? [kitty, "--detach", editor, path] : (((_truthy) => _truthy !== false && _truthy != null)(foot)) ? [foot, editor, path] : (((_truthy) => _truthy !== false && _truthy != null)(xterm)) ? [xterm, "-e", editor, path] : null);
    if ((argv == null)) {
      (() => { throw new Error("no supported terminal found for edit"); })();
    }
    const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: argv, [$$bc$property_key($$bc$keyword("stdin"))]: "ignore", [$$bc$property_key($$bc$keyword("stdout"))]: "ignore", [$$bc$property_key($$bc$keyword("stderr"))]: "ignore"});
    child.unref();
    return publish_line_bang(runtime, $$bc$str("editing ", path));
  }
}

async function toggle_config_entry_bang(runtime) {
  const entries = config_panel_rows(runtime);
  if ((entries.length > 0)) {
    const entry_count = entries.length;
    const raw = runtime.configIndex;
    const index = Math.max(0, Math.min((((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0), (entry_count - 1)));
    const entry = entries[index];
    const verb = config_toggle_verb(configentry_state(entry));
    await run_command([AGENTS_BIN, verb, config_cli_name(configentry_kind(entry), configentry_name(entry))]);
    await load_config_entries_bang(runtime);
    return runtime.render();
  }
}

function handle_sound_command_bang(runtime, rest) {
  const request = rest.trim().toLowerCase();
  return ((((_truthy) => _truthy !== false && _truthy != null)(((request === "") || (request === "status")))) ? publish_line_bang(runtime, sound_status(runtime)) : ((request === "on")) ? (() => { (runtime.soundEnabled = true);
publish_line_bang(runtime, sound_status(runtime));
if ((runtime.soundPlayer == null)) {
  return sound_warning_bang(runtime, "install mpv, ffplay, or pw-play to play local assets");
} })() : ((request === "off")) ? (() => { (runtime.soundEnabled = false);
return publish_line_bang(runtime, sound_status(runtime)); })() : (((_truthy) => _truthy !== false && _truthy != null)(request.startsWith("pack "))) ? (() => { const pack = request.slice(5).trim(); if ((!((_truthy) => _truthy !== false && _truthy != null)(((pack === "peon") || (pack === "peasant"))))) {
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
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const error = _catch_8;
        return append_error_bang(runtime, error_message(error));
      }
    }
  } })();
}

function handle_local_command_bang(runtime, ui, input) {
  const trimmed = input.trim();
  if ((!((_truthy) => _truthy !== false && _truthy != null)(trimmed.startsWith("/")))) {
    return false;
  } else {
    const parsed = command(trimmed);
    const name = parsedcommand_name(parsed);
    const rest = parsedcommand_rest(parsed);
    return ((((_truthy) => _truthy !== false && _truthy != null)(((name === "glyph") || (name === "prompt")))) ? (() => { if ((rest.toLowerCase() === "reset")) {
  set_prompt_glyph_bang(runtime, DEFAULT_PROMPT_GLYPH);
} else {
  set_prompt_glyph_bang(runtime, rest);
}
return true; })() : ((name === "emoji")) ? (() => { const options = emoji_options(rest); if ((options.length === 0)) {
  (() => { throw new Error($$bc$str("no emoji matches ", rest)); })();
}
const input_renderable = ui.composerInput;
(input_renderable.value = slashcommand_completion(options[0]));
input_renderable.focus();
runtime.render();
return true; })() : ((name === "sound")) ? (() => { handle_sound_command_bang(runtime, rest);
return true; })() : ((name === "mute")) ? (() => { (runtime.soundEnabled = false);
publish_line_bang(runtime, sound_status(runtime));
return true; })() : ((name === "transcript")) ? (() => { const requested = rest.trim().toLowerCase(); if ((!((_truthy) => _truthy !== false && _truthy != null)(((requested === "selected") || (requested === "all"))))) {
  (() => { throw new Error("transcript requires selected or all"); })();
}
(runtime.transcriptView = requested);
runtime.render();
return true; })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "provider") || ((name === "model") || (name === "effort"))))) ? (() => { set_launch_route_bang(runtime, name, rest);
return true; })() : ((name === "config")) ? (() => { open_config_panel_bang(runtime, ui, "all");
return true; })() : ((name === "hooks")) ? (() => { open_config_panel_bang(runtime, ui, "hook");
return true; })() : ((name === "skills")) ? (() => { open_config_panel_bang(runtime, ui, "skill");
return true; })() : ((name === "mcp")) ? (() => { const parts = rest.trim().split(/\\s+/).filter((part) => (!(part === "")));
run_command([north_bin(), "config", "mcp"].concat(parts)).then((output) => publish_line_bang(runtime, output.trim())).catch((error) => publish_line_bang(runtime, $$bc$str("error: ", error_message(error))));
return true; })() : ((name === "plugins")) ? (() => { open_config_panel_bang(runtime, ui, "plugin");
return true; })() : ((name === "modules")) ? (() => { open_config_panel_bang(runtime, ui, "module");
return true; })() : ((name === "globals")) ? (() => { open_config_panel_bang(runtime, ui, "globals");
return true; })() : ((name === "agentsmd")) ? (() => { open_config_panel_bang(runtime, ui, "agentsmd");
return true; })() : ((name === "restart")) ? (() => { restart_daemon_bang(runtime);
return true; })() : ((name === "agents")) ? (() => { show_view_bang(runtime, ui, "agents");
return true; })() : ((name === "threads")) ? (() => { if ((rest.trim().toLowerCase() === "popout")) {
  popout_bang(runtime, text(runtime.activeView));
} else {
  show_view_bang(runtime, ui, "threads");
}
return true; })() : (thread_view_command_p(name)) ? (() => { show_thread_view_bang(runtime, ui, name);
return true; })() : ((name === "help")) ? (() => { toggle_help_bang(runtime, ui);
return true; })() : (quit_command_p(name)) ? (() => { destroy_bang(runtime);
return true; })() : (escape_command_p(name)) ? (() => { escape_step_bang(runtime, ui);
return true; })() : false);
  }
}

function render_after_suspend_bang(runtime) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed)) && runtime.render))) {
    return runtime.render();
  }
}

function cleanup_suspend_bang(runtime, process_api) {
  const handler = runtime.suspendResume;
  const suspended_p = (((_truthy) => _truthy !== false && _truthy != null)(runtime.rendererSuspended) ? true : false);
  const active_p = ((_logical) => (_logical !== false && _logical != null ? _logical : suspended_p))(handler);
  if (((_truthy) => _truthy !== false && _truthy != null)(handler)) {
    process_api.removeListener("SIGCONT", handler);
  }
  (runtime.suspendResume = null);
  (runtime.rendererSuspended = false);
  if (suspended_p) {
    (() => { try {
    return runtime.renderer.resume();
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const error = _catch_9;
        return (runtime.suspendError = error_message(error));
      }
    }
  } })();
  }
  render_after_suspend_bang(runtime);
  return (((_truthy) => _truthy !== false && _truthy != null)(active_p) ? true : false);
}

function suspend_runtime_bang(runtime, platform, process_api) {
  if (((_truthy) => _truthy !== false && _truthy != null)(((platform === "win32") || runtime.suspendResume))) {
    return false;
  } else {
    const state = {[$$bc$property_key($$bc$keyword("handler"))]: null};
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
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const error = _catch_10;
        (runtime.suspendError = error_message(error));
        cleanup_suspend_bang(runtime, process_api);
        return false;
      }
    }
  } })();
  }
}

function quiesce_bang(runtime) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed))) {
    (runtime.disposed = true);
    cleanup_suspend_bang(runtime, process);
    if (((_truthy) => _truthy !== false && _truthy != null)(runtime.spinnerTimer)) {
      clearInterval(runtime.spinnerTimer);
    }
    (runtime.spinnerTimer = null);
    runtime.soundChildren.forEach((child) => (() => { try {
    return child.kill();
  } catch (_catch_11) {
    switch ($$bd$catch_dispatch(_catch_11, [Error])) {
      case 0: {
        const __ = _catch_11;
        return null;
      }
    }
  } })());
    return runtime.soundChildren.clear();
  }
}

function destroy_bang(runtime) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed))) {
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
return ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "z") : _logical))(key.ctrl))) ? (() => { key.preventDefault();
key.stopPropagation();
return suspend_runtime_bang(runtime, text(process.platform), process); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "q") : _logical))(key.ctrl))) ? (() => { key.preventDefault();
key.stopPropagation();
return destroy_bang(runtime); })() : null); });
}

function selected_agent_id(state, selected) {
  const agents = bridgesnapshot_agents(state);
  const total = agents.length;
  return ((total > 0) ? agent_id(agents[Math.max(0, Math.min(selected, (total - 1)))]) : "");
}

function reconcile_agent_selection_bang(runtime, prior_id) {
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

async function refresh_bang(runtime) {
  const payloads = await Promise.all([run_json([north_bin(), "agents", "--json"]).catch((__) => null), run_json([north_bin(), "json", "board", "--all"]).catch((__) => null), run_json([north_bin(), "json", "done"]).catch((__) => null)]);
  const agent_payload = payloads[0];
  const board = payloads[1];
  const done = payloads[2];
  const state = snapshot(runtime.model);
  const current_agents = bridgesnapshot_agents(state);
  const remote_agents = (((_truthy) => _truthy !== false && _truthy != null)(agent_payload) ? normalize_agents(agent_payload) : []);
  const bridge_agents = current_agents.filter((agent) => runtime.bridgeExecutions.has(agent_id(agent))).map((agent) => { const remote = remote_agents.find((candidate) => (agent_id(candidate) === agent_id(agent)));
return (((_truthy) => _truthy !== false && _truthy != null)(remote) ? agent_with_route(agent, remote) : agent); });
  const distinct_remote = remote_agents.filter((agent) => (!((_truthy) => _truthy !== false && _truthy != null)(runtime.bridgeExecutions.has(agent_id(agent)))));
  const agents = (((_truthy) => _truthy !== false && _truthy != null)(agent_payload) ? bridge_agents.concat(distinct_remote) : current_agents);
  const open_rows = (Array.isArray(board) ? board : []);
  const done_rows = (Array.isArray(done) ? done : []);
  const ids = board_ids(open_rows).concat(board_ids(done_rows));
  const facts = ((ids.length > 0) ? (async () => { const request = run_json([north_bin(), "json", "show-many", ids.join(",")]).catch((__) => []); return await request; })() : []);
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
  return ((((_truthy) => _truthy !== false && _truthy != null)(((view_id === "graph") || (view_id === "dag")))) ? "graph" : (((_truthy) => _truthy !== false && _truthy != null)(((view_id === "board") || (view_id === "kanban")))) ? "board" : "list");
}

function thread_view_command_p(name) {
  return ((name === "list") || ((name === "board") || (name === "graph")));
}

function view_list(state) {
  return [WorkView("list", "List", bridgesnapshot_list(state)), WorkView("board", "Board", bridgesnapshot_board(state)), WorkView("graph", "Graph", bridgesnapshot_list(state))];
}

function selected_view(state, view_id) {
  const views = view_list(state);
  const canonical = canonical_work_view(view_id);
  const selected = views.find((view) => (workview_id(view) === canonical));
  return ((_logical) => (_logical !== false && _logical != null ? _logical : views[0]))(selected);
}

function roster_row_suppressed_p(agent_id, supervisor_id, banner_p) {
  return (banner_p && ((!(supervisor_id === "")) && (agent_id === supervisor_id)));
}

function roster_text_bang(state, selected, supervisor_id, banner_p) {
  const agents = bridgesnapshot_agents(state);
  const width = Math.max(1, (terminal_columns() - 6));
  const rows = agents.map((agent, index) => ({[$$bc$property_key($$bc$keyword("id"))]: agent_id(agent), [$$bc$property_key($$bc$keyword("text"))]: agent_row_text_bang(agent, (index === selected), width)})).filter((row) => (!roster_row_suppressed_p(row.id, supervisor_id, banner_p)));
  return (((agents.length === 0)) ? "No agents attached" : ((rows.length === 0)) ? "" : rows.map((row) => row.text).join("\n"));
}

function roster_visible_rows(content) {
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
  return $$bc$str(agent_display_name(agent), ((status === "") ? "" : $$bc$str(" (", status, ")")), ((task === "") ? "" : $$bc$str(" — ", task)));
}

function agent_row_text_bang(agent, selected_p, width) {
  const prefix = (selected_p ? "› " : "  ");
  const prefix_width = Bun.stringWidth(prefix);
  return $$bc$str(prefix, agent_cell_text_bang(agent_summary(agent), Math.max(1, (width - prefix_width))));
}

function route_provider(agent) {
  const label = agent_field_text(agent_provider_label(agent));
  const provider = agent_field_text(agent_provider(agent));
  const target = agent_field_text(agent_provider_target(agent));
  return (((!(label === ""))) ? label : (((_truthy) => _truthy !== false && _truthy != null)(((!(provider === "")) && (!(target === ""))))) ? $$bc$str(provider, ":", target) : text_or(provider, target));
}

function route_model(agent) {
  return text_or(agent_field_text(agent_model_display(agent)), agent_field_text(agent_model(agent)));
}

function agent_route_text_bang(agent, width) {
  const provider = route_provider(agent);
  const model = route_model(agent);
  const effort = agent_field_text(agent_effort(agent));
  const provenance = agent_field_text(agent_orchestration_provenance(agent));
  const state = agent_field_text(agent_state(agent));
  const goal = agent_field_text(agent_goal(agent));
  const parts = [((provider === "") ? "" : $$bc$str("provider ", provider)), ((model === "") ? "" : $$bc$str("model ", model)), ((effort === "") ? "" : $$bc$str("effort ", effort)), ((provenance === "") ? "" : provenance), ((state === "") ? "" : $$bc$str("state ", state)), ((goal === "") ? "" : $$bc$str("goal ", goal))].filter((part) => (!(part === "")));
  return agent_cell_text_bang(parts.join(" · "), width);
}

function agent_bucket(status) {
  const value = status.trim().toLowerCase();
  return (((value === "")) ? "other" : (((_truthy) => _truthy !== false && _truthy != null)(value.startsWith("finished("))) ? (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : value.includes("process:error")))(value.includes("process:failed"))) ? "failed" : "other") : (((_truthy) => _truthy !== false && _truthy != null)(value.startsWith("inconsistent"))) ? "blocked" : (((_truthy) => _truthy !== false && _truthy != null)(((value === "stalled") || ((value === "blocked") || ((value === "waiting") || ((value === "paused") || (value === "queued"))))))) ? "blocked" : (((_truthy) => _truthy !== false && _truthy != null)(((value === "failed") || ((value === "error") || (value === "crashed"))))) ? "failed" : (((_truthy) => _truthy !== false && _truthy != null)(((value === "working") || ((value === "running") || ((value === "starting") || ((value === "ready") || ((value === "active") || (value === "online")))))))) ? "running" : "other");
}

function agents_in_bucket(agents, bucket) {
  return agents.filter((agent) => (agent_bucket(agent_status(agent)) === bucket));
}

function segment_agents(agents, segment_id) {
  return ((segment_id === "all") ? agents : agents_in_bucket(agents, segment_id));
}

function agent_total_label(total) {
  return $$bc$str(total, ((total === 1) ? " agent" : " agents"));
}

function agent_segments(agents) {
  const total = agents.length;
  const segments = [AgentSegment("all", agent_total_label(total), total)];
  STRIP_BUCKETS.forEach((bucket) => { const id = stripbucket_id(bucket);
const members = agents_in_bucket(agents, id);
const count = members.length;
if ((count > 0)) {
  return segments.push(AgentSegment(id, $$bc$str(stripbucket_glyph(bucket), count, " ", id), count));
} });
  return segments;
}

function segment_columns(segments) {
  const starts = [];
  segments.forEach((__segment, index) => starts.push(((index === 0) ? STRIP_INDENT : (() => { const previous_start = starts[(index - 1)]; const previous_width = agentsegment_label(segments[(index - 1)]).length; const separator_width = STRIP_SEPARATOR.length; return (previous_start + previous_width + separator_width); })())));
  return starts;
}

function segment_at_column(segments, column) {
  const starts = segment_columns(segments);
  return starts.findIndex((start, index) => { const segment_width = agentsegment_label(segments[index]).length;
return ((column >= start) && (column < (start + segment_width))); });
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
    push_chunk_bang(chunks, brightBlack($$bc$str("  … ", more, " more")));
  }
  return new StyledText(chunks);
}

function terminal_columns() {
  const stdout = process.stdout;
  const columns = (((_truthy) => _truthy !== false && _truthy != null)(stdout) ? stdout.columns : null);
  return Math.max(1, ((typeof columns === "number") ? columns : 120));
}

function terminal_rows() {
  const stdout = process.stdout;
  const rows = (((_truthy) => _truthy !== false && _truthy != null)(stdout) ? stdout.rows : null);
  return Math.max(1, ((typeof rows === "number") ? rows : 40));
}

function apply_view_visibility_bang(runtime, ui) {
  const threads_p = threads_view_p(runtime.view);
  (ui.agentsPane.visible = (!threads_p));
  return (ui.workPane.visible = threads_p);
}

function available_view_width() {
  return Math.max(24, (terminal_columns() - 6));
}

function available_agent_width(__runtime) {
  return available_view_width();
}

function user_block_text(runtime, body) {
  const width = available_agent_width(runtime);
  const lines = body.split("\n");
  return lines.map((line, index) => { const prefix = ((index === 0) ? "❯ " : "  ");
return $$bc$str(prefix, line).padEnd(width, " "); }).join("\n");
}

function short_directory(directory) {
  const path = text(directory);
  const home = text(process.env.HOME);
  return (((_truthy) => _truthy !== false && _truthy != null)(((!(home === "")) && path.startsWith(home))) ? $$bc$str("~", path.slice(home.length)) : path);
}

function session_context_text(runtime) {
  return $$bc$str(text_or(runtime.sessionModel, "model pending"), " ", text_or(runtime.sessionEffort, "effort pending"), " · ", text_or(short_directory(runtime.sessionCwd), "directory pending"), " · ", text_or(runtime.sessionBranch, "branch pending"));
}

function transcript_context_text(runtime) {
  const selected = runtime_selected_agent_id(runtime);
  return ((aggregate_transcript_p(runtime)) ? "all Bridge executions" : ((selected === text(runtime.supervisorId))) ? session_context_text(runtime) : ((!(selected === ""))) ? $$bc$str("Bridge execution ", selected.slice(0, 8)) : "Bridge execution pending");
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
    const line_count = lines.length;
    const overflow = Math.max(0, (line_count - limit));
    const visible = lines.slice(0, ((overflow > 0) ? (limit - 1) : limit));
    visible.forEach((line, index) => { const visible_count = visible.length;
const last_p = ((overflow === 0) && (index === (visible_count - 1)));
return push_chunk_bang(chunks, dim($$bc$str("\n  ", (((_truthy) => _truthy !== false && _truthy != null)(last_p) ? "└ " : "│ "), line))); });
    if ((overflow > 0)) {
      return push_chunk_bang(chunks, dim($$bc$str("\n  └ … +", (overflow + 1), " lines")));
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
    push_chunk_bang(chunks, dim($$bc$str(" ", commandparts_arguments(parts))));
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
  return $$bc$record_value("north.bridge.app/DiffState", {...state, rows: diffstate_rows(state).concat(row)});
}

function diff_rows(diff) {
  const source = clean_text(diff);
  return ((source === "") ? DiffState(0, 0, 0, 0, []) : source.split("\n").reduce((state, line) => ((((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("@@ "))) ? (() => { const parts = line.split(" "); return append_diff_row($$bc$record_value("north.bridge.app/DiffState", {...state, old_line: diff_start_line(parts[1]), new_line: diff_start_line(parts[2])}), DiffRow("hunk", "", "", line)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? _logical : line.startsWith("+++ ")))(line.startsWith("--- "))))(line.startsWith("index "))))(line.startsWith("diff --git ")))) ? state : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("+"))) ? append_diff_row($$bc$record_value("north.bridge.app/DiffState", {...state, new_line: (diffstate_new_line(state) + 1), additions: (diffstate_additions(state) + 1)}), DiffRow("add", "", $$bc$str(diffstate_new_line(state)), line)) : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("-"))) ? append_diff_row($$bc$record_value("north.bridge.app/DiffState", {...state, old_line: (diffstate_old_line(state) + 1), deletions: (diffstate_deletions(state) + 1)}), DiffRow("delete", $$bc$str(diffstate_old_line(state)), "", line)) : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("\\ No newline"))) ? append_diff_row(state, DiffRow("meta", "", "", line)) : append_diff_row($$bc$record_value("north.bridge.app/DiffState", {...state, old_line: (diffstate_old_line(state) + 1), new_line: (diffstate_new_line(state) + 1)}), DiffRow("context", $$bc$str(diffstate_old_line(state)), $$bc$str(diffstate_new_line(state)), line))), DiffState(0, 0, 0, 0, [])));
}

function file_change_details(change) {
  const parsed = diff_rows(change.diff);
  return FileChangeDetails(text(change.path), text(change.kind), diffstate_additions(parsed), diffstate_deletions(parsed), diffstate_rows(parsed));
}

function file_change_summary(changes) {
  return changes.reduce((summary, change) => { const details = file_change_details(change);
return $$bc$record_value("north.bridge.app/FileChangeSummary", {...summary, additions: (filechangesummary_additions(summary) + filechangedetails_additions(details)), deletions: (filechangesummary_deletions(summary) + filechangedetails_deletions(details)), files: filechangesummary_files(summary).concat(details)}); }, FileChangeSummary(0, 0, []));
}

function diff_line_number(value) {
  const number_text = text(value);
  return number_text.padStart(4, " ");
}

function push_diff_rows_bang(chunks, rows, width) {
  const limit = 36;
  const visible = rows.slice(0, limit);
  const row_count = rows.length;
  const overflow = Math.max(0, (row_count - limit));
  visible.forEach((row) => { const kind = diffrow_kind(row);
const numbers = $$bc$str(diff_line_number(diffrow_old(row)), " ", diff_line_number(diffrow_new(row)), " │ ");
const line = compact_text($$bc$str(numbers, diffrow_text(row)), width);
push_chunk_bang(chunks, white("\n"));
return push_chunk_bang(chunks, (((kind === "add")) ? (bg("#173326"))(brightGreen(line.padEnd(width, " "))) : ((kind === "delete")) ? (bg("#382127"))(brightRed(line.padEnd(width, " "))) : ((kind === "hunk")) ? brightCyan(line) : dim(line))); });
  if ((overflow > 0)) {
    return push_chunk_bang(chunks, dim($$bc$str("\n          └ … +", overflow, " diff lines")));
  }
}

function push_file_change_card_bang(chunks, item, status, runtime) {
  const data = conversationitem_data(item);
  const changes = (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? Array.isArray(data.changes) : _logical))(data)) ? data.changes : []);
  const summary = file_change_summary(changes);
  const files = filechangesummary_files(summary);
  const width = Math.max(24, (available_agent_width(runtime) - 2));
  push_chunk_bang(chunks, ((status === "failed") ? brightRed("• ") : brightGreen("• ")));
  push_chunk_bang(chunks, brightWhite($$bc$str(((status === "running") ? "Editing " : "Edited "), files.length, ((files.length === 1) ? " file" : " files"), " (+", filechangesummary_additions(summary), " -", filechangesummary_deletions(summary), ")")));
  files.forEach((file, index) => { const last_p = (index === (files.length - 1));
push_chunk_bang(chunks, brightBlack($$bc$str("\n  ", (last_p ? "└ " : "├ "), filechangedetails_path(file), " (+", filechangedetails_additions(file), " -", filechangedetails_deletions(file), ")")));
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

function transcript_placeholder(label, status, item_count, working_p) {
  const value = status.trim().toLowerCase();
  return (((item_count > 0)) ? "" : (working_p) ? "" : (((_truthy) => _truthy !== false && _truthy != null)(((value === "") || (value === "starting")))) ? $$bc$str("Starting ", label, "…") : (((_truthy) => _truthy !== false && _truthy != null)(((value === "offline") || ((value === "failed") || (value === "error"))))) ? $$bc$str(label, " is offline.") : "");
}

function transcript_banner_p(status, item_count, working_p) {
  const value = status.trim().toLowerCase();
  return ((item_count === 0) && ((!working_p) && ((!(value === "")) && ((!(value === "starting")) && ((!(value === "offline")) && ((!(value === "failed")) && (!(value === "error"))))))));
}

function banner_visible_p(runtime) {
  return ((!aggregate_transcript_p(runtime)) && ((runtime_selected_agent_id(runtime) === text(runtime.supervisorId)) && transcript_banner_p(supervisor_status(runtime), projected_conversation(runtime).length, transcript_working_p(runtime))));
}

const BANNER_LABEL_WIDTH = 13;

const BANNER_MIN_COLUMNS = 55;

const BANNER_MODEL_HINT = "/model changes the next launch";

function banner_revision(identity) {
  const value = text(identity).trim();
  return ((value === "") ? "unknown" : value.slice(0, 8));
}

function banner_permissions(mode) {
  const value = text(mode).trim();
  return (((value === "")) ? "pending" : ((value === "bypassPermissions")) ? "YOLO mode" : value);
}

function banner_field(label, value) {
  return $$bc$str($$bc$str(label, ":").padEnd(BANNER_LABEL_WIDTH, " "), value);
}

function session_banner_lines(identity, model, effort, directory, permissions) {
  const named = text_or(text(model), "pending");
  const graded = text(effort);
  const model_text = ((graded === "") ? named : $$bc$str(named, " ", graded));
  return [$$bc$str(">_ North Bridge (", banner_revision(identity), ")"), "", banner_field("model", $$bc$str(model_text, "   ", BANNER_MODEL_HINT)), banner_field("directory", text_or(text(directory), "pending")), banner_field("permissions", banner_permissions(permissions))];
}

function banner_clip(line, width) {
  const limit = Math.max(1, width);
  return ((grapheme_count(line) > limit) ? $$bc$str(line.slice(0, Math.max(0, (limit - 1))), "…") : line);
}

function widest_line_bang(lines) {
  const widest = {[$$bc$property_key($$bc$keyword("n"))]: 0};
  lines.forEach((line) => { if ((grapheme_count(line) > widest.n)) {
  return (widest.n = grapheme_count(line));
} });
  return widest.n;
}

function banner_box_bang(lines, width) {
  const inner = Math.max(1, (width - 4));
  const clipped = lines.map((line) => banner_clip(line, inner));
  if ((width < BANNER_MIN_COLUMNS)) {
    return clipped;
  } else {
    const span = Math.min(inner, widest_line_bang(clipped));
    const rule = "─".repeat((span + 2));
    return [$$bc$str("╭", rule, "╮")].concat(clipped.map((line) => $$bc$str("│ ", line.padEnd(span, " "), " │")), [$$bc$str("╰", rule, "╯")]);
  }
}

function session_banner_bang(identity, model, effort, directory, permissions, width) {
  return banner_box_bang(session_banner_lines(identity, model, effort, directory, permissions), width);
}

const BANNER_BOX_PREFIX = "│ ";

const BANNER_BOX_SUFFIX = " │";

function banner_rule_line_p(line) {
  return ((_logical) => (_logical !== false && _logical != null ? _logical : line.startsWith("╰")))(line.startsWith("╭"));
}

function banner_line_segments(line) {
  const line_length = line.length;
  return ((banner_rule_line_p(line)) ? [line, "", ""] : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (line_length >= 4) : _logical))(line.endsWith(BANNER_BOX_SUFFIX)) : _logical))(line.startsWith(BANNER_BOX_PREFIX)))) ? [BANNER_BOX_PREFIX, line.slice(2, (line_length - 2)), BANNER_BOX_SUFFIX] : ["", line, ""]);
}

function session_banner_runs(lines) {
  const runs = [];
  lines.forEach((line, index) => { const segments = banner_line_segments(line);
const prefix = segments[0];
const content = segments[1];
const suffix = segments[2];
if ((index > 0)) {
  runs.push({[$$bc$property_key($$bc$keyword("text"))]: "\n", [$$bc$property_key($$bc$keyword("tone"))]: "snapshot"});
}
if ((!(prefix === ""))) {
  runs.push({[$$bc$property_key($$bc$keyword("text"))]: prefix, [$$bc$property_key($$bc$keyword("tone"))]: "snapshot"});
}
if ((!(content === ""))) {
  runs.push({[$$bc$property_key($$bc$keyword("text"))]: content, [$$bc$property_key($$bc$keyword("tone"))]: (((_truthy) => _truthy !== false && _truthy != null)(content.includes("North Bridge")) ? "title" : "field")});
}
if ((!(suffix === ""))) {
  return runs.push({[$$bc$property_key($$bc$keyword("text"))]: suffix, [$$bc$property_key($$bc$keyword("tone"))]: "snapshot"});
} });
  return runs;
}

function render_conversation_bang(runtime) {
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
  push_chunk_bang(chunks, dim($$bc$str("\n  └ ", body.replaceAll("\n", "\n    "))));
}
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "thought")) ? (() => { push_chunk_bang(chunks, brightBlack("• Explored "));
push_chunk_bang(chunks, dim(body));
return push_chunk_bang(chunks, white("\n\n")); })() : ((kind === "error")) ? (() => { push_chunk_bang(chunks, brightRed("• Error\n  "));
push_chunk_bang(chunks, red(body.replaceAll("\n", "\n  ")));
return push_chunk_bang(chunks, white("\n\n")); })() : (() => { push_chunk_bang(chunks, brightBlack($$bc$str("• ", body)));
return push_chunk_bang(chunks, white("\n\n")); })()); });
  if (transcript_working_p(runtime)) {
    const working_since = runtime.workingSince;
    const elapsed = Math.floor(((Date.now() - working_since) / 1000));
    push_working_wave_bang(chunks, runtime);
    push_chunk_bang(chunks, brightBlack($$bc$str(" (", elapsed, "s · esc or ctrl-c to cancel)\n  ")));
    push_chunk_bang(chunks, brightBlack(transcript_context_text(runtime)));
  }
  const status = supervisor_status(runtime);
  const working_p = transcript_working_p(runtime);
  const supervisor_view_p = ((!aggregate_transcript_p(runtime)) && (runtime_selected_agent_id(runtime) === text(runtime.supervisorId)));
  const placeholder = (((_truthy) => _truthy !== false && _truthy != null)(supervisor_view_p) ? transcript_placeholder(main_agent_label(runtime), status, items.length, working_p) : "");
  if ((!(placeholder === ""))) {
    push_chunk_bang(chunks, brightBlack(placeholder));
  }
  if (banner_visible_p(runtime)) {
    session_banner_runs(session_banner_bang(runtime.sourceIdentity, runtime.sessionModel, runtime.sessionEffort, short_directory(runtime.sessionCwd), runtime.sessionPermissions, available_view_width())).forEach((run) => push_chunk_bang(chunks, (((run.tone === "title") ? brightWhite : brightBlack))(run.text)));
  }
  return new StyledText(chunks);
}

function visible_notice(notice) {
  const value = text(notice);
  return (((value === "view dag")) ? "view graph" : ((value === "view kanban")) ? "view board" : value);
}

function config_section_title(role) {
  return (((role === "moduleset")) ? "SETS" : ((role === "module")) ? "MODULES" : ((role === "skill")) ? "SKILLS" : ((role === "hook")) ? "HOOKS" : ((role === "plugin")) ? "PLUGINS" : ((role === "other")) ? "OTHER" : "");
}

function config_header_roles(role) {
  return (((role === "moduleset")) ? ["moduleset"] : ((role === "module")) ? ["skill", "module"] : ((role === "boundhook")) ? ["skill", "module"] : ((role === "skill")) ? ["skill"] : ((role === "hook")) ? ["hook"] : ((role === "plugin")) ? ["plugin"] : ((role === "other")) ? ["other"] : []);
}

function config_header_keys(entry, rows) {
  const role = config_row_role(entry, rows);
  const scope = config_row_scope(configentry_kind(entry), configentry_name(entry));
  return config_header_roles(role).map((heading) => $$bc$str(scope, " ", heading));
}

function config_header_shared_bang(prior, current) {
  const count = {[$$bc$property_key($$bc$keyword("n"))]: 0};
  current.forEach((heading, index) => { if (((_truthy) => _truthy !== false && _truthy != null)(((count.n === index) && ((index < prior.length) && (prior[index] === heading))))) {
  return (count.n = (index + 1));
} });
  return count.n;
}

function config_panel_title(config_filter) {
  return (((config_filter === "hook")) ? "hooks" : ((config_filter === "skill")) ? "skills" : ((config_filter === "plugin")) ? "plugins" : ((config_filter === "module")) ? "modules" : ((config_filter === "globals")) ? "globals" : ((config_filter === "agentsmd")) ? "directory context" : "context switchboard");
}

function config_empty_note(loaded_p, filtering_p) {
  return (((!loaded_p)) ? " loading…" : (filtering_p) ? " nothing matches" : " nothing to configure here");
}

function config_query_field(filtering_p, query) {
  return (filtering_p ? $$bc$str("  /", query) : "");
}

function config_panel_legend(filtering_p) {
  return (filtering_p ? "  ↑/↓ move · tab fold · space toggle · enter edit · esc clears filter" : "  ↑/↓ move · tab fold · space toggle · enter edit · / filter · esc close");
}

function dimmest(value) {
  return dim(brightBlack(value));
}

function config_member_count_text(count) {
  return $$bc$str(count, ((count === 1) ? " member" : " members"));
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

function config_kind_tag(kind, role) {
  const headings = config_header_roles(role);
  const depth = headings.length;
  const innermost = ((depth > 0) ? headings[(depth - 1)] : "");
  return (((_truthy) => _truthy !== false && _truthy != null)(((innermost === "") || (innermost === role))) ? "" : $$bc$str(kind, " · "));
}

function config_row_parts(entry, memberships, expanded_p, role, state_text, width) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const dir_p = (kind === "dir");
  const members = config_module_members(memberships, name);
  const detail = (((kind === "hook")) ? "" : (config_subtree_kind_p(kind)) ? "" : (dir_p) ? "" : ((kind === "module")) ? ((members == null) ? "" : config_member_count_text(members.length)) : text(configentry_detail(entry)));
  const indent = config_row_indent(role);
  const glyph = config_fold_glyph(dir_p, expanded_p);
  const tag = config_kind_tag(kind, role);
  const label = (dir_p ? config_dir_label(entry) : config_row_label(kind, name));
  const indent_width = indent.length;
  const glyph_width = glyph.length;
  const tag_width = tag.length;
  const state_width = state_text.length;
  const detail_width = detail.length;
  const room = Math.max(8, (width - indent_width - glyph_width - tag_width - state_width - detail_width - 4));
  return {[$$bc$property_key($$bc$keyword("indent"))]: indent, [$$bc$property_key($$bc$keyword("glyph"))]: glyph, [$$bc$property_key($$bc$keyword("tag"))]: tag, [$$bc$property_key($$bc$keyword("name"))]: compact_text(label, room), [$$bc$property_key($$bc$keyword("state"))]: state_text, [$$bc$property_key($$bc$keyword("detail"))]: detail};
}

const CONFIG_INDENT_WIDTH = 2;

function config_row_depth(role) {
  return (((role === "dir")) ? 0 : ((role === "ins")) ? 1 : ((role === "memroot")) ? 1 : ((role === "module")) ? 3 : ((role === "boundhook")) ? 4 : 2);
}

function config_row_indent(role) {
  return " ".repeat((CONFIG_INDENT_WIDTH * config_row_depth(role)));
}

function config_header_indent(index) {
  return " ".repeat((CONFIG_INDENT_WIDTH * (index + 2)));
}

function render_config_panel_bang(runtime) {
  const entries = config_panel_rows(runtime);
  const total = entries.length;
  const stored_entries = runtime.configEntries;
  const stored_manifest = runtime.configAllEntries;
  const manifest = (((_truthy) => _truthy !== false && _truthy != null)(stored_manifest) ? stored_manifest : (((_truthy) => _truthy !== false && _truthy != null)(stored_entries) ? stored_entries : entries));
  const stored_memberships = runtime.configMemberships;
  const memberships = (((_truthy) => _truthy !== false && _truthy != null)(stored_memberships) ? stored_memberships : []);
  const basis = (((_truthy) => _truthy !== false && _truthy != null)(stored_entries) ? stored_entries : entries);
  const expanded = panel_expanded(runtime);
  const config_filter = text_or(text(runtime.configFilter), "all");
  const filtering_p = panel_filtering_p(runtime);
  const focused_p = panel_focused_p(runtime);
  const query = panel_query(runtime);
  if ((total === 0)) {
    return new StyledText([brightYellow(config_panel_title(config_filter)), brightCyan(config_query_field(filtering_p, query)), brightBlack(config_empty_note((((_truthy) => _truthy !== false && _truthy != null)(runtime.configLoaded) ? true : false), filtering_p))]);
  } else {
    const index = clamped_index(runtime.configIndex, total);
    const window = config_visible_count(total, config_filter);
    const start = window_start(index, total, window);
    const stop = Math.min(total, (start + window));
    const width = Math.max(12, (terminal_columns() - 12));
    const parts = [brightYellow(config_panel_title(config_filter)), brightCyan(config_query_field(filtering_p, query)), brightBlack($$bc$str(config_panel_legend(filtering_p), "\n"))];
    entries.slice(start, stop).forEach((entry, offset) => { const i = (start + offset);
const cursor_p = (i === index);
const kind = configentry_kind(entry);
const active_p = config_entry_active_p(entry, manifest, memberships);
const pinned_p = ((kind === "hook") && (!config_hook_enabled_p(configentry_state(entry))));
const context_p = config_row_context_only_p(entry, query);
const role = config_row_role(entry, basis);
const open_p = ((kind === "dir") && ((!(query.trim() === "")) || ((!config_view_folds_p(config_filter)) || config_node_expanded_p(expanded, configentry_name(entry)))));
const nested_p = (role === "boundhook");
const state_text = config_state_text(entry, manifest, memberships, active_p, nested_p);
const row = config_row_parts(entry, memberships, open_p, role, state_text, width);
const headings = config_header_keys(entry, basis);
const prior = ((i === start) ? [] : config_header_keys(entries[(i - 1)], basis));
const shared = config_header_shared_bang(prior, headings);
const tail = (((i + 1) === stop) ? "" : "\n");
config_header_roles(role).forEach((heading, at) => { if ((at >= shared)) {
  return parts.push(brightYellow($$bc$str(config_header_indent(at), config_section_title(heading), "\n")));
} });
parts.push((((_truthy) => _truthy !== false && _truthy != null)((cursor_p && focused_p)) ? brightCyan("› ") : (cursor_p ? brightBlack("› ") : brightBlack("  "))));
const name_tone = ((((_truthy) => _truthy !== false && _truthy != null)(pinned_p)) ? dimmest : (((_truthy) => _truthy !== false && _truthy != null)((cursor_p && focused_p))) ? brightWhite : (context_p) ? dimmest : brightBlack);
const state_tone = ((((_truthy) => _truthy !== false && _truthy != null)(pinned_p)) ? dimmest : (active_p) ? brightGreen : brightBlack);
parts.push(name_tone($$bc$str(row.indent, row.glyph)));
if ((!(row.tag === ""))) {
  parts.push(dimmest(row.tag));
}
parts.push(name_tone(row.name));
parts.push(name_tone(": "));
parts.push(state_tone(row.state));
return parts.push(name_tone($$bc$str(((row.detail === "") ? "" : $$bc$str("  ", row.detail)), tail))); });
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

function config_section_rows(view) {
  return (((view === "all")) ? CONFIG_SECTION_ROWS : ((view === "globals")) ? 5 : ((view === "agentsmd")) ? 0 : 1);
}

function config_visible_count(total, view) {
  return detail_visible_count(total, config_section_rows(view));
}

function clamped_index(raw, total) {
  return Math.max(0, Math.min((((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0), (total - 1)));
}

function selected_detail_agent(runtime) {
  const agents = detail_agents(runtime);
  const total = agents.length;
  return ((total > 0) ? agents[clamped_index(runtime.detailIndex, total)] : null);
}

function config_header_lines_bang(entries, basis, start, stop) {
  const count = {[$$bc$property_key($$bc$keyword("n"))]: 0};
  entries.slice(start, stop).forEach((entry, offset) => { const i = (start + offset);
const headings = config_header_keys(entry, basis);
const prior = ((i === start) ? [] : config_header_keys(entries[(i - 1)], basis));
const count_n = count.n;
const heading_count = headings.length;
return (count.n = (count_n + (heading_count - config_header_shared_bang(prior, headings)))); });
  return count.n;
}

function config_detail_lines_bang(runtime) {
  const entries = config_panel_rows(runtime);
  const total = entries.length;
  const stored_entries = runtime.configEntries;
  const basis = (((_truthy) => _truthy !== false && _truthy != null)(stored_entries) ? stored_entries : entries);
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
  return ((detail_showing_p(runtime, "config")) ? config_detail_lines_bang(runtime) : (detail_showing_p(runtime, "agents")) ? (() => { const total = detail_agents(runtime).length; const agent = selected_detail_agent(runtime); const metadata = (((_truthy) => _truthy !== false && _truthy != null)(agent) ? agent_route_text_bang(agent, Math.max(12, (terminal_columns() - 8))) : ""); const extra = ((metadata === "") ? 0 : 1); return (1 + Math.max(1, Math.min(total, detail_visible_count(total, extra))) + extra); })() : (detail_showing_p(runtime, "help")) ? (1 + help_visible_rows(panel_query(runtime))) : 0);
}

function detail_height_bang(runtime) {
  return (2 + detail_body_lines_bang(runtime));
}

function agent_detail_title(segment_id) {
  return ((segment_id === "all") ? "agents" : $$bc$str("agents · ", segment_id));
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
    const metadata = (((_truthy) => _truthy !== false && _truthy != null)(selected_agent) ? agent_route_text_bang(selected_agent, width) : "");
    const extra = ((metadata === "") ? 0 : 1);
    const window = detail_visible_count(total, extra);
    const start = window_start(index, total, window);
    const stop = Math.min(total, (start + window));
    agents.slice(start, stop).forEach((agent, offset) => { const i = (start + offset);
const cursor_p = (i === index);
const tail = (((_truthy) => _truthy !== false && _truthy != null)((((i + 1) === stop) && (metadata === ""))) ? "" : "\n");
parts.push((cursor_p ? brightCyan("› ") : brightBlack("  ")));
return parts.push(((cursor_p ? brightWhite : brightBlack))($$bc$str(agent_detail_row_bang(agent, width), tail))); });
    if ((!(metadata === ""))) {
      parts.push(brightBlack($$bc$str("  ", metadata)));
    }
    return new StyledText(parts);
  }
}

function HelpRow(keys, meaning) {
  return $$bc$record_value("north.bridge.app/HelpRow", {_tag: "HelpRow", keys, meaning});
}

function helprow_keys(r) { return r.keys; }

function helprow_meaning(r) { return r.meaning; }

const HELP_ROWS = [HelpRow("Tab", "swap Agents/Threads; folds in the switchboard"), HelpRow("←/→", "switch thread view"), HelpRow("Ctrl-J / ↓", "into the agent strip, esc back out"), HelpRow("Esc /close /esc", "back or dismiss; cancels a turn at root"), HelpRow("Ctrl-C /interrupt", "cancel the turn; the message comes back"), HelpRow("/q /exit / Ctrl-Q", "quit Northbridge"), HelpRow("/help", "this panel"), HelpRow("/glyph <one>|reset", "prompt glyph"), HelpRow("/emoji <query>", "picker"), HelpRow("/sound on|off|pack", "voice lines"), HelpRow("/mute", "quiet")];

const HELP_KEY_WIDTH = 22;

function help_query_rows(query) {
  const needle = query.trim().toLowerCase();
  return ((needle === "") ? HELP_ROWS : HELP_ROWS.filter((row) => $$bc$str(helprow_keys(row), " ", helprow_meaning(row)).toLowerCase().includes(needle)));
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
  const row_count = rows.length;
  if ((row_count === 0)) {
    push_chunk_bang(chunks, brightBlack(" nothing matches"));
  } else {
    rows.forEach((row, index) => { push_chunk_bang(chunks, brightWhite(helprow_keys(row).padEnd(HELP_KEY_WIDTH, " ")));
push_chunk_bang(chunks, brightBlack(helprow_meaning(row)));
if ((index < (row_count - 1))) {
  return push_chunk_bang(chunks, brightBlack("\n"));
} });
  }
  return new StyledText(chunks);
}

function render_detail_panel_bang(runtime) {
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

const VIEW_TAB_SEPARATOR = " | ";

const SUBVIEW_TAB_SEPARATOR = " > ";

const THREADS_TAB_START = (() => { const agents_label_width = AGENTS_TAB_LABEL.length; const view_separator_width = VIEW_TAB_SEPARATOR.length; return (agents_label_width + view_separator_width); })();

const SUBVIEW_TAB_ORIGIN = (() => { const threads_label_width = THREADS_TAB_LABEL.length; const subview_separator_width = SUBVIEW_TAB_SEPARATOR.length; return (THREADS_TAB_START + threads_label_width + subview_separator_width); })();

const SUBVIEW_TAB_GAP = 2;

function push_session_identity_bang(chunks, session) {
  push_chunk_bang(chunks, brightYellow($$bc$str(text_or(session.sessionModel, "model pending"), " ", text_or(session.sessionEffort, "effort pending"))));
  push_chunk_bang(chunks, brightBlack(" · "));
  push_chunk_bang(chunks, brightGreen(text_or(short_directory(session.sessionCwd), "directory pending")));
  push_chunk_bang(chunks, brightBlack(" · "));
  return push_chunk_bang(chunks, dim(text_or(session.sessionBranch, "branch pending")));
}

function render_view_tabs_bang(view, state, view_id, session) {
  const chunks = [];
  const threads_p = threads_view_p(view);
  const views = view_list(state);
  push_chunk_bang(chunks, ((threads_p ? brightBlack : brightGreen))(AGENTS_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(VIEW_TAB_SEPARATOR));
  push_chunk_bang(chunks, ((threads_p ? brightGreen : brightBlack))(THREADS_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(SUBVIEW_TAB_SEPARATOR));
  if (threads_p) {
    views.forEach((view, index) => { const selected_p = (workview_id(view) === view_id);
const title = workview_title(view);
push_chunk_bang(chunks, ((selected_p ? brightGreen : brightBlack))($$bc$str((selected_p ? "[" : " "), title, (selected_p ? "]" : " "))));
if ((index < (views.length - 1))) {
  return push_chunk_bang(chunks, white("  "));
} });
    push_chunk_bang(chunks, brightBlack("  ←/→ switch"));
  } else {
    push_session_identity_bang(chunks, session);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((!threads_p) && aggregate_transcript_p(session)))) {
    push_chunk_bang(chunks, brightYellow(" · all transcripts"));
  }
  return new StyledText(chunks);
}

function compact_text(value, width) {
  const source = text(value);
  const limit = Math.max(1, width);
  return ((source.length > limit) ? $$bc$str(source.slice(0, Math.max(0, (limit - 1))), "…") : source);
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
  return available_view_width();
}

function render_list_view_bang(runtime, items, selected, width) {
  const chunks = [];
  const rows = list_rows(runtime, items);
  const collapsed = runtime.collapsedListConditions;
  rows.forEach((row, visual_index) => { const kind = listrow_kind(row);
const condition = listrow_condition(row);
if ((kind === "header")) {
  const collapsed_p = collapsed.has(condition);
  const header = $$bc$str(" ", (((_truthy) => _truthy !== false && _truthy != null)(collapsed_p) ? "▸" : "▾"), "  ", list_section_title(condition), "  ", listrow_count(row));
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
  push_chunk_bang(chunks, dim($$bc$str("  @", short_thread_id(item))));
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
push_chunk_bang(chunks, dim($$bc$str("  @", short_thread_id(item), "\n")));
if ((dependencies.length === 0)) {
  push_chunk_bang(chunks, brightBlack("    ╰─ root\n"));
} else {
  dependencies.forEach((dependency) => { const target = work_item_by_id(items, dependency);
push_chunk_bang(chunks, brightBlack("    ╰─ requires ← "));
push_chunk_bang(chunks, ((((_truthy) => _truthy !== false && _truthy != null)(target) ? brightCyan : brightBlack))($$bc$str("@", dependency.slice(0, 8))));
return push_chunk_bang(chunks, (((_truthy) => _truthy !== false && _truthy != null)(target) ? dim($$bc$str("  ", compact_text(workitem_title(target), Math.max(8, (width - 28))), "\n")) : brightBlack("  outside current board\n"))); });
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
  return $$bc$str("board-card-", thread_id);
}

function board_signature(items, selected, width) {
  return $$bc$str(width, "|", selected, "|", items.map((item) => $$bc$str(workitem_id(item), "\x01", workitem_title(item), "\x01", workitem_body(item), "\x01", workitem_condition(item))).join("\u0002"));
}

function board_card_node(source) {
  return (() => { let node = source; while (true) {
    if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(node)) || (!(text(node.northThreadId) === ""))))) { return node; } else { const _recur_0 = node.parent; node = _recur_0; continue; }
  } })();
}

function set_board_notice_bang(runtime, notice) {
  (runtime.workspaceNotice = notice);
  return runtime.render();
}

function select_board_card_bang(runtime, ui, item, index) {
  (runtime.workIndex = index);
  (runtime.model = select_thread(runtime.model, workitem_id(item)));
  show_view_bang(runtime, ui, "threads");
  return ui.workScroll.scrollChildIntoView(board_card_id(workitem_id(item)));
}

function prefill_outcome_bang(runtime, ui, thread_id) {
  (ui.composerInput.value = $$bc$str("/outcome @", thread_id, " "));
  show_view_bang(runtime, ui, "threads");
  return set_board_notice_bang(runtime, $$bc$str("Finish the outcome, then press Enter; Done is derived from north tell @", thread_id, " outcome <result>."));
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
  const source_id = (((_truthy) => _truthy !== false && _truthy != null)(source_card) ? bare(source_card.northThreadId) : "");
  const state = snapshot(runtime.model);
  const items = bridgesnapshot_board(state);
  const source_condition = work_item_condition_for(items, source_id);
  const source_lane = board_lane_id(source_condition);
  const target_id = (((_truthy) => _truthy !== false && _truthy != null)(target_card) ? bare(target_card.northThreadId) : "");
  const target_condition = work_item_condition_for(items, target_id);
  return (((source_id === "")) ? set_board_notice_bang(runtime, "Drop ignored: the dragged source was not a North work card.") : (((_truthy) => _truthy !== false && _truthy != null)(((target_lane === "done") && (!terminal_condition_p(source_condition))))) ? prefill_outcome_bang(runtime, ui, source_id) : (((_truthy) => _truthy !== false && _truthy != null)(((source_lane === "not-started") && ((target_lane === "not-started") && ((source_condition === "ready") && ((target_id === "") || (target_condition === "ready"))))))) ? ((source_id === target_id) ? set_board_notice_bang(runtime, "Queue order unchanged: a card cannot be moved relative to itself.") : (() => { const event_x = event.x; const position = ((target_id === "") ? (() => { const current_left = event.currentTarget.screenX; const current_width = event.currentTarget.width; const current_midpoint = (current_width / 2); return ((event_x < (current_left + current_midpoint)) ? "first" : "last"); })() : (() => { const target_left = target_card.screenX; const target_width = target_card.width; const target_midpoint = (target_width / 2); return ((event_x < (target_left + target_midpoint)) ? "before" : "after"); })()); return report_promise_bang(runtime, move_ready_thread_bang(runtime, source_id, position, target_id)); })()) : set_board_notice_bang(runtime, unsupported_drop_notice(source_condition, target_lane)));
}

function card_content(item, width) {
  const body = compact_body(workitem_body(item), Math.max(8, (width - 4)));
  const fallback = $$bc$str("@", short_thread_id(item));
  return new StyledText([brightWhite(compact_text(workitem_title(item), Math.max(8, (width - 4)))), white("\n"), dim(((body === "") ? fallback : body))]);
}

function make_board_card_bang(runtime, ui, item, index, lane_index, width) {
  const renderer = runtime.renderer;
  const thread_id = workitem_id(item);
  const condition = workitem_condition(item);
  const selected_p = (index === runtime.workIndex);
  const next_up_p = ((condition === "ready") && (lane_index === 0));
  const card = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("id"))]: board_card_id(thread_id), [$$bc$property_key($$bc$keyword("width"))]: width, [$$bc$property_key($$bc$keyword("height"))]: 4, [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("paddingX"))]: 1, [$$bc$property_key($$bc$keyword("border"))]: true, [$$bc$property_key($$bc$keyword("borderColor"))]: (selected_p ? "#22d3ee" : "#64748b"), [$$bc$property_key($$bc$keyword("title"))]: (((_truthy) => _truthy !== false && _truthy != null)(next_up_p) ? "Next Up" : null), [$$bc$property_key($$bc$keyword("titleColor"))]: (((_truthy) => _truthy !== false && _truthy != null)(next_up_p) ? "#4ade80" : "#94a3b8")});
  const content = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("height"))]: 2, [$$bc$property_key($$bc$keyword("selectable"))]: false, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("truncate"))]: true, [$$bc$property_key($$bc$keyword("content"))]: card_content(item, width)});
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
  const lane_box = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexDirection"))]: "column", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("border"))]: ["bottom"], [$$bc$property_key($$bc$keyword("borderColor"))]: "#334155", [$$bc$property_key($$bc$keyword("paddingBottom"))]: 1});
  const header = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("height"))]: 1, [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("selectable"))]: false, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("content"))]: new StyledText([brightGreen(title), brightBlack($$bc$str("  ", lane_items.length))])});
  const cards = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("minHeight"))]: 4, [$$bc$property_key($$bc$keyword("flexDirection"))]: "row", [$$bc$property_key($$bc$keyword("flexWrap"))]: "wrap", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("gap"))]: 1, [$$bc$property_key($$bc$keyword("rowGap"))]: 1});
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
    if (((_truthy) => _truthy !== false && _truthy != null)(((selected >= 0) && (selected < items.length)))) {
      return ui.workScroll.scrollChildIntoView(board_card_id(workitem_id(items[selected])));
    }
  }
}

function work_content_bang(runtime, state, view, selected) {
  const items = workview_items(view);
  const width = available_work_width(runtime, state);
  return ((items.length === 0) ? new StyledText([brightBlack($$bc$str("No ", workview_title(view), " items"))]) : (((workview_id(view) === "graph")) ? render_dag_view_bang(items, selected, width) : render_list_view_bang(runtime, items, selected, width)));
}

function composer_hint(pane, label) {
  return ((pane === "agents") ? $$bc$str("Message ", label, "…") : "/list, /board, /graph, /capture, /filter, /assign");
}

function minibuffer_placeholder(runtime) {
  return composer_hint(text(runtime.view), main_agent_label(runtime));
}

function palette_visible_count(total, rows, docked) {
  return fitted_window(total, rows, (CHROME_ROWS + MIN_WORKSPACE_ROWS + docked));
}

function palette_option_rows(total, window) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((total > window) && (window > 1))) ? (window - 1) : window);
}

function palette_overflow(total, window, rows) {
  return (((_truthy) => _truthy !== false && _truthy != null)(((total > rows) && (window > rows))) ? (total - rows) : 0);
}

function render_minibuffer_bang(runtime, ui) {
  const view = text(runtime.view);
  const input = ui.composerInput;
  const options = palette_options(view, text(input.value));
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
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed))) {
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
    apply_view_visibility_bang(runtime, ui);
    (ui.viewTabsText.content = render_view_tabs_bang(runtime.view, state, workview_id(current), runtime));
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
    (ui.statusText.visible = (threads_view_p(runtime.view) && notice_p));
    (ui.agentStatusText.visible = ((!threads_view_p(runtime.view)) && notice_p));
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
  const updated = Agent(execution_id, ((role === "supervisor") ? main_agent_label(runtime) : $$bc$str("Codex ", execution_id.slice(0, 8))), status, ((role === "supervisor") ? "Northbridge control session" : "Bridge execution"), "", "", "", "", "", "", "", "", "");
  runtime.bridgeExecutions.add(execution_id);
  if ((role === "supervisor")) {
    (runtime.supervisorId = execution_id);
  }
  (runtime.model = upsert_agent(runtime.model, (((_truthy) => _truthy !== false && _truthy != null)(existing) ? agent_with_route(updated, existing) : updated)));
  reconcile_agent_selection_bang(runtime, text_or(prior_id, ((role === "supervisor") ? execution_id : "")));
  return runtime.render();
}

function record_line(line) {
  const close = line.indexOf("] ");
  if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("["))) || (close < 0)))) {
    return null;
  } else {
    const rest = line.slice((close + 2));
    const space = rest.indexOf(" ");
    const kind = ((space < 0) ? rest : rest.slice(0, space));
    const payload = ((space < 0) ? "" : rest.slice((space + 1)).trim());
    return (() => { try {
    return ParsedRecord(Number(line.slice(1, close)), kind, ((payload === "") ? {} : JSON.parse(payload)));
  } catch (_catch_12) {
    switch ($$bd$catch_dispatch(_catch_12, [Error])) {
      case 0: {
        const __ = _catch_12;
        return null;
      }
    }
  } })();
  }
}

function event_item_id(execution_id, item_id) {
  return $$bc$str(execution_id, ":", text(item_id));
}

function safe_sequence(value) {
  const candidate = Number(value);
  return (Number.isSafeInteger(candidate) ? candidate : 0);
}

function wire_conversation_item(existing, id, kind, title, body, status, item_data, execution_id, event) {
  return owned_conversation_item(id, kind, title, body, status, item_data, execution_id, (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_at(existing) : text(event.at)), (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_cursor(existing) : (safe_sequence(event.sequence) + 1)), (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_sequence(existing) : 0));
}

function control_conversation_item(execution_id, record, kind, title, body, status) {
  const data = ((_logical) => (_logical !== false && _logical != null ? _logical : {}))(parsedrecord_data(record));
  const record_sequence = parsedrecord_sequence(record);
  const cursor_candidate = Number(data.wireCursor);
  const cursor = (Number.isSafeInteger(cursor_candidate) ? cursor_candidate : 9007199254740991);
  return owned_conversation_item(event_item_id(execution_id, $$bc$str("control:", record_sequence)), kind, title, body, status, null, execution_id, text(data.bridgeRecordAt), cursor, (record_sequence + 1));
}

function append_item_delta_bang(runtime, stream_state, event, id, kind, title, delta, status) {
  const existing = conversation_item_by_id(runtime, id);
  const prior = (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_body(existing) : "");
  const actual_title = (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_title(existing) : title);
  return upsert_conversation_bang(runtime, wire_conversation_item(existing, id, kind, actual_title, clipped($$bc$str(prior, delta), 6000), status, (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_data(existing) : null), text(stream_state.executionId), event));
}

function wire_content_text(value) {
  return (((value == null)) ? "" : ((typeof value === "string")) ? value : (Array.isArray(value)) ? value.map((part) => wire_content_text(part)).filter((part) => (!(part === ""))).join("\n") : safe_json(value));
}

function adopt_wire_model_bang(runtime, model, effort) {
  if (((_truthy) => _truthy !== false && _truthy != null)(model)) {
    const provider = text(model.provider);
    const tier = text(model.tier);
    const label = ((((_truthy) => _truthy !== false && _truthy != null)(((!(provider === "")) && (!(tier === ""))))) ? $$bc$str(provider, "/", tier) : ((!(provider === ""))) ? provider : "");
    if (((_truthy) => _truthy !== false && _truthy != null)(((!(label === "")) && (text(runtime.sessionModel).trim() === "")))) {
      (runtime.sessionModel = label);
    }
  }
  const effort_label = text(effort);
  if ((!(effort_label === ""))) {
    return (runtime.sessionEffort = effort_label);
  }
}

function intermediate_provider_session_replacement_p(data) {
  return ((text(data.status) === "failed") && ((text(data.origin) === "north") && (text(data.errorCode) === "provider_session_replaced")));
}

function handle_wire_message_bang(runtime, stream_state, data) {
  const role = text(data.role);
  const stage = text(data.stage);
  const execution_id = text(stream_state.executionId);
  const id = event_item_id(execution_id, data.messageId);
  const body = clean_text(wire_content_text(data.content));
  const existing = conversation_item_by_id(runtime, id);
  if (((_truthy) => _truthy !== false && _truthy != null)(((role === "assistant") && (!((_truthy) => _truthy !== false && _truthy != null)(stream_state.booting))))) {
    return (((stage === "started")) ? upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "assistant", "", "", "running", null, execution_id, data)) : ((stage === "delta")) ? append_item_delta_bang(runtime, stream_state, data, id, "assistant", "", body, "running") : ((stage === "completed")) ? (() => { const completed_body = ((body === "") ? (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_body(existing) : "") : body); (runtime.lastAssistantText = completed_body);
return upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "assistant", "", completed_body, "done", null, execution_id, data)); })() : null);
  }
}

function handle_wire_tool_bang(runtime, stream_state, data, kind) {
  const id = event_item_id(text(stream_state.executionId), data.toolCallId);
  const existing = conversation_item_by_id(runtime, id);
  return (((kind === "tool.admitted")) ? upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "tool", text(data.name), clean_text(data.argumentPreview), "running", null, text(stream_state.executionId), data)) : ((kind === "tool.progress")) ? append_item_delta_bang(runtime, stream_state, data, id, "tool", (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_title(existing) : "Tool activity"), wire_content_text(data.progress), "running") : ((kind === "tool.terminal")) ? upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "tool", (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_title(existing) : "Tool activity"), text_or(data.resultPreview, (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_body(existing) : "")), ((text(data.status) === "succeeded") ? "done" : "failed"), (((_truthy) => _truthy !== false && _truthy != null)(existing) ? conversationitem_data(existing) : null), text(stream_state.executionId), data)) : null);
}

function handle_record_bang(runtime, stream_state, record) {
  const kind = parsedrecord_kind(record);
  const data = ((_logical) => (_logical !== false && _logical != null ? _logical : {}))(parsedrecord_data(record));
  const execution_id = text(stream_state.executionId);
  return (((kind === "execution.accepted")) ? (() => { const cwd = text(data.cwd); const prompt = text(data.prompt).trim(); if ((!(cwd === ""))) {
  (runtime.sessionCwd = cwd);
}
if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(stream_state.booting)) && (!(prompt === ""))))) {
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
} })() : (((_truthy) => _truthy !== false && _truthy != null)(((kind === "control.submit_input") || (kind === "control.redirect_now")))) ? (() => { const input = text(data.input).trim(); if ((!(input === ""))) {
  return upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "user", "", input, "done"));
} })() : ((kind === "control.interrupt_turn")) ? upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "interrupted", "", "Conversation interrupted — tell the model what to do differently.", "interrupted")) : ((kind === "model-call.started")) ? (() => { const booting = stream_state.booting; adopt_wire_model_bang(runtime, data.model, data.effort);
set_execution_working_bang(runtime, execution_id, true, (((_truthy) => _truthy !== false && _truthy != null)(booting) ? $$bc$str("Starting ", main_agent_label(runtime), "…") : "Agent is working"));
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), (((_truthy) => _truthy !== false && _truthy != null)(booting) ? "starting" : "working"));
} })() : ((kind === "message.recorded")) ? handle_wire_message_bang(runtime, stream_state, data) : (((_truthy) => _truthy !== false && _truthy != null)(((kind === "tool.admitted") || ((kind === "tool.progress") || (kind === "tool.terminal"))))) ? handle_wire_tool_bang(runtime, stream_state, data, kind) : ((kind === "run.progress")) ? (() => { const progress = ((_logical) => (_logical !== false && _logical != null ? _logical : {}))(data.progress); const action = text(progress.currentAction); const lifecycle = text(data.lifecycle); adopt_wire_model_bang(runtime, progress.model, progress.effort);
if (((_truthy) => _truthy !== false && _truthy != null)(progress.branch)) {
  (runtime.sessionBranch = text(progress.branch.name));
}
if ((lifecycle === "waiting")) {
  return set_working_bang(runtime, false, "");
} else {
  if ((!(action === ""))) {
    return set_working_bang(runtime, true, action);
  }
} })() : ((kind === "artifact.published")) ? (() => { const id = event_item_id(execution_id, data.artifactId); const existing = conversation_item_by_id(runtime, id); return upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "change", text_or(data.label, "Published artifact"), text(data.mediaType), "done", null, execution_id, data)); })() : ((kind === "model-call.completed")) ? ((!intermediate_provider_session_replacement_p(data)) ? (() => { return set_execution_working_bang(runtime, execution_id, false, ""); })() : null) : ((kind === "session.idle")) ? (() => { const disposition = text(data.disposition); const pending_inputs = Number(((_logical) => (_logical !== false && _logical != null ? _logical : 0))(data.pendingInputs)); const booting = stream_state.booting; set_execution_working_bang(runtime, execution_id, false, "");
if (((_truthy) => _truthy !== false && _truthy != null)(booting)) {
  play_sound_event_bang(runtime, stream_state, "ready");
} else if ((disposition === "interrupted")) {
  play_sound_event_bang(runtime, stream_state, "interrupted");
} else if (((_truthy) => _truthy !== false && _truthy != null)(((disposition === "completed") && (pending_inputs <= 0)))) {
  play_sound_event_bang(runtime, stream_state, "done");
} else {
  null;
}
(stream_state.booting = false);
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "ready");
} })() : ((kind === "run.terminated")) ? (() => { set_execution_working_bang(runtime, execution_id, false, "");
if (((_truthy) => _truthy !== false && _truthy != null)(((text(data.lifecycle) === "failed") || (text(data.lifecycle) === "blocked")))) {
  play_sound_event_bang(runtime, stream_state, "failed");
  const id = event_item_id(execution_id, $$bc$str("terminal:", data.sequence));
  const existing = conversation_item_by_id(runtime, id);
  upsert_conversation_bang(runtime, wire_conversation_item(existing, id, "error", "Error", failure_summary(data.reason), "failed", null, execution_id, data));
}
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "offline");
} })() : ((kind === "execution.failure")) ? (() => { set_execution_working_bang(runtime, execution_id, false, "");
return upsert_conversation_bang(runtime, control_conversation_item(execution_id, record, "error", "Error", $$bc$str(kind, ": ", failure_summary(data)), "failed")); })() : null);
}

function parse_bridge_stream_bang(runtime, stream_state, chunk) {
  const lines = $$bc$str(stream_state.buffer, chunk).split("\n");
  const remainder = lines.pop();
  (stream_state.buffer = remainder);
  return lines.forEach((raw_line) => { const line = raw_line.trim();
return ((((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("execution "))) ? (() => { const execution_id = line.slice(10).trim(); (stream_state.executionId = execution_id);
(stream_state.soundLive = true);
return bridge_agent_bang(runtime, execution_id, text(stream_state.role), "starting"); })() : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("attached "))) ? (stream_state.soundLive = true) : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("["))) ? (() => { const record = record_line(line); if (((_truthy) => _truthy !== false && _truthy != null)(record)) {
  return handle_record_bang(runtime, stream_state, record);
} })() : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("northd: "))) ? publish_line_bang(runtime, line.slice(8)) : (((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("north bridge:"))) ? append_error_bang(runtime, line) : null); });
}

function launch_route_flags(provider, tier, model, effort) {
  const provider_value = text(provider).trim();
  const tier_value = text(tier).trim();
  const model_value = text(model).trim();
  const effort_value = text(effort).trim();
  return [].concat(((provider_value === "") ? [] : ["--provider", provider_value]), ((tier_value === "") ? [] : ["--tier", tier_value]), ((model_value === "") ? [] : ["--model", model_value]), ((effort_value === "") ? [] : ["--effort", effort_value]));
}

function launch_route_summary(runtime) {
  return $$bc$str("next launch: provider ", text_or(runtime.launchProvider, "auto"), ", model ", text_or(text_or(runtime.launchModel, text(runtime.launchTier)), "auto"), ", effort ", text_or(runtime.launchEffort, "auto"));
}

function set_launch_route_bang(runtime, name, value) {
  const trimmed = value.trim();
  const choice = trimmed.toLowerCase();
  if ((choice === "")) {
    (() => { throw new Error($$bc$str(name, " requires a value or auto")); })();
  }
  if ((name === "provider")) {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(((choice === "auto") || ((choice === "anthropic") || (choice === "openai")))))) {
      (() => { throw new Error("provider requires anthropic, openai, or auto"); })();
    }
    (runtime.launchProvider = ((choice === "auto") ? "" : choice));
  } else if ((name === "model")) {
    if ((choice === "auto")) {
      (runtime.launchTier = "");
      (runtime.launchModel = "");
    } else {
      if (((_truthy) => _truthy !== false && _truthy != null)(["economy", "standard", "senior", "frontier"].includes(choice))) {
        (runtime.launchTier = choice);
        (runtime.launchModel = "");
      } else {
        (runtime.launchTier = "");
        (runtime.launchModel = trimmed);
      }
    }
  } else {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(((choice === "auto") || ["low", "medium", "high", "xhigh", "max"].includes(choice))))) {
      (() => { throw new Error("effort requires low, medium, high, xhigh, max, or auto"); })();
    }
    (runtime.launchEffort = ((choice === "auto") ? "" : choice));
  }
  return publish_line_bang(runtime, launch_route_summary(runtime));
}

function take_launch_route_flags_bang(runtime) {
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
  set_working_bang(runtime, true, $$bc$str("Starting ", main_agent_label(runtime), "…"));
  const stream_state = {[$$bc$property_key($$bc$keyword("buffer"))]: "", [$$bc$property_key($$bc$keyword("stderr"))]: "", [$$bc$property_key($$bc$keyword("executionId"))]: "", [$$bc$property_key($$bc$keyword("role"))]: role, [$$bc$property_key($$bc$keyword("booting"))]: (role === "supervisor"), [$$bc$property_key($$bc$keyword("soundLive"))]: false};
  const exit_code = await stream_command([north_bin(), "bridge", "--role", ((role === "supervisor") ? "director" : "implementer")].concat(take_launch_route_flags_bang(runtime), [prompt]), (chunk) => parse_bridge_stream_bang(runtime, stream_state, chunk), (chunk) => (stream_state.stderr = clipped($$bc$str(stream_state.stderr, chunk), 6000)));
  if ((!(exit_code === 0))) {
    set_working_bang(runtime, false, "");
    return append_error_bang(runtime, $$bc$str("Bridge exited ", exit_code, ((text(stream_state.stderr).trim() === "") ? "" : $$bc$str("\n", text(stream_state.stderr).trim()))));
  }
}

function popout_bang(runtime, view_id) {
  const ghostty = Bun.which("ghostty");
  const kitty = Bun.which("kitty");
  const wezterm = Bun.which("wezterm");
  const foot = Bun.which("foot");
  const xterm = Bun.which("xterm");
  const argv = ((((_truthy) => _truthy !== false && _truthy != null)(ghostty)) ? [ghostty, "-e", north_bin(), "bridge", "app", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(kitty)) ? [kitty, "--detach", north_bin(), "bridge", "app", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(wezterm)) ? [wezterm, "start", "--always-new-process", "--", north_bin(), "bridge", "app", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(foot)) ? [foot, north_bin(), "bridge", "app", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(xterm)) ? [xterm, "-e", north_bin(), "bridge", "app", "--view-id", view_id] : null);
  if ((argv == null)) {
    (() => { throw new Error("no supported terminal found for pop-out"); })();
  }
  const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: argv, [$$bc$property_key($$bc$keyword("stdin"))]: "ignore", [$$bc$property_key($$bc$keyword("stdout"))]: "ignore", [$$bc$property_key($$bc$keyword("stderr"))]: "ignore"});
  child.unref();
  return publish_line_bang(runtime, $$bc$str("opened ", view_id, " in a separate terminal"));
}

function selected_work(runtime, selection) {
  const state = snapshot(runtime.model);
  const view_id = text_or(workselection_view(selection), text(runtime.activeView));
  const view = selected_view(state, view_id);
  const items = workview_items(view);
  const index = workselection_index(selection);
  if (((_truthy) => _truthy !== false && _truthy != null)(((index >= 0) && (index < items.length)))) {
    const item = items[index];
    return (((_truthy) => _truthy !== false && _truthy != null)(((!(workview_id(view) === "list")) || (!((_truthy) => _truthy !== false && _truthy != null)(runtime.collapsedListConditions.has(list_section_id(workitem_condition(item))))))) ? item : null);
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

function restore_submitted_text_bang(runtime, ui) {
  const pending = text(runtime.lastSubmitted);
  const input = ui.composerInput;
  if (((_truthy) => _truthy !== false && _truthy != null)(((!(pending === "")) && input))) {
    (runtime.lastSubmitted = "");
    (input.value = pending);
    return input.focus();
  }
}

async function cancel_turn_bang(runtime, ui, target) {
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtime.bridgeExecutions.has(target)))) {
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
  return (handle_local_command_bang(runtime, ui, input) ? null : ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "launch") : _logical))(slash_p))) ? await launch_agent_bang(runtime, rest, "worker") : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "capture") : _logical))(slash_p))) ? await capture_thread_bang(runtime, rest) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "refresh") : _logical))(slash_p))) ? await refresh_bang(runtime) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "popout") : _logical))(slash_p))) ? popout_bang(runtime, text_or(rest, text(runtime.activeView))) : (async () => { if ((target === "")) {
  (() => { throw new Error("select an agent before messaging or interrupting"); })();
}
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "interrupt") : _logical))(slash_p))) {
  return await cancel_turn_bang(runtime, ui, target);
} else {
  const message = trimmed;
  if ((message === "")) {
    (() => { throw new Error("nothing to send"); })();
  }
  if ((!((_truthy) => _truthy !== false && _truthy != null)(runtime.bridgeExecutions.has(target)))) {
    const id = next_item_id_bang(runtime, "user");
    upsert_conversation_bang(runtime, owned_conversation_item(id, "user", "", message, "done", null, target, new Date().toISOString(), runtime.itemSequence, 1));
  }
  (runtime.lastSubmitted = message);
  if (((_truthy) => _truthy !== false && _truthy != null)(runtime.bridgeExecutions.has(target))) {
    set_execution_working_bang(runtime, target, true, "Codex is working");
  } else {
    set_working_bang(runtime, true, "Codex is working");
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(runtime.bridgeExecutions.has(target))) {
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
  return ((!((_truthy) => _truthy !== false && _truthy != null)(slash_p)) ? await submit_agent_bang(runtime, ui, input, runtime.supervisorId) : (handle_local_command_bang(runtime, ui, input) ? null : (((name === "filter")) ? (() => { (runtime.model = set_filter(runtime.model, rest));
return runtime.render(); })() : ((name === "refresh")) ? await refresh_bang(runtime) : ((name === "popout")) ? popout_bang(runtime, ((rest === "") ? runtime.activeView : rest)) : ((name === "capture")) ? await capture_thread_bang(runtime, rest) : ((name === "assign")) ? (async () => { const item = selected_work(runtime, selection); const words = rest.split(" ").filter((word) => (!(word.trim() === ""))); const current = (((_truthy) => _truthy !== false && _truthy != null)(item) ? workitem_driver(item) : ""); const prior = ((words.length > 1) ? words[0] : current); const next_driver = ((words.length > 1) ? words[1] : text(words[0])); const thread_id = (((_truthy) => _truthy !== false && _truthy != null)(item) ? workitem_id(item) : ""); if ((thread_id === "")) {
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
publish_line_bang(runtime, $$bc$str("assigned @", thread_id, " to ", next_driver));
return await refresh_bang(runtime); })() : ((name === "outcome")) ? (async () => { const split_at = rest.indexOf(" "); const thread_id = ((split_at < 0) ? "" : bare(rest.slice(0, split_at))); const result = ((split_at < 0) ? "" : rest.slice((split_at + 1)).trim()); if ((thread_id === "")) {
  (() => { throw new Error("outcome requires: /outcome <thread-id> <result>"); })();
}
if ((result === "")) {
  (() => { throw new Error("outcome requires a result"); })();
}
await run_command([north_bin(), "tell", thread_id, "outcome", result]);
(runtime.workspaceNotice = $$bc$str("Recorded outcome for @", thread_id, "."));
return await refresh_bang(runtime); })() : (() => { throw new Error("unknown thread command; use /help"); })())));
}

function report_promise_bang(runtime, promise) {
  return promise.catch((error) => publish_line_bang(runtime, $$bc$str("error: ", error_message(error))));
}

function select_view_bang(runtime, view) {
  (runtime.view = view);
  (runtime.paletteIndex = 0);
  (runtime.workspaceNotice = "");
  return clear_strip_focus_bang(runtime);
}

function show_view_bang(runtime, ui, view) {
  select_view_bang(runtime, view);
  ui.composerInput.focus();
  return runtime.render();
}

function show_thread_view_bang(runtime, ui, view_id) {
  (runtime.model = focus_view(runtime.model, canonical_work_view(view_id)));
  (runtime.workIndex = 0);
  runtime.workScroll.scrollTo(0);
  return show_view_bang(runtime, ui, "threads");
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
  if (((_truthy) => _truthy !== false && _truthy != null)(runtime.stripFocused)) {
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
    const strip_index = runtime.stripIndex;
    const next_index = ((strip_index + delta + total) % total);
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
  const view = text(runtime.view);
  const input = active_input(runtime, ui);
  return palette_options(view, text(input.value));
}

function install_composer_keymap_bang(runtime) {
  const keymap = createDefaultOpenTuiKeymap(runtime.renderer);
  registerEmacsBindings(keymap);
  registerEscapeClearsPendingSequence(keymap);
  return (runtime.keymap = keymap);
}

function palette_completion_text(candidate) {
  const completion = slashcommand_completion(candidate);
  return ((completion === "") ? $$bc$str(slashcommand_name(candidate), (slashcommand_arguments(candidate) ? " " : "")) : completion);
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

function submit_input_bang(runtime, ui, input) {
  if ((!(input === ""))) {
    (ui.composerInput.value = "");
    (runtime.paletteIndex = 0);
    if ((!threads_view_p(runtime.view))) {
      const state = snapshot(runtime.model);
      const selected = selected_agent_id(state, runtime.agentIndex);
      return report_promise_bang(runtime, submit_agent_bang(runtime, ui, input, selected));
    } else {
      return report_promise_bang(runtime, submit_work_bang(runtime, ui, input, WorkSelection(runtime.activeView, runtime.workIndex)));
    }
  }
}

function palette_enter_action(matches, takes_arguments_p, insert_only_p, completed_p) {
  return (((matches < 1)) ? "" : (completed_p) ? "fire" : (((_truthy) => _truthy !== false && _truthy != null)((insert_only_p || takes_arguments_p))) ? "complete" : "fire");
}

function install_input_bang(runtime, ui) {
  ui.composerInput.on(InputRenderableEvents.INPUT, (__value) => { (runtime.paletteIndex = 0);
return render_minibuffer_bang(runtime, ui); });
  return ui.composerInput.on(InputRenderableEvents.ENTER, () => submit_input_bang(runtime, ui, text(ui.composerInput.value).trim()));
}

function subview_tab_id_at_bang(views, column) {
  const cursor = {[$$bc$property_key($$bc$keyword("x"))]: SUBVIEW_TAB_ORIGIN, [$$bc$property_key($$bc$keyword("id"))]: ""};
  views.forEach((view) => { const start = cursor.x;
const title_width = workview_title(view).length;
const width = (title_width + 2);
if (((_truthy) => _truthy !== false && _truthy != null)(((column >= start) && (column < (start + width))))) {
  (cursor.id = workview_id(view));
}
return (cursor.x = (start + width + SUBVIEW_TAB_GAP)); });
  return text(cursor.id);
}

function view_tab_id_at_bang(view, views, column) {
  const agents_tab_width = AGENTS_TAB_LABEL.length;
  const threads_tab_width = THREADS_TAB_LABEL.length;
  return ((((_truthy) => _truthy !== false && _truthy != null)(((column >= 0) && (column < agents_tab_width)))) ? "agents" : (((_truthy) => _truthy !== false && _truthy != null)(((column >= THREADS_TAB_START) && (column < (THREADS_TAB_START + threads_tab_width))))) ? "threads" : (((_truthy) => _truthy !== false && _truthy != null)((threads_view_p(view) && (column >= SUBVIEW_TAB_ORIGIN)))) ? subview_tab_id_at_bang(views, column) : "");
}

function view_tab_at_bang(runtime, tabs, event, views) {
  const event_x = event.x;
  const tabs_x = tabs.x;
  const column = Math.floor((event_x - tabs_x));
  return view_tab_id_at_bang(runtime.view, views, column);
}

function complete_clicked_palette_bang(runtime, ui, view, palette_renderable, event) {
  if ((event.button === 0)) {
    select_view_bang(runtime, view);
    const input = ui.composerInput;
    const options = palette_options(view, text(input.value));
    const event_y = event.y;
    const palette_y = palette_renderable.y;
    const row = Math.floor((event_y - palette_y));
    const scrolled = runtime.paletteStart;
    const start = (((_truthy) => _truthy !== false && _truthy != null)(scrolled) ? scrolled : 0);
    const drawn = runtime.paletteRows;
    const rows = (((_truthy) => _truthy !== false && _truthy != null)(drawn) ? drawn : 0);
    const picked = (start + row);
    const option_count = options.length;
    if (((_truthy) => _truthy !== false && _truthy != null)(((row >= 0) && ((row < rows) && (picked < option_count))))) {
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
  if (((_truthy) => _truthy !== false && _truthy != null)(((indices.length > 0) && (!((_truthy) => _truthy !== false && _truthy != null)(indices.includes(current)))))) {
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
      const event_y = event.y;
      const list_y = ui.workText.screenY;
      const row_index = Math.floor((event_y - list_y));
      const row_count = rows.length;
      if (((_truthy) => _truthy !== false && _truthy != null)(((row_index >= 0) && (row_index < row_count)))) {
        const row = rows[row_index];
        event.preventDefault();
        event.stopPropagation();
        if ((listrow_kind(row) === "header")) {
          const condition = listrow_condition(row);
          const collapsed = runtime.collapsedListConditions;
          if (((_truthy) => _truthy !== false && _truthy != null)(collapsed.has(condition))) {
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
  const event_x = event.x;
  const strip_x = ui.agentStripText.screenX;
  const column = Math.floor((event_x - strip_x));
  const index = segment_at_column(segments, column);
  if ((index >= 0)) {
    show_segment_detail_bang(runtime, index);
  }
  return focus_strip_bang(runtime, ui);
} });
  (ui.composerPalette.onMouseDown = (event) => complete_clicked_palette_bang(runtime, ui, text(runtime.view), ui.composerPalette, event));
  (ui.workText.onMouseDown = (event) => handle_list_click_bang(runtime, ui, event));
  return (ui.viewTabsText.onMouseDown = (event) => { if ((event.button === 0)) {
  const tab = view_tab_at_bang(runtime, ui.viewTabsText, event, view_list(snapshot(runtime.model)));
  if ((!(tab === ""))) {
    event.preventDefault();
    event.stopPropagation();
    return ((tab === "agents") ? show_view_bang(runtime, ui, "agents") : ((tab === "threads") ? show_view_bang(runtime, ui, "threads") : show_thread_view_bang(runtime, ui, tab)));
  }
} });
}

function escape_step_bang(runtime, ui) {
  const palette = active_palette_options(runtime, ui);
  const action = escape_rung((palette.length > 0), panel_filtering_p(runtime), detail_open_p(runtime), (((_truthy) => _truthy !== false && _truthy != null)(runtime.stripFocused) ? true : false), threads_view_p(runtime.view), (((_truthy) => _truthy !== false && _truthy != null)(runtime.working) ? true : false));
  return (((action === "close-palette")) ? (() => { (active_input(runtime, ui).value = "");
render_minibuffer_bang(runtime, ui);
return true; })() : ((action === "clear-filter")) ? (() => { clear_panel_filter_bang(runtime);
runtime.render();
return true; })() : ((action === "close-detail")) ? (() => { close_detail_bang(runtime);
focus_composer_bang(runtime, ui);
runtime.render();
return true; })() : ((action === "focus-composer")) ? (() => { leave_strip_bang(runtime, ui);
runtime.render();
return true; })() : ((action === "show-agents")) ? (() => { show_view_bang(runtime, ui, "agents");
return true; })() : ((action === "cancel-turn")) ? (() => { const target = text(runtime.supervisorId); if ((!(target === ""))) {
  report_promise_bang(runtime, cancel_turn_bang(runtime, ui, target));
}
return true; })() : false);
}

function panel_filterable_p(runtime) {
  return (detail_showing_p(runtime, "config") || detail_showing_p(runtime, "help"));
}

function filter_character(name, sequence, ctrl_p, meta_p) {
  return (((_truthy) => _truthy !== false && _truthy != null)((ctrl_p || (meta_p || ((name === "space") || ((!(sequence.length === 1)) || (sequence.charCodeAt(0) < 32)))))) ? "" : sequence);
}

function filter_key_action(filtering_p, query, name, character) {
  return ((((_truthy) => _truthy !== false && _truthy != null)(((!filtering_p) && (character === "/")))) ? "open" : ((!filtering_p)) ? "" : ((name === "backspace")) ? ((query === "") ? "close" : "erase") : ((!(character === ""))) ? "type" : "");
}

function ctrl_down_key_p(name, key) {
  return ((_logical) => (_logical !== false && _logical != null ? (name === "j") : _logical))(key.ctrl);
}

function bare_letter_p(name, key, letter) {
  return ((name === letter) && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && (!((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : key.option))(key.meta)))));
}

function fold_key_p(runtime, name, key) {
  return ((name === "left") || ((name === "right") || ((!panel_filtering_p(runtime)) && (bare_letter_p(name, key, "h") || bare_letter_p(name, key, "l")))));
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

function tab_fold_step_bang(runtime) {
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
  return ((_logical) => (_logical !== false && _logical != null ? (name === "k") : _logical))(key.ctrl);
}

function bare_key_p(name, key, letter) {
  return ((name === letter) && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && (!((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : key.option))(key.meta)))));
}

function strip_key_p(name, key) {
  return ((name === "left") || ((name === "right") || ((name === "up") || ((name === "down") || (submit_key_p(name) || (ctrl_down_key_p(name, key) || (ctrl_up_key_p(name, key) || (bare_key_p(name, key, "h") || (bare_key_p(name, key, "l") || (bare_key_p(name, key, "j") || bare_key_p(name, key, "k")))))))))));
}

function install_keys_bang(runtime, ui) {
  return runtime.renderer.keyInput.on("keypress", (key) => { if (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(key.defaultPrevented)) && (!((_truthy) => _truthy !== false && _truthy != null)(key.propagationStopped))))) {
  const name = text(key.name).toLowerCase();
  const meta = ((_logical) => (_logical !== false && _logical != null ? _logical : key.option))(key.meta);
  const palette = active_palette_options(runtime, ui);
  const palette_open = (palette.length > 0);
  const plain_view_arrow = (threads_view_p(runtime.view) && ((text(ui.composerInput.value).trim() === "") && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && ((!((_truthy) => _truthy !== false && _truthy != null)(meta)) && ((name === "left") || (name === "right"))))));
  return ((((_truthy) => _truthy !== false && _truthy != null)(((name === "escape") || (name === "esc")))) ? (escape_step_bang(runtime, ui) ? (() => { key.preventDefault();
return key.stopPropagation(); })() : null) : (((_truthy) => _truthy !== false && _truthy != null)((detail_open_p(runtime) && ((!panel_focused_p(runtime)) && ((!palette_open) && ctrl_down_key_p(name, key)))))) ? (() => { key.preventDefault();
key.stopPropagation();
focus_panel_bang(runtime, ui);
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)((detail_showing_p(runtime, "config") && (panel_focused_p(runtime) && ((!palette_open) && ((name === "up") || ((name === "down") || (ctrl_up_key_p(name, key) || (ctrl_down_key_p(name, key) || ((name === "space") || (submit_key_p(name) || ((name === "tab") || fold_key_p(runtime, name, key))))))))))))) ? (() => { const up_p = ((name === "up") || ctrl_up_key_p(name, key)); const down_p = ((name === "down") || ctrl_down_key_p(name, key)); key.preventDefault();
key.stopPropagation();
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : down_p))(up_p))) {
  const total = config_panel_rows(runtime).length;
  if ((total > 0)) {
    const raw = runtime.configIndex;
    const current = (((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0);
    const delta = (((_truthy) => _truthy !== false && _truthy != null)(up_p) ? -1 : 1);
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
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)((detail_showing_p(runtime, "config") && (panel_focused_p(runtime) && ((!panel_filtering_p(runtime)) && ((!palette_open) && (filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false)) === "@"))))))) ? (() => { const rows = config_panel_rows(runtime); const total = rows.length; key.preventDefault();
key.stopPropagation();
if ((total > 0)) {
  const entry = rows[clamped_index(runtime.configIndex, total)];
  const input = active_input(runtime, ui);
  (input.value = $$bc$str(text(input.value), config_reference_text(configentry_kind(entry), configentry_name(entry))));
  focus_composer_bang(runtime, ui);
  render_minibuffer_bang(runtime, ui);
}
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)((panel_filterable_p(runtime) && (panel_focused_p(runtime) && ((!palette_open) && (!(filter_key_action(panel_filtering_p(runtime), panel_query(runtime), name, filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false))) === ""))))))) ? (() => { const character = filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false)); const query = panel_query(runtime); const action = filter_key_action(panel_filtering_p(runtime), query, name, character); key.preventDefault();
key.stopPropagation();
if ((action === "open")) {
  set_panel_query_bang(runtime, "");
} else if ((action === "type")) {
  set_panel_query_bang(runtime, $$bc$str(query, character));
} else if ((action === "erase")) {
  const query_length = query.length;
  set_panel_query_bang(runtime, query.slice(0, (query_length - 1)));
} else {
  clear_panel_filter_bang(runtime);
}
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)((panel_focused_p(runtime) && ((!palette_open) && (!(filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false)) === "")))))) ? (() => { key.preventDefault();
return key.stopPropagation(); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? strip_key_p(name, key) : _logical))(runtime.stripFocused))) ? (() => { const expanded_p = detail_showing_p(runtime, "agents"); const up_p = ((name === "up") || (ctrl_up_key_p(name, key) || bare_key_p(name, key, "k"))); const down_p = ((name === "down") || (ctrl_down_key_p(name, key) || bare_key_p(name, key, "j"))); const left_p = ((name === "left") || bare_key_p(name, key, "h")); const right_p = ((name === "right") || bare_key_p(name, key, "l")); key.preventDefault();
key.stopPropagation();
if (submit_key_p(name)) {
  toggle_segment_detail_bang(runtime);
} else if (((_truthy) => _truthy !== false && _truthy != null)(left_p)) {
  move_strip_segment_bang(runtime, -1);
} else if (((_truthy) => _truthy !== false && _truthy != null)(right_p)) {
  move_strip_segment_bang(runtime, 1);
} else if (((_truthy) => _truthy !== false && _truthy != null)(up_p)) {
  if (expanded_p) {
    move_detail_cursor_bang(runtime, -1);
  } else {
    leave_strip_bang(runtime, ui);
  }
} else if (((_truthy) => _truthy !== false && _truthy != null)(down_p)) {
  if (expanded_p) {
    move_detail_cursor_bang(runtime, 1);
  } else {
    toggle_segment_detail_bang(runtime);
  }
} else {
  null;
}
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)((palette_open && ((name === "up") || ((name === "down") || ((_logical) => (_logical !== false && _logical != null ? ((name === "j") || (name === "k")) : _logical))(key.ctrl)))))) ? (() => { key.preventDefault();
key.stopPropagation();
const palette_index = runtime.paletteIndex;
const palette_delta = (((_truthy) => _truthy !== false && _truthy != null)(((name === "up") || ((_logical) => (_logical !== false && _logical != null ? (name === "k") : _logical))(key.ctrl))) ? -1 : 1);
const palette_count = palette.length;
(runtime.paletteIndex = ((palette_index + palette_delta + palette_count) % palette_count));
active_input(runtime, ui).focus();
return render_minibuffer_bang(runtime, ui); })() : (((_truthy) => _truthy !== false && _truthy != null)((palette_open && (name === "tab")))) ? (() => { key.preventDefault();
key.stopPropagation();
return complete_palette_bang(runtime, ui, palette); })() : (((_truthy) => _truthy !== false && _truthy != null)((palette_open && submit_key_p(name)))) ? (() => { const candidate = palette_candidate(runtime, palette); const completed = palette_completion_text(candidate); const current = text(active_input(runtime, ui).value); const action = palette_enter_action(palette.length, slashcommand_arguments(candidate), slashcommand_emoji(candidate), (current === completed)); key.preventDefault();
key.stopPropagation();
return ((action === "fire") ? submit_input_bang(runtime, ui, completed.trim()) : complete_palette_bang(runtime, ui, palette)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((!((_truthy) => _truthy !== false && _truthy != null)(runtime.stripFocused)) && ((!palette_open) && (ctrl_down_key_p(name, key) || ((name === "down") && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && ((!((_truthy) => _truthy !== false && _truthy != null)(meta)) && (text(ui.composerInput.value).trim() === ""))))))))) ? (() => { key.preventDefault();
key.stopPropagation();
return focus_strip_bang(runtime, ui); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "tab") || (name === "f2")))) ? (() => { key.preventDefault();
key.stopPropagation();
return show_view_bang(runtime, ui, tab_swap_view(text(runtime.view))); })() : ((name === "f1")) ? (() => { key.preventDefault();
key.stopPropagation();
return toggle_help_bang(runtime, ui); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "f3") || ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? ((name === "h") || (name === "l")) : _logical))(meta)))(plain_view_arrow)))) ? (() => { const state = snapshot(runtime.model); const views = view_list(state); const current = selected_view(state, runtime.activeView); const index = views.findIndex((view) => (workview_id(view) === workview_id(current))); const delta = (((_truthy) => _truthy !== false && _truthy != null)(((name === "left") || ((_logical) => (_logical !== false && _logical != null ? (name === "h") : _logical))(meta))) ? -1 : 1); const view_count = views.length; const next_index = ((index + delta + view_count) % view_count); const next_id = text(views[next_index].id); key.preventDefault();
key.stopPropagation();
return show_thread_view_bang(runtime, ui, next_id); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "f5") || ((_logical) => (_logical !== false && _logical != null ? (name === "r") : _logical))(key.ctrl)))) ? (() => { key.preventDefault();
key.stopPropagation();
return report_promise_bang(runtime, refresh_bang(runtime)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "f6") || ((_logical) => (_logical !== false && _logical != null ? (name === "o") : _logical))(key.ctrl)))) ? (() => { key.preventDefault();
key.stopPropagation();
return popout_bang(runtime, runtime.activeView); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((name === "j") || (name === "k")) : _logical))(meta))) ? (() => { const state = snapshot(runtime.model); const delta = ((name === "k") ? -1 : 1); key.preventDefault();
key.stopPropagation();
if ((!threads_view_p(runtime.view))) {
  const agents = bridgesnapshot_agents(state);
  const agent_count = agents.length;
  const max_index = Math.max(0, (agent_count - 1));
  const agent_index = runtime.agentIndex;
  const next_index = Math.max(0, Math.min(max_index, (agent_index + delta)));
  const selected_agent_id = ((agent_count > 0) ? agent_id(agents[next_index]) : "");
  (runtime.agentIndex = next_index);
  (runtime.model = select_agent(runtime.model, selected_agent_id));
} else {
  const view = selected_view(state, runtime.activeView);
  const items = workview_items(view);
  const item_count = items.length;
  const max_index = Math.max(0, (item_count - 1));
  const work_index = runtime.workIndex;
  const next_index = ((workview_id(view) === "list") ? next_visible_list_index(runtime, items, work_index, delta) : Math.max(0, Math.min(max_index, (work_index + delta))));
  const thread_id = ((item_count > 0) ? workitem_id(items[next_index]) : "");
  (runtime.workIndex = next_index);
  (runtime.model = select_thread(runtime.model, thread_id));
  ui.workScroll.scrollBy((delta * (((workview_id(view) === "board")) ? 2 : ((workview_id(view) === "graph")) ? 3 : 1)), "step");
}
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "c") : _logical))(key.ctrl))) ? (() => { const target = text(runtime.supervisorId); key.preventDefault();
key.stopPropagation();
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(target === "")) : _logical))(runtime.working))) {
  return report_promise_bang(runtime, cancel_turn_bang(runtime, ui, target));
} })() : null);
} });
}

async function open_app_bang(view_id, source_identity) {
  const view = canonical_work_view(view_id);
  const renderer_promise = createCliRenderer({[$$bc$property_key($$bc$keyword("exitOnCtrlC"))]: false, [$$bc$property_key($$bc$keyword("clearOnShutdown"))]: true});
  const renderer = await renderer_promise;
  const runtime = {[$$bc$property_key($$bc$keyword("model"))]: make_model(view), [$$bc$property_key($$bc$keyword("renderer"))]: renderer, [$$bc$property_key($$bc$keyword("disposed"))]: false, [$$bc$property_key($$bc$keyword("rendererSuspended"))]: false, [$$bc$property_key($$bc$keyword("suspendResume"))]: null, [$$bc$property_key($$bc$keyword("suspendError"))]: "", [$$bc$property_key($$bc$keyword("view"))]: BOOT_VIEW, [$$bc$property_key($$bc$keyword("activeView"))]: view, [$$bc$property_key($$bc$keyword("agentIndex"))]: 0, [$$bc$property_key($$bc$keyword("workIndex"))]: 0, [$$bc$property_key($$bc$keyword("collapsedListConditions"))]: new Set(["blocked", "dormant", "draft", "terminal", "other"]), [$$bc$property_key($$bc$keyword("workScroll"))]: null, [$$bc$property_key($$bc$keyword("boardSignature"))]: "", [$$bc$property_key($$bc$keyword("dragThreadId"))]: "", [$$bc$property_key($$bc$keyword("bridgeExecutions"))]: new Set(), [$$bc$property_key($$bc$keyword("supervisorId"))]: "", [$$bc$property_key($$bc$keyword("conversation"))]: [], [$$bc$property_key($$bc$keyword("transcriptView"))]: "selected", [$$bc$property_key($$bc$keyword("itemSequence"))]: 0, [$$bc$property_key($$bc$keyword("lastAssistantText"))]: "", [$$bc$property_key($$bc$keyword("lastSubmitted"))]: "", [$$bc$property_key($$bc$keyword("working"))]: false, [$$bc$property_key($$bc$keyword("workingExecutions"))]: new Set(), [$$bc$property_key($$bc$keyword("workingLabel"))]: "", [$$bc$property_key($$bc$keyword("workingSince"))]: 0, [$$bc$property_key($$bc$keyword("spinnerIndex"))]: 0, [$$bc$property_key($$bc$keyword("spinnerTimer"))]: null, [$$bc$property_key($$bc$keyword("stripFocused"))]: false, [$$bc$property_key($$bc$keyword("stripIndex"))]: 0, [$$bc$property_key($$bc$keyword("detailView"))]: "", [$$bc$property_key($$bc$keyword("detailSegment"))]: "all", [$$bc$property_key($$bc$keyword("detailIndex"))]: 0, [$$bc$property_key($$bc$keyword("paletteIndex"))]: 0, [$$bc$property_key($$bc$keyword("paletteStart"))]: 0, [$$bc$property_key($$bc$keyword("paletteRows"))]: 0, [$$bc$property_key($$bc$keyword("promptGlyph"))]: DEFAULT_PROMPT_GLYPH, [$$bc$property_key($$bc$keyword("soundEnabled"))]: sound_enabled_from_env(text(process.env.NORTH_BRIDGE_SOUND)), [$$bc$property_key($$bc$keyword("soundPack"))]: sound_pack_from_env(text(process.env.NORTH_BRIDGE_SOUND_PACK)), [$$bc$property_key($$bc$keyword("soundDirectory"))]: sound_directory_from_env(text(process.env.NORTH_BRIDGE_SOUND_DIR)), [$$bc$property_key($$bc$keyword("soundPlayer"))]: discover_sound_player(), [$$bc$property_key($$bc$keyword("soundChildren"))]: new Set(), [$$bc$property_key($$bc$keyword("soundWarningShown"))]: false, [$$bc$property_key($$bc$keyword("soundSequence"))]: 0, [$$bc$property_key($$bc$keyword("lastSoundPath"))]: "", [$$bc$property_key($$bc$keyword("lastSoundAt"))]: 0, [$$bc$property_key($$bc$keyword("workspaceNotice"))]: "", [$$bc$property_key($$bc$keyword("keymap"))]: null, [$$bc$property_key($$bc$keyword("sessionModel"))]: text_or(process.env.NORTH_BRIDGE_MODEL, text(process.env.AGENT_MODEL)), [$$bc$property_key($$bc$keyword("sessionEffort"))]: text(process.env.AGENT_REASONING), [$$bc$property_key($$bc$keyword("launchProvider"))]: text(process.env.NORTH_BRIDGE_PROVIDER), [$$bc$property_key($$bc$keyword("launchTier"))]: text(process.env.NORTH_BRIDGE_TIER), [$$bc$property_key($$bc$keyword("launchModel"))]: text(process.env.NORTH_BRIDGE_MODEL), [$$bc$property_key($$bc$keyword("launchEffort"))]: text(process.env.NORTH_BRIDGE_EFFORT), [$$bc$property_key($$bc$keyword("sessionCwd"))]: text(process.cwd()), [$$bc$property_key($$bc$keyword("sessionBranch"))]: "", [$$bc$property_key($$bc$keyword("sessionPermissions"))]: "", [$$bc$property_key($$bc$keyword("sourceIdentity"))]: source_identity, [$$bc$property_key($$bc$keyword("renderConversation"))]: () => null, [$$bc$property_key($$bc$keyword("render"))]: () => null};
  const root = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexDirection"))]: "column", [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("height"))]: "100%", [$$bc$property_key($$bc$keyword("gap"))]: 0, [$$bc$property_key($$bc$keyword("paddingTop"))]: 1, [$$bc$property_key($$bc$keyword("paddingBottom"))]: 0, [$$bc$property_key($$bc$keyword("paddingLeft"))]: 1, [$$bc$property_key($$bc$keyword("paddingRight"))]: 1, [$$bc$property_key($$bc$keyword("onSizeChange"))]: () => runtime.render()});
  const workspace = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexDirection"))]: "row", [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexGrow"))]: 1, [$$bc$property_key($$bc$keyword("gap"))]: 0});
  const view_tabs_text = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("height"))]: 1, [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("truncate"))]: true});
  const agents_pane = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexDirection"))]: "column", [$$bc$property_key($$bc$keyword("width"))]: "100%"});
  const work_pane = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexDirection"))]: "column", [$$bc$property_key($$bc$keyword("width"))]: "100%"});
  const agents_text = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("height"))]: 4, [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "word"});
  const transcript_scroll = new ScrollBoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexGrow"))]: 1, [$$bc$property_key($$bc$keyword("scrollY"))]: true, [$$bc$property_key($$bc$keyword("stickyScroll"))]: true, [$$bc$property_key($$bc$keyword("stickyStart"))]: "bottom", [$$bc$property_key($$bc$keyword("viewportCulling"))]: true, [$$bc$property_key($$bc$keyword("verticalScrollbarOptions"))]: {[$$bc$property_key($$bc$keyword("visible"))]: false}});
  const transcript_text_view = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "word"});
  const work_scroll = new ScrollBoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexGrow"))]: 1, [$$bc$property_key($$bc$keyword("scrollY"))]: true, [$$bc$property_key($$bc$keyword("viewportCulling"))]: true, [$$bc$property_key($$bc$keyword("verticalScrollbarOptions"))]: {[$$bc$property_key($$bc$keyword("visible"))]: false}});
  const work_text_view = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("truncate"))]: true});
  const board_root = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("visible"))]: false, [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexDirection"))]: "column", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("gap"))]: 1});
  const status_text = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "word"});
  const agent_status_text = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("visible"))]: false, [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "word"});
  const composer_palette = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("visible"))]: false, [$$bc$property_key($$bc$keyword("height"))]: 1, [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("truncate"))]: true, [$$bc$property_key($$bc$keyword("bg"))]: "#25272d"});
  const composer = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexDirection"))]: "row", [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("height"))]: 1, [$$bc$property_key($$bc$keyword("paddingLeft"))]: 1, [$$bc$property_key($$bc$keyword("paddingRight"))]: 1, [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("backgroundColor"))]: "#25272d"});
  const agent_strip_text = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("height"))]: 1, [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("truncate"))]: true});
  const detail_panel = new BoxRenderable(renderer, {[$$bc$property_key($$bc$keyword("visible"))]: false, [$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("height"))]: 5, [$$bc$property_key($$bc$keyword("flexDirection"))]: "column", [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("border"))]: true, [$$bc$property_key($$bc$keyword("borderColor"))]: "#64748b", [$$bc$property_key($$bc$keyword("paddingLeft"))]: 1, [$$bc$property_key($$bc$keyword("paddingRight"))]: 1});
  const detail_text = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: "100%", [$$bc$property_key($$bc$keyword("flexGrow"))]: 1, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none"});
  const composer_prompt = new TextRenderable(renderer, {[$$bc$property_key($$bc$keyword("width"))]: 2, [$$bc$property_key($$bc$keyword("height"))]: 1, [$$bc$property_key($$bc$keyword("flexShrink"))]: 0, [$$bc$property_key($$bc$keyword("wrapMode"))]: "none", [$$bc$property_key($$bc$keyword("content"))]: new StyledText([brightCyan("❯ ")])});
  const composer_input = new InputRenderable(renderer, {[$$bc$property_key($$bc$keyword("flexGrow"))]: 1, [$$bc$property_key($$bc$keyword("flexShrink"))]: 1, [$$bc$property_key($$bc$keyword("flexBasis"))]: 0, [$$bc$property_key($$bc$keyword("minWidth"))]: 0, [$$bc$property_key($$bc$keyword("backgroundColor"))]: "#25272d", [$$bc$property_key($$bc$keyword("focusedBackgroundColor"))]: "#25272d", [$$bc$property_key($$bc$keyword("textColor"))]: "#e5e7eb", [$$bc$property_key($$bc$keyword("focusedTextColor"))]: "#f8fafc", [$$bc$property_key($$bc$keyword("placeholderColor"))]: "#6b7280", [$$bc$property_key($$bc$keyword("placeholder"))]: composer_hint("agents", main_agent_label(runtime))});
  const ui = {[$$bc$property_key($$bc$keyword("root"))]: root, [$$bc$property_key($$bc$keyword("agentsPane"))]: agents_pane, [$$bc$property_key($$bc$keyword("workPane"))]: work_pane, [$$bc$property_key($$bc$keyword("viewTabsText"))]: view_tabs_text, [$$bc$property_key($$bc$keyword("agentsText"))]: agents_text, [$$bc$property_key($$bc$keyword("transcriptText"))]: transcript_text_view, [$$bc$property_key($$bc$keyword("workScroll"))]: work_scroll, [$$bc$property_key($$bc$keyword("workText"))]: work_text_view, [$$bc$property_key($$bc$keyword("boardRoot"))]: board_root, [$$bc$property_key($$bc$keyword("statusText"))]: status_text, [$$bc$property_key($$bc$keyword("agentStatusText"))]: agent_status_text, [$$bc$property_key($$bc$keyword("composerPalette"))]: composer_palette, [$$bc$property_key($$bc$keyword("composer"))]: composer, [$$bc$property_key($$bc$keyword("composerPrompt"))]: composer_prompt, [$$bc$property_key($$bc$keyword("composerInput"))]: composer_input, [$$bc$property_key($$bc$keyword("agentStripText"))]: agent_strip_text, [$$bc$property_key($$bc$keyword("detailPanel"))]: detail_panel, [$$bc$property_key($$bc$keyword("detailText"))]: detail_text};
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

function run_northbridge_app_bang(options) {
  return open_app_bang(text_or(options.viewId, "list"), text(options.sourceIdentity));
}

export { active_focus as "active-focus" };
export { agent_cell_text_bang as "agent-cell-text!" };
export { agent_field_text as "agent-field-text" };
export { agent_route_text_bang as "agent-route-text!" };
export { agent_row_text_bang as "agent-row-text!" };
export { apply_view_visibility_bang as "apply-view-visibility!" };
export { banner_box_bang as "banner-box!" };
export { banner_line_segments as "banner-line-segments" };
export { banner_permissions as "banner-permissions" };
export { banner_revision as "banner-revision" };
export { banner_rule_line_p as "banner-rule-line?" };
export { boot_view as "boot-view" };
export { clamp_panel_cursor_bang as "clamp-panel-cursor!" };
export { cleanup_suspend_bang as "cleanup-suspend!" };
export { clear_panel_filter_bang as "clear-panel-filter!" };
export { composer_hint as "composer-hint" };
export { config_cli_name as "config-cli-name" };
export { config_detail_lines_bang as "config-detail-lines!" };
export { config_empty_note as "config-empty-note" };
export { config_entry_active_p as "config-entry-active?" };
export { config_fold_rows as "config-fold-rows" };
export { config_gate_modules as "config-gate-modules" };
export { config_header_keys as "config-header-keys" };
export { config_header_roles as "config-header-roles" };
export { config_header_shared_bang as "config-header-shared!" };
export { config_kind_tag as "config-kind-tag" };
export { config_kind_word as "config-kind-word" };
export { config_membership_of_json as "config-membership-of-json" };
export { config_module_members as "config-module-members" };
export { config_node_expanded_p as "config-node-expanded?" };
export { config_panel_legend as "config-panel-legend" };
export { config_panel_rows as "config-panel-rows" };
export { config_provenance_name as "config-provenance-name" };
export { config_query_field as "config-query-field" };
export { config_query_rows as "config-query-rows" };
export { config_reference_text as "config-reference-text" };
export { config_row_context_only_p as "config-row-context-only?" };
export { config_row_depth as "config-row-depth" };
export { config_row_label as "config-row-label" };
export { config_row_matches_p as "config-row-matches?" };
export { config_row_node as "config-row-node" };
export { config_row_parts as "config-row-parts" };
export { config_row_role as "config-row-role" };
export { config_row_scope as "config-row-scope" };
export { config_row_search_text as "config-row-search-text" };
export { config_section_rows as "config-section-rows" };
export { config_section_title as "config-section-title" };
export { config_skill_hooks as "config-skill-hooks" };
export { config_state_text as "config-state-text" };
export { config_toggle_verb as "config-toggle-verb" };
export { config_unit_active_p as "config-unit-active?" };
export { config_view_folds_p as "config-view-folds?" };
export { config_view_includes_p as "config-view-includes?" };
export { config_view_rows as "config-view-rows" };
export { config_visible_count as "config-visible-count" };
export { detail_height_bang as "detail-height!" };
export { escape_rung as "escape-rung" };
export { filter_character as "filter-character" };
export { filter_key_action as "filter-key-action" };
export { fold_key_action as "fold-key-action" };
export { handle_local_command_bang as "handle-local-command!" };
export { help_query_rows as "help-query-rows" };
export { launch_route_flags as "launch-route-flags" };
export { load_config_memberships_bang as "load-config-memberships!" };
export { normalize_agents as "normalize-agents" };
export { palette_enter_action as "palette-enter-action" };
export { palette_options as "palette-options" };
export { parse_bridge_stream_bang as "parse-bridge-stream!" };
export { project_conversation as "project-conversation" };
export { quit_command_p as "quit-command?" };
export { reconcile_agent_selection_bang as "reconcile-agent-selection!" };
export { refresh_bang as "refresh!" };
export { render_config_panel_bang as "render-config-panel!" };
export { render_conversation_bang as "render-conversation!" };
export { render_detail_panel_bang as "render-detail-panel!" };
export { render_view_tabs_bang as "render-view-tabs!" };
export { restore_submitted_text_bang as "restore-submitted-text!" };
export { roster_row_suppressed_p as "roster-row-suppressed?" };
export { roster_text_bang as "roster-text!" };
export { roster_visible_rows as "roster-visible-rows" };
export { run_northbridge_app_bang as "run-northbridge-app!" };
export { selected_agent_id as "selected-agent-id" };
export { session_banner_bang as "session-banner!" };
export { session_banner_lines as "session-banner-lines" };
export { session_banner_runs as "session-banner-runs" };
export { set_launch_route_bang as "set-launch-route!" };
export { set_node_expanded_bang as "set-node-expanded!" };
export { set_panel_query_bang as "set-panel-query!" };
export { submit_input_bang as "submit-input!" };
export { suspend_runtime_bang as "suspend-runtime!" };
export { tab_action as "tab-action" };
export { tab_fold_step_bang as "tab-fold-step!" };
export { tab_swap_view as "tab-swap-view" };
export { take_launch_route_flags_bang as "take-launch-route-flags!" };
export { thread_view_command_p as "thread-view-command?" };
export { threads_view_p as "threads-view?" };
export { transcript_banner_p as "transcript-banner?" };
export { transcript_placeholder as "transcript-placeholder" };
export { view_list as "view-list" };
export { view_tab_id_at_bang as "view-tab-id-at!" };
