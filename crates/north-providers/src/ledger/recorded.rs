use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::closed::ClosedReport;
use super::closed_source::{ClosedIssue, Listing, github_closed, github_open, report_over, threads_database, window_bounds, within};

const BOUND_S: i64 = 45 * 60;
const UNREAD_RUNS_BEFORE_FAILING: u32 = 3;

#[derive(serde::Deserialize, serde::Serialize)]
struct Recorded {
    at: i64,
    report: ClosedReport,
}

#[derive(Clone, Default, serde::Deserialize, serde::Serialize)]
pub(super) struct Held {
    pub(super) issues: Vec<ClosedIssue>,
    pub(super) open: HashMap<String, String>,
}

fn path(window: &str) -> PathBuf {
    threads_database().with_file_name(format!("closed-report-{window}.json"))
}

fn last_good(repo: &str) -> PathBuf {
    threads_database().with_file_name(format!("closed-listing-{repo}.json"))
}

fn unread_runs() -> PathBuf {
    threads_database().with_file_name("closed-report-unread-runs")
}

/// Records each window's closed-box report and returns the repositories whose GitHub read failed this run, whose last good
/// rows stood in for them; a run in which some repository failed for the third run in a row is an error naming them.
pub fn record_closed_reports(windows: &[&str]) -> NorthResult<Vec<String>> {
    let now = Timestamp::now();
    let (reports, unread) = reports(windows, now)?;
    for (window, report) in windows.iter().zip(reports) {
        write(&path(window), &Recorded { at: now.unix_seconds(), report })?;
    }
    settle(&unread_runs(), unread)
}

pub fn recorded_closed_report(window: &str) -> NorthResult<ClosedReport> {
    read(&path(window), Timestamp::now())
}

pub(super) fn settle(counter: &Path, unread: Vec<String>) -> NorthResult<Vec<String>> {
    if unread.is_empty() {
        let _ = std::fs::remove_file(counter);
        return Ok(unread);
    }
    let runs = std::fs::read_to_string(counter).ok().and_then(|text| text.trim().parse::<u32>().ok()).unwrap_or(0) + 1;
    std::fs::write(counter, runs.to_string()).map_err(|why| NorthError::Configuration(format!("{}: {why}", counter.display())))?;
    if runs >= UNREAD_RUNS_BEFORE_FAILING {
        return Err(NorthError::Configuration(format!("GitHub unread for {} in {runs} runs in a row", unread.join(", "))));
    }
    Ok(unread)
}

/// The listing of `repo` as `fetch` reads it, kept as the repository's last good rows; when the read fails, those rows
/// (none when it never succeeded) and the repository named in `unread`.
pub(super) fn listed(repo: &str, fetch: impl FnOnce() -> NorthResult<Held>, store: &Path, unread: &Mutex<BTreeSet<String>>) -> Held {
    match fetch() {
        Ok(held) => {
            let _ = serde_json::to_string(&held).map(|text| std::fs::write(store, text));
            held
        }
        Err(_) => {
            unread.lock().unwrap_or_else(PoisonError::into_inner).insert(repo.to_owned());
            std::fs::read_to_string(store).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
        }
    }
}

fn reports(windows: &[&str], now: Timestamp) -> NorthResult<(Vec<ClosedReport>, Vec<String>)> {
    let database = threads_database();
    let corroborated = super::corroborated::checked_boxes(&north_graph::default_path())?;
    let bounds = windows.iter().map(|window| window_bounds(window, now)).collect::<NorthResult<Vec<_>>>()?;
    let widest = bounds.iter().map(|(since, _)| since.clone()).min().unwrap_or_default();
    let until = now.iso();
    let fetched: Mutex<HashMap<String, Held>> = Mutex::default();
    let unread: Mutex<BTreeSet<String>> = Mutex::default();
    let github = |repo: &str, since: &str, end: &str| -> NorthResult<Listing> {
        let held = fetched.lock().unwrap_or_else(PoisonError::into_inner).get(repo).cloned();
        let Held { issues, open } = match held {
            Some(held) => held,
            None => {
                let held = listed(repo, || Ok(Held { issues: github_closed(repo, &widest, &until)?, open: github_open(repo)? }), &last_good(repo), &unread);
                fetched.lock().unwrap_or_else(PoisonError::into_inner).insert(repo.to_owned(), held.clone());
                held
            }
        };
        Ok(Listing { closed: within(&issues, since, end), open })
    };
    let reports = bounds.iter().map(|(since, end)| report_over(&database, since, end, |_| true, corroborated.clone(), github)).collect::<NorthResult<Vec<_>>>()?;
    Ok((reports, unread.into_inner().unwrap_or_else(PoisonError::into_inner).into_iter().collect()))
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
