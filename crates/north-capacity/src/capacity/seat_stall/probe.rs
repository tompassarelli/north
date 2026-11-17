//! `north capacity seat-probe`: every 2 s writes and syncs a small file on the owner's filesystem and logs how long the sync took. A watcher apart from the syncing thread trips the breaker once a sync has been out for the trip time, without waiting for it to return, and what the breaker acts with (the unit patterns) is read into memory at start. The log is written by a third thread, so a stalled append holds up neither.

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

use super::{EVENTS, Event, INTERVAL_MS, SAMPLES, SLOW_MS, Sample, append, dir, now_ms, trip_ms, unit_patterns};

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

struct Fleet {
    patterns: Vec<String>,
    trip_ms: u64,
    records: Sender<Record>,
}

impl Breaker for Fleet {
    fn trip(&mut self, stall_since_ms: u64) {
        let units = running(&self.patterns);
        if !units.is_empty() {
            stop(&units);
        }
        let _ = self.records.send(Record::Event(Event::Stop { at_ms: now_ms(), stall_since_ms, units: units.clone() }));
        if !units.is_empty() {
            let since = Timestamp::from_unix_millis(i64::try_from(stall_since_ms).unwrap_or(i64::MAX)).iso();
            page(&format!("seat-probe: a synced write on the owner's laptop has been out over {:.0} s since {since}, so the breaker stopped {} fleet units: {}. Start them with `north capacity seat-stall resume` once the stall ends.", self.trip_ms as f64 / 1000.0, units.len(), units.join(", ")));
        }
    }

    fn sample(&mut self, mut sample: Sample) -> ControlFlow<()> {
        if sample.sync_ms > SLOW_MS {
            sample.units = running(&self.patterns);
        }
        let _ = self.records.send(Record::Sample(sample));
        ControlFlow::Continue(())
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
    let (records, written) = mpsc::channel();
    let log_dir = dir.clone();
    thread::spawn(move || write_records(&log_dir, &written));
    let trip_ms = trip_ms();
    let mut fleet = Fleet { patterns, trip_ms, records };
    watch(FileSeat(dir.join(PROBE_FILE)), &mut fleet, Duration::from_millis(trip_ms), Duration::from_millis(INTERVAL_MS));
    eprintln!("north capacity seat-probe: the syncing thread ended");
    1
}
