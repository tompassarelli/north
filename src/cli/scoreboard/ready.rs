//! `north scoreboard --ready`: one table of the bars that say the harness is ready for production
//! work, read per repository from the same records as the weekly scoreboard. The thresholds are the
//! `ready` class of north's goals.toml; days in a row counts the newest consecutive 24 h windows in
//! which every bar passed (docs/rationale/scoreboard.md).
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

use north_goals::registry::{self, Bound, Class, Goal};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{Sources, Window, duration, landing, open, state_root, tom, workers};
use crate::foundation::clock::Timestamp;
use crate::land::trace::profile::{Trace, percentile};
use crate::land::trace::report::{every_queue, queue_repo};
use crate::land::trace::Spans;
use crate::threads::calibration::rework::Sources as ReworkSources;

pub const REPOS: [&str; 2] = ["north", "makgora"];
const DAYS: usize = 7;
const DAY_S: i64 = 86_400;
const NEEDED_DAYS: usize = 3;
const PRIORITY: &str = "priority:now";
const PRIORITY_LOG: &str = "north/scoreboard/ready-priority.jsonl";
const PACE_HISTORY: &str = "agents/pace-control.jsonl";
const PACE_GAP_S: f64 = 600.0;
const NOTE_EVERY_S: i64 = 1_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bar {
    LandingP90,
    VerificationP50,
    VerificationP90,
    Rework,
    FirstTry,
    AskToLanding,
    Corrections,
    PriorityNow,
    IdleAccount,
}

impl Bar {
    const ALL: [Self; 9] = [
        Self::LandingP90,
        Self::VerificationP50,
        Self::VerificationP90,
        Self::Rework,
        Self::FirstTry,
        Self::AskToLanding,
        Self::Corrections,
        Self::PriorityNow,
        Self::IdleAccount,
    ];

    fn goal(self) -> &'static str {
        match self {
            Self::LandingP90 => "ready-landing-p90",
            Self::VerificationP50 => "ready-verification-p50",
            Self::VerificationP90 => "ready-verification-p90",
            Self::Rework => "ready-rework",
            Self::FirstTry => "ready-first-try",
            Self::AskToLanding => "ready-ask-to-landing",
            Self::Corrections => "ready-corrections",
            Self::PriorityNow => "ready-priority-now",
            Self::IdleAccount => "ready-idle-account",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::LandingP90 => "landing queue to main, p90",
            Self::VerificationP50 => "verification after the push, p50",
            Self::VerificationP90 => "verification after the push, p90",
            Self::Rework => "rework within 48 h",
            Self::FirstTry => "worker first-try success",
            Self::AskToLanding => "Tom's ask to first landed commit, median",
            Self::Corrections => "Tom's corrections per active hour (all repos)",
            Self::PriorityNow => "priority:now issues open",
            Self::IdleAccount => "longest account idle while work waits (all repos)",
        }
    }

    /// Whether a window with nothing to observe still counts as passed: Tom's own activity can be absent.
    fn quiet_passes(self) -> bool {
        matches!(self, Self::AskToLanding | Self::Corrections)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub value: f64,
    pub sample: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Pass,
    Fail,
    NoRecord,
}

impl Standing {
    fn of(bound: Bound, reading: Option<Reading>) -> Self {
        match reading {
            Some(reading) if bound.holds(reading.value) => Self::Pass,
            Some(_) => Self::Fail,
            None => Self::NoRecord,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "FAIL",
            Self::NoRecord => "no record",
        }
    }
}

fn counts(standing: Standing, bar: Bar) -> bool {
    standing == Standing::Pass || (standing == Standing::NoRecord && bar.quiet_passes())
}

#[derive(Deserialize)]
struct PaceRow {
    account: Option<String>,
    at: f64,
    #[serde(default)]
    allowed_workers: u32,
    #[serde(default)]
    workers: u32,
    #[serde(default)]
    held: bool,
    #[serde(default)]
    over: bool,
}

struct Idle {
    stretches: Vec<(i64, i64)>,
}

impl Idle {
    fn read(since: i64) -> Self {
        let path = state_root().join(PACE_HISTORY);
        let Ok(mut file) = fs::File::open(&path) else { return Self { stretches: Vec::new() } };
        let length = file.metadata().map_or(0, |meta| meta.len());
        let wanted = DAYS as u64 * 8 * 1_048_576;
        let start = length.saturating_sub(wanted);
        if file.seek(SeekFrom::Start(start)).is_err() {
            return Self { stretches: Vec::new() };
        }
        let mut samples: HashMap<String, Vec<(f64, bool)>> = HashMap::new();
        let mut lines = BufReader::new(file).split(b'\n');
        if start > 0 {
            lines.next();
        }
        for line in lines.map_while(Result::ok) {
            if !line.windows(10).any(|window| window == b"\"account\":") && !line.windows(11).any(|window| window == b"\"account\": ") {
                continue;
            }
            let Ok(row) = serde_json::from_slice::<PaceRow>(&line) else { continue };
            let Some(account) = row.account.filter(|account| account.starts_with("claude:")) else { continue };
            if row.at >= since as f64 {
                samples.entry(account).or_default().push((row.at, row.allowed_workers > 0 && !row.held && !row.over && row.workers == 0));
            }
        }
        let mut stretches = Vec::new();
        for series in samples.values_mut() {
            series.sort_by(|left, right| left.0.total_cmp(&right.0));
            let mut open: Option<(f64, f64)> = None;
            for (at, idle) in series.iter().copied() {
                open = match (idle, open) {
                    (true, Some((from, last))) if at - last <= PACE_GAP_S => Some((from, at)),
                    (true, Some((from, last))) => {
                        stretches.push((from as i64, last as i64));
                        Some((at, at))
                    }
                    (true, None) => Some((at, at)),
                    (false, Some((from, last))) => {
                        stretches.push((from as i64, last as i64));
                        None
                    }
                    (false, None) => None,
                };
            }
            if let Some((from, last)) = open {
                stretches.push((from as i64, last as i64));
            }
        }
        Self { stretches }
    }

    fn longest_minutes(&self, window: Window) -> f64 {
        self.stretches.iter().filter(|&&(from, to)| to >= window.start && from < window.end).map(|&(from, to)| (to.min(window.end - 1) - from.max(window.start)).max(0)).max().unwrap_or(0) as f64 / 60.0
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Noted {
    at: i64,
    repo: String,
    count: usize,
}

fn priority_count(repo: &str) -> Option<usize> {
    let copy = north_github_mirror::store::Mirror::local().read(&north_github_mirror::store::key(repo, north_github_mirror::store::OPEN_ISSUES))?;
    Some(copy.body.as_array()?.iter().filter(|issue| issue["labels"]["nodes"].as_array().is_some_and(|labels| labels.iter().any(|label| label["name"] == PRIORITY))).count())
}

fn noted() -> Vec<Noted> {
    fs::read_to_string(state_root().join(PRIORITY_LOG)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// Records the open priority:now counts for the history the ready table reads.
pub fn record(now: i64) {
    note(now);
}

/// Appends the open priority:now counts to the daily history, at most every half hour, since GitHub keeps no past count.
fn note(now: i64) -> Vec<Noted> {
    let mut history = noted();
    if history.iter().any(|entry| now - entry.at < NOTE_EVERY_S) {
        return history;
    }
    let fresh: Vec<Noted> = REPOS.iter().filter_map(|repo| Some(Noted { at: now, repo: (*repo).into(), count: priority_count(repo)? })).collect();
    let path = state_root().join(PRIORITY_LOG);
    let text: String = fresh.iter().filter_map(|entry| serde_json::to_string(entry).ok()).map(|line| line + "\n").collect();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let written = fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
    if written.is_ok() {
        history.extend(fresh);
    }
    history
}

struct Records {
    landings: HashMap<String, Vec<(i64, f64)>>,
    verification: HashMap<String, Vec<(i64, f64)>>,
    reworked: Vec<landing::Judged>,
    attempts: Vec<workers::Attempt>,
    tom: tom::Tom,
    firsts: HashMap<String, Vec<i64>>,
    priority: Vec<Noted>,
    idle: Idle,
}

fn landed(db: &Connection, since: i64) -> Result<HashMap<String, Vec<(i64, f64)>>, String> {
    let mut statement = db.prepare("SELECT repo, queued_at, landed_at FROM landings WHERE verdict = 'landed' AND coalesce(landed_at, '') != ''").map_err(|why| why.to_string())?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(|why| why.to_string())?;
    let mut found: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
    for (repo, queued, landed) in rows.filter_map(Result::ok) {
        let (Ok(queued), Ok(landed)) = (Timestamp::parse(&queued), Timestamp::parse(&landed)) else { continue };
        if landed.unix_seconds() >= since {
            found.entry(repo).or_default().push((landed.unix_seconds(), (landed.unix_seconds() - queued.unix_seconds()).max(0) as f64));
        }
    }
    Ok(found)
}

fn verified(since: i64) -> HashMap<String, Vec<(i64, f64)>> {
    let mut found: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
    for file in every_queue() {
        let Some(repo) = queue_repo(&file).and_then(|name| REPOS.iter().find(|repo| **repo == name)) else { continue };
        let spans = Spans::at(&file).read();
        let trace = Trace::of(&spans);
        let walls = trace.landings(since as f64).into_iter().map(|landing| (landing.end as i64, trace.profile(landing).after_wall)).filter(|(_, wall)| *wall > 0.0);
        found.entry((*repo).into()).or_default().extend(walls);
    }
    found
}

impl Records {
    fn read(sources: &Sources, now: i64) -> Result<Self, String> {
        let since = now - DAYS as i64 * DAY_S;
        let threads = open(&sources.threads)?;
        let failed = |why: rusqlite::Error| format!("{}: {why}", sources.threads.display());
        let rework: &ReworkSources = &sources.places.rework;
        Ok(Self {
            landings: landed(&threads, since)?,
            verification: verified(since),
            reworked: landing::judged_landings(&threads, rework)?,
            attempts: workers::attempts(&threads, since).map_err(failed)?,
            firsts: landing::first_landings(&threads).map_err(failed)?,
            tom: tom::Tom::read(&sources.places, &sources.home, since, now)?,
            priority: note(now),
            idle: Idle::read(since),
        })
    }

    fn priority(&self, repo: &str, window: Window) -> Option<Reading> {
        let latest = self.priority.iter().filter(|entry| entry.repo == repo && window.holds(entry.at)).max_by_key(|entry| entry.at)?;
        Some(Reading { value: latest.count as f64, sample: 1 })
    }

    fn work_waited(&self, window: Window) -> Option<bool> {
        let held: Vec<&Noted> = self.priority.iter().filter(|entry| window.holds(entry.at)).collect();
        (!held.is_empty()).then(|| held.iter().any(|entry| entry.count > 0))
    }

    fn reading(&self, bar: Bar, repo: &str, window: Window) -> Option<Reading> {
        let spread = |held: &HashMap<String, Vec<(i64, f64)>>, quantile: f64| {
            let seconds: Vec<f64> = held.get(repo)?.iter().filter(|(at, _)| window.holds(*at)).map(|(_, seconds)| *seconds).collect();
            (!seconds.is_empty()).then(|| Reading { value: percentile(&seconds, quantile), sample: seconds.len() })
        };
        let share = |hits: usize, of: usize, scale: f64| (of > 0).then(|| Reading { value: hits as f64 / of as f64 * scale, sample: of });
        match bar {
            Bar::LandingP90 => spread(&self.landings, 0.9),
            Bar::VerificationP50 => spread(&self.verification, 0.5),
            Bar::VerificationP90 => spread(&self.verification, 0.9),
            Bar::Rework => {
                let held: Vec<&landing::Judged> = self.reworked.iter().filter(|landing| window.holds(landing.at) && landing.repos.contains(repo)).collect();
                share(held.iter().filter(|landing| landing.reworked).count(), held.len(), 100.0)
            }
            Bar::FirstTry => {
                let held: Vec<&workers::Attempt> = self.attempts.iter().filter(|attempt| window.holds(attempt.ended) && attempt.repos.contains(repo)).collect();
                share(held.iter().filter(|attempt| attempt.succeeded).count(), held.len(), 100.0)
            }
            Bar::AskToLanding => {
                let seconds: Vec<f64> = self.tom.asked(window, &self.firsts).iter().filter(|ask| ask.items.iter().any(|item| item.split_once('#').is_some_and(|(name, _)| name == repo))).filter_map(|ask| ask.seconds).collect();
                (!seconds.is_empty()).then(|| Reading { value: percentile(&seconds, 0.5), sample: seconds.len() })
            }
            Bar::Corrections => {
                let (count, hours) = self.tom.correcting(window);
                (hours > 0).then(|| Reading { value: count as f64 / hours as f64, sample: hours })
            }
            Bar::PriorityNow => self.priority(repo, window),
            Bar::IdleAccount => {
                match self.work_waited(window)? {
                    true => Some(Reading { value: self.idle.longest_minutes(window), sample: 1 }),
                    false => Some(Reading { value: 0.0, sample: 1 }),
                }
            }
        }
    }
}

struct Goals(HashMap<&'static str, Goal>);

impl Goals {
    fn read() -> Result<Self, String> {
        let path = north_goals::code_root().join("north/main").join(registry::PATH);
        let text = fs::read_to_string(&path).map_err(|why| format!("{}: {why}", path.display()))?;
        let mut goals = registry::parse(&text).map_err(|found| found.join("; "))?;
        let mut held = HashMap::new();
        for bar in Bar::ALL {
            let at = goals.iter().position(|goal| goal.name == bar.goal() && goal.class == Class::Ready).ok_or_else(|| format!("{}: no ready goal {}", path.display(), bar.goal()))?;
            held.insert(bar.goal(), goals.swap_remove(at));
        }
        Ok(Self(held))
    }

    fn of(&self, bar: Bar) -> &Goal {
        &self.0[bar.goal()]
    }
}

fn show(unit: &str, value: f64) -> String {
    match unit {
        "s" => duration(value),
        "%" => format!("{value:.0}%"),
        "per h" => format!("{value:.2}/h"),
        "min" => format!("{value:.0} min"),
        _ => format!("{value:.0}"),
    }
}

fn threshold(goal: &Goal) -> String {
    format!("{} {}", if matches!(goal.bound, Bound::Max(_)) { "<=" } else { ">=" }, show(&goal.unit, goal.bound.limit()))
}

struct Repo {
    name: &'static str,
    week: Vec<(Option<Reading>, Standing)>,
    days_in_a_row: usize,
}

fn assess(records: &Records, goals: &Goals, repo: &'static str, now: i64) -> Repo {
    let week = Window { start: now - DAYS as i64 * DAY_S, end: now + 1 };
    let standing = |bar: Bar, window: Window| {
        let reading = records.reading(bar, repo, window);
        (reading, Standing::of(goals.of(bar).bound, reading))
    };
    let days_in_a_row = (0..DAYS as i64)
        .map(|day| Window { start: now - (day + 1) * DAY_S + 1, end: now - day * DAY_S + 1 })
        .take_while(|window| Bar::ALL.iter().all(|bar| counts(standing(*bar, *window).1, *bar)))
        .count();
    Repo { name: repo, week: Bar::ALL.iter().map(|bar| standing(*bar, week)).collect(), days_in_a_row }
}

fn cell(goal: &Goal, reading: Option<Reading>, standing: Standing) -> String {
    match reading {
        Some(reading) => format!("{} {} (n={})", show(&goal.unit, reading.value), standing.word(), reading.sample),
        None => standing.word().into(),
    }
}

fn render(repos: &[Repo], goals: &Goals, now: i64) -> String {
    let mut rows: Vec<Vec<String>> = vec![["bar".into(), "ready threshold".into()].into_iter().chain(repos.iter().map(|repo| repo.name.to_owned())).collect()];
    for (index, bar) in Bar::ALL.iter().enumerate() {
        let goal = goals.of(*bar);
        rows.push([bar.label().to_owned(), threshold(goal)].into_iter().chain(repos.iter().map(|repo| cell(goal, repo.week[index].0, repo.week[index].1))).collect());
    }
    rows.push(["days in a row all bars pass".into(), format!(">= {NEEDED_DAYS}")].into_iter().chain(repos.iter().map(|repo| format!("{} {}", repo.days_in_a_row, if repo.days_in_a_row >= NEEDED_DAYS { "pass" } else { "FAIL" }))).collect());
    let widths: Vec<usize> = (0..rows[0].len()).map(|column| rows.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    let mut out = String::new();
    let _ = writeln!(out, "north scoreboard --ready: last 7 days at {}; days in a row counts the newest 24 h windows in which every bar passed", Timestamp::from_unix_seconds(now).local_twelve_hour());
    for row in &rows {
        let line: Vec<String> = row.iter().zip(&widths).map(|(text, width)| format!("{text:<width$}")).collect();
        let _ = writeln!(out, "{}", line.join("  ").trim_end());
    }
    let ready = repos.iter().all(|repo| repo.days_in_a_row >= NEEDED_DAYS);
    let _ = writeln!(out, "ready for production work: {}", if ready { "yes" } else { "no" });
    let _ = writeln!(out, "thresholds: goals.toml, ready class. from the landings ledger and land-queue spans, the worker ledger, Tom's transcripts, the GitHub mirror and pace-control history");
    out
}

pub fn run(now: i64) -> Result<String, String> {
    let goals = Goals::read()?;
    let records = Records::read(&Sources::live(), now)?;
    let repos: Vec<Repo> = REPOS.iter().map(|repo| assess(&records, &goals, repo, now)).collect();
    Ok(render(&repos, &goals, now))
}
