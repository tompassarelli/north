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

use crate::threads::calibration::rework::{Rework, Sources};
use crate::threads::catalog::{CATALOG, Catalog};

use super::closed::{ClosedReport, SpendRun, summarize};
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
    closed: HashSet<String>,
    catalog: &'a Catalog,
}

/// The report over `sources`, asking `github` for the closed issues of each repository the native
/// ledger does not hold. A landed run naming a repository outside the catalog (an archived or
/// renamed one) can never match a closed issue, so it is left out of every cell and named in
/// `skipped` rather than failing the report for the repositories that are known.
fn report_between(sources: &Inputs, since: &str, until: &str, github: impl Fn(&str, &str, &str) -> NorthResult<HashSet<String>>, prices: &PriceTable) -> NorthResult<ClosedReport> {
    let mut landed: BTreeMap<&str, usize> = BTreeMap::new();
    for repo in sources.runs.iter().filter(|run| run.landed).filter_map(|run| run.item.as_deref()?.split_once('#').map(|(repo, _)| repo)) {
        *landed.entry(repo).or_default() += 1;
    }
    let known = |repo: &str| sources.catalog.repos.iter().chain(&sources.catalog.native).any(|known| known == repo);
    let mut closed = sources.closed.clone();
    for repo in landed.keys().filter(|repo| known(repo) && !sources.native.contains(**repo)) {
        closed.extend(github(repo, since, until)?);
    }
    let mut report = summarize(&sources.runs, &closed, since, prices);
    report.skipped = landed.into_iter().filter(|(repo, _)| !known(repo)).map(|(repo, runs)| (repo.to_owned(), runs)).collect();
    Ok(report)
}

pub fn window_start_ms(window: &str) -> NorthResult<i64> {
    Ok(Timestamp::parse(&window_start(window)?)?.unix_millis())
}

/// A finished worker run that was launched with an estimate: its minutes and cost against the ETA
/// and the expected spend it carried, the spend absent while no run records one.
#[derive(Clone, Debug, PartialEq)]
pub struct EstimatedRun {
    pub tier: String,
    pub category: String,
    pub minutes: f64,
    pub eta_min: f64,
    pub usd: f64,
    pub estimated_usd: Option<f64>,
}

/// The worker runs that ended in the last `window` with an ETA and a wall time.
pub fn estimated_runs(window: &str) -> NorthResult<Vec<EstimatedRun>> {
    let since = window_start(window)?;
    let rows = sqlite::rows(
        &threads_database(),
        Access::ReadOnly,
        Duration::from_secs(5),
        &format!(
            "SELECT {ROLE_SQL} AS role, tier, category, model, minutes, eta_min, COALESCE(input_tokens, 0) AS input, COALESCE(cache_read_tokens, 0) AS cache_read, COALESCE(tokens, 0) AS output \
             FROM runs WHERE outcome != 'running' AND ended >= '{}' AND eta_min > 0 AND minutes > 0",
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
            Some(EstimatedRun { tier: super::closed::tier_label(&tier), category: text(row, "category"), minutes: row["minutes"].as_f64()?, eta_min: row["eta_min"].as_f64()?, usd, estimated_usd: None })
        })
        .collect())
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

#[cfg(test)]
mod tests {
    use super::{CATALOG, Inputs, report_between, spend_runs};
    use crate::ledger::SpendRun;
    use crate::ledger::closed::summarize;
    use crate::router::pricing::PriceTable;

    #[test]
    fn runs_per_closed_counts_each_box_of_an_umbrella_item_and_no_lead_proxy_reviewer_judge_routine_probe_or_running_row() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("threads.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE runs(agent TEXT PRIMARY KEY, item TEXT, role TEXT NOT NULL DEFAULT 'worker', tier TEXT NOT NULL, category TEXT NOT NULL, arm TEXT, model TEXT, started TEXT, ended TEXT NOT NULL, minutes INTEGER NOT NULL, outcome TEXT NOT NULL, landed INTEGER, landed_sha TEXT, landed_at TEXT, input_tokens INTEGER, cache_read_tokens INTEGER, tokens INTEGER, brief_text TEXT) STRICT",
        )
        .unwrap();
        let rows = [
            ("box-1", "north#242", "worker", "feature", "done", 30, 1, ""),
            ("box-2", "north#242", "worker", "feature", "done", 25, 1, ""),
            ("box-3", "north#242", "worker", "feature", "not done", 40, 0, ""),
            ("box-3-rerun", "north#242", "worker", "feature", "done", 20, 1, ""),
            ("single", "north#247", "worker", "feature", "done", 15, 1, ""),
            ("lead-turn", "north#242", "parent", "feature", "done", 60, 1, ""),
            ("proxy-turn", "north#247", "proxy", "unknown", "done", 5, 1, ""),
            ("reviewer", "north#242", "worker", "unknown", "done", 12, 0, "Item: north#242\nCategory: review"),
            ("judge", "north#247", "worker", "unknown", "done", 0, 0, ""),
            ("routine", "north#242", "routine", "unknown", "done", 3, 0, ""),
            ("probe", "north#247", "probe", "unknown", "done", 1, 0, ""),
            ("open-item", "north#999", "worker", "feature", "done", 10, 1, ""),
        ];
        for (agent, item, role, category, outcome, minutes, landed, brief) in rows {
            db.execute(
                "INSERT INTO runs(agent, item, role, tier, category, started, ended, minutes, outcome, landed, brief_text, model) VALUES (?1, ?2, ?3, 'claude:sonnet-high', ?4, '2026-11-04T12:00:00Z', '2026-11-04T13:00:00Z', ?5, ?6, ?7, ?8, 'claude-sonnet-5-5')",
                rusqlite::params![agent, item, role, category, minutes, outcome, landed, brief],
            )
            .unwrap();
        }
        db.execute("INSERT INTO runs(agent, item, role, tier, category, started, ended, minutes, outcome) VALUES ('running', 'north#242', 'worker', 'claude:sonnet-high', 'feature', '2026-11-05T09:00:00Z', '', 0, 'running')", []).unwrap();
        let runs = spend_runs(&path).unwrap();
        let closed = ["north#242", "north#247"].map(String::from).into();
        let report = summarize(&runs, &closed, "2026-11-04T10:20:00Z", &PriceTable::load().unwrap());
        assert_eq!(report.runs_per_closed(), [("feature".to_owned(), 5, 2)], "four worker runs on north#242's three boxes and one on north#247");
        assert_eq!(report.runs_line("24h"), "runs/closed 24h: feature 2.50 (5/2)");
    }

    fn run(item: &str, tier: &str, model: &str, category: &str) -> SpendRun {
        SpendRun {
            agent: String::new(),
            item: Some(item.into()),
            role: "worker".into(),
            tier: tier.into(),
            category: category.into(),
            arm: String::new(),
            model: Some(model.into()),
            started: "2026-11-03T10:00:00Z".into(),
            ended: "2026-11-03T10:30:00Z".into(),
            landed: true,
            reworked: false,
            input: 3_000_000,
            cache_read: 2_000_000,
            output: 100_000,
            skills: Vec::new(),
        }
    }

    #[test]
    fn landed_runs_of_an_archived_repository_are_named_and_every_known_cell_is_still_priced() {
        let runs = vec![
            run("north#1", "claude:sonnet-high", "claude-sonnet-5-5", "feature"),
            run("north#2", "claude:opus-medium", "claude-opus-5-5", "feature"),
            run("north#3", "claude:sonnet-high", "claude-sonnet-5-5", "tooling"),
            run("north#4", "claude:opus-medium", "claude-opus-5-5", "tooling"),
            run("galileo#7", "claude:sonnet-high", "claude-sonnet-5-5", "feature"),
            run("galileo#8", "claude:sonnet-high", "claude-sonnet-5-5", "feature"),
        ];
        let sources = Inputs { runs, native: ["north".to_owned()].into(), closed: ["north#1", "north#2", "north#3", "north#4"].map(String::from).into(), catalog: &CATALOG };
        let report = report_between(&sources, "2026-11-01T00:00:00Z", "2026-11-04T00:00:00Z", |repo, _, _| panic!("{repo} asked github"), &PriceTable::load().unwrap()).unwrap();
        assert_eq!(report.cells.len(), 4, "{:?}", report.cells);
        assert!(report.cells.iter().all(|cell| cell.closed == 1 && cell.usd_per_closed() > 0.0));
        assert_eq!(report.skipped_line().unwrap(), "skipped repositories the catalog does not know, so their boxes are not counted: galileo (2 landed runs)");
    }
}
