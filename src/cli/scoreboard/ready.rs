//! `north dash --bars`: one table of the bars that say the harness is ready for production
//! work, read per repository from the same records as the weekly scoreboard. The thresholds are the
//! `ready` class of north's goals.toml; days in a row counts the newest consecutive 24 h windows in
//! which every bar passed (docs/rationale/scoreboard.md).
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use north_goals::registry::{self, Bound, Class, Goal};
use rusqlite::Connection;
use serde::Deserialize;

use super::{Sources, Window, landing, open, state_root, tom, workers};
use crate::foundation::clock::Timestamp;
use crate::land::trace::profile::{Trace, percentile};
use crate::land::trace::report::{every_queue, queue_repo};
use crate::land::trace::Spans;
use crate::threads::attribution::Cause;
use crate::flow::priority_log::{self, Noted};
use crate::flow::usage_yield::{self, Facts};
use crate::threads::calibration::rework::Sources as ReworkSources;

mod main_green;
mod plain;
pub mod propagation;
mod table;

pub const REPOS: [&str; 2] = ["north", "makgora"];
const DAYS: usize = 7;
const DAY_S: i64 = 86_400;
const NEEDED_DAYS: usize = 3;
const PRIORITY: &str = "priority:now";
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
    HarnessLoss,
    AskToLanding,
    Corrections,
    PriorityNow,
    IdleAccount,
    RuntimeBreaks,
    RuntimeRollback,
    RuntimeLive,
    FirnClosureLive,
    UsagePerLanding,
    StrandedUsage,
    MainGreenLander,
    MainGreenCi,
}

impl Bar {
    const ALL: [Self; 18] = [
        Self::LandingP90,
        Self::VerificationP50,
        Self::VerificationP90,
        Self::Rework,
        Self::FirstTry,
        Self::HarnessLoss,
        Self::AskToLanding,
        Self::Corrections,
        Self::PriorityNow,
        Self::IdleAccount,
        Self::RuntimeBreaks,
        Self::RuntimeRollback,
        Self::RuntimeLive,
        Self::FirnClosureLive,
        Self::UsagePerLanding,
        Self::StrandedUsage,
        Self::MainGreenLander,
        Self::MainGreenCi,
    ];

    fn goal(self) -> &'static str {
        match self {
            Self::LandingP90 => "ready-landing-p90",
            Self::VerificationP50 => "ready-verification-p50",
            Self::VerificationP90 => "ready-verification-p90",
            Self::Rework => "ready-rework",
            Self::FirstTry => "ready-first-try",
            Self::HarnessLoss => "ready-harness-loss",
            Self::AskToLanding => "ready-ask-to-landing",
            Self::Corrections => "ready-corrections",
            Self::PriorityNow => "ready-priority-now",
            Self::IdleAccount => "ready-idle-account",
            Self::RuntimeBreaks => "ready-runtime-breaks",
            Self::RuntimeRollback => "ready-runtime-rollback",
            Self::RuntimeLive => "ready-runtime-live-p90",
            Self::FirnClosureLive => "ready-firn-closure-live-p90",
            Self::UsagePerLanding => "ready-usage-per-landing",
            Self::StrandedUsage => "ready-stranded-usage",
            Self::MainGreenLander => "ready-main-green-lander",
            Self::MainGreenCi => "ready-main-green-ci",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::LandingP90 => "landing queue p90",
            Self::VerificationP50 => "verification p50",
            Self::VerificationP90 => "verification p90",
            Self::Rework => "rework in 48 h",
            Self::FirstTry => "first-try success",
            Self::HarnessLoss => "first tries lost to the harness",
            Self::AskToLanding => "ask to landing p50",
            Self::Corrections => "corrections per h (all)",
            Self::PriorityNow => "priority:now open",
            Self::IdleAccount => "longest idle account (all)",
            Self::RuntimeBreaks => "runtime breaks reaching a 2nd host (all)",
            Self::RuntimeRollback => "runtime rollback after swap, slowest (all)",
            Self::RuntimeLive => "landing to live p90 (all)",
            Self::FirnClosureLive => "firn closure landing to live p90 (all)",
            Self::UsagePerLanding => "usage per first try landed (all)",
            Self::StrandedUsage => "usage stranded while work waited (all)",
            Self::MainGreenLander => "main green by the lander's verdicts",
            Self::MainGreenCi => "main green by GitHub Cargo",
        }
    }

    /// Whether a window with nothing to observe still counts as passed: Tom's own activity can be absent, and a window with no rollback or no landing that changed the runtime has none to time.
    fn quiet_passes(self) -> bool {
        matches!(self, Self::AskToLanding | Self::Corrections | Self::RuntimeRollback | Self::RuntimeLive | Self::FirnClosureLive)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub value: f64,
    pub sample: usize,
}

impl From<(f64, usize)> for Reading {
    fn from((value, sample): (f64, usize)) -> Self {
        Self { value, sample }
    }
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
    admits: bool,
    #[serde(default)]
    workers: u32,
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
                samples.entry(account).or_default().push((row.at, row.admits && row.workers == 0));
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

fn priority_count(repo: &str) -> Option<usize> {
    let copy = north_github_mirror::store::Mirror::local().read(&north_github_mirror::store::key(repo, north_github_mirror::store::OPEN_ISSUES))?;
    Some(copy.body.as_array()?.iter().filter(|issue| issue["labels"]["nodes"].as_array().is_some_and(|labels| labels.iter().any(|label| label["name"] == PRIORITY))).count())
}

/// Records the open priority:now counts for the history the ready table reads.
pub fn record(now: i64) {
    note(now);
}

/// Appends the open priority:now counts to the daily history, at most every half hour, since GitHub keeps no past count.
fn note(now: i64) -> Vec<Noted> {
    let mut history = priority_log::read(&state_root());
    if history.iter().any(|entry| now - entry.at < NOTE_EVERY_S) {
        return history;
    }
    let fresh: Vec<Noted> = REPOS.iter().filter_map(|repo| Some(Noted { at: now, repo: (*repo).into(), count: priority_count(repo)? })).collect();
    let path = state_root().join(priority_log::PATH);
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
    pins: Vec<north_runtime::runtime::pin::Event>,
    propagation: Vec<propagation::Row>,
    lander_red: HashMap<String, main_green::Reds>,
    ci_red: HashMap<String, main_green::Reds>,
    usage: Facts,
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

fn verified(records: &Path, since: i64) -> HashMap<String, Vec<(i64, f64)>> {
    let mut found: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
    for file in every_queue(records) {
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
        Self::since(sources, now, now - DAYS as i64 * DAY_S)
    }

    fn since(sources: &Sources, now: i64, since: i64) -> Result<Self, String> {
        let threads = open(&sources.threads)?;
        let failed = |why: rusqlite::Error| format!("{}: {why}", sources.threads.display());
        let rework: &ReworkSources = &sources.places.rework;
        Ok(Self {
            landings: landed(&threads, since)?,
            verification: verified(&sources.records, since),
            reworked: landing::judged_landings(&threads, rework)?,
            attempts: workers::attempts(&threads, since, &workers::missed_checks(&north_github_mirror::store::Mirror::local()), &workers::Pending::read(&threads, now)).map_err(failed)?,
            firsts: landing::first_landings(&threads).map_err(failed)?,
            tom: tom::Tom::read(&sources.places, &sources.home, since, now)?,
            priority: note(now),
            lander_red: main_green::lander(&threads, now)?,
            ci_red: REPOS.iter().filter_map(|repo| Some(((*repo).to_owned(), main_green::github(repo, now)?))).collect(),
            idle: Idle::read(since),
            usage: Facts::load(&usage_yield::Sources { db: &threads, policy: &crate::platform::orchestration::path(), state: &state_root() }, now),
            pins: north_runtime::runtime::pin::read(&sources.records.join("north")),
            propagation: propagation::read(&sources.records),
        })
    }

    /// The pinned runtime's bars, one fleet for every repository.
    fn runtime(&self, bar: Bar, window: Window) -> Option<Reading> {
        let pins = north_runtime::runtime::pin::bars(&self.pins, window.start, window.end);
        let reading = match bar {
            Bar::RuntimeBreaks => pins.breaks(),
            Bar::RuntimeRollback => pins.rollback(),
            _ => (!pins.landing_to_live_seconds.is_empty()).then(|| (percentile(&pins.landing_to_live_seconds, 0.9), pins.landing_to_live_seconds.len())),
        };
        reading.map(Reading::from)
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
            Bar::RuntimeBreaks | Bar::RuntimeRollback | Bar::RuntimeLive => self.runtime(bar, window),
            Bar::FirnClosureLive => {
                let seconds = propagation::totals(&self.propagation, propagation::FIRN_CLOSURE, window.start, window.end);
                (!seconds.is_empty()).then(|| Reading { value: percentile(&seconds, 0.9), sample: seconds.len() })
            }
            Bar::Rework => {
                let held: Vec<&landing::Judged> = self.reworked.iter().filter(|landing| window.holds(landing.at) && landing.repos.contains(repo)).collect();
                share(held.iter().filter(|landing| landing.reworked).count(), held.len(), 100.0)
            }
            Bar::FirstTry => {
                let held: Vec<&workers::Attempt> = self.attempts.iter().filter(|attempt| window.holds(attempt.ended) && attempt.repos.contains(repo) && !attempt.deferred).collect();
                share(held.iter().filter(|attempt| attempt.succeeded).count(), held.len(), 100.0)
            }
            Bar::HarnessLoss => {
                let held: Vec<&workers::Attempt> = self.attempts.iter().filter(|attempt| window.holds(attempt.ended) && attempt.repos.contains(repo)).collect();
                share(held.iter().filter(|attempt| matches!(attempt.cause, Cause::Harness(_))).count(), held.len(), 100.0)
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
            Bar::MainGreenLander => Some(main_green::reading(self.lander_red.get(repo).map_or(&[], Vec::as_slice), window)),
            Bar::MainGreenCi => Some(main_green::reading(self.ci_red.get(repo)?, window)),
            Bar::UsagePerLanding => {
                let usage = self.usage.reading(window.start, window.end, false);
                usage.per_landing().map(|value| Reading { value, sample: usage.landed })
            }
            Bar::StrandedUsage => Some(Reading { value: self.usage.reading(window.start, window.end, self.work_waited(window).unwrap_or(false)).stranded(), sample: 1 }),
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
        let tree = north_runtime::runtime::own_runtime().map(|(_, runtime)| runtime.join(north_runtime::runtime::layout::TREE)).filter(|tree| tree.join(registry::PATH).is_file());
        let path = tree.unwrap_or_else(|| north_goals::code_root().join("north/main")).join(registry::PATH);
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

/// One bar's readings for one repository: the last 1 h carries the verdict, 10 min, 3 h, 6 h and 24 h are context.
struct Cells {
    ten_minutes: Option<Reading>,
    hour: (Option<Reading>, Standing),
    three_hours: Option<Reading>,
    six_hours: Option<Reading>,
    day: Option<Reading>,
}

struct Repo {
    name: &'static str,
    bars: Vec<Cells>,
    days_in_a_row: usize,
}

/// The `minutes` ending at `now`, bounded as days in a row bounds its newest day.
fn last(now: i64, minutes: i64) -> Window {
    Window { start: now - minutes * 60 + 1, end: now + 1 }
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
    let bars = Bar::ALL.iter().map(|&bar| Cells { ten_minutes: reading(bar, last(now, 10)), hour: standing(bar, last(now, 60)), three_hours: reading(bar, last(now, 180)), six_hours: reading(bar, last(now, 360)), day: reading(bar, last(now, 1_440)) }).collect();
    Repo { name: repo, bars, days_in_a_row }
}

/// How the ready tables print: `explain` adds each cell's sample and the sources, `color` marks the verdict on a terminal.
#[derive(Clone, Copy, Default)]
pub struct View {
    pub all: bool,
    pub explain: bool,
    pub color: bool,
}

/// The repositories whose projects are not paused in orchestration.toml, or every one with `--all`.
fn shown(all: bool) -> Vec<&'static str> {
    let paused = if all { Vec::new() } else { crate::platform::orchestration::read().map(|cfg| crate::platform::orchestration::set_aside_repos(&cfg)).unwrap_or_default() };
    REPOS.into_iter().filter(|repo| !paused.iter().any(|name| name == repo)).collect()
}

pub fn run(now: i64, view: View) -> Result<String, String> {
    let goals = Goals::read()?;
    let records = Records::read(&Sources::live(), now)?;
    let repos: Vec<Repo> = shown(view.all).into_iter().map(|repo| assess(|bar, window| records.reading(bar, repo, window), &goals, repo, now)).collect();
    Ok(table::render(&repos, &goals, now, view))
}

const TOLD: &str = "north/scoreboard/ready-told";

/// What the proxy hears first each day: the ready verdict of every repository as one line, so Tom reads whether the harness is ready without asking.
pub fn report(now: i64) -> Vec<String> {
    let path = state_root().join(TOLD);
    let last = fs::read_to_string(&path).ok().and_then(|text| text.trim().parse::<i64>().ok()).unwrap_or(0);
    if (0..DAY_S).contains(&(now - last)) {
        return Vec::new();
    }
    let line = match (Goals::read(), Records::read(&Sources::live(), now)) {
        (Ok(goals), Ok(records)) => {
            let heads: Vec<String> = shown(false).into_iter().map(|repo| table::headline(&assess(|bar, window| records.reading(bar, repo, window), &goals, repo, now))).collect();
            format!("ready bar: {} (north dash --bars prints the table)", heads.join("; "))
        }
        (Err(why), _) | (_, Err(why)) => format!("ready bar not read: {why}"),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, now.to_string());
    vec![line]
}

/// One readiness bar of one repository as `north dash` shows it: its target, the verdict on the last 1 h, and its
/// reading over the last 1 h and over the dash window.
/// Where a target stands: missed over the last 1 h, met now but missed over the window, or met on both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Missed,
    Slipping,
    Met,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Line {
    pub repo: &'static str,
    pub bar: &'static str,
    #[serde(skip)]
    pub plain: &'static str,
    pub target: String,
    pub verdict: Verdict,
    pub hour: Option<f64>,
    pub hour_n: Option<usize>,
    pub window: Option<f64>,
    pub window_n: Option<usize>,
    #[serde(skip)]
    pub five_text: String,
    #[serde(skip)]
    pub fifteen_text: String,
    #[serde(skip)]
    pub hour_text: String,
    #[serde(skip)]
    pub window_text: String,
}

/// Every bar of every repository not paused, with readings no older than `span` seconds before `now`.
pub fn lines(now: i64, span: i64) -> Result<Vec<Line>, String> {
    let goals = Goals::read()?;
    let records = Records::since(&Sources::live(), now, now - span.max(3_600))?;
    let mut found = Vec::new();
    for repo in shown(false) {
        for bar in Bar::ALL {
            let goal = goals.of(bar);
            let (hour, window) = (records.reading(bar, repo, last(now, 60)), records.reading(bar, repo, last(now, span / 60)));
            let (five, fifteen) = (records.reading(bar, repo, last(now, 5)), records.reading(bar, repo, last(now, 15)));
            found.push(Line {
                repo,
                bar: bar.label(),
                plain: bar.plain(),
                target: table::threshold(goal),
                verdict: match (counts(Standing::of(goal.bound, hour), bar), counts(Standing::of(goal.bound, window), bar)) {
                    (false, _) => Verdict::Missed,
                    (true, false) => Verdict::Slipping,
                    (true, true) => Verdict::Met,
                },
                hour: hour.map(|reading| reading.value),
                hour_n: hour.map(|reading| reading.sample),
                window: window.map(|reading| reading.value),
                window_n: window.map(|reading| reading.sample),
                five_text: table::cell(goal, five, true),
                fifteen_text: table::cell(goal, fifteen, true),
                hour_text: table::cell(goal, hour, true),
                window_text: table::cell(goal, window, true),
            });
        }
    }
    Ok(found)
}
