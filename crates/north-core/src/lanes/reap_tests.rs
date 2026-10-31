use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, SystemTime};

use crate::platform::git;
use super::reap::{Counts, Sweep, all_containers};
use super::repo::find_container;
use super::test_git::{git_in, init_repo};

const AGED: Duration = Duration::from_secs(3 * 3600);

struct Container {
    root: tempfile::TempDir,
}

impl Container {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let container = Self { root };
        init_repo(&container.main(), None);
        fs::write(container.main().join(".gitignore"), "docs/private/\n.direnv/\nresult\nresult-*\n").unwrap();
        git_in(&container.main(), &["add", ".gitignore"]);
        git_in(&container.main(), &["commit", "-qm", "ignores"]);
        container
    }

    fn path(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap()
    }

    fn main(&self) -> PathBuf {
        self.root.path().canonicalize().unwrap().join("main")
    }

    fn lane(&self, name: &str) -> PathBuf {
        self.path().join("worktrees").join(name)
    }

    fn add(&self, name: &str, aged: bool) -> PathBuf {
        let lane = self.lane(name);
        git_in(&self.main(), &["worktree", "add", "-q", "-b", name, lane.to_str().unwrap(), "main"]);
        if aged {
            age(&lane);
        }
        lane
    }

    fn commit_in(&self, lane: &Path, file: &str) {
        fs::write(lane.join(file), format!("{file} payload\n")).unwrap();
        git_in(lane, &["add", file]);
        git_in(lane, &["commit", "-qm", &format!("{file} work")]);
    }

    fn land_equivalent(&self, branch: &str) {
        let sha = git_in(&self.main(), &["rev-parse", branch]);
        let log = self.main().join("log.txt");
        let previous = fs::read_to_string(&log).unwrap_or_default();
        fs::write(&log, format!("{previous}{branch} landed\n")).unwrap();
        git_in(&self.main(), &["add", "log.txt"]);
        git_in(&self.main(), &["commit", "-qm", &format!("main advances before {branch} lands")]);
        git_in(&self.main(), &["cherry-pick", &sha]);
    }

    fn sweep(&self, dry_run: bool) -> (Vec<String>, Counts) {
        let sweep = Sweep { dry_run, now: SystemTime::now(), proc_root: PathBuf::from("/proc") };
        let (report, failure) = sweep.reap_containers(&[self.path()]);
        assert_eq!(failure, None);
        (report.lines, report.counts)
    }
}

fn age(lane: &Path) {
    fs::File::options().write(true).open(lane.join(".git")).unwrap().set_modified(SystemTime::now() - AGED).unwrap();
}

fn evil_merge_lane(container: &Container) -> PathBuf {
    let lane = container.add("merge-lane", false);
    git_in(&lane, &["checkout", "-q", "-b", "merge-side"]);
    fs::write(lane.join("f.txt"), "side version\n").unwrap();
    git_in(&lane, &["commit", "-qam", "side work"]);
    git_in(&lane, &["checkout", "-q", "merge-lane"]);
    fs::write(lane.join("merge-lane.txt"), "merge-lane payload\n").unwrap();
    fs::write(lane.join("f.txt"), "lane version\n").unwrap();
    git_in(&lane, &["add", "merge-lane.txt", "f.txt"]);
    git_in(&lane, &["commit", "-qm", "merge-lane work"]);
    container.land_equivalent("merge-lane");
    fs::write(container.main().join("f.txt"), "base\n").unwrap();
    git_in(&container.main(), &["commit", "-qam", "main restores f.txt"]);
    git_in(&container.main(), &["cherry-pick", "merge-side"]);
    let identity = ["-c", "user.name=lanes-test", "-c", "user.email=lanes-test@example.invalid"];
    let conflict = git::run(&lane, &[identity.as_slice(), &["merge", "--no-ff", "--no-edit", "merge-side"]].concat()).unwrap();
    assert!(!conflict.status.success(), "the side branch must conflict with the lane");
    fs::write(lane.join("f.txt"), "evil resolution found nowhere else\n").unwrap();
    git_in(&lane, &["add", "f.txt"]);
    git_in(&lane, &["commit", "-qm", "merge merge-side (evil resolution)"]);
    let cherry = git_in(&container.main(), &["cherry", "refs/heads/main", "refs/heads/merge-lane"]);
    assert!(!cherry.lines().any(|line| line.starts_with('+')), "only the merge gate may keep this lane: {cherry}");
    assert!(!git_in(&container.main(), &["rev-list", "--merges", "refs/heads/main..refs/heads/merge-lane"]).is_empty());
    lane
}

fn branch_exists(container: &Container, name: &str) -> bool {
    Command::new("git").current_dir(container.main()).args(["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")]).output().unwrap().status.success()
}

#[test]
fn a_sweep_reaps_only_spent_lanes_and_keeps_every_other_shape() {
    let container = Container::new();
    container.add("merged-fresh", false);
    container.add("merged-aged", true);
    container.add("rescue-merged", true);
    container.add("merged-locked", true);
    git_in(&container.main(), &["worktree", "lock", container.lane("merged-locked").to_str().unwrap()]);
    let live_lane = container.add("merged-live", true);
    let mut live: Child = Command::new("sleep").arg("60").current_dir(&live_lane).spawn().unwrap();

    let equiv = container.add("equiv-aged", false);
    container.commit_in(&equiv, "equiv-aged.txt");
    let equiv_sha = git_in(&container.main(), &["rev-parse", "equiv-aged"]);
    container.land_equivalent("equiv-aged");
    age(&equiv);

    let dirty = container.add("dirty", true);
    fs::write(dirty.join("f.txt"), "uncommitted\n").unwrap();
    let untracked = container.add("untracked", true);
    fs::write(untracked.join("scratch.txt"), "x\n").unwrap();
    let unmerged = container.add("unmerged", false);
    container.commit_in(&unmerged, "unique.txt");
    age(&unmerged);

    let ignored = container.add("equiv-ignored", false);
    container.commit_in(&ignored, "equiv-ignored.txt");
    container.land_equivalent("equiv-ignored");
    fs::create_dir_all(ignored.join("docs/private")).unwrap();
    fs::create_dir_all(ignored.join(".direnv/bin")).unwrap();
    fs::write(ignored.join("docs/private/note.md"), "notes\n").unwrap();
    fs::write(ignored.join(".direnv/bin/x"), "cache\n").unwrap();
    std::os::unix::fs::symlink("/nix/store/nonexistent", ignored.join("result")).unwrap();
    age(&ignored);

    let shadow = container.add("tag-shadow", false);
    container.commit_in(&shadow, "tag-shadow.txt");
    git_in(&container.main(), &["tag", "tag-shadow", "main"]);
    age(&shadow);

    let merge_lane = evil_merge_lane(&container);
    age(&merge_lane);

    let pin_oid = git_in(&container.main(), &["rev-parse", "HEAD"]);
    let pin = container.path().join("pins").join(&pin_oid);
    git_in(&container.main(), &["worktree", "add", "-q", "--detach", pin.to_str().unwrap(), &pin_oid]);
    age(&pin);
    let detached = container.lane("detached-aged");
    git_in(&container.main(), &["worktree", "add", "-q", "--detach", detached.to_str().unwrap(), "main"]);
    age(&detached);

    let (dry_lines, _) = container.sweep(true);
    assert!(container.lane("merged-aged").is_dir() && container.lane("equiv-aged").is_dir(), "dry run must change nothing");
    let (lines, counts) = container.sweep(false);
    live.kill().unwrap();
    live.wait().unwrap();

    for kept in ["merged-fresh", "rescue-merged", "merged-locked", "merged-live", "dirty", "untracked", "unmerged", "equiv-ignored", "tag-shadow", "merge-lane"] {
        assert!(container.lane(kept).is_dir() && branch_exists(&container, kept), "{kept} must be kept");
    }
    for reaped in ["merged-aged", "equiv-aged", "detached-aged"] {
        assert!(!container.lane(reaped).exists(), "{reaped} must be reaped");
    }
    assert!(!branch_exists(&container, "merged-aged") && !branch_exists(&container, "equiv-aged"));
    assert!(git_in(&container.main(), &["cat-file", "-t", &equiv_sha]) == "commit");
    assert!(pin.is_dir());
    assert!(!lines.iter().chain(&dry_lines).any(|line| line.contains(&pin_oid)));
    assert!(git_in(&container.main(), &["tag", "--list", "tag-shadow"]) == "tag-shadow");
    assert_eq!(counts, Counts { reaped: 2, reaped_equivalent: 1, kept_dirty: 2, kept_unmerged: 3, kept_protected: 5 }, "{lines:#?}");

    fs::remove_file(ignored.join("docs/private/note.md")).unwrap();
    fs::remove_dir(ignored.join("docs/private")).unwrap();
    fs::remove_dir(ignored.join("docs")).unwrap();
    container.sweep(false);
    assert!(!ignored.exists() && !branch_exists(&container, "equiv-ignored"));
}

#[test]
fn all_finds_every_home_container_and_a_lane_subdirectory_finds_its_container() {
    let home = tempfile::tempdir().unwrap();
    let home_path = home.path().canonicalize().unwrap();
    let main = home_path.join("code/projA/main");
    init_repo(&main, None);
    let done = home_path.join("code/projA/worktrees/done");
    git_in(&main, &["worktree", "add", "-q", "-b", "done-branch", done.to_str().unwrap(), "main"]);
    git_in(&main, &["merge", "-q", "done-branch"]);
    age(&done);
    fs::create_dir_all(home_path.join("code/not-a-container")).unwrap();
    let containers = all_containers(&home_path);
    assert_eq!(containers, vec![home_path.join("code/projA")]);

    let deep = done.join("sub/dir");
    fs::create_dir_all(&deep).unwrap();
    assert_eq!(find_container(&deep), Some(home_path.join("code/projA")));

    let sweep = Sweep { dry_run: false, now: SystemTime::now(), proc_root: PathBuf::from("/proc") };
    let (_, failure) = sweep.reap_containers(&containers);
    assert_eq!(failure, None);
    assert!(!done.exists());
    assert!(git_in(&main, &["branch", "--list", "done-branch"]).is_empty());
}
