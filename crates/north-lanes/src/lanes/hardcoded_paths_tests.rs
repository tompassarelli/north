use super::repository_name;
use super::super::test_git::{git_in, init_repo};

#[test]
fn a_checkout_is_named_by_its_origin_not_its_container_directory() {
    let scratch = tempfile::tempdir().unwrap();
    let main = scratch.path().join("nixos-config/main");
    init_repo(&main, None);
    assert_eq!(repository_name(&main), "nixos-config");
    git_in(&main, &["remote", "add", "origin", "https://github.com/owner/firn.git"]);
    assert_eq!(repository_name(&main), "firn");
    let lane = scratch.path().join("nixos-config/worktrees/lane");
    git_in(&main, &["worktree", "add", "-q", "--detach", lane.to_str().unwrap()]);
    assert_eq!(repository_name(&lane), "firn");
}
