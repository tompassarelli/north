//! `north land gate-stats`: each land gate's refusals over a window and what they cost the workers, read from the
//! lander's done tickets, its after-landing logs and the enqueue refusal log, with a deletion proposal for a gate
//! whose refused lanes mostly landed later with the same patch.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

const USAGE: &str = "usage: north land gate-stats [--since 7d|48h]";
const UNCHANGED_SHARE: f64 = 0.5;
const UNCHANGED_FLOOR: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct Ticket {
    pub micros: u64,
    pub root: String,
    pub branch: String,
    pub head: String,
    pub verdict: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Row {
    pub refusals: usize,
    pub requeued: Vec<f64>,
    pub landed_later: usize,
    pub unchanged: usize,
}

pub fn gate(verdict: &str, log_line: impl Fn(&str) -> Option<String>) -> Option<String> {
    if verdict.starts_with("landed") || verdict.starts_with("replaced") || verdict.starts_with("conflict") {
        return None;
    }
    let rest = verdict.strip_prefix("failed ").unwrap_or(verdict);
    if let Some(command) = rest.strip_prefix("land check ") {
        return Some(check_name(command));
    }
    if let Some(path) = rest.strip_prefix('/').map(|_| rest.split_whitespace().next().unwrap_or(rest)) {
        return Some(log_line(path).unwrap_or_else(|| "lander scan".to_owned()));
    }
    Some(rest.split([';', ':']).next().unwrap_or(rest).trim().to_owned())
}

fn check_name(command: &str) -> String {
    for (marker, name) in [("cargo clippy", "clippy"), ("cargo nextest", "nextest"), ("VerbKinds", "verbs sort"), ("crates.txt", "tests/it module list")] {
        if command.contains(marker) {
            return name.to_owned();
        }
    }
    let words: Vec<&str> = command.split_whitespace().collect();
    let Some(start) = words.iter().position(|word| word.ends_with("head-north")) else { return command.chars().take(32).collect() };
    let mut rest = words[start + 1..].iter().copied();
    let mut named = Vec::new();
    while let Some(word) = rest.next() {
        match word {
            "--code" => {
                rest.next();
            }
            "--no-build" => {}
            _ if word.starts_with('-') || word.starts_with('$') || named.len() == 3 => break,
            _ => named.push(word),
        }
    }
    named.join(" ")
}

pub fn first_refusal_line(log: &str) -> Option<String> {
    log.lines().find_map(|line| line.split_once("✗ ").map(|(_, rest)| rest.chars().take(48).collect::<String>().trim_end().to_owned()))
}

pub fn tally(tickets: &[Ticket], since_micros: u64, log_line: impl Fn(&str) -> Option<String>, same_patch: impl Fn(&Ticket, &Ticket) -> bool) -> BTreeMap<String, Row> {
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    for ticket in tickets.iter().filter(|ticket| ticket.micros >= since_micros) {
        let Some(name) = gate(&ticket.verdict, &log_line) else { continue };
        let row = rows.entry(name).or_default();
        row.refusals += 1;
        let later: Vec<&Ticket> = tickets.iter().filter(|other| other.root == ticket.root && other.branch == ticket.branch && other.micros > ticket.micros).collect();
        if let Some(next) = later.iter().min_by_key(|other| other.micros) {
            row.requeued.push((next.micros - ticket.micros) as f64 / 60e6);
        }
        if let Some(landed) = later.iter().filter(|other| other.verdict.starts_with("landed")).min_by_key(|other| other.micros) {
            row.landed_later += 1;
            row.unchanged += usize::from(same_patch(ticket, landed));
        }
    }
    rows
}

fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len().checked_sub(1)? / 2).copied()
}

pub fn proposed(row: &Row) -> bool {
    row.landed_later >= UNCHANGED_FLOOR && row.unchanged as f64 >= row.landed_later as f64 * UNCHANGED_SHARE
}

pub fn render(window: &str, rows: &BTreeMap<String, Row>, after: &BTreeMap<String, usize>, enqueue: &BTreeMap<String, usize>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "land gate refusals, last {window}; cost is worker wall time from a refusal to the same lane's next ticket");
    let _ = writeln!(out, "{:<34} {:>8} {:>8} {:>10} {:>8} {:>12} {:>9}", "gate", "refusals", "requeued", "median-min", "worker-h", "landed-later", "unchanged");
    let mut ordered: Vec<(&String, &Row)> = rows.iter().collect();
    ordered.sort_by(|left, right| right.1.refusals.cmp(&left.1.refusals).then_with(|| left.0.cmp(right.0)));
    for (name, row) in &ordered {
        let hours: f64 = row.requeued.iter().sum::<f64>() / 60.0;
        let middle = median(&row.requeued).map_or_else(|| "-".to_owned(), |minutes| format!("{minutes:.1}"));
        let name: String = name.chars().take(34).collect();
        let _ = writeln!(out, "{name:<34} {:>8} {:>8} {middle:>10} {hours:>8.1} {:>12} {:>9}", row.refusals, row.requeued.len(), row.landed_later, row.unchanged);
    }
    let listed = |counts: &BTreeMap<String, usize>| counts.iter().map(|(name, count)| format!("{name} {count}")).collect::<Vec<_>>().join(", ");
    let _ = writeln!(out, "after-landing gates (no revert; each failure is a section of the After-landing gate debt issue): {}", if after.is_empty() { "none".to_owned() } else { listed(after) });
    let _ = writeln!(out, "enqueue refusals (inside the worker's turn): {}", if enqueue.is_empty() { "none recorded".to_owned() } else { listed(enqueue) });
    for (name, row) in ordered.iter().filter(|(_, row)| proposed(row)) {
        let _ = writeln!(out, "proposed for deletion: {name}: {} of its {} refused lanes landed later with the same patch, so the refusal protected nothing there", row.unchanged, row.landed_later);
    }
    out
}

fn queues(state: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(state.join("north/land-queue")).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.join("done").is_dir()).collect();
    found.sort();
    found
}

fn tickets(queue: &Path) -> Vec<Ticket> {
    std::fs::read_dir(queue.join("done"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| serde_json::from_str::<Value>(&std::fs::read_to_string(entry.path()).ok()?).ok())
        .filter_map(|ticket| {
            let text = |key: &str| ticket[key].as_str().unwrap_or_default().to_owned();
            Some(Ticket { micros: ticket["queued_micros"].as_u64()?, root: text("root"), branch: text("branch"), head: text("head"), verdict: text("verdict") })
        })
        .collect()
}

fn patch_id(repository: &Path, commit: &str) -> Option<String> {
    let diff = Command::new("git").arg("-C").arg(repository).args(["diff-tree", "-p", commit]).stderr(Stdio::null()).output().ok()?;
    let mut child = Command::new("git").args(["patch-id", "--stable"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().ok()?;
    std::io::Write::write_all(child.stdin.as_mut()?, &diff.stdout).ok()?;
    let output = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&output.stdout).split_whitespace().next().map(str::to_owned)
}

fn repository(root: &str) -> PathBuf {
    let root = Path::new(root);
    root.ancestors().find(|dir| dir.file_name().is_some_and(|name| name == "worktrees")).and_then(Path::parent).map_or_else(|| root.to_path_buf(), |project| project.join("main"))
}

fn after_failures(queue: &Path, since: SystemTime, counts: &mut BTreeMap<String, usize>) {
    for entry in std::fs::read_dir(queue.join("logs")).into_iter().flatten().flatten() {
        let path = entry.path();
        let recent = entry.metadata().and_then(|metadata| metadata.modified()).is_ok_and(|modified| modified >= since);
        if !recent || !path.file_name().is_some_and(|name| name.to_string_lossy().contains("follow")) {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
            let named = line.strip_prefix("after-landing: gate ").or_else(|| line.strip_prefix("after-landing: check "));
            if let Some(name) = named.and_then(|rest| rest.strip_suffix(" failed")) {
                *counts.entry(name.to_owned()).or_default() += 1;
            }
        }
    }
}

fn enqueue_refusals(state: &Path, since_micros: u64) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for line in std::fs::read_to_string(state.join(north_landing::land::enqueue::REFUSALS)).unwrap_or_default().lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else { continue };
        if row["micros"].as_u64().is_some_and(|micros| micros >= since_micros) {
            *counts.entry(enqueue_gate(row["error"].as_str().unwrap_or("unknown"))).or_default() += 1;
        }
    }
    counts
}

pub fn enqueue_gate(error: &str) -> String {
    let reason = error.split(';').next().unwrap_or(error);
    match reason.split_once(": ") {
        Some((head, rest)) if head.contains("check before queueing") => check_name(rest),
        _ => reason.chars().take(48).collect::<String>().trim_end().to_owned(),
    }
}

fn window(arguments: &[String]) -> Option<(String, u64)> {
    let text = match arguments {
        [] => "7d",
        [flag, value] if flag == "--since" => value.as_str(),
        _ => return None,
    };
    let (number, unit) = text.split_at(text.len().checked_sub(1)?);
    let seconds = number.parse::<u64>().ok()? * match unit { "d" => 86_400, "h" => 3_600, _ => return None };
    Some((text.to_owned(), seconds))
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((label, seconds)) = window(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let records = north_base::platform::records_mirror::read("north land gate-stats");
    let state = records.home();
    let now = SystemTime::now();
    let since = now - std::time::Duration::from_secs(seconds);
    let since_micros = since.duration_since(UNIX_EPOCH).map_or(0, |elapsed| u64::try_from(elapsed.as_micros()).unwrap_or(0));
    let (mut rows, mut after): (BTreeMap<String, Row>, BTreeMap<String, usize>) = (BTreeMap::new(), BTreeMap::new());
    for queue in queues(state) {
        let found = tickets(&queue);
        let log_line = |path: &str| first_refusal_line(&std::fs::read_to_string(records.local(Path::new(path))).ok()?);
        let same = |refused: &Ticket, landed: &Ticket| {
            let repository = repository(&refused.root);
            patch_id(&repository, &refused.head).is_some_and(|id| patch_id(&repository, &landed.head).is_some_and(|other| other == id))
        };
        for (name, row) in tally(&found, since_micros, log_line, same) {
            let merged = rows.entry(name).or_default();
            merged.refusals += row.refusals;
            merged.requeued.extend(row.requeued);
            merged.landed_later += row.landed_later;
            merged.unchanged += row.unchanged;
        }
        after_failures(&queue, since, &mut after);
    }
    print!("{}", render(&label, &rows, &after, &enqueue_refusals(state, since_micros)));
    0
}
