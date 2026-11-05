//! `north tempo`: how fast lanes reach main through a repository's lander, read from the tickets the
//! lander decided (`land-queue/<origin key>/done/`, each landed at its file's modification time) and
//! the tickets still waiting. `--record` writes the last hour and the last day into the ledger's
//! `land_tempo` table, which the Payback line of a harness box reads (docs/feedback-loop.md).
#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::foundation::clock::Timestamp;
use crate::ops::flow::Places;
use crate::threads::landing::tempo::percentiles;

const USAGE: &str = "usage: north tempo [--repo NAME] [--window MINUTES] [--json] [--record]";
const HOUR: u64 = 60;
const DAY: u64 = 1440;

#[derive(Deserialize)]
struct Decided {
    queued_at: String,
    #[serde(default)]
    first_commit_secs: Option<i64>,
    #[serde(default)]
    verdict: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Reading {
    pub repo: String,
    pub window_minutes: u64,
    pub landed: usize,
    pub queued_to_main_p50_minutes: Option<f64>,
    pub queued_to_main_p90_minutes: Option<f64>,
    pub first_commit_to_main_p50_minutes: Option<f64>,
    pub first_commit_to_main_p90_minutes: Option<f64>,
    pub first_commit_samples: usize,
    pub refused_alone: usize,
    pub conflicts: usize,
    pub queued_now: usize,
}

fn modified(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn tickets(directory: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "json"))
}

fn decided(done: &Path, since: f64) -> Vec<(f64, Decided)> {
    tickets(done)
        .filter_map(|path| {
            let at = modified(&path).filter(|at| *at >= since)?;
            Some((at, serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?))
        })
        .collect()
}

fn minutes(from: f64, to: f64) -> f64 {
    (to - from).max(0.0) / 60.0
}

pub fn read(repo: &str, queue: &Path, now: f64, window_minutes: u64) -> Reading {
    let decided = decided(&queue.join("done"), now - window_minutes as f64 * 60.0);
    let verdict = |ticket: &Decided| ticket.verdict.clone().unwrap_or_default();
    let landed: Vec<&(f64, Decided)> = decided.iter().filter(|(_, ticket)| verdict(ticket).starts_with("landed")).collect();
    let queued: Vec<f64> = landed.iter().filter_map(|(at, ticket)| Some(minutes(Timestamp::parse_python_datetime(&ticket.queued_at)?.unix_seconds_f64(), *at))).collect();
    let first: Vec<f64> = landed.iter().filter_map(|(at, ticket)| Some(minutes(ticket.first_commit_secs.filter(|secs| *secs > 0)? as f64, *at))).collect();
    let ((queued_p50, queued_p90), (first_p50, first_p90)) = (percentiles(&queued), percentiles(&first));
    let refused = |prefix: &str| decided.iter().filter(|(_, ticket)| verdict(ticket).starts_with(prefix)).count();
    Reading {
        repo: repo.to_owned(),
        window_minutes,
        landed: landed.len(),
        queued_to_main_p50_minutes: queued_p50,
        queued_to_main_p90_minutes: queued_p90,
        first_commit_to_main_p50_minutes: first_p50,
        first_commit_to_main_p90_minutes: first_p90,
        first_commit_samples: first.len(),
        refused_alone: refused("failed ") - refused("failed interaction"),
        conflicts: refused("conflict "),
        queued_now: tickets(queue).count(),
    }
}

fn figure(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".into(), |minutes| format!("{minutes:.1}"))
}

pub fn line(reading: &Reading) -> String {
    format!(
        "tempo {}, last {} min: {} lanes landed, queued->main p50 {} p90 {} min, first commit->main p50 {} p90 {} min over {} lanes, {} refused alone, {} conflicts, {} queued now",
        reading.repo,
        reading.window_minutes,
        reading.landed,
        figure(reading.queued_to_main_p50_minutes),
        figure(reading.queued_to_main_p90_minutes),
        figure(reading.first_commit_to_main_p50_minutes),
        figure(reading.first_commit_to_main_p90_minutes),
        reading.first_commit_samples,
        reading.refused_alone,
        reading.conflicts,
        reading.queued_now,
    )
}

pub fn record_arguments(reading: &Reading, at: f64) -> Vec<String> {
    let mut words: Vec<String> = ["land-train", "tempo", "--repo", &reading.repo, "--at", &at.to_string(), "--window-minutes", &reading.window_minutes.to_string()].map(str::to_owned).to_vec();
    let counts = [("--landed", reading.landed), ("--first-commit-samples", reading.first_commit_samples), ("--refused-alone", reading.refused_alone), ("--conflicts", reading.conflicts), ("--queued-now", reading.queued_now)];
    words.extend(counts.iter().flat_map(|(flag, count)| [(*flag).to_owned(), count.to_string()]));
    let figures = [
        ("--queued-p50", reading.queued_to_main_p50_minutes),
        ("--queued-p90", reading.queued_to_main_p90_minutes),
        ("--first-commit-p50", reading.first_commit_to_main_p50_minutes),
        ("--first-commit-p90", reading.first_commit_to_main_p90_minutes),
    ];
    words.extend(figures.into_iter().filter_map(|(flag, value)| Some([flag.to_owned(), value?.to_string()])).flatten());
    words
}

struct Options {
    repo: String,
    window_minutes: u64,
    json: bool,
    record: bool,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut options = Options { repo: "north".into(), window_minutes: HOUR, json: false, record: false };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--repo" => options.repo = words.next()?.clone(),
            "--window" => options.window_minutes = words.next()?.parse().ok().filter(|minutes| *minutes > 0)?,
            "--json" => options.json = true,
            "--record" => options.record = true,
            _ => return None,
        }
    }
    Some(options)
}

fn queue(places: &Places, repo: &str) -> Result<PathBuf, String> {
    let checkout = places.checkout(repo);
    let output = Command::new("git").arg("-C").arg(&checkout).args(["remote", "get-url", "--push", "origin"]).output().map_err(|error| format!("git: {error}"))?;
    if !output.status.success() {
        return Err(format!("{} has no origin remote", checkout.display()));
    }
    Ok(crate::platform::land_queue::directory(&places.state, String::from_utf8_lossy(&output.stdout).trim()))
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let places = Places::from_env();
    let queue = match queue(&places, &options.repo) {
        Ok(queue) => queue,
        Err(error) => {
            eprintln!("north tempo: {error}");
            return 1;
        }
    };
    let now = Timestamp::now().unix_seconds_f64();
    let windows = if options.record { vec![HOUR, DAY] } else { vec![options.window_minutes] };
    let readings: Vec<Reading> = windows.into_iter().map(|window| read(&options.repo, &queue, now, window)).collect();
    for reading in &readings {
        if options.json {
            println!("{}", serde_json::to_string(reading).unwrap_or_default());
        } else {
            println!("{}", line(reading));
        }
        if options.record {
            let code = crate::threads::client::run(&record_arguments(reading, now), Arc::new(crate::threads::outside::Unread));
            if code != 0 {
                eprintln!("north tempo: recording the {}-minute reading into the ledger exited {code}", reading.window_minutes);
                return code;
            }
        }
    }
    0
}
