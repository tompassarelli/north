use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::LazyLock;

use north_github_mirror::store::{self as mirror, Mirror};
use regex::Regex;
use rusqlite::Connection;

use super::{Better, Figure, Measure, Window};
use crate::foundation::clock::Timestamp;
use crate::ledger::outcome::attention::references;
use crate::platform::attribution::awaits_lander;
use crate::platform::run_roles::worker_where;
use crate::platform::{land_queue, records_mirror};
use crate::providers::claude::runs::outcome::waits_on_time;
use crate::threads::attribution::{Cause, attribute, fleet_column, outcome_column, recorded_column, report_column};

const SOURCE: &str = "worker-ledger runs with no Follows: (first attempts), done with no later run following them; split, idle, running, deferred, lander-queued and job-waking runs left out unless the deferred box's check missed (threads runs, GitHub mirror)";
const ALL: &str = "all categories";

pub struct Attempt {
    pub category: String,
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
        "SELECT agent, category, ended, {}, coalesce(item, ''), {}, coalesce(compactions, 0), {}, {} FROM runs WHERE {} AND coalesce(follows, '') = '' AND outcome NOT IN ('running', 'idle', 'split') AND ended >= ?1",
        outcome_column(db),
        report_column(db),
        recorded_column(db),
        fleet_column(db),
        worker_where()
    ))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, u32>(6)?, row.get::<_, Option<String>>(7)?, row.get::<_, String>(8)?)))?;
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|(agent, category, ended, outcome, item, report, compactions, recorded, session)| {
            let ended = Timestamp::parse(&ended).ok()?.unix_seconds();
            let checked_miss = || references(&item).iter().any(|issue| missed.contains(issue));
            if outcome != "done" && pending.holds(&report, &session) {
                return None;
            }
            let deferred = outcome != "done" && waits_on_time(&report) && !checked_miss();
            Some(Attempt { succeeded: outcome == "done" && !followed.contains(&agent), category, ended, repos: repos(&item), cause: attribute(&outcome, &report, compactions, recorded.as_deref()), deferred })
        })
        .collect())
}

/// The fleet sessions whose unfinished first try is pending: its next report, which the lander's verdict or its own job's
/// exit wakes it to write, replaces this one.
#[derive(Default)]
pub struct Pending {
    /// Sessions whose lane waits undecided in this host's lander queues, pending when the report stops on landing.
    pub queued: BTreeSet<String>,
    /// Sessions holding an unexpired wait lease in the fleet ledger (a `job-run --notify` job, farm, branch or capture), whose end wakes them.
    pub waking: BTreeSet<String>,
}

impl Pending {
    pub fn read(db: &Connection, now: i64) -> Self {
        Self { queued: land_queue::queued_sessions(&records_mirror::state_home()), waking: waking_sessions(db, now) }
    }

    fn holds(&self, report: &str, session: &str) -> bool {
        (awaits_lander(report) && self.queued.contains(session)) || self.waking.contains(session)
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
