#![cfg(test)]

use super::break_glass::{Grant, Refusal, World, invoke};
use std::cell::RefCell;

const NOW: u64 = 1_800_000_000_000;

struct Fake {
    launches: Vec<bool>,
    probe: Option<i32>,
    recorded: RefCell<Vec<Grant>>,
    boxes: RefCell<Vec<(String, String)>>,
}

impl Fake {
    fn new(launches: &[bool], probe: Option<i32>) -> Self {
        Self { launches: launches.to_vec(), probe, recorded: RefCell::default(), boxes: RefCell::default() }
    }
}

impl World for Fake {
    fn now_ms(&self) -> u64 {
        NOW
    }
    fn who(&self) -> String {
        "proxy".into()
    }
    fn recent_launches(&self, count: usize) -> Vec<bool> {
        self.launches[self.launches.len().saturating_sub(count)..].to_vec()
    }
    fn probe_exit(&self) -> Option<i32> {
        self.probe
    }
    fn grants(&self) -> Vec<Grant> {
        self.recorded.borrow().clone()
    }
    fn record(&self, grant: &Grant) -> Result<(), String> {
        self.recorded.borrow_mut().push(grant.clone());
        Ok(())
    }
    fn open_box(&self, issue: &str, text: &str) -> Result<(), String> {
        self.boxes.borrow_mut().push((issue.to_owned(), text.to_owned()));
        Ok(())
    }
}

#[test]
fn break_glass_is_refused_while_a_worker_can_launch() {
    for world in [Fake::new(&[false, true], Some(0)), Fake::new(&[true, false], None), Fake::new(&[false], Some(0)), Fake::new(&[], Some(0))] {
        let refused = invoke(&world, "load trace", "north#1106");
        assert!(matches!(refused, Err(Refusal::WorkersCanLaunch(_))), "{refused:?}");
        assert!(world.recorded.borrow().is_empty() && world.boxes.borrow().is_empty());
    }
    assert!(matches!(invoke(&Fake::new(&[false, false], Some(0)), "  ", "north#1106"), Err(Refusal::NoReason)));
}
