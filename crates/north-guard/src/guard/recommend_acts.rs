use crate::guard::proxy_role;
use crate::hook::{HookInput, Verdict};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

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

static GH_EDIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgh\b([^\n;&|]*?)\bissue\s+edit\b([^\n;&|]*)").unwrap());
static GH_CREATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgh\b[^\n;&|]*?\bissue\s+create\b").unwrap());
static THREADS_NOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bthreads\s+(?:box\s+add|note)\s+(?:-\S+\s+)*(\S+)").unwrap());
static THREADS_NEW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bthreads\s+new\b").unwrap());
static ISSUE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"github\.com/[\w.-]+/([\w.-]+)/issues/(\d+)").unwrap());
static CITATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[^\w#/])(?:[\w.-]+/)?([A-Za-z][\w.-]*)#(\d+)\b").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub repo: Option<String>,
    pub number: u64,
}

impl Issue {
    fn names(&self, cited: &Issue) -> bool {
        self.number == cited.number && (self.repo.is_none() || cited.repo.is_none() || self.repo == cited.repo)
    }

    pub fn label(&self) -> String {
        format!("{}#{}", self.repo.as_deref().unwrap_or(""), self.number)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Turn {
    pub id: String,
    pub commands: Vec<String>,
    pub moved: Vec<Issue>,
    pub reply: Option<String>,
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    if !proxy_role::is_tom_window(&proxy_role::Live, input) {
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

#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    pub phrase: String,
    pub cited: Vec<Issue>,
    pub acted: bool,
}

/// The reply's recommendation or question and whether its turn created or edited an issue it cites, or None when it makes none or names an ask-once decision.
pub fn recommendation(reply: &str, turn: &Turn) -> Option<Recommendation> {
    let reply = reply.replace(['\u{2019}', '\u{2018}'], "'");
    let triggered = triggering_sentences(&reply);
    let phrase = triggered.first()?.0.clone();
    if triggered.iter().any(|(_, sentence)| names_ask_once(sentence)) {
        return None;
    }
    let cited = cited_issues(&reply);
    let acted = cited.iter().any(|cited| turn.moved.iter().any(|moved| moved.names(cited)));
    Some(Recommendation { phrase, cited, acted })
}

pub fn missing_step(reply: &str, turn: &Turn) -> Option<String> {
    let Recommendation { phrase, cited, acted } = recommendation(reply, turn)?;
    if acted {
        return None;
    }
    let step = match (turn.moved.first(), cited.first()) {
        (None, Some(cited)) => format!("this turn created or edited no issue, so no box tracks this on {}: add it with `gh issue edit N` (a box) or `gh issue create`, then cite that issue", cited.label()),
        (None, None) => "this turn created or edited no issue: file it with `gh issue create` or add a box with `gh issue edit N`, and cite the issue as repo#N".to_owned(),
        (Some(moved), None) => format!("the reply cites no issue: name {} as repo#N", moved.label()),
        (Some(moved), Some(cited)) => format!("the reply cites {} but this turn only created or edited {}: cite that issue, or add the box to {} with `gh issue edit N`", cited.label(), moved.label(), cited.label()),
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
    let results: HashMap<&str, String> = entries[begin + 1..].iter().filter(|entry| entry["type"] == "user").filter_map(|entry| entry["message"]["content"].as_array()).flatten().filter(|block| block["type"] == "tool_result").filter_map(|block| Some((block["tool_use_id"].as_str()?, result_text(&block["content"])))).collect();
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
            if let Some(command) = block["input"]["command"].as_str() {
                turn.moved.extend(moved_by(command, results.get(block["id"].as_str().unwrap_or_default()).map_or("", String::as_str)));
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

fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn moved_by(command: &str, output: &str) -> Vec<Issue> {
    let mut moved = Vec::new();
    for edit in GH_EDIT.captures_iter(command) {
        moved.extend(edited_issue(&format!("{} {}", &edit[1], &edit[2])));
    }
    if GH_CREATE.is_match(command) {
        moved.extend(cited_issues(output));
    }
    for note in THREADS_NOTE.captures_iter(command) {
        moved.extend(cited_issues(&format!(" {}", &note[1])));
    }
    if THREADS_NEW.is_match(command) {
        moved.extend(cited_issues(output));
    }
    moved
}

fn edited_issue(arguments: &str) -> Option<Issue> {
    let mut repo = None;
    let mut tokens = arguments.split_whitespace();
    let mut number = None;
    let mut positional = false;
    while let Some(token) = tokens.next() {
        if let Some(value) = token.strip_prefix("--repo=") {
            repo = value.rsplit('/').next().map(str::to_owned);
        } else if matches!(token, "-R" | "--repo") {
            repo = tokens.next().and_then(|value| value.rsplit('/').next()).map(str::to_owned);
        } else if !token.starts_with('-') && !positional {
            positional = true;
            number = issue_number(token);
        }
    }
    let (url_repo, number) = number?;
    Some(Issue { repo: repo.or(url_repo), number })
}

fn issue_number(token: &str) -> Option<(Option<String>, u64)> {
    if let Some(url) = ISSUE_URL.captures(token) {
        return Some((Some(url[1].to_owned()), url[2].parse().ok()?));
    }
    token.trim_start_matches('#').parse().ok().map(|number| (None, number))
}

fn cited_issues(text: &str) -> Vec<Issue> {
    let urls = ISSUE_URL.captures_iter(text).filter_map(|url| Some(Issue { repo: Some(url[1].to_owned()), number: url[2].parse().ok()? }));
    let short = CITATION.captures_iter(text).filter_map(|cited| Some(Issue { repo: Some(cited[1].to_owned()), number: cited[2].parse().ok()? }));
    urls.chain(short).collect()
}

fn option_label() -> &'static Regex {
    static LABEL: OnceLock<Regex> = OnceLock::new();
    LABEL.get_or_init(|| Regex::new(r"\b[Oo]ption\s+(?:[A-D]|[1-9])\b").unwrap())
}

fn numbered_line() -> &'static Regex {
    static NUMBERED: OnceLock<Regex> = OnceLock::new();
    NUMBERED.get_or_init(|| Regex::new(r"^\s*(?:\d+[.)]|\(\d+\))\s+\S").unwrap())
}
