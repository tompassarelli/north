use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{Inputs, Ledger, drain, run_job};
use crate::ops::laptop::hostname;

const THREADS: &str = r#"#!/bin/sh
d=$(dirname "$0")
case "$1 $2 $4" in
"queue --by take") echo "$3 $5" >> "$d/takes.log"; head -n1 "$d/queue.txt"; sed -i 1d "$d/queue.txt" ;;
"queue msg "*) printf '%s|%s|%s\n' "$3" "$4" "$(echo "$5" | head -n1)" >> "$d/msgs.log" ;;
esac
"#;

struct Rig {
    root: tempfile::TempDir,
    inputs: Inputs,
}

fn script(path: &Path, body: &str) {
    crate::foundation::shim::write_script(path, body);
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

impl Rig {
    fn new(jobs: &[String]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        fs::write(base.join("queue.txt"), jobs.iter().map(|job| format!("{job}\n")).collect::<String>()).unwrap();
        script(&base.join("threads.sh"), THREADS.strip_prefix("#!/bin/sh\n").unwrap());
        script(&base.join("north.sh"), &format!("cat {}/state.json 2>/dev/null\n", base.display()));
        script(&base.join("job-run.sh"), "shift 2\nexec \"$@\"\n");
        let inputs = Inputs {
            placement: "laptop".into(),
            poll: Duration::from_millis(200),
            runtime: base.to_owned(),
            logs: base.join("logs"),
            power_supply: base.join("power"),
            ledgers: vec![Ledger { host: None, command: vec![base.join("threads.sh").display().to_string()] }],
            job_run: base.join("job-run.sh").display().to_string(),
            north: base.join("north.sh").display().to_string(),
            bun: "false".into(),
            agents: "false".into(),
        };
        Self { root, inputs }
    }

    fn state(&self, json: &str) {
        fs::write(self.root.path().join("state.json"), json).unwrap();
    }

    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.root.path().join(name)).unwrap_or_default()
    }

    fn done(&self, id: &str) -> String {
        format!("{id}|done|rc=0 log={}:{}/logs/queue-{id}.log\n", hostname(), self.root.path().display())
    }
}

fn job(id: &str, words: &[&str]) -> String {
    serde_json::json!({"id": id, "cmd": words}).to_string()
}

#[test]
fn nothing_is_taken_unless_the_state_is_available_then_the_queue_runs() {
    let mut rig = Rig::new(&[job("1", &["echo", "a"]), job("2", &["echo", "b"])]);
    rig.state(&format!(r#"{{"state":"draining","deadline":{}}}"#, now() + 3600));
    assert_eq!(drain(&rig.inputs), 0);
    assert_eq!(rig.file("takes.log"), "");
    rig.inputs.runtime = rig.root.path().join("second-run");
    fs::create_dir(&rig.inputs.runtime).unwrap();
    rig.state(r#"{"state":"available"}"#);
    assert_eq!(drain(&rig.inputs), 0);
    assert_eq!(rig.file("takes.log"), format!("{0} laptop\n{0} laptop\n{0} laptop\n", hostname()));
    assert_eq!(rig.file("msgs.log"), rig.done("1") + &rig.done("2"));
}

#[test]
fn with_the_ledger_host_unreachable_the_cached_available_state_keeps_the_runner_taking_jobs() {
    let rig = Rig::new(&[job("7", &["echo", "u"])]);
    fs::create_dir_all(&rig.inputs.logs).unwrap();
    fs::write(rig.inputs.logs.join("laptop-state.json"), "{\"state\":\"available\"}\n").unwrap();
    script(&rig.root.path().join("north.sh"), "exit 1\n");
    drain(&rig.inputs);
    assert_eq!(rig.file("msgs.log"), rig.done("7"));
}

#[test]
fn past_the_drain_deadline_the_job_group_is_terminated_and_requeued_and_nothing_more_is_taken() {
    let rig = Rig::new(&[]);
    let pid = rig.root.path().join("pid");
    let long = job("3", &["sh", "-c", &format!("echo $$ > {}; exec sleep 120", pid.display())]);
    fs::write(rig.root.path().join("queue.txt"), format!("{long}\n{}\n", job("4", &["echo", "after"]))).unwrap();
    rig.state(r#"{"state":"available"}"#);
    let state = rig.root.path().join("state.json");
    let flip = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(1));
        fs::write(state, format!("{{\"state\":\"draining\",\"deadline\":{}}}", now())).unwrap();
    });
    let started = Instant::now();
    drain(&rig.inputs);
    flip.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(15));
    assert!(rig.file("msgs.log").starts_with("3|requeued|rc=143 log="), "{}", rig.file("msgs.log"));
    assert!(rig.file("queue.txt").contains("\"id\":\"4\""));
    let child = rig.file("pid").trim().to_owned();
    let gone = (0..20).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        !Path::new(&format!("/proc/{child}")).exists()
    });
    assert!(gone);
}

#[test]
fn a_job_whose_log_cannot_be_created_is_reported_failed_with_the_error() {
    let mut rig = Rig::new(&[]);
    let blocker = rig.root.path().join("not-a-directory");
    fs::write(&blocker, "").unwrap();
    rig.inputs.logs = blocker.join("logs");
    run_job(&rig.inputs, &rig.inputs.ledgers[0], &serde_json::from_str(&job("9", &["echo", "x"])).unwrap());
    assert!(rig.file("msgs.log").starts_with("9|failed|rc=1 log="), "{}", rig.file("msgs.log"));
}

#[test]
fn jobs_on_a_fleet_machine_ledger_are_taken_after_the_local_ones_and_reported_back_to_that_ledger() {
    let mut rig = Rig::new(&[job("1", &["echo", "local"])]);
    let far = rig.root.path().join("far");
    fs::create_dir(&far).unwrap();
    script(&far.join("threads.sh"), THREADS.strip_prefix("#!/bin/sh\n").unwrap());
    fs::write(far.join("queue.txt"), format!("{}\n", job("1", &["echo", "it's far"]))).unwrap();
    let remote = vec!["sh".into(), "-c".into(), "eval \"$*\"".into(), "ssh".into(), far.join("threads.sh").display().to_string()];
    rig.inputs.ledgers.push(Ledger { host: Some("far".into()), command: remote });
    rig.state(r#"{"state":"available"}"#);
    assert_eq!(drain(&rig.inputs), 0);
    assert_eq!(rig.file("msgs.log"), rig.done("1"));
    let log = format!("{}/logs/queue-far-1.log", rig.root.path().display());
    assert_eq!(rig.file("far/msgs.log"), format!("1|done|rc=0 log={}:{log}\n", hostname()));
    assert_eq!(fs::read_to_string(log).unwrap(), "it's far\n");
    assert_eq!(rig.file("far/takes.log"), format!("{0} laptop\n{0} laptop\n", hostname()));
}
