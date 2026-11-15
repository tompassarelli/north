use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Liveness, Owner, Store};

use super::{AfterLandingRun, Run, journal, misses, moved, retire_unadopted, settled, take};

struct Gone(u32);

impl Liveness for Gone {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        Some(owner.pid != self.0)
    }
}

fn lander(path: &std::path::Path, pid: u32, gone: u32) -> Store {
    Store::open_as(path, Owner { host: "gex45".into(), pid, started: 1, booted_ms: 1 }, Box::new(Gone(gone))).unwrap()
}

#[test]
fn a_lander_killed_mid_run_leaves_an_orphaned_run_that_health_reports_and_the_next_lander_reruns_keeping_its_misses_and_an_abandoned_after_landing_run_nobody_adopts_is_retired_and_no_longer_reported_overdue() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let mut first = lander(&path, 10, 0);
    moved::<AfterLandingRun>(&mut first, "abc", Run::Journaled, "landed on base").unwrap();
    moved::<AfterLandingRun>(&mut first, "abc", Run::Running, "started").unwrap();
    moved::<AfterLandingRun>(&mut first, "abc", Run::Unsettled, "no verdict").unwrap();
    moved::<AfterLandingRun>(&mut first, "abc", Run::Running, "started again").unwrap();

    let mut next = lander(&path, 11, 10);
    let faults = next.faults(Timestamp::now()).unwrap();
    assert_eq!(faults.len(), 1);
    assert!(matches!(&faults[0].kind, FaultKind::Orphaned { owner } if owner.pid == 10), "{faults:?}");

    let rerun = moved::<AfterLandingRun>(&mut next, "abc", Run::Journaled, "rerun: a stopped lander left it without a verdict").unwrap();
    assert_eq!(rerun.state, Run::Journaled);
    assert!(next.history("after-landing-run", "abc").unwrap().iter().any(|logged| logged.to == "abandoned"));
    assert_eq!(misses(&next, "abc"), 1);
    assert!(next.faults(Timestamp::now()).unwrap().is_empty());

    moved::<AfterLandingRun>(&mut first, "def", Run::Journaled, "landed on base").unwrap();
    moved::<AfterLandingRun>(&mut first, "def", Run::Running, "started").unwrap();
    let mut third = lander(&path, 12, 10);
    assert_eq!(third.retire_gone::<AfterLandingRun>(Timestamp::now()).unwrap().len(), 1);
    let later = Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 31 * 60 * 1000);
    assert_eq!(third.faults(later).unwrap().len(), 1, "the abandoned run is overdue past its dwell until a pass retires it");
    assert_eq!(retire_unadopted(&mut third, later).unwrap(), 1);
    assert_eq!(retire_unadopted(&mut third, later).unwrap(), 0);
    assert!(third.faults(later).unwrap().is_empty());
    assert_eq!(third.current::<AfterLandingRun>("def").unwrap().unwrap().state, Run::Retired);
    assert_eq!(third.current::<AfterLandingRun>("abc").unwrap().unwrap().state, Run::Journaled);

    for step in [Run::Journaled, Run::Running, Run::Searching] {
        moved::<AfterLandingRun>(&mut first, "ghi", step, "searching for the culprit lane").unwrap();
    }
    let mut fourth = lander(&path, 13, 10);
    assert_eq!(fourth.retire_gone::<AfterLandingRun>(Timestamp::now()).unwrap().len(), 1);
    assert!(super::lost_in_search(&fourth, "ghi"), "a run whose lander stopped in its culprit search resumes the search");
    assert!(!super::lost_in_search(&fourth, "def") && !super::lost_in_search(&fourth, "abc"), "a run lost mid-checks reruns them");
}

#[test]
fn an_after_landing_run_lost_to_each_lander_killed_mid_run_is_taken_by_the_next_and_stays_unfinished_until_a_verdict() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("ledger.db");
    let queue = std::path::Path::new("/queue");
    journal(&mut lander(&path, 100, 0), queue, "abc", "landed on base", serde_json::json!({ "head": "abc" })).unwrap();
    for pid in 101..=110 {
        let mut next = lander(&path, pid, pid - 1);
        let taken = take(&mut next, queue).unwrap();
        assert_eq!(taken.iter().map(|leased| leased.instance.as_str()).collect::<Vec<_>>(), ["abc"], "lander {pid} did not take the run its killed predecessor held");
        assert_ne!(next.current::<AfterLandingRun>("abc").unwrap().unwrap().state, Run::Retired);
        moved::<AfterLandingRun>(&mut next, "abc", Run::Running, "started").unwrap();
    }
    let mut last = lander(&path, 110, 0);
    moved::<AfterLandingRun>(&mut last, "abc", Run::Retired, "verdict green").unwrap();
    settled(&mut last, queue, "abc");
    assert!(take(&mut lander(&path, 111, 110), queue).unwrap().is_empty());
}
