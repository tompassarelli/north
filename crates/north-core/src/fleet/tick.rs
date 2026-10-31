use std::time::{Duration, Instant, SystemTime};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

use super::invariant::{self, Facts, Kept};
use super::model::SessionRecord;
use super::registry::{Fleet, HandedIn};
use super::store::{Addressee, Exchange, Exchanged, FarRegistry, FarSpools, FleetRow, Handed, HandedRow, LedgerHost, SessionRow, WaitKey, WaitRow};

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
    pub pulled: usize,
    pub pull_failed: Vec<(String, String)>,
    pub drop_failed: Vec<(String, String)>,
    pub refused: Vec<(String, String)>,
    pub probed: Vec<String>,
    pub kept: Kept,
    pub unsettled: Option<String>,
    pub uncached: Vec<(String, String)>,
    pub far_failed: Vec<(String, String)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FarHost {
    pub name: String,
    pub ssh: String,
}

pub struct Pulls<'a> {
    pub spools: &'a dyn FarSpools,
    pub always: Vec<FarHost>,
}

pub struct Probed {
    pub lines: Vec<String>,
    pub settled: Vec<WaitKey>,
}

pub trait WaitProbes {
    fn settle(&self, fleet: &Fleet) -> Probed;
}

pub struct NoProbes;

impl WaitProbes for NoProbes {
    fn settle(&self, _: &Fleet) -> Probed {
        Probed { lines: Vec::new(), settled: Vec::new() }
    }
}

struct Taken {
    host: FarHost,
    ids: Vec<String>,
    refused: Vec<String>,
}

impl Ticked {
    fn note(&mut self, handed: Vec<HandedRow>) {
        for HandedRow { id, to, handed } in handed {
            match handed {
                Handed::Delivered => self.delivered.push((id, to)),
                Handed::Already => self.already += 1,
                Handed::Deferred => self.deferred += 1,
                Handed::Failed(why) => self.failed.push((id, why)),
            }
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let delivered = self.delivered.iter().map(|(id, to)| format!("delivered {id} to {to}"));
        let failed = self.failed.iter().map(|(id, why)| format!("not delivered {id}: {why}"));
        let held = self.held_elsewhere.iter().map(|name| format!("not delivering to {name}: another host holds the name"));
        let unpulled = self.pull_failed.iter().map(|(name, why)| format!("not pulled from {name}: {why}"));
        let undropped = self.drop_failed.iter().map(|(name, why)| format!("not dropped or set aside on {name}: {why}"));
        let refused = self.refused.iter().map(|(name, why)| format!("set aside on {name}, the store refuses it: {why}"));
        let uncached = self.uncached.iter().map(|(name, why)| format!("fleet table not cached on {name}: {why}"));
        let undelivered = self.far_failed.iter().map(|(name, why)| format!("not delivered to the registry on {name}: {why}"));
        let counts = format!("flushed {}, pulled {}, delivered {}, already {}, deferred {}, pruned {}", self.flushed, self.pulled, self.delivered.len(), self.already, self.deferred, self.pruned);
        let unsettled = self.unsettled.iter().map(|why| format!("waits not settled in the store: {why}"));
        delivered.chain(failed).chain(held).chain(unpulled).chain(undropped).chain(refused).chain(uncached).chain(undelivered).chain(self.probed.iter().cloned()).chain(self.kept.lines()).chain(unsettled).chain([counts]).collect()
    }
}

pub fn tick(fleet: &Fleet, ledger: &mut dyn LedgerHost, pulls: &Pulls, probes: &dyn WaitProbes, now_ms: u64, budget: Duration) -> NorthResult<Ticked> {
    let spool = fleet.spool();
    let mut rows = spool.pending()?;
    let spooled: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let records = fleet.sessions()?;
    let mut ticked = Ticked::default();
    let taken = take_far(fleet, &records, pulls, &mut rows, &mut ticked);
    let pulled = rows.len() - spooled.len();
    let FarRegistry { sessions, addressees, .. } = registry(fleet, &records, now_ms)?;
    let exchange = Exchange { rows, host: fleet.host().to_owned(), sessions, addressees };
    let exchanged = ledger.exchange(&exchange)?;
    for id in &spooled {
        spool.withdraw(id)?;
    }
    fleet.cache_sessions(&exchanged.sessions)?;
    let started = Instant::now();
    ticked.flushed = spooled.len();
    ticked.pulled = pulled;
    ticked.held_elsewhere = exchanged.held_elsewhere.clone();
    let here: Vec<FleetRow> = exchanged.rows.iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).cloned().collect();
    ticked.note(hand_in_all(fleet, &here, started, budget));
    ticked.drop_failed = drop_far(pulls, &taken);
    ticked.uncached = cache_far(pulls, &taken, &exchanged.sessions);
    deliver_far(ledger, pulls, &taken, &mut ticked, started, budget);
    let probed = probes.settle(fleet);
    ticked.probed = probed.lines;
    let pending = fleet.unsettled_waits();
    let waits: Vec<WaitRow> = exchanged.waits.iter().filter(|wait| !probed.settled.contains(&wait.key()) && !pending.contains(&wait.key())).cloned().collect();
    match keep_invariant(fleet, &ticked, &exchanged, &waits, now_ms) {
        Ok(kept) => ticked.kept = kept,
        Err(error) => ticked.kept.failed.push(("the invariant".into(), error.user_message())),
    }
    let settled: Vec<WaitKey> = pending.into_iter().chain(probed.settled).chain(ticked.kept.settled.iter().cloned()).collect();
    if !settled.is_empty() {
        let left = match ledger.settle(&settled) {
            Ok(()) => Vec::new(),
            Err(error) => {
                ticked.unsettled = Some(error.user_message());
                settled
            }
        };
        fleet.set_unsettled_waits(&left)?;
    }
    ticked.pruned = fleet.prune_delivered(SystemTime::now() - JOURNAL_KEPT)?;
    Ok(ticked)
}

pub fn registry(fleet: &Fleet, records: &[SessionRecord], now_ms: u64) -> NorthResult<FarRegistry> {
    let floor = now_ms.saturating_sub(u64::try_from(RETENTION.as_millis()).unwrap_or(u64::MAX));
    Ok(FarRegistry { host: fleet.host().to_owned(), sessions: records.iter().map(|record| fleet.table_row(record, now_ms)).collect(), addressees: records.iter().map(|record| addressee(fleet, record, floor)).collect::<NorthResult<_>>()? })
}

pub fn hand_in_all(fleet: &Fleet, rows: &[FleetRow], started: Instant, budget: Duration) -> Vec<HandedRow> {
    rows.iter()
        .map(|row| {
            let handed = if started.elapsed() >= budget {
                Handed::Deferred
            } else {
                match fleet.deliver_row(row) {
                    Ok(HandedIn::Delivered) => Handed::Delivered,
                    Ok(HandedIn::Already) => Handed::Already,
                    Err(error) => Handed::Failed(error.user_message()),
                }
            };
            HandedRow { id: row.id.clone(), to: row.to.clone(), handed }
        })
        .collect()
}

fn deliver_far(ledger: &mut dyn LedgerHost, pulls: &Pulls, taken: &[Taken], ticked: &mut Ticked, started: Instant, budget: Duration) {
    let registries = in_parallel(taken, |taken| pulls.spools.registry(&taken.host.ssh));
    for (taken, registry) in taken.iter().zip(registries) {
        let name = taken.host.name.clone();
        if started.elapsed() >= budget {
            ticked.far_failed.push((name, "deferred: the tick spent its budget".into()));
            continue;
        }
        let delivered = registry.and_then(|FarRegistry { host, sessions, addressees }| {
            if host.is_empty() {
                return Ok(Vec::new());
            }
            let exchanged = ledger.exchange(&Exchange { rows: Vec::new(), host, sessions, addressees })?;
            let rows: Vec<FleetRow> = exchanged.rows.into_iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).collect();
            if rows.is_empty() { Ok(Vec::new()) } else { pulls.spools.deliver(&taken.host.ssh, &rows) }
        });
        match delivered {
            Ok(handed) => ticked.note(handed),
            Err(error) => ticked.far_failed.push((name, error.user_message())),
        }
    }
}

fn keep_invariant(fleet: &Fleet, ticked: &Ticked, exchanged: &Exchanged, waits: &[WaitRow], now_ms: u64) -> NorthResult<Kept> {
    let pull_failed: Vec<String> = ticked.pull_failed.iter().map(|(name, _)| name.clone()).collect();
    let gated = fleet.gated_sessions(now_ms)?;
    let records: Vec<SessionRecord> = fleet.sessions()?.into_iter().filter(|record| !exchanged.held_elsewhere.contains(&record.name)).collect();
    Ok(invariant::keep(fleet, &records, &Facts { waits, now: Timestamp::parse(&exchanged.now)?, pull_failed: &pull_failed, gated: &gated }))
}

fn take_far(fleet: &Fleet, records: &[SessionRecord], pulls: &Pulls, rows: &mut Vec<FleetRow>, ticked: &mut Ticked) -> Vec<Taken> {
    let hosts = far_hosts(fleet, records, pulls);
    let answers = in_parallel(&hosts, |host| pulls.spools.take(&host.ssh));
    let mut taken = Vec::new();
    for (host, answer) in hosts.into_iter().zip(answers) {
        match answer {
            Ok(found) => {
                let (mut ids, mut refused) = (Vec::new(), Vec::new());
                for row in found {
                    match row.validate() {
                        Ok(()) => {
                            ids.push(row.id.clone());
                            rows.push(row);
                        }
                        Err(error) => {
                            ticked.refused.push((host.name.clone(), format!("{}: {}", row.id, error.user_message())));
                            if FleetRow::check_id(&row.id).is_ok() {
                                refused.push(row.id);
                            }
                        }
                    }
                }
                taken.push(Taken { host, ids, refused });
            }
            Err(error) => ticked.pull_failed.push((host.name.clone(), error.user_message())),
        }
    }
    taken
}

fn drop_far(pulls: &Pulls, taken: &[Taken]) -> Vec<(String, String)> {
    let held: Vec<&Taken> = taken.iter().filter(|taken| !taken.ids.is_empty() || !taken.refused.is_empty()).collect();
    let answers = in_parallel(&held, |taken| {
        let dropped = if taken.ids.is_empty() { Ok(()) } else { pulls.spools.drop(&taken.host.ssh, &taken.ids) };
        dropped.and_then(|()| if taken.refused.is_empty() { Ok(()) } else { pulls.spools.set_aside(&taken.host.ssh, &taken.refused) })
    });
    held.iter().zip(answers).filter_map(|(taken, answer)| answer.err().map(|error| (taken.host.name.clone(), error.user_message()))).collect()
}

fn cache_far(pulls: &Pulls, taken: &[Taken], sessions: &[SessionRow]) -> Vec<(String, String)> {
    let answers = in_parallel(taken, |taken| pulls.spools.cache(&taken.host.ssh, sessions));
    taken.iter().zip(answers).filter_map(|(taken, answer)| answer.err().map(|error| (taken.host.name.clone(), error.user_message()))).collect()
}

fn in_parallel<T: Sync, R: Send>(items: &[T], run: impl Fn(&T) -> R + Sync) -> Vec<R> {
    std::thread::scope(|scope| {
        let running: Vec<_> = items.iter().map(|item| scope.spawn(|| run(item))).collect();
        running.into_iter().map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

fn far_hosts(fleet: &Fleet, records: &[SessionRecord], pulls: &Pulls) -> Vec<FarHost> {
    let placed = records.iter().filter(|record| !record.stopped).filter_map(|record| record.machine.as_ref()).map(|machine| FarHost { name: machine.name.clone(), ssh: machine.ssh.clone() });
    let mut hosts: Vec<FarHost> = Vec::new();
    for host in placed.chain(pulls.always.iter().cloned()).filter(|host| host.name != fleet.host()) {
        if !hosts.iter().any(|seen| seen.name == host.name || seen.ssh == host.ssh) {
            hosts.push(host);
        }
    }
    hosts
}

fn addressee(fleet: &Fleet, record: &SessionRecord, floor: u64) -> NorthResult<Addressee> {
    Ok(Addressee { name: record.name.clone(), stopped: record.stopped, control_since_ms: Some(record.created_ms.max(floor)), since_ms: record.addressed_since().max(floor), taken: fleet.delivered(&record.name)?.into_iter().filter(|id| sent_at(id).is_none_or(|ms| ms >= floor)).collect() })
}

fn sent_at(id: &str) -> Option<u64> {
    id.split('-').next()?.parse().ok()
}

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tests;
