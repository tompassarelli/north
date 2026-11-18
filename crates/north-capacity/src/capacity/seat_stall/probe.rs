//! `north capacity seat-probe`: every 2 s writes and syncs a small file on the owner's filesystem and logs how long the sync took. A watcher apart from the syncing thread trips the breaker once a sync has been out for the trip time, without waiting for it to return, and what the breaker acts with (the unit patterns) is read into memory at start. The log is written by a third thread, so a stalled append holds up neither. The breaker starts the units it stopped once no synced write has been slow for `CALM_MS`, and the probe exits for its unit to start the new runtime once the live link moves.

use std::fs::OpenOptions;
use std::io::Write;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use north_foundation::foundation::clock::Timestamp;
use north_runtime_follow::Follow;

use super::report::{resume_stopped, start_units, unit_loaded};
use super::{CALM_MS, EVENTS, Event, INTERVAL_MS, Log, SAMPLES, SLOW_MS, Sample, append, dir, now_ms, trip_ms, unit_patterns};

const POLL: Duration = Duration::from_millis(100);
const PROBE_FILE: &str = "probe";

pub trait Seat: Send + 'static {
    fn sync(&mut self) -> std::io::Result<()>;
}

pub trait Breaker {
    fn trip(&mut self, stall_since_ms: u64);
    fn sample(&mut self, sample: Sample) -> ControlFlow<()>;
}

/// Syncs `seat` every `interval` on its own thread and trips `breaker` once for each sync out for `trip` or longer, while it is still out.
pub fn watch(seat: impl Seat, breaker: &mut impl Breaker, trip: Duration, interval: Duration) {
    let out = Arc::new(AtomicU64::new(0));
    let (sender, returns) = mpsc::channel();
    let syncing = Arc::clone(&out);
    thread::spawn(move || sync_every(seat, &syncing, &sender, interval));
    let trip_ms = u64::try_from(trip.as_millis()).unwrap_or(u64::MAX);
    let mut tripped = 0;
    loop {
        match returns.recv_timeout(POLL) {
            Ok(sample) => {
                if sample.sync_ms >= trip_ms && tripped != sample.at_ms {
                    tripped = sample.at_ms;
                    breaker.trip(sample.at_ms);
                }
                if breaker.sample(sample).is_break() {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let since = out.load(Ordering::Acquire);
        if since != 0 && now_ms().saturating_sub(since) >= trip_ms && tripped != since {
            tripped = since;
            breaker.trip(since);
        }
    }
}

fn sync_every(mut seat: impl Seat, out: &AtomicU64, sender: &Sender<Sample>, interval: Duration) {
    loop {
        let started = Instant::now();
        let at_ms = now_ms();
        out.store(at_ms, Ordering::Release);
        let result = seat.sync();
        let took = started.elapsed();
        out.store(0, Ordering::Release);
        let sample = Sample { at_ms, sync_ms: u64::try_from(took.as_millis()).unwrap_or(u64::MAX), units: Vec::new(), error: result.err().map(|error| error.to_string()) };
        if sender.send(sample).is_err() {
            return;
        }
        thread::sleep(interval.saturating_sub(took));
    }
}

struct FileSeat(PathBuf);

impl Seat for FileSeat {
    fn sync(&mut self) -> std::io::Result<()> {
        let mut file = OpenOptions::new().create(true).write(true).truncate(true).open(&self.0)?;
        file.write_all(now_ms().to_string().as_bytes())?;
        file.sync_all()
    }
}

enum Record {
    Sample(Sample),
    Event(Event),
}

/// The units the breaker stopped and the end of the newest slow synced write, so they start again once the seat has been calm for `CALM_MS`.
#[derive(Default)]
pub(super) struct Calm {
    stopped: Vec<String>,
    slow_until_ms: u64,
}

impl Calm {
    pub(super) fn stopped(&mut self, units: &[String], at_ms: u64) {
        for unit in units {
            if !self.stopped.contains(unit) {
                self.stopped.push(unit.clone());
            }
        }
        self.slow_until_ms = self.slow_until_ms.max(at_ms);
    }

    /// The stopped units to start once `sample` ends `CALM_MS` with no slow synced write, None before.
    pub(super) fn after(&mut self, sample: &Sample) -> Option<Vec<String>> {
        if sample.sync_ms > SLOW_MS {
            self.slow_until_ms = self.slow_until_ms.max(sample.at_ms + sample.sync_ms);
        }
        let calm = sample.at_ms.saturating_sub(self.slow_until_ms) >= CALM_MS;
        (calm && !self.stopped.is_empty()).then(|| std::mem::take(&mut self.stopped))
    }
}

struct Fleet {
    patterns: Vec<String>,
    trip_ms: u64,
    records: Sender<Record>,
    calm: Calm,
    runtime: Follow,
    moved: Option<String>,
}

impl Fleet {
    fn resume(&mut self, units: &[String], at_ms: u64) {
        match resume_stopped(units, unit_loaded, start_units) {
            Ok(started) => {
                let _ = self.records.send(Record::Event(Event::Resume { at_ms: now_ms(), units: started.clone() }));
                page(&format!("seat-probe: no synced write on the owner's laptop was over {:.0} s for {} min, so the breaker started the {} units it stopped: {}", SLOW_MS as f64 / 1000.0, CALM_MS / 60_000, started.len(), started.join(", ")));
            }
            Err(error) => {
                page(&format!("seat-probe: starting the units the breaker stopped failed ({error}); it tries again after another {} calm min", CALM_MS / 60_000));
                self.calm.stopped(units, at_ms);
            }
        }
    }
}

impl Breaker for Fleet {
    fn trip(&mut self, stall_since_ms: u64) {
        let units = running(&self.patterns);
        if !units.is_empty() {
            stop(&units);
        }
        self.calm.stopped(&units, now_ms());
        let _ = self.records.send(Record::Event(Event::Stop { at_ms: now_ms(), stall_since_ms, units: units.clone() }));
        if !units.is_empty() {
            let since = Timestamp::from_unix_millis(i64::try_from(stall_since_ms).unwrap_or(i64::MAX)).iso();
            page(&format!("seat-probe: a synced write on the owner's laptop has been out over {:.0} s since {since}, so the breaker stopped {} fleet units: {}. It starts them once no synced write is over {:.0} s for {} min.", self.trip_ms as f64 / 1000.0, units.len(), units.join(", "), SLOW_MS as f64 / 1000.0, CALM_MS / 60_000));
        }
    }

    fn sample(&mut self, mut sample: Sample) -> ControlFlow<()> {
        let slow = sample.sync_ms > SLOW_MS;
        if slow {
            sample.units = running(&self.patterns);
        }
        let due = self.calm.after(&sample);
        let at_ms = sample.at_ms;
        let _ = self.records.send(Record::Sample(sample));
        if let Some(units) = due {
            self.resume(&units, at_ms);
        }
        if slow {
            return ControlFlow::Continue(());
        }
        self.moved = self.runtime.replaced();
        if self.moved.is_some() { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
    }
}

/// The units matching `patterns` that are active or starting.
pub(super) fn running(patterns: &[String]) -> Vec<String> {
    let listed = Command::new("systemctl").args(["--user", "list-units", "--plain", "--no-legend", "--state=active,activating,reloading", "--"]).args(patterns).stdin(Stdio::null()).output();
    match listed {
        Ok(output) => String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.split_whitespace().next().map(str::to_owned)).collect(),
        Err(error) => {
            eprintln!("north capacity seat-probe: systemctl list-units: {error}");
            Vec::new()
        }
    }
}

fn stop(units: &[String]) {
    match Command::new("systemctl").args(["--user", "stop", "--no-block", "--"]).args(units).stdin(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("north capacity seat-probe: systemctl stop exited {status}"),
        Err(error) => eprintln!("north capacity seat-probe: systemctl stop: {error}"),
    }
}

fn page(text: &str) {
    eprintln!("north capacity seat-probe: {text}");
}

fn write_records(dir: &Path, records: &Receiver<Record>) {
    for record in records {
        let (name, line) = match record {
            Record::Sample(sample) => (SAMPLES, serde_json::to_string(&sample)),
            Record::Event(event) => (EVENTS, serde_json::to_string(&event)),
        };
        if let Err(error) = line.map_err(|error| error.to_string()).and_then(|line| append(dir, name, &line)) {
            eprintln!("north capacity seat-probe: {error}");
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north capacity seat-probe");
        return 2;
    }
    let dir = dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("north capacity seat-probe: {}: {error}", dir.display());
        return 1;
    }
    let patterns = unit_patterns();
    if patterns.is_empty() {
        eprintln!("north capacity seat-probe: harness/defaults.toml names no unit in seat_stall_units");
        return 1;
    }
    let mut calm = Calm::default();
    if let Ok(Some(log)) = Log::read(&dir)
        && let Some(Event::Stop { units, .. }) = log.standing_stop()
    {
        calm.stopped(units, now_ms());
    }
    let (records, written) = mpsc::channel();
    let log_dir = dir.clone();
    let writer = thread::spawn(move || write_records(&log_dir, &written));
    let trip_ms = trip_ms();
    let mut fleet = Fleet { patterns, trip_ms, records, calm, runtime: Follow::current(), moved: None };
    watch(FileSeat(dir.join(PROBE_FILE)), &mut fleet, Duration::from_millis(trip_ms), Duration::from_millis(INTERVAL_MS));
    let Some(next) = fleet.moved.take() else {
        eprintln!("north capacity seat-probe: the syncing thread ended");
        return 1;
    };
    drop(fleet);
    let _ = writer.join();
    eprintln!("north capacity seat-probe: the live runtime changed to {next}; exiting so the service restarts on it");
    0
}
