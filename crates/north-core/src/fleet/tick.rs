use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use crate::error::NorthResult;

use super::model::SessionRecord;
use super::peer_row::{self, Refusal};
use super::registry::{Fleet, HandedIn};
use super::store::{Exchange, FarRegistry, FarSpools, FleetRow, FleetTable, Handed, HandedRow, LAUNCH_GRACE_MS, LedgerHost, RowKind, SessionRow, Undecodable, UnknownKind, WaitKey};

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
    pub left: Vec<(String, String)>,
    pub unregistered: Vec<(String, String)>,
    pub probed: Vec<String>,
    pub unsettled: Option<String>,
    pub uncached: Vec<(String, String)>,
    pub far_failed: Vec<(String, String)>,
    pub wall: Duration,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    pub name: &'static str,
    pub began: Duration,
    pub took: Duration,
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

pub trait WaitProbes: Sync {
    fn prepare(&self, _: &Fleet) {}

    fn settle(&self, fleet: &Fleet) -> Probed;
}

pub struct NoProbes;

impl WaitProbes for NoProbes {
    fn settle(&self, _: &Fleet) -> Probed {
        Probed { lines: Vec::new(), settled: Vec::new() }
    }
}

struct Clock {
    origin: Instant,
    steps: Mutex<Vec<Step>>,
}

impl Clock {
    fn new() -> Self {
        Self { origin: Instant::now(), steps: Mutex::new(Vec::new()) }
    }

    fn time<R>(&self, name: &'static str, run: impl FnOnce() -> R) -> R {
        let began = self.origin.elapsed();
        let ran = run();
        let took = self.origin.elapsed().saturating_sub(began);
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(Step { name, began, took });
        }
        ran
    }

    fn stop(self, ticked: &mut Ticked) {
        ticked.wall = self.origin.elapsed();
        let mut steps = self.steps.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
        steps.sort_by_key(|step| step.began);
        ticked.steps = steps;
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
        let left = self.left.iter().map(|(name, why)| format!("left on {name} for a build that knows its kind: {why}"));
        let unregistered = self.unregistered.iter().map(|(name, why)| format!("left on {name} until the ledger lists the session: {why}"));
        let uncached = self.uncached.iter().map(|(name, why)| format!("fleet table not cached on {name}: {why}"));
        let undelivered = self.far_failed.iter().map(|(name, why)| format!("not delivered to the registry on {name}: {why}"));
        let counts = format!("flushed {}, pulled {}, delivered {}, already {}, deferred {}, pruned {}", self.flushed, self.pulled, self.delivered.len(), self.already, self.deferred, self.pruned);
        let unsettled = self.unsettled.iter().map(|why| format!("waits not settled in the store: {why}"));
        delivered.chain(failed).chain(held).chain(unpulled).chain(undropped).chain(refused).chain(left).chain(unregistered).chain(uncached).chain(undelivered).chain(self.probed.iter().cloned()).chain(unsettled).chain([counts]).collect()
    }

    pub fn timing(&self) -> String {
        let wall = self.wall.as_secs_f64().max(f64::EPSILON);
        let steps: Vec<String> = self.steps.iter().map(|step| format!("{} {}ms {:.0}%", step.name, step.took.as_millis(), 100.0 * step.took.as_secs_f64() / wall)).collect();
        format!("tick took {}ms: {}", self.wall.as_millis(), steps.join(", "))
    }
}

pub fn tick(fleet: &Fleet, ledger: &mut dyn LedgerHost, pulls: &Pulls, probes: &dyn WaitProbes, now_ms: u64, budget: Duration) -> NorthResult<Ticked> {
    let clock = Clock::new();
    let mut ticked = Ticked::default();
    std::thread::scope(|scope| -> NorthResult<()> {
        let placing = scope.spawn(|| clock.time("placement", || probes.prepare(fleet)));
        let exchanged = exchange_and_hand_in(fleet, ledger, pulls, &clock, &mut ticked, (now_ms, budget));
        joined(placing);
        exchanged?;
        settle(fleet, ledger, probes, &clock, &mut ticked)
    })?;
    ticked.pruned = clock.time("prune", || fleet.prune_delivered(SystemTime::now() - JOURNAL_KEPT))?;
    clock.stop(&mut ticked);
    Ok(ticked)
}

fn exchange_and_hand_in(fleet: &Fleet, ledger: &mut dyn LedgerHost, pulls: &Pulls, clock: &Clock, ticked: &mut Ticked, (now_ms, budget): (u64, Duration)) -> NorthResult<()> {
    let spool = fleet.spool();
    let (rows, records, cached) = std::thread::scope(|scope| {
        let reading = scope.spawn(|| clock.time("sessions", || fleet.sessions()));
        let caching = scope.spawn(|| clock.time("cache_read", || fleet.cached_sessions().ok().flatten().unwrap_or_default()));
        let rows = clock.time("spool", || spool.pending());
        (rows, joined(reading), joined(caching))
    });
    let (mut rows, records) = (rows?, records?);
    let spooled: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let (taken, registered) = std::thread::scope(|scope| {
        let building = scope.spawn(|| clock.time("registry", || registry(fleet, &records, now_ms)));
        let taken = clock.time("take_far", || take_far(fleet, &records, cached, pulls, &mut rows, ticked, now_ms));
        (taken, joined(building))
    });
    let pulled = rows.len() - spooled.len();
    let FarRegistry { sessions, addressees, gates, .. } = registered?;
    let outranks = far_hosts(fleet, &records, pulls).into_iter().map(|host| host.name).collect();
    let exchange = Exchange { rows, host: fleet.host().to_owned(), sessions, addressees, gates, outranks };
    let exchanged = clock.time("ledger.exchange", || ledger.exchange(&exchange))?;
    clock.time("spool.withdraw", || spooled.iter().try_for_each(|id| spool.withdraw(id)))?;
    let table = FleetTable { sessions: exchanged.sessions.clone(), gates: exchanged.gates.clone() };
    clock.time("cache_table", || fleet.cache_table(&table))?;
    let started = Instant::now();
    ticked.flushed = spooled.len();
    ticked.pulled = pulled;
    ticked.held_elsewhere = exchanged.held_elsewhere.clone();
    let here: Vec<FleetRow> = exchanged.rows.iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).cloned().collect();
    ticked.note(clock.time("hand_in_all", || hand_in_all(fleet, &here, started, budget)));
    ticked.drop_failed = clock.time("drop_far", || drop_far(pulls, &taken));
    ticked.uncached = clock.time("cache_far", || cache_far(pulls, &taken, &table));
    clock.time("deliver_far", || deliver_far(ledger, pulls, &taken, &exchanged.sessions, ticked, (started, budget, now_ms)));
    Ok(())
}

fn settle(fleet: &Fleet, ledger: &mut dyn LedgerHost, probes: &dyn WaitProbes, clock: &Clock, ticked: &mut Ticked) -> NorthResult<()> {
    let probed = clock.time("probes.settle", || probes.settle(fleet));
    ticked.probed = probed.lines;
    let settled: Vec<WaitKey> = fleet.unsettled_waits().into_iter().chain(probed.settled).collect();
    if settled.is_empty() {
        return Ok(());
    }
    let left = match clock.time("settle", || ledger.settle(&settled)) {
        Ok(()) => Vec::new(),
        Err(error) => {
            ticked.unsettled = Some(error.user_message());
            settled
        }
    };
    fleet.set_unsettled_waits(&left)
}

fn joined<R>(handle: std::thread::ScopedJoinHandle<'_, R>) -> R {
    handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

pub fn registry(fleet: &Fleet, records: &[SessionRecord], now_ms: u64) -> NorthResult<FarRegistry> {
    let floor = now_ms.saturating_sub(u64::try_from(RETENTION.as_millis()).unwrap_or(u64::MAX));
    Ok(FarRegistry { host: fleet.host().to_owned(), sessions: records.iter().map(|record| fleet.table_row(record, now_ms)).collect(), addressees: records.iter().map(|record| fleet.addressee(record, floor)).collect::<NorthResult<_>>()?, gates: fleet.open_gate_rows()? })
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

fn deliver_far(ledger: &mut dyn LedgerHost, pulls: &Pulls, taken: &[Taken], known: &[SessionRow], ticked: &mut Ticked, (started, budget, now_ms): (Instant, Duration, u64)) {
    let registries = in_parallel(taken, |taken| pulls.spools.registry(&taken.host.ssh));
    for (taken, registry) in taken.iter().zip(registries) {
        let name = taken.host.name.clone();
        if started.elapsed() >= budget {
            ticked.far_failed.push((name, "deferred: the tick spent its budget".into()));
            continue;
        }
        let delivered = registry.and_then(|FarRegistry { host, sessions, addressees, gates }| {
            if host.is_empty() {
                return Ok(Vec::new());
            }
            let admitted = peer_row::admit_exchange(Exchange { rows: Vec::new(), host, sessions, addressees, gates, outranks: Vec::new() }, &taken.host.name, known, now_ms)?;
            let exchanged = ledger.exchange(&admitted)?;
            let rows: Vec<FleetRow> = exchanged.rows.into_iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).collect();
            if rows.is_empty() { Ok(Vec::new()) } else { pulls.spools.deliver(&taken.host.ssh, &rows) }
        });
        match delivered {
            Ok(handed) => ticked.note(handed),
            Err(error) => ticked.far_failed.push((name, error.user_message())),
        }
    }
}

fn take_far(fleet: &Fleet, records: &[SessionRecord], cached: Vec<SessionRow>, pulls: &Pulls, rows: &mut Vec<FleetRow>, ticked: &mut Ticked, now_ms: u64) -> Vec<Taken> {
    let hosts = far_hosts(fleet, records, pulls);
    let answers = in_parallel(&hosts, |host| pulls.spools.take(&host.ssh));
    let known = ledger_records(fleet, records, cached, now_ms);
    let archived: Vec<SessionRecord> = fleet.past_sessions();
    let mut taken = Vec::new();
    for (host, answer) in hosts.into_iter().zip(answers) {
        match answer {
            Ok(found) => {
                let (mut ids, mut refused) = (Vec::new(), Vec::new());
                for UnknownKind { id, kind } in found.unknown_kind {
                    ticked.left.push((host.name.clone(), format!("{id}: kind {kind} is not known to this build")));
                }
                for Undecodable { id, why } in found.undecodable {
                    ticked.refused.push((host.name.clone(), format!("{id}: {why}")));
                    if FleetRow::check_id(&id).is_ok() {
                        refused.push(id);
                    }
                }
                for row in found.rows {
                    let admitted = row.validate().map_err(|error| Refusal::Spoofed(format!("{}: {}", row.id, error.user_message()))).and_then(|()| peer_row::admit(&row, &host.name, &known));
                    match admitted {
                        Ok(admitted) => {
                            ids.push(admitted.id.clone());
                            rows.push(admitted);
                        }
                        Err(Refusal::Unregistered(_)) if retired_report(&row, &host.name, &archived) => {
                            ids.push(row.id.clone());
                            rows.push(FleetRow { origin: Some(host.name.clone()), urgent: false, ..row });
                        }
                        Err(Refusal::Unregistered(why)) if now_ms.abs_diff(row.sent_at) > LAUNCH_GRACE_MS => {
                            ticked.refused.push((host.name.clone(), format!("{why}, and the ledger has not listed it within {} minutes", LAUNCH_GRACE_MS / 60_000)));
                            refused.push(row.id);
                        }
                        Err(Refusal::Unregistered(why)) => ticked.unregistered.push((host.name.clone(), why)),
                        Err(Refusal::Spoofed(why)) => {
                            ticked.refused.push((host.name.clone(), why));
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

fn ledger_records(fleet: &Fleet, records: &[SessionRecord], mut known: Vec<SessionRow>, now_ms: u64) -> Vec<SessionRow> {
    let unlisted: Vec<SessionRow> = records.iter().filter(|record| !known.iter().any(|row| row.name == record.name)).map(|record| fleet.table_row(record, now_ms)).collect();
    known.extend(unlisted);
    known
}

fn retired_report(row: &FleetRow, peer: &str, archived: &[SessionRecord]) -> bool {
    row.kind == RowKind::Message && row.sender.session().is_some_and(|sender| archived.iter().any(|record| record.name == sender && record.machine.as_ref().is_some_and(|machine| machine.name == peer)))
}

fn drop_far(pulls: &Pulls, taken: &[Taken]) -> Vec<(String, String)> {
    let held: Vec<&Taken> = taken.iter().filter(|taken| !taken.ids.is_empty() || !taken.refused.is_empty()).collect();
    let answers = in_parallel(&held, |taken| {
        let dropped = if taken.ids.is_empty() { Ok(()) } else { pulls.spools.drop(&taken.host.ssh, &taken.ids) };
        dropped.and_then(|()| if taken.refused.is_empty() { Ok(()) } else { pulls.spools.set_aside(&taken.host.ssh, &taken.refused) })
    });
    held.iter().zip(answers).filter_map(|(taken, answer)| answer.err().map(|error| (taken.host.name.clone(), error.user_message()))).collect()
}

fn cache_far(pulls: &Pulls, taken: &[Taken], table: &FleetTable) -> Vec<(String, String)> {
    let answers = in_parallel(taken, |taken| pulls.spools.cache(&taken.host.ssh, table));
    taken.iter().zip(answers).filter_map(|(taken, answer)| answer.err().map(|error| (taken.host.name.clone(), error.user_message()))).collect()
}

fn in_parallel<T: Sync, R: Send>(items: &[T], run: impl Fn(&T) -> R + Sync) -> Vec<R> {
    std::thread::scope(|scope| {
        let running: Vec<_> = items.iter().map(|item| scope.spawn(|| run(item))).collect();
        running.into_iter().map(joined).collect()
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

#[cfg(test)]
#[path = "tick_tests.rs"]
mod tests;
