#![cfg(test)]

use std::ops::ControlFlow;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use super::probe::{Breaker, Calm, Seat, watch};
use super::report::resume_stopped;
use super::{CALM_MS, Sample};

struct Held(Receiver<()>);

impl Seat for Held {
    fn sync(&mut self) -> std::io::Result<()> {
        let _ = self.0.recv();
        Ok(())
    }
}

struct Trips(Sender<u64>);

impl Breaker for Trips {
    fn trip(&mut self, stall_since_ms: u64) {
        let _ = self.0.send(stall_since_ms);
    }

    fn sample(&mut self, _: Sample) -> ControlFlow<()> {
        ControlFlow::Break(())
    }
}

#[test]
fn a_sync_still_out_past_the_trip_time_trips_the_breaker_before_it_returns_and_the_units_it_stopped_start_after_a_calm_spell() {
    let (release, held) = mpsc::channel();
    let (tripped, trips) = mpsc::channel();
    let probe = thread::spawn(move || watch(Held(held), &mut Trips(tripped), Duration::from_millis(50), Duration::from_millis(10)));
    assert!(trips.recv_timeout(Duration::from_secs(10)).is_ok(), "a sync held past the 50 ms trip trips the breaker while it is still held (north#1553: a 10 s pressure average let 83 stalls of 6 s or more through in an hour)");
    release.send(()).unwrap();
    probe.join().unwrap();
    assert!(trips.try_recv().is_err(), "the sync that tripped trips no second time when it returns");
    let stopped = vec!["north-land-adopt.service".to_owned(), "north-land@north.service".to_owned()];
    let mut ran = Vec::new();
    let started = resume_stopped(&stopped, |unit| unit != "north-land-adopt.service", |kept| {
        ran = kept.to_vec();
        Ok(())
    }).unwrap();
    assert_eq!(started, vec!["north-land@north.service".to_owned()]);
    assert_eq!(ran, started);
    let fast = |at_ms| Sample { at_ms, sync_ms: 5, ..Sample::default() };
    let mut calm = Calm::default();
    calm.stopped(&stopped, 1_000);
    calm.stopped(&stopped[1..], 1_500);
    assert_eq!(calm.after(&fast(1_500 + CALM_MS - 1)), None, "the stopped units wait out the calm spell");
    assert_eq!(calm.after(&Sample { at_ms: 1_000_000, sync_ms: 1_200, ..Sample::default() }), None, "a slow synced write restarts the calm spell");
    assert_eq!(calm.after(&fast(1_001_200 + CALM_MS - 1)), None);
    assert_eq!(calm.after(&fast(1_001_200 + CALM_MS)), Some(stopped.clone()), "the breaker starts each unit it stopped once, after the calm spell, with no operator (2026-11-18: the landers stayed stopped 6.8 h past the last stall)");
    assert_eq!(calm.after(&fast(1_001_200 + 2 * CALM_MS)), None, "units already started are not started again");
}
