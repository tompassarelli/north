use std::sync::LazyLock;

use regex::Regex;

use super::json::Json;
use super::text::{short_lower, strip};

const INJECTED: [&str; 10] = [
    "base directory for this skill:", "[request interrupted by user", "stop hook feedback:", "caveat:", "[image:", "# agents.md instructions",
    "this session is being continued from a previous conversation", "another claude session sent", "the coordinator sent a message",
    "[system notification",
];
const PING: &str = "[north real-time ping";
const SENT: [&str; 3] = ["[from ", "[gate ", "[urgent message"];
const SIGNED_SINCE: &str = "2026-11-06";

static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^</?(?:task-notification|system-reminder|local-command-[a-z]+|command-[a-z]+|user-prompt-submit-hook|environment_context|user_instructions|recommended_plugins|codex_internal_context|turn_aborted|image|subagent_notification)\b",
    )
    .expect("tag expression")
});

pub(super) fn speaker(role: &str, text: &str) -> String {
    let head = short_lower(text, 80);
    let injected = TAG.is_match(text) || head.starts_with(PING) || INJECTED.iter().any(|prefix| head.starts_with(prefix));
    let sent = SENT.iter().any(|prefix| strip(text).starts_with(prefix));
    (if role != "user" {
        role
    } else if injected {
        "system"
    } else if sent {
        "sent"
    } else {
        role
    })
    .to_owned()
}

pub(super) fn delivered(record: &Json, opening: bool, home: &str) -> bool {
    let human = record.get("origin").and_then(|origin| origin.text_of("kind")) == Some("human");
    let unsigned = record.text_of("timestamp").is_none_or(|ts| ts < SIGNED_SINCE) && record.text_of("cwd") != Some(home);
    !human && record.text_of("promptSource") == Some("sdk") && (opening || unsigned)
}
