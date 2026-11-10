use std::fs;
use std::path::{Path, PathBuf};

use super::reap_tests::Container;
use super::remove::remove;
use super::test_git::{git_in, init_repo};

fn nested_lane(home: &Path) -> (PathBuf, PathBuf) {
    let main = home.join("code/proj/main");
    init_repo(&main, None);
    let lane = main.join("proj/worktrees/config-drift-993");
    git_in(&main, &["worktree", "add", "-q", "--detach", lane.to_str().unwrap(), "HEAD"]);
    (main, lane)
}

#[test]
fn an_empty_shell_nested_inside_the_main_checkout_is_removed_north_1006() {
    let home = tempfile::tempdir().unwrap();
    let (main, lane) = nested_lane(home.path());
    std::fs::remove_file(lane.join("f.txt")).unwrap();
    git_in(&main, &["worktree", "lock", lane.to_str().unwrap()]);

    let queues = home.path().join("queues");
    assert!(remove(&lane, &queues).is_ok());
    assert!(!lane.exists() && !main.join("proj").exists());
    assert!(!git_in(&main, &["worktree", "list"]).contains("config-drift-993"));
}

#[test]
fn a_nested_lane_that_holds_files_is_kept_north_1006() {
    let home = tempfile::tempdir().unwrap();
    let (_, lane) = nested_lane(home.path());

    let kept = remove(&lane, &home.path().join("queues")).unwrap_err();
    assert!(kept.contains("worktree move"), "{kept}");
    assert!(lane.join("f.txt").exists());
}

#[test]
fn a_squash_landed_lane_is_removed_and_one_with_unlanded_work_is_kept_north_958() {
    let container = Container::new();
    let lane = container.add("squash-landed", false);
    container.commit_in(&lane, "first.txt");
    container.commit_in(&lane, "second.txt");
    container.squash_land("squash-landed", "squash-landed work");
    fs::write(container.main().join("later.txt"), "later\n").unwrap();
    git_in(&container.main(), &["add", "later.txt"]);
    git_in(&container.main(), &["commit", "-qm", "main advances after the landing"]);

    let removed = remove(&lane, &container.path().join("queues")).expect("a squash-landed lane is spent");
    assert!(!lane.exists(), "{removed}");
    assert!(!git_in(&container.main(), &["worktree", "list"]).contains("squash-landed"));

    let partial = container.add("partly-landed", false);
    container.commit_in(&partial, "alpha.txt");
    container.commit_in(&partial, "beta.txt");
    fs::write(container.main().join("alpha.txt"), "alpha.txt payload\n").unwrap();
    git_in(&container.main(), &["add", "alpha.txt"]);
    git_in(&container.main(), &["commit", "-qm", "land only the first file"]);

    let queues = container.path().join("queues");
    let kept = remove(&partial, &queues).unwrap_err();
    assert!(kept.contains("unmerged"), "{kept}");
    assert!(partial.exists());

    let dirty = container.add("dirty", false);
    container.commit_in(&dirty, "third.txt");
    container.squash_land("dirty", "land every commit");
    fs::write(dirty.join("scratch.txt"), "uncommitted\n").unwrap();
    let kept = remove(&dirty, &queues).unwrap_err();
    assert!(kept.contains("dirty"), "{kept}");
    assert!(dirty.exists());
}

#[test]
fn a_lane_landed_on_origin_is_removed_while_the_hosts_local_main_lags_north_1036() {
    let container = Container::new();
    let lane = container.add("landed-on-origin", false);
    container.commit_in(&lane, "work.txt");
    let main = container.main();
    let landed = git_in(&lane, &["rev-parse", "HEAD"]);
    git_in(&main, &["remote", "add", "origin", container.path().join("origin.git").to_str().unwrap()]);
    git_in(&main, &["update-ref", "refs/remotes/origin/main", landed.trim()]);
    git_in(&main, &["branch", "--set-upstream-to=origin/main", "main"]);

    let removed = remove(&lane, &container.path().join("queues")).expect("a lane on origin/main is spent");
    assert!(!lane.exists(), "{removed}");
    assert!(!git_in(&main, &["branch", "--list", "landed-on-origin"]).contains("landed-on-origin"));
}
