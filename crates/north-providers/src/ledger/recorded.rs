use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::closed::ClosedReport;
use super::closed_source::{ClosedIssue, Listing, github_closed, github_open, report_over, threads_database, window_bounds, within};

const BOUND_S: i64 = 45 * 60;

#[derive(serde::Deserialize, serde::Serialize)]
struct Recorded {
    at: i64,
    report: ClosedReport,
}

#[derive(Clone)]
struct Held {
    issues: Vec<ClosedIssue>,
    open: HashMap<String, String>,
}

fn path(window: &str) -> PathBuf {
    threads_database().with_file_name(format!("closed-report-{window}.json"))
}

pub fn record_closed_reports(windows: &[&str]) -> NorthResult<()> {
    let now = Timestamp::now();
    for (window, report) in windows.iter().zip(reports(windows, now)?) {
        write(&path(window), &Recorded { at: now.unix_seconds(), report })?;
    }
    Ok(())
}

pub fn recorded_closed_report(window: &str) -> NorthResult<ClosedReport> {
    read(&path(window), Timestamp::now())
}

fn reports(windows: &[&str], now: Timestamp) -> NorthResult<Vec<ClosedReport>> {
    let database = threads_database();
    let corroborated = super::corroborated::checked_boxes(&north_graph::default_path())?;
    let bounds = windows.iter().map(|window| window_bounds(window, now)).collect::<NorthResult<Vec<_>>>()?;
    let widest = bounds.iter().map(|(since, _)| since.clone()).min().unwrap_or_default();
    let until = now.iso();
    let fetched: Mutex<HashMap<String, Held>> = Mutex::default();
    let github = |repo: &str, since: &str, end: &str| -> NorthResult<Listing> {
        let held = fetched.lock().unwrap_or_else(PoisonError::into_inner).get(repo).cloned();
        let Held { issues, open } = match held {
            Some(held) => held,
            None => {
                let held = Held { issues: github_closed(repo, &widest, &until)?, open: github_open(repo)? };
                fetched.lock().unwrap_or_else(PoisonError::into_inner).insert(repo.to_owned(), held.clone());
                held
            }
        };
        Ok(Listing { closed: within(&issues, since, end), open })
    };
    bounds.iter().map(|(since, end)| report_over(&database, since, end, |_| true, corroborated.clone(), github)).collect()
}

fn write(path: &Path, recorded: &Recorded) -> NorthResult<()> {
    let text = serde_json::to_string(recorded).map_err(|why| NorthError::Configuration(format!("{}: {why}", path.display())))?;
    let staged = path.with_extension("json.part");
    std::fs::write(&staged, text).and_then(|()| std::fs::rename(&staged, path)).map_err(|why| NorthError::Configuration(format!("{}: {why}", path.display())))
}

fn read(path: &Path, now: Timestamp) -> NorthResult<ClosedReport> {
    let text = std::fs::read_to_string(path).map_err(|why| NorthError::Configuration(format!("{} unread, which `north ledger worker` records: {why}", path.display())))?;
    let recorded: Recorded = serde_json::from_str(&text).map_err(|why| NorthError::Configuration(format!("{}: {why}", path.display())))?;
    let age = now.unix_seconds() - recorded.at;
    if age > BOUND_S {
        return Err(NorthError::Configuration(format!("{} was recorded {} min ago, past its {} min bound", path.display(), age / 60, BOUND_S / 60)));
    }
    Ok(recorded.report)
}
