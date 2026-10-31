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
    Ok(Timestamp::from_unix_seconds(Timestamp::now().unix_seconds() - span_seconds(window)?).iso())
}

fn span_seconds(window: &str) -> NorthResult<i64> {
    let split = window.find(|character: char| !character.is_ascii_digit()).ok_or_else(|| usage(window))?;
    let (count, unit) = window.split_at(split);
    let seconds = match unit {
        "h" => 3_600,
        "d" => 86_400,
        _ => return Err(usage(window)),
    };
    Ok(count.parse::<i64>().map_err(|_| usage(window))? * seconds)
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
        "SELECT item, role, tier, category, COALESCE(arm, '') AS arm, model, COALESCE(started, '') AS started, ended, (COALESCE(landed, 0) = 1 OR landed_sha IS NOT NULL) AS landed, \
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
            arm: text(row, "arm"),
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

fn native_closed(database: &std::path::Path, since: &str, until: &str) -> NorthResult<HashSet<String>> {
    let query = format!(
        "SELECT ns || '#' || number AS item FROM items WHERE state = 'closed' AND closed >= '{}' AND closed < '{}'",
        since.replace('\'', ""),
        until.replace('\'', "")
    );
    Ok(sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), &query)?.iter().map(|row| text(row, "item")).collect())
}

fn github_closed(repo: &str, since: &str, until: &str) -> NorthResult<HashSet<String>> {
    let search = format!("closed:{}..{}", &since[..10], &until[..10]);
    let repository = format!("{OWNER}/{repo}");
    let issues = github::json(&["issue", "list", "-R", &repository, "--state", "closed", "--search", &search, "--limit", ISSUE_LIMIT, "--json", "number,closedAt"])
        .map_err(|error| NorthError::Configuration(format!("gh issue list {repository}: {}", error.trim())))?;
    Ok(issues
        .as_array()
        .into_iter()
        .flatten()
        .filter(|issue| issue["closedAt"].as_str().is_some_and(|at| at >= since && at < until))
        .filter_map(|issue| Some(format!("{repo}#{}", issue["number"].as_i64()?)))
        .collect())
}

/// The runs of `category` stamped with each arm, by arm name.
pub fn arm_runs(category: &str) -> NorthResult<BTreeMap<String, usize>> {
    let query = format!("SELECT arm, COUNT(*) AS runs FROM runs WHERE lower(category) = '{}' AND COALESCE(arm, '') != '' GROUP BY arm", category.to_lowercase().replace('\'', ""));
    let rows = sqlite::rows(&threads_database(), Access::ReadOnly, Duration::from_secs(5), &query)?;
    Ok(rows.iter().map(|row| (text(row, "arm"), usize::try_from(row["runs"].as_i64().unwrap_or(0)).unwrap_or(0))).collect())
}

pub fn closed_report(window: &str) -> NorthResult<ClosedReport> {
    closed_report_between(&window_start(window)?, &Timestamp::now().iso())
}

/// The closed-box reports of the last `window` and of the `window` before it, both measured from
/// one clock reading so no box falls in both or in neither.
pub fn closed_report_and_previous(window: &str) -> NorthResult<(ClosedReport, ClosedReport)> {
    let [(since, until), (earlier, start)] = adjacent_windows(Timestamp::now(), span_seconds(window)?);
    Ok((closed_report_between(&since, &until)?, closed_report_between(&earlier, &start)?))
}

/// The `[since, until)` bounds of the `span` seconds ending `now` and of the span before it.
pub fn adjacent_windows(now: Timestamp, span: i64) -> [(String, String); 2] {
    let at = |back: i64| Timestamp::from_unix_seconds(now.unix_seconds() - back).iso();
    [(at(span), at(0)), (at(2 * span), at(span))]
}

fn closed_report_between(since: &str, until: &str) -> NorthResult<ClosedReport> {
    let database = threads_database();
    let runs: Vec<SpendRun> = spend_runs(&database)?.into_iter().filter(|run| run.ended.as_str() < until).collect();
    let native = native_namespaces(&database)?;
    let mut closed = native_closed(&database, since, until)?;
    let repos: HashSet<&str> = runs.iter().filter(|run| run.landed).filter_map(|run| run.item.as_deref()?.split_once('#').map(|(repo, _)| repo)).collect();
    for repo in repos.iter().filter(|repo| !native.contains(**repo)) {
        closed.extend(github_closed(repo, since, until)?);
    }
    Ok(summarize(&runs, &closed, since, &PriceTable::load()?))
}

pub fn window_start_ms(window: &str) -> NorthResult<i64> {
    Ok(Timestamp::parse(&window_start(window)?)?.unix_millis())
}

/// A worker's Not done or Stop report and when its run ended, as unix seconds.
#[derive(Clone, Debug, PartialEq)]
pub struct PriorReport {
    pub ended: f64,
    pub report: String,
}

/// Each item's Not done or Stop reports, newest first, as the ledger stored them.
pub fn prior_reports() -> NorthResult<BTreeMap<String, Vec<PriorReport>>> {
    let rows = sqlite::rows(
        &threads_database(),
        Access::ReadOnly,
        Duration::from_secs(5),
        "SELECT item, ended, report FROM runs WHERE item IS NOT NULL AND report != '' AND role = 'worker' AND (outcome = 'not done' OR report LIKE '%Stop:%') ORDER BY ended DESC",
    )?;
    let mut found: BTreeMap<String, Vec<PriorReport>> = BTreeMap::new();
    for row in &rows {
        let ended = Timestamp::parse_rfc3339(&text(row, "ended")).map_or(0.0, Timestamp::unix_seconds_f64);
        found.entry(text(row, "item")).or_default().push(PriorReport { ended, report: text(row, "report") });
    }
    Ok(found)
}
