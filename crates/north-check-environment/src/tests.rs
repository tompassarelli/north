use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{Liveness, Owner, State, Store};

use super::{CheckEnvironment, Environment, Reading, active_in, credential, judge_in};

struct Alive;

impl Liveness for Alive {
    fn alive(&self, _: &Owner) -> Option<bool> {
        Some(true)
    }
}

fn store(path: &std::path::Path) -> Store {
    Store::open_as(path, Owner { host: "whiterabbit".into(), pid: 10, started: 1, booted_ms: 1 }, Box::new(Alive)).unwrap()
}

fn broken(environment: Environment) -> (Environment, Reading) {
    (environment, Reading::Broken(format!("a 1 s deadline test failed in {}", environment.name())))
}

// KEEP(bug): on 2026-11-07 a base red only in the build hosts' environment paused every landing, though the laptop environment was green (north#782)
#[test]
fn a_base_broken_in_the_placement_lands_in_this_host_environment_and_new_candidates_stay_there() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = store(&directory.path().join("ledger.db"));
    let mut ran = Vec::new();

    let landed = judge_in(Some(&mut store), "lander", |environment| {
        ran.push(environment);
        if environment.is_local() { (environment, Reading::Reached) } else { broken(environment) }
    });

    assert_eq!((landed, ran), (Environment::Local, vec![Environment::Placed, Environment::Local]));
    assert_eq!(active_in(&store, "lander", Timestamp::now()), Environment::Local);
    let mut next = Vec::new();
    judge_in(Some(&mut store), "lander", |environment| {
        next.push(environment);
        (environment, Reading::Reached)
    });
    assert_eq!(next, vec![Environment::Local]);
}

#[test]
fn a_base_broken_in_every_environment_returns_the_last_reading_to_pause_on_and_keeps_the_active_environment() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = store(&directory.path().join("ledger.db"));

    let paused = judge_in(Some(&mut store), "lander", broken);

    assert_eq!(paused, Environment::Local);
    assert_eq!(active_in(&store, "lander", Timestamp::now()), Environment::Placed);
    assert!(store.current::<CheckEnvironment>("lander").unwrap().is_none());
}

#[test]
fn the_placement_is_tried_again_once_the_fallback_is_old_and_its_verdict_ends_the_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = store(&directory.path().join("ledger.db"));
    let then = Timestamp::from_unix_millis(Timestamp::now().unix_millis() - 31 * 60 * 1000);
    store.transition::<CheckEnvironment>("lander", Environment::Local, "main fails in placed", then).unwrap().unwrap();
    assert_eq!(active_in(&store, "lander", Timestamp::now()), Environment::Placed);

    let mut ran = Vec::new();
    judge_in(Some(&mut store), "lander", |environment| {
        ran.push(environment);
        (environment, Reading::Reached)
    });

    assert_eq!(ran, vec![Environment::Placed]);
    assert_eq!(store.current::<CheckEnvironment>("lander").unwrap().unwrap().state, Environment::Placed);
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
