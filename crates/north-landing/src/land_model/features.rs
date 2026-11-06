use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use super::{Class, Era, Lane};
use crate::land::queue;

const INFRA_MARKERS: [&str; 5] = ["hung:", "exited 255", "cannot lock ref", "timed out", "red-main"];
const LANDER_PATHS: [&str; 3] = ["crates/north-landing/", "crates/north-safe-push/", "tests/it/land"];
const TOP_LEVEL_AREAS: [&str; 5] = ["src", "tests", "harness", "bin", "docs"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub id: u128,
    pub branch: String,
    pub head: String,
    pub class: Class,
    pub decided: i64,
}

impl Record {
    pub fn queued(&self) -> i64 {
        (self.id / 1_000_000) as i64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub files: BTreeSet<String>,
    pub lines: u32,
    pub commits: u32,
    pub fork_time: i64,
    pub main_overlap: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Columns {
    pub shared_files: u32,
    pub depth: u32,
    pub history_seen: u32,
    pub history_bad: u32,
    pub prior_tries: u32,
    pub main_overlap: u32,
    pub lines: u32,
    pub files: u32,
    pub commits: u32,
    pub rust: u32,
    pub lander: u32,
    pub nondocs: u32,
    pub areas: u32,
    pub fork_age_minutes: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub lane: Lane,
    pub columns: Columns,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collected {
    pub rows: Vec<Row>,
    pub skipped: usize,
}

pub fn lanes(rows: &[Row]) -> Vec<Lane> {
    rows.iter().map(|row| row.lane.clone()).collect()
}

pub fn classify(verdict: &str, log: impl FnOnce(&Path) -> Option<String>) -> Option<Class> {
    let mut words = verdict.split_whitespace();
    match words.next()? {
        "landed" => Some(Class::Landed),
        "conflict" => Some(Class::Conflict),
        "replaced" => Some(Class::Replaced),
        "failed" => {
            let hung = words.next().and_then(|path| log(Path::new(path))).is_some_and(|text| INFRA_MARKERS.iter().any(|marker| text.contains(marker)));
            Some(if hung { Class::Infra } else { Class::Failed })
        }
        _ => None,
    }
}

pub fn load_done(directory: &Path) -> io::Result<Vec<Record>> {
    let mut records = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let Ok(ticket) = queue::read(&path) else { continue };
        let Some(class) = ticket.verdict.as_deref().and_then(|verdict| classify(verdict, |log| fs::read_to_string(log).ok())) else { continue };
        let decided = fs::metadata(&path)?.modified()?.duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64);
        records.push(Record { id: queue::queued_micros(&path), branch: ticket.branch, head: ticket.head, class, decided });
    }
    records.sort_by(|a, b| (a.id, &a.branch).cmp(&(b.id, &b.branch)));
    Ok(records)
}

pub fn collect(repository: &Path, base: &str, records: Vec<Record>) -> Collected {
    let mut kept = Vec::new();
    let mut skipped = 0;
    for record in records {
        match change(repository, base, &record) {
            Some(change) => kept.push((record, change)),
            None => skipped += 1,
        }
    }
    Collected { rows: rows(&kept), skipped }
}

pub fn change(repository: &Path, base: &str, record: &Record) -> Option<Change> {
    git(repository, &["cat-file", "-e", &format!("{}^{{commit}}", record.head)])?;
    let fork = git(repository, &["merge-base", &record.head, base])?.trim().to_owned();
    let range = format!("{fork}..{}", record.head);
    let files: BTreeSet<String> = git(repository, &["diff", "--no-renames", "--name-only", &fork, &record.head])?.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect();
    let lines = git(repository, &["diff", "--no-renames", "--numstat", &fork, &record.head])?.lines().map(numstat_lines).sum();
    let commits = git(repository, &["rev-list", "--count", &range])?.trim().parse().ok()?;
    let fork_time: i64 = git(repository, &["log", "-1", "--format=%ct", &fork])?.trim().parse().ok()?;
    let since = format!("--since=@{fork_time}");
    let until = format!("--until=@{}", record.queued());
    let main_changed: BTreeSet<String> = git(repository, &["log", "--format=", "--name-only", &since, &until, base])?.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect();
    let main_overlap = main_changed.intersection(&files).count() as u32;
    Some(Change { files, lines, commits, fork_time, main_overlap })
}

fn numstat_lines(line: &str) -> u32 {
    line.split('\t').take(2).map(|count| count.parse::<u32>().unwrap_or(0)).sum()
}

fn git(repository: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(repository).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn rows(entries: &[(Record, Change)]) -> Vec<Row> {
    entries.iter().map(|(record, change)| row(record, change, entries)).collect()
}

fn row(record: &Record, change: &Change, entries: &[(Record, Change)]) -> Row {
    let queued = record.queued();
    let mut shared_files = 0;
    let mut depth = 0;
    let mut history_seen = 0;
    let mut history_bad = 0;
    let mut prior_tries = 0;
    for (other, other_change) in entries {
        if other.id == record.id && other.branch == record.branch {
            continue;
        }
        if other.branch == record.branch {
            prior_tries += u32::from(other.queued() < queued);
            continue;
        }
        if other.class == Class::Replaced {
            continue;
        }
        let shared = change.files.intersection(&other_change.files).count() as u32;
        if other.queued() < queued && other.decided > queued {
            depth += 1;
            shared_files += shared;
        }
        if other.decided <= queued && matches!(other.class, Class::Landed | Class::Failed) && shared > 0 {
            history_seen += 1;
            history_bad += u32::from(other.class == Class::Failed);
        }
    }
    let files: Vec<&str> = change.files.iter().map(String::as_str).collect();
    let nondocs = files.iter().filter(|file| !is_docs(file)).count() as u32;
    let lander = files.iter().filter(|file| LANDER_PATHS.iter().any(|prefix| file.starts_with(prefix))).count() as u32;
    let areas = files.iter().filter_map(|file| area(file)).collect::<BTreeSet<_>>().len() as u32;
    let columns = Columns {
        shared_files,
        depth,
        history_seen,
        history_bad,
        prior_tries,
        main_overlap: change.main_overlap,
        lines: change.lines,
        files: files.len() as u32,
        commits: change.commits,
        rust: files.iter().filter(|file| file.ends_with(".rs") || file.contains("Cargo")).count() as u32,
        lander,
        nondocs,
        areas,
        fork_age_minutes: (queued - change.fork_time) / 60,
    };
    let lane = Lane {
        branch: record.branch.clone(),
        era: Era::at(record.decided),
        class: record.class,
        queued,
        decided: record.decided,
        shared_files,
        resubmit: prior_tries > 0,
        lander_paths: lander > 0,
        history_refused: history_bad > 0,
        docs_only: !files.is_empty() && nondocs == 0,
    };
    Row { lane, columns }
}

fn is_docs(file: &str) -> bool {
    file.starts_with("docs/") || file.ends_with(".md")
}

fn area(file: &str) -> Option<String> {
    let mut parts = file.split('/');
    let first = parts.next()?;
    if first == "crates" {
        return Some(format!("crates/{}", parts.next()?));
    }
    TOP_LEVEL_AREAS.contains(&first).then(|| first.to_owned())
}
