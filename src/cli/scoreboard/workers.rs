use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use north_github_mirror::store::{self as mirror, Mirror};
use regex::Regex;
use rusqlite::Connection;

use super::chain::{self, End, Marks, Raw};
use super::{Better, Figure, Measure, Window};
use crate::foundation::clock::Timestamp;
use crate::ledger::outcome::attention::references;
use crate::platform::run_roles::worker_where;
use crate::platform::{land_queue, records_mirror};
use crate::capacity::host::{capacity_database_readonly, queued_behind_capacity};
use crate::land_ticket::{Lane, lanes_by_session};
use crate::threads::attribution::{Cause, Harness, attribute, fleet_column, outcome_column, recorded_column, report_column};

const SOURCE: &str = "worker-ledger runs with no Follows: (first attempts), done with no later run following them; split, idle, running, deferred, lander-queued and job-waking runs left out unless the deferred box's check missed (threads runs, GitHub mirror)";
const ALL: &str = "all categories";
/// How long a first try stopped on `Stop: host` waits for dispatch to relaunch it on the named host before its own end judges it.
const RELAUNCH_WAIT_S: i64 = 24 * 3_600;

pub struct Attempt {
    pub agent: String,
    pub category: String,
    pub started: i64,
    pub ended: i64,
    pub succeeded: bool,
    pub repos: BTreeSet<String>,
    pub cause: Cause,
    /// A failed box deferred to a dated check that has not missed: left out of first-try success, counted as a waits loss.
    pub deferred: bool,
}

static MISSED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<!-- check \S+ missed -->").unwrap());

/// The issues, as `repo#N`, whose body records a deferred check that missed, from the GitHub mirror's open issues: a miss reopens its issue.
pub fn missed_checks(mirror: &Mirror) -> BTreeSet<String> {
    let mut missed = BTreeSet::new();
    for repo in mirror::REPOS {
        let Some(copy) = mirror.read(&mirror::key(repo, mirror::OPEN_ISSUES)) else { continue };
        for issue in copy.body.as_array().into_iter().flatten() {
            if issue["body"].as_str().is_some_and(|body| MISSED.is_match(body)) {
                missed.insert(format!("{repo}#{}", issue["number"]));
            }
        }
    }
    missed
}

pub fn attempts(db: &Connection, since: i64, missed: &BTreeSet<String>, pending: &Pending) -> rusqlite::Result<Vec<Attempt>> {
    let mut statement = db.prepare("SELECT DISTINCT follows FROM runs WHERE coalesce(follows, '') != ''")?;
    let followed: HashSet<String> = statement.query_map([], |row| row.get(0))?.filter_map(Result::ok).collect();
    let mut statement = db.prepare(&format!(
        "SELECT agent, category, started, ended, {}, coalesce(item, ''), {}, {} FROM runs WHERE {} AND coalesce(follows, '') = '' AND outcome NOT IN ('running', 'idle', 'split') AND ended >= ?1",
        outcome_column(db),
        end_columns(db),
        fleet_column(db),
        worker_where()
    ))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, String>(6)?, row.get::<_, u32>(7)?, row.get::<_, Option<String>>(8)?, row.get::<_, String>(9)?)))?;
    let raws = rows
        .filter_map(Result::ok)
        .filter_map(|(agent, category, started, ended, outcome, item, report, compactions, recorded, session)| {
            let (started, ended) = (Timestamp::parse(&started).map_or(0, |at| at.unix_seconds()), Timestamp::parse(&ended).ok()?.unix_seconds());
            Some(Raw { agent, category, item, session, started, ended, end: End { outcome, report, compactions, recorded } })
        })
        .collect();
    let relaunches = relaunches(db)?;
    Ok(chain::fold(raws, &Marks::read(db), &relaunches, pending.now.saturating_sub(RELAUNCH_WAIT_S))
        .into_iter()
        .filter_map(|Raw { agent, category, item, session, started, ended, end: End { outcome, report, compactions, recorded } }| {
            let outcome = if outcome != "done" && pending.landed_during(&session, started, ended) { "done".to_owned() } else { outcome };
            let recorded = recorded.or_else(|| (outcome != "done" && pending.queued_behind_capacity(&session, started, ended)).then(|| Cause::Harness(Harness::Capacity).label().to_owned()));
            let checked_miss = || references(&item).iter().any(|issue| missed.contains(issue));
            if outcome != "done" && pending.holds(&session) {
                return None;
            }
            let deferred = outcome != "done" && recorded.as_deref() == Some(Cause::Harness(Harness::Waits).label()) && !checked_miss();
            Some(Attempt { succeeded: outcome == "done" && !followed.contains(&agent), agent, category, started, ended, repos: repos(&item), cause: attribute(&outcome, &report, compactions, recorded.as_deref()), deferred })
        })
        .collect())
}

/// The columns of how a run ended, in the order `End` reads them: its report, its compactions and its recorded cause.
fn end_columns(db: &Connection) -> String {
    format!("{}, coalesce(compactions, 0), {}", report_column(db), recorded_column(db))
}

/// The ends of the runs that relaunched a session that recorded a host need, by the stopped session.
fn relaunches(db: &Connection) -> rusqlite::Result<BTreeMap<String, End>> {
    let mut statement = db.prepare(&format!("SELECT follows, {}, {} FROM runs WHERE follows LIKE '% (Stop: host %'", outcome_column(db), end_columns(db)))?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, End { outcome: row.get(1)?, report: row.get(2)?, compactions: row.get(3)?, recorded: row.get(4)? })))?;
    Ok(rows.filter_map(Result::ok).filter_map(|(follows, end)| Some((follows.split_once(" (Stop: host ")?.0.to_owned(), end))).collect())
}

/// The fleet sessions whose unfinished first try is pending: its next report, which the lander's verdict or its own job's
/// exit wakes it to write, replaces this one.
#[derive(Default)]
pub struct Pending {
    /// Each session's lanes in this host's lander queues: a session with an undecided lane waits on the lander's verdict, which wakes it to report again.
    pub lanes: BTreeMap<String, Vec<Lane>>,
    /// Sessions holding an unexpired wait lease in the fleet ledger (a `job-run --notify` job, farm, branch or capture), whose end wakes them.
    pub waking: BTreeSet<String>,
    /// This host's capacity admission state: a session with a build still queued or running there is woken by its result to report again.
    pub admission: Option<std::path::PathBuf>,
    /// This host's capacity database: a failed run whose session queued past the bound for admission inside its window lost to capacity, whatever its report says.
    pub capacity: Option<Connection>,
    /// When the scorer reads: a first try with a recorded host need that ended less than `RELAUNCH_WAIT_S` before it waits on its relaunch.
    pub now: i64,
}

impl Pending {
    pub fn read(db: &Connection, now: i64) -> Self {
        Self { lanes: lanes_by_session(&land_queue::queues(&records_mirror::state_home())).unwrap_or_default(), waking: waking_sessions(db, now), admission: Some(crate::platform::admission_root::here()), capacity: capacity_database_readonly(), now }
    }

    fn queued_behind_capacity(&self, session: &str, from: i64, to: i64) -> bool {
        self.capacity.as_ref().is_some_and(|db| !queued_behind_capacity(db, session, from, to).is_empty())
    }

    /// Whether the session queued a lane inside the run that the lander has landed: done is the landing, read from the lander's own record when the scorer reads, not from the ledger's landed sha, which its 15-minute join fills in later.
    fn landed_during(&self, session: &str, from: i64, to: i64) -> bool {
        self.lanes.get(session).is_some_and(|lanes| lanes.iter().any(|lane| lane.landed() && (from..=to).contains(&i64::try_from(lane.queued_ms() / 1000).unwrap_or(i64::MAX))))
    }

    fn holds(&self, session: &str) -> bool {
        self.lanes.get(session).is_some_and(|lanes| lanes.iter().any(Lane::undecided)) || self.waking.contains(session) || self.admission.as_ref().is_some_and(|root| !crate::fleet::builds::held(root, Path::new("/proc"), session).is_empty())
    }
}

/// The fleet sessions with a wait whose lease outlasts `now`, from the ledger's `fleet_waits`; none in a ledger without it.
pub fn waking_sessions(db: &Connection, now: i64) -> BTreeSet<String> {
    let Ok(mut statement) = db.prepare("SELECT session, deadline FROM fleet_waits") else { return BTreeSet::new() };
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)));
    rows.into_iter().flatten().filter_map(Result::ok).filter(|(_, deadline)| Timestamp::parse(deadline).is_ok_and(|deadline| deadline.unix_seconds() > now)).map(|(session, _)| session).collect()
}

/// The repositories an item names, by `repo#N`.
pub fn repos(item: &str) -> BTreeSet<String> {
    references(item).iter().filter_map(|name| name.split_once('#').map(|(repo, _)| repo.to_owned())).collect()
}

pub fn first_try(db: &Connection, weeks: [Window; 2]) -> rusqlite::Result<Vec<Measure>> {
    let attempts = attempts(db, weeks[1].start, &missed_checks(&Mirror::local()), &Pending::read(db, weeks[0].end))?.into_iter().filter(|attempt| !attempt.deferred).collect::<Vec<_>>();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for attempt in attempts.iter().filter(|attempt| weeks[0].holds(attempt.ended)) {
        *counts.entry(attempt.category.as_str()).or_default() += 1;
    }
    let mut categories: Vec<&str> = attempts.iter().map(|attempt| attempt.category.as_str()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    categories.sort_by_key(|category| std::cmp::Reverse(counts.get(category).copied().unwrap_or(0)));
    let figure = |category: &str, window: Window| {
        let held: Vec<&Attempt> = attempts.iter().filter(|attempt| window.holds(attempt.ended) && (category == ALL || attempt.category == category)).collect();
        Figure::Share { hits: held.iter().filter(|attempt| attempt.succeeded).count(), of: held.len(), pending: 0, pending_label: "" }
    };
    Ok(std::iter::once(ALL)
        .chain(categories)
        .map(|category| Measure { name: format!("worker first-try success, {category}"), better: Better::Higher, source: SOURCE, weeks: weeks.map(|window| figure(category, window)) })
        .collect())
}

#[cfg(test)]
#[path = "workers_tests.rs"]
mod tests;
