use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::rescue::{Finished, Rescue, apply_diff};
use super::test_git::{git_in, init_repo};

struct Project {
    root: tempfile::TempDir,
}

impl Project {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = Self { root };
        init_repo(&project.main(), None);
        fs::write(project.main().join("other.txt"), "kept\n").unwrap();
        git_in(&project.main(), &["add", "other.txt"]);
        git_in(&project.main(), &["commit", "-qm", "other"]);
        project
    }

    fn path(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    fn main(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap().join("main")
    }

    fn rescue(&self, dry_run: bool) -> Finished {
        Rescue { now: SystemTime::now(), apply_diff }.rescue(&self.main(), dry_run)
    }

    fn lanes(&self) -> Vec<PathBuf> {
        fs::read_dir(self.path().join("worktrees")).map(|entries| entries.flatten().map(|entry| entry.path()).collect()).unwrap_or_default()
    }
}

fn skip_apply(_: &Path, _: &[u8]) -> bool {
    true
}

#[test]
fn a_dirty_main_is_relocated_verified_committed_and_restored() {
    let project = Project::new();
    let main = project.main();
    fs::write(main.join(".gitignore"), "*.ignoreme\n").unwrap();
    git_in(&main, &["add", ".gitignore"]);
    git_in(&main, &["commit", "-qm", "ignore"]);
    fs::write(main.join("f.txt"), "base\nunstaged change\n").unwrap();
    fs::write(main.join("new.txt"), "staged new\n").unwrap();
    git_in(&main, &["add", "new.txt"]);
    fs::create_dir_all(main.join("sub")).unwrap();
    fs::write(main.join("loose.txt"), "loose\n").unwrap();
    fs::write(main.join("sub/nested.txt"), "nested\n").unwrap();
    fs::write(main.join("secret.ignoreme"), "should stay\n").unwrap();
    fs::write(main.join("read-only.txt"), "locked\n").unwrap();
    fs::set_permissions(main.join("read-only.txt"), std::os::unix::fs::PermissionsExt::from_mode(0o444)).unwrap();

    let Finished::Done(line) = project.rescue(false) else { panic!("rescue failed") };
    assert!(line.starts_with("rescued -> "), "{line}");
    assert_eq!(git_in(&main, &["status", "--porcelain"]), "");
    assert_eq!(fs::read_to_string(main.join("f.txt")).unwrap(), "base\n");
    assert!(!main.join("new.txt").exists() && !main.join("loose.txt").exists() && !main.join("sub").exists());
    assert!(main.join("secret.ignoreme").exists() && main.join(".gitignore").exists());

    let lanes = project.lanes();
    assert_eq!(lanes.len(), 1);
    let rescued = &lanes[0];
    assert!(rescued.file_name().unwrap().to_string_lossy().starts_with("rescue-"));
    assert!(git_in(rescued, &["symbolic-ref", "--short", "HEAD"]).starts_with("rescue-"));
    assert_eq!(git_in(rescued, &["status", "--porcelain"]), "");
    assert_eq!(fs::read_to_string(rescued.join("f.txt")).unwrap(), "base\nunstaged change\n");
    assert_eq!(fs::read_to_string(rescued.join("new.txt")).unwrap(), "staged new\n");
    assert_eq!(fs::read_to_string(rescued.join("sub/nested.txt")).unwrap(), "nested\n");
    assert_eq!(fs::read_to_string(rescued.join("read-only.txt")).unwrap(), "locked\n");
    assert!(!rescued.join("secret.ignoreme").exists());
    assert!(git_in(&main, &["worktree", "list"]).contains(rescued.to_str().unwrap()));
}

#[test]
fn an_unreadable_main_status_fails_instead_of_reporting_clean() {
    let project = Project::new();
    let git_dir = project.main().join(".git");
    fs::remove_dir_all(&git_dir).unwrap();
    fs::write(&git_dir, "gitdir: /nonexistent\n").unwrap();
    assert!(matches!(project.rescue(false), Finished::Failed(_)));
}

#[test]
fn a_verification_mismatch_aborts_with_main_untouched() {
    let project = Project::new();
    fs::write(project.main().join("f.txt"), "base\ndirty\n").unwrap();
    let before = git_in(&project.main(), &["status", "--porcelain"]);
    let head = git_in(&project.main(), &["rev-parse", "HEAD"]);
    let failed = Rescue { now: SystemTime::now(), apply_diff: skip_apply }.rescue(&project.main(), false);
    let Finished::Failed(lines) = failed else { panic!("a corrupted copy must abort") };
    assert!(lines.iter().any(|line| line.contains("VERIFY MISMATCH: f.txt")), "{lines:?}");
    assert_eq!(git_in(&project.main(), &["status", "--porcelain"]), before);
    assert_eq!(git_in(&project.main(), &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read_to_string(project.main().join("f.txt")).unwrap(), "base\ndirty\n");
}

#[test]
fn a_deleted_tracked_archive_and_an_untracked_archive_are_rescued() {
    let project = Project::new();
    let main = project.main();
    let old_archive: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
    let new_archive: Vec<u8> = (0..=255u8).rev().cycle().take(4096).collect();
    fs::create_dir_all(main.join("vendor")).unwrap();
    fs::write(main.join("vendor/old.tgz"), &old_archive).unwrap();
    git_in(&main, &["add", "vendor/old.tgz"]);
    git_in(&main, &["commit", "-qm", "archive"]);
    fs::remove_file(main.join("vendor/old.tgz")).unwrap();
    fs::write(main.join("vendor/new.tgz"), &new_archive).unwrap();

    let Finished::Done(line) = project.rescue(false) else { panic!("rescue failed") };
    assert!(line.starts_with("rescued -> "), "{line}");
    assert_eq!(git_in(&main, &["status", "--porcelain"]), "");
    assert_eq!(fs::read(main.join("vendor/old.tgz")).unwrap(), old_archive);
    assert!(!main.join("vendor/new.tgz").exists());

    let rescued = &project.lanes()[0];
    assert_eq!(git_in(rescued, &["status", "--porcelain"]), "");
    assert!(!rescued.join("vendor/old.tgz").exists());
    assert_eq!(fs::read(rescued.join("vendor/new.tgz")).unwrap(), new_archive);
    assert_eq!(git_in(rescued, &["show", "--name-status", "--format=", "HEAD"]), "A\tvendor/new.tgz\nD\tvendor/old.tgz");
}
