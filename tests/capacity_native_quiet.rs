use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const SYSTEMD_RUN: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";
const SYSTEMCTL: &str = "#!/bin/sh\ncase \" $* \" in *\" show \"*) echo inactive ;; esac\n";

struct Wrapper(Child);

impl Drop for Wrapper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Wrapper {
    fn exited(&mut self) -> Option<ExitStatus> {
        self.0.try_wait().unwrap()
    }

    fn stderr(&mut self) -> String {
        let mut text = String::new();
        self.0.stderr.take().unwrap().read_to_string(&mut text).unwrap();
        text
    }
}

struct Host {
    scratch: tempfile::TempDir,
}

impl Host {
    fn new() -> Self {
        let scratch = tempfile::Builder::new().prefix("native-quiet-").tempdir().unwrap();
        let shims = scratch.path().join("bin");
        fs::create_dir_all(&shims).unwrap();
        for (name, body) in [("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL)] {
            fs::write(shims.join(name), body).unwrap();
            fs::set_permissions(shims.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::create_dir_all(scratch.path().join("agent-capacity-v1")).unwrap();
        fs::write(scratch.path().join("agent-capacity-v1/mode"), "away\n").unwrap();
        fs::write(scratch.path().join("pressure"), "some avg10=0.00 avg60=0.00 avg300=0.00 total=0\n").unwrap();
        fs::write(scratch.path().join("gpu"), "0\n").unwrap();
        Self { scratch }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.scratch.path().join(name)
    }

    fn helper(&self, arguments: &[&str]) -> Command {
        let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/skills/machine-capacity/scripts/machine-capacity.mjs");
        let mut command = Command::new("bun");
        command.arg(bundle).args(arguments);
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap_or_default());
        for (key, value) in [
            ("PATH", path),
            ("XDG_RUNTIME_DIR", self.path("").display().to_string()),
            ("XDG_STATE_HOME", self.path("state").display().to_string()),
            ("AGENT_CAPACITY_DB", self.path("capacity.db").display().to_string()),
            ("AGENT_CAPACITY_USAGE_LOG", self.path("usage.jsonl").display().to_string()),
            ("AGENT_CAPACITY_CGROUP_ROOT", self.path("cgroup").display().to_string()),
            ("AGENT_CAPACITY_CPU_PRESSURE", self.path("pressure").display().to_string()),
            ("AGENT_CAPACITY_GPU_BUSY", self.path("gpu").display().to_string()),
            ("AGENT_CAPACITY_GPU_CLIENTS", "0".to_owned()),
            ("AGENT_CAPACITY_CORES", "24".to_owned()),
            ("AGENT_CAPACITY_NORTH", env!("CARGO_BIN_EXE_north").to_owned()),
            ("CAPACITY_WATCHDOG_DIR", self.path("no-watchdog").display().to_string()),
        ] {
            command.env(key, value);
        }
        command
    }

    fn leases(&self, class: &str) -> usize {
        fs::read_dir(self.path("agent-capacity-v1/leases")).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
                .filter_map(|entry| fs::read_to_string(entry.path()).ok())
                .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
                .filter(|lease| lease["class"] == class || (class == "quiet" && lease["quiet"] == true))
                .count()
        })
    }

    fn until(&self, what: &str, mut done: impl FnMut(&Self) -> bool) -> Instant {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(self) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
        Instant::now()
    }

    fn native_session(&self, owner: &str, quiet: bool) -> (Wrapper, ChildStdin) {
        let live = self.leases("native");
        let mut child = self
            .helper(&[&["session", "--class", "native", "--new-clients", "0", "--owner", owner][..], if quiet { &["--quiet"][..] } else { &[][..] }, &["--", "cat"]].concat())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let session = Wrapper(child);
        self.until("the native lease", |host| host.leases("native") == live + 1);
        (session, stdin)
    }

    fn batch(&self, recorded: &Value, timeout: &str) -> Wrapper {
        let class = recorded["class"].as_str().unwrap();
        let owner = recorded["owner"].as_str().unwrap();
        let command = ["run", "--class", class, "--owner", owner, "--timeout-seconds", timeout, "--", "true"];
        Wrapper(self.helper(&command).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap())
    }

    fn last_usage(&self) -> Value {
        let usage = fs::read_to_string(self.path("usage.jsonl")).unwrap();
        serde_json::from_str(usage.lines().last().unwrap()).unwrap()
    }
}

fn recorded() -> Value {
    let text = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-quiet.recorded.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn recorded_batch_lease_waits_for_the_native_session_and_starts_within_a_second_of_its_end() {
    let recorded = recorded();
    let (native, batch) = (&recorded["native"], &recorded["batch"]);
    let batch_class = batch["class"].as_str().unwrap();
    let host = Host::new();
    let (_pool, _pool_stdin) = host.native_session("wisp-lan-pair-0", false);
    let (_session, stdin) = host.native_session(native["owner"].as_str().unwrap(), true);

    let light = Instant::now();
    let reserved = host.helper(&["reserve", "--class", "agent", "--owner", "fixture-agent", "--timeout-seconds", "60"]).output().unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&reserved.stdout).unwrap()["decision"], "RESERVED");
    assert!(light.elapsed() < Duration::from_secs(1), "a light lease waited {:?}", light.elapsed());

    let mut waiting = host.batch(batch, "60");
    std::thread::sleep(Duration::from_secs(3));
    assert!(waiting.exited().is_none(), "the batch lease ran beside a live native session");
    assert_eq!(host.leases(batch_class), 0);

    drop(stdin);
    let ended = host.until("the quiet native lease to end", |host| host.leases("quiet") == 0);
    let admitted = host.until("the batch lease", |host| host.leases(batch_class) == 1 || host.path("usage.jsonl").exists());
    assert!(admitted - ended < Duration::from_secs(1), "the batch lease started {:?} after the native lease ended", admitted - ended);
    let mut status = None;
    host.until("the batch run", |_| {
        status = waiting.exited();
        status.is_some()
    });
    assert!(status.unwrap().success());
    let stderr = waiting.stderr();
    let queued = BufReader::new(stderr.as_bytes())
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find(|line| line["decision"] == "QUEUED")
        .expect("a QUEUED line");
    assert_eq!(queued["reason"], "DEFER_NATIVE_QUIET");
    assert_eq!(queued["quietOwners"], serde_json::json!([native["owner"]]));
    assert_eq!(host.leases("native"), 1, "the idle pool lease stays live");
    let entry = host.last_usage();
    assert_eq!(entry["owner"], batch["owner"]);
    assert!(entry["queuedSeconds"].as_f64().unwrap() >= 2.0, "{entry}");
}

#[test]
fn batch_lease_outlasted_by_a_native_session_is_refused_naming_its_owner() {
    let recorded = recorded();
    let native_owner = recorded["native"]["owner"].as_str().unwrap();
    let host = Host::new();
    let _session = host.native_session(native_owner, true);
    let mut waiting = host.batch(&recorded["batch"], "2");
    let mut status = None;
    host.until("the refusal", |_| {
        status = waiting.exited();
        status.is_some()
    });
    assert_eq!(status.unwrap().code(), Some(75));
    let stderr = waiting.stderr();
    assert!(stderr.contains(&format!("native evidence run {native_owner} is live")), "{stderr}");

    let beside = host
        .helper(&["run", "--class", "moderate", "--owner", "native-build", "--beside-native", native_owner, "--timeout-seconds", "60", "--", "true"])
        .output()
        .unwrap();
    assert!(beside.status.success(), "{}", String::from_utf8_lossy(&beside.stderr));
    assert_eq!(host.last_usage()["besideNative"], native_owner);
}
