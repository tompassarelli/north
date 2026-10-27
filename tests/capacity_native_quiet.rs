mod capacity_host;

use capacity_host::{Host, Wrapper};
use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{ChildStdin, Stdio};
use std::time::{Duration, Instant};

const SYSTEMD_RUN: &str = "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";
const SYSTEMCTL: &str = "#!/bin/sh\ncase \" $* \" in *\" show \"*) echo inactive ;; esac\n";

impl Host {
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
    let host = Host::new("native-quiet-", &[("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL)]);
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
    assert_eq!(host.status()["queued"][0]["reason"], "DEFER_NATIVE_QUIET", "a held ticket names why it waits");

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
    let host = Host::new("native-quiet-", &[("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL)]);
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

impl Host {
    fn quiet_lease(&self) -> String {
        let leases = fs::read_dir(self.path("agent-capacity-v1/leases")).unwrap();
        leases
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .find(|entry| serde_json::from_str::<Value>(&fs::read_to_string(entry.path()).unwrap()).unwrap()["quiet"] == true)
            .map(|entry| entry.path().file_stem().unwrap().to_string_lossy().into_owned())
            .unwrap()
    }

    fn measure_quiet_lease(&self, idle_seconds: u64) {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let usage = serde_json::json!({ "at": now, "lastBusyAt": now - idle_seconds * 1000, "recentCores": 0.0, "peakCores": 0.0, "meanCores": 0.0 });
        fs::write(self.path(&format!("agent-capacity-v1/leases/{}.usage", self.quiet_lease())), usage.to_string()).unwrap();
    }

    fn probe_moderate(&self) -> Value {
        serde_json::from_slice(&self.helper(&["probe", "--class", "moderate"]).output().unwrap().stdout).unwrap()
    }
}

#[test]
fn a_quiet_lease_idle_past_its_bound_stops_deferring_batch_work_and_status_names_its_owner_and_how_long_it_deferred_others() {
    let host = Host::new("native-quiet-", &[("systemd-run", SYSTEMD_RUN), ("systemctl", SYSTEMCTL)]);
    let (_session, _stdin) = host.native_session("sc-434-t1915", true);

    host.measure_quiet_lease(1);
    let busy = host.probe_moderate();
    assert_eq!((busy["reason"].as_str(), &busy["quietOwners"]), (Some("DEFER_NATIVE_QUIET"), &serde_json::json!(["sc-434-t1915"])));

    let mut waiting = host.batch(&serde_json::json!({ "class": "moderate", "owner": "map-build" }), "60");
    host.until("the batch job to queue", |host| host.queued("moderate") == 1);
    std::thread::sleep(Duration::from_secs(2));
    host.measure_quiet_lease(1);
    let quiet = host.status()["holding"].as_array().unwrap().iter().find_map(|lease| lease.get("quiet").cloned()).unwrap();
    assert_eq!((quiet["owner"].as_str(), quiet["holding"].as_bool(), quiet["deferring"]["jobs"].as_u64()), (Some("sc-434-t1915"), Some(true), Some(1)), "{quiet}");
    assert!(quiet["deferring"]["longestWaitingSeconds"].as_u64().unwrap() >= 2, "{quiet}");
    assert!(waiting.exited().is_none());

    host.measure_quiet_lease(600);
    let stale = host.probe_moderate();
    assert_ne!(stale["reason"], "DEFER_NATIVE_QUIET", "{stale}");
    let mut status = None;
    host.until("the queued batch job to run past the stale quiet lease", |_| {
        status = waiting.exited();
        status.is_some()
    });
    assert!(status.unwrap().success());
    let quiet = host.status()["holding"].as_array().unwrap().iter().find_map(|lease| lease.get("quiet").cloned()).unwrap();
    assert_eq!((quiet["holding"].as_bool(), &quiet["deferring"]), (Some(false), &Value::Null), "{quiet}");
}
