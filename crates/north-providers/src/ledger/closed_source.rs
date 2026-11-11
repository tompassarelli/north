use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
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

use super::closed::{ClosedReport, SpendRun, summarize};
use crate::router::pricing::PriceTable;

const OWNER: &str = "tompassarelli";
const ISSUE_LIMIT: &str = "1000";

pub fn window_start(window: &str) -> NorthResult<String> {
    window_bounds(window, Timestamp::now()).map(|(since, _)| since)
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

pub use crate::threads::client::threads_database;

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
            "SELECT agent, item, {ROLE_SQL} AS role, tier, category, COALESCE(arm, '') AS arm, box, model, COALESCE(effort, '') AS effort, COALESCE(started, '') AS started, ended, (COALESCE(landed, 0) = 1 OR landed_sha IS NOT NULL) AS landed, \
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
            box_text: row["box"].as_str().filter(|text| !text.is_empty()).map(str::to_owned),
            model: row["model"].as_str().map(str::to_owned),
            effort: text(row, "effort"),
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

fn native_closed(database: &std::path::Path, since: &str, until: &str) -> NorthResult<HashMap<String, String>> {
    let query = format!(
        "SELECT ns || '#' || number AS item, body FROM items WHERE state = 'closed' AND closed >= '{}' AND closed < '{}'",
        since.replace('\'', ""),
        until.replace('\'', "")
    );
    Ok(sqlite::rows(database, Access::ReadOnly, Duration::from_secs(5), &query)?.iter().map(|row| (text(row, "item"), text(row, "body"))).collect())
}

fn github_closed(repo: &str, since: &str, until: &str) -> NorthResult<HashMap<String, String>> {
    let search = format!("closed:{}..{}", &since[..10], &until[..10]);
    let repository = format!("{OWNER}/{repo}");
    let issues = github::json(&["issue", "list", "-R", &repository, "--state", "closed", "--search", &search, "--limit", ISSUE_LIMIT, "--json", "number,closedAt,body"])
        .map_err(|error| NorthError::Configuration(format!("gh issue list {repository}: {}", error.trim())))?;
    Ok(issues
        .as_array()
        .into_iter()
        .flatten()
        .filter(|issue| issue["closedAt"].as_str().is_some_and(|at| at >= since && at < until))
        .filter_map(|issue| Some((format!("{repo}#{}", issue["number"].as_i64()?), text(issue, "body"))))
        .collect())
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
    let corroborated = super::corroborated::checked_boxes(&north_graph::default_path())?;
    report_over(&threads_database(), since, until, |_| true, corroborated)
}

/// The same runs without the closed-box report, for a caller that needs only what each run cost.
pub fn runs_inside(window: &str) -> NorthResult<Vec<WindowRun>> {
    runs_inside_in(&threads_database(), window, Timestamp::now())
}

/// The runs of every role that started between `now - window` and `now`, read from `database`, so a caller can read a
/// window other than this host's clock.
pub fn runs_inside_in(database: &Path, window: &str, now: Timestamp) -> NorthResult<Vec<WindowRun>> {
    let (since, until) = window_bounds(window, now)?;
    window_runs(database, &since, &until)
}

/// The runs of every role that started in the last `window`, each priced at API-equivalent rates, and the closed-box
/// report over those runs alone, so a box closed in the window counts only the spend its runs made inside it.
pub fn inside(window: &str) -> NorthResult<(Vec<WindowRun>, ClosedReport)> {
    let database = threads_database();
    let (since, until) = window_bounds(window, Timestamp::now())?;
    let corroborated = super::corroborated::checked_boxes(&north_graph::default_path())?;
    let started = |run: &SpendRun| run.started.as_str() >= since.as_str() && run.started.as_str() < until.as_str();
    Ok((window_runs(&database, &since, &until)?, report_over(&database, &since, &until, started, corroborated)?))
}

/// How often a run took over another run's work (a handover, revive or relaunch in a new conversation) and what the
/// first turn of those runs cost to write to the cache, both scaled to a week.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Handovers {
    pub per_week: f64,
    pub first_turn_cache_write_usd: f64,
}

/// The handovers of the last `window`: runs that began with a `Follows:`. A run's first turn writes its opening context
/// to the cache, which cannot exceed the run's peak context nor the input it left uncached, so the smaller of the two
/// is priced at the run model's cache write rate.
pub fn handovers(window: &str) -> NorthResult<Handovers> {
    handovers_in(&threads_database(), window, Timestamp::now())
}

pub fn handovers_in(database: &Path, window: &str, now: Timestamp) -> NorthResult<Handovers> {
    let (since, until) = window_bounds(window, now)?;
    let rows = sqlite::rows(
        database,
        Access::ReadOnly,
        Duration::from_secs(5),
        &format!(
            "SELECT tier, model, COALESCE(peak_ctx, 0) AS peak, COALESCE(input_tokens, 0) AS input, COALESCE(cache_read_tokens, 0) AS cache_read \
             FROM runs WHERE COALESCE(follows, '') != '' AND started >= '{}' AND started < '{}'",
            since.replace('\'', ""),
            until.replace('\'', "")
        ),
    )?;
    let prices = PriceTable::load()?;
    let first_turn_usd: f64 = rows
        .iter()
        .map(|row| {
            let written = (row["input"].as_i64().unwrap_or(0) - row["cache_read"].as_i64().unwrap_or(0)).max(0);
            let peak = row["peak"].as_i64().unwrap_or(0);
            let first_turn = if peak > 0 { written.min(peak) } else { written };
            prices.cache_write_usd(row["model"].as_str(), &text(row, "tier"), first_turn)
        })
        .sum();
    let week = 7.0 * 86_400.0 / span_seconds(window)? as f64;
    Ok(Handovers { per_week: rows.len() as f64 * week, first_turn_cache_write_usd: first_turn_usd * week })
}

/// The `[since, until)` bounds of `window` ending at `now`, both in the ledger's timestamp text.
fn window_bounds(window: &str, now: Timestamp) -> NorthResult<(String, String)> {
    Ok((Timestamp::from_unix_seconds(now.unix_seconds() - span_seconds(window)?).iso(), now.iso()))
}

fn report_over(database: &Path, since: &str, until: &str, keep: impl Fn(&SpendRun) -> bool, corroborated: Option<HashSet<String>>) -> NorthResult<ClosedReport> {
    let runs: Vec<SpendRun> = spend_runs(database)?.into_iter().filter(|run| run.ended.as_str() < until && keep(run)).collect();
    let native = native_namespaces(database)?;
    let closed = native_closed(database, since, until)?;
    report_between(&Inputs { runs, native, closed, corroborated, catalog: &CATALOG }, since, until, github_closed, &PriceTable::load()?)
}

struct Inputs<'a> {
    runs: Vec<SpendRun>,
    native: HashSet<String>,
    closed: HashMap<String, String>,
    corroborated: Option<HashSet<String>>,
    catalog: &'a Catalog,
}

/// The report over `sources`, asking `github` for the closed issues of each repository the native
/// ledger does not hold. A landed run naming a repository outside the catalog (an archived or
/// renamed one) can never match a closed issue, so it is left out of every cell and named in
/// `skipped` rather than failing the report for the repositories that are known.
fn report_between(sources: &Inputs, since: &str, until: &str, github: impl Fn(&str, &str, &str) -> NorthResult<HashMap<String, String>> + Sync, prices: &PriceTable) -> NorthResult<ClosedReport> {
    let mut landed: BTreeMap<&str, usize> = BTreeMap::new();
    for repo in sources.runs.iter().filter(|run| run.landed).filter_map(|run| run.item.as_deref()?.split_once('#').map(|(repo, _)| repo)) {
        *landed.entry(repo).or_default() += 1;
    }
    let known = |repo: &str| sources.catalog.repos.iter().chain(&sources.catalog.native).any(|known| known == repo);
    let mut closed = sources.closed.clone();
    let repos: Vec<&str> = landed.keys().copied().filter(|repo| known(repo) && !sources.native.contains(*repo)).collect();
    let read: Vec<NorthResult<HashMap<String, String>>> = std::thread::scope(|scope| {
        let reads: Vec<_> = repos.iter().map(|repo| scope.spawn(|| github(repo, since, until))).collect();
        reads.into_iter().map(|read| read.join().unwrap_or_else(|_| Err(NorthError::State("a closed-issue read panicked".into())))).collect()
    });
    for issues in read {
        closed.extend(issues?);
    }
    let items: HashSet<String> = closed.keys().cloned().collect();
    let mut report = summarize(&sources.runs, &items, since, prices).verified(&closed, sources.corroborated.as_ref());
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

/// A run's wall minutes and the ETA its brief stated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunTime {
    pub minutes: u64,
    pub eta_min: u64,
}

/// Each named run's wall minutes and stated ETA, for the runs the ledger holds one for.
pub fn run_times() -> NorthResult<BTreeMap<String, RunTime>> {
    run_times_in(&threads_database())
}

pub fn run_times_in(database: &std::path::Path) -> NorthResult<BTreeMap<String, RunTime>> {
    let rows = sqlite::rows(
        database,
        Access::ReadOnly,
        Duration::from_secs(5),
        "SELECT agent, minutes, eta_min FROM runs WHERE eta_min > 0 AND minutes >= 0",
    )?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let agent = text(row, "agent");
            Some((agent, RunTime { minutes: row["minutes"].as_u64()?, eta_min: row["eta_min"].as_u64()? }))
        })
        .filter(|(agent, _)| !agent.is_empty())
        .collect())
}

/// How the ledger settled a fleet session's newest worker run: whether it ended not done, and whether a boundary of the
/// harness or Tom recorded a cause for it, which hands the failure on rather than leaving it to the worker's rung.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RunEnd {
    pub open: bool,
    pub caused: bool,
}

/// Each fleet session's newest settled worker run end.
pub fn run_ends() -> NorthResult<BTreeMap<String, RunEnd>> {
    run_ends_in(&threads_database())
}

pub fn run_ends_in(database: &Path) -> NorthResult<BTreeMap<String, RunEnd>> {
    let failed = |why: rusqlite::Error| NorthError::Configuration(format!("run ends in {}: {why}", database.display()));
    let db = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
    db.busy_timeout(Duration::from_secs(5)).map_err(failed)?;
    let (outcome, cause) = (crate::threads::attribution::outcome_column(&db), crate::threads::attribution::recorded_column(&db));
    let sql = format!("SELECT runs.fleet_name, {outcome}, {cause} FROM runs WHERE runs.role = 'worker' AND coalesce(runs.fleet_name, '') != '' AND runs.outcome NOT IN ('running', 'idle') ORDER BY runs.started");
    let mut statement = db.prepare(&sql).map_err(failed)?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?))).map_err(failed)?;
    let mut ends = BTreeMap::new();
    for row in rows {
        let (session, outcome, cause) = row.map_err(failed)?;
        ends.insert(session, RunEnd { open: outcome.as_deref() == Some("not done"), caused: cause.is_some_and(|cause| cause != "worker") });
    }
    Ok(ends)
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

/// A run of any role that started inside a window, with its model family and effort and its API-equivalent cost so far.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowRun {
    pub agent: String,
    pub role: String,
    pub category: String,
    /// The dispatch tier as the closed-box cells name it, such as `opus-medium` or `sonnet`.
    pub tier: String,
    pub model: String,
    pub effort: String,
    /// The login that ran it as the ledger names it, such as `claude-b`, or `claude-api` for a run billed to an API organization's credit; empty when unrecorded.
    pub account: String,
    /// When it started, in the ledger's own timestamp text, which orders as time does.
    pub started: String,
    /// How the run ended, such as `done`, `not done` or `running`; empty when the ledger kept no outcome.
    pub outcome: String,
    pub usd: f64,
}

impl WindowRun {
    /// The model family and effort that ran it, such as `opus medium`.
    pub fn mix(&self) -> String {
        format!("{} {}", self.model, self.effort).trim().to_owned()
    }
}

fn window_runs(database: &Path, since: &str, until: &str) -> NorthResult<Vec<WindowRun>> {
    let rows = sqlite::rows(
        database,
        Access::ReadOnly,
        Duration::from_secs(5),
        &format!(
            "SELECT agent, {ROLE_SQL} AS role, tier, category, model, COALESCE(effort, '') AS effort, COALESCE(account, '') AS account, COALESCE(started, '') AS started, COALESCE(outcome, '') AS outcome, COALESCE(input_tokens, 0) AS input, COALESCE(cache_read_tokens, 0) AS cache_read, COALESCE(tokens, 0) AS output \
             FROM runs WHERE started >= '{}' AND started < '{}'",
            since.replace('\'', ""),
            until.replace('\'', "")
        ),
    )?;
    let prices = PriceTable::load()?;
    Ok(rows
        .iter()
        .map(|row| {
            let tier = text(row, "tier");
            let model = row["model"].as_str().filter(|model| !model.is_empty());
            let named = model.map_or_else(|| super::closed::tier_label(&tier), str::to_owned);
            let family = ["opus", "sonnet", "haiku", "fable"].into_iter().find(|family| named.contains(family));
            let effort = Some(text(row, "effort")).filter(|effort| !effort.is_empty()).or_else(|| ["low", "medium", "high", "xhigh", "max"].contains(&tier.as_str()).then(|| tier.clone()));
            WindowRun {
                agent: text(row, "agent"),
                role: text(row, "role"),
                category: text(row, "category"),
                tier: super::closed::tier_label(&tier),
                model: family.map_or(named.clone(), str::to_owned),
                effort: effort.unwrap_or_default(),
                account: text(row, "account"),
                started: text(row, "started"),
                outcome: text(row, "outcome"),
                usd: prices.run_usd(model, &tier, row["input"].as_i64().unwrap_or(0), row["cache_read"].as_i64().unwrap_or(0), row["output"].as_i64().unwrap_or(0)),
            }
        })
        .collect())
}
