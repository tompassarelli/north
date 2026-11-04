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
