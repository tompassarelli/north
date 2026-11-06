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
use serde::Deserialize;

use super::priority::{Noted, note};
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
const PACE_HISTORY: &str = "agents/pace-control.jsonl";
const PACE_GAP_S: f64 = 600.0;
const MORNING_TOLD: &str = "north/scoreboard/ready-told.txt";
const MORNING_EVERY_S: i64 = 20 * 3_600;

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
            Self::LandingP90 => "landing queue p90",
            Self::VerificationP50 => "verification p50",
            Self::VerificationP90 => "verification p90",
            Self::Rework => "rework in 48 h",
            Self::FirstTry => "first-try success",
            Self::AskToLanding => "ask to landing p50",
            Self::Corrections => "corrections per h (all)",
            Self::PriorityNow => "priority:now open",
            Self::IdleAccount => "longest idle account (all)",
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
            attempts: workers::attempts(&threads, since, &workers::missed_checks(&north_github_mirror::store::Mirror::local())).map_err(failed)?,
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
        Self::parse(&text).map_err(|why| format!("{}: {why}", path.display()))
    }

    fn parse(text: &str) -> Result<Self, String> {
        let mut goals = registry::parse(text).map_err(|found| found.join("; "))?;
        let mut held = HashMap::new();
        for bar in Bar::ALL {
            let at = goals.iter().position(|goal| goal.name == bar.goal() && goal.class == Class::Ready).ok_or_else(|| format!("no ready goal {}", bar.goal()))?;
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

/// One bar's readings for one repository: the last 24 h carries the verdict, 12 h and 7 d are context.
struct Cells {
    half_day: Option<Reading>,
    day: (Option<Reading>, Standing),
    week: Option<Reading>,
}

struct Repo {
    name: &'static str,
    bars: Vec<Cells>,
    days_in_a_row: usize,
}

/// The `hours` ending at `now`, bounded as days in a row bounds its newest day.
fn last(now: i64, hours: i64) -> Window {
    Window { start: now - hours * 3_600 + 1, end: now + 1 }
}

fn assess(reading: impl Fn(Bar, Window) -> Option<Reading>, goals: &Goals, repo: &'static str, now: i64) -> Repo {
    let standing = |bar: Bar, window: Window| {
        let reading = reading(bar, window);
        (reading, Standing::of(goals.of(bar).bound, reading))
    };
    let days_in_a_row = (0..DAYS as i64)
        .map(|day| Window { start: now - (day + 1) * DAY_S + 1, end: now - day * DAY_S + 1 })
        .take_while(|window| Bar::ALL.iter().all(|bar| counts(standing(*bar, *window).1, *bar)))
        .count();
    let bars = Bar::ALL.iter().map(|&bar| Cells { half_day: reading(bar, last(now, 12)), day: standing(bar, last(now, 24)), week: reading(bar, last(now, DAYS as i64 * 24)) }).collect();
    Repo { name: repo, bars, days_in_a_row }
}

/// How the ready tables print: `explain` adds each cell's sample and the sources, `color` marks the verdict on a terminal.
#[derive(Clone, Copy, Default)]
pub struct View {
    pub all: bool,
    pub explain: bool,
    pub color: bool,
}

const MARK: usize = 3;

fn cell(goal: &Goal, reading: Option<Reading>, explain: bool) -> String {
    match reading {
        Some(reading) if explain => format!("{} (n={})", show(&goal.unit, reading.value), reading.sample),
        Some(reading) => show(&goal.unit, reading.value),
        None => "none".into(),
    }
}

fn mark(text: &str, color: bool) -> String {
    match (color, text.trim_end()) {
        (true, "pass") => format!("\x1b[32m{text}\x1b[0m"),
        (true, "FAIL") => format!("\x1b[1;31m{text}\x1b[0m"),
        _ => text.to_owned(),
    }
}

/// The first line of a repository's table: its verdict, how many bars fail on the last 24 h and the days in a row.
fn headline(repo: &Repo) -> String {
    let failing = Bar::ALL.iter().zip(&repo.bars).filter(|(bar, cells)| !counts(cells.day.1, **bar)).count();
    let verdict = if repo.days_in_a_row >= NEEDED_DAYS { "ready" } else { "not ready" };
    format!("{}: {verdict}, {failing} of {} bars fail on the last 24 h, {} of {NEEDED_DAYS} days", repo.name, Bar::ALL.len(), repo.days_in_a_row)
}

fn table(out: &mut String, repo: &Repo, goals: &Goals, view: View) {
    let mut rows: Vec<Vec<String>> = vec![["bar", "target", "24 h", "", "12 h", "7 d"].map(String::from).to_vec()];
    for (bar, cells) in Bar::ALL.iter().zip(&repo.bars) {
        let goal = goals.of(*bar);
        let passed = counts(cells.day.1, *bar);
        rows.push(vec![bar.label().into(), threshold(goal), cell(goal, cells.day.0, view.explain), if passed { "pass" } else { "FAIL" }.into(), cell(goal, cells.half_day, view.explain), cell(goal, cells.week, view.explain)]);
    }
    let _ = writeln!(out, "{}", headline(repo));
    let widths: Vec<usize> = (0..rows[0].len()).map(|column| rows.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    for row in &rows {
        let line: Vec<String> = row.iter().zip(&widths).enumerate().map(|(column, (text, width))| {
            let padded = format!("{text:<width$}");
            if column == MARK { mark(&padded, view.color) } else { padded }
        }).collect();
        let _ = writeln!(out, "{}", line.join("  ").trim_end());
    }
}

fn render(repos: &[Repo], goals: &Goals, now: i64, view: View) -> String {
    let mut out = String::new();
    for (index, repo) in repos.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        table(&mut out, repo, goals, view);
    }
    if view.explain {
        let _ = writeln!(out, "\nat {}: each bar passes or fails on its last 24 h; 12 h and 7 d are context with no verdict", Timestamp::from_unix_seconds(now).local_twelve_hour());
        let _ = writeln!(out, "days: the newest 24 h windows in a row in which every bar passed; n: each cell's sample");
        let _ = writeln!(out, "targets: goals.toml, ready class");
        let _ = writeln!(out, "sources: landings ledger, land-queue spans, worker ledger, Tom's transcripts, GitHub mirror, pace-control history");
    }
    out
}

/// The repositories whose projects are not paused in orchestration.toml, or every one with `--all`.
fn shown(all: bool) -> Vec<&'static str> {
    let paused = if all { Vec::new() } else { crate::platform::orchestration::read().map(|cfg| crate::platform::orchestration::set_aside_repos(&cfg)).unwrap_or_default() };
    REPOS.into_iter().filter(|repo| !paused.iter().any(|name| name == repo)).collect()
}

/// The morning line: every active repository's verdict, told by the fleet tick when the last telling is 20 hours old.
pub fn morning(now: i64) -> Vec<String> {
    let told = state_root().join(MORNING_TOLD);
    let last = fs::read_to_string(&told).ok().and_then(|text| text.trim().parse::<i64>().ok());
    if last.is_some_and(|at| (0..MORNING_EVERY_S).contains(&(now - at))) {
        return Vec::new();
    }
    let read = Goals::read().and_then(|goals| Ok((Records::read(&Sources::live(), now)?, goals)));
    let (records, goals) = match read {
        Ok(read) => read,
        Err(why) => return vec![format!("ready bar not read: {why}")],
    };
    let lines: Vec<String> = shown(false).into_iter().map(|repo| headline(&assess(|bar, window| records.reading(bar, repo, window), &goals, repo, now))).collect();
    if let Some(parent) = told.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&told, now.to_string());
    if lines.is_empty() { Vec::new() } else { vec![format!("ready bar (`north scoreboard --ready` prints the table): {}", lines.join("; "))] }
}

pub fn run(now: i64, view: View) -> Result<String, String> {
    let goals = Goals::read()?;
    let records = Records::read(&Sources::live(), now)?;
    let repos: Vec<Repo> = shown(view.all).into_iter().map(|repo| assess(|bar, window| records.reading(bar, repo, window), &goals, repo, now)).collect();
    Ok(render(&repos, &goals, now, view))
}
