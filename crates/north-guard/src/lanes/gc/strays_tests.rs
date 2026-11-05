use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime};

use super::local::{lane_activity_ms, reflog_ms};
use super::strays::{Sweep, modified_within, move_idle};
use crate::lanes::live_process::open_paths;
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

    fn sweep<'a>(&'a self, code: &'a Path, main: &'a Path, live: &'a [PathBuf], now: SystemTime, dry_run: bool) -> Sweep<'a> {
        let container = Box::leak(Box::new(self.container()));
        Sweep {
            code,
            home: self.root.path(),
            container,
            main,
            common: Box::leak(Box::new(main.join(".git"))),
            live,
            now_ms: now.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as i64,
            idle_hours: 3.0,
            dry_run,
        }
    }
}

fn sleeper_in(directory: &Path) -> Child {
    Command::new("sleep").arg("60").current_dir(directory).stdout(Stdio::null()).spawn().unwrap()
}

fn never_in_fleet(_: &Path) -> Result<bool, String> {
    Ok(false)
}

#[test]
fn lane_gc_moves_an_idle_stray_clean_or_dirty_into_worktrees_and_leaves_a_busy_one() {
    let rig = Rig::new();
    let now = SystemTime::now() + Duration::from_secs(5 * 3600);
    let clean = rig.stray("lanes/clean", false);
    let dirty = rig.stray("dirty", true);
    let busy = rig.stray("lanes/busy", true);
    let fresh = rig.stray("fresh", false);
    for tree in [&clean, &dirty, &busy] {
        rig.idle_since(tree, 4, now);
    }
    rig.idle_since(&fresh, 1, now);
    let mut holder = sleeper_in(&busy);
    let live = open_paths(Path::new("/proc"));
    let code = rig.container().parent().unwrap().to_path_buf();
    let main = rig.main();

    let outcomes = move_idle(&rig.sweep(&code, &main, &live, now, false), &mut never_in_fleet);
    holder.kill().unwrap();
    holder.wait().unwrap();

    let result = |from: &Path| outcomes.iter().find(|outcome| outcome.from == from).map(|outcome| outcome.result.clone());
    assert_eq!(result(&clean), Some(Ok(())));
    assert_eq!(result(&dirty), Some(Ok(())));
    assert_eq!(result(&busy), Some(Err("process inside".into())));
    assert!(!clean.exists() && !dirty.exists());
    assert!(rig.container().join("worktrees/clean/f.txt").exists());
    assert_eq!(fs::read_to_string(rig.container().join("worktrees/dirty/f.txt")).unwrap(), "edited\n");
    assert!(busy.exists() && fresh.exists());
    let listed = git_in(&main, &["worktree", "list", "--porcelain"]);
    assert!(listed.contains(&format!("worktree {}", rig.container().join("worktrees/dirty").display())), "{listed}");
    assert!(!rig.container().join("worktrees/busy").exists());
}

#[test]
fn lane_gc_leaves_a_stray_changed_within_the_idle_window_and_moves_nothing_in_a_dry_run() {
    let rig = Rig::new();
    let now = SystemTime::now() + Duration::from_secs(5 * 3600);
    let recent = rig.stray("recent", false);
    let idle = rig.stray("lanes/idle", false);
    rig.idle_since(&recent, 1, now);
    rig.idle_since(&idle, 4, now);
    let code = rig.container().parent().unwrap().to_path_buf();
    let main = rig.main();

    let dry = move_idle(&rig.sweep(&code, &main, &[], now, true), &mut never_in_fleet);
    assert!(idle.exists() && !rig.container().join("worktrees/idle").exists());
    assert_eq!(dry.iter().filter(|outcome| outcome.result.is_ok()).count(), 1);

    let outcomes = move_idle(&rig.sweep(&code, &main, &[], now, false), &mut never_in_fleet);
    let kept = outcomes.iter().find(|outcome| outcome.from == recent).unwrap();
    assert_eq!(kept.result, Err("touched within 3h".into()));
    assert!(recent.exists() && rig.container().join("worktrees/idle").exists());
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
