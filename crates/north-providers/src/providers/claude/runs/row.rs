use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use super::{activity, int, number, outcome, present};
use crate::foundation::clock::Timestamp;

static WAITING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*(waiting|i.ll wait|i.ll pick|the test run)").unwrap());
static ETA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bETA\b[^0-9\n]{0,12}([0-9]+(?:\.[0-9]+)?)\s*(hours?|hrs?|h|minutes?|mins?|m)\b").unwrap());
static STATED_ETA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^ETA:[ \t]*([0-9]+(?:\.[0-9]+)?)[ \t]*(hours?|hrs?|h|minutes?|mins?|m)\b").unwrap());
pub static FLAWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\baccepted flaws?:\s*([0-9]+)").unwrap());
pub static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static BACKGROUNDED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"running in background with ID: (\w+)|moved to the background \(ID: (\w+)\)|job-run: ([\w.-]+) runs detached").unwrap());
static NOTIFIED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<task-id>(\w+)</task-id>|job-run: ([\w.-]+) exited").unwrap());
const RUNNING_SECONDS: i64 = 7200;
pub const TRANSCRIPT_ORIGIN: &str = "claude.transcript";
const INTERRUPTED: &str = "[Request interrupted";
const DELIVERED: [&str; 2] = ["[from ", "[north:"];

pub(super) struct Transcript<'a> {
    pub lines: &'a [Value],
    pub id: &'a str,
    pub role: &'a str,
    pub tier: &'a str,
    pub since: &'a str,
    pub backfill: bool,
    pub now: Timestamp,
}

fn texts(record: &Value) -> String {
    match &record["message"]["content"] {
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .map(|part| match &part["text"] {
                Value::String(text) => text.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

pub fn prefix(text: &str, length: usize) -> String {
    text.chars().take(length).collect()
}

fn seconds(text: &str) -> Option<i64> {
    Timestamp::parse(text).ok().map(Timestamp::unix_seconds)
}

fn finished(last: &Value) -> bool {
    let Some(parts) = last["message"]["content"].as_array().filter(|_| last["type"] == "assistant") else { return false };
    let text = texts(last);
    let length = text.chars().count();
    !parts.iter().any(|part| part["type"] == "tool_use") && length > 0 && (length > 200 || !WAITING.is_match(&text))
}

/// The minutes the brief's own `ETA:` head line states, else the first ETA its text names.
pub fn eta(brief: &str) -> Value {
    STATED_ETA.captures(brief).or_else(|| ETA.captures(brief)).map_or(Value::Null, |found| {
        let amount: f64 = found[1].parse().unwrap_or(0.0);
        number(amount * if found[2].starts_with('h') { 60.0 } else { 1.0 })
    })
}

fn tokens(assistant: &[&Value]) -> i64 {
    let mut last: HashMap<String, &Value> = HashMap::new();
    for record in assistant {
        let key = [&record["requestId"], &record["message"]["id"]].into_iter().find(|value| present(value)).unwrap_or(&Value::Null);
        last.insert(key.to_string(), record);
    }
    last.values().map(|record| int(&record["message"]["usage"]["output_tokens"])).sum()
}

fn report(assistant: &[&Value]) -> Option<String> {
    let reports: Vec<String> = assistant
        .iter()
        .filter(|record| record["message"]["content"].as_array().is_some_and(|parts| parts.iter().any(|part| part["type"] == "text")))
        .map(|record| texts(record))
        .collect();
    reports[reports.len().saturating_sub(3)..].iter().max_by_key(|text| text.chars().count()).cloned()
}

fn human(record: &Value) -> bool {
    record["type"] == "user"
        && match &record["message"]["content"] {
            Value::String(_) => true,
            Value::Array(parts) => !parts.iter().any(|part| part["type"] == "tool_result"),
            _ => false,
        }
}

/// Tracks the background commands a line starts (a tool result naming its id, or a `north job-run --notify` that runs
/// detached) and ends (a task notification, or the job-run exit message), so a
/// turn that ends while one still runs is read as a wait north will wake, whatever its last line says.
fn background(line: &Value, running: &mut BTreeSet<String>) {
    let raw = line["message"]["content"].to_string();
    running.extend(BACKGROUNDED.captures_iter(&raw).filter_map(|found| found.iter().skip(1).flatten().next().map(|id| id.as_str().to_owned())));
    for id in NOTIFIED.captures_iter(&raw).filter_map(|found| found.iter().skip(1).flatten().next()) {
        running.remove(id.as_str());
    }
}

/// One turn of a transcript: the lines from one human line to the next.
#[derive(Default)]
struct Turn<'a> {
    /// north will follow it with another turn: cut short by an interrupt or a delivered message, opened by one, or
    /// ended while a background command ran.
    aside: bool,
    /// Opened only because something the session waited on ended: a task notification, a job-run exit or a wait's end.
    woken: bool,
    /// Ended while a background command the session started still ran.
    pending: bool,
    lines: Vec<&'a Value>,
}

fn woken(line: &Value, said: &str) -> bool {
    line["origin"]["kind"] == "task-notification"
        || said.starts_with("<task-notification>")
        || NOTIFIED.captures(said).is_some_and(|found| found.get(2).is_some())
        || crate::fleet::resume::WAITS.iter().any(|opener| said.contains(opener))
}

/// The transcript cut at each human line, and whether a background command still runs at its end.
fn segments(lines: &[Value]) -> (Vec<Turn<'_>>, bool) {
    let mut turns: Vec<Turn> = vec![Turn::default()];
    let mut running: BTreeSet<String> = BTreeSet::new();
    for line in lines {
        let said = human(line).then(|| texts(line));
        let delivered = said.as_deref().is_some_and(|said| DELIVERED.iter().any(|opening| said.starts_with(opening)));
        if let Some(said) = &said {
            end_turn(&mut turns, &running, delivered || said.starts_with(INTERRUPTED));
        }
        background(line, &mut running);
        match said {
            Some(said) => turns.push(Turn { aside: delivered, woken: woken(line, &said), ..Turn::default() }),
            None => turns.last_mut().map_or((), |turn| turn.lines.push(line)),
        }
    }
    end_turn(&mut turns, &running, false);
    (turns, !running.is_empty())
}

fn end_turn(turns: &mut [Turn], running: &BTreeSet<String>, cut: bool) {
    if let Some(turn) = turns.last_mut() {
        turn.pending = !running.is_empty();
        turn.aside |= cut || turn.pending;
    }
}

/// The run's report and outcome: the latest of its best-ranked turn reports, where a turn a wake-up opened and that ran no
/// tool never replaces an earlier report that stands (one with an opener and a final outcome, written with nothing left running).
fn turn_report(lines: &[Value]) -> Option<(String, &'static str)> {
    let (turns, waiting) = segments(lines);
    let rank = |outcome: &str| match outcome {
        "idle" => 0,
        "unclear" => 1,
        _ => 2,
    };
    let mut standing = false;
    let mut reports: Vec<(String, &'static str)> = Vec::new();
    for turn in &turns {
        let assistant: Vec<&Value> = turn.lines.iter().copied().filter(|line| line["type"] == "assistant" && !line["message"]["usage"].is_null()).collect();
        let Some(report) = report(&assistant) else { continue };
        let quiet = quiet_run(turn.lines.iter().copied());
        if turn.woken && quiet && standing {
            continue;
        }
        let opener = crate::platform::attribution::has_opener(&report);
        let outcome = match outcome::classify(&report, quiet) {
            "unclear" | "not done" if turn.aside && !opener => "idle",
            outcome => outcome,
        };
        standing |= opener && rank(outcome) == 2 && !turn.pending;
        reports.push((report, outcome));
    }
    let best = reports.iter().map(|(_, outcome)| rank(outcome)).max()?;
    let (report, outcome) = reports.into_iter().rev().find(|(_, outcome)| rank(outcome) == best)?;
    Some((report, if waiting && outcome != "done" { "idle" } else { outcome }))
}

pub(super) fn build(transcript: &Transcript) -> Option<Map<String, Value>> {
    let lines = transcript.lines;
    let assistant: Vec<&Value> = lines.iter().filter(|line| line["type"] == "assistant" && !line["message"]["usage"].is_null()).collect();
    let last_usage = assistant.last()?;
    let tier = match transcript.role {
        "worker" => transcript.tier.to_owned(),
        _ => last_usage["message"]["model"].as_str().unwrap_or("unknown").to_owned(),
    };
    let last = lines.iter().rev().find(|line| line["type"] == "user" || line["type"] == "assistant")?;
    let done = finished(last);
    let brief = lines.iter().find(|line| line["type"] == "user").map_or_else(|| "null".to_owned(), texts);
    let times: Vec<&Value> = lines.iter().map(|line| &line["timestamp"]).filter(|at| present(at)).collect();
    let ended = times.last()?.as_str()?;
    if ended < transcript.since {
        return None;
    }
    let started = times[0].clone();
    let mut row = Map::new();
    row.insert("id".into(), json!(transcript.id));
    row.insert("session".into(), lines[0]["sessionId"].clone());
    row.insert("brief".into(), json!(prefix(&brief, 4000)));
    row.insert("started".into(), started.clone());
    row.insert("tier".into(), json!(tier));
    row.insert("eta_min".into(), eta(&brief));
    if !done && transcript.role == "worker" {
        let running = transcript.now.unix_seconds() - seconds(ended)? < RUNNING_SECONDS;
        if running || transcript.backfill {
            row.insert("status".into(), json!(if running { "running" } else { "stale" }));
            row.insert("backfill".into(), json!(transcript.backfill));
            return Some(row);
        }
    }
    let (report, outcome) = turn_report(lines)?;
    let head = prefix(&report, 600);
    let usage = &last_usage["message"]["usage"];
    row.insert("status".into(), json!(if done || transcript.role == "worker" { "finished" } else { "active" }));
    row.insert("ended".into(), json!(ended));
    row.insert("actual_min".into(), json!((seconds(ended)? - seconds(started.as_str()?)?).div_euclid(60)));
    activity::insert(&mut row, &activity::measure(lines), seconds(ended)? - seconds(started.as_str()?)?);
    row.insert("tokens".into(), json!(tokens(&assistant)));
    row.insert("peak_ctx".into(), json!(int(&usage["input_tokens"]) + int(&usage["cache_read_input_tokens"]) + int(&usage["cache_creation_input_tokens"])));
    row.insert("outcome".into(), json!(outcome));
    row.insert("flaws".into(), FLAWS.captures(&report).map_or(Value::Null, |found| number(found[1].parse().unwrap_or(0.0))));
    row.insert("backfill".into(), json!(transcript.backfill));
    row.insert("report".into(), json!(prefix(&SPACE.replace_all(&head, " "), 600)));
    row.insert("stop".into(), json!(outcome::stop_line(&report)));
    if brief != "null" {
        row.insert("brief_text".into(), json!(brief));
        row.insert("brief_origin".into(), json!(TRANSCRIPT_ORIGIN));
    }
    Some(row)
}

fn quiet_run<'a>(lines: impl Iterator<Item = &'a Value>) -> bool {
    !lines
        .filter(|line| line["type"] == "assistant")
        .any(|line| line["message"]["content"].as_array().is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_use")))
}
