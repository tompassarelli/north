use crate::guard::proxy_role;
use crate::guard::recommend_acts;
use crate::hook::{HookInput, Verdict};
use std::path::{Path, PathBuf};

const UNIT: &str = "status-not-action";

pub const ANNOUNCEMENTS: [&str; 5] = ["i'll", "i will", "next i", "will now", "i'm going to"];

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    if !proxy_role::is_tom_window(&proxy_role::Live, input) {
        return Verdict::Allow;
    }
    let Some(home) = std::env::var_os("HOME") else { return Verdict::Allow };
    decide(input, &PathBuf::from(home).join(".local/state/north/status-not-action"))
}

pub fn decide(input: &HookInput, state: &Path) -> Verdict {
    let Some(mut turn) = input.data["transcript_path"].as_str().and_then(recommend_acts::read_turn) else { return Verdict::Allow };
    if let Some(text) = input.data["last_assistant_message"].as_str() {
        turn.reply = Some(text.to_owned());
    }
    let Some(announced) = turn.reply.as_deref().and_then(announced_step) else { return Verdict::Allow };
    let session = input.data["session_id"].as_str().unwrap_or("unknown").replace(['/', '.'], "_");
    if !recommend_acts::count_block(&state.join(session), &turn.id) {
        return Verdict::Allow;
    }
    Verdict::Deny(format!("BLOCKED ({UNIT}): your final sentence announces a step (\"{announced}\") but no tool call follows it; make the call now, or end on what was done, or on the one decision Tom must make as a multiple-choice question."))
}

pub fn announced_step(reply: &str) -> Option<String> {
    let reply = reply.replace(['\u{2019}', '\u{2018}'], "'");
    let last = recommend_acts::sentences(&reply).pop()?;
    let words = recommend_acts::words(last);
    ANNOUNCEMENTS.iter().find(|phrase| words.contains(&format!(" {phrase} "))).map(|phrase| (*phrase).to_owned())
}
