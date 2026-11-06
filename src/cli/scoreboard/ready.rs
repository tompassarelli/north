//! `north ledger scoreboard --ready`: one table of the bars that say the harness is ready for production
//! work, read per repository from the same records as the weekly scoreboard. The thresholds are the
//! `ready` class of north's goals.toml; days in a row counts the newest consecutive 24 h windows in
//! which every bar passed (docs/rationale/scoreboard.md).
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

use north_goals::registry::{self, Bound, Class, Goal};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{Sources, Window, landing, open, state_root, tom, workers};
use crate::foundation::clock::Timestamp;
use crate::land::trace::profile::{Trace, percentile};
use crate::land::trace::report::{every_queue, queue_repo};
use crate::land::trace::Spans;
use crate::threads::attribution::Cause;
use crate::threads::calibration::rework::Sources as ReworkSources;

mod table;

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
    HarnessLoss,
    AskToLanding,
    Corrections,
    PriorityNow,
    IdleAccount,
}

impl Bar {
    const ALL: [Self; 10] = [
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

/// One bar's readings for one repository: the last 24 h carries the verdict, 1 h, 12 h and 7 d are context.
struct Cells {
    hour: Option<Reading>,
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
    let bars = Bar::ALL.iter().map(|&bar| Cells { hour: reading(bar, last(now, 1)), half_day: reading(bar, last(now, 12)), day: standing(bar, last(now, 24)), week: reading(bar, last(now, DAYS as i64 * 24)) }).collect();
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
            format!("ready bar: {} (north ledger scoreboard --ready prints the table)", heads.join("; "))
        }
        (Err(why), _) | (_, Err(why)) => format!("ready bar not read: {why}"),
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, now.to_string());
    vec![line]
}
