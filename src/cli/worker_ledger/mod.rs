mod landed_issues;
mod pull;
mod recalibrate;
mod rows;

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use crate::providers::claude::runs::landing::repositories;
use crate::foundation::exit::TRANSPORT_FAILED_STATUS;

const USAGE: &str = include_str!("usage.txt");

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
    /// Each host's newest recorded capacity admission, `HOST<TAB>AT` per line.
    #[serde(default)]
    capacity: String,
}

enum Mode {
    Record,
    Emit,
    Summary { by: String },
    EtaCalibration,
    Recommend { facets: String },
    LandedIssues { account: String, day: String },
}

fn parse(arguments: &[String]) -> Option<(Options, Mode)> {
    let (mut session, mut since, mut by) = (String::new(), String::new(), "category".to_owned());
    let (mut summary, mut backfill, mut facets, mut emit, mut eta) = (false, false, String::new(), false, false);
    let (mut landed, mut day) = (None, String::new());
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--session" => session = words.next()?.clone(),
            "--since" => since = words.next()?.clone(),
            "--summary" => summary = true,
            "--by" => by = words.next()?.clone(),
            "--recommend" => facets = words.next()?.clone(),
            "--backfill" => backfill = true,
            "--emit" => emit = true,
            "--eta-calibration" => eta = true,
            "--landed-issues" => landed = Some(words.next()?.clone()),
            "--day" => day = words.next()?.clone(),
            _ => return None,
        }
    }
    let mode = if let Some(account) = landed {
        Mode::LandedIssues { account, day }
    } else if emit {
        Mode::Emit
    } else if eta {
        Mode::EtaCalibration
    } else if summary {
        Mode::Summary { by }
    } else if !facets.is_empty() {
        Mode::Recommend { facets }
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
        Mode::EtaCalibration => threads(&[&["eta-calibration"][..], &since].concat()),
        Mode::Recommend { facets } => threads(&[&["recommend", facets][..], &since].concat()),
        Mode::Record => return record(&options),
        Mode::Emit => return emit(&options),
        Mode::LandedIssues { account, day } => return landed_issues::run(account, day),
    };
    let mut command = match built {
        Ok(command) => command,
        Err(error) => {
            eprintln!("north ledger worker: cannot find the running north: {error}");
            return 126;
        }
    };
    eprintln!("north ledger worker: cannot run north threads: {}", command.exec());
    126
}

fn run_ids(arguments: &[&str]) -> Option<String> {
    let output = threads(arguments).and_then(|mut command| command.stderr(Stdio::inherit()).output()).map_err(|error| eprintln!("north ledger worker: cannot run north threads: {error}")).ok()?;
    match output.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(TRANSPORT_FAILED_STATUS) => Some(String::new()),
        _ => None,
    }
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

fn record(options: &Options) -> u8 {
    if let Err(error) = crate::providers::ledger_rows::cutoff(&options.since) {
        eprintln!("north ledger worker: {}", error.user_message());
        return 2;
    }
    let (Some(recorded), Some(settled)) = (run_ids(&["run-ids"]), run_ids(&["run-ids", "--settled", "--ended"])) else { return 1 };
    let Some(capacity) = run_ids(&["run-ids", "--capacity"]) else { return 1 };
    let ids = Ids { recorded, settled, capacity };
    let Some(rows::Rows { mut rows, unread, missed }) = rows::rows(options, &ids) else { return 1 };
    let whole_fleet = options.session.is_empty();
    if whole_fleet && recalibrate::owns_ledger() {
        pull::hosts(options, &ids, &mut rows);
    }
    let ingested = ingest(&rows);
    if whole_fleet {
        if recalibrate::owns_ledger() {
            label_runs();
        }
        recalibrate::run();
        match crate::dispatch::record_closed_reports() {
            Ok(unread) if unread.is_empty() => {}
            Ok(unread) => eprintln!("north ledger worker: closed-box reports recorded; GitHub failed for {}, whose last good rows stand in until the next run", unread.join(", ")),
            Err(error) => {
                eprintln!("north ledger worker: closed-box reports unrecorded, so launches draw arms at equal shares and brief no budget once the last ones age out: {}", error.user_message());
                return 1;
            }
        }
    }
    if missed || (ingested == 0 && unread) { 1 } else { ingested }
}

/// `--emit`: the rows this host would record against the ledger's ids read from stdin, printed instead of ingested.
fn emit(options: &Options) -> u8 {
    if let Err(error) = crate::providers::ledger_rows::cutoff(&options.since) {
        eprintln!("north ledger worker: {}", error.user_message());
        return 2;
    }
    let ids: Ids = match serde_json::from_reader(std::io::stdin().lock()) {
        Ok(ids) => ids,
        Err(error) => {
            eprintln!("north ledger worker: --emit reads the ledger's ids as JSON on stdin: {error}");
            return 2;
        }
    };
    let Some(rows::Rows { rows, unread, .. }) = rows::rows(options, &ids) else { return 1 };
    if std::io::stdout().lock().write_all(&rows).is_err() || (rows.is_empty() && unread) { 1 } else { 0 }
}

fn label_runs() {
    match threads(&["label-runs"]).and_then(|mut command| command.status()) {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("north ledger worker: north threads label-runs exited {status}"),
        Err(error) => eprintln!("north ledger worker: cannot run north threads label-runs: {error}"),
    }
}

fn ingest(rows: &[u8]) -> u8 {
    let spawned = threads(&["ingest"]).and_then(|mut command| command.stdin(Stdio::piped()).spawn());
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("north ledger worker: cannot run north threads ingest: {error}");
            return 1;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(rows);
    }
    child.wait().ok().and_then(|status| status.code()).map_or(1, |code| u8::try_from(code).unwrap_or(1))
}
