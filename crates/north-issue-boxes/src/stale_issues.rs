//! `north issue close-stale`: the hourly job that closes every open issue of north and south older than 24 hours and
//! messages the issue's last owner session to rewrite it as one-day issues (owner's rule of 2026-11-14).
use std::path::PathBuf;
use std::process::Command;

use crate::foundation::clock::Timestamp;
use crate::threads::claims::ledger;
use crate::threads::client::{ledger_host_name, threads_database};
use crate::threads::rewrites;
use north_ops::ops::stale_issues::{self as sweep, Hands, Lanes, Owners, Record, Stale};

use crate::issue_convert::boxes::checkboxes;

const REPOSITORIES: [(&str, &str); 2] = [("tompassarelli", "north"), ("tompassarelli", "south")];
const SENDER: &str = "service:north-stale-issues";

const USAGE: &str = "usage: north issue close-stale [--dry-run]

Closes every open issue of north and south created more than 24 hours ago as not planned, with a comment that names the 24-hour rule,
the issue's open boxes and the unlanded lanes whose Item: names it, and files a successor carrying the open boxes, records the rewrite in the threads ledger (a lead holding the closed issue launches on the successor), and messages the issue's last owner session (the newest claim on it, else its Owner: line, else proxy) to split the successor if a box needs more than a day and re-point those lanes. It runs on the ledger host. Each issue is messaged once:
the messaged issues are recorded under ~/.local/state/north/stale-issues.txt. --dry-run prints what would happen and
changes nothing. An hourly timer runs it.";

struct Live {
    repository: String,
    ledger: PathBuf,
}

impl Hands for Live {
    fn message(&mut self, session: &str, text: &str) -> Result<(), String> {
        let north = std::env::current_exe().map_err(|error| error.to_string())?;
        let output = Command::new(north).args(["fleet", "send", "--from", SENDER, session, text]).output().map_err(|error| error.to_string())?;
        if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        let args = ["issue".to_owned(), "close".to_owned(), number.to_string(), "-R".to_owned(), self.repository.clone(), "--reason".to_owned(), "not planned".to_owned(), "--comment".to_owned(), comment.to_owned()];
        crate::platform::github::run_patiently(&args, "north issue close-stale").map(drop)
    }

    fn file(&mut self, title: &str, body: &str) -> Result<u64, String> {
        let args = ["issue", "create", "-R", &self.repository, "--title", title, "--body", body].map(str::to_owned);
        let url = crate::platform::github::run_patiently(&args, "north issue close-stale")?;
        url.trim().rsplit('/').next().and_then(|number| number.parse().ok()).ok_or_else(|| format!("gh issue create printed no issue URL: {}", url.trim()))
    }

    fn recorded_successor(&mut self, closed: &str) -> Result<Option<String>, String> {
        rewrites::successor(&ledger::open(&self.ledger, false)?, closed).map_err(|error| format!("ledger {}: {error}", self.ledger.display()))
    }

    fn record_successor(&mut self, closed: &str, successor: &str) -> Result<(), String> {
        rewrites::record(&ledger::open(&self.ledger, true)?, closed, successor, &Timestamp::now().iso()).map_err(|error| format!("ledger {}: {error}", self.ledger.display()))
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
    if !dry_run && let Some(host) = ledger_host_name() {
        eprintln!("north issue close-stale: the threads ledger lives on {host}, and the closer records each rewrite in it, so it runs there");
        return 2;
    }
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
    let lanes = Lanes::read(&PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code").join(name).join("main"), name)?;
    let stale: Vec<Stale> = repository
        .issues
        .iter()
        .filter(|issue| issue.open)
        .map(|issue| Stale {
            number: issue.number,
            created: issue.created,
            title: issue.title.clone(),
            owner: owners.of(issue.number, &issue.body),
            open_boxes: checkboxes(&issue.body).into_iter().filter(|checkbox| !checkbox.ticked).map(|checkbox| checkbox.text).collect(),
            lanes: lanes.naming(issue.number),
        })
        .collect();
    let mut record = Record::open(&record_file())?;
    let mut hands = Live { repository: format!("{organisation}/{name}"), ledger: threads_database() };
    let outcome = sweep::sweep(name, &stale, now, dry_run, &mut hands, &mut record);
    outcome.lines.iter().for_each(|line| println!("{line}"));
    Ok(outcome.failed)
}
