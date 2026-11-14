//! `north issue close-stale`: the hourly job that closes every open issue of north and south older than 24 hours and
//! messages the issue's last owner session to rewrite it as one-day issues (owner's rule of 2026-11-14).
use std::path::PathBuf;
use std::process::Command;

use crate::foundation::clock::Timestamp;
use crate::threads::client::threads_database;
use north_ops::ops::stale_issues::{self as sweep, Hands, Owners, Record, Stale};

const REPOSITORIES: [(&str, &str); 2] = [("tompassarelli", "north"), ("tompassarelli", "south")];
const SENDER: &str = "service:north-stale-issues";

const USAGE: &str = "usage: north issue close-stale [--dry-run]

Closes every open issue of north and south created more than 24 hours ago, with a comment that names the 24-hour rule
and nothing of the issue, and messages the issue's last owner session (the newest claim on it) to rewrite the work as issues that can each be finished within a day. Each issue's owner is messaged once:
the messaged issues are recorded under ~/.local/state/north/stale-issues.txt. --dry-run prints what would happen and
changes nothing. An hourly timer runs it.";

struct Live {
    repository: String,
}

impl Hands for Live {
    fn message(&mut self, session: &str, text: &str) -> Result<(), String> {
        let north = std::env::current_exe().map_err(|error| error.to_string())?;
        let output = Command::new(north).args(["fleet", "send", "--from", SENDER, session, text]).output().map_err(|error| error.to_string())?;
        if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        let args = ["issue".to_owned(), "close".to_owned(), number.to_string(), "-R".to_owned(), self.repository.clone(), "--comment".to_owned(), comment.to_owned()];
        crate::platform::github::run_patiently(&args, "north issue close-stale").map(drop)
    }
}

fn record_file() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north/stale-issues.txt")
}

pub fn run(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [] => false,
        [flag] if flag == "--dry-run" => true,
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return 0;
        }
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let now = Timestamp::now().unix_seconds();
    let mut failures = 0;
    for (organisation, name) in REPOSITORIES {
        match close_repository(organisation, name, now, dry_run) {
            Ok(failed) => failures += failed,
            Err(why) => {
                eprintln!("north issue close-stale: {name}: {why}");
                failures += 1;
            }
        }
    }
    u8::from(failures > 0)
}

fn close_repository(organisation: &str, name: &str, now: i64, dry_run: bool) -> Result<usize, String> {
    let repository = crate::issue_convert::read::repository(organisation, name, false)?;
    let owners = Owners::read(&threads_database(), name)?;
    let stale: Vec<Stale> = repository.issues.iter().filter(|issue| issue.open).map(|issue| Stale { number: issue.number, created: issue.created, owner: owners.of(issue.number) }).collect();
    let mut record = Record::open(&record_file())?;
    let mut hands = Live { repository: format!("{organisation}/{name}") };
    let outcome = sweep::sweep(name, &stale, now, dry_run, &mut hands, &mut record);
    outcome.lines.iter().for_each(|line| println!("{line}"));
    Ok(outcome.failed)
}
