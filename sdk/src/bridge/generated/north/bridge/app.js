import { BoxRenderable, ScrollBoxRenderable, StyledText, bg, brightBlack, brightCyan, brightGreen, brightRed, brightWhite, brightYellow, createCliRenderer, dim, InputRenderable, InputRenderableEvents, red, stripAnsiSequences, TextRenderable, white } from '@opentui/core';
import { registerEmacsBindings, registerEscapeClearsPendingSequence } from '@opentui/keymap/addons';
import { createDefaultOpenTuiKeymap } from '@opentui/keymap/opentui';
import { "Agent" as Agent, "BridgeSnapshot" as BridgeSnapshot, "TrackedThing" as TrackedThing, "WorkItem" as WorkItem, "agent-effort" as agent_effort, "agent-goal" as agent_goal, "agent-id" as agent_id, "agent-model" as agent_model, "agent-model-display" as agent_model_display, "agent-name" as agent_name, "agent-orchestration-provenance" as agent_orchestration_provenance, "agent-provider" as agent_provider, "agent-provider-label" as agent_provider_label, "agent-provider-target" as agent_provider_target, "agent-state" as agent_state, "agent-status" as agent_status, "agent-task" as agent_task, "bridgesnapshot-active-view-id" as bridgesnapshot_active_view_id, "bridgesnapshot-agents" as bridgesnapshot_agents, "bridgesnapshot-all" as bridgesnapshot_all, "bridgesnapshot-goals" as bridgesnapshot_goals, "bridgesnapshot-board" as bridgesnapshot_board, "bridgesnapshot-list" as bridgesnapshot_list, "bridgesnapshot-notice" as bridgesnapshot_notice, "bridgesnapshot-semantic-agents" as bridgesnapshot_semantic_agents, "bridgesnapshot-selected-agent" as bridgesnapshot_selected_agent, "bridgesnapshot-selected-tracked-thing" as bridgesnapshot_selected_tracked_thing, "focus-view" as focus_view, "make-model" as make_model, "remove-agent" as remove_agent, "replace-catalog" as replace_catalog, "replace-projection" as replace_projection, "select-agent" as select_agent, "select-tracked-thing" as select_tracked_thing, "set-filter" as set_filter, "snapshot" as snapshot, "upsert-agent" as upsert_agent, "trackedthing-agent" as trackedthing_agent, "trackedthing-assignee" as trackedthing_assignee, "trackedthing-assignee-title" as trackedthing_assignee_title, "trackedthing-desired-outcome" as trackedthing_desired_outcome, "trackedthing-id" as trackedthing_id, "trackedthing-plan" as trackedthing_plan, "trackedthing-project" as trackedthing_project, "trackedthing-status" as trackedthing_status, "trackedthing-task" as trackedthing_task, "trackedthing-title" as trackedthing_title, "workitem-body" as workitem_body, "workitem-condition" as workitem_condition, "workitem-dependencies" as workitem_dependencies, "workitem-driver" as workitem_driver, "workitem-id" as workitem_id, "workitem-title" as workitem_title } from './model.js';
import { "referent-action-request!" as referent_action_request_bang, "run-referent-action!" as run_referent_action_bang, "semantic-action-result-text!" as semantic_action_result_text_bang } from './referent-actions.js';
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

const AGENT_COMMANDS = [SlashCommand("/launch", "start another Codex worker", true, "", false), SlashCommand("/delegate", "invoke North delegation", true, "", false), SlashCommand("/provider", "set next launch: openai or auto", true, "", false), SlashCommand("/model", "set next launch: tier, exact model, or auto", true, "", false), SlashCommand("/effort", "set next launch: low, medium, high, xhigh, max, or auto", true, "", false), SlashCommand("/interrupt", "interrupt the active agent turn", false, "", false), SlashCommand("/transcript", "show selected or all execution transcripts", true, "", false), SlashCommand("/goals", "show Goals", false, "", false), SlashCommand("/all", "show all tracked things", false, "", false), SlashCommand("/refresh", "refresh tracked things", false, "", false), SlashCommand("/restart", "retire the control daemon now", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/config", "toggle the switchboard", false, "", false), SlashCommand("/hooks", "switchboard: hooks only", false, "", false), SlashCommand("/skills", "switchboard: skills only", false, "", false), SlashCommand("/mcp", "share MCP: add <name> <url|-- command> | remove <name> | list", true, "", false), SlashCommand("/modules", "switchboard: recursive modules only", false, "", false), SlashCommand("/q", "quit Northbridge", false, "", false), SlashCommand("/help", "show Northbridge controls", false, "", false)];

const GOAL_COMMANDS = [SlashCommand("/track", "create a tracked thing", true, "", false), SlashCommand("/plan", "record an exact Plan revision", true, "", false), SlashCommand("/start", "authorize an exact Plan revision", true, "", false), SlashCommand("/assign", "record an immutable Assignment", true, "", false), SlashCommand("/request", "send an immutable Request", true, "", false), SlashCommand("/ack", "record receipt of one exact Request", true, "", false), SlashCommand("/ownership", "record a work-ownership-v1 transition", true, "", false), SlashCommand("/settle", "record a Settlement", true, "", false), SlashCommand("/delegate", "invoke North delegation", true, "", false), SlashCommand("/filter", "filter Goals", true, "", false), SlashCommand("/agents", "show Agents", false, "", false), SlashCommand("/all", "show all tracked things", false, "", false), SlashCommand("/refresh", "refresh tracked things", false, "", false), SlashCommand("/restart", "retire the control daemon now", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/config", "toggle the switchboard", false, "", false), SlashCommand("/hooks", "switchboard: hooks only", false, "", false), SlashCommand("/skills", "switchboard: skills only", false, "", false), SlashCommand("/mcp", "share MCP: add <name> <url|-- command> | remove <name> | list", true, "", false), SlashCommand("/modules", "switchboard: recursive modules only", false, "", false), SlashCommand("/q", "quit Northbridge", false, "", false), SlashCommand("/help", "show Northbridge controls", false, "", false)];

const ALL_COMMANDS = [SlashCommand("/filter", "search all tracked things", true, "", false), SlashCommand("/show", "show one tracked thing", true, "", false), SlashCommand("/history", "show occurrence history for one tracked thing", true, "", false), SlashCommand("/inbox", "show durable Requests for one recipient", true, "", false), SlashCommand("/agents", "show Agents", false, "", false), SlashCommand("/goals", "show Goals", false, "", false), SlashCommand("/refresh", "refresh tracked things", false, "", false), SlashCommand("/restart", "retire the control daemon now", false, "", false), SlashCommand("/popout", "open the current view in another terminal", true, "", false), SlashCommand("/glyph", "set the shared prompt glyph", true, "", false), SlashCommand("/emoji", "insert a curated emoji or glyph", true, "", false), SlashCommand("/sound", "configure completion sounds", true, "", false), SlashCommand("/mute", "turn completion sounds off", false, "", false), SlashCommand("/config", "toggle the switchboard", false, "", false), SlashCommand("/hooks", "switchboard: hooks only", false, "", false), SlashCommand("/skills", "switchboard: skills only", false, "", false), SlashCommand("/mcp", "share MCP: add <name> <url|-- command> | remove <name> | list", true, "", false), SlashCommand("/modules", "switchboard: recursive modules only", false, "", false), SlashCommand("/q", "quit Northbridge", false, "", false), SlashCommand("/help", "show Northbridge controls", false, "", false)];

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
        break;
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
return ((((code === 9) || ((code === 10) || (code === 13)))) ? " " : (((code < 32) || ((code >= 127) && (code <= 159)))) ? "" : character); }).join("");
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

const TOP_LEVEL_VIEWS = ["agents", "goals", "all"];

function top_level_view_p(view) {
  const candidate = text(view);
  return TOP_LEVEL_VIEWS.includes(candidate);
}

function tracked_thing_view_p(view) {
  const candidate = text(view);
  return ((candidate === "goals") || (candidate === "all"));
}

function escape_rung(palette_open_p, filtering_p, detail_open_p, strip_focused_p, tracked_things_p, working_p) {
  return ((palette_open_p) ? "close-palette" : (filtering_p) ? "clear-filter" : (detail_open_p) ? "close-detail" : (strip_focused_p) ? "focus-composer" : (tracked_things_p) ? "show-agents" : (working_p) ? "cancel-turn" : "");
}

function active_focus(palette_open_p, panel_open_p, panel_focused_p, filtering_p, strip_focused_p) {
  return ((palette_open_p) ? "palette" : ((panel_open_p && (panel_focused_p && filtering_p))) ? "filter" : ((panel_open_p && panel_focused_p)) ? "panel" : (strip_focused_p) ? "strip" : "composer");
}

function tab_swap_view(view) {
  return (((view === "agents")) ? "goals" : ((view === "goals")) ? "all" : "agents");
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
  const commands = (((view === "goals")) ? GOAL_COMMANDS : ((view === "all")) ? ALL_COMMANDS : AGENT_COMMANDS);
  return (((!((_truthy) => _truthy !== false && _truthy != null)(query.startsWith("/")))) ? [] : ((name === "emoji")) ? emoji_options(parsedcommand_rest(parsed)) : (((name === "glyph") || (name === "prompt"))) ? glyph_options(parsedcommand_rest(parsed)) : ((query.indexOf(" ") >= 0)) ? [] : commands.filter((candidate) => slashcommand_name(candidate).startsWith(query)));
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
        break;
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
  if (((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed)) && (!(line.trim() === "")))) {
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
  if (((glyph.trim() === "") || (!(grapheme_count(glyph) === 1)))) {
    (() => { throw new Error("glyph requires exactly one grapheme, or use /glyph reset"); })();
  }
  (runtime.promptGlyph = glyph);
  publish_line_bang(runtime, $$bc$str("prompt glyph set to ", glyph));
  return runtime.render();
}

function sound_enabled_from_env(value) {
  const normalized = value.trim().toLowerCase();
  return (!((normalized === "0") || ((normalized === "false") || ((normalized === "off") || (normalized === "no")))));
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
  if (((!((_truthy) => _truthy !== false && _truthy != null)(runtime.disposed)) && (!((_truthy) => _truthy !== false && _truthy != null)(runtime.soundWarningShown)))) {
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
        break;
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
        break;
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
    const index = (((count > 1) && (base_path === text(runtime.lastSoundPath))) ? ((base_index + 1) % count) : base_index);
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

function ConfigEntry(kind, name, state, detail, title, active, owner, members, supports, distributions, activationPaths) {
  return $$bc$record_value("north.bridge.app/ConfigEntry", {_tag: "ConfigEntry", kind, name, state, detail, title, active, owner, members, supports, distributions, activationPaths});
}

function configentry_kind(r) { return r.kind; }

function configentry_name(r) { return r.name; }

function configentry_state(r) { return r.state; }

function configentry_detail(r) { return r.detail; }

function configentry_title(r) { return r.title; }

function configentry_active(r) { return r.active; }

function configentry_owner(r) { return r.owner; }

function configentry_members(r) { return r.members; }

function configentry_supports(r) { return r.supports; }

function configentry_distributions(r) { return r.distributions; }

function configentry_activationPaths(r) { return r.activationPaths; }

function config_permission_on_p(state) {
  return (state === "on");
}

function config_entry_members(entry) {
  const members = entry.members;
  return (Array.isArray(members) ? members : []);
}

function config_reference_text(kind, name) {
  return $$bc$str("@", kind, ":", name, " ");
}

function config_find_entry(entries, name) {
  return entries.find((entry) => (configentry_name(entry) === name));
}

function config_entry_active_p(entry) {
  return (((_truthy) => _truthy !== false && _truthy != null)(entry.active) ? true : false);
}

function config_unit_active_p(entries, name) {
  const entry = config_find_entry(entries, name);
  return (((_truthy) => _truthy !== false && _truthy != null)(entry) ? config_entry_active_p(entry) : false);
}

function config_state_text(entry) {
  return (config_entry_active_p(entry) ? "on" : "off");
}

function config_toggle_verb(resolved) {
  return ((resolved === "on") ? "off" : "on");
}

function config_view_includes_p(view, kind, __name) {
  return (((view === "all")) ? true : (kind === view));
}

function config_row_role(entry, __rows) {
  return configentry_kind(entry);
}

function config_section_rank(role) {
  return (((role === "hook")) ? 0 : ((role === "module")) ? 1 : ((role === "skill")) ? 2 : 10);
}

function config_tree_rows(entries) {
  return entries.slice().sort((a, b) => { const section_order = (config_section_rank(config_row_role(a, entries)) - config_section_rank(config_row_role(b, entries)));
return ((!(section_order === 0)) ? section_order : compare_text(configentry_name(a), configentry_name(b))); });
}

function config_view_rows(entries, view) {
  return config_tree_rows(entries.filter((entry) => config_view_includes_p(view, configentry_kind(entry), configentry_name(entry))));
}

function config_row_search_text(entry) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  return $$bc$str(kind, " ", name, " ", configentry_title(entry), " ", configentry_detail(entry), " ", configentry_owner(entry)).toLowerCase();
}

function config_row_matches_p(entry, query) {
  const needle = query.trim().toLowerCase();
  return ((needle === "") || config_row_search_text(entry).includes(needle));
}

function config_query_rows(entries, query) {
  return ((query.trim() === "") ? entries : entries.filter((entry) => config_row_matches_p(entry, query)));
}

function config_row_context_only_p(__entry, __query) {
  return false;
}

const ACTIVATION_SCHEMA = "north.agent-activation/v1";

function config_activation_path_from(runtime, environment) {
  const state_root = text_or(text(environment.NORTH_AGENT_STATE_ROOT), $$bc$str(text(environment.HOME), "/.local/state/north/agents"));
  return text_or(text(runtime.configActivationPath), $$bc$str(state_root, "/current/activation.json"));
}

function config_activation_path(runtime) {
  return config_activation_path_from(runtime, process.env);
}

function config_array(value) {
  return (Array.isArray(value) ? value : []);
}

function config_owner_text(owner) {
  return (((typeof owner === "string")) ? text(owner) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (typeof owner === "object") : _logical))(owner))) ? (() => { const repo = text(owner.repo); const path = text(owner.path); return ((((!(repo === "")) && (!(path === "")))) ? $$bc$str(repo, ":", path) : ((!(repo === ""))) ? repo : path); })() : "");
}

function config_unit_entry(unit) {
  return ConfigEntry(text(unit.kind), text(unit.id), text_or(text(unit.permission), "default"), text(unit.triggerDescription), text_or(text(unit.title), text(unit.id)), (((_truthy) => _truthy !== false && _truthy != null)(unit.active) ? true : false), config_owner_text(unit.owner), config_array(unit.members), config_array(unit.supports), config_array(unit.distributions), config_array(unit.activationPaths));
}

function config_activation_of_json(content) {
  const parsed = JSON.parse(content);
  const schema = text(parsed.schema);
  const units = parsed.units;
  if ((!(schema === ACTIVATION_SCHEMA))) {
    (() => { throw new Error($$bc$str("unsupported activation schema: ", text_or(schema, "missing"))); })();
  }
  if ((!Array.isArray(units))) {
    (() => { throw new Error("activation generation has no ordered units"); })();
  }
  const invalid = units.find((unit) => { const kind = text(unit.kind);
return (!((kind === "module") || ((kind === "skill") || (kind === "hook")))); });
  if (((_truthy) => _truthy !== false && _truthy != null)(invalid)) {
    (() => { throw new Error($$bc$str("activation generation has invalid unit kind: ", text_or(text(invalid.kind), "missing"))); })();
  }
  return {[$$bc$property_key($$bc$keyword("schema"))]: schema, [$$bc$property_key($$bc$keyword("digest"))]: text(parsed.catalogDigest), [$$bc$property_key($$bc$keyword("generation"))]: text(parsed.generationId), [$$bc$property_key($$bc$keyword("units"))]: units.map(config_unit_entry)};
}

function panel_filtering_p(runtime) {
  return (((_truthy) => _truthy !== false && _truthy != null)(runtime.panelFiltering) ? true : false);
}

function panel_query(runtime) {
  return (panel_filtering_p(runtime) ? text(runtime.panelQuery) : "");
}

function config_panel_rows(runtime) {
  const stored = runtime.configEntries;
  const entries = (((_truthy) => _truthy !== false && _truthy != null)(stored) ? stored : []);
  const query = panel_query(runtime);
  return ((!(query.trim() === "")) ? config_query_rows(entries, query) : entries);
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

async function load_config_entries_bang(runtime) {
  await (async () => { try {
    const file = Bun.file(config_activation_path(runtime));
  const content = await file.text();
  const activation = config_activation_of_json(content);
  const all_entries = activation.units;
  const config_filter = text_or(text(runtime.configFilter), "all");
  const entries = config_view_rows(all_entries, config_filter);
  (runtime.configDiagnostic = "");
  (runtime.configGeneration = activation.generation);
  (runtime.configDigest = activation.digest);
  (runtime.configAllEntries = all_entries);
  return (runtime.configEntries = entries);
  } catch (_catch_4) {
    switch ($$bd$catch_dispatch(_catch_4, [Error])) {
      case 0: {
        const error = _catch_4;
        (runtime.configAllEntries = []);
        (runtime.configEntries = []);
        return (runtime.configDiagnostic = $$bc$str("activation unavailable — ", error_message(error)));
        break;
      }
    }
  } })();
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
    if ((configentry_kind(entry) === "module")) {
      return (runtime.configInspectId = ((text(runtime.configInspectId) === configentry_name(entry)) ? "" : configentry_name(entry)));
    }
  }
}

async function toggle_config_entry_bang(runtime) {
  const entries = config_panel_rows(runtime);
  if ((entries.length > 0)) {
    const entry_count = entries.length;
    const raw = runtime.configIndex;
    const index = Math.max(0, Math.min((((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0), (entry_count - 1)));
    const entry = entries[index];
    const verb = config_toggle_verb(config_state_text(entry));
    await run_command([north_bin(), "config", "agents", verb, configentry_name(entry)]);
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
return publish_line_bang(runtime, sound_status(runtime)); })() : (((_truthy) => _truthy !== false && _truthy != null)(request.startsWith("pack "))) ? (() => { const pack = request.slice(5).trim(); if ((!((pack === "peon") || (pack === "peasant")))) {
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
  if ((launch_thread_id(runtime, "supervisor") === "")) {
    return publish_line_bang(runtime, "control daemon replaced; select a tracked thing before /launch");
  } else {
    publish_line_bang(runtime, "control daemon replaced; session restored");
    return await launch_agent_bang(runtime, SUPERVISOR_BOOT_PROMPT, "supervisor");
  }
  } catch (_catch_5) {
    switch ($$bd$catch_dispatch(_catch_5, [Error])) {
      case 0: {
        const error = _catch_5;
        return append_error_bang(runtime, error_message(error));
        break;
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
    return ((((name === "glyph") || (name === "prompt"))) ? (() => { if ((rest.toLowerCase() === "reset")) {
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
return true; })() : ((name === "transcript")) ? (() => { const requested = rest.trim().toLowerCase(); if ((!((requested === "selected") || (requested === "all")))) {
  (() => { throw new Error("transcript requires selected or all"); })();
}
(runtime.transcriptView = requested);
runtime.render();
return true; })() : (((name === "provider") || ((name === "model") || (name === "effort")))) ? (() => { set_launch_route_bang(runtime, name, rest);
return true; })() : ((name === "config")) ? (() => { open_config_panel_bang(runtime, ui, "all");
return true; })() : ((name === "hooks")) ? (() => { open_config_panel_bang(runtime, ui, "hook");
return true; })() : ((name === "skills")) ? (() => { open_config_panel_bang(runtime, ui, "skill");
return true; })() : ((name === "mcp")) ? (() => { const parts = rest.trim().split(/\\s+/).filter((part) => (!(part === "")));
run_command([north_bin(), "config", "mcp"].concat(parts)).then((output) => publish_line_bang(runtime, output.trim())).catch((error) => publish_line_bang(runtime, $$bc$str("error: ", error_message(error))));
return true; })() : ((name === "modules")) ? (() => { open_config_panel_bang(runtime, ui, "module");
return true; })() : ((name === "restart")) ? (() => { restart_daemon_bang(runtime);
return true; })() : (((_truthy) => _truthy !== false && _truthy != null)(TOP_LEVEL_VIEWS.includes(name))) ? (() => { show_view_bang(runtime, ui, name);
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
  } catch (_catch_6) {
    switch ($$bd$catch_dispatch(_catch_6, [Error])) {
      case 0: {
        const error = _catch_6;
        return (runtime.suspendError = error_message(error));
        break;
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
  } catch (_catch_7) {
    switch ($$bd$catch_dispatch(_catch_7, [Error])) {
      case 0: {
        const error = _catch_7;
        (runtime.suspendError = error_message(error));
        cleanup_suspend_bang(runtime, process_api);
        return false;
        break;
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
  } catch (_catch_8) {
    switch ($$bd$catch_dispatch(_catch_8, [Error])) {
      case 0: {
        const __ = _catch_8;
        return null;
        break;
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

function selected_semantic_agent_id(state, selected) {
  const agents = bridgesnapshot_semantic_agents(state);
  const total = agents.length;
  return ((total > 0) ? trackedthing_id(agents[Math.max(0, Math.min(selected, (total - 1)))]) : "");
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
  const request = referent_action_request_bang("catalog", []);
  const catalog = await run_referent_action_bang(request, {[$$bc$property_key($$bc$keyword("northExecutable"))]: north_bin(), [$$bc$property_key($$bc$keyword("runCommand"))]: run_command});
  const state = snapshot(runtime.model);
  const selected_id = bridgesnapshot_selected_tracked_thing(state);
  const next_model = replace_catalog(runtime.model, catalog.trackedThings, catalog.storeSpace, catalog.storeVersion);
  const next_state = snapshot(next_model);
  const view = text(runtime.view);
  const next_items = (((view === "agents")) ? bridgesnapshot_semantic_agents(next_state) : ((view === "goals")) ? bridgesnapshot_goals(next_state) : bridgesnapshot_all(next_state));
  const next_index = next_items.findIndex((item) => (trackedthing_id(item) === selected_id));
  (runtime.model = next_model);
  (runtime.workIndex = ((next_index >= 0) ? next_index : 0));
  return runtime.render();
}

function canonical_top_level_view(view_id) {
  return (top_level_view_p(view_id) ? view_id : BOOT_VIEW);
}

function view_list(state) {
  return [WorkView("agents", "Agents", bridgesnapshot_semantic_agents(state)), WorkView("goals", "Goals", bridgesnapshot_goals(state)), WorkView("all", "All", bridgesnapshot_all(state))];
}

function selected_view(state, view_id) {
  const views = view_list(state);
  const canonical = canonical_top_level_view(view_id);
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
  return (((!(label === ""))) ? label : (((!(provider === "")) && (!(target === "")))) ? $$bc$str(provider, ":", target) : text_or(provider, target));
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
  return (((value === "")) ? "other" : (((_truthy) => _truthy !== false && _truthy != null)(value.startsWith("finished("))) ? (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : value.includes("process:error")))(value.includes("process:failed"))) ? "failed" : "other") : (((_truthy) => _truthy !== false && _truthy != null)(value.startsWith("inconsistent"))) ? "blocked" : (((value === "stalled") || ((value === "blocked") || ((value === "waiting") || ((value === "paused") || (value === "queued")))))) ? "blocked" : (((value === "failed") || ((value === "error") || (value === "crashed")))) ? "failed" : (((value === "working") || ((value === "running") || ((value === "starting") || ((value === "ready") || ((value === "active") || (value === "online"))))))) ? "running" : "other");
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
  const agents_p = (text(runtime.view) === "agents");
  (ui.agentsPane.visible = agents_p);
  return (ui.workPane.visible = (!agents_p));
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
  return (((item_count > 0)) ? "" : (working_p) ? "" : (((value === "") || (value === "starting"))) ? $$bc$str("Starting ", label, "…") : (((value === "offline") || ((value === "failed") || (value === "error")))) ? $$bc$str(label, " is offline.") : "");
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
  return (((role === "module")) ? "MODULES" : ((role === "skill")) ? "SKILLS" : ((role === "hook")) ? "HOOKS" : "");
}

function config_header_roles(role) {
  return (((role === "module")) ? ["module"] : ((role === "skill")) ? ["skill"] : ((role === "hook")) ? ["hook"] : []);
}

function config_header_keys(entry, rows) {
  const role = config_row_role(entry, rows);
  return config_header_roles(role).map((heading) => heading);
}

function config_header_shared_bang(prior, current) {
  const count = {[$$bc$property_key($$bc$keyword("n"))]: 0};
  current.forEach((heading, index) => { if (((count.n === index) && ((index < prior.length) && (prior[index] === heading)))) {
  return (count.n = (index + 1));
} });
  return count.n;
}

function config_panel_title(config_filter) {
  return (((config_filter === "hook")) ? "hooks" : ((config_filter === "skill")) ? "skills" : ((config_filter === "module")) ? "modules" : "switchboard");
}

function config_empty_note(loaded_p, filtering_p) {
  return (((!loaded_p)) ? " loading…" : (filtering_p) ? " nothing matches" : " nothing to configure here");
}

function config_query_field(filtering_p, query) {
  return (filtering_p ? $$bc$str("  /", query) : "");
}

function config_panel_legend(filtering_p) {
  return (filtering_p ? "  ↑/↓ move · space toggle · enter inspect module · esc clears filter" : "  ↑/↓ move · space toggle · enter inspect module · / filter · esc close");
}

function dimmest(value) {
  return dim(brightBlack(value));
}

function config_member_count_text(count) {
  return $$bc$str(count, ((count === 1) ? " member" : " members"));
}

function config_row_parts(entry, state_text, width) {
  const kind = configentry_kind(entry);
  const name = configentry_name(entry);
  const members = config_entry_members(entry);
  const detail = (((kind === "hook")) ? (() => { const claims = config_array(entry.supports); return ((claims.length > 0) ? $$bc$str("supports ", claims.join(", ")) : ""); })() : ((kind === "module")) ? config_member_count_text(members.length) : text(configentry_detail(entry)));
  const indent = config_row_indent();
  const indent_width = indent.length;
  const state_width = state_text.length;
  const detail_width = detail.length;
  const room = Math.max(8, (width - indent_width - state_width - detail_width - 4));
  return {[$$bc$property_key($$bc$keyword("indent"))]: indent, [$$bc$property_key($$bc$keyword("name"))]: compact_text(name, room), [$$bc$property_key($$bc$keyword("state"))]: state_text, [$$bc$property_key($$bc$keyword("detail"))]: detail};
}

const CONFIG_INDENT_WIDTH = 2;

function config_row_indent() {
  return " ".repeat(CONFIG_INDENT_WIDTH);
}

function config_header_indent(index) {
  return " ".repeat((CONFIG_INDENT_WIDTH * (index + 1)));
}

function config_unit_parents(entries, id) {
  return entries.filter((entry) => ((configentry_kind(entry) === "module") && config_entry_members(entry).includes(id))).map((entry) => configentry_name(entry));
}

function config_path_text(path) {
  return (Array.isArray(path) ? path.map((id) => text(id)).join(" → ") : text(path));
}

function config_unit_summary(entry, entries, parent) {
  const id = configentry_name(entry);
  const parents = config_unit_parents(entries, id);
  const others = parents.filter((owner) => (!(owner === parent)));
  const supports = config_array(entry.supports);
  return $$bc$str(configentry_kind(entry), " ", id, ": ", (((_truthy) => _truthy !== false && _truthy != null)(entry.active) ? "on" : "off"), " · permission ", text_or(configentry_state(entry), "default"), ((text(entry.owner) === "") ? "" : $$bc$str(" · owner ", text(entry.owner))), ((supports.length > 0) ? $$bc$str(" · supports ", supports.join(", ")) : ""), ((others.length > 0) ? $$bc$str(" · also in ", others.join(", ")) : ""));
}

function config_inspection_walk_bang(lines, entries, entry, parent, depth, trail) {
  const id = configentry_name(entry);
  const indent = "  ".repeat(depth);
  const paths = config_array(entry.activationPaths);
  lines.push($$bc$str(indent, ((depth > 0) ? "└─ " : ""), config_unit_summary(entry, entries, parent)));
  paths.forEach((path) => lines.push($$bc$str(indent, "   activation: ", config_path_text(path))));
  if ((configentry_kind(entry) === "module")) {
    config_entry_members(entry).slice().sort((left, right) => compare_text(text(left), text(right))).forEach((member_id) => { const member = text(member_id);
const child = config_find_entry(entries, member);
return ((child == null) ? lines.push($$bc$str(indent, "  └─ missing unit ", member)) : (((_truthy) => _truthy !== false && _truthy != null)(trail.includes(member)) ? lines.push($$bc$str(indent, "  └─ cycle ", member)) : config_inspection_walk_bang(lines, entries, child, id, (depth + 1), trail.concat([member])))); });
  }
  return lines;
}

function config_module_inspection_text_bang(entries, id) {
  const entry = config_find_entry(entries, id);
  return (((entry == null) || (!(configentry_kind(entry) === "module"))) ? $$bc$str("module unavailable: ", id) : config_inspection_walk_bang([], entries, entry, "", 0, [id]).join("\n"));
}

function render_config_panel_bang(runtime) {
  const entries = config_panel_rows(runtime);
  const total = entries.length;
  const stored_entries = runtime.configEntries;
  const stored_all = runtime.configAllEntries;
  const all_entries = (((_truthy) => _truthy !== false && _truthy != null)(stored_all) ? stored_all : (((_truthy) => _truthy !== false && _truthy != null)(stored_entries) ? stored_entries : entries));
  const basis = (((_truthy) => _truthy !== false && _truthy != null)(stored_entries) ? stored_entries : entries);
  const config_filter = text_or(text(runtime.configFilter), "all");
  const filtering_p = panel_filtering_p(runtime);
  const focused_p = panel_focused_p(runtime);
  const query = panel_query(runtime);
  const inspect_id = text(runtime.configInspectId);
  return (((!(inspect_id === ""))) ? new StyledText([brightYellow("module inspection"), brightBlack("  enter returns\n"), brightWhite(config_module_inspection_text_bang(all_entries, inspect_id))]) : ((total === 0)) ? new StyledText([brightYellow(config_panel_title(config_filter)), brightCyan(config_query_field(filtering_p, query)), brightBlack(text_or(text(runtime.configDiagnostic), config_empty_note((((_truthy) => _truthy !== false && _truthy != null)(runtime.configLoaded) ? true : false), filtering_p)))]) : (() => { const index = clamped_index(runtime.configIndex, total); const window = config_visible_count(total, config_filter); const start = window_start(index, total, window); const stop = Math.min(total, (start + window)); const width = Math.max(12, (terminal_columns() - 12)); const parts = [brightYellow(config_panel_title(config_filter)), brightCyan(config_query_field(filtering_p, query)), brightBlack($$bc$str(config_panel_legend(filtering_p), "\n"))]; entries.slice(start, stop).forEach((entry, offset) => { const i = (start + offset);
const cursor_p = (i === index);
const kind = configentry_kind(entry);
const active_p = config_entry_active_p(entry);
const permission_off_p = (!config_permission_on_p(configentry_state(entry)));
const context_p = config_row_context_only_p(entry, query);
const role = config_row_role(entry, basis);
const state_text = config_state_text(entry);
const row = config_row_parts(entry, state_text, width);
const headings = config_header_keys(entry, basis);
const prior = ((i === start) ? [] : config_header_keys(entries[(i - 1)], basis));
const shared = config_header_shared_bang(prior, headings);
const tail = (((i + 1) === stop) ? "" : "\n");
config_header_roles(role).forEach((heading, at) => { if ((at >= shared)) {
  return parts.push(brightYellow($$bc$str(config_header_indent(at), config_section_title(heading), "\n")));
} });
parts.push(((cursor_p && focused_p) ? brightCyan("› ") : (cursor_p ? brightBlack("› ") : brightBlack("  "))));
const name_tone = ((permission_off_p) ? dimmest : ((cursor_p && focused_p)) ? brightWhite : (context_p) ? dimmest : brightBlack);
const state_tone = ((permission_off_p) ? dimmest : (active_p) ? brightGreen : brightBlack);
parts.push(name_tone(row.indent));
parts.push(name_tone(row.name));
parts.push(name_tone(": "));
parts.push(state_tone(row.state));
return parts.push(name_tone($$bc$str(((row.detail === "") ? "" : $$bc$str("  ", row.detail)), tail))); });
return new StyledText(parts); })());
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

const CONFIG_SECTION_ROWS = 3;

function detail_visible_count(total, extra) {
  return fitted_window(total, terminal_rows(), (CHROME_ROWS + MIN_WORKSPACE_ROWS + DETAIL_CHROME_ROWS + extra));
}

function config_section_rows(view) {
  return (((view === "all")) ? CONFIG_SECTION_ROWS : 1);
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
const tail = ((((i + 1) === stop) && (metadata === "")) ? "" : "\n");
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

const HELP_ROWS = [HelpRow("Tab", "cycle Agents / Goals / All"), HelpRow("←/→", "switch product route"), HelpRow("Ctrl-J / ↓", "into the agent strip, esc back out"), HelpRow("Esc /close /esc", "back or dismiss; cancels a turn at root"), HelpRow("Ctrl-C /interrupt", "cancel the turn; the message comes back"), HelpRow("/q /exit / Ctrl-Q", "quit Northbridge"), HelpRow("/help", "this panel"), HelpRow("/glyph <one>|reset", "prompt glyph"), HelpRow("/emoji <query>", "picker"), HelpRow("/sound on|off|pack", "voice lines"), HelpRow("/mute", "quiet")];

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

const GOALS_TAB_LABEL = "Goals";

const ALL_TAB_LABEL = "All";

const VIEW_TAB_SEPARATOR = " | ";

const VIEW_TAIL_SEPARATOR = " > ";

function push_session_identity_bang(chunks, session) {
  push_chunk_bang(chunks, brightYellow($$bc$str(text_or(session.sessionModel, "model pending"), " ", text_or(session.sessionEffort, "effort pending"))));
  push_chunk_bang(chunks, brightBlack(" · "));
  push_chunk_bang(chunks, brightGreen(text_or(short_directory(session.sessionCwd), "directory pending")));
  push_chunk_bang(chunks, brightBlack(" · "));
  return push_chunk_bang(chunks, dim(text_or(session.sessionBranch, "branch pending")));
}

function render_view_tabs_bang(view, state, __view_id, session) {
  const chunks = [];
  const active = canonical_top_level_view(text(view));
  push_chunk_bang(chunks, (((active === "agents") ? brightGreen : brightBlack))(AGENTS_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(VIEW_TAB_SEPARATOR));
  push_chunk_bang(chunks, (((active === "goals") ? brightGreen : brightBlack))(GOALS_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(VIEW_TAB_SEPARATOR));
  push_chunk_bang(chunks, (((active === "all") ? brightGreen : brightBlack))(ALL_TAB_LABEL));
  push_chunk_bang(chunks, brightBlack(VIEW_TAIL_SEPARATOR));
  if ((active === "agents")) {
    push_session_identity_bang(chunks, session);
  } else if ((active === "goals")) {
    push_chunk_bang(chunks, brightYellow("desired outcomes"));
  } else {
    push_chunk_bang(chunks, brightYellow("all tracked things"));
  }
  if (((active === "agents") && aggregate_transcript_p(session))) {
    push_chunk_bang(chunks, brightYellow(" · all transcripts"));
  }
  return new StyledText(chunks);
}

function tracked_thing_tags(item) {
  const tags = [];
  if (trackedthing_agent(item)) {
    tags.push("Agent");
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(trackedthing_desired_outcome(item))) {
    tags.push("Goal");
  }
  if (trackedthing_plan(item)) {
    tags.push("Plan");
  }
  if (trackedthing_project(item)) {
    tags.push("Project");
  }
  if (trackedthing_task(item)) {
    tags.push("Task");
  }
  if ((tags.length === 0)) {
    tags.push("Tracked");
  }
  return tags;
}

function selection_prefix(selected_p) {
  return (selected_p ? "› " : "  ");
}

function optional_clean_text(value) {
  return (((_truthy) => _truthy !== false && _truthy != null)(value) ? clean_text(value) : "");
}

function semantic_agent_row(item, selected_p) {
  const status = optional_clean_text(trackedthing_status(item));
  return $$bc$str(selection_prefix(selected_p), clean_text(trackedthing_title(item)), ((status === "") ? "" : $$bc$str(" (", status, ")")));
}

function semantic_goal_row(item, selected_p) {
  const assignee = optional_clean_text(trackedthing_assignee_title(item));
  const outcome = optional_clean_text(trackedthing_desired_outcome(item));
  return $$bc$str(selection_prefix(selected_p), ((assignee === "") ? "[unassigned]" : $$bc$str("[assigned: ", assignee, "]")), " ", clean_text(trackedthing_title(item)), " — ", outcome);
}

function semantic_all_row(item, selected_p) {
  const tags = tracked_thing_tags(item).join(" · ");
  const outcome = optional_clean_text(trackedthing_desired_outcome(item));
  const assignee = optional_clean_text(trackedthing_assignee_title(item));
  return $$bc$str(selection_prefix(selected_p), "[", tags, "] ", clean_text(trackedthing_title(item)), ((outcome === "") ? "" : $$bc$str(" — ", outcome)), (((!(assignee === ""))) ? $$bc$str(" · assigned to ", assignee) : ((!(outcome === ""))) ? " · unassigned" : ""));
}

function semantic_view_text_bang(state, view, selected, width) {
  const canonical = canonical_top_level_view(view);
  const items = (((canonical === "agents")) ? bridgesnapshot_semantic_agents(state) : ((canonical === "goals")) ? bridgesnapshot_goals(state) : bridgesnapshot_all(state));
  return ((items.length === 0) ? (((canonical === "agents")) ? "No Agents" : ((canonical === "goals")) ? "No Goals" : "No tracked things") : items.map((item, index) => { const row = (((canonical === "agents")) ? semantic_agent_row(item, (index === selected)) : ((canonical === "goals")) ? semantic_goal_row(item, (index === selected)) : semantic_all_row(item, (index === selected)));
return clipped(row, width); }).join("\n"));
}

function compact_text(value, width) {
  const source = text(value);
  const limit = Math.max(1, width);
  return ((source.length > limit) ? $$bc$str(source.slice(0, Math.max(0, (limit - 1))), "…") : source);
}

function composer_hint(pane, label) {
  return (((pane === "agents")) ? $$bc$str("Message ", label, "…") : ((pane === "goals")) ? "/track, /plan, /start, /assign, /request, /ack, /ownership, /settle" : "/filter, /show, /history, /inbox");
}

function minibuffer_placeholder(runtime) {
  return composer_hint(text(runtime.view), main_agent_label(runtime));
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
    const execution_agents = bridgesnapshot_agents(state);
    const views = view_list(state);
    const current = selected_view(state, text(runtime.view));
    const items = workview_items(current);
    const semantic_agents = bridgesnapshot_semantic_agents(state);
    const agent_max = Math.max(0, (semantic_agents.length - 1));
    const work_max = Math.max(0, (items.length - 1));
    const roster = semantic_view_text_bang(state, "agents", runtime.agentIndex, available_view_width());
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
    (ui.workText.visible = true);
    (ui.boardRoot.visible = false);
    (ui.workText.content = semantic_view_text_bang(state, workview_id(current), runtime.workIndex, available_view_width()));
    (ui.statusText.content = render_status(runtime, state));
    (ui.agentStatusText.content = render_status(runtime, state));
    (ui.statusText.visible = (tracked_thing_view_p(runtime.view) && notice_p));
    (ui.agentStatusText.visible = ((!tracked_thing_view_p(runtime.view)) && notice_p));
    render_prompt_bang(runtime, ui.composerPrompt);
    const segments = agent_segments(execution_agents);
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
  if (((!((_truthy) => _truthy !== false && _truthy != null)(line.startsWith("["))) || (close < 0))) {
    return null;
  } else {
    const rest = line.slice((close + 2));
    const space = rest.indexOf(" ");
    const kind = ((space < 0) ? rest : rest.slice(0, space));
    const payload = ((space < 0) ? "" : rest.slice((space + 1)).trim());
    return (() => { try {
    return ParsedRecord(Number(line.slice(1, close)), kind, ((payload === "") ? {} : JSON.parse(payload)));
  } catch (_catch_9) {
    switch ($$bd$catch_dispatch(_catch_9, [Error])) {
      case 0: {
        const __ = _catch_9;
        return null;
        break;
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
    const label = ((((!(provider === "")) && (!(tier === "")))) ? $$bc$str(provider, "/", tier) : ((!(provider === ""))) ? provider : "");
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
  return ((text(data.status) === "failed") && ((text(data.origin) === "north") && (text(data.errorCode) === "provider_session_replaced")));
}

function handle_wire_message_bang(runtime, stream_state, data) {
  const role = text(data.role);
  const stage = text(data.stage);
  const execution_id = text(stream_state.executionId);
  const id = event_item_id(execution_id, data.messageId);
  const body = clean_text(wire_content_text(data.content));
  const existing = conversation_item_by_id(runtime, id);
  if (((role === "assistant") && (!((_truthy) => _truthy !== false && _truthy != null)(stream_state.booting)))) {
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
if (((!((_truthy) => _truthy !== false && _truthy != null)(stream_state.booting)) && (!(prompt === "")))) {
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
set_execution_working_bang(runtime, execution_id, true, (((_truthy) => _truthy !== false && _truthy != null)(booting) ? $$bc$str("Starting ", main_agent_label(runtime), "…") : "Agent is working"));
if ((!(execution_id === ""))) {
  return bridge_agent_bang(runtime, execution_id, text(stream_state.role), (((_truthy) => _truthy !== false && _truthy != null)(booting) ? "starting" : "working"));
} })() : ((kind === "message.recorded")) ? handle_wire_message_bang(runtime, stream_state, data) : (((kind === "tool.admitted") || ((kind === "tool.progress") || (kind === "tool.terminal")))) ? handle_wire_tool_bang(runtime, stream_state, data, kind) : ((kind === "run.progress")) ? (() => { const progress = ((_logical) => (_logical !== false && _logical != null ? _logical : {}))(data.progress); const action = text(progress.currentAction); const lifecycle = text(data.lifecycle); adopt_wire_model_bang(runtime, progress.model, progress.effort);
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
  if ((!((provider_value === "") || (provider_value === "openai")))) {
    (() => { throw new Error("Bridge app launches require provider openai or auto"); })();
  }
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
    if ((!((choice === "auto") || (choice === "openai")))) {
      (() => { throw new Error("provider requires openai or auto"); })();
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
  return (((value === "openai")) ? "Codex Main" : "Main");
}

function launch_thread_id(runtime, role) {
  const state = snapshot(runtime.model);
  const selected = bridgesnapshot_selected_tracked_thing(state);
  return ((role === "supervisor") ? text(runtime.controlThreadId) : selected);
}

function bridge_app_launch_argv_bang(runtime, prompt, role) {
  const thread_id = launch_thread_id(runtime, role);
  if ((thread_id === "")) {
    (() => { throw new Error("launch requires a selected tracked thing or managed control identity"); })();
  }
  const route_flags = take_launch_route_flags_bang(runtime);
  return [north_bin(), "bridge", "app-launch", "--thread", thread_id, "--role", ((role === "supervisor") ? "director" : "implementer")].concat(route_flags, [prompt]);
}

async function launch_agent_bang(runtime, prompt, role) {
  if ((prompt.trim() === "")) {
    (() => { throw new Error("launch requires a prompt"); })();
  }
  set_working_bang(runtime, true, $$bc$str("Starting ", main_agent_label(runtime), "…"));
  const stream_state = {[$$bc$property_key($$bc$keyword("buffer"))]: "", [$$bc$property_key($$bc$keyword("stderr"))]: "", [$$bc$property_key($$bc$keyword("executionId"))]: "", [$$bc$property_key($$bc$keyword("role"))]: role, [$$bc$property_key($$bc$keyword("booting"))]: (role === "supervisor"), [$$bc$property_key($$bc$keyword("soundLive"))]: false};
  const exit_code = await stream_command(bridge_app_launch_argv_bang(runtime, prompt, role), (chunk) => parse_bridge_stream_bang(runtime, stream_state, chunk), (chunk) => (stream_state.stderr = clipped($$bc$str(stream_state.stderr, chunk), 6000)));
  if ((!(exit_code === 0))) {
    set_working_bang(runtime, false, "");
    return append_error_bang(runtime, $$bc$str("Bridge exited ", exit_code, ((text(stream_state.stderr).trim() === "") ? "" : $$bc$str("\n", text(stream_state.stderr).trim()))));
  }
}

function boot_bang(runtime, launch) {
  if ((!(launch_thread_id(runtime, "supervisor") === ""))) {
    report_promise_bang(runtime, launch(SUPERVISOR_BOOT_PROMPT, "supervisor"));
  }
  return runtime;
}

function popout_bang(runtime, view_id) {
  const ghostty = Bun.which("ghostty");
  const kitty = Bun.which("kitty");
  const wezterm = Bun.which("wezterm");
  const foot = Bun.which("foot");
  const xterm = Bun.which("xterm");
  const argv = ((((_truthy) => _truthy !== false && _truthy != null)(ghostty)) ? [ghostty, "-e", north_bin(), "bridge", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(kitty)) ? [kitty, "--detach", north_bin(), "bridge", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(wezterm)) ? [wezterm, "start", "--always-new-process", "--", north_bin(), "bridge", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(foot)) ? [foot, north_bin(), "bridge", "--view-id", view_id] : (((_truthy) => _truthy !== false && _truthy != null)(xterm)) ? [xterm, "-e", north_bin(), "bridge", "--view-id", view_id] : null);
  if ((argv == null)) {
    (() => { throw new Error("no supported terminal found for pop-out"); })();
  }
  const child = Bun.spawn({[$$bc$property_key($$bc$keyword("cmd"))]: argv, [$$bc$property_key($$bc$keyword("stdin"))]: "ignore", [$$bc$property_key($$bc$keyword("stdout"))]: "ignore", [$$bc$property_key($$bc$keyword("stderr"))]: "ignore"});
  child.unref();
  return publish_line_bang(runtime, $$bc$str("opened ", view_id, " in a separate terminal"));
}

const SEMANTIC_ACTION_NAMES = ["track", "plan", "start", "assign", "request", "ack", "ownership", "settle", "show", "history", "inbox"];

const SEMANTIC_MUTATION_NAMES = ["track", "plan", "start", "assign", "request", "ack", "ownership", "settle"];

function semantic_action_p(name) {
  return SEMANTIC_ACTION_NAMES.includes(name);
}

function action_arguments(value) {
  const trimmed = value.trim();
  return ((trimmed === "") ? [] : trimmed.split("|").map((part) => part.trim()));
}

async function run_semantic_action_bang(runtime, action, argument_text) {
  const request = referent_action_request_bang(action, action_arguments(argument_text));
  const result = await run_referent_action_bang(request, {[$$bc$property_key($$bc$keyword("northExecutable"))]: north_bin(), [$$bc$property_key($$bc$keyword("runCommand"))]: run_command});
  publish_line_bang(runtime, semantic_action_result_text_bang(request, result));
  if (((_truthy) => _truthy !== false && _truthy != null)(SEMANTIC_MUTATION_NAMES.includes(action))) {
    return await refresh_bang(runtime);
  }
}

function delegation_argv_bang(north_executable, argument_text) {
  const north = north_executable.trim();
  const arguments$ = action_arguments(argument_text);
  if (((north === "") || ((!(north === north_executable)) || (!((_truthy) => _truthy !== false && _truthy != null)(north.startsWith("/")))))) {
    (() => { throw new Error("delegate requires the absolute North executable"); })();
  }
  if ((arguments$.length === 0)) {
    (() => { throw new Error("delegate requires the exact North task and routing arguments"); })();
  }
  return [north, "delegate"].concat(arguments$);
}

async function run_delegation_bang(runtime, argument_text) {
  const argv = delegation_argv_bang(north_bin(), argument_text);
  return (async () => { try {
    await run_command(argv);
  return publish_line_bang(runtime, "delegation request accepted by North");
  } catch (_catch_10) {
    switch ($$bd$catch_dispatch(_catch_10, [Error])) {
      case 0: {
        const __ = _catch_10;
        return (() => { throw new Error("North delegation refused"); })();
        break;
      }
    }
  } })();
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
  return (handle_local_command_bang(runtime, ui, input) ? null : ((((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "launch") : _logical))(slash_p))) ? await launch_agent_bang(runtime, rest, "worker") : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "delegate") : _logical))(slash_p))) ? await run_delegation_bang(runtime, rest) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "refresh") : _logical))(slash_p))) ? await refresh_bang(runtime) : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "popout") : _logical))(slash_p))) ? popout_bang(runtime, text_or(rest, text(runtime.activeView))) : (async () => { if ((target === "")) {
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

async function submit_work_bang(runtime, ui, input) {
  const trimmed = input.trim();
  const slash_p = trimmed.startsWith("/");
  const parsed = command(input);
  const name = parsedcommand_name(parsed);
  const rest = parsedcommand_rest(parsed);
  return ((!((_truthy) => _truthy !== false && _truthy != null)(slash_p)) ? (() => { throw new Error("tracked-thing text requires an explicit /request or /delegate command"); })() : (handle_local_command_bang(runtime, ui, input) ? null : (((name === "filter")) ? (() => { (runtime.model = set_filter(runtime.model, rest));
return runtime.render(); })() : ((name === "refresh")) ? await refresh_bang(runtime) : ((name === "popout")) ? popout_bang(runtime, ((rest === "") ? runtime.activeView : rest)) : ((name === "delegate")) ? await run_delegation_bang(runtime, rest) : (semantic_action_p(name)) ? await run_semantic_action_bang(runtime, name, rest) : (() => { throw new Error("unknown tracked-thing command; use /help"); })())));
}

function report_promise_bang(runtime, promise) {
  return promise.catch((error) => publish_line_bang(runtime, $$bc$str("error: ", error_message(error))));
}

function select_view_bang(runtime, view) {
  (runtime.view = canonical_top_level_view(view));
  (runtime.paletteIndex = 0);
  (runtime.workspaceNotice = "");
  return clear_strip_focus_bang(runtime);
}

function show_view_bang(runtime, ui, view) {
  select_view_bang(runtime, view);
  ui.composerInput.focus();
  return runtime.render();
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
    if ((text(runtime.view) === "agents")) {
      const state = snapshot(runtime.model);
      const selected = selected_semantic_agent_id(state, runtime.agentIndex);
      return report_promise_bang(runtime, submit_agent_bang(runtime, ui, input, selected));
    } else {
      return report_promise_bang(runtime, submit_work_bang(runtime, ui, input));
    }
  }
}

function palette_enter_action(matches, takes_arguments_p, insert_only_p, completed_p) {
  return (((matches < 1)) ? "" : (completed_p) ? "fire" : ((insert_only_p || takes_arguments_p)) ? "complete" : "fire");
}

function install_input_bang(runtime, ui) {
  ui.composerInput.on(InputRenderableEvents.INPUT, (__value) => { (runtime.paletteIndex = 0);
return render_minibuffer_bang(runtime, ui); });
  return ui.composerInput.on(InputRenderableEvents.ENTER, () => submit_input_bang(runtime, ui, text(ui.composerInput.value).trim()));
}

function view_tab_id_at_bang(__view, __views, column) {
  return ((((column >= 0) && (column < 6))) ? "agents" : (((column >= 9) && (column < 14))) ? "goals" : (((column >= 17) && (column < 20))) ? "all" : "");
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
    if (((row >= 0) && ((row < rows) && (picked < option_count)))) {
      event.preventDefault();
      event.stopPropagation();
      (runtime.paletteIndex = picked);
      return complete_palette_bang(runtime, ui, options);
    }
  }
}

function handle_list_click_bang(runtime, ui, event) {
  if (((event.button === 0) && tracked_thing_view_p(runtime.view))) {
    const state = snapshot(runtime.model);
    const view = selected_view(state, text(runtime.view));
    const items = workview_items(view);
    const event_y = event.y;
    const list_y = ui.workText.screenY;
    const row_index = Math.floor((event_y - list_y));
    if (((row_index >= 0) && (row_index < items.length))) {
      const item = items[row_index];
      event.preventDefault();
      event.stopPropagation();
      (runtime.workIndex = row_index);
      (runtime.model = select_tracked_thing(runtime.model, trackedthing_id(item)));
      return runtime.render();
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
    return show_view_bang(runtime, ui, tab);
  }
} });
}

function escape_step_bang(runtime, ui) {
  const palette = active_palette_options(runtime, ui);
  const action = escape_rung((palette.length > 0), panel_filtering_p(runtime), detail_open_p(runtime), (((_truthy) => _truthy !== false && _truthy != null)(runtime.stripFocused) ? true : false), tracked_thing_view_p(runtime.view), (((_truthy) => _truthy !== false && _truthy != null)(runtime.working) ? true : false));
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
  return ((ctrl_p || (meta_p || ((name === "space") || ((!(sequence.length === 1)) || (sequence.charCodeAt(0) < 32))))) ? "" : sequence);
}

function filter_key_action(filtering_p, query, name, character) {
  return ((((!filtering_p) && (character === "/"))) ? "open" : ((!filtering_p)) ? "" : ((name === "backspace")) ? ((query === "") ? "close" : "erase") : ((!(character === ""))) ? "type" : "");
}

function ctrl_down_key_p(name, key) {
  return ((_logical) => (_logical !== false && _logical != null ? (name === "j") : _logical))(key.ctrl);
}

function bare_letter_p(name, key, letter) {
  return ((name === letter) && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && (!((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : key.option))(key.meta)))));
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
  return runtime.renderer.keyInput.on("keypress", (key) => { if (((!((_truthy) => _truthy !== false && _truthy != null)(key.defaultPrevented)) && (!((_truthy) => _truthy !== false && _truthy != null)(key.propagationStopped)))) {
  const name = text(key.name).toLowerCase();
  const meta = ((_logical) => (_logical !== false && _logical != null ? _logical : key.option))(key.meta);
  const palette = active_palette_options(runtime, ui);
  const palette_open = (palette.length > 0);
  const plain_view_arrow = (top_level_view_p(runtime.view) && ((text(ui.composerInput.value).trim() === "") && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && ((!((_truthy) => _truthy !== false && _truthy != null)(meta)) && ((name === "left") || (name === "right"))))));
  return ((((name === "escape") || (name === "esc"))) ? (escape_step_bang(runtime, ui) ? (() => { key.preventDefault();
return key.stopPropagation(); })() : null) : ((detail_open_p(runtime) && ((!panel_focused_p(runtime)) && ((!palette_open) && ctrl_down_key_p(name, key))))) ? (() => { key.preventDefault();
key.stopPropagation();
focus_panel_bang(runtime, ui);
return runtime.render(); })() : ((detail_showing_p(runtime, "agents") && (panel_focused_p(runtime) && ((!palette_open) && ctrl_up_key_p(name, key))))) ? (() => { key.preventDefault();
key.stopPropagation();
close_detail_bang(runtime);
focus_composer_bang(runtime, ui);
return runtime.render(); })() : ((detail_showing_p(runtime, "config") && (panel_focused_p(runtime) && ((!palette_open) && ((name === "up") || ((name === "down") || (ctrl_up_key_p(name, key) || (ctrl_down_key_p(name, key) || ((name === "space") || submit_key_p(name)))))))))) ? (() => { const up_p = ((name === "up") || ctrl_up_key_p(name, key)); const down_p = ((name === "down") || ctrl_down_key_p(name, key)); key.preventDefault();
key.stopPropagation();
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? _logical : down_p))(up_p))) {
  const total = config_panel_rows(runtime).length;
  if ((total > 0)) {
    const raw = runtime.configIndex;
    const current = (((_truthy) => _truthy !== false && _truthy != null)(raw) ? raw : 0);
    const delta = (((_truthy) => _truthy !== false && _truthy != null)(up_p) ? -1 : 1);
    (runtime.configIndex = ((current + delta + total) % total));
  }
} else if ((name === "space")) {
  report_promise_bang(runtime, toggle_config_entry_bang(runtime));
} else {
  report_promise_bang(runtime, edit_config_entry_bang(runtime));
}
return runtime.render(); })() : ((detail_showing_p(runtime, "config") && (panel_focused_p(runtime) && ((!panel_filtering_p(runtime)) && ((!palette_open) && (filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false)) === "@")))))) ? (() => { const rows = config_panel_rows(runtime); const total = rows.length; key.preventDefault();
key.stopPropagation();
if ((total > 0)) {
  const entry = rows[clamped_index(runtime.configIndex, total)];
  const input = active_input(runtime, ui);
  (input.value = $$bc$str(text(input.value), config_reference_text(configentry_kind(entry), configentry_name(entry))));
  focus_composer_bang(runtime, ui);
  render_minibuffer_bang(runtime, ui);
}
return runtime.render(); })() : ((panel_filterable_p(runtime) && (panel_focused_p(runtime) && ((!palette_open) && (!(filter_key_action(panel_filtering_p(runtime), panel_query(runtime), name, filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false))) === "")))))) ? (() => { const character = filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false)); const query = panel_query(runtime); const action = filter_key_action(panel_filtering_p(runtime), query, name, character); key.preventDefault();
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
return runtime.render(); })() : ((panel_focused_p(runtime) && ((!palette_open) && (!(filter_character(name, text(key.sequence), (((_truthy) => _truthy !== false && _truthy != null)(key.ctrl) ? true : false), (((_truthy) => _truthy !== false && _truthy != null)(meta) ? true : false)) === ""))))) ? (() => { key.preventDefault();
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
return render_minibuffer_bang(runtime, ui); })() : ((palette_open && (name === "tab"))) ? (() => { key.preventDefault();
key.stopPropagation();
return complete_palette_bang(runtime, ui, palette); })() : ((palette_open && submit_key_p(name))) ? (() => { const candidate = palette_candidate(runtime, palette); const completed = palette_completion_text(candidate); const current = text(active_input(runtime, ui).value); const action = palette_enter_action(palette.length, slashcommand_arguments(candidate), slashcommand_emoji(candidate), (current === completed)); key.preventDefault();
key.stopPropagation();
return ((action === "fire") ? submit_input_bang(runtime, ui, completed.trim()) : complete_palette_bang(runtime, ui, palette)); })() : (((!((_truthy) => _truthy !== false && _truthy != null)(runtime.stripFocused)) && ((!palette_open) && (ctrl_down_key_p(name, key) || ((name === "down") && ((!((_truthy) => _truthy !== false && _truthy != null)(key.ctrl)) && ((!((_truthy) => _truthy !== false && _truthy != null)(meta)) && (text(ui.composerInput.value).trim() === "")))))))) ? (() => { key.preventDefault();
key.stopPropagation();
return focus_strip_bang(runtime, ui); })() : (((name === "tab") || (name === "f2"))) ? (() => { key.preventDefault();
key.stopPropagation();
return show_view_bang(runtime, ui, tab_swap_view(text(runtime.view))); })() : ((name === "f1")) ? (() => { key.preventDefault();
key.stopPropagation();
return toggle_help_bang(runtime, ui); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "f3") || ((_logical) => (_logical !== false && _logical != null ? _logical : ((_logical) => (_logical !== false && _logical != null ? ((name === "h") || (name === "l")) : _logical))(meta)))(plain_view_arrow)))) ? (() => { const state = snapshot(runtime.model); const views = view_list(state); const current = selected_view(state, runtime.activeView); const index = views.findIndex((view) => (workview_id(view) === workview_id(current))); const delta = (((_truthy) => _truthy !== false && _truthy != null)(((name === "left") || ((_logical) => (_logical !== false && _logical != null ? (name === "h") : _logical))(meta))) ? -1 : 1); const view_count = views.length; const next_index = ((index + delta + view_count) % view_count); const next_id = text(views[next_index].id); key.preventDefault();
key.stopPropagation();
return show_view_bang(runtime, ui, next_id); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "f5") || ((_logical) => (_logical !== false && _logical != null ? (name === "r") : _logical))(key.ctrl)))) ? (() => { key.preventDefault();
key.stopPropagation();
return report_promise_bang(runtime, refresh_bang(runtime)); })() : (((_truthy) => _truthy !== false && _truthy != null)(((name === "f6") || ((_logical) => (_logical !== false && _logical != null ? (name === "o") : _logical))(key.ctrl)))) ? (() => { key.preventDefault();
key.stopPropagation();
return popout_bang(runtime, runtime.activeView); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((name === "j") || (name === "k")) : _logical))(meta))) ? (() => { const state = snapshot(runtime.model); const delta = ((name === "k") ? -1 : 1); key.preventDefault();
key.stopPropagation();
if ((text(runtime.view) === "agents")) {
  const agents = bridgesnapshot_semantic_agents(state);
  const agent_count = agents.length;
  const max_index = Math.max(0, (agent_count - 1));
  const agent_index = runtime.agentIndex;
  const next_index = Math.max(0, Math.min(max_index, (agent_index + delta)));
  const selected_agent_id = ((agent_count > 0) ? trackedthing_id(agents[next_index]) : "");
  (runtime.agentIndex = next_index);
  (runtime.model = select_tracked_thing(runtime.model, selected_agent_id));
} else {
  const view = selected_view(state, runtime.activeView);
  const items = workview_items(view);
  const item_count = items.length;
  const max_index = Math.max(0, (item_count - 1));
  const work_index = runtime.workIndex;
  const next_index = Math.max(0, Math.min(max_index, (work_index + delta)));
  const tracked_thing_id = ((item_count > 0) ? trackedthing_id(items[next_index]) : "");
  (runtime.workIndex = next_index);
  (runtime.model = select_tracked_thing(runtime.model, tracked_thing_id));
  ui.workScroll.scrollBy(delta, "step");
}
return runtime.render(); })() : (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (name === "c") : _logical))(key.ctrl))) ? (() => { const target = text(runtime.supervisorId); key.preventDefault();
key.stopPropagation();
if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (!(target === "")) : _logical))(runtime.working))) {
  return report_promise_bang(runtime, cancel_turn_bang(runtime, ui, target));
} })() : null);
} });
}

async function open_app_bang(view_id, source_identity) {
  const view = canonical_top_level_view(view_id);
  const renderer_promise = createCliRenderer({[$$bc$property_key($$bc$keyword("exitOnCtrlC"))]: false, [$$bc$property_key($$bc$keyword("clearOnShutdown"))]: true});
  const renderer = await renderer_promise;
  const runtime = {[$$bc$property_key($$bc$keyword("model"))]: make_model(view), [$$bc$property_key($$bc$keyword("renderer"))]: renderer, [$$bc$property_key($$bc$keyword("disposed"))]: false, [$$bc$property_key($$bc$keyword("rendererSuspended"))]: false, [$$bc$property_key($$bc$keyword("suspendResume"))]: null, [$$bc$property_key($$bc$keyword("suspendError"))]: "", [$$bc$property_key($$bc$keyword("view"))]: view, [$$bc$property_key($$bc$keyword("activeView"))]: view, [$$bc$property_key($$bc$keyword("agentIndex"))]: 0, [$$bc$property_key($$bc$keyword("workIndex"))]: 0, [$$bc$property_key($$bc$keyword("collapsedListConditions"))]: new Set(["blocked", "dormant", "draft", "terminal", "other"]), [$$bc$property_key($$bc$keyword("workScroll"))]: null, [$$bc$property_key($$bc$keyword("boardSignature"))]: "", [$$bc$property_key($$bc$keyword("dragThreadId"))]: "", [$$bc$property_key($$bc$keyword("bridgeExecutions"))]: new Set(), [$$bc$property_key($$bc$keyword("supervisorId"))]: "", [$$bc$property_key($$bc$keyword("conversation"))]: [], [$$bc$property_key($$bc$keyword("transcriptView"))]: "selected", [$$bc$property_key($$bc$keyword("itemSequence"))]: 0, [$$bc$property_key($$bc$keyword("lastAssistantText"))]: "", [$$bc$property_key($$bc$keyword("lastSubmitted"))]: "", [$$bc$property_key($$bc$keyword("working"))]: false, [$$bc$property_key($$bc$keyword("workingExecutions"))]: new Set(), [$$bc$property_key($$bc$keyword("workingLabel"))]: "", [$$bc$property_key($$bc$keyword("workingSince"))]: 0, [$$bc$property_key($$bc$keyword("spinnerIndex"))]: 0, [$$bc$property_key($$bc$keyword("spinnerTimer"))]: null, [$$bc$property_key($$bc$keyword("stripFocused"))]: false, [$$bc$property_key($$bc$keyword("stripIndex"))]: 0, [$$bc$property_key($$bc$keyword("detailView"))]: "", [$$bc$property_key($$bc$keyword("detailSegment"))]: "all", [$$bc$property_key($$bc$keyword("detailIndex"))]: 0, [$$bc$property_key($$bc$keyword("paletteIndex"))]: 0, [$$bc$property_key($$bc$keyword("paletteStart"))]: 0, [$$bc$property_key($$bc$keyword("paletteRows"))]: 0, [$$bc$property_key($$bc$keyword("promptGlyph"))]: DEFAULT_PROMPT_GLYPH, [$$bc$property_key($$bc$keyword("soundEnabled"))]: sound_enabled_from_env(text(process.env.NORTH_BRIDGE_SOUND)), [$$bc$property_key($$bc$keyword("soundPack"))]: sound_pack_from_env(text(process.env.NORTH_BRIDGE_SOUND_PACK)), [$$bc$property_key($$bc$keyword("soundDirectory"))]: sound_directory_from_env(text(process.env.NORTH_BRIDGE_SOUND_DIR)), [$$bc$property_key($$bc$keyword("soundPlayer"))]: discover_sound_player(), [$$bc$property_key($$bc$keyword("soundChildren"))]: new Set(), [$$bc$property_key($$bc$keyword("soundWarningShown"))]: false, [$$bc$property_key($$bc$keyword("soundSequence"))]: 0, [$$bc$property_key($$bc$keyword("lastSoundPath"))]: "", [$$bc$property_key($$bc$keyword("lastSoundAt"))]: 0, [$$bc$property_key($$bc$keyword("workspaceNotice"))]: "", [$$bc$property_key($$bc$keyword("keymap"))]: null, [$$bc$property_key($$bc$keyword("sessionModel"))]: text_or(process.env.NORTH_BRIDGE_MODEL, text(process.env.AGENT_MODEL)), [$$bc$property_key($$bc$keyword("sessionEffort"))]: text(process.env.AGENT_REASONING), [$$bc$property_key($$bc$keyword("launchProvider"))]: text(process.env.NORTH_BRIDGE_PROVIDER), [$$bc$property_key($$bc$keyword("launchTier"))]: text(process.env.NORTH_BRIDGE_TIER), [$$bc$property_key($$bc$keyword("launchModel"))]: text(process.env.NORTH_BRIDGE_MODEL), [$$bc$property_key($$bc$keyword("launchEffort"))]: text(process.env.NORTH_BRIDGE_EFFORT), [$$bc$property_key($$bc$keyword("controlThreadId"))]: text_or(text(process.env.NORTH_BRIDGE_CONTROL_THREAD), text_or(text(process.env.NORTH_THREAD_ID), text(process.env.AGENT_THREAD))), [$$bc$property_key($$bc$keyword("sessionCwd"))]: text(process.cwd()), [$$bc$property_key($$bc$keyword("sessionBranch"))]: "", [$$bc$property_key($$bc$keyword("sessionPermissions"))]: "", [$$bc$property_key($$bc$keyword("sourceIdentity"))]: source_identity, [$$bc$property_key($$bc$keyword("renderConversation"))]: () => null, [$$bc$property_key($$bc$keyword("render"))]: () => null};
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
  boot_bang(runtime, (prompt, role) => launch_agent_bang(runtime, prompt, role));
  return runtime;
}

function run_northbridge_app_bang(options) {
  return open_app_bang(text_or(options.viewId, BOOT_VIEW), text(options.sourceIdentity));
}

export { action_arguments as "action-arguments" };
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
export { boot_bang as "boot!" };
export { boot_view as "boot-view" };
export { bridge_app_launch_argv_bang as "bridge-app-launch-argv!" };
export { clamp_panel_cursor_bang as "clamp-panel-cursor!" };
export { cleanup_suspend_bang as "cleanup-suspend!" };
export { clear_panel_filter_bang as "clear-panel-filter!" };
export { composer_hint as "composer-hint" };
export { config_activation_of_json as "config-activation-of-json" };
export { config_activation_path as "config-activation-path" };
export { config_activation_path_from as "config-activation-path-from" };
export { config_detail_lines_bang as "config-detail-lines!" };
export { config_empty_note as "config-empty-note" };
export { config_entry_active_p as "config-entry-active?" };
export { config_header_keys as "config-header-keys" };
export { config_header_roles as "config-header-roles" };
export { config_header_shared_bang as "config-header-shared!" };
export { config_module_inspection_text_bang as "config-module-inspection-text!" };
export { config_panel_legend as "config-panel-legend" };
export { config_panel_rows as "config-panel-rows" };
export { config_query_field as "config-query-field" };
export { config_query_rows as "config-query-rows" };
export { config_reference_text as "config-reference-text" };
export { config_row_context_only_p as "config-row-context-only?" };
export { config_row_matches_p as "config-row-matches?" };
export { config_row_parts as "config-row-parts" };
export { config_row_role as "config-row-role" };
export { config_row_search_text as "config-row-search-text" };
export { config_section_rows as "config-section-rows" };
export { config_section_title as "config-section-title" };
export { config_state_text as "config-state-text" };
export { config_toggle_verb as "config-toggle-verb" };
export { config_unit_active_p as "config-unit-active?" };
export { config_view_includes_p as "config-view-includes?" };
export { config_view_rows as "config-view-rows" };
export { config_visible_count as "config-visible-count" };
export { delegation_argv_bang as "delegation-argv!" };
export { detail_height_bang as "detail-height!" };
export { escape_rung as "escape-rung" };
export { filter_character as "filter-character" };
export { filter_key_action as "filter-key-action" };
export { handle_local_command_bang as "handle-local-command!" };
export { help_query_rows as "help-query-rows" };
export { install_keys_bang as "install-keys!" };
export { launch_route_flags as "launch-route-flags" };
export { launch_thread_id as "launch-thread-id" };
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
export { semantic_view_text_bang as "semantic-view-text!" };
export { session_banner_bang as "session-banner!" };
export { session_banner_lines as "session-banner-lines" };
export { session_banner_runs as "session-banner-runs" };
export { set_launch_route_bang as "set-launch-route!" };
export { set_panel_query_bang as "set-panel-query!" };
export { submit_input_bang as "submit-input!" };
export { suspend_runtime_bang as "suspend-runtime!" };
export { tab_swap_view as "tab-swap-view" };
export { take_launch_route_flags_bang as "take-launch-route-flags!" };
export { top_level_view_p as "top-level-view?" };
export { transcript_banner_p as "transcript-banner?" };
export { transcript_placeholder as "transcript-placeholder" };
export { view_list as "view-list" };
export { view_tab_id_at_bang as "view-tab-id-at!" };
