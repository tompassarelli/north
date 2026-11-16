#![cfg(test)]

use std::ops::ControlFlow;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use super::Sample;
use super::probe::{Breaker, Seat, watch};

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
fn a_sync_still_out_past_the_trip_time_trips_the_breaker_before_it_returns() {
    let (release, held) = mpsc::channel();
    let (tripped, trips) = mpsc::channel();
    let probe = thread::spawn(move || watch(Held(held), &mut Trips(tripped), Duration::from_millis(50), Duration::from_millis(10)));
    assert!(trips.recv_timeout(Duration::from_secs(10)).is_ok(), "a sync held past the 50 ms trip trips the breaker while it is still held (north#1553: a 10 s pressure average let 83 stalls of 6 s or more through in an hour)");
    release.send(()).unwrap();
    probe.join().unwrap();
    assert!(trips.try_recv().is_err(), "the sync that tripped trips no second time when it returns");
}
