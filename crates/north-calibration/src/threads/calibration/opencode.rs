//! `north ledger opencode`: what each DeepSeek model attempted, landed and cost each week, against the ledger's Sonnet rate and the Go plan's own windows.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use rusqlite::{Connection, params, params_from_iter};

use super::boxes::moment;
use super::summary::has_table;
use super::verbs::since_start;
use super::median;
use crate::foundation::clock::Timestamp;
use crate::threads::verb::{Args, Context, Failure, open, report, table};

const DEEPSEEK: &str = "(provider IN ('opencode', 'pi') OR lower(coalesce(model, '')) LIKE '%deepseek%')";
const GO_FULL_PCT: f64 = 99.5;
const WEEK_SECS: i64 = 7 * 86_400;

struct Run {
    agent: String,
    item: Option<String>,
    outcome: String,
    landed: bool,
    landed_sha: Option<String>,
    started: Option<String>,
    ended: String,
    category: String,
    model: Option<String>,
    staffed: Option<String>,
}

impl Run {
    fn read(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            agent: row.get(0)?,
            item: row.get(1)?,
            outcome: row.get(2)?,
            landed: row.get::<_, Option<i64>>(3)?.is_some_and(|landed| landed != 0),
            landed_sha: row.get(4)?,
            started: row.get(5)?,
            ended: row.get(6)?,
            category: row.get(7)?,
            model: row.get(8)?,
            staffed: row.get(9)?,
        })
    }

    fn begin(&self) -> Option<String> {
        self.started.clone().filter(|started| !started.is_empty()).or_else(|| Some(self.ended.clone()))
    }

    fn landed_done(&self) -> bool {
        self.outcome == "done" && self.landed
    }
}

/// The paid Go model and the free model's rows: every other DeepSeek model keeps its own name.
fn model_class(model: Option<&str>) -> String {
    let lower = model.unwrap_or_default().to_lowercase();
    if lower.contains("deepseek") && lower.contains("free") {
        "free-flash".to_owned()
    } else if lower.contains("deepseek") {
        "go-flash".to_owned()
    } else {
        model.filter(|model| !model.is_empty()).unwrap_or("-").to_owned()
    }
}

/// The Monday 00:00Z that opens `at`'s week, as a date.
fn week_of(at: &str) -> Option<String> {
    let instant = moment(at)?;
    let day = instant.unix_seconds().div_euclid(86_400);
    let monday = day - (day + 3).rem_euclid(7);
    Some(Timestamp::from_unix_seconds(monday * 86_400).iso_date())
}

fn week_close(week: &str) -> Option<i64> {
    moment(&format!("{week}T00:00:00Z")).map(|start| start.unix_seconds() + WEEK_SECS)
}

/// The Go window a reading names: its 5-hour, weekly or monthly share.
fn window_of(name: &str) -> Option<&'static str> {
    let name = name.to_lowercase();
    if name.contains("5h") || name.contains("five_hour") || name == "session" {
        Some("5h")
    } else if name.contains("week") || name == "7d" {
        Some("week")
    } else if name.contains("month") || name == "30d" {
        Some("month")
    } else {
        None
    }
}

struct Reading {
    at: Timestamp,
    window: &'static str,
    used: f64,
}

fn readings(db: &Connection, from: &str) -> Vec<Reading> {
    if !has_table(db, "usage").unwrap_or(false) {
        return Vec::new();
    }
    let mut statement = match db.prepare("SELECT ts, name, used_pct FROM usage WHERE kind = 'window' AND used_pct IS NOT NULL AND lower(provider) LIKE 'opencode%' AND ts >= ?1") {
        Ok(statement) => statement,
        Err(_) => return Vec::new(),
    };
    statement
        .query_map([from], |row| {
            let ts: String = row.get(0)?;
            let name: String = row.get(1)?;
            let used: f64 = row.get(2)?;
            Ok((ts, name, used))
        })
        .map(|rows| {
            rows.filter_map(Result::ok)
                .filter_map(|(ts, name, used)| Some(Reading { at: moment(&ts)?, window: window_of(&name)?, used }))
                .collect()
        })
        .unwrap_or_default()
}

fn readings_in(readings: &[Reading], start: i64, end: i64) -> impl Iterator<Item = &Reading> {
    readings.iter().filter(move |reading| (start..end).contains(&reading.at.unix_seconds()))
}

/// The agents a later Claude run follows, and the Claude dollars that followed them.
fn escalations(db: &Connection, agents: &[String]) -> rusqlite::Result<BTreeMap<String, f64>> {
    let mut cost: BTreeMap<String, f64> = BTreeMap::new();
    if agents.is_empty() {
        return Ok(cost);
    }
    let holders = vec!["?"; agents.len()].join(",");
    let mut statement = db.prepare(&format!("SELECT follows, provider, coalesce(cost_usd, 0) FROM runs WHERE follows IN ({holders})"))?;
    let mut rows = statement.query(params_from_iter(agents.iter().map(String::as_str)))?;
    while let Some(row) = rows.next()? {
        let follows: Option<String> = row.get(0)?;
        let provider: Option<String> = row.get(1)?;
        let Some(follows) = follows else { continue };
        if provider.as_deref().is_some_and(|provider| matches!(provider, "opencode" | "pi" | "codex" | "agy")) {
            continue;
        }
        *cost.entry(follows).or_default() += row.get::<_, f64>(2)?;
    }
    Ok(cost)
}

fn checked(db: &Connection, run: &Run) -> bool {
    let (Some(item), Some(sha)) = (run.item.as_deref(), run.landed_sha.as_deref()) else { return false };
    if !has_table(db, "done_checks").unwrap_or(false) {
        return false;
    }
    db.query_row("SELECT 1 FROM done_checks WHERE task_ref = ?1 AND lower(head_sha) = lower(?2) AND exit = 0 LIMIT 1", params![item, sha], |_| Ok(()))
        .is_ok()
}

/// The ledger's measured Sonnet rate: the median landed Sonnet run's dollars in each category, else over all of them.
fn sonnet_rate(db: &Connection, from: &str) -> (BTreeMap<String, f64>, Option<f64>) {
    let mut by_category: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let Ok(mut statement) = db.prepare(
        "SELECT category, cost_usd FROM runs WHERE role = 'worker' AND landed = 1 AND coalesce(cost_usd, 0) > 0 AND lower(coalesce(model, '')) LIKE '%sonnet%' AND coalesce(started, ended) >= ?1",
    ) else {
        return (BTreeMap::new(), None);
    };
    let Ok(rows) = statement.query_map([from], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))) else {
        return (BTreeMap::new(), None);
    };
    let mut pooled = Vec::new();
    for row in rows.flatten() {
        pooled.push(row.1);
        by_category.entry(row.0).or_default().push(row.1);
    }
    let pooled = median(&pooled);
    (by_category.into_iter().filter_map(|(category, values)| Some((category, median(&values)?))).collect(), pooled)
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.1}%"))
}

fn usd(value: f64) -> String {
    format!("{:.2}", if value == 0.0 { 0.0 } else { value })
}

fn rows_text(db: &Connection, from: &str, now: Timestamp) -> Result<String, Failure> {
    let since = since_start(from, now);
    let mut statement = db.prepare(&format!(
        "SELECT agent, item, outcome, landed, landed_sha, started, ended, category, model, staffed FROM runs WHERE role = 'worker' AND {DEEPSEEK} AND coalesce(started, ended) >= ?1"
    ))?;
    let runs: Vec<Run> = statement.query_map([since.as_str()], Run::read)?.collect::<Result<_, _>>()?;
    if runs.is_empty() {
        return Err(Failure::Refused(format!(
            "no DeepSeek run in {since}: the report needs the paid Go model and the free model, each with a run in the window"
        )));
    }
    let agents: Vec<String> = runs.iter().map(|run| run.agent.clone()).collect();
    let followed = escalations(db, &agents)?;
    let (categories, pooled) = sonnet_rate(db, &since);
    let readings = readings(db, &since);
    let mut weeklies: BTreeMap<(String, String), Vec<&Run>> = BTreeMap::new();
    for run in &runs {
        let week = run.begin().as_deref().and_then(week_of).unwrap_or_else(|| "unknown".to_owned());
        weeklies.entry((week, model_class(run.model.as_deref()))).or_default().push(run);
    }
    let mut out = vec![
        ["week", "model", "attempted", "landed", "checked", "escalated", "claude_usd", "sonnet_usd", "go_5h", "go_week", "go_month", "wait_h"]
            .iter()
            .map(|cell| (*cell).to_owned())
            .collect::<Vec<String>>(),
    ];
    let mut unchecked = false;
    for ((week, model), runs) in &weeklies {
        let attempted = runs
            .iter()
            .map(|run| run.item.clone().filter(|item| !item.is_empty()).unwrap_or_else(|| run.agent.clone()))
            .collect::<BTreeSet<String>>()
            .len();
        let landed: Vec<&&Run> = runs.iter().filter(|run| run.landed_done()).collect();
        let checked = landed.iter().filter(|run| checked(db, run)).count();
        let escalated = runs.iter().filter(|run| followed.contains_key(&run.agent)).count();
        let glad: f64 = runs.iter().filter_map(|run| followed.get(&run.agent)).sum();
        unchecked |= checked < landed.len();
        let sonnet = if landed.is_empty() {
            "-".to_owned()
        } else {
            let rate: f64 = landed
                .iter()
                .map(|run| categories.get(&run.category).copied().or(pooled).unwrap_or(0.0))
                .sum();
            if rate > 0.0 { usd(rate) } else { "-".to_owned() }
        };
        let (go_five, go_week, go_month, wait) = match (model.as_str(), week.as_str()) {
            ("free-flash", _) | (_, "unknown") => ("-".to_owned(), "-".to_owned(), "-".to_owned(), "-".to_owned()),
            (_, week) => {
                let start = moment(&format!("{week}T00:00:00Z")).map(|start| start.unix_seconds()).unwrap_or(0);
                let end = week_close(week).unwrap_or(start);
                let peak = |window: &str| readings_in(&readings, start, end).filter(|reading| reading.window == window).map(|reading| reading.used).reduce(f64::max);
                let waits: Vec<f64> = runs
                    .iter()
                    .filter_map(|run| {
                        let (staffed, started) = (moment(run.staffed.as_deref()?)?, moment(&run.begin()?)?);
                        let full = readings_in(&readings, staffed.unix_seconds(), started.unix_seconds()).any(|reading| reading.used >= GO_FULL_PCT);
                        let hours = (started.unix_seconds() - staffed.unix_seconds()).max(0) as f64 / 3_600.0;
                        (full && hours > 0.0).then_some(hours)
                    })
                    .collect();
                let wait = if readings.is_empty() { "-".to_owned() } else { format!("{:.1}", waits.iter().sum::<f64>()) };
                (percent(peak("5h")), percent(peak("week")), percent(peak("month")), wait)
            }
        };
        out.push(vec![
            week.clone(),
            model.clone(),
            attempted.to_string(),
            landed.len().to_string(),
            checked.to_string(),
            escalated.to_string(),
            usd(glad),
            sonnet,
            go_five,
            go_week,
            go_month,
            wait,
        ]);
    }
    let mut text = table(&out);
    text += "sonnet_usd is the ledger's median landed Sonnet run in the same category, summed over the row's landed boxes\n";
    text += "go_5h, go_week and go_month are the week's peak Go window readings and wait_h the hours a run sat staffed while a reading was at or above 99.5%; unmeasured until a Go window reader records usage rows\n";
    if unchecked {
        text += "checked counts the landed boxes with a passing done check on their landed sha; a landing the ledger holds no check for counts as unchecked\n";
    }
    Ok(text)
}

pub(super) fn run(argv: &[String], context: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = Args::parse(argv, &["--since"], &[]).and_then(|args| {
        let db = open(context.database, false)?;
        rows_text(&db, args.get("--since").unwrap_or(""), context.now)
    });
    report("opencode", result, out, err)
}

#[cfg(test)]
#[path = "opencode_tests.rs"]
mod tests;
