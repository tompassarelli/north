#![cfg(test)]

use super::break_glass::{Grant, Live, Refusal, World, active, digest_section, invoke, lifted, minutes};
use north_fleet_store::landing_stall::{Episode, State};
use std::cell::RefCell;

const NOW: u64 = 1_800_000_000_000;

struct Fake {
    launches: Vec<bool>,
    probe: Option<i32>,
    alarm: bool,
    booted: Option<u64>,
    recorded: RefCell<Vec<Grant>>,
    boxes: RefCell<Vec<(String, String)>>,
    landing: State,
}

impl Fake {
    fn new(launches: &[bool], probe: Option<i32>) -> Self {
        Self { launches: launches.to_vec(), probe, alarm: false, booted: None, recorded: RefCell::default(), boxes: RefCell::default(), landing: State::new() }
    }

    fn alarm(self) -> Self {
        Self { alarm: true, ..self }
    }

    fn booted(self, at: u64) -> Self {
        Self { booted: Some(at), ..self }
    }

    fn stalled(mut self, repo: &str, opened_ms: u64) -> Self {
        self.landing.insert(repo.to_owned(), Episode { opened_ms, ..Episode::default() });
        self
    }
}

impl World for Fake {
    fn now_ms(&self) -> u64 {
        NOW
    }
    fn who(&self) -> String {
        "coordinator".into()
    }
    fn recent_launches(&self, count: usize) -> Vec<bool> {
        self.launches[self.launches.len().saturating_sub(count)..].to_vec()
    }
    fn probe_exit(&self) -> Option<i32> {
        self.probe
    }
    fn load_alarm(&self) -> bool {
        self.alarm
    }
    fn booted_ms(&self) -> Option<u64> {
        self.booted
    }
    fn landing_stalls(&self) -> State {
        self.landing.clone()
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
fn break_glass_opens_on_incident_lifts_then_expires_and_is_in_the_digest() {
    let launching = || Fake::new(&[true, true], Some(0));
    for (world, evidence) in [(launching().booted(NOW - 10 * 60_000), "the coordinator host rebooted 10 minutes ago"), (launching().booted(NOW - 29 * 60_000), "the coordinator host rebooted 29 minutes ago"), (launching().alarm(), "the laptop load alarm is firing")] {
        let grant = invoke(&world, "an incident", Some("north#1626")).expect("an incident opens break glass");
        assert_eq!(grant.evidence, evidence);
        assert_eq!(grant.until_ms - grant.at_ms, 30 * 60_000);
        assert_eq!(world.boxes.borrow().len(), 1);
        assert!(world.boxes.borrow()[0].1.starts_with("- [ ] ") && world.boxes.borrow()[0].1.contains("Done when:"));
    }
    for world in [launching().booted(NOW - 31 * 60_000), launching()] {
        let refused = invoke(&world, "an incident", Some("north#1626"));
        assert!(matches!(refused, Err(Refusal::NoIncident(_))), "{refused:?}");
        assert!(world.recorded.borrow().is_empty() && world.boxes.borrow().is_empty());
    }
    for world in [Fake::new(&[false, true], Some(0)), Fake::new(&[true, false], None), Fake::new(&[false], Some(0)), Fake::new(&[], Some(0))] {
        let refused = invoke(&world, "load trace", Some("north#1106"));
        assert!(matches!(refused, Err(Refusal::NoIncident(_))), "{refused:?}");
        assert!(world.recorded.borrow().is_empty() && world.boxes.borrow().is_empty());
    }
    assert!(matches!(invoke(&Fake::new(&[false, false], Some(0)), "  ", Some("north#1106")), Err(Refusal::NoReason)));
    assert!(matches!(invoke(&Fake::new(&[false, false], Some(0)), "workers cannot start", None), Err(Refusal::NoIssue(_))));

    let failed_launches = Fake::new(&[true, false, false], Some(0));
    let grant = invoke(&failed_launches, "workers cannot start", Some("north#1106")).unwrap();
    assert_eq!(grant.until_ms - grant.at_ms, minutes() * 60_000);
    assert_eq!(minutes(), 30);
    assert_eq!(failed_launches.boxes.borrow().len(), 1);
    assert!(failed_launches.boxes.borrow()[0].1.starts_with("- [ ] ") && failed_launches.boxes.borrow()[0].1.contains("Done when:"));

    let failed_probe = Fake::new(&[true, true], Some(1));
    assert_eq!(invoke(&failed_probe, "probe fails", Some("north#1106")).unwrap().evidence, "the launch probe exited 1");

    let grants = failed_launches.grants();
    assert!(active(&grants, NOW + 29 * 60_000).is_some());
    assert!(active(&grants, NOW + 30 * 60_000).is_none());

    let expired = digest_section(&grants, NOW - 1, NOW + 31 * 60_000);
    for part in ["coordinator", "workers cannot start", "the last 2 launches failed", "north#1106", "expired at"] {
        assert!(expired.contains(part), "{part} missing from {expired}");
    }
    assert!(digest_section(&grants, NOW + 1, NOW + 31 * 60_000).is_empty());
    assert!(digest_section(&grants, NOW - 1, NOW + 60_000).contains("lifted until"));

    let state = tempfile::tempdir().unwrap();
    let live = Live::in_state(state.path().to_path_buf());
    live.record(&grant).unwrap();
    assert_eq!(live.grants(), grants);
}

#[test]
fn a_landing_stall_grant_ends_after_its_minutes() {
    let stalled = Fake::new(&[true, true], Some(0)).stalled("north", NOW - 60_000);
    let landing_grant = invoke(&stalled, "landing outage", None).unwrap();
    assert!(stalled.boxes.borrow().is_empty(), "a landing stall already has its own issue");
    assert_eq!(landing_grant.landing.as_ref().map(|landing| landing.repo.as_str()), Some("north"));
    assert_eq!(landing_grant.until_ms - landing_grant.at_ms, minutes() * 60_000);

    let stall_grants = stalled.grants();
    assert!(lifted(&stall_grants, &stalled.landing, NOW));
    assert!(lifted(&stall_grants, &stalled.landing, NOW + 29 * 60_000));
    assert!(!lifted(&stall_grants, &stalled.landing, NOW + 30 * 60_000), "the lift ends after the configured minutes even while the stall record stays fresh");
    assert!(!lifted(&stall_grants, &State::new(), NOW), "a cleared stall lifts nothing");
    let reopened: State = [("north".to_owned(), Episode { opened_ms: NOW, ..Episode::default() })].into_iter().collect();
    assert!(!lifted(&stall_grants, &reopened, NOW), "a new episode of the stall is not the one granted");

    let stall_digest = digest_section(&stall_grants, NOW - 1, NOW + 31 * 60_000);
    assert!(stall_digest.contains("expired at") && stall_digest.contains("the landing-is-stalled issue of north"), "{stall_digest}");
    assert!(digest_section(&stall_grants, NOW - 1, NOW + 60_000).contains("the landing stall of north stays fresh"));

    let other_repo = Fake::new(&[true, true], Some(0)).stalled("wisp", NOW - 60_000);
    assert!(matches!(invoke(&other_repo, "landing outage", None), Err(Refusal::NoIncident(_))), "a stall in another repository lifts nothing");
}
