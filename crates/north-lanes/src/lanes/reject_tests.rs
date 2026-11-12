use std::fs;
use std::path::Path;
use std::process::Command;

use super::lane_rejection;
use super::reap_tests::Container;
use super::reject::{reject, reject_branch};
use super::remove::remove;
use super::test_git::git_in;

#[test]
fn a_rejected_lane_holding_unlanded_work_is_bundled_with_its_verdict_and_removed_north_892() {
    let container = Container::new();
    let queues = container.path().join("queues");
    let archive = container.path().join("archive/lanes");
    let lane = container.add("rejected", false);
    container.commit_in(&lane, "scope.txt");
    fs::write(lane.join("notes.txt"), "uncommitted\n").unwrap();
    assert!(remove(&lane, &queues).is_err(), "plain removal keeps a lane holding unlanded work");

    let mut reviewer = Command::new("sleep").arg("30").current_dir(&lane).spawn().unwrap();
    let recorded = reject(&lane, "out of\tscope", &queues, &archive, 0);
    reviewer.kill().unwrap();
    reviewer.wait().unwrap();
    assert!(recorded.unwrap().starts_with("recorded"), "the reviewer still working in the lane defers it to lane gc");
    assert_eq!(lane_rejection::recorded(&container.main(), "rejected").as_deref(), Some("out of scope"));

    let archived = reject(&lane, "out of scope", &queues, &archive, 0).unwrap();
    assert!(!lane.exists(), "{archived}");
    assert!(git_in(&container.main(), &["branch", "--list", "rejected"]).is_empty());
    assert_eq!(lane_rejection::recorded(&container.main(), "rejected"), None);
    let project = container.path().file_name().unwrap().to_string_lossy().into_owned();
    let index = fs::read_to_string(archive.join(&project).join("index.tsv")).unwrap();
    assert!(index.contains("\trejected: out of scope\t"), "{index}");

    let orphan = container.add("half-removed", false);
    container.commit_in(&orphan, "duplicate.txt");
    fs::remove_dir_all(container.main().join(".git/worktrees/half-removed")).unwrap();
    let tip = git_in(&container.main(), &["rev-parse", "half-removed"]);
    let lock = archive.join(&project).join(format!("half-removed-{}.bundle.lock", &tip[..12]));
    fs::write(&lock, "left by a bundle that failed\n").unwrap();
    assert!(Command::new("touch").args(["-d", "2 minutes ago"]).arg(&lock).status().unwrap().success());
    reject(&orphan, "duplicate of main", &queues, &archive, 0).unwrap();
    assert!(!orphan.exists());
    assert!(git_in(&container.main(), &["branch", "--list", "half-removed"]).is_empty());
    assert_eq!(fs::read_to_string(archive.join(&project).join("index.tsv")).unwrap().lines().count(), 2);

    git_in(&container.main(), &["checkout", "-q", "-b", "codex/x-1"]);
    container.commit_in(&container.main(), "x.txt");
    git_in(&container.main(), &["checkout", "-q", "main"]);
    let archived = reject_branch(&container.path(), "codex/x-1", "superseded", &archive, 0).unwrap();
    assert!(archived.starts_with("archived: branch codex/x-1"), "{archived}");
    assert!(git_in(&container.main(), &["branch", "--list", "codex/x-1"]).is_empty());
    let branch_index = fs::read_to_string(archive.join(&project).join("index.tsv")).unwrap();
    assert!(branch_index.contains("\trejected: superseded\t"), "{branch_index}");
    let bundle = Path::new(branch_index.lines().last().unwrap().rsplit('\t').next().unwrap());
    assert!(bundle.is_file(), "{}", bundle.display());
    assert!(bundle.file_name().unwrap().to_string_lossy().starts_with("codex-x-1-"), "{}", bundle.display());
}
