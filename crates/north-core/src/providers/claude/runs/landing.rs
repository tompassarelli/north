use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use super::truthy;
use crate::fleet::landings::Push;
use crate::foundation::clock::Timestamp;

static LAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[;&|(\n])\s*(?:\w+=\S*\s+)*(?:(?:sudo|time|nohup|command|timeout\s+\S+)\s+)?(?:\S*/)?(?:safe-push|git(?:\s+-C\s+\S+)?\s+push)\b").unwrap()
});
static COMMIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b[^|;&\n]*?\scommit\b|(?:^|[;&|(\n])\s*\$\{?\w+\}?\s+commit\b").unwrap());
static SUBJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s-[a-zA-Z]*m[ =]?\s*(?:"\$\(cat <<-?['"]?\w+['"]?\s*\n)?["']?([^"'\n]+)"#).unwrap()
});
static HEREDOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\s-[a-zA-Z]*F\s*-\s*<<-?['"]?\w+['"]?[^\n]*\n([^\n]+)"#).unwrap());
static LANE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^([0-9a-f]{7,40}) (.+)$").unwrap());
static MADE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\[[^\]\s]+(?: \([^)]*\))? ([0-9a-f]{7,40})\] (.+)$").unwrap());
static PUSHED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(?:\s*\d+\t)?safe-push: (?:landed|✓ Autoland landed this lane: origin \S+ is at) ([0-9a-f]{40})\b").unwrap());
static ISSUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:tompassarelli/)?([a-z][\w.-]*#\d+)\b").unwrap());
static GH_ISSUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgh issue (?:view|close|comment|edit|reopen) (\d+)\b.*?(?:-R|--repo)[ =](?:[\w-]+/)?([\w.-]+)").unwrap()
});
static THREADS_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:\S+/)?threads (?:show|claim|release) ").unwrap());

pub struct MainIndex {
    shas: Vec<(String, String, String)>,
    subjects: HashMap<String, Vec<(String, String)>>,
}

impl MainIndex {
    pub fn parse(text: &str) -> Self {
        let mut shas: Vec<(String, String, String)> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        let mut subjects: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(3, ' ');
            let (sha, date, subject) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            match positions.get(sha) {
                Some(&position) => shas[position].1 = date.to_owned(),
                None => {
                    positions.insert(sha.to_owned(), shas.len());
                    shas.push((sha.to_owned(), date.to_owned(), subject.trim().to_owned()));
                }
            }
            subjects.entry(subject.trim().to_owned()).or_default().push((sha.to_owned(), date.to_owned()));
        }
        Self { shas, subjects }
    }

    /// The commit a push put on main: the pushed tip itself, else a commit with the tip's subject. A lander that
    /// logged its trunk commit cherry-picked the lane before checking and pushing, so the lane's own commit is the
    /// latest with that subject at or before that trunk commit; without one (Autoland, another host's lander) it is
    /// the first committed at or after the push.
    pub fn reached(&self, oid: &str, subject: Option<&str>, pushed_ms: u64, trunk: &str) -> Option<String> {
        if oid.len() >= 7 && let Some((sha, ..)) = self.shas.iter().find(|(sha, ..)| sha.starts_with(oid)) {
            return Some(sha.clone());
        }
        let landings = self.subjects.get(subject?.trim())?;
        let dated = landings.iter().filter_map(|(sha, date)| utc(Some(date)).map(|date| (date, sha)));
        let trunk_date = (trunk.len() >= 7).then(|| self.shas.iter().find(|(sha, ..)| sha.starts_with(trunk))).flatten().and_then(|(_, date, _)| utc(Some(date)));
        match trunk_date {
            Some(trunk) => dated.filter(|(date, _)| *date <= trunk).max().map(|(_, sha)| sha.clone()),
            None => {
                let pushed = Timestamp::from_unix_millis(pushed_ms as i64).iso();
                dated.filter(|(date, _)| *date >= pushed).min().map(|(_, sha)| sha.clone())
            }
        }
    }
}

/// The pushes with the commit each put on main: the commit [`MainIndex::reached`] finds for its tip, else the trunk
/// commit safe-push logged, which for a batched landing is the batch's tip that every lane in it shares.
pub fn settle(pushes: Vec<Push>, main: &MainIndex, subject: impl Fn(&Push) -> Option<String>) -> Vec<Push> {
    let newest = main.shas.iter().filter_map(|(_, date, _)| utc(Some(date))).max();
    let indexed = |push: &Push| newest.as_deref().is_some_and(|newest| Timestamp::from_unix_millis(push.at_ms as i64).iso().as_str() <= newest);
    pushes
        .into_iter()
        .map(|mut push| {
            if !push.oid.is_empty() {
                let rebased = || indexed(&push).then(|| main.reached("", subject(&push).as_deref(), push.at_ms, &push.landed)).flatten();
                if let Some(own) = main.reached(&push.oid, None, push.at_ms, "").or_else(rebased) {
                    push.landed = own;
                }
            }
            push
        })
        .collect()
}

/// The main checkouts whose origin/main the ledger reads: `LEDGER_REPOS`, else `~/code/*/main`.
pub fn repositories() -> Vec<PathBuf> {
    if let Some(listed) = std::env::var("LEDGER_REPOS").ok().filter(|listed| !listed.is_empty()) {
        return listed.split_whitespace().map(PathBuf::from).collect();
    }
    let code = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code");
    let mut found: Vec<PathBuf> = std::fs::read_dir(code).into_iter().flatten().flatten().map(|entry| entry.path().join("main")).collect();
    found.sort();
    found
}

/// The subject of a pushed tip, read from the checkout of the repository it was pushed to.
pub fn subject_in(repositories: &[PathBuf], push: &Push) -> Option<String> {
    let named = |repo: &&PathBuf| checkout_name(repo) == Some(push.repository.as_str());
    repositories.iter().filter(named).find_map(|repo| {
        let output = Command::new("git").arg("-C").arg(repo).args(["log", "-1", "--format=%s", &push.oid, "--"]).stderr(Stdio::null()).output().ok()?;
        let subject = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (output.status.success() && !subject.is_empty()).then_some(subject)
    })
}

fn checkout_name(repo: &Path) -> Option<&str> {
    let named = if repo.file_name().is_some_and(|name| name == "main") { repo.parent()? } else { repo };
    named.file_name()?.to_str()
}

pub struct Step {
    pub command: Option<String>,
    pub code: Option<i64>,
    pub output: Option<String>,
    pub finished: Option<String>,
}

pub fn utc(text: Option<&str>) -> Option<String> {
    text.filter(|text| !text.is_empty()).and_then(|text| Timestamp::parse(text).ok()).map(Timestamp::iso)
}

pub(super) fn checks(records: &[Value], main: &MainIndex) -> Map<String, Value> {
    let mut calls: HashMap<String, usize> = HashMap::new();
    let mut steps: Vec<Step> = Vec::new();
    let (mut staffed, mut pending, mut review): (Option<&str>, Option<&str>, Option<&str>) = (None, None, None);
    for record in records {
        staffed = staffed.or_else(|| record["timestamp"].as_str().filter(|at| !at.is_empty()));
        let content = &record["message"]["content"];
        let texted = |parts: &Value| parts.as_array().is_some_and(|parts| parts.iter().any(|part| part["type"] == "text"));
        if record["type"] == "user" && !truthy(&record["isMeta"]) && (content.is_string() || texted(content)) {
            review = review.or(pending);
        }
        if let Some(notice) = notice(record) {
            steps.push(Step { command: None, code: Some(0), output: Some(notice.to_owned()), finished: record["timestamp"].as_str().map(str::to_owned) });
        }
        let Some(parts) = content.as_array() else { continue };
        if record["type"] == "assistant" {
            if parts.iter().any(|part| part["type"] == "tool_use") {
                pending = None;
            } else if texted(content) {
                pending = record["timestamp"].as_str();
            }
        }
        for part in parts {
            let id = part["id"].to_string();
            if part["type"] == "tool_use" {
                let command = (part["name"] == "Bash").then(|| part["input"]["command"].as_str().map(str::to_owned)).flatten();
                calls.insert(id, steps.len());
                steps.push(Step { command, code: None, output: None, finished: None });
            } else if part["type"] == "tool_result"
                && let Some(&index) = calls.get(&part["tool_use_id"].to_string())
            {
                let step = &mut steps[index];
                step.code = Some(i64::from(truthy(&part["is_error"])));
                if step.output.is_none() {
                    step.output = Some(part["content"].as_str().map_or_else(|| python_dumps(&part["content"]), str::to_owned));
                    step.finished = record["timestamp"].as_str().map(str::to_owned);
                }
            }
        }
    }
    let mut out = tally(&steps, main);
    out.insert("staffed".into(), json!(utc(staffed)));
    out.insert("review_requested".into(), json!(utc(review.or(pending))));
    out
}

fn notice(record: &Value) -> Option<&str> {
    let (attachment, origin) = (&record["attachment"], &record["origin"]["kind"]);
    if record["type"] == "attachment" && attachment["type"] == "queued_command" && attachment["commandMode"] == "task-notification" {
        return attachment["prompt"].as_str();
    }
    (record["type"] == "user" && origin == "task-notification").then(|| record["message"]["content"].as_str()).flatten()
}

pub fn tally(steps: &[Step], main: &MainIndex) -> Map<String, Value> {
    let (mut pushed, mut committed, mut first_commit) = (false, false, None);
    let (mut made_shas, mut made_subjects, mut listed_subjects, mut refs) = (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new());
    for step in steps {
        let Some(command) = step.command.as_deref().filter(|command| !command.is_empty()) else {
            if committed && step.code == Some(0) {
                made_shas.extend(PUSHED.captures_iter(step.output.as_deref().unwrap_or("")).map(|landed| landed[1].to_owned()));
            }
            continue;
        };
        let succeeded = step.code == Some(0);
        pushed |= succeeded && LAND.is_match(command);
        if let Some(found) = COMMIT.find(command).filter(|_| step.code.unwrap_or(0) == 0) {
            committed = true;
            first_commit = first_commit.or(step.finished.as_deref());
            refs.extend(ISSUE.captures_iter(&command[found.end()..]).map(|c| c[1].to_owned()));
            if let Some(subject) = SUBJECT.captures_at(command, found.end()).or_else(|| HEREDOC.captures_at(command, found.end())) {
                made_subjects.push(subject[1].trim().to_owned());
            }
            for made in MADE.captures_iter(step.output.as_deref().unwrap_or("")) {
                made_shas.push(made[1].to_owned());
                made_subjects.push(made[2].trim().to_owned());
            }
        } else if committed && succeeded && command.contains("origin/main..") && let Some(output) = &step.output {
            listed_subjects.extend(LANE.captures_iter(output).map(|lane| lane[2].trim().to_owned()));
        } else if succeeded && THREADS_REF.is_match(command) {
            refs.extend(ISSUE.captures_iter(command).map(|c| c[1].to_owned()));
        }
        if committed && succeeded {
            made_shas.extend(PUSHED.captures_iter(step.output.as_deref().unwrap_or("")).map(|landed| landed[1].to_owned()));
        }
        if succeeded {
            refs.extend(GH_ISSUE.captures_iter(command).map(|c| format!("{}#{}", &c[2], &c[1])));
        }
    }
    let made = landed_commits(main, &made_shas, &made_subjects, first_commit);
    let commits = if made.is_empty() { landed_commits(main, &[], &listed_subjects, first_commit) } else { made };
    landing(&commits, committed || pushed, refs)
}

fn landed_commits<'a>(main: &'a MainIndex, made_shas: &[String], made_subjects: &[String], first_commit: Option<&str>) -> Vec<(String, Option<String>, &'a str)> {
    let mut seen_subjects: Vec<&String> = Vec::new();
    for subject in made_subjects {
        if !seen_subjects.contains(&subject) {
            seen_subjects.push(subject);
        }
    }
    let mut on_main: Vec<(String, Option<String>, &str)> = main
        .shas
        .iter()
        .filter(|(sha, _, _)| made_shas.iter().any(|prefix| sha.starts_with(prefix.as_str())))
        .map(|(sha, date, subject)| (sha.clone(), utc(Some(date)), subject.as_str()))
        .collect();
    for subject in seen_subjects {
        if let Some((subject, landings)) = main.subjects.get_key_value(subject) {
            on_main.extend(landings.iter().map(|(sha, date)| (sha.clone(), utc(Some(date)), subject.as_str())));
        }
    }
    if let Some(first) = first_commit {
        let day: String = utc(Some(first)).unwrap_or_default().chars().take(11).collect();
        on_main.retain(|(_, date, _)| date.as_deref().is_some_and(|date| date >= day.as_str()));
    }
    on_main
}

fn landing(on_main: &[(String, Option<String>, &str)], expects: bool, refs: Vec<String>) -> Map<String, Value> {
    let landing = on_main.iter().min_by(|left, right| (&left.1, &left.0).cmp(&(&right.1, &right.0)));
    let landed_ref = landing.and_then(|(_, _, subject)| ISSUE.captures_iter(subject).last()).map(|found| found[1].to_owned());
    let mut ranked: Vec<&String> = Vec::new();
    for reference in &refs {
        if !ranked.contains(&reference) {
            ranked.push(reference);
        }
    }
    let count = |reference: &String| refs.iter().filter(|other| *other == reference).count();
    ranked.sort_by_key(|reference| std::cmp::Reverse(count(reference)));
    let mut out = Map::new();
    out.insert("landed".into(), json!(i32::from(landing.is_some())));
    out.insert("expects_landing".into(), json!(i32::from(expects)));
    out.insert("landed_sha".into(), json!(landing.map(|(sha, _, _)| sha)));
    out.insert("landed_at".into(), json!(landing.and_then(|(_, date, _)| date.as_ref())));
    out.insert("landed_ref".into(), json!(landed_ref));
    out.insert("refs".into(), json!(ranked));
    out
}

fn python_dumps(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let mut out = String::from("\"");
            for unit in text.encode_utf16() {
                match unit {
                    0x22 => out.push_str("\\\""),
                    0x5c => out.push_str("\\\\"),
                    0x0a => out.push_str("\\n"),
                    0x0d => out.push_str("\\r"),
                    0x09 => out.push_str("\\t"),
                    0x08 => out.push_str("\\b"),
                    0x0c => out.push_str("\\f"),
                    0x20..=0x7e => out.push(unit as u8 as char),
                    _ => out.push_str(&format!("\\u{unit:04x}")),
                }
            }
            out.push('"');
            out
        }
        Value::Array(items) => format!("[{}]", items.iter().map(python_dumps).collect::<Vec<_>>().join(", ")),
        Value::Object(entries) => format!(
            "{{{}}}",
            entries.iter().map(|(key, item)| format!("{}: {}", python_dumps(&Value::from(key.as_str())), python_dumps(item))).collect::<Vec<_>>().join(", ")
        ),
        other => other.to_string(),
    }
}
