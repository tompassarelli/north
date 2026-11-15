use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{Inputs, Policy, reclaim};
use crate::platform::disk_budget::DiskBudget;

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

    fn session(&self, provider: &str, name: &str, state: Session, cwd: &Path) {
        let (stopped, ended, host_pid) = match state {
            Ended => (true, Some("host-ended"), None),
            Live => (false, None, Some(std::process::id())),
            Parked => (false, None, None),
        };
        let record = serde_json::json!({"name": name, "provider": provider, "account": "a", "cwd": cwd, "model": null, "created_ms": 1, "host_pid": host_pid, "conversation": null, "stopped": stopped, "ended": ended});
        let directory = self.path(&format!("state/north/fleet/{name}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("session.json"), record.to_string()).unwrap();
    }

    fn scratch(&self, provider: &str, name: &str, state: Session, age: Duration) -> PathBuf {
        self.session(provider, name, state, Path::new("/x"));
        self.dir(&format!("tmp/north-{provider}-{name}"), age)
    }

    pub(super) fn inputs(&self) -> Inputs {
        Inputs {
            tmp: self.path("tmp"),
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
            host: "here".to_owned(),
            open_issue_text: || Ok(String::new()),
            uid: rustix::process::getuid().as_raw(),
            now: self.now,
        }
    }

    fn lane(&self, name: &str, state: Session) -> PathBuf {
        let lane = self.path(&format!("code/north/worktrees/{name}"));
        for built in ["target/debug", "buck-out/v2", "src"] {
            fs::create_dir_all(lane.join(built)).unwrap();
        }
        fs::write(lane.join(".gitignore"), "/target/\n/buck-out/\n").unwrap();
        fs::write(lane.join("src/lib.rs"), "").unwrap();
        assert!(std::process::Command::new("git").arg("-C").arg(&lane).args(["init", "-q"]).status().unwrap().success());
        self.session("claude", name, state, &lane);
        fs::canonicalize(lane).unwrap()
    }

    fn reclaim(&self, dry_run: bool) -> Vec<PathBuf> {
        let reclaimed = reclaim(&self.inputs(), &Policy { dry_run, ..Policy::everything(DAY) }).unwrap();
        let mut paths = reclaimed.removals.into_iter().map(|removal| removal.path).collect::<Vec<_>>();
        paths.sort();
        paths
    }
}

#[derive(Clone, Copy)]
enum Session {
    Ended,
    Live,
    Parked,
}
use Session::{Ended, Live, Parked};

pub(super) fn touch(path: &Path, at: SystemTime) {
    File::open(path).unwrap().set_modified(at).unwrap();
}

#[test]
fn scratch_of_an_ended_session_goes_once_idle_past_the_limit_and_held_by_no_process() {
    let machine = Machine::new();
    let abandoned = machine.scratch("opencode", "abandoned", Ended, 3 * DAY);
    let fresh = machine.scratch("opencode", "fresh", Ended, Duration::from_secs(600));
    let working = machine.scratch("opencode", "working", Ended, 3 * DAY);
    machine.file("tmp/north-opencode-working/frame.png", Duration::from_secs(60));
    touch(&working, machine.now - 3 * DAY);
    let held = machine.scratch("opencode", "held", Ended, 3 * DAY);
    machine.dir("proc/4242/fd", DAY);
    symlink(held.join("out.log"), machine.path("proc/4242/fd/3")).unwrap();
    let stale = machine.scratch("claude", "stale", Ended, 3 * DAY);
    drop(std::os::unix::net::UnixListener::bind(stale.join("broker.sock")).unwrap());
    touch(&stale, machine.now - 3 * DAY);

    assert_eq!(machine.reclaim(false), vec![stale.clone(), abandoned.clone()]);
    assert!(!abandoned.exists() && !stale.exists());
    assert!(fresh.exists() && working.exists() && held.exists());
}

#[test]
fn a_live_or_parked_session_keeps_its_scratch_and_lane_while_an_ended_lane_loses_only_its_build_output() {
    let machine = Machine::new();
    let live = machine.scratch("opencode", "live", Live, 3 * DAY);
    let ended = machine.lane("ended", Ended);
    let working = machine.lane("working", Live);
    let parked = machine.lane("parked", Ended);
    machine.session("claude", "parker", Parked, Path::new("/x"));
    let handing_in = machine.lane("handing-in", Ended);
    let ticket = |area: &str, root: &Path, session: &str| {
        let queue = machine.path(&format!("state/north/land-queue/north/{area}"));
        fs::create_dir_all(&queue).unwrap();
        fs::write(queue.join(format!("{session}.json")), serde_json::json!({"root": root, "host": "here", "session": session}).to_string()).unwrap();
    };
    ticket("done", &parked, "parker");
    ticket("", &handing_in, "handing-in");

    assert_eq!(machine.reclaim(false), vec![ended.join("buck-out"), ended.join("target")]);
    assert!(live.exists() && ended.join("src/lib.rs").is_file() && ended.join(".git").is_dir());
    assert!([&working, &parked, &handing_in].iter().all(|lane| lane.join("target/debug").is_dir() && lane.join("buck-out/v2").is_dir()));
}

#[test]
fn a_recorded_scratch_that_is_a_link_out_of_the_temporary_root_is_left() {
    let machine = Machine::new();
    let outside = machine.dir("elsewhere/results", 3 * DAY);
    machine.session("opencode", "linked", Ended, Path::new("/x"));
    symlink(&outside, machine.path("tmp/north-opencode-linked")).unwrap();
    assert_eq!(machine.reclaim(false), Vec::<PathBuf>::new());
    assert!(outside.exists());
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
