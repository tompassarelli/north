use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::platform::sqlite::{self, Access};
use crate::platform::github;

use super::closed::{ClosedReport, SpendRun, summarize};
use super::pricing::PriceTable;

const OWNER: &str = "tompassarelli";
const ISSUE_LIMIT: &str = "1000";

pub fn window_start(window: &str) -> NorthResult<String> {
    let split = window.find(|character: char| !character.is_ascii_digit()).ok_or_else(|| usage(window))?;
    let (count, unit) = window.split_at(split);
    let seconds = match unit {
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err(usage(window)),
    };
    let span = count.parse::<i64>().map_err(|_| usage(window))? * seconds;
    Ok(Timestamp::from_unix_seconds(Timestamp::now().unix_seconds() - span).iso())
}

fn usage(window: &str) -> NorthError {
    NorthError::Usage(format!("--since takes hours or days such as 24h or 7d, not {window:?}"))
}

fn threads_database() -> PathBuf {
    if let Some(path) = std::env::var_os("THREADS_DB") {
        return path.into();
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = home.join(".local/state/threads");
    state.join(if home.join(".config/threads/remote").exists() { "snapshot.db" } else { "threads.db" })
}

fn text(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or_default().to_owned()
}

fn spend_runs(database: &std::path::Path) -> NorthResult<Vec<SpendRun>> {
    let rows = sqlite::rows(
        database,
        Access::ReadOnly,
        Duration::from_secs(5),
        "SELECT item, role, tier, category, model, COALESCE(started, '') AS started, ended, (COALESCE(landed, 0) = 1 OR landed_sha IS NOT NULL) AS landed, \
         COALESCE(input_tokens, 0) AS input, COALESCE(cache_read_tokens, 0) AS cache_read, COALESCE(tokens, 0) AS output \
         FROM runs WHERE outcome != 'running' AND ended != ''",
    )?;
    Ok(rows
        .iter()
        .map(|row| SpendRun {
            item: row["item"].as_str().map(str::to_owned),
            role: text(row, "role"),
            tier: text(row, "tier"),
            category: text(row, "category"),
            model: row["model"].as_str().map(str::to_owned),
            started: text(row, "started"),
            ended: text(row, "ended"),
            landed: row["landed"].as_i64() == Some(1),
            input: row["input"].as_i64().unwrap_or(0),
            cache_read: row["cache_read"].as_i64().unwrap_or(0),
            output: row["output"].as_i64().unwrap_or(0),
        })
        .collect())
}

fn native_namespaces(database: &std::path::Path) -> NorthResult<HashSet<String>> {
    let rows = sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), "SELECT DISTINCT ns FROM items")?;
    Ok(rows.iter().map(|row| text(row, "ns")).collect())
}

fn native_closed(database: &std::path::Path, since: &str) -> NorthResult<HashSet<String>> {
    let query = format!("SELECT ns || '#' || number AS item FROM items WHERE state = 'closed' AND closed >= '{}'", since.replace('\'', ""));
    Ok(sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), &query)?.iter().map(|row| text(row, "item")).collect())
}

fn github_closed(repo: &str, since: &str) -> NorthResult<HashSet<String>> {
    let search = format!("closed:>={}", &since[..10]);
    let repository = format!("{OWNER}/{repo}");
    let issues = github::json(&["issue", "list", "-R", &repository, "--state", "closed", "--search", &search, "--limit", ISSUE_LIMIT, "--json", "number,closedAt"])
        .map_err(|error| NorthError::Configuration(format!("gh issue list {repository}: {}", error.trim())))?;
    Ok(issues
        .as_array()
        .into_iter()
        .flatten()
        .filter(|issue| issue["closedAt"].as_str().is_some_and(|at| at >= since))
        .filter_map(|issue| Some(format!("{repo}#{}", issue["number"].as_i64()?)))
        .collect())
}

pub fn closed_report(window: &str) -> NorthResult<ClosedReport> {
    let since = window_start(window)?;
    let database = threads_database();
    let runs = spend_runs(&database)?;
    let native = native_namespaces(&database)?;
    let mut closed = native_closed(&database, &since)?;
    let repos: HashSet<&str> = runs.iter().filter(|run| run.landed).filter_map(|run| run.item.as_deref()?.split_once('#').map(|(repo, _)| repo)).collect();
    for repo in repos.iter().filter(|repo| !native.contains(**repo)) {
        closed.extend(github_closed(repo, &since)?);
    }
    Ok(summarize(&runs, &closed, &since, &PriceTable::load()?))
}

pub fn window_start_ms(window: &str) -> NorthResult<i64> {
    Ok(Timestamp::parse(&window_start(window)?)?.unix_millis())
}

pub const PRIOR_REPORTS: usize = 2;

/// Each item's newest Not done or Stop reports, newest first, as the ledger stored them.
pub fn prior_reports() -> NorthResult<BTreeMap<String, Vec<String>>> {
    let rows = sqlite::rows(
        &threads_database(),
        Access::ReadOnly,
        Duration::from_secs(5),
        "SELECT item, report FROM runs WHERE item IS NOT NULL AND report != '' AND role = 'worker' AND (outcome = 'not done' OR report LIKE '%Stop:%') ORDER BY ended DESC",
    )?;
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in &rows {
        let reports = found.entry(text(row, "item")).or_default();
        if reports.len() < PRIOR_REPORTS {
            reports.push(text(row, "report"));
        }
    }
    Ok(found)
}
