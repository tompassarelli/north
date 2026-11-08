use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Liveness, Owner, Store};

use super::{CheckEnvironment, Environment, active_in, credential};

struct Alive;

impl Liveness for Alive {
    fn alive(&self, _: &Owner) -> Option<bool> {
        Some(true)
    }
}

fn store(path: &std::path::Path) -> Store {
    Store::open_as(path, Owner { host: "whiterabbit".into(), pid: 10, started: 1, booted_ms: 1 }, Box::new(Alive)).unwrap()
}

#[test]
fn the_placement_is_tried_again_once_the_fallback_is_old() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = store(&directory.path().join("ledger.db"));
    let then = Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 31 * 60 * 1000);
    store.transition::<CheckEnvironment>("lander", Environment::Local, "main fails in placed", then).unwrap().unwrap();
    assert_eq!(active_in(&store, "lander", Timestamp::now()), Environment::Placed);
}

#[test]
fn a_red_log_showing_a_refused_credential_or_a_failed_sibling_fetch_is_an_infrastructure_incident_and_a_test_failure_is_not() {
    let directory = tempfile::tempdir().unwrap();
    let log = directory.path().join("post-land.log");
    std::fs::write(&log, "check-owners --south-root /home/tom/code/south/main\nfatal: credential helper '/home/tom/.local/bin/north git-credential' told us to quit\nError: cannot fetch /home/tom/code/south/main origin/main; refusing landing\n").unwrap();
    let verdict = credential::incident(&log).unwrap();
    assert!(verdict.contains("told us to quit") && verdict.ends_with("nothing is reverted"), "{verdict}");
    std::fs::write(&log, "Error: cannot fetch /home/tom/code/south/main origin/main; refusing landing\n").unwrap();
    assert!(credential::incident(&log).is_some_and(|verdict| verdict.contains("cannot fetch /home/tom/code/south/main origin/main")));
    std::fs::write(&log, "test suite::x ... FAILED\n").unwrap();
    assert_eq!(credential::incident(&log), None);
    assert_eq!(credential::incident(&directory.path().join("missing.log")), None);
}
