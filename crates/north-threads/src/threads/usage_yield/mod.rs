//! Usage spent per first try that landed (north#793): the subscription share of weekly windows summed across
//! accounts plus the list-price dollars of api runs, over the first tries that landed, and the usage that reset or
//! expired unspent while ready work waited. Read from the ledger's usage readouts, runs and landings and the api
//! credit grants, never from a run's report.
mod week;

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

use self::week::Week;
use crate::foundation::clock::{Timestamp, parse_day_number};
use crate::platform::run_roles::worker_where;
use crate::router::pricing::PriceTable;
use crate::threads::attribution::outcome_column;

const DAY_S: i64 = 86_400;
const HISTORY_DAYS: i64 = 9;

/// Where the readings come from: the ledger, the orchestration policy that names the api grants, and the state root that holds the api spend.
pub struct Sources<'a> {
    pub db: &'a Connection,
    pub policy: &'a Path,
    pub state: &'a Path,
}

/// What one reading needs, loaded once and cut into windows after.
#[derive(Default)]
pub struct Facts {
    now: i64,
    landed: Vec<i64>,
    weeks: Vec<Week>,
    api_runs: Vec<(i64, f64)>,
    expired: Vec<(i64, f64)>,
    held_back: f64,
    pub unread: Vec<String>,
}

/// One window's usage: points are percent of one account's weekly window or one api dollar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    pub landed: usize,
    pub subscription_pct: f64,
    pub api_usd: f64,
    pub reset_unspent_pct: f64,
    pub expired_usd: f64,
    pub held_back_usd: f64,
}

impl Reading {
    pub fn per_landing(&self) -> Option<f64> {
        (self.landed > 0).then(|| (self.subscription_pct + self.api_usd) / self.landed as f64)
    }

    pub fn stranded(&self) -> f64 {
        self.reset_unspent_pct + self.expired_usd + self.held_back_usd
    }

    pub fn line(&self, hours: i64) -> String {
        let per = self.per_landing().map_or_else(|| "n/a".to_owned(), |points| format!("{points:.2}"));
        format!(
            "usage per landing {hours} h: {per} points ({:.0}% of weekly windows + ${:.2} api over {} first tries landed); stranded {hours} h: {:.0} points ({:.0}% of weekly windows reset unspent, ${:.2} api credit expired, ${:.2} api credit held back)",
            self.subscription_pct, self.api_usd, self.landed, self.stranded(), self.reset_unspent_pct, self.expired_usd, self.held_back_usd
        )
    }
}

impl Facts {
    /// The facts from the last nine days (the week a reading's baseline can sit in), each source that cannot be read named in `unread`.
    pub fn load(sources: &Sources, now: i64) -> Self {
        let since = now - HISTORY_DAYS * DAY_S;
        let mut facts = Self { now, ..Self::default() };
        match landed(sources.db, since) {
            Ok(landed) => facts.landed = landed,
            Err(why) => facts.unread.push(format!("first tries: {why}")),
        }
        match week::load(sources.db, since) {
            Ok(weeks) => facts.weeks = weeks,
            Err(why) => facts.unread.push(format!("usage readouts: {why}")),
        }
        if let Err(why) = facts.read_credit(sources, since) {
            facts.unread.push(format!("api credit: {why}"));
        }
        facts
    }

    fn read_credit(&mut self, sources: &Sources, since: i64) -> Result<(), String> {
        let prices = PriceTable::load().map_err(|why| why.to_string())?;
        let ledger = sources.state.join("north/ledger.db");
        let at = Timestamp::from_unix_seconds(self.now);
        self.api_runs = north_pace::credit::api_runs(&ledger, &prices, since * 1000)?;
        let spends = north_pace::credit::spends(&ledger, &prices, at)?;
        let (today, from) = (at.iso_date(), Timestamp::from_unix_seconds(since).iso_date());
        self.expired = north_pace::credit::expired_unspent(sources.policy, &spends.orgs, &from, &today).into_iter().filter_map(|(day, usd)| Some((parse_day_number(&day)? * DAY_S, usd))).collect();
        self.held_back = north_pace::credit::sources(sources.policy, sources.state, at).iter().map(|source| source.left.max(0.0)).fold(0.0, |all, left| all + left);
        Ok(())
    }

    /// The usage in `start..end`; `waited` says ready work waited then, which is what makes unspent usage stranded.
    pub fn reading(&self, start: i64, end: i64, waited: bool) -> Reading {
        let within = |at: i64| (start..end).contains(&at);
        let stranded = |usage: f64| if waited { usage } else { 0.0 };
        Reading {
            landed: self.landed.iter().filter(|at| within(**at)).count(),
            subscription_pct: self.weeks.iter().map(|week| week.spent(start, end)).fold(0.0, |all, spent| all + spent),
            api_usd: self.api_runs.iter().filter(|(at_ms, _)| within(at_ms / 1000)).map(|(_, usd)| usd).fold(0.0, |all, usd| all + usd),
            reset_unspent_pct: stranded(self.weeks.iter().filter(|week| within(week.resets)).map(Week::unspent).fold(0.0, |all, left| all + left)),
            expired_usd: stranded(self.expired.iter().filter(|(at, _)| within(*at)).map(|(_, usd)| usd).fold(0.0, |all, usd| all + usd)),
            held_back_usd: stranded(if end > self.now { self.held_back } else { 0.0 }),
        }
    }
}

/// When each worker first try that landed ended: a run with no `Follows:` that no later run follows and whose outcome is done, which the bars count as landed on origin/main.
fn landed(db: &Connection, since: i64) -> rusqlite::Result<Vec<i64>> {
    let mut statement = db.prepare("SELECT DISTINCT follows FROM runs WHERE coalesce(follows, '') != ''")?;
    let followed: HashSet<String> = statement.query_map([], |row| row.get(0))?.filter_map(Result::ok).collect();
    let mut statement = db.prepare(&format!("SELECT agent, ended FROM runs WHERE {} AND coalesce(follows, '') = '' AND {} = 'done' AND ended >= ?1", worker_where(), outcome_column(db)))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    Ok(rows.filter_map(Result::ok).filter(|(agent, _)| !followed.contains(agent)).filter_map(|(_, ended)| Some(Timestamp::parse(&ended).ok()?.unix_seconds())).collect())
}

#[cfg(test)]
mod tests;
