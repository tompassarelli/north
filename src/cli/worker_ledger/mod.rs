mod pull;
mod recalibrate;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::providers::claude::runs::landing::MainIndex;
use crate::providers::claude::runs::telemetry::Prices;
use crate::providers::claude::runs::{CATALOG, DEFAULTS};
use crate::providers::ledger_rows::Shared;
use crate::providers::{agy, claude, codex};

const USAGE: &str = include_str!("usage.txt");
const REMOTE_UNREACHABLE: i32 = 255;

struct Options {
    session: String,
    since: String,
    backfill: bool,
}

/// The ledger's run ids a recording run skips or rereads: every recorded id, and the settled ones with their end.
#[derive(serde::Serialize, serde::Deserialize)]
struct Ids {
    recorded: String,
    settled: String,
}

enum Mode {
    Record,
    Emit,
    Summary { by: String },
    Recommend { facets: String, planner: bool },
}

fn parse(arguments: &[String]) -> Option<(Options, Mode)> {
    let (mut session, mut since, mut by) = (String::new(), String::new(), "category".to_owned());
    let (mut summary, mut backfill, mut planner, mut facets, mut emit) = (false, false, false, String::new(), false);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--session" => session = words.next()?.clone(),
            "--since" => since = words.next()?.clone(),
            "--summary" => summary = true,
            "--by" => by = words.next()?.clone(),
            "--recommend" => {
                let mut next = words.next()?.clone();
                if next == "--planner" {
                    planner = true;
                    next = words.next()?.clone();
                }
                facets = next;
            }
            "--planner" => planner = true,
            "--backfill" => backfill = true,
            "--emit" => emit = true,
            _ => return None,
        }
    }
    if planner && facets.is_empty() {
        return None;
    }
    let mode = if emit {
        Mode::Emit
    } else if summary {
        Mode::Summary { by }
    } else if !facets.is_empty() {
        Mode::Recommend { facets, planner }
    } else {
        Mode::Record
    };
    Some((Options { session, since, backfill }, mode))
}

pub(super) fn north() -> std::io::Result<Command> {
    std::env::current_exe().map(Command::new)
}

fn threads(arguments: &[&str]) -> std::io::Result<Command> {
    let mut command = north()?;
    command.arg("threads").args(arguments);
    Ok(command)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((options, mode)) = parse(arguments) else {
        eprint!("{USAGE}");
        return 2;
    };
    let since: Vec<&str> = if options.since.is_empty() { Vec::new() } else { vec!["--since", &options.since] };
    let built = match &mode {
        Mode::Summary { by } => threads(&[&["summary", "--group", by][..], &since].concat()),
        Mode::Recommend { facets, planner } => threads(&[&["recommend", facets][..], if *planner { &["--planner"] } else { &[] }, &since].concat()),
        Mode::Record => return record(&options),
        Mode::Emit => return emit(&options),
    };
    let mut command = match built {
        Ok(command) => command,
        Err(error) => {
            eprintln!("north worker-ledger: cannot find the running north: {error}");
            return 126;
        }
    };
    eprintln!("north worker-ledger: cannot run north threads: {}", command.exec());
    126
}

fn run_ids(arguments: &[&str]) -> Option<String> {
    let output = threads(arguments).and_then(|mut command| command.stderr(Stdio::inherit()).output()).map_err(|error| eprintln!("north worker-ledger: cannot run north threads: {error}")).ok()?;
    match output.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(REMOTE_UNREACHABLE) => Some(String::new()),
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

fn settled_ids(listed: &str) -> HashMap<String, Option<String>> {
    listed
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once('\t') {
            Some((id, ended)) => (id.to_owned(), Some(ended.to_owned())),
            None => (line.to_owned(), None),
        })
        .collect()
}

fn record(options: &Options) -> u8 {
    if let Err(error) = crate::providers::ledger_rows::cutoff(&options.since) {
        eprintln!("north worker-ledger: {}", error.user_message());
        return 2;
    }
    let (Some(recorded), Some(settled)) = (run_ids(&["run-ids"]), run_ids(&["run-ids", "--settled", "--ended"])) else { return 1 };
    let ids = Ids { recorded, settled };
    let Some((mut rows, unread)) = rows(options, &ids) else { return 1 };
    if recalibrate::owns_ledger() {
        pull::hosts(options, &ids, &mut rows);
    }
    let ingested = ingest(&rows);
    recalibrate::run();
    if ingested == 0 && unread { 1 } else { ingested }
}

/// `--emit`: the rows this host would record against the ledger's ids read from stdin, printed instead of ingested.
fn emit(options: &Options) -> u8 {
    if let Err(error) = crate::providers::ledger_rows::cutoff(&options.since) {
        eprintln!("north worker-ledger: {}", error.user_message());
        return 2;
    }
    let ids: Ids = match serde_json::from_reader(std::io::stdin().lock()) {
        Ok(ids) => ids,
        Err(error) => {
            eprintln!("north worker-ledger: --emit reads the ledger's ids as JSON on stdin: {error}");
            return 2;
        }
    };
    let Some((rows, unread)) = rows(options, &ids) else { return 1 };
    if std::io::stdout().lock().write_all(&rows).is_err() || (rows.is_empty() && unread) { 1 } else { 0 }
}

/// This host's provider rows the ledger lacks, and whether a provider's store went unread.
fn rows(options: &Options, ids: &Ids) -> Option<(Vec<u8>, bool)> {
    let recorded: HashSet<String> = settled_ids(&ids.recorded).into_keys().collect();
    let known = if options.backfill { HashMap::new() } else { settled_ids(&ids.settled) };
    let text = merged(&options.since);
    let mut rows = Vec::new();
    let session = Some(options.session.as_str()).filter(|session| !session.is_empty());
    if let Err(error) = claude::runs::rows(session, &options.since, known.clone(), recorded.clone(), &text, &mut rows) {
        eprintln!("error: {}", error.user_message());
        return None;
    }
    let known: HashSet<String> = known.into_keys().collect();
    let main = MainIndex::parse(&text);
    let prices = Prices::parse(CATALOG, DEFAULTS);
    let shared = Shared { session: &options.session, since: &options.since, known: &known, recorded: &recorded, main: &main, prices: &prices };
    let mut unread = false;
    for (name, outcome) in [("codex", codex::runs::rows(&shared, &mut rows)), ("agy", agy::rows(&shared, &mut rows))] {
        if let Err(error) = outcome {
            eprintln!("north worker-ledger: {name} rows unread: {}", error.user_message());
            unread = true;
        }
    }
    Some((rows, unread))
}

fn ingest(rows: &[u8]) -> u8 {
    let spawned = threads(&["ingest"]).and_then(|mut command| command.stdin(Stdio::piped()).spawn());
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("north worker-ledger: cannot run north threads ingest: {error}");
            return 1;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(rows);
    }
    child.wait().ok().and_then(|status| status.code()).map_or(1, |code| u8::try_from(code).unwrap_or(1))
}
