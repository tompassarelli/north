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
    assert_eq!(steps.len(), 190);
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

fn minute(start: i64, minutes: i64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds(start + minutes * 60).iso()
}

#[test]
fn the_recorded_laptop_auto_observation_journey_holds_gaming_idle_and_window_states() {
    let root = tempfile::tempdir().unwrap();
    let stub = rig(root.path());
    fs::write(root.path().join("rows.json"), "[]").unwrap();
    fs::write(root.path().join("window.json"), json!({"tz": "UTC", "open": "23:00", "close": "10:00"}).to_string()).unwrap();
    let start = 1_791_936_000;
    let state = || read(&root.path().join("placement/laptop.json"));
    let run = |minutes: i64, args: &[&str]| {
        run(root.path(), &stub, &minute(start, minutes), args).unwrap();
        state()
    };
    let observe = |minutes: i64, sample: Value| {
        let mut full = json!({"gaming": false, "idle": null, "gpu_busy": null, "agent_gpu": false, "battery_low": false, "sleeping": false});
        for (key, value) in sample.as_object().unwrap() {
            full[key] = value.clone();
        }
        run(minutes, &["observe", &full.to_string()])
    };
    run(0, &["open"]);
    let gaming = observe(0, json!({"gaming": true}));
    assert!(gaming["state"] == "unavailable" && gaming["reason"] == "gaming" && gaming["grace"] == 120);
    assert_eq!(observe(1, json!({"gaming": true}))["since"], gaming["since"]);
    for at in 2..12 {
        assert_eq!(observe(at, json!({}))["state"], "unavailable");
    }
    assert_eq!(observe(12, json!({}))["state"], "available");
    run(13, &["close", "manual"]);
    run(13, &["tick"]);
    for at in 14..45 {
        let held = observe(at, json!({"idle": true}));
        assert!(held["state"] == "unavailable" && held["reason"] == "drained (manual)");
    }
    run(45, &["open"]);
    for at in 45..50 {
        assert_eq!(observe(at, json!({"idle": false}))["state"], "available");
    }
    let active = observe(50, json!({"idle": false}));
    assert!(active["state"] == "draining" && active["reason"] == "active" && active["grace"] == 900);
    run(50, &["tick"]);
    for at in 51..81 {
        assert_eq!(observe(at, json!({"idle": true}))["state"], "unavailable");
    }
    assert_eq!(observe(81, json!({"idle": true}))["state"], "available");
    for at in 82..88 {
        assert_eq!(observe(at, json!({"gpu_busy": 75, "agent_gpu": true}))["state"], "available");
    }
    for at in 88..91 {
        assert_eq!(observe(at, json!({"gpu_busy": 75}))["state"], "available");
    }
    assert_eq!(observe(91, json!({"gpu_busy": 75}))["reason"], "gaming");
    run(92, &["open"]);
    observe(92, json!({"gpu_busy": 75, "agent_gpu": true}));
    observe(93, json!({"gpu_busy": 75}));
    observe(94, json!({"gpu_busy": 75}));
    assert_eq!(observe(100, json!({"gpu_busy": 75}))["state"], "available");
    run(600, &["tick"]);
    for at in 601..631 {
        assert_eq!(observe(at, json!({"idle": true}))["state"], "unavailable");
    }
    run(1374, &["close", "manual"]);
    for at in 1374..1380 {
        assert_eq!(observe(at, json!({"idle": false}))["state"], "unavailable");
    }
    assert_eq!(run(1380, &["tick"])["state"], "available");
    for at in 1380..1385 {
        assert_eq!(observe(at, json!({"idle": false}))["state"], "available");
    }
    let last = observe(1385, json!({"idle": false}));
    assert!(last["state"] == "draining" && last["reason"] == "active" && last["grace"] == 900);
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
