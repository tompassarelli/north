use super::super::canary::{self, Landers, Machine, Probe, Ran, Rollback, Verdict};
use super::super::failure::failed;
use super::super::layout::{self, Layout};
use super::super::live::Swap;
use super::super::pin::{Event, read};
use super::{handed_to_live, judged, ship_if_walked, walk_verdict};
use north_base::foundation::shim::write_executable;
use std::cell::Cell;
use std::fs;

const PINNED: &str = "1111111111111111111111111111111111111111";
const MAIN: &str = "2222222222222222222222222222222222222222";

#[test]
fn a_rollback_is_journaled_with_its_seconds_live_and_whether_the_walking_host_took_it() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let swaps = vec![Swap { runtime: PINNED.to_owned(), at: 10, estimated: false }, Swap { runtime: MAIN.to_owned(), at: super::super::pin::now() - 90, estimated: false }];
    judged(&layout, "tom@gex45", MAIN, &Verdict::RolledBack, &swaps);
    judged(&layout, "tom@gex45", MAIN, &Verdict::Passed, &swaps);
    judged(&layout, &crate::platform::host::name(), MAIN, &Verdict::Passed, &swaps);
    let events = read(&layout.state);
    assert_eq!(events.len(), 2, "a pass on another host is no walk: {events:?}");
    match &events[0] {
        Event::RolledBack { host, seconds, walker, .. } => assert!(host == "tom@gex45" && !*walker && (90..95).contains(seconds), "{events:?}"),
        other => panic!("not a rollback: {other:?}"),
    }
    assert!(matches!(&events[1], Event::Walked { sha, .. } if sha == MAIN));
}

#[test]
fn nothing_ships_to_another_host_until_the_runtime_walked_the_spine_here() {
    let shipped = Cell::new(0);
    let ship = |_: &str| {
        shipped.set(shipped.get() + 1);
        Ok(())
    };
    assert_eq!(ship_if_walked(Err("rolled back".to_owned()), ship).unwrap(), None);
    assert_eq!(shipped.get(), 0);
    assert_eq!(ship_if_walked(Ok(MAIN.to_owned()), ship).unwrap(), Some(()));
    assert_eq!(shipped.get(), 1);
    assert!(ship_if_walked(Ok(MAIN.to_owned()), |_| Err::<(), _>(failed(1, "unreachable"))).is_err());
}

#[test]
fn a_promotion_leaves_the_walk_after_its_swap_to_the_runtime_it_promoted() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let promoted = layout.runtime(MAIN);
    fs::create_dir_all(&promoted).unwrap();
    for program in layout::PROGRAMS {
        write_executable(&promoted.join(program), "#!/bin/sh\nhere=$(dirname \"$0\")\necho \"$*\" >> \"$here/asked\"\ncode=$(cat \"$here/exit\")\n[ \"$code\" = killed ] && kill -KILL $$\nexit \"$code\"\n");
    }
    std::os::unix::fs::symlink(layout::link(MAIN), layout.current()).unwrap();
    let exits = |code: &str| fs::write(promoted.join("exit"), code).unwrap();

    exits("0");
    assert!(matches!(handed_to_live(&layout, Some(PINNED)), Some(Ok(()))), "the promoting runtime walked the runtime it had just swapped in with its own code");
    assert_eq!(fs::read_to_string(promoted.join("asked")).unwrap(), "runtime walk\n");
    exits("3");
    assert_eq!(handed_to_live(&layout, Some(PINNED)).unwrap().unwrap_err().code, 3);
    exits("64");
    assert!(handed_to_live(&layout, Some(PINNED)).is_none(), "a runtime older than the verb is walked by the one that promoted it");
    for died in ["killed", "101"] {
        exits(died);
        assert!(handed_to_live(&layout, Some(PINNED)).is_none(), "a runtime that died walking itself ({died}) stayed live with nobody to walk it or roll it back");
    }
    assert!(handed_to_live(&layout, Some(MAIN)).is_none(), "the live runtime walks itself");
    assert!(handed_to_live(&layout, None).is_none());
    assert_eq!(fs::read_to_string(promoted.join("asked")).unwrap().lines().count(), 5);
}

const STEP_TWO: &str = "the worker hands in with one `hand-in` tool call, which returns `Done: landed`";

struct FailsTheWalk(String);

impl Machine for FailsTheWalk {
    fn probe(&self, probe: &Probe) -> Ran {
        let log = if probe.name.contains("verify-north") { self.0.clone() } else { String::new() };
        Ran::Exited { code: u8::from(!log.is_empty()), log }
    }

    fn roll_back(&self, _sha: &str) -> Rollback {
        Rollback::To(PINNED.to_owned())
    }

    fn alarm(&self, _text: &str) {}

    fn page(&self, _key: &str, _text: &str) {}

    fn rollback_target(&self, _sha: &str) -> Option<String> {
        None
    }

    fn launchable_providers(&self) -> Vec<String> {
        Vec::new()
    }

    fn sessions_reach_hub(&self, _host: &str) -> bool {
        false
    }

    fn offloads_to_hub(&self, _host: &str) -> bool {
        false
    }

    fn settles_fleet(&self, _host: &str) -> bool {
        true
    }

    fn takes_land_checks(&self, _host: &str) -> bool {
        false
    }

    fn lander_repositories(&self) -> Option<Landers> {
        None
    }
}

#[test]
fn a_failed_spine_walk_keeps_the_failing_step_s_report_and_log() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let report = format!("step 2 FAIL   44912 ms  {STEP_TWO}: vn-1794354314 did not settle within 120 s; working");
    let log = format!(
        "step 0 ok       812 ms  use the given north build: /tmp/runtimes/{MAIN}/north\nstep 1 ok      9120 ms  `north land serve` serves a scratch repository's queue: vn-1794354314 on claude/b\n{report}\nverify-north failed step 2: {STEP_TWO}\nstep 3 skip       0 ms  the lander landed the worker's commit on origin main\nverify-north kept the scratch ledger in /home/someone/.local/state/north/verify-north/failed/1794354314\nverify-north FAILED at step 2  total 44913 ms\n"
    );
    let verdict = canary::check(&layout, "whiterabbit", MAIN, &FailsTheWalk(log));
    assert_eq!(verdict, Verdict::RolledBack);
    let kept = state.path().join("verify-north/failed/1794354314");
    let read = |name: &str| fs::read_to_string(kept.join(name)).unwrap_or_else(|error| panic!("on 2026-11-11 the kept directory of a walk rejected at step 2 was empty: {name}: {error}"));
    assert_eq!(read("step"), format!("step 2: {STEP_TWO}\n"));
    assert_eq!(read("report"), format!("{report}\n"));
    assert!(read("log").contains("did not settle within 120 s") && read("log").ends_with("verify-north FAILED at step 2  total 44913 ms\n"), "{}", read("log"));
    let why = walk_verdict(&layout, "whiterabbit", MAIN, &verdict).unwrap_err();
    assert!(why.contains(&kept.display().to_string()) && why.contains("did not settle"), "the line runtime-follow.log gets names neither the kept directory nor the step: {why}");
}
