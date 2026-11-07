use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::NorthResult;

const FILE: &str = "promises.jsonl";
const CHECKED: &str = "promises-checked.jsonl";
static COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([A-Za-z0-9_./-]+(?: [^`]+)+)`").expect("command pattern"));
static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").expect("sha pattern"));
const BOX_MARKER: &str = "Done when:";
static UTC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(\d{4}-\d{2}-\d{2}T\d{2}:\d{2})(?::\d{2})?Z\b").expect("utc time pattern"));
static WALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(1[0-2]|0?[1-9]):([0-5]\d)\s?([AaPp])\.?[Mm]\b").expect("wall time pattern"));

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum When {
    Utc { at_ms: u64 },
    Wall { hour: u8, minute: u8 },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Promise {
    pub session: String,
    pub turn: u64,
    pub line: String,
    pub command: String,
    pub when: When,
    pub said_ms: u64,
    pub cwd: PathBuf,
}

impl Promise {
    pub fn key(&self) -> String {
        format!("{}:{}:{}", self.session, self.turn, self.command)
    }
}

pub fn promised(reply: &str) -> Vec<(String, String, When)> {
    reply
        .lines()
        .filter_map(|line| {
            let command = COMMAND.captures_iter(command_text(line)).last()?[1].trim().to_owned();
            Some((line.trim().to_owned(), command, when(line)?))
        })
        .collect()
}

fn command_text(line: &str) -> &str {
    let is_box = line.trim_start().starts_with("- [");
    match line.rsplit_once(BOX_MARKER) {
        Some((_, command_line)) if is_box => command_line,
        _ => line,
    }
}

pub fn landed_as(command: &str, events: &str) -> String {
    let mut landed = BTreeMap::new();
    let mut shas: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for event in events.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()) {
        let (Some(pid), Some(sha)) = (event["pid"].as_u64(), event["sha"].as_str().filter(|sha| !sha.is_empty())) else { continue };
        shas.entry(pid).or_default().insert(sha.to_owned());
        if event["status"] == "landed" {
            landed.insert(pid, sha.to_owned());
        }
    }
    SHA.replace_all(command, |found: &regex::Captures| {
        let token = &found[0];
        let rebased = landed.iter().find_map(|(pid, landed_sha)| shas[pid].iter().any(|sha| sha != landed_sha && sha.starts_with(token)).then_some(landed_sha));
        rebased.map_or_else(|| token.to_owned(), |landed_sha| landed_sha[..token.len()].to_owned())
    })
    .into_owned()
}

fn when(line: &str) -> Option<When> {
    if let Some(found) = UTC.captures(line) {
        let at = crate::foundation::clock::Timestamp::parse(&format!("{}:00Z", &found[1])).ok()?;
        return u64::try_from(at.unix_millis()).ok().map(|at_ms| When::Utc { at_ms });
    }
    let found = WALL.captures(line)?;
    let hour: u8 = found[1].parse().ok()?;
    let afternoon = found[3].eq_ignore_ascii_case("p");
    Some(When::Wall { hour: hour % 12 + if afternoon { 12 } else { 0 }, minute: found[2].parse().ok()? })
}

pub struct Book {
    root: PathBuf,
}

impl Book {
    pub fn at(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }

    pub fn keep(&self, promises: &[Promise]) -> NorthResult<()> {
        append(&self.root.join(FILE), promises)
    }

    pub fn open(&self) -> Vec<Promise> {
        let checked: BTreeSet<String> = lines(&self.root.join(CHECKED)).collect();
        lines(&self.root.join(FILE)).filter_map(|line| serde_json::from_str::<Promise>(&line).ok()).filter(|promise| !checked.contains(&promise.key())).collect()
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
