use north_base::foundation::clock::Timestamp;
use north_base::lifecycle::{FaultKind, Liveness, Owner, Refusal, Store};

use super::{AfterLandingRun, Main, RedMain, Run, misses, moved};

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
fn an_illegal_move_of_a_red_main_episode_or_an_after_landing_run_leaves_its_record_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let mut store = lander(&directory.path().join("ledger.db"), 10, 0);
    moved::<RedMain>(&mut store, "owner/repo", Main::Opened, "the after-landing run of abc is red").unwrap();

    let skipped = store.transition::<RedMain>("owner/repo", Main::Green, "green without clearing", Timestamp::now()).unwrap();

    assert_eq!(skipped, Err(Refusal::Illegal { from: Some(Main::Opened), to: Main::Green }));
    assert_eq!(store.current::<RedMain>("owner/repo").unwrap().unwrap().state, Main::Opened);
    moved::<AfterLandingRun>(&mut store, "abc", Run::Journaled, "landed on base").unwrap();
    moved::<AfterLandingRun>(&mut store, "abc", Run::Running, "started").unwrap();
    moved::<AfterLandingRun>(&mut store, "abc", Run::Retired, "verdict green").unwrap();
    assert!(moved::<AfterLandingRun>(&mut store, "abc", Run::Running, "a stale lander reruns a retired head").is_none());
    assert_eq!(store.current::<AfterLandingRun>("abc").unwrap().unwrap().state, Run::Retired);
}

#[test]
fn a_lander_killed_mid_run_leaves_an_orphaned_run_that_health_reports_and_the_next_lander_reruns_keeping_its_misses() {
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
}
