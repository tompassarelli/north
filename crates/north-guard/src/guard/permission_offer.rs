use crate::hook::{HookInput, Verdict};
use crate::platform::pyjson::{self, Value};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

const TAIL_BYTES: u64 = 256 * 1024;

pub const OFFERS: [&str; 10] = [
    "say if you want me to",
    "want me to",
    "should i ",
    "shall i ",
    "let me know if you'd like",
    "if you'd like, i can",
    "do you want me to",
    "would you like me to",
    "let me know if you want",
    "if you like, i can",
];

pub const REASON: &str = "Your report hands Tom reversible work as a question. If it's reversible, do it now under standing authority. If it needs Tom (cost, access, accounts, direction), rewrite it as a decision: your recommendation first and why, then at least one alternative with what it costs.";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    let session = std::env::var(crate::fleet::runtime::SESSION_ENV).unwrap_or_default();
    decide(input, &session)
}

pub fn decide(input: &HookInput, session: &str) -> Verdict {
    match proxy_report(input, session).as_deref().and_then(offer) {
        Some(_) => Verdict::Deny(REASON.into()),
        None => Verdict::Allow,
    }
}

pub fn proxy_report(input: &HookInput, session: &str) -> Option<String> {
    if input.data["stop_hook_active"] == Value::Bool(true) || !(session.is_empty() || session == crate::fleet::runtime::PROXY) {
        return None;
    }
    match input.data["last_assistant_message"].as_str() {
        Some(text) => Some(text.to_owned()),
        None => input.data["transcript_path"].as_str().and_then(final_text),
    }
}

pub fn offer(text: &str) -> Option<&'static str> {
    let closing = words(&closing_paragraph(text));
    let has = |phrase: &str| closing.contains(&words(phrase));
    let decision = (has("recommend") || has("recommendation")) && (has("alternative") || has("alternatives") || has("otherwise"));
    if decision {
        return None;
    }
    OFFERS.into_iter().find(|phrase| has(phrase))
}

fn closing_paragraph(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let end = lines.iter().rposition(|line| !line.trim().is_empty()).map_or(0, |at| at + 1);
    let start = lines[..end].iter().rposition(|line| line.trim().is_empty()).map_or(0, |at| at + 1);
    lines[start..end].join("\n")
}

fn words(text: &str) -> String {
    let text = text.to_lowercase().replace('\u{2019}', "'");
    let words: Vec<&str> = text.split(|character: char| !(character.is_alphanumeric() || character == '\'')).filter(|word| !word.is_empty()).collect();
    format!(" {} ", words.join(" "))
}

fn final_text(path: &str) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES))).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    last_assistant_text(&tail)
}

pub fn last_assistant_text(tail: &[u8]) -> Option<String> {
    for line in tail.split(|byte| *byte == b'\n').rev() {
        let Ok(entry) = pyjson::loads(line) else { continue };
        match entry["type"].as_str() {
            Some("assistant") => {}
            Some("user") if entry["isMeta"] != Value::Bool(true) => return None,
            _ => continue,
        }
        let Some(blocks) = entry["message"]["content"].as_array() else { continue };
        if blocks.iter().any(|block| block["type"] == "tool_use") {
            return None;
        }
        let text: Vec<&str> = blocks.iter().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();
        if !text.is_empty() {
            return Some(text.join("\n\n"));
        }
    }
    None
}
