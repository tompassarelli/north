use super::*;

const TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[test]
fn a_base_check_is_judged_by_every_timing_recorded_for_its_tree_command_and_host_class() {
    let root = tempfile::tempdir().unwrap();
    let timings = Timings::at(root.path().to_owned());
    assert_eq!(timings.base(TREE, "check", "local", 60), Base::Untimed);
    timings.record(TREE, "check", "local", Timing::Finished(12)).unwrap();
    timings.record(TREE, "check", "local", Timing::Finished(30)).unwrap();
    assert_eq!(timings.base(TREE, "check", "local", 60), Base::Within(30));
    assert_eq!(timings.base(TREE, "check", "local", 20), Base::Exceeded, "a base run slower than this deadline did not finish within it");
    assert_eq!(timings.base(TREE, "check", "buildbox", 60), Base::Untimed);
    assert_eq!(timings.base(TREE, "other", "local", 60), Base::Untimed);
    timings.record(TREE, "check", "local", Timing::Exceeded(60)).unwrap();
    assert_eq!(timings.base(TREE, "check", "local", 60), Base::Exceeded, "a base tree that once hung is not evidence against a lane");
}

#[test]
fn a_timing_names_its_tree_by_object_id() {
    let root = tempfile::tempdir().unwrap();
    let timings = Timings::at(root.path().to_owned());
    assert!(timings.record("../escape", "check", "local", Timing::Finished(1)).is_err());
    assert_eq!(timings.base("../escape", "check", "local", 60), Base::Untimed);
}

#[test]
fn check_timings_deadline_is_three_p90s_of_the_last_twenty_passes_between_300_and_2100_seconds() {
    let unmeasured = Deadline::from_passes(&[100, 100, 100]);
    assert_eq!(unmeasured, Deadline { seconds: 2100, basis: Basis::Unmeasured { passes: 3 } });
    assert_eq!(unmeasured.to_string(), "deadline 2100 s, 3 passes recorded");
    assert_eq!(Deadline::from_passes(&[]).seconds, 2100);
    assert_eq!(Deadline::from_passes(&[1, 1, 1, 1]).seconds, 2100, "four passes are too few to measure");
    let floored = Deadline::from_passes(&[60; 5]);
    assert_eq!(floored, Deadline { seconds: 300, basis: Basis::Measured { p90: 60, passes: 5 } });
    let mut walls = vec![10_000; 30];
    walls.extend([100; 17]);
    walls.extend([140, 140, 900]);
    let measured = Deadline::from_passes(&walls);
    assert_eq!(measured.to_string(), "deadline 420 s from p90 140 s over 20 passes", "only the last 20 passes count, and the slowest tenth is above the p90");
    assert_eq!(Deadline::from_passes(&[800; 20]).seconds, 2100, "the deadline is capped at the flat ceiling");
    assert_eq!(Deadline::configured(2).to_string(), "deadline 2 s from SAFE_PUSH_LAND_CHECK_TIMEOUT");
}

#[test]
fn check_timings_deadline_reads_the_passes_of_its_command_and_host_class() {
    let root = tempfile::tempdir().unwrap();
    let timings = Timings::at(root.path().to_owned());
    for _ in 0..5 {
        timings.passed("check", "local", 150).unwrap();
    }
    assert_eq!(timings.deadline("check", "local").to_string(), "deadline 450 s from p90 150 s over 5 passes");
    assert_eq!(timings.deadline("check", "buildbox").basis, Basis::Unmeasured { passes: 0 });
    assert_eq!(timings.deadline("other", "local").basis, Basis::Unmeasured { passes: 0 });
}
