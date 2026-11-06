use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::ROLE_SQL;
use crate::platform::sqlite::{self, Access};
use crate::platform::github;

use crate::threads::calibration::predictions::{self, Brier};
use crate::threads::calibration::rework::{Rework, Sources};
use crate::threads::catalog::{CATALOG, Catalog};

use super::closed::{ClosedReport, SpendRun, checker_verified, summarize};
use crate::router::pricing::PriceTable;

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

/// The walk-forward `eta_pred` calibration of `tier` in the threads ledger: its scale when calibrated, and its label.
pub fn eta_calibration(tier: &str) -> NorthResult<(Option<f64>, String)> {
    crate::threads::calibration::eta_pred_scale(&threads_database(), Timestamp::now(), tier).map_err(NorthError::State)
}

pub fn threads_database() -> PathBuf {
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
    let reworked = reworked_agents(database)?;
    let mut skills = run_skills(database)?;
    let rows = sqlite::rows(
        database,
        Access::ReadOnly,
        Duration::from_secs(5),
        &format!(
            "SELECT agent, item, {ROLE_SQL} AS role, tier, category, COALESCE(arm, '') AS arm, model, COALESCE(started, '') AS started, ended, (COALESCE(landed, 0) = 1 OR landed_sha IS NOT NULL) AS landed, \
         COALESCE(input_tokens, 0) AS input, COALESCE(cache_read_tokens, 0) AS cache_read, COALESCE(tokens, 0) AS output \
         FROM runs WHERE outcome != 'running' AND ended != ''"
        ),
    )?;
    Ok(rows
        .iter()
        .map(|row| SpendRun {
            agent: text(row, "agent"),
            item: row["item"].as_str().map(str::to_owned),
            role: text(row, "role"),
            tier: text(row, "tier"),
            category: text(row, "category"),
            arm: text(row, "arm"),
            model: row["model"].as_str().map(str::to_owned),
            started: text(row, "started"),
            ended: text(row, "ended"),
            landed: row["landed"].as_i64() == Some(1),
            reworked: reworked.contains(row["agent"].as_str().unwrap_or_default()),
            input: row["input"].as_i64().unwrap_or(0),
            cache_read: row["cache_read"].as_i64().unwrap_or(0),
            output: row["output"].as_i64().unwrap_or(0),
            skills: skills.remove(row["agent"].as_str().unwrap_or_default()).unwrap_or_default(),
        })
        .collect())
}

/// Each run's loaded skills by agent, empty on a ledger the threads migration has not given the column yet.
fn run_skills(database: &std::path::Path) -> NorthResult<HashMap<String, Vec<String>>> {
    let column = sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), "SELECT name FROM pragma_table_info('runs') WHERE name = 'skills'")?;
    if column.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), "SELECT agent, skills FROM runs WHERE skills IS NOT NULL AND skills != '[]'")?;
    Ok(rows.iter().map(|row| (text(row, "agent"), serde_json::from_str(row["skills"].as_str().unwrap_or_default()).unwrap_or_default())).collect())
}

fn reworked_agents(database: &std::path::Path) -> NorthResult<HashSet<String>> {
    let db = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(|why| NorthError::Configuration(format!("ledger {}: {why}", database.display())))?;
    let rework = Rework::read(&db, &Sources::live()).map_err(|why| NorthError::Configuration(format!("rework from {}: {why}", database.display())))?;
    Ok(rework.reworked().cloned().collect())
}

fn native_namespaces(database: &std::path::Path) -> NorthResult<HashSet<String>> {
    let rows = sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), "SELECT DISTINCT ns FROM items")?;
    Ok(rows.iter().map(|row| text(row, "ns")).collect())
}

fn native_closed(database: &std::path::Path, since: &str, until: &str) -> NorthResult<HashMap<String, bool>> {
    let query = format!(
        "SELECT ns || '#' || number AS item, body FROM items WHERE state = 'closed' AND closed >= '{}' AND closed < '{}'",
        since.replace('\'', ""),
        until.replace('\'', "")
    );
    Ok(sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), &query)?.iter().map(|row| (text(row, "item"), checker_verified(&text(row, "body")))).collect())
}

fn github_closed(repo: &str, since: &str, until: &str) -> NorthResult<HashMap<String, bool>> {
    let search = format!("closed:{}..{}", &since[..10], &until[..10]);
    let repository = format!("{OWNER}/{repo}");
    let issues = github::json(&["issue", "list", "-R", &repository, "--state", "closed", "--search", &search, "--limit", ISSUE_LIMIT, "--json", "number,closedAt,body"])
        .map_err(|error| NorthError::Configuration(format!("gh issue list {repository}: {}", error.trim())))?;
    Ok(issues
        .as_array()
        .into_iter()
        .flatten()
        .filter(|issue| issue["closedAt"].as_str().is_some_and(|at| at >= since && at < until))
        .filter_map(|issue| Some((format!("{repo}#{}", issue["number"].as_i64()?), checker_verified(issue["body"].as_str().unwrap_or_default()))))
        .collect())
}

/// The runs of `category` stamped with each arm, by arm name.
pub fn arm_runs(category: &str) -> NorthResult<BTreeMap<String, usize>> {
    arm_runs_in(&threads_database(), category)
}

/// The runs of `category` stamped with each arm in `database`, by arm name.
pub fn arm_runs_in(database: &std::path::Path, category: &str) -> NorthResult<BTreeMap<String, usize>> {
    let query = format!("SELECT arm, COUNT(*) AS runs FROM runs WHERE lower(category) = '{}' AND COALESCE(arm, '') != '' GROUP BY arm", category.to_lowercase().replace('\'', ""));
    let rows = sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), &query)?;
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
    let closed = native_closed(&database, since, until)?;
    report_between(&Inputs { runs, native, closed, catalog: &CATALOG }, since, until, github_closed, &PriceTable::load()?)
}

struct Inputs<'a> {
    runs: Vec<SpendRun>,
    native: HashSet<String>,
    closed: HashMap<String, bool>,
    catalog: &'a Catalog,
}

/// The report over `sources`, asking `github` for the closed issues of each repository the native
/// ledger does not hold. A landed run naming a repository outside the catalog (an archived or
/// renamed one) can never match a closed issue, so it is left out of every cell and named in
/// `skipped` rather than failing the report for the repositories that are known.
fn report_between(sources: &Inputs, since: &str, until: &str, github: impl Fn(&str, &str, &str) -> NorthResult<HashMap<String, bool>>, prices: &PriceTable) -> NorthResult<ClosedReport> {
    let mut landed: BTreeMap<&str, usize> = BTreeMap::new();
    for repo in sources.runs.iter().filter(|run| run.landed).filter_map(|run| run.item.as_deref()?.split_once('#').map(|(repo, _)| repo)) {
        *landed.entry(repo).or_default() += 1;
    }
    let known = |repo: &str| sources.catalog.repos.iter().chain(&sources.catalog.native).any(|known| known == repo);
    let mut closed = sources.closed.clone();
    for repo in landed.keys().filter(|repo| known(repo) && !sources.native.contains(**repo)) {
        closed.extend(github(repo, since, until)?);
    }
    let verified: HashSet<String> = closed.iter().filter(|(_, verified)| **verified).map(|(item, _)| item.clone()).collect();
    let closed: HashSet<String> = closed.into_keys().collect();
    let mut report = summarize(&sources.runs, &closed, since, prices).verified(&verified);
    report.skipped = landed.into_iter().filter(|(repo, _)| !known(repo)).map(|(repo, runs)| (repo.to_owned(), runs)).collect();
    Ok(report)
}

pub fn window_start_ms(window: &str) -> NorthResult<i64> {
    Ok(Timestamp::parse(&window_start(window)?)?.unix_millis())
}

/// A finished worker run that was launched with an estimate: its minutes and cost against the ETA
/// and the Budget (expected spend) it carried, the ETA 0 and the Budget absent when its brief set none.
#[derive(Clone, Debug, PartialEq)]
pub struct EstimatedRun {
    pub tier: String,
    pub category: String,
    pub minutes: f64,
    pub eta_min: f64,
    pub usd: f64,
    pub estimated_usd: Option<f64>,
}

/// The worker runs that ended in the last `window` with a wall time and an ETA or a Budget.
pub fn estimated_runs(window: &str) -> NorthResult<Vec<EstimatedRun>> {
    let since = window_start(window)?;
    let rows = sqlite::rows(
        &threads_database(),
        Access::ReadOnly,
        Duration::from_secs(5),
        &format!(
            "SELECT {ROLE_SQL} AS role, tier, category, model, minutes, COALESCE(eta_min, 0) AS eta_min, budget_usd, COALESCE(input_tokens, 0) AS input, COALESCE(cache_read_tokens, 0) AS cache_read, COALESCE(tokens, 0) AS output \
             FROM runs WHERE outcome != 'running' AND ended >= '{}' AND (eta_min > 0 OR budget_usd > 0) AND minutes > 0",
            since.replace('\'', "")
        ),
    )?;
    let prices = PriceTable::load()?;
    Ok(rows
        .iter()
        .filter(|row| row["role"].as_str() == Some("worker"))
        .filter_map(|row| {
            let tier = text(row, "tier");
            let usd = prices.run_usd(row["model"].as_str(), &tier, row["input"].as_i64().unwrap_or(0), row["cache_read"].as_i64().unwrap_or(0), row["output"].as_i64().unwrap_or(0));
            Some(EstimatedRun { tier: super::closed::tier_label(&tier), category: text(row, "category"), minutes: row["minutes"].as_f64()?, eta_min: row["eta_min"].as_f64()?, usd, estimated_usd: row["budget_usd"].as_f64().filter(|budget| *budget > 0.0) })
        })
        .collect())
}

/// Each category's tier success predictions scored against the first runs that ended in the window.
pub fn tier_briers(window: &str) -> NorthResult<Vec<Brier>> {
    let since = window_start(window)?;
    let database = threads_database();
    let failed = |why: rusqlite::Error| NorthError::Configuration(format!("tier predictions in {}: {why}", database.display()));
    let db = Connection::open_with_flags(&database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
    predictions::score(&db, &since).map_err(failed)
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
