use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn command(root: &Path, verb: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["fleet", verb, "recorded"])
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("NORTH_FLEET_SESSION", "")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .env("SCOPE_RECORDING", root)
        .output()
        .unwrap()
}

#[test]
fn forget_waits_for_scope_cleanup_even_after_the_host_exits() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let session = root.join(".local/state/north/fleet/recorded");
    fs::create_dir_all(&session).unwrap();
    fs::create_dir(root.join("bin")).unwrap();
    let controller = root.join("bin/systemctl");
    fs::write(&controller, "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$SCOPE_RECORDING/calls\"\nexit \"$(cat \"$SCOPE_RECORDING/exit\")\"\n").unwrap();
    fs::set_permissions(&controller, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(session.join("host.scope"), "north-session-recorded.scope").unwrap();
    fs::write(session.join("session.json"), serde_json::to_vec(&json!({
        "name": "recorded", "provider": "claude", "account": "fixture", "cwd": root,
        "model": null, "created_ms": 1, "host_pid": null, "conversation": null,
        "host_dir": session, "stopped": false
    })).unwrap()).unwrap();
    fs::write(root.join("exit"), "1").unwrap();
    let refused = command(root, "forget");
    assert!(!refused.status.success(), "forget accepted failed scope cleanup");
    let record: Value = serde_json::from_slice(&fs::read(session.join("session.json")).unwrap()).unwrap();
    assert_eq!(record["stopped"], false);
    fs::write(root.join("exit"), "0").unwrap();
    let stopped = command(root, "stop");
    assert!(stopped.status.success(), "{}", String::from_utf8_lossy(&stopped.stderr));
    assert!(session.exists());
    fs::write(root.join("exit"), "5").unwrap();
    let forgotten = command(root, "forget");
    assert!(forgotten.status.success(), "{}", String::from_utf8_lossy(&forgotten.stderr));
    assert!(!session.exists());
    assert_eq!(fs::read_to_string(root.join("calls")).unwrap(), "--user stop north-session-recorded.scope\n".repeat(3));
}
