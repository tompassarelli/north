use crate::guard::proxy_role;
use crate::guard::recommend_acts;
use crate::hook::{HookInput, Verdict};
use std::path::{Path, PathBuf};

const UNIT: &str = "status-not-action";

pub const ANNOUNCEMENTS: [&str; 5] = ["i'll", "i will", "next i", "will now", "i'm going to"];
pub const NOTHING_NEEDED: [&str; 12] = ["no action needed", "no action is needed", "no action required", "no action is required", "nothing needs your decision", "nothing needs your attention", "nothing needs you", "nothing for you to do", "nothing needed from you", "no decision needed", "no decision is needed", "nothing to decide"];

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
    let reply = turn.reply.as_deref().unwrap_or_default();
    let (announced, nothing) = (announced_step(reply), nothing_needed(reply));
    if announced.is_none() && nothing.is_none() {
        return Verdict::Allow;
    }
    let session = input.data["session_id"].as_str().unwrap_or("unknown").replace(['/', '.'], "_");
    if !recommend_acts::count_block(&state.join(session), &turn.id) {
        return Verdict::Allow;
    }
    if let Some(line) = nothing {
        return Verdict::Deny(format!("BLOCKED ({UNIT}): your final sentence (\"{line}\") tells Tom nothing is needed, which is noise: delete it and end on what was done, or on the one decision Tom must make as a multiple-choice question."));
    }
    let announced = announced.unwrap_or_default();
    Verdict::Deny(format!("BLOCKED ({UNIT}): your final sentence announces a step (\"{announced}\") but no tool call follows it; make the call now, or end on what was done, or on the one decision Tom must make as a multiple-choice question."))
}

pub fn nothing_needed(reply: &str) -> Option<String> {
    closing_phrase(reply, &NOTHING_NEEDED)
}

pub fn announced_step(reply: &str) -> Option<String> {
    closing_phrase(reply, &ANNOUNCEMENTS)
}

fn closing_phrase(reply: &str, phrases: &[&str]) -> Option<String> {
    let reply = reply.replace(['\u{2019}', '\u{2018}'], "'");
    let last = recommend_acts::sentences(&reply).pop()?;
    let words = recommend_acts::words(last);
    phrases.iter().find(|phrase| words.contains(&format!(" {phrase} "))).map(|phrase| (*phrase).to_owned())
}
