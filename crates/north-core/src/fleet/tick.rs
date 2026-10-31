use std::time::{Duration, Instant, SystemTime};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

use super::model::SessionRecord;
use super::registry::{Fleet, HandedIn};
use super::store::{Addressee, Exchange, LedgerHost, SessionRow};

pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const JOURNAL_KEPT: Duration = Duration::from_secs(2 * 7 * 24 * 60 * 60);

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Ticked {
    pub flushed: usize,
    pub delivered: Vec<(String, String)>,
    pub already: usize,
    pub failed: Vec<(String, String)>,
    pub deferred: usize,
    pub held_elsewhere: Vec<String>,
    pub pruned: usize,
}

impl Ticked {
    pub fn lines(&self) -> Vec<String> {
        let delivered = self.delivered.iter().map(|(id, to)| format!("delivered {id} to {to}"));
        let failed = self.failed.iter().map(|(id, why)| format!("not delivered {id}: {why}"));
        let held = self.held_elsewhere.iter().map(|name| format!("not delivering to {name}: another host holds the name"));
        let counts = format!("flushed {}, delivered {}, already {}, deferred {}, pruned {}", self.flushed, self.delivered.len(), self.already, self.deferred, self.pruned);
        delivered.chain(failed).chain(held).chain([counts]).collect()
    }
}

pub fn tick(fleet: &Fleet, ledger: &mut dyn LedgerHost, now_ms: u64, budget: Duration) -> NorthResult<Ticked> {
    let started = Instant::now();
    let spool = fleet.spool();
    let rows = spool.pending()?;
    let spooled: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let records = fleet.sessions()?;
    let floor = now_ms.saturating_sub(u64::try_from(RETENTION.as_millis()).unwrap_or(u64::MAX));
    let exchange = Exchange {
        rows,
        sessions: records.iter().map(|record| published(fleet, record, now_ms)).collect(),
        addressees: records.iter().filter(|record| !record.stopped).map(|record| addressee(fleet, record, floor)).collect::<NorthResult<_>>()?,
    };
    let exchanged = ledger.exchange(&exchange)?;
    for id in &spooled {
        spool.withdraw(id)?;
    }
    fleet.cache_sessions(&exchanged.sessions)?;
    let mut ticked = Ticked { flushed: spooled.len(), held_elsewhere: exchanged.held_elsewhere.clone(), ..Ticked::default() };
    for row in exchanged.rows.iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)) {
        if started.elapsed() >= budget {
            ticked.deferred += 1;
            continue;
        }
        match fleet.hand_in(row) {
            Ok(HandedIn::Delivered) => ticked.delivered.push((row.id.clone(), row.to.clone())),
            Ok(HandedIn::Already) => ticked.already += 1,
            Err(error) => ticked.failed.push((row.id.clone(), error.user_message())),
        }
    }
    ticked.pruned = fleet.prune_delivered(SystemTime::now() - JOURNAL_KEPT)?;
    Ok(ticked)
}

fn addressee(fleet: &Fleet, record: &SessionRecord, floor: u64) -> NorthResult<Addressee> {
    Ok(Addressee { name: record.name.clone(), since_ms: record.created_ms.max(floor), taken: fleet.delivered(&record.name)?.into_iter().filter(|id| sent_at(id).is_none_or(|ms| ms >= floor)).collect() })
}

fn sent_at(id: &str) -> Option<u64> {
    id.split('-').next()?.parse().ok()
}

fn published(fleet: &Fleet, record: &SessionRecord, now_ms: u64) -> SessionRow {
    SessionRow {
        name: record.name.clone(),
        host: fleet.host().to_owned(),
        status: fleet.status(record).label().to_owned(),
        created: iso(record.created_ms),
        updated: iso(now_ms),
        provider: record.provider.label().to_owned(),
        account: record.account.clone(),
        cwd: record.cwd.display().to_string(),
        parent: record.parent.clone(),
        placement: record.placement.clone(),
        machine: record.machine.as_ref().map(|machine| machine.name.clone()),
        tasks: record.tasks().map(str::to_owned).collect(),
    }
}

fn iso(ms: u64) -> String {
    Timestamp::from_unix_millis(i64::try_from(ms).unwrap_or(i64::MAX)).iso()
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tests;
