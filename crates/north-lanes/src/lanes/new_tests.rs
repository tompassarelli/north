use super::lane_issue::recorded;
use super::new::create;
use super::test_git::{git_in, init_repo};

#[test]
fn a_new_lane_records_its_issue_in_branch_config() {
    let root = tempfile::tempdir().unwrap();
    let container = root.path().canonicalize().unwrap().join("north");
    let main = container.join("main");
    init_repo(&main, None);
    git_in(&main, &["remote", "add", "origin", "https://github.com/tompassarelli/north.git"]);
    let lane = container.join("worktrees/lanes-995");

    assert_eq!(create(&lane, "#995", None).unwrap(), "north#995");

    assert_eq!(git_in(&lane, &["branch", "--show-current"]), "lanes-995");
    assert_eq!(git_in(&main, &["config", "--get", "branch.lanes-995.north-issue"]), "north#995");
    assert_eq!(recorded(&main, "lanes-995").as_deref(), Some("north#995"));
    git_in(&main, &["worktree", "remove", lane.to_str().unwrap()]);
    assert_eq!(recorded(&main, "lanes-995").as_deref(), Some("north#995"), "the record outlives the worktree while its branch remains");

    assert!(create(&container.join("worktrees/other"), "995", None).is_err(), "an issue must name its number as #N");
    assert!(!container.join("worktrees/other").exists());
}
