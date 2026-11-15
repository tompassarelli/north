#![cfg(test)]

use super::break_glass::{Grant, Live, Refusal, World, active, digest_section, invoke, minutes};
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

#[test]
fn break_glass_expires_and_is_in_the_digest() {
    let failed_launches = Fake::new(&[true, false, false], Some(0));
    let grant = invoke(&failed_launches, "workers cannot start", "north#1106").unwrap();
    assert_eq!(grant.until_ms - grant.at_ms, minutes() * 60_000);
    assert_eq!(minutes(), 30);
    assert_eq!(failed_launches.boxes.borrow().len(), 1);
    assert!(failed_launches.boxes.borrow()[0].1.starts_with("- [ ] ") && failed_launches.boxes.borrow()[0].1.contains("Done when:"));

    let failed_probe = Fake::new(&[true, true], Some(1));
    assert_eq!(invoke(&failed_probe, "probe fails", "north#1106").unwrap().evidence, "the launch probe exited 1");

    let grants = failed_launches.grants();
    assert!(active(&grants, NOW + 29 * 60_000).is_some());
    assert!(active(&grants, NOW + 30 * 60_000).is_none());

    let expired = digest_section(&grants, NOW - 1, NOW + 31 * 60_000);
    for part in ["proxy", "workers cannot start", "the last 2 launches failed", "north#1106", "expired at"] {
        assert!(expired.contains(part), "{part} missing from {expired}");
    }
    assert!(digest_section(&grants, NOW + 1, NOW + 31 * 60_000).is_empty());
    assert!(digest_section(&grants, NOW - 1, NOW + 60_000).contains("lifted until"));

    let state = tempfile::tempdir().unwrap();
    let live = Live::in_state(state.path().to_path_buf());
    live.record(&grant).unwrap();
    assert_eq!(live.grants(), grants);
}
