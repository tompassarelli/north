use crate::guard::proxy_role;
use crate::guard::recommend_acts;
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const UNIT: &str = "unproven-cause";

pub const CAUSE_WORDS: [&str; 4] = ["because", "caused by", "the cause is", "the cause was"];

static EVIDENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"`[^`\n]+`|"[^"\n]{3,}"|\u{201c}[^\u{201d}\n]{3,}\u{201d}|\b\d+(?:\.\d+)?\s?(?:ms|s|sec|min|h|%|KiB|MiB|GiB|KB|MB|GB|bytes|cpus?|cores?)\b"#).unwrap());

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    if !proxy_role::is_tom_window(&proxy_role::Live, input) {
        return Verdict::Allow;
    }
    let Some(home) = std::env::var_os("HOME") else { return Verdict::Allow };
    decide(input, &PathBuf::from(home).join(".local/state/north/unproven-cause"))
}

pub fn decide(input: &HookInput, state: &Path) -> Verdict {
    let Some(mut turn) = input.data["transcript_path"].as_str().and_then(recommend_acts::read_turn) else { return Verdict::Allow };
    if let Some(text) = input.data["last_assistant_message"].as_str() {
        turn.reply = Some(text.to_owned());
    }
    let Some(claim) = unproven_cause(turn.reply.as_deref().unwrap_or_default()) else { return Verdict::Allow };
    let session = input.data["session_id"].as_str().unwrap_or("unknown").replace(['/', '.'], "_");
    if !recommend_acts::count_block(&state.join(session), &turn.id) {
        return Verdict::Allow;
    }
    Verdict::Deny(format!("BLOCKED ({UNIT}): your reply names a cause (\"{claim}\") and its paragraph quotes no command, log line or measurement; quote the `command`, the \"log line\" or the measured number that shows it, or say the cause is unproven and what would prove it."))
}

pub fn unproven_cause(reply: &str) -> Option<String> {
    let reply = reply.replace(['\u{2019}', '\u{2018}'], "'");
    reply.split("\n\n").filter(|paragraph| !EVIDENCE.is_match(paragraph)).find_map(|paragraph| {
        let words = recommend_acts::words(paragraph);
        CAUSE_WORDS.iter().find(|phrase| words.contains(&format!(" {phrase} "))).map(|phrase| (*phrase).to_owned())
    })
}
