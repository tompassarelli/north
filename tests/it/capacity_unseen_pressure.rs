use crate::capacity_host;

use capacity_host::{Exclusive, Host};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const RECORDED: &str = include_str!("../fixtures/capacity-unseen-pressure.recorded.json");

struct Recorded {
    host: Host,
}

impl Recorded {
    fn new() -> Self {
        let host = Host::new("capacity-unseen-pressure-", &[]);
        fs::remove_file(host.path("agent-capacity-v1/mode")).unwrap();
        Self { host }
    }

    fn helper(&self, arguments: &[&str]) -> Value {
        self.run(arguments, &self.host.path(""))
    }

    fn run(&self, arguments: &[&str], cwd: &Path) -> Value {
        let output = self
            .host
            .command(env!("CARGO_BIN_EXE_north"))
            .current_dir(cwd)
            .args(["capacity", "host"])
            .args(arguments)
            .env_remove("AGENT_CAPACITY_CPU_PRESSURE")
            .env("AGENT_CAPACITY_CORES", "20")
            .env("HOME", self.host.path("home"))
            .output_exclusive()
            .unwrap();
        assert!(matches!(output.status.code(), Some(0 | 75)), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn estimate(&self, command: &[&str]) -> Value {
        let checkout = self.host.path("home/code/north");
        fs::create_dir_all(&checkout).unwrap();
        self.run(&[&["estimate", "--json", "--"], command].concat(), &checkout)
    }

    fn probe(&self, class: &str) -> String {
        self.helper(&["probe", "--class", class])["reason"].as_str().unwrap().to_owned()
    }

    fn manager(&self) -> PathBuf {
        let uid = rustix::process::getuid().as_raw();
        self.host.path(&format!("cgroup/user.slice/user-{uid}.slice/user@{uid}.service"))
    }

    fn calm(&self, non_leased: f64) {
        psi(&self.host.path("cgroup/system.slice"), non_leased);
        for slice in ["app", "background", "session", "native"] {
            psi(&self.manager().join(format!("{slice}.slice")), non_leased);
        }
    }
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

fn psi(path: &Path, some_avg10: f64) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join("cpu.pressure"), format!("some avg10={some_avg10} avg60=0 avg300=0 total=0\n")).unwrap();
}

#[test]
fn an_unseen_expensive_command_is_sized_by_its_class_even_beside_history_of_its_family() {
    let recorded: Value = serde_json::from_str(RECORDED).unwrap();
    let host = Recorded::new();
    let cheap = host.estimate(&["python3", "x.py"]);
    assert_eq!((&cheap["class"], &cheap["confidence"], &cheap["sampleCount"]), (&"moderate".into(), &"low".into(), &0.into()));
    let repo = cheap["signature"]["repo"].clone();
    assert!(repo.is_string(), "the estimate runs inside a ~/code/REPO checkout so family-repo history can match: {cheap}");
    let db = rusqlite::Connection::open(host.host.path("capacity.db")).unwrap();
    let insert = |id: &str, at: &str, family: &str, package: &str, class: &str| {
        db.execute(
            "INSERT INTO capacity_runs (source_id, at, family, repo, package, class, signature_exact, meanCores, p90Cores, peakMemoryMiB) VALUES (?, ?, ?, ?, ?, ?, 1, 0.4, 0.5, 128)",
            rusqlite::params![id, at, family, repo.as_str(), package, class],
        )
        .unwrap();
    };
    for item in recorded["unseen"].as_array().unwrap() {
        let family = item["family"].as_str().unwrap();
        for index in 0..3 {
            insert(&format!("{family}:{index}"), &format!("2026-10-11T00:00:0{index}Z"), family, "already-seen", "heavy");
        }
        let command: Vec<&str> = item["command"].as_array().unwrap().iter().map(|word| word.as_str().unwrap()).collect();
        let estimate = host.estimate(&command);
        let expected_p90 = if item["class"] == "native" { 2.0 } else { 6.0 };
        assert_eq!((&estimate["signature"]["family"], &estimate["class"], &estimate["confidence"]), (&item["family"], &item["class"], &"low".into()), "{command:?}");
        assert_eq!((&estimate["sampleCount"], &estimate["matchLevel"], number(&estimate["cores"]["p90"])), (&0.into(), &"default".into(), expected_p90), "{command:?}");
    }
    insert("known-cargo-target", "2026-10-11T00:00:04Z", "cargo build", "known-target", "heavy");
    let known = host.estimate(&["cargo", "build", "-p", "known-target"]);
    assert_eq!((&known["matchLevel"], &known["confidence"], number(&known["cores"]["p90"])), (&"family-repo".into(), &"medium".into(), 0.5));
    for index in 0..3 {
        insert(&format!("cheap-family:{index}"), &format!("2026-10-11T00:00:0{index}Z"), "other", "seen.py", "moderate");
    }
    assert_eq!(host.estimate(&["python3", "x.py"])["matchLevel"], "family-repo");
}

#[test]
fn a_leased_scopes_own_quota_stall_holds_nothing_and_unleased_or_protected_pressure_does() {
    let recorded: Value = serde_json::from_str(RECORDED).unwrap();
    let pressure = &recorded["pressure"];
    let host = Recorded::new();
    let (root, manager) = (host.host.path("cgroup"), host.manager());
    let non_leased = number(&pressure["nonLeasedSomeAvg10"]);
    psi(&root, number(&pressure["systemSomeAvg10"]));
    host.calm(non_leased);
    for scope in ["agent.slice", "agent.slice/agent-capacity.slice", "agent.slice/agent-capacity.slice/agent-capacity-recorded.scope"] {
        psi(&manager.join(scope), number(&pressure["leasedScopeSomeAvg10"]));
    }
    let unleased = manager.join("agent.slice/agent-bounded.slice");
    psi(&unleased, non_leased);
    for class in ["moderate", "heavy"] {
        let baseline = host.helper(&["probe", "--class", class]);
        assert_eq!((&baseline["reason"], number(&baseline["cpuSomeAvg10"]), &baseline["queuedAhead"]), (&"RUN".into(), non_leased, &0.into()), "{class}");
    }
    psi(&unleased, number(&pressure["unleasedOverloadSomeAvg10"]));
    assert_eq!(host.probe("heavy"), "DEFER_RUN_QUEUE");
    psi(&unleased, 24.99);
    assert_eq!(host.probe("heavy"), "RUN", "heavy holds from the attended high mark of 25%");
    psi(&unleased, 30.01);
    assert_eq!((host.probe("heavy"), host.probe("moderate")), ("DEFER_RUN_QUEUE".to_owned(), "DEFER_CPU_PRESSURE".to_owned()), "moderate holds from 30%");
    psi(&unleased, non_leased);
    psi(&manager.join("session.slice"), number(&pressure["protectedOverloadSomeAvg10"]));
    assert_eq!(host.probe("moderate"), "DEFER_INTERACTIVE_PRESSURE");
}

#[test]
fn a_busy_gpu_defers_native_clients_and_its_busy_since_persists_until_it_drops_below_85_percent() {
    let host = Recorded::new();
    psi(&host.host.path("cgroup"), 0.0);
    host.calm(0.0);
    let native = || host.helper(&["probe", "--class", "native", "--new-clients", "0"]);
    let since_file = host.host.path("agent-capacity-v1/gpu-busy-since");
    fs::write(host.host.path("gpu"), "99\n").unwrap();
    let deferred = native();
    assert_eq!((&deferred["reason"], number(&deferred["gpuBusyPercent"]), &deferred["gpuProcess"]), (&"DEFER_GPU_BUSY".into(), 99.0, &Value::Null));
    assert!(number(&deferred["gpuBusySeconds"]) >= 0.0);
    assert_eq!(fs::read_to_string(&since_file).unwrap().trim().parse::<f64>().unwrap(), number(&deferred["gpuBusySince"]));
    let persisted = number(&deferred["gpuBusySince"]) - 30000.0;
    fs::write(&since_file, persisted.to_string()).unwrap();
    let repeated = native();
    assert_eq!(number(&repeated["gpuBusySince"]), persisted);
    assert!(number(&repeated["gpuBusySeconds"]) >= 30.0);
    fs::write(host.host.path("gpu"), "84\n").unwrap();
    assert_eq!(native()["reason"], "RUN");
    assert!(!since_file.exists());
}
