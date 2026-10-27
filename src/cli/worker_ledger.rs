use std::collections::HashSet;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::providers::claude::runs::landing::MainIndex;
use crate::providers::claude::runs::telemetry::Prices;
use crate::providers::claude::runs::{CATALOG, DEFAULTS};
use crate::providers::ledger_rows::Shared;
use crate::providers::{agy, claude, codex};

const USAGE: &str = include_str!("worker_ledger_usage.txt");
const REMOTE_UNREACHABLE: i32 = 255;

struct Options {
    session: String,
    since: String,
    backfill: bool,
}

enum Mode {
    Record,
    Summary { by: String },
    Recommend { facets: String, planner: bool },
}

fn parse(arguments: &[String]) -> Option<(Options, Mode)> {
    let (mut session, mut since, mut by) = (String::new(), String::new(), "category".to_owned());
    let (mut summary, mut backfill, mut planner, mut facets) = (false, false, false, String::new());
    let mut words = arguments.iter().peekable();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--session" => session = words.next()?.clone(),
            "--since" => since = words.next()?.clone(),
            "--summary" => summary = true,
            "--by" => by = words.next()?.clone(),
            "--recommend" => {
                if words.next_if(|next| next.as_str() == "--planner").is_some() {
                    planner = true;
                }
                facets = words.next()?.clone();
            }
            "--planner" => planner = true,
            "--backfill" => backfill = true,
            _ => return None,
        }
    }
    if planner && facets.is_empty() {
        return None;
    }
    let mode = if summary {
        Mode::Summary { by }
    } else if !facets.is_empty() {
        Mode::Recommend { facets, planner }
    } else {
        Mode::Record
    };
    Some((Options { session, since, backfill }, mode))
}

fn root() -> PathBuf {
    crate::platform::harness::harness_root().unwrap_or_default()
}

pub(super) fn run(arguments: &[String]) -> ExitCode {
    let Some((options, mode)) = parse(arguments) else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let threads = root().join("bin/threads");
    let mut command = Command::new(&threads);
    match mode {
        Mode::Summary { by } => {
            command.args(["summary", "--group", &by]);
        }
        Mode::Recommend { facets, planner } => {
            command.args(["recommend", &facets]);
            if planner {
                command.arg("--planner");
            }
        }
        Mode::Record => return record(&options, &threads),
    }
    if !options.since.is_empty() {
        command.args(["--since", &options.since]);
    }
    eprintln!("north worker-ledger: cannot run {}: {}", threads.display(), command.exec());
    ExitCode::from(126)
}

fn run_ids(threads: &Path, settled: bool) -> Option<HashSet<String>> {
    let mut command = Command::new(threads);
    command.arg("run-ids");
    if settled {
        command.arg("--settled");
    }
    let output = command.stderr(Stdio::inherit()).output().ok()?;
    match output.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()),
        Some(REMOTE_UNREACHABLE) => Some(HashSet::new()),
        _ => None,
    }
}

fn repositories() -> Vec<PathBuf> {
    if let Some(listed) = std::env::var("LEDGER_REPOS").ok().filter(|listed| !listed.is_empty()) {
        return listed.split_whitespace().map(PathBuf::from).collect();
    }
    let code = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code");
    let mut found: Vec<PathBuf> = std::fs::read_dir(code).into_iter().flatten().flatten().map(|entry| entry.path().join("main")).collect();
    found.sort();
    found
}

fn merged(since: &str) -> String {
    let dated = since.len() == 10 && since.bytes().enumerate().all(|(at, byte)| if matches!(at, 4 | 7) { byte == b'-' } else { byte.is_ascii_digit() });
    let bound = if dated { format!("{since}T00:00:00Z") } else if since.is_empty() { "30 days ago".to_owned() } else { since.to_owned() };
    let mut text = String::new();
    for repo in repositories() {
        let log = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["-c", "maintenance.auto=false", "-c", "gc.auto=0", "log", "origin/main"])
            .arg(format!("--since={bound}"))
            .arg("--format=%H %cI %s")
            .stderr(Stdio::null())
            .output();
        if let Ok(log) = log {
            text.push_str(&String::from_utf8_lossy(&log.stdout));
        }
    }
    text
}

fn record(options: &Options, threads: &Path) -> ExitCode {
    let (Some(recorded), Some(settled)) = (run_ids(threads, false), run_ids(threads, true)) else { return ExitCode::FAILURE };
    let known = if options.backfill { HashSet::new() } else { settled };
    let text = merged(&options.since);
    let mut rows = Vec::new();
    let session = Some(options.session.as_str()).filter(|session| !session.is_empty());
    if let Err(error) = claude::runs::rows(session, &options.since, known.clone(), recorded.clone(), &text, &mut rows) {
        eprintln!("error: {}", error.user_message());
        return ExitCode::FAILURE;
    }
    let main = MainIndex::parse(&text);
    let prices = Prices::parse(CATALOG, DEFAULTS);
    let shared = Shared { session: &options.session, since: &options.since, known: &known, recorded: &recorded, main: &main, prices: &prices };
    for (name, outcome) in [("codex", codex::runs::rows(&shared, &mut rows)), ("agy", agy::rows(&shared, &mut rows))] {
        if let Err(error) = outcome {
            eprintln!("north worker-ledger: {name} rows unread: {}", error.user_message());
        }
    }
    let ingested = ingest(threads, &rows);
    let python = Command::new("python3").arg(root().join("harness/lib/recalibrate.py")).status();
    if let Err(error) = python {
        eprintln!("north worker-ledger: recalibrate not run: {error}");
    }
    ExitCode::from(ingested)
}

fn ingest(threads: &Path, rows: &[u8]) -> u8 {
    let Ok(mut child) = Command::new(threads).arg("ingest").stdin(Stdio::piped()).spawn() else { return 1 };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(rows);
    }
    child.wait().ok().and_then(|status| status.code()).map_or(1, |code| u8::try_from(code).unwrap_or(1))
}
