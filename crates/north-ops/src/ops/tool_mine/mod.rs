mod sequence;
mod steps;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::process::Command;
use std::time::Instant;

use serde::Deserialize;

use crate::foundation::clock::Timestamp;
use crate::ops::detectors::{Detector, Filer, Spend, record};
use crate::ops::flow::{Machine, Places};

use sequence::{Limits, Session, recurring};

const USAGE: &str = "usage: north tool-mine [--days N] [--min-sessions N] [--min-steps N] [--max N] [--owner REPO#N] [--dry-run]";
const OWNER: &str = "north#112";

struct Options {
    limits: Limits,
    owner: String,
    dry_run: bool,
}

#[derive(Deserialize)]
struct Row {
    session: String,
    #[serde(default)]
    id: serde_json::Value,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { limits: Limits::default(), owner: OWNER.to_owned(), dry_run: false };
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        if flag == "--dry-run" {
            options.dry_run = true;
            continue;
        }
        let value = rest.next()?;
        match flag.as_str() {
            "--days" => options.limits.window_s = value.parse::<f64>().ok().filter(|days| *days > 0.0)? * 86_400.0,
            "--min-sessions" => options.limits.min_sessions = value.parse().ok().filter(|count| *count > 1)?,
            "--min-steps" => options.limits.min_steps = value.parse().ok().filter(|count| *count > 1 && *count <= options.limits.max_steps)?,
            "--max" => options.limits.max_filed = value.parse().ok().filter(|count| *count > 0)?,
            "--owner" => options.owner = value.clone(),
            _ => return None,
        }
    }
    Some(options)
}

fn at(row: &Row) -> f64 {
    row.ts.as_deref().and_then(|ts| Timestamp::parse(ts).ok()).map_or(0.0, Timestamp::unix_seconds_f64)
}

fn sessions(rows: &[Row]) -> Vec<Session> {
    let mut ordered: Vec<&Row> = rows.iter().collect();
    ordered.sort_by(|a, b| at(a).total_cmp(&at(b)).then_with(|| a.id.as_i64().cmp(&b.id.as_i64())));
    let mut grouped: BTreeMap<&str, Session> = BTreeMap::new();
    for row in ordered {
        let Some(command) = row.text.as_deref().and_then(steps::command_of) else { continue };
        let session = grouped.entry(&row.session).or_insert_with(|| Session { id: row.session.clone(), at: 0.0, steps: Vec::new() });
        session.at = session.at.max(at(row));
        session.steps.extend(steps::step(command));
    }
    grouped.into_values().collect()
}

fn transcripts(days: f64) -> Result<Vec<Row>, String> {
    let since = format!("{}d", days.ceil());
    let arguments = ["-r", "tool", "-m", "--since", &since, "-n", "1000000", "--json", "--full"];
    let output = Command::new("north").arg("convo").args(arguments).output().map_err(|error| format!("north convo: {error}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return Err(format!("north convo {} exited {:?}: {}", arguments.join(" "), output.status.code(), String::from_utf8_lossy(&output.stderr)));
    }
    if !output.stdout.trim_ascii_start().starts_with(b"[") {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("convo output: {error}"))
}

fn mine(options: &Options) -> Result<(), String> {
    let started = Instant::now();
    let now = Timestamp::now().unix_seconds_f64();
    let mined = recurring(&sessions(&transcripts(options.limits.window_s / 86_400.0)?), now, &options.limits);
    let findings: Vec<_> = mined.kept.iter().map(|pattern| pattern.finding(Some(&options.owner), &options.limits)).collect();
    let spend = Spend { detector: Detector::RecurringSequence, millis: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX), findings: findings.len() };
    for pattern in &mined.kept {
        println!("tool-mine: {} sessions: {} -> {}", pattern.count(), pattern.sequence(), pattern.subcommand());
    }
    println!("tool-mine: dropped {} more qualifying sequences below the top {}", mined.dropped, options.limits.max_filed);
    if options.dry_run {
        return Ok(());
    }
    let (places, machine) = (Places::from_env(), Machine::default());
    let outcomes = Filer::new(&places, &machine).file(&findings, now)?;
    for outcome in &outcomes {
        println!("{}", outcome.line());
    }
    let filed: Vec<_> = outcomes.into_iter().filter(|outcome| outcome.filed).map(|outcome| outcome.finding).collect();
    record(&places, &[spend], &filed)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match mine(&options) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("tool-mine: {error}");
            1
        }
    }
}
