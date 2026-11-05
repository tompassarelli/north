use super::*;

#[test]
fn a_lane_holds_unlanded_work_while_it_has_unmerged_commits_or_uncommitted_changes() {
    let lane = |ahead: u32, dirty: bool, landed: bool| Lane { path: "/l".into(), branch: None, ahead, dirty, landed };
    assert!(lane(1, false, false).unlanded());
    assert!(lane(0, true, true).unlanded());
    assert!(!lane(0, false, false).unlanded(), "every commit is an ancestor of origin/main");
    assert!(!lane(3, false, true).unlanded(), "its change landed in another form");
}
