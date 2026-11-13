use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use super::checkpoint::Answer;
use super::runtime::PROXY;

const FILE: &str = "promises.jsonl";
const CHECKED: &str = "promises-checked.jsonl";
const FENCES: [&str; 2] = ["```", "~~~"];
const BOX_MARKER: &str = "Done when:";
const STOP: &str = "Stop:";
const SENTENCE_ENDS: [char; 4] = ['.', '!', '?', ';'];
const QUOTE_CHARS: usize = 240;
const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
static UTC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})(?::\d{2})?Z\b").expect("utc time pattern"));
static WALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\b(?i:(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec))[a-z]*\.?\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(?:at\s+)?)?\b(1[0-2]|0?[1-9]):([0-5]\d)\s?([AaPp])\.?[Mm]\b").expect("wall time pattern")
});

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Utc { at_ms: u64 },
    Wall {
        hour: u8,
        minute: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        date: Option<(u8, u8)>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Promise {
    pub session: String,
    pub turn: u64,
    pub line: String,
    pub command: String,
    pub when: When,
    pub said_ms: u64,
    pub parent: String,
}

pub trait Reckon: Send + Sync {
    fn moment(&self, when: &When, said_ms: u64) -> Option<u64>;
}

pub struct Unreckoned;

impl Reckon for Unreckoned {
    fn moment(&self, _when: &When, _said_ms: u64) -> Option<u64> {
        None
    }
}

impl Promise {
    pub fn key(&self) -> String {
        let time = match self.when {
            When::Utc { at_ms } => at_ms.to_string(),
            When::Wall { hour, minute, date: Some((month, day)) } => format!("{month}-{day}T{hour}:{minute}"),
            When::Wall { hour, minute, date: None } => format!("{hour}:{minute}"),
        };
        format!("{}:{}@{time}", self.session, self.command)
    }

    fn turn_key(&self) -> String {
        match self.when {
            When::Wall { date: Some((month, day)), .. } => format!("{}:{}:{}@{month}-{day}", self.session, self.turn, self.command),
            _ => format!("{}:{}:{}", self.session, self.turn, self.command),
        }
    }

    pub fn dated(mut self) -> Self {
        if matches!(self.when, When::Wall { date: None, .. })
            && let Some((when @ When::Wall { .. }, _)) = read(&self.line)
        {
            self.when = when;
        }
        self
    }
}

pub struct Claim {
    pub line: String,
    pub when: When,
    pub command: Option<String>,
}

pub fn promised(reply: &str, said_ms: u64, reckon: &dyn Reckon, resolves: &dyn Fn(&str) -> bool) -> Vec<Claim> {
    let mut fenced = false;
    reply
        .lines()
        .filter_map(|line| {
            if FENCES.iter().any(|fence| line.trim_start().starts_with(fence)) {
                fenced = !fenced;
                return None;
            }
            if fenced || line.trim_start().starts_with(STOP) || Answer::parse(line).is_some() {
                return None;
            }
            let spans = spans(command_text(line));
            if spans.is_empty() {
                return None;
            }
            let command = spans.into_iter().rev().map(str::trim).find(|span| runnable(span, resolves)).map(str::to_owned);
            let (when, at) = when(line, said_ms, reckon)?;
            Some(Claim { line: sentence(line, at), when, command })
        })
        .collect()
}

/// The inline code spans of `text`, paired the way Markdown pairs backtick runs: a run opens a span
/// only when a later run of the same length closes it, and the runs between them are its content,
/// so a quotation holding a single backtick inside a doubled span stays one span and a run with no
/// partner stays literal text instead of re-pairing the spans after it.
fn spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut at = 0;
    while let Some((open, length)) = run(text, at) {
        let mut search = open + length;
        let mut close = None;
        while let Some((start, width)) = run(text, search) {
            if width == length {
                close = Some(start);
                break;
            }
            search = start + width;
        }
        match close {
            Some(end) => {
                spans.push(code(&text[open + length..end]));
                at = end + length;
            }
            None => at = open + length,
        }
    }
    spans
}

/// The backtick run at or after byte `from`: where it starts and how many backticks it holds.
fn run(text: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < bytes.len() && bytes[at] != b'`' {
        at += 1;
    }
    if at == bytes.len() {
        return None;
    }
    let start = at;
    while at < bytes.len() && bytes[at] == b'`' {
        at += 1;
    }
    Some((start, at - start))
}

/// Markdown strips one space from each end of a span whose content starts and ends with one and is
/// not all spaces, so a doubled delimiter around a shorter span holds it without padded spaces.
fn code(content: &str) -> &str {
    let trimmed = content.strip_prefix(' ').and_then(|rest| rest.strip_suffix(' '));
    trimmed.filter(|inner| !inner.is_empty() && !inner.bytes().all(|byte| byte == b' ')).unwrap_or(content)
}

pub fn resolver<'a>(cwd: &'a Path, search: &'a OsStr) -> impl Fn(&str) -> bool + 'a {
    move |program: &str| {
        if !program.contains('/') {
            return std::env::split_paths(search).any(|dir| executable(&dir.join(program)));
        }
        executable(&match program.strip_prefix("~/") {
            Some(rest) => std::env::var_os("HOME").map_or_else(|| PathBuf::from(program), |home| PathBuf::from(home).join(rest)),
            None => cwd.join(program),
        })
    }
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0)
}

fn command_text(line: &str) -> &str {
    let is_box = line.trim_start().starts_with("- [");
    match line.rsplit_once(BOX_MARKER) {
        Some((_, command_line)) if is_box => command_line,
        _ => line,
    }
}

fn runnable(span: &str, resolves: &dyn Fn(&str) -> bool) -> bool {
    let mut words = span.split_whitespace();
    let Some(program) = words.next() else { return false };
    resolves(program) && (program.contains('/') || words.next().is_some())
}

/// The sentence of `line` holding byte `at`, at most `QUOTE_CHARS` long: what a note may quote of a report.
fn sentence(line: &str, at: usize) -> String {
    let ends = |index: usize, text: &str| text[index..].chars().next().is_some_and(|letter| SENTENCE_ENDS.contains(&letter)) && text[index + 1..].chars().next().is_none_or(char::is_whitespace);
    let start = line[..at].char_indices().rfind(|(index, _)| ends(*index, line)).map_or(0, |(index, _)| index + 1);
    let end = line[at..].char_indices().find(|(index, _)| ends(at + index, line)).map_or(line.len(), |(index, _)| at + index + 1);
    line[start..end].trim().chars().take(QUOTE_CHARS).collect()
}

fn when(line: &str, said_ms: u64, reckon: &dyn Reckon) -> Option<(When, usize)> {
    let (when, at) = read(line)?;
    let moment = match when {
        When::Utc { at_ms } => Some(at_ms),
        When::Wall { .. } => reckon.moment(&when, said_ms),
    };
    moment.is_none_or(|at_ms| at_ms > said_ms).then_some((when, at))
}

fn read(line: &str) -> Option<(When, usize)> {
    if let Some(found) = UTC.captures(line) {
        let at = crate::foundation::clock::Timestamp::parse(&format!("{}:00Z", &found[1])).ok()?;
        return u64::try_from(at.unix_millis()).ok().map(|at_ms| (When::Utc { at_ms }, found.get(0).map_or(0, |whole| whole.start())));
    }
    let found = WALL.captures(line)?;
    let date = found.get(1).and_then(|month| Some((MONTHS.iter().position(|name| month.as_str().eq_ignore_ascii_case(name))? as u8 + 1, found[2].parse().ok()?)));
    let hour: u8 = found[3].parse().ok()?;
    let afternoon = found[5].eq_ignore_ascii_case("p");
    Some((When::Wall { hour: hour % 12 + if afternoon { 12 } else { 0 }, minute: found[4].parse().ok()?, date }, found.get(0).map_or(0, |whole| whole.start())))
}

pub struct Book {
    root: PathBuf,
}

impl Book {
    pub fn at(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    pub fn keep(&self, promises: &[Promise]) -> NorthResult<()> {
        let to_leads: Vec<&Promise> = promises.iter().filter(|promise| promise.parent != PROXY).collect();
        append(&self.root.join(FILE), &to_leads)
    }

    pub fn open(&self) -> Vec<Promise> {
        let checked: BTreeSet<String> = lines(&self.root.join(CHECKED)).collect();
        let mut seen = BTreeSet::new();
        lines(&self.root.join(FILE))
            .filter_map(|line| serde_json::from_str::<Promise>(&line).ok())
            .map(Promise::dated)
            .filter(|promise| !checked.contains(&promise.key()) && !checked.contains(&promise.turn_key()))
            .filter(|promise| seen.insert(promise.key()))
            .collect()
    }

    pub fn checked(&self, keys: &[String]) -> NorthResult<()> {
        append(&self.root.join(CHECKED), keys)
    }
}

fn lines(path: &Path) -> impl Iterator<Item = String> {
    fs::read_to_string(path).unwrap_or_default().lines().map(str::to_owned).collect::<Vec<_>>().into_iter()
}

fn append<T: Serialize>(path: &Path, rows: &[T]) -> NorthResult<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let mut text = String::new();
    for row in rows {
        match serde_json::to_value(row)? {
            serde_json::Value::String(plain) => text.push_str(&plain),
            value => text.push_str(&value.to_string()),
        }
        text.push('\n');
    }
    fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
#[path = "promises_tests.rs"]
mod tests;
