use std::sync::LazyLock;

use regex::Regex;
use regex::bytes::Regex as ByteRegex;

use super::json::{Json, dumps};
use super::text::{clip, short_lower};

const TOOL_KEYS: [&str; 13] = [
    "skill", "command", "cmd", "file_path", "path", "pattern", "query", "prompt", "description", "old_string", "new_string", "url", "content",
];
const INJECTED: [&str; 10] = [
    "base directory for this skill:", "[request interrupted by user", "stop hook feedback:", "caveat:", "[image:", "# agents.md instructions",
    "this session is being continued from a previous conversation", "another claude session sent", "the coordinator sent a message",
    "[system notification",
];
const PING: &str = "[north real-time ping";

static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^</?(?:task-notification|system-reminder|local-command-[a-z]+|command-[a-z]+|user-prompt-submit-hook|environment_context|user_instructions|recommended_plugins|codex_internal_context|turn_aborted|image|subagent_notification)\b",
    )
    .expect("tag expression")
});
static CLAUDE_KEEP: LazyLock<ByteRegex> =
    LazyLock::new(|| ByteRegex::new(r#""type": ?"(?:user|assistant|ai-title)""#).expect("keep expression"));

pub(super) struct Message {
    pub role: String,
    pub text: String,
    pub tool: Option<String>,
}

#[derive(Default)]
pub(super) struct Meta {
    pub session_id: Option<String>,
    pub agent: Option<String>,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub cwd: Option<String>,
    pub branch: Option<String>,
}

impl Meta {
    pub(super) fn merge(&mut self, other: Meta) {
        for (mine, theirs) in [
            (&mut self.session_id, other.session_id),
            (&mut self.agent, other.agent),
            (&mut self.title, other.title),
            (&mut self.slug, other.slug),
            (&mut self.cwd, other.cwd),
            (&mut self.branch, other.branch),
        ] {
            if theirs.is_some() {
                *mine = theirs;
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Kind {
    Claude,
    Codex,
}

pub(super) fn speaker(role: &str, text: &str) -> String {
    let head = short_lower(text, 80);
    let injected = TAG.is_match(text) || head.starts_with(PING) || INJECTED.iter().any(|prefix| head.starts_with(prefix));
    (if role == "user" && injected { "system" } else { role }).to_owned()
}

pub(super) fn tool_text(name: &str, args: Option<&Json>) -> String {
    let parsed = match args {
        Some(Json::Text(text)) => Json::parse(text.as_bytes()).filter(Json::is_object),
        _ => None,
    };
    let body = match parsed.as_ref().or(args) {
        Some(object @ Json::Object(_)) => {
            let parts: Vec<String> = TOOL_KEYS
                .iter()
                .filter_map(|key| object.text_of(key).filter(|value| !value.trim().is_empty()).map(|value| format!("{key}={value}")))
                .collect();
            if parts.is_empty() {
                dumps(object, true).chars().take(2000).collect()
            } else {
                parts.join("\n")
            }
        }
        Some(Json::Text(text)) => text.clone(),
        _ => String::new(),
    };
    clip(&format!("{name} {body}"))
}

fn named(value: Option<&Json>) -> String {
    value.and_then(Json::text).filter(|name| !name.is_empty()).unwrap_or("tool").to_owned()
}

fn claude_meta(record: &Json) -> Meta {
    let field = |key: &str| record.text_of(key).filter(|value| !value.is_empty()).map(str::to_owned);
    Meta { session_id: field("sessionId"), cwd: field("cwd"), branch: field("gitBranch"), slug: field("slug"), ..Meta::default() }
}

pub(super) fn extract_claude(record: &Json) -> (Vec<Message>, Meta) {
    let mut meta = claude_meta(record);
    let kind = record.text_of("type").unwrap_or("");
    if kind == "ai-title" {
        meta.title = record.text_of("aiTitle").filter(|title| !title.is_empty()).map(str::to_owned);
        return (Vec::new(), meta);
    }
    let Some(message) = record.get("message").filter(|message| message.is_object()) else {
        return (Vec::new(), meta);
    };
    if kind != "user" && kind != "assistant" {
        return (Vec::new(), meta);
    }
    let content = message.get("content");
    let single = [Json::Object(vec![("type".to_owned(), Json::Text("text".to_owned())), ("text".to_owned(), content.and_then(Json::text).map_or(Json::Null, |t| Json::Text(t.to_owned())))])];
    let blocks = if content.and_then(Json::text).is_some() { &single[..] } else { content.map_or(&[][..], Json::items) };
    let injected = kind == "user" && (record.get("isMeta").is_some_and(Json::truthy) || record.get("isCompactSummary").is_some_and(Json::truthy));
    let mut out = Vec::new();
    for block in blocks.iter().filter(|block| block.is_object()) {
        let (role, text, tool) = match block.text_of("type") {
            Some("text") => (kind.to_owned(), clip(block.text_of("text").unwrap_or("")), None),
            Some("thinking") => ("thinking".to_owned(), clip(block.text_of("thinking").unwrap_or("")), None),
            Some("tool_use") => {
                let tool = named(block.get("name"));
                ("tool".to_owned(), tool_text(&tool, block.get("input")), Some(tool))
            }
            _ => continue,
        };
        if !text.is_empty() {
            let role = if injected { "system".to_owned() } else { speaker(&role, &text) };
            out.push(Message { role, text, tool });
        }
    }
    (out, meta)
}

fn codex_meta(payload: &Json) -> Meta {
    let mut meta = Meta::default();
    let field = |key: &str| payload.text_of(key).filter(|value| !value.is_empty()).map(str::to_owned);
    let session = field("session_id").or_else(|| field("id"));
    if let Some(session) = session {
        meta.agent = payload.text_of("id").filter(|id| *id != session).map(str::to_owned);
        meta.session_id = Some(session);
    }
    meta.cwd = field("cwd");
    meta.title = field("agent_nickname");
    meta
}

fn codex_texts(payload: &Json) -> Vec<String> {
    let blocks = match payload.get("content") {
        Some(Json::Text(text)) => return vec![clip(text)],
        Some(content) => content.items(),
        None => &[],
    };
    blocks
        .iter()
        .filter(|block| block.is_object() && matches!(block.text_of("type"), Some("input_text" | "output_text" | "text")))
        .map(|block| clip(block.text_of("text").unwrap_or("")))
        .collect()
}

pub(super) fn extract_codex(record: &Json) -> (Vec<Message>, Meta) {
    let Some(payload) = record.get("payload").filter(|payload| payload.is_object()) else {
        return (Vec::new(), Meta::default());
    };
    match record.text_of("type") {
        Some("session_meta") => (Vec::new(), codex_meta(payload)),
        Some("response_item") => (codex_items(payload), Meta::default()),
        _ => (Vec::new(), Meta::default()),
    }
}

fn codex_items(payload: &Json) -> Vec<Message> {
    let make = |role: &str, text: String, tool: Option<String>| Message { role: role.to_owned(), text, tool };
    match payload.text_of("type") {
        Some("message") if payload.text_of("role") != Some("developer") => {
            let role = payload.text_of("role").filter(|role| !role.is_empty()).unwrap_or("user");
            codex_texts(payload).into_iter().filter(|text| !text.is_empty()).map(|text| make(&speaker(role, &text), text, None)).collect()
        }
        Some("reasoning") => ["summary", "content"]
            .iter()
            .filter_map(|key| payload.get(key).filter(|value| matches!(value, Json::List(_))))
            .flat_map(Json::items)
            .filter(|block| block.is_object())
            .map(|block| clip(block.text_of("text").unwrap_or("")))
            .filter(|text| !text.is_empty())
            .map(|text| make("thinking", text, None))
            .collect(),
        Some("function_call" | "custom_tool_call") => {
            let tool = named(payload.get("name"));
            let args = payload.get("arguments").filter(|value| !matches!(value, Json::Null)).or_else(|| payload.get("input"));
            vec![make("tool", tool_text(&tool, args), Some(tool))]
        }
        _ => Vec::new(),
    }
}

pub(super) fn wanted(kind: Kind, line: &[u8]) -> bool {
    match kind {
        Kind::Codex => {
            let head = &line[..line.len().min(200)];
            head.windows(13).any(|window| window == b"response_item") || head.windows(12).any(|window| window == b"session_meta")
        }
        Kind::Claude => CLAUDE_KEEP.is_match(line),
    }
}
