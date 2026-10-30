use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::{Ctx, Die};

const JOURNEY: &str = include_str!("fixtures/journey.json");

fn rig(root: &Path) -> String {
    let stub = root.join("threads.sh");
    crate::foundation::shim::write_script(&stub, &format!("echo \"$*\" >> {0}/threads.log\ncat {0}/rows.json\n", root.display()));
    stub.display().to_string()
}

fn read(path: &Path) -> Value {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

fn run(root: &Path, stub: &str, now: &str, args: &[&str]) -> Result<String, Die> {
    let arguments: Vec<String> = args.iter().map(|word| (*word).to_owned()).collect();
    Ctx::fixture(root, now, stub).main(&arguments)
}

#[test]
fn recorded_steps_of_the_python_laptop_replay_to_the_same_output_and_files() {
    let steps: Vec<Value> = serde_json::from_str(JOURNEY).unwrap();
    let root = tempfile::tempdir().unwrap();
    let stub = rig(root.path());
    for (index, step) in steps.iter().enumerate() {
        fs::write(root.path().join("window.json"), step["window"].to_string()).unwrap();
        fs::write(root.path().join("rows.json"), step["rows"].to_string()).unwrap();
        let args: Vec<&str> = step["args"].as_array().unwrap().iter().map(|word| word.as_str().unwrap()).collect();
        let result = run(root.path(), &stub, step["now"].as_str().unwrap(), &args);
        let (code, stdout, stderr) = match result {
            Ok(out) => (0, out, String::new()),
            Err(die) => (i64::from(die.code), String::new(), format!("laptop: {}\n", die.message)),
        };
        let label = format!("step {index} {args:?}");
        assert_eq!(code, step["rc"].as_i64().unwrap(), "{label}");
        if args.first() == Some(&"status") && args.get(1) == Some(&"--json") {
            let recorded: Value = serde_json::from_str(step["stdout"].as_str().unwrap()).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&stdout).unwrap(), recorded, "{label}");
        } else {
            assert_eq!(stdout, step["stdout"].as_str().unwrap(), "{label}");
        }
        assert_eq!(stderr.replace("north laptop", "laptop"), step["stderr"].as_str().unwrap(), "{label}");
        assert_eq!(read(&root.path().join("placement/laptop.json")), step["state"], "{label}");
        assert_eq!(read(&root.path().join("placement/laptop-observations.json")), step["observations"], "{label}");
    }
}

#[test]
fn drain_is_draining_for_15_minutes_and_notes_the_ledger_item() {
    let root = tempfile::tempdir().unwrap();
    let stub = rig(root.path());
    fs::write(root.path().join("window.json"), json!({"tz": "Etc/GMT-8", "open": "23:00", "close": "10:00"}).to_string()).unwrap();
    fs::write(root.path().join("rows.json"), "[]").unwrap();
    let out = run(root.path(), &stub, "2026-10-13T12:00:00Z", &["drain"]).unwrap();
    assert_eq!(out, "laptop: draining since 20:00, drain by tester@host\n");
    let status = run(root.path(), &stub, "2026-10-13T12:00:00Z", &["status", "--json"]).unwrap();
    let status: Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["deadline"], 1_791_893_700);
    let log = fs::read_to_string(root.path().join("threads.log")).unwrap();
    assert!(log.contains("note x laptop: draining since 20:00, drain by tester@host"), "{log}");
}

#[test]
fn a_ledger_note_that_fails_still_leaves_the_state_written() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("window.json"), "{}").unwrap();
    let out = run(root.path(), "false", "2026-10-13T12:00:00Z", &["open"]).unwrap();
    assert!(out.starts_with("laptop: available since"), "{out}");
    assert_eq!(read(&root.path().join("placement/laptop.json"))["state"], "available");
}

#[test]
fn a_stalled_ssh_forward_fails_with_124_within_its_bound() {
    assert_eq!(super::ctx::FORWARD_WAIT, std::time::Duration::from_secs(90));
    let root = tempfile::tempdir().unwrap();
    let ssh = root.path().join("ssh");
    crate::foundation::shim::write_script(&ssh, "exec sleep 60\n");
    fs::write(root.path().join("remote"), "nexus").unwrap();
    let ctx = Ctx::fixture(root.path(), "2026-10-13T12:00:00Z", "false").stalled(&ssh, std::time::Duration::from_millis(100));
    let started = std::time::Instant::now();
    let died = ctx.forward(&["close".to_owned()]).unwrap_err();
    assert_eq!(died.code, 124);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[test]
fn status_names_why_a_queued_laptop_job_still_waits() {
    let root = tempfile::tempdir().unwrap();
    let stub = rig(root.path());
    fs::write(root.path().join("window.json"), json!({"tz": "UTC", "open": "23:00", "close": "10:00"}).to_string()).unwrap();
    fs::write(root.path().join("rows.json"), "[]").unwrap();
    run(root.path(), &stub, "2026-10-13T12:00:00Z", &["open"]).unwrap();
    let rows = json!([
        {"id": 4, "placement": "nexus", "state": "queued", "host": null, "waiting": "job 4 waits 30m: no runner has polled this ledger for nexus"},
        {"id": 5, "placement": "laptop", "state": "queued", "host": null, "waiting": "job 5 waits 30m: no runner has polled this ledger for laptop"},
    ]);
    fs::write(root.path().join("rows.json"), rows.to_string()).unwrap();
    let out = run(root.path(), &stub, "2026-10-13T12:30:00Z", &["status"]).unwrap();
    assert_eq!(out, "laptop: available since 12:00, open by tester@host (0 running, 1 queued; job 5 waits 30m: no runner has polled this ledger for laptop)\n");
}

const ACTIVE: &str = r#"{"gaming":false,"agent_gpu":false,"sleeping":false,"battery_low":false,"idle":false}"#;

fn observe_active(root: &Path, stub: &str, from: i64, minutes: i64) -> Value {
    for minute in 0..=minutes {
        let now = format!("2026-10-13T{:02}:{:02}:00Z", 20 + (from + minute) / 60, (from + minute) % 60);
        run(root, stub, &now, &["observe", ACTIVE]).unwrap();
    }
    read(&root.join("placement/laptop.json"))
}

fn windowed_root(args: &[&str]) -> (tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let stub = rig(root.path());
    fs::write(root.path().join("window.json"), json!({"tz": "Etc/GMT-8", "open": "23:00", "close": "10:00"}).to_string()).unwrap();
    fs::write(root.path().join("rows.json"), "[]").unwrap();
    run(root.path(), &stub, "2026-10-13T20:00:00Z", args).unwrap();
    (root, stub)
}

#[test]
fn an_open_with_an_unexpired_until_is_not_drained_by_an_active_seat_and_drains_after_it() {
    let (root, stub) = windowed_root(&["open", "until", "06:00", "away"]);
    let held = observe_active(root.path(), &stub, 1, 20);
    assert_eq!(held["state"], "available");
    assert_eq!(held["until"], "2026-10-13T22:00:00Z");
    let drained = observe_active(root.path(), &stub, 120, 10);
    assert_eq!(drained["state"], "draining");
    assert_eq!(drained["reason"], "active");
}

#[test]
fn an_open_without_an_until_is_still_drained_by_an_active_seat() {
    let (root, stub) = windowed_root(&["open", "away"]);
    let drained = observe_active(root.path(), &stub, 1, 10);
    assert_eq!(drained["state"], "draining");
    assert_eq!(drained["reason"], "active");
}
