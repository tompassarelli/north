use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::local::{lane_activity_ms, reflog_ms};
use super::strays::modified_within;
use crate::lanes::test_git::{git_in, init_repo};

struct Rig {
    root: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let rig = Self { root: tempfile::tempdir().unwrap() };
        init_repo(&rig.main(), None);
        fs::create_dir_all(rig.container().join("worktrees")).unwrap();
        rig
    }

    fn container(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap().join("code/proj")
    }

    fn main(&self) -> PathBuf {
        self.container().join("main")
    }

    fn stray(&self, path: &str, dirty: bool) -> PathBuf {
        let at = self.container().join(path);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        git_in(&self.main(), &["worktree", "add", "-q", "--detach", at.to_str().unwrap()]);
        if dirty {
            fs::write(at.join("f.txt"), "edited\n").unwrap();
        }
        at
    }

    fn idle_since(&self, tree: &Path, hours_ago: u64, now: SystemTime) {
        let then = now - Duration::from_secs(hours_ago * 3600);
        let admin = git_in(tree, &["rev-parse", "--path-format=absolute", "--git-dir"]);
        for touched in [tree.to_path_buf(), PathBuf::from(&admin).join("HEAD"), PathBuf::from(&admin).join("index")] {
            if touched.exists() {
                File::open(&touched).unwrap().set_modified(then).unwrap();
            }
        }
        for entry in fs::read_dir(tree).unwrap().flatten().filter(|entry| entry.file_name() != ".git") {
            if entry.path().is_file() {
                File::options().write(true).open(entry.path()).unwrap().set_modified(then).unwrap();
            }
        }
    }

}

#[test]
fn a_lane_whose_reflog_files_were_rewritten_by_git_maintenance_keeps_its_reflog_entry_time() {
    let rig = Rig::new();
    let lane = rig.stray("lanes/old", false);
    let admin = PathBuf::from(git_in(&lane, &["rev-parse", "--path-format=absolute", "--git-dir"]));
    let now = SystemTime::now();
    let secs = now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() - 500 * 3600;
    let entry = format!("0000000 1111111 tester <t@example.invalid> {secs} +0800\trebase (finish): returning to refs/heads/old\n");
    let head_log = admin.join("logs/HEAD");
    fs::create_dir_all(head_log.parent().unwrap()).unwrap();
    fs::write(&head_log, &entry).unwrap();
    let branch_log = rig.main().join(".git/logs/refs/heads/old");
    fs::create_dir_all(branch_log.parent().unwrap()).unwrap();
    fs::write(&branch_log, &entry).unwrap();
    rig.idle_since(&lane, 700, now);

    assert_eq!(reflog_ms(&head_log), secs as f64 * 1000.0);
    assert_eq!(reflog_ms(&rig.main().join(".git/logs/refs/heads/absent")), 0.0);
    assert_eq!(lane_activity_ms(&lane, &admin, &rig.main().join(".git"), "old", 0.0), secs as f64 * 1000.0);
}

#[test]
fn build_output_the_repository_ignores_never_counts_as_a_lane_being_touched() {
    let rig = Rig::new();
    let lane = rig.stray("lanes/built", false);
    let now = SystemTime::now();
    rig.idle_since(&lane, 100, now);
    let now_ms = now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64;
    fs::write(rig.main().join(".git/info/exclude"), "target/\n").unwrap();
    fs::create_dir_all(lane.join("target/debug")).unwrap();
    fs::write(lane.join("target/debug/out"), "built\n").unwrap();

    assert!(!modified_within(&lane, now_ms, 72.0));
    fs::write(lane.join("notes.txt"), "work\n").unwrap();
    assert!(modified_within(&lane, now_ms, 72.0));
}
