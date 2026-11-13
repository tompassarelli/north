//! What the ready bars are read from: the landings, verification times, judged landings, worker attempts, Tom's asks and
//! corrections, the priority:now history, the usage and the runtime's own records, each over one window of time.
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use rusqlite::Connection;

use super::bar::{Bar, Reading};
use super::idle::Idle;
use super::bar::{DAYS, DAY_S, REPOS};
use crate::cli::scoreboard::{Sources, Window, landing, open, state_root, tom, workers};
use crate::flow::priority_log::{self, Noted};
use crate::flow::usage_yield::{self, Facts};
use crate::foundation::clock::Timestamp;
use crate::land::trace::Spans;
use crate::land::trace::profile::{Trace, percentile};
use crate::land::trace::report::{every_queue, queue_repo};
use crate::threads::attribution::Cause;
use crate::threads::calibration::rework::Sources as ReworkSources;

use super::main_green;
use super::propagation;

const PRIORITY: &str = "priority:now";
const NOTE_EVERY_S: i64 = 1_800;

pub(super) struct Records {
    landings: HashMap<String, Vec<(i64, f64)>>,
    verification: HashMap<String, Vec<(i64, f64)>>,
    reworked: Vec<landing::Judged>,
    attempts: Vec<workers::Attempt>,
    tom: tom::Tom,
    firsts: HashMap<String, Vec<i64>>,
    priority: Vec<Noted>,
    idle: Idle,
    pub(super) pins: Vec<north_runtime::runtime::pin::Event>,
    pub(super) propagation: Vec<propagation::Row>,
    pub(super) lags: Vec<north_runtime::runtime::lag::samples::Sample>,
    lander_red: HashMap<String, main_green::Reds>,
    ci_red: HashMap<String, main_green::Reds>,
    usage: Facts,
}

/// Records the open priority:now counts for the history the ready table reads.
pub(super) fn record(now: i64) {
    note(now);
}

fn priority_count(repo: &str) -> Option<usize> {
    let copy = north_github_mirror::store::Mirror::local().read(&north_github_mirror::store::key(repo, north_github_mirror::store::OPEN_ISSUES))?;
    Some(copy.body.as_array()?.iter().filter(|issue| issue["labels"]["nodes"].as_array().is_some_and(|labels| labels.iter().any(|label| label["name"] == PRIORITY))).count())
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

/// The seconds of the landings in the window, as the reading of a percentile.
fn spread(held: &HashMap<String, Vec<(i64, f64)>>, repo: &str, window: Window, quantile: f64) -> Option<Reading> {
    let seconds: Vec<f64> = held.get(repo)?.iter().filter(|(at, _)| window.holds(*at)).map(|(_, seconds)| *seconds).collect();
    (!seconds.is_empty()).then(|| Reading { value: percentile(&seconds, quantile), sample: seconds.len() })
}

/// The share of `hits` out of `of`, scaled to a percentage.
fn share(hits: usize, of: usize, scale: f64) -> Option<Reading> {
    (of > 0).then_some(Reading { value: hits as f64 / of as f64 * scale, sample: of })
}

impl Records {
    pub(super) fn read(sources: &Sources, now: i64) -> Result<Self, String> {
        Self::since(sources, now, now - DAYS as i64 * DAY_S)
    }

    pub(super) fn since(sources: &Sources, now: i64, since: i64) -> Result<Self, String> {
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
            lags: north_runtime::runtime::lag::samples::read(&sources.records.join("north")),
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

    pub(super) fn reading(&self, bar: Bar, repo: &str, window: Window) -> Option<Reading> {
        match bar {
            Bar::LandingP90 | Bar::VerificationP50 | Bar::VerificationP90 => self.timed(bar, repo, window),
            Bar::Rework | Bar::FirstTry | Bar::HarnessLoss => self.attempted(bar, repo, window),
            Bar::AskToLanding | Bar::Corrections | Bar::PriorityNow => self.told(bar, repo, window),
            Bar::RuntimeBreaks | Bar::RuntimeRollback | Bar::RuntimeLive | Bar::RuntimeCanary | Bar::RuntimeShip | Bar::RuntimeAge => self.runtime(bar, window),
            Bar::FirnClosureLive | Bar::RuntimeLag => self.propagation_held(bar, window),
            Bar::MainGreenLander | Bar::MainGreenCi => self.main_green(bar, repo, window),
            Bar::UsagePerLanding | Bar::StrandedUsage | Bar::IdleAccount => self.usage(bar, window),
        }
    }

    fn timed(&self, bar: Bar, repo: &str, window: Window) -> Option<Reading> {
        let (held, quantile) = match bar {
            Bar::LandingP90 => (&self.landings, 0.9),
            Bar::VerificationP50 => (&self.verification, 0.5),
            _ => (&self.verification, 0.9),
        };
        spread(held, repo, window, quantile)
    }

    fn attempted(&self, bar: Bar, repo: &str, window: Window) -> Option<Reading> {
        match bar {
            Bar::Rework => {
                let held: Vec<&landing::Judged> = self.reworked.iter().filter(|landing| window.holds(landing.at) && landing.repos.contains(repo)).collect();
                share(held.iter().filter(|landing| landing.reworked).count(), held.len(), 100.0)
            }
            Bar::FirstTry => {
                let held: Vec<&workers::Attempt> = self.attempts.iter().filter(|attempt| window.holds(attempt.ended) && attempt.repos.contains(repo) && !attempt.deferred).collect();
                share(held.iter().filter(|attempt| attempt.succeeded).count(), held.len(), 100.0)
            }
            _ => {
                let held: Vec<&workers::Attempt> = self.attempts.iter().filter(|attempt| window.holds(attempt.ended) && attempt.repos.contains(repo)).collect();
                share(held.iter().filter(|attempt| matches!(attempt.cause, Cause::Harness(_))).count(), held.len(), 100.0)
            }
        }
    }

    fn told(&self, bar: Bar, repo: &str, window: Window) -> Option<Reading> {
        match bar {
            Bar::AskToLanding => {
                let seconds: Vec<f64> = self.tom.asked(window, &self.firsts).iter().filter(|ask| ask.items.iter().any(|item| item.split_once('#').is_some_and(|(name, _)| name == repo))).filter_map(|ask| ask.seconds).collect();
                (!seconds.is_empty()).then(|| Reading { value: percentile(&seconds, 0.5), sample: seconds.len() })
            }
            Bar::Corrections => {
                let (count, hours) = self.tom.correcting(window);
                (hours > 0).then_some(Reading { value: count as f64 / hours as f64, sample: hours })
            }
            _ => self.priority(repo, window),
        }
    }

    fn main_green(&self, bar: Bar, repo: &str, window: Window) -> Option<Reading> {
        match bar {
            Bar::MainGreenLander => Some(main_green::reading(self.lander_red.get(repo).map_or(&[], Vec::as_slice), window)),
            _ => Some(main_green::reading(self.ci_red.get(repo)?, window)),
        }
    }

    fn usage(&self, bar: Bar, window: Window) -> Option<Reading> {
        match bar {
            Bar::UsagePerLanding => {
                let usage = self.usage.reading(window.start, window.end, false);
                usage.per_landing().map(|value| Reading { value, sample: usage.landed })
            }
            Bar::StrandedUsage => Some(Reading { value: self.usage.reading(window.start, window.end, self.work_waited(window).unwrap_or(false)).stranded(), sample: 1 }),
            _ => match self.work_waited(window)? {
                true => Some(Reading { value: self.idle.longest_minutes(window), sample: 1 }),
                false => Some(Reading { value: 0.0, sample: 1 }),
            },
        }
    }
}
