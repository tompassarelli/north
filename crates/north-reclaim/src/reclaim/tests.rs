use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Inputs, Policy, reclaim};
use north_base::platform::disk_budget::DiskBudget;

pub(super) const DAY: Duration = Duration::from_secs(24 * 3600);

pub(super) struct Machine {
    root: tempfile::TempDir,
    pub(super) now: SystemTime,
}

impl Machine {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["tmp", "proc", "clients", "state", "code", "cache"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        Self { root, now: UNIX_EPOCH + Duration::from_secs(1_800_000_000) }
    }

    pub(super) fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    pub(super) fn dir(&self, relative: &str, age: Duration) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(&path).unwrap();
        touch(&path, self.now - age);
        path
    }

    fn file(&self, relative: &str, age: Duration) -> PathBuf {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
        touch(&path, self.now - age);
        path
    }

    pub(super) fn inputs(&self) -> Inputs {
        Inputs {
            proc_root: self.path("proc"),
            code: self.path("code"),
            buck_daemons: self.path("buckd"),
            state: self.path("state"),
            cache: self.path("cache"),
            lan_clients: self.path("clients"),
            online_clients: self.path("online"),
            lan_state: self.path("state"),
            evidence: self.path("evidence"),
            capture_queue: self.path("capture-queue"),
            disk_budget: DiskBudget::default(),
            open_issue_text: || Ok(String::new()),
            now: self.now,
        }
    }

    fn reclaim(&self, dry_run: bool) -> Vec<PathBuf> {
        let removals = reclaim(&self.inputs(), &Policy { dry_run, ..Policy::everything(DAY) }).unwrap();
        let mut paths = removals.into_iter().map(|removal| removal.path).collect::<Vec<_>>();
        paths.sort();
        paths
    }
}

pub(super) fn touch(path: &Path, at: SystemTime) {
    File::open(path).unwrap().set_modified(at).unwrap();
}

#[test]
fn a_scratch_root_no_session_record_names_is_left_in_place() {
    let machine = Machine::new();
    let evaluation = machine.dir("tmp/eval-run-7", 3 * DAY);
    machine.file("tmp/eval-run-7/results.jsonl", 3 * DAY);
    touch(&evaluation, machine.now - 3 * DAY);
    let download = machine.file("tmp/sc50-native.png", 3 * DAY);

    assert_eq!(machine.reclaim(false), Vec::<PathBuf>::new());
    assert!(evaluation.join("results.jsonl").exists() && download.exists());
}

#[test]
fn a_cargo_target_of_a_crate_below_an_idle_lane_is_reclaimed_like_the_lane_s_own() {
    let machine = Machine::new();
    let lane = machine.dir("code/wisp/worktrees/perf-ticks", 3 * DAY);
    let nested = machine.dir("code/wisp/worktrees/perf-ticks/window/target", 3 * DAY);
    fs::write(nested.join("CACHEDIR.TAG"), "Signature: 8a477f597d28d172789f06886806bc55\n").unwrap();
    touch(&nested.join("CACHEDIR.TAG"), machine.now - 3 * DAY);
    touch(&nested, machine.now - 3 * DAY);

    assert_eq!(machine.reclaim(false), vec![nested.clone()]);
    assert!(lane.exists() && !nested.exists());
}
