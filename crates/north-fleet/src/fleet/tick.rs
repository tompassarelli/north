use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use crate::error::NorthResult;

use super::model::SessionRecord;
use super::peer_row::{self, Refusal};
use super::registry::{Fleet, HandedIn};
use super::stage::{Late, detach, each_by};
use super::store::{Exchange, Exchanged, FarRegistry, FarSpools, FleetRow, FleetTable, Handed, HandedRow, LAUNCH_GRACE_MS, LeaseTable, LedgerHost, RowKind, SessionRow, Undecodable, UnknownKind, WaitKey};

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
}

impl Budgets {
    pub const TICK: Self = Self {
        placement: Duration::from_secs(8),
        take_far: Duration::from_secs(4),
        ledger: Duration::from_secs(6),
        hand_in: Duration::from_secs(4),
        far: Duration::from_secs(4),
        probes: Duration::from_secs(14),
    };

    pub const fn each(budget: Duration) -> Self {
        Self { placement: budget, take_far: budget, ledger: budget, hand_in: budget, far: budget, probes: budget }
    }

    pub fn worst(&self) -> Duration {
        self.placement.max(self.take_far + self.ledger) + self.hand_in + self.far + self.probes + self.ledger
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
    steps: Mutex<Vec<Step>>,
}

impl Clock {
    fn new() -> Self {
        Self { origin: Instant::now(), steps: Mutex::new(Vec::new()) }
    }

    fn time<R>(&self, name: &'static str, run: impl FnOnce() -> R) -> R {
        let began = Instant::now();
        let ran = run();
        self.record(name, began, Instant::now());
        ran
    }

    fn record(&self, name: &'static str, began: Instant, ended: Instant) {
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(Step { name, began: began.saturating_duration_since(self.origin), took: ended.saturating_duration_since(began) });
        }
    }

    fn stop(self, ticked: &mut Ticked) {
        ticked.wall = self.origin.elapsed();
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
        let steps: Vec<String> = self.steps.iter().map(|step| format!("{} {}ms {:.0}%", step.name, step.took.as_millis(), 100.0 * step.took.as_secs_f64() / wall)).collect();
        format!("tick took {}ms: {}", self.wall.as_millis(), steps.join(", "))
    }
}

pub fn tick(fleet: &Fleet, ledger: &OpenLedger, pulls: &Pulls, probes: &dyn WaitProbes, now_ms: u64, budgets: Budgets) -> NorthResult<Ticked> {
    let clock = Clock::new();
    let mut ticked = Ticked::default();
    let placing = probes.placement().map(|place| {
        let began = Instant::now();
        (began, detach("placement", move || {
            place();
            Instant::now()
        }))
    });
    exchange_and_hand_in(fleet, ledger, pulls, &clock, &mut ticked, (now_ms, budgets))?;
    if let Some((began, placing)) = placing {
        match placing.by(began + budgets.placement) {
            Ok(ended) => clock.record("placement", began, ended),
            Err(late) => {
                clock.record("placement", began, Instant::now());
                ticked.put_off("placement", &late, budgets.placement);
            }
        }
    }
    settle(fleet, ledger, probes, &clock, &mut ticked, budgets)?;
    ticked.pruned = clock.time("prune", || fleet.prune_delivered(SystemTime::now() - JOURNAL_KEPT))?;
    clock.stop(&mut ticked);
    Ok(ticked)
}

fn exchange_by(ledger: &OpenLedger, exchange: Exchange, deadline: Instant) -> Result<NorthResult<Exchanged>, Late> {
    let ledger = ledger.clone();
    detach("ledger.exchange", move || ledger()?.exchange(&exchange)).by(deadline)
}

fn exchange_and_hand_in(fleet: &Fleet, ledger: &OpenLedger, pulls: &Pulls, clock: &Clock, ticked: &mut Ticked, (now_ms, budgets): (u64, Budgets)) -> NorthResult<()> {
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
    let exchanged = match clock.time("ledger.exchange", || exchange_by(ledger, exchange, Instant::now() + budgets.ledger)) {
        Ok(exchanged) => exchanged?,
        Err(late) => {
            ticked.put_off("ledger.exchange", &late, budgets.ledger);
            return Ok(());
        }
    };
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

fn settle(fleet: &Fleet, ledger: &OpenLedger, probes: &dyn WaitProbes, clock: &Clock, ticked: &mut Ticked, budgets: Budgets) -> NorthResult<()> {
    let probed = probe(fleet, probes.jobs(), clock, budgets.probes);
    ticked.probed = probed.lines;
    let settled: Vec<WaitKey> = fleet.unsettled_waits().into_iter().chain(probed.settled).collect();
    if settled.is_empty() {
        return Ok(());
    }
    let settling = {
        let (ledger, settled) = (ledger.clone(), settled.clone());
        detach("settle", move || ledger()?.settle(&settled))
    };
    let left = match clock.time("settle", || settling.by(Instant::now() + budgets.ledger)) {
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
            (name, by, detach(name, move || (run(fleet), Instant::now())))
        })
        .collect();
    let mut probed = Probed { lines: Vec::new(), settled: Vec::new() };
    for ProbeJob { name, run, .. } in shared {
        if Instant::now() >= deadline {
            probed.lines.push(format!("deferred {name} to the next tick: probes spent their {}ms budget before it began", budget.as_millis()));
            continue;
        }
        let fleet = fleet.clone();
        let running = detach(name, move || run(fleet));
        let answer = clock.time(name, || running.by(deadline));
        probed.hear(name, answer, || format!("still running when probes spent their {}ms budget", budget.as_millis()));
    }
    for (name, by, running) in beside {
        let answer = running.by(by).map(|(probed, ended)| {
            clock.record(name, started, ended);
            probed
        });
        if answer.is_err() {
            clock.record(name, started, Instant::now());
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

fn deliver_far(ledger: &OpenLedger, pulls: &Pulls, taken: &[Taken], known: Vec<SessionRow>, ticked: &mut Ticked, ((deadline, budget), now_ms): ((Instant, Duration), u64)) {
    let known = Arc::new(known);
    let runs = taken.iter().map(|taken| {
        let (ledger, spools, known, host) = (ledger.clone(), pulls.spools.clone(), known.clone(), taken.host.clone());
        move || -> NorthResult<Vec<HandedRow>> {
            let FarRegistry { host: registered, sessions, addressees, gates } = spools.registry(&host.ssh)?;
            if registered.is_empty() {
                return Ok(Vec::new());
            }
            let admitted = peer_row::admit_exchange(Exchange { rows: Vec::new(), host: registered, sessions, addressees, gates, outranks: Vec::new() }, &host.name, &known, now_ms)?;
            let exchanged = ledger()?.exchange(&admitted)?;
            let rows: Vec<FleetRow> = exchanged.rows.into_iter().filter(|row| !exchanged.held_elsewhere.contains(&row.to)).collect();
            if rows.is_empty() { Ok(Vec::new()) } else { spools.deliver(&host.ssh, &rows) }
        }
    });
    for (taken, delivered) in taken.iter().zip(each_by("deliver_far", runs, deadline)) {
        match delivered {
            Ok(Ok(handed)) => ticked.note(handed),
            Ok(Err(error)) => ticked.far_failed.push((taken.host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("deliver_far on {}", taken.host.name), &late, budget),
        }
    }
}

fn take_far(fleet: &Fleet, records: &[SessionRecord], cached: Vec<SessionRow>, pulls: &Pulls, rows: &mut Vec<FleetRow>, ticked: &mut Ticked, (now_ms, budget): (u64, Duration)) -> Vec<Taken> {
    let hosts = far_hosts(fleet, records, pulls);
    let runs = hosts.iter().map(|host| {
        let (spools, ssh) = (pulls.spools.clone(), host.ssh.clone());
        move || spools.take(&ssh)
    });
    let answers = each_by("take_far", runs, Instant::now() + budget);
    let known = ledger_records(fleet, records, cached, now_ms);
    let archived: Vec<SessionRecord> = fleet.past_sessions();
    let mut taken = Vec::new();
    for (host, answer) in hosts.into_iter().zip(answers) {
        match answer {
            Ok(Ok(found)) => {
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
            Ok(Err(error)) => ticked.pull_failed.push((host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("take_far on {}", host.name), &late, budget),
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

fn drop_far(pulls: &Pulls, taken: &[Taken], ticked: &mut Ticked, (deadline, budget): (Instant, Duration)) {
    let held: Vec<&Taken> = taken.iter().filter(|taken| !taken.ids.is_empty() || !taken.refused.is_empty()).collect();
    let runs = held.iter().map(|taken| {
        let (spools, ssh, ids, refused) = (pulls.spools.clone(), taken.host.ssh.clone(), taken.ids.clone(), taken.refused.clone());
        move || {
            let dropped = if ids.is_empty() { Ok(()) } else { FarSpools::drop(&*spools, &ssh, &ids) };
            dropped.and_then(|()| if refused.is_empty() { Ok(()) } else { spools.set_aside(&ssh, &refused) })
        }
    });
    for (taken, answer) in held.iter().zip(each_by("drop_far", runs, deadline)) {
        match answer {
            Ok(Ok(())) => {}
            Ok(Err(error)) => ticked.drop_failed.push((taken.host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("drop_far on {}", taken.host.name), &late, budget),
        }
    }
}

fn cache_far(pulls: &Pulls, taken: &[Taken], table: &FleetTable, ticked: &mut Ticked, (deadline, budget): (Instant, Duration)) {
    let table = Arc::new(table.clone());
    let runs = taken.iter().map(|taken| {
        let (spools, ssh, table) = (pulls.spools.clone(), taken.host.ssh.clone(), table.clone());
        move || spools.cache(&ssh, &table)
    });
    for (taken, answer) in taken.iter().zip(each_by("cache_far", runs, deadline)) {
        match answer {
            Ok(Ok(())) => {}
            Ok(Err(error)) => ticked.uncached.push((taken.host.name.clone(), error.user_message())),
            Err(late) => ticked.put_off(&format!("cache_far on {}", taken.host.name), &late, budget),
        }
    }
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
