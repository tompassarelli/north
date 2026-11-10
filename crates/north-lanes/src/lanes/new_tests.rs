use std::path::Path;
use std::sync::Barrier;

use north_base::platform::git;

use super::lane_issue::recorded;
use super::new::create;
use super::test_git::{git_in, init_repo};

fn tracks_upstream(dir: &Path) -> bool {
    git::run(dir, &["rev-parse", "--verify", "-q", "@{upstream}"]).is_ok_and(|output| output.status.success())
}

#[test]
fn a_new_lane_records_its_issue_in_a_per_branch_file_and_never_writes_upstream_config() {
    let root = tempfile::tempdir().unwrap();
    let container = root.path().canonicalize().unwrap().join("north");
    let main = container.join("main");
    init_repo(&main, None);
    git_in(&main, &["remote", "add", "origin", "https://github.com/tompassarelli/north.git"]);
    git_in(&main, &["update-ref", "refs/remotes/origin/main", "refs/heads/main"]);
    let lane = container.join("worktrees/lanes-995");

    assert_eq!(create(&lane, "#995", None).unwrap(), "north#995");

    assert_eq!(git_in(&lane, &["branch", "--show-current"]), "lanes-995");
    assert!(!tracks_upstream(&lane), "a lane does not track its base, because the shared git config is not written at creation");
    let file = main.join(".git/north/lane-issues/lanes-995");
    assert_eq!(std::fs::read_to_string(&file).unwrap().trim(), "north#995");
    assert_eq!(recorded(&main, "lanes-995").as_deref(), Some("north#995"));
    git_in(&main, &["worktree", "remove", lane.to_str().unwrap()]);
    assert_eq!(recorded(&main, "lanes-995").as_deref(), Some("north#995"), "the record outlives the worktree while its branch remains");

    let lanes = 6;
    let barrier = Barrier::new(lanes);
    let results: Vec<Result<String, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..lanes)
            .map(|index| {
                let barrier = &barrier;
                let lane = container.join(format!("worktrees/concurrent-{index}"));
                scope.spawn(move || {
                    barrier.wait();
                    create(&lane, "#1047", None)
                })
            })
            .collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect()
    });
    for (index, result) in results.into_iter().enumerate() {
        assert_eq!(result.as_deref(), Ok("north#1047"), "concurrent lane {index} is created without a shared-config lock");
        let branch = format!("concurrent-{index}");
        assert!(!tracks_upstream(&container.join("worktrees").join(&branch)));
        assert_eq!(recorded(&main, &branch).as_deref(), Some("north#1047"));
    }

    assert!(create(&container.join("worktrees/other"), "995", None).is_err(), "an issue must name its number as #N");
    assert!(!container.join("worktrees/other").exists());
}
