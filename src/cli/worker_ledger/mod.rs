mod landed_issues;
mod pull;
mod recalibrate;
mod rows;

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use clap::Parser;

use crate::providers::claude::runs::landing::repositories;
use crate::foundation::exit::TRANSPORT_FAILED_STATUS;

const USAGE: &str = include_str!("usage.txt");

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, args_override_self = true)]
struct Flags {
    #[arg(long, default_value = "")]
    session: String,
    #[arg(long, default_value = "")]
    since: String,
    #[arg(long)]
    summary: bool,
    #[arg(long, default_value = "category")]
    by: String,
    #[arg(long, default_value = "")]
    recommend: String,
    #[arg(long)]
    backfill: bool,
    #[arg(long)]
    emit: bool,
    #[arg(long)]
    eta_calibration: bool,
    #[arg(long)]
    landed_issues: Option<String>,
    #[arg(long, default_value = "")]
    day: String,
}

pub(super) struct Options {
    pub(super) session: String,
    pub(super) since: String,
    pub(super) backfill: bool,
}

/// The ledger's run ids a recording run skips or rereads: every recorded id, and the settled ones with their end.
#[derive(serde::Serialize, serde::Deserialize)]
struct Ids {
    recorded: String,
    settled: String,
}

#[derive(Debug, PartialEq)]
pub(super) enum Mode {
    Record,
    Emit,
    Summary { by: String },
    EtaCalibration,
    Recommend { facets: String },
    LandedIssues { account: String, day: String },
}

pub(super) fn parse(arguments: &[String]) -> Option<(Options, Mode)> {
    let Flags { session, since, summary, by, recommend, backfill, emit, eta_calibration, landed_issues, day } = north_foundation::foundation::arguments::parse(arguments).ok()?;
    let mode = if let Some(account) = landed_issues {
        Mode::LandedIssues { account, day }
    } else if emit {
        Mode::Emit
    } else if eta_calibration {
        Mode::EtaCalibration
    } else if summary {
        Mode::Summary { by }
    } else if !recommend.is_empty() {
        Mode::Recommend { facets: recommend }
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
    let ids = Ids { recorded, settled };
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
