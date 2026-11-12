use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use crate::error::NorthResult;

use super::model::SessionRecord;

mod cpu;
mod far;
pub use cpu::{Cpu, detach_metered, metered};
use far::{cache_far, deliver_far, drop_far, far_hosts, take_far};
use super::registry::{Fleet, HandedIn};
use super::stage::{Detached, Late, detach};
use super::store::{Exchange, Exchanged, FarRegistry, FarSpools, FleetRow, FleetTable, Handed, HandedRow, LeaseTable, LedgerHost, WaitKey};

pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const JOURNAL_KEPT: Duration = Duration::from_secs(2 * 7 * 24 * 60 * 60);

pub type OpenLedger = Arc<dyn Fn() -> NorthResult<Box<dyn LedgerHost + Send>> + Send + Sync>;

pub fn open_ledger<L: LedgerHost + Send + 'static>(open: impl Fn() -> NorthResult<L> + Send + Sync + 'static) -> OpenLedger {
    Arc::new(move || Ok(Box::new(open()?) as Box<dyn LedgerHost + Send>))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Budgets {
    pub placement: Duration,
    pub take_far: Duration,
    pub ledger: Duration,
    pub hand_in: Duration,
    pub far: Duration,
    pub probes: Duration,
    pub queue: Duration,
}

impl Budgets {
    pub const TICK: Self = Self {
        placement: Duration::from_secs(8),
        take_far: Duration::from_secs(4),
        ledger: Duration::from_secs(6),
        hand_in: Duration::from_secs(4),
        far: Duration::from_secs(4),
        probes: Duration::from_secs(14),
        queue: Duration::from_secs(5),
    };

    pub const fn each(budget: Duration) -> Self {
        Self { placement: budget, take_far: budget, ledger: budget, hand_in: budget, far: budget, probes: budget, queue: budget }
    }

    pub fn worst(&self) -> Duration {
        self.placement.max(self.take_far + self.ledger) + self.hand_in + self.far + self.probes + self.ledger
    }

    pub fn queue_after(&self, spent: Duration) -> Duration {
        self.queue.min(self.worst().saturating_sub(spent))
    }
}

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
    pub put_off: Vec<String>,
    pub unsettled: Option<String>,
    pub uncached: Vec<(String, String)>,
    pub far_failed: Vec<(String, String)>,
    pub wall: Duration,
    pub cpu: Duration,
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Step {
    pub name: &'static str,
    pub began: Duration,
    pub took: Duration,
    pub cpu: Option<Duration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FarHost {
    pub name: String,
    pub ssh: String,
}

pub struct Pulls {
    pub spools: Arc<dyn FarSpools + Send>,
    pub always: Vec<FarHost>,
}

pub struct Probed {
    pub lines: Vec<String>,
    pub settled: Vec<WaitKey>,
}

pub struct ProbeJob {
    pub name: &'static str,
    pub own: Option<Duration>,
    pub run: Box<dyn FnOnce(Fleet) -> Probed + Send>,
}

pub trait WaitProbes: Sync {
    fn placement(&self) -> Option<Box<dyn FnOnce() + Send>> {
        None
    }

    fn jobs(&self) -> Vec<ProbeJob>;
}

pub struct NoProbes;

impl WaitProbes for NoProbes {
    fn jobs(&self) -> Vec<ProbeJob> {
        Vec::new()
    }
}

struct Clock {
    origin: Instant,
    spent: Cpu,
    steps: Mutex<Vec<Step>>,
}

impl Clock {
    fn new() -> Self {
        Self { origin: Instant::now(), spent: Cpu::process(), steps: Mutex::new(Vec::new()) }
    }

    fn time<R>(&self, name: &'static str, run: impl FnOnce() -> R) -> R {
        let began = Instant::now();
        let (ran, cpu) = metered(run);
        self.record(name, began, Instant::now(), Some(cpu));
        ran
    }

    fn wait<R>(&self, name: &'static str, running: &Detached<(R, Duration)>, deadline: Instant) -> Result<R, Late> {
        let began = Instant::now();
        let answer = running.by(deadline);
        self.record(name, began, Instant::now(), answer.as_ref().ok().map(|(_, cpu)| *cpu));
        answer.map(|(ran, _)| ran)
    }

    fn record(&self, name: &'static str, began: Instant, ended: Instant, cpu: Option<Duration>) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(Step { name, began: began.saturating_duration_since(self.origin), took: ended.saturating_duration_since(began), cpu });
        }
    }

    fn stop(self, ticked: &mut Ticked) {
        ticked.wall = self.origin.elapsed();
        ticked.cpu = self.spent.spent();
        let mut steps = self.steps.into_inner().unwrap_or_else(PoisonError::into_inner);
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

    fn put_off(&mut self, stage: &str, late: &Late, budget: Duration) {
        self.put_off.push(format!("deferred {stage} to the next tick: {}", late.why(budget)));
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
        delivered
            .chain(failed)
            .chain(held)
            .chain(unpulled)
            .chain(undropped)
            .chain(refused)
            .chain(left)
            .chain(unregistered)
            .chain(uncached)
            .chain(undelivered)
            .chain(self.probed.iter().cloned())
            .chain(self.put_off.iter().cloned())
            .chain(unsettled)
            .chain([counts])
            .collect()
    }

    pub fn timing(&self) -> String {
        let wall = self.wall.as_secs_f64().max(f64::EPSILON);
        let steps: Vec<String> = self
            .steps
            .iter()
            .map(|step| {
                let cpu = step.cpu.map_or_else(String::new, |cpu| format!(" {}cpu-ms", cpu.as_millis()));
                format!("{} {}ms{cpu} {:.0}%", step.name, step.took.as_millis(), 100.0 * step.took.as_secs_f64() / wall)
            })
            .collect();
        format!("tick took {}ms {}cpu-ms: {}", self.wall.as_millis(), self.cpu.as_millis(), steps.join(", "))
    }
}

pub fn tick(fleet: &Fleet, ledger: &OpenLedger, pulls: &Pulls, probes: &dyn WaitProbes, now_ms: u64, budgets: Budgets) -> NorthResult<Ticked> {
    let clock = Clock::new();
    let mut ticked = Ticked::default();
    let placing = probes.placement().map(|place| {
        let began = Instant::now();
        (began, detach("placement", move || {
            let ((), cpu) = metered(place);
            (Instant::now(), cpu)
        }))
    });
    let late = exchange_and_hand_in(fleet, ledger, pulls, &clock, &mut ticked, (now_ms, budgets))?;
    if let Some((began, placing)) = placing {
        match placing.by(began + budgets.placement) {
            Ok((ended, cpu)) => clock.record("placement", began, ended, Some(cpu)),
            Err(late) => {
                clock.record("placement", began, Instant::now(), None);
                ticked.put_off("placement", &late, budgets.placement);
            }
        }
    }
    probe_and_prune(fleet, ledger, probes, clock, ticked, budgets, |clock, ticked| finish_late_exchange(late, (fleet, ledger, pulls), clock, ticked, budgets))
}

pub fn probe_pass(fleet: &Fleet, ledger: &OpenLedger, probes: &dyn WaitProbes, budgets: Budgets) -> NorthResult<Ticked> {
    probe_and_prune(fleet, ledger, probes, Clock::new(), Ticked::default(), budgets, |_, _| Ok(()))
}

fn probe_and_prune(fleet: &Fleet, ledger: &OpenLedger, probes: &dyn WaitProbes, clock: Clock, mut ticked: Ticked, budgets: Budgets, finish: impl FnOnce(&Clock, &mut Ticked) -> NorthResult<()>) -> NorthResult<Ticked> {
    settle(fleet, ledger, probes, &clock, &mut ticked, budgets)?;
    finish(&clock, &mut ticked)?;
    if let Err(error) = clock.time("watchdogs", || fleet.start_watchdogs()) {
        eprintln!("north fleet: committed watchdog starts were not delivered: {}", error.user_message());
    }
    ticked.pruned = clock.time("prune", || fleet.prune_delivered(SystemTime::now() - JOURNAL_KEPT))?;
    clock.stop(&mut ticked);
    Ok(ticked)
}

struct Exchanging {
    running: Detached<(NorthResult<Exchanged>, Duration)>,
    began: Instant,
    spooled: Vec<String>,
    covered: Vec<String>,
    pulled: usize,
    taken: Vec<Taken>,
    now_ms: u64,
}

impl Exchanging {
    fn hand_in(self, exchanged: Exchanged, (fleet, ledger, pulls): (&Fleet, &OpenLedger, &Pulls), clock: &Clock, ticked: &mut Ticked, budgets: Budgets) -> NorthResult<()> {
        let Self { spooled, covered, pulled, taken, now_ms, .. } = self;
        let spool = fleet.spool();
        clock.time("spool.withdraw", || spooled.iter().try_for_each(|id| spool.withdraw(id)))?;
        let table = FleetTable { sessions: exchanged.sessions.clone(), gates: exchanged.gates.clone() };
        clock.time("cache_table", || fleet.cache_table(&table))?;
        clock.time("cache_leases", || fleet.cache_leases(&LeaseTable::read(&exchanged.waits, covered, &exchanged.now, now_ms)))?;
        ticked.flushed = spooled.len();
        ticked.pulled = pulled;
        ticked.held_elsewhere = exchanged.held_elsewhere.clone();
        let here: Vec<FleetRow> = exchanged.rows.iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).cloned().collect();
        ticked.note(clock.time("hand_in_all", || hand_in_all(fleet, &here, Instant::now(), budgets.hand_in)));
        let far = (Instant::now() + budgets.far, budgets.far);
        clock.time("drop_far", || drop_far(pulls, &taken, ticked, far));
        clock.time("cache_far", || cache_far(pulls, &taken, &table, ticked, far));
        clock.time("deliver_far", || deliver_far(ledger, pulls, &taken, exchanged.sessions, ticked, (far, now_ms)));
        Ok(())
    }
}

fn exchange_and_hand_in(fleet: &Fleet, ledger: &OpenLedger, pulls: &Pulls, clock: &Clock, ticked: &mut Ticked, (now_ms, budgets): (u64, Budgets)) -> NorthResult<Option<Exchanging>> {
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
        let taken = clock.time("take_far", || take_far(fleet, &records, cached, pulls, &mut rows, ticked, (now_ms, budgets.take_far)));
        (taken, joined(building))
    });
    let pulled = rows.len() - spooled.len();
    let FarRegistry { sessions, addressees, gates, .. } = registered?;
    let outranks = far_hosts(fleet, &records, pulls).into_iter().map(|host| host.name).collect();
    let covered: Vec<String> = addressees.iter().map(|addressee| addressee.name.clone()).collect();
    let exchange = Exchange { rows, host: fleet.host().to_owned(), sessions, addressees, gates, outranks };
    let began = Instant::now();
    let running = {
        let ledger = ledger.clone();
        detach_metered("ledger.exchange", move || ledger()?.exchange(&exchange))
    };
    let exchanging = Exchanging { running, began, spooled, covered, pulled, taken, now_ms };
    match clock.wait("ledger.exchange", &exchanging.running, began + budgets.ledger) {
        Ok(exchanged) => exchanging.hand_in(exchanged?, (fleet, ledger, pulls), clock, ticked, budgets).map(|()| None),
        Err(_) => Ok(Some(exchanging)),
    }
}

fn finish_late_exchange(late: Option<Exchanging>, (fleet, ledger, pulls): (&Fleet, &OpenLedger, &Pulls), clock: &Clock, ticked: &mut Ticked, budgets: Budgets) -> NorthResult<()> {
    let Some(exchanging) = late else { return Ok(()) };
    match clock.wait("ledger.exchange.late", &exchanging.running, exchanging.began + budgets.ledger * 2) {
        Ok(exchanged) => exchanging.hand_in(exchanged?, (fleet, ledger, pulls), clock, ticked, budgets),
        Err(late) => {
            ticked.put_off("ledger.exchange", &late, budgets.ledger);
            Ok(())
        }
    }
}

fn settle(fleet: &Fleet, ledger: &OpenLedger, probes: &dyn WaitProbes, clock: &Clock, ticked: &mut Ticked, budgets: Budgets) -> NorthResult<()> {
    let probed = probe(fleet, probes.jobs(), clock, budgets.probes);
    ticked.probed = probed.lines;
    let settled: Vec<WaitKey> = fleet.unsettled_waits().into_iter().chain(probed.settled).collect();
    if settled.is_empty() {
        return Ok(());
    }
    let settling = {
        let (ledger, settled) = (ledger.clone(), settled.clone());
        detach_metered("settle", move || ledger()?.settle(&settled))
    };
    let left = match clock.wait("settle", &settling, Instant::now() + budgets.ledger) {
        Ok(Ok(())) => Vec::new(),
        Ok(Err(error)) => {
            ticked.unsettled = Some(error.user_message());
            settled
        }
        Err(late) => {
            ticked.put_off("settle", &late, budgets.ledger);
            settled
        }
    };
    fleet.set_unsettled_waits(&left)
}

fn probe(fleet: &Fleet, jobs: Vec<ProbeJob>, clock: &Clock, budget: Duration) -> Probed {
    let started = Instant::now();
    let deadline = started + budget;
    let (own, shared): (Vec<ProbeJob>, Vec<ProbeJob>) = jobs.into_iter().partition(|job| job.own.is_some());
    let beside: Vec<_> = own
        .into_iter()
        .map(|ProbeJob { name, own, run }| {
            let fleet = fleet.clone();
            let by = own.map_or(deadline, |own| (started + own).min(deadline));
            (name, by, detach(name, move || {
                let (probed, cpu) = metered(|| run(fleet));
                (probed, Instant::now(), cpu)
            }))
        })
        .collect();
    let mut probed = Probed { lines: Vec::new(), settled: Vec::new() };
    for ProbeJob { name, run, .. } in shared {
        if Instant::now() >= deadline {
            probed.lines.push(format!("deferred {name} to the next tick: probes spent their {}ms budget before it began", budget.as_millis()));
            continue;
        }
        let fleet = fleet.clone();
        let running = detach_metered(name, move || run(fleet));
        let answer = clock.wait(name, &running, deadline);
        probed.hear(name, answer, || format!("still running when probes spent their {}ms budget", budget.as_millis()));
    }
    for (name, by, running) in beside {
        let answer = running.by(by).map(|(probed, ended, cpu)| {
            clock.record(name, started, ended, Some(cpu));
            probed
        });
        if answer.is_err() {
            clock.record(name, started, Instant::now(), None);
        }
        probed.hear(name, answer, || format!("still running when it spent its own {}ms budget", by.saturating_duration_since(started).as_millis()));
    }
    probed
}

impl Probed {
    fn hear(&mut self, name: &str, answer: Result<Probed, Late>, running: impl FnOnce() -> String) {
        match answer {
            Ok(Probed { lines, settled }) => {
                self.lines.extend(lines);
                self.settled.extend(settled);
            }
            Err(Late::Running) => self.lines.push(format!("deferred {name} to the next tick: {}", running())),
            Err(Late::Panicked) => self.lines.push(format!("{name} failed: it panicked")),
            Err(Late::Unstarted(error)) => self.lines.push(format!("deferred {name} to the next tick: its thread did not start: {error}")),
        }
    }
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
