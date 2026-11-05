use crate::guard::proxy_role;
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use serde_json::Value;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const UNIT: &str = "recommend-acts";
const MAX_BLOCKS: u32 = 2;
const FIRST_WINDOW: u64 = 256 * 1024;

pub const TRIGGERS: [&str; 10] = [
    "i recommend",
    "my recommendation",
    "i'd go with",
    "i would go with",
    "i suggest",
    "want me to",
    "should i",
    "shall i",
    "would you like",
    "do you want",
];

pub const ASK_ONCE: [&str; 16] = [
    "cost", "costs", "money", "spend", "billing", "paid", "account", "accounts", "public", "publish", "credential", "credentials", "password", "delete", "deleting", "deletion",
];

pub const ACTIONS: [&str; 3] = [
    r"\bgh\b[^\n;&|]*?\bissue\s+(?:create|edit)\b",
    r"\bthreads\s+(?:new|box\s+add)\b",
    r"\bnorth\s+fleet\s+(?:launch|send)\b",
];

pub const ACTION_TOOLS: [&str; 1] = ["SendMessage"];

#[derive(Debug, Default, PartialEq)]
pub struct Turn {
    pub id: String,
    pub commands: Vec<String>,
    pub tools: Vec<String>,
    pub reply: Option<String>,
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    if !proxy_role::is_proxy(&proxy_role::Live, input) {
        return Verdict::Allow;
    }
    let Some(home) = std::env::var_os("HOME") else { return Verdict::Allow };
    decide(input, &PathBuf::from(home).join(".local/state/north/recommend-acts"))
}

pub fn decide(input: &HookInput, state: &Path) -> Verdict {
    let Some(mut turn) = input.data["transcript_path"].as_str().and_then(read_turn) else { return Verdict::Allow };
    if let Some(text) = input.data["last_assistant_message"].as_str() {
        turn.reply = Some(text.to_owned());
    }
    let Some(missing) = turn.reply.as_deref().and_then(|reply| missing_step(reply, &turn)) else {
        return Verdict::Allow;
    };
    let session = input.data["session_id"].as_str().unwrap_or("unknown").replace(['/', '.'], "_");
    if !count_block(&state.join(session), &turn.id) {
        return Verdict::Allow;
    }
    Verdict::Deny(missing)
}

pub fn missing_step(reply: &str, turn: &Turn) -> Option<String> {
    let reply = reply.replace(['\u{2019}', '\u{2018}'], "'");
    let triggered = triggering_sentences(&reply);
    let phrase = triggered.first()?.0.clone();
    if triggered.iter().any(|(_, sentence)| names_ask_once(sentence)) {
        return None;
    }
    let acted = turn.tools.iter().any(|tool| ACTION_TOOLS.contains(&tool.as_str())) || turn.commands.iter().any(|command| actions().iter().any(|action| action.is_match(command)));
    let cited = citation().is_match(&reply);
    let step = match (acted, cited) {
        (true, true) => return None,
        (false, _) => "this turn filed no issue and staffed no one: act on it now, filing it (`gh issue create|edit`, `threads new`) or staffing it (`north fleet launch|send`), and cite the issue as repo#N",
        (true, false) => "the reply cites no issue: name the issue it filed or moved as repo#N",
    };
    Some(format!("BLOCKED ({UNIT}): your reply recommends or asks (\"{phrase}\") but {step}; ask Tom first only for cost, accounts, making something public, credentials or deleting valuable data."))
}

fn triggering_sentences(reply: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for sentence in sentences(reply) {
        let words = words(sentence);
        if let Some(phrase) = TRIGGERS.iter().find(|phrase| words.contains(&format!(" {phrase} "))) {
            found.push(((*phrase).to_owned(), sentence.to_owned()));
        }
    }
    if let Some(question) = option_list_question(reply) {
        found.push(("option list".to_owned(), question));
    }
    found
}

fn option_list_question(reply: &str) -> Option<String> {
    let reply = reply.trim_end();
    if !reply.ends_with('?') {
        return None;
    }
    let labels: Vec<usize> = option_label().find_iter(reply).map(|found| found.start()).collect();
    let numbered: Vec<usize> = reply.match_indices('\n').map(|(at, _)| at + 1).chain([0]).filter(|at| numbered_line().is_match(reply[*at..].lines().next().unwrap_or_default())).collect();
    let starts = if labels.len() >= 2 { labels } else { numbered };
    (starts.len() >= 2).then(|| reply[*starts.iter().min().unwrap_or(&0)..].to_owned())
}

fn names_ask_once(sentence: &str) -> bool {
    let words = words(sentence);
    ASK_ONCE.iter().any(|word| words.contains(&format!(" {word} ")))
}

fn sentences(text: &str) -> Vec<&str> {
    text.split_inclusive(['.', '!', '?', '\n']).map(str::trim).filter(|sentence| !sentence.is_empty()).collect()
}

fn words(text: &str) -> String {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower.split(|character: char| !(character.is_alphanumeric() || character == '\'')).filter(|word| !word.is_empty()).collect();
    format!(" {} ", words.join(" "))
}

fn count_block(file: &Path, turn: &str) -> bool {
    let recorded = std::fs::read_to_string(file).ok();
    let blocks = recorded.as_deref().and_then(|text| text.split_once('\t')).filter(|(id, _)| *id == turn).and_then(|(_, count)| count.trim().parse::<u32>().ok()).unwrap_or(0);
    if blocks >= MAX_BLOCKS {
        return false;
    }
    let _ = file.parent().map(std::fs::create_dir_all);
    let _ = std::fs::write(file, format!("{turn}\t{}\n", blocks + 1));
    true
}

pub fn read_turn(path: &str) -> Option<Turn> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let mut window = FIRST_WINDOW;
    loop {
        let start = length.saturating_sub(window);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut tail = Vec::new();
        file.read_to_end(&mut tail).ok()?;
        let lines: &[u8] = if start > 0 { tail.splitn(2, |byte| *byte == b'\n').nth(1).unwrap_or_default() } else { &tail };
        if let Some(turn) = turn_in(lines) {
            return Some(turn);
        }
        if start == 0 {
            return None;
        }
        window *= 4;
    }
}

pub fn turn_in(lines: &[u8]) -> Option<Turn> {
    let entries: Vec<Value> = lines.split(|byte| *byte == b'\n').filter_map(|line| serde_json::from_slice(line).ok()).collect();
    let begin = entries.iter().rposition(starts_turn)?;
    let mut turn = Turn { id: entries[begin]["uuid"].as_str().unwrap_or_default().to_owned(), ..Turn::default() };
    let failed: Vec<&str> = entries[begin + 1..].iter().filter(|entry| entry["type"] == "user").filter_map(|entry| entry["message"]["content"].as_array()).flatten().filter(|block| block["type"] == "tool_result" && block["is_error"] == true).filter_map(|block| block["tool_use_id"].as_str()).collect();
    for entry in &entries[begin + 1..] {
        if entry["type"] != "assistant" {
            continue;
        }
        let Some(blocks) = entry["message"]["content"].as_array() else { continue };
        let text: Vec<&str> = blocks.iter().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();
        if !text.is_empty() {
            turn.reply = Some(text.join("\n\n"));
        }
        for block in blocks.iter().filter(|block| block["type"] == "tool_use") {
            turn.reply = None;
            if block["id"].as_str().is_some_and(|id| failed.contains(&id)) {
                continue;
            }
            turn.tools.push(block["name"].as_str().unwrap_or_default().rsplit('.').next().unwrap_or_default().to_owned());
            if let Some(command) = block["input"]["command"].as_str() {
                turn.commands.push(command.to_owned());
            }
        }
    }
    Some(turn)
}

fn starts_turn(entry: &Value) -> bool {
    if entry["type"] != "user" || entry["isMeta"] == true || entry["isSidechain"] == true {
        return false;
    }
    match &entry["message"]["content"] {
        Value::String(_) => true,
        Value::Array(blocks) => !blocks.iter().any(|block| block["type"] == "tool_result"),
        _ => false,
    }
}

fn actions() -> &'static [Regex] {
    static ACTIONS_RE: OnceLock<Vec<Regex>> = OnceLock::new();
    ACTIONS_RE.get_or_init(|| ACTIONS.iter().map(|pattern| Regex::new(pattern).unwrap()).collect())
}

fn citation() -> &'static Regex {
    static CITATION: OnceLock<Regex> = OnceLock::new();
    CITATION.get_or_init(|| Regex::new(r"(?:^|[^\w#/])(?:[\w.-]+/)?[A-Za-z][\w.-]*#\d+\b").unwrap())
}

fn option_label() -> &'static Regex {
    static LABEL: OnceLock<Regex> = OnceLock::new();
    LABEL.get_or_init(|| Regex::new(r"\b[Oo]ption\s+(?:[A-D]|[1-9])\b").unwrap())
}

fn numbered_line() -> &'static Regex {
    static NUMBERED: OnceLock<Regex> = OnceLock::new();
    NUMBERED.get_or_init(|| Regex::new(r"^\s*(?:\d+[.)]|\(\d+\))\s+\S").unwrap())
}
