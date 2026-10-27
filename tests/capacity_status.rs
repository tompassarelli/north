use std::process::Command;

#[test]
fn capacity_status_writes_json_to_stdout() {
    let scratch = tempfile::tempdir().unwrap();
    let bundle = std::env::current_dir().unwrap().join("harness/skills/machine-capacity/scripts/machine-capacity.mjs");
    let output = Command::new("bun")
        .arg(bundle)
        .arg("status")
        .env("XDG_RUNTIME_DIR", scratch.path())
        .env("XDG_STATE_HOME", scratch.path())
        .env("AGENT_CAPACITY_NORTH", env!("CARGO_BIN_EXE_north"))
        .env("AGENT_CAPACITY_DB", scratch.path().join("capacity.db"))
        .env("CAPACITY_WATCHDOG_DIR", scratch.path().join("no-watchdog"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(status["holding"].is_array(), "{status}");
    assert!(output.stderr.is_empty());
}
