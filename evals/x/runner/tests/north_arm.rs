use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const HOST: &str = r#"#!/bin/sh
fleet=$XDG_STATE_HOME/north/fleet
case "$1 $2" in
  "fleet launch")
    shift 2
    while [ $# -gt 1 ]; do
      case $1 in --name) name=$2 ;; --cwd) cwd=$2 ;; esac
      shift
    done
    mkdir -p "$fleet/$name/g1"
    echo working > "$fleet/$name/status"
    "$0" host "$name" "$cwd" > /dev/null 2>&1 &
    echo $! > "$fleet/$name/pid" ;;
  "host "*)
    session=$fleet/$2
    frame() { printf '%s\n' "$1" >> "$session/g1/stream.jsonl"; }
    frame '{"type":"system","subtype":"init"}'
    frame '{"type":"user","message":{"content":"crack it"}}'
    frame '{"type":"system","subtype":"task_started","task_id":"b1","is_backgrounded":true}'
    frame '{"type":"assistant","message":{"content":[{"type":"text","text":"Waiting for the background crack."}]}}'
    REAL_SLEEP 0.2
    frame '{"type":"result","subtype":"success","result":"Waiting for the background crack to complete."}'
    echo idle > "$session/status"
    REAL_SLEEP 1
    frame '{"type":"system","subtype":"task_notification","task_id":"b1","status":"completed"}'
    frame '{"type":"system","subtype":"init"}'
    REAL_SLEEP 0.3
    frame '{"type":"result","subtype":"success","result":"The password is found; the solution file comes next."}'
    until [ -s "$session/inbox" ]; do REAL_SLEEP 0.05; done
    frame '{"type":"system","subtype":"init"}'
    [ -e "$session/stopped" ] || printf secret > "$3/solution.txt"
    frame '{"type":"result","subtype":"success","result":"**Done:** solution.txt holds the password."}' ;;
  "fleet show")
    printf '{"status":"%s","conversation":"c1","entries":[{"kind":"user","text":"crack it"},{"kind":"assistant","text":"Waiting for the background crack."}]}\n' "$(cat "$fleet/$3/status")" ;;
  "fleet send") printf '%s\n' "$6" >> "$fleet/$5/inbox" ;;
  "fleet stop")
    : > "$fleet/$3/stopped"
    kill -KILL "$(cat "$fleet/$3/pid")" 2> /dev/null
    echo exited > "$fleet/$3/status" ;;
esac
exit 0
"#;

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn on_path(program: &str) -> PathBuf {
    env::split_paths(&env::var_os("PATH").unwrap())
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .unwrap()
}

fn north_arm(root: &Path) -> Output {
    let [bin, trial, home] = ["bin", "trial", "home"].map(|name| root.join(name));
    for directory in [&bin, &trial, &home, &root.join("cwd")] {
        fs::create_dir_all(directory).unwrap();
    }
    executable(
        &bin.join("north"),
        &HOST.replace("REAL_SLEEP", &on_path("sleep").display().to_string()),
    );
    executable(&bin.join("sleep"), "#!/bin/sh\nexit 0\n");
    fs::write(
        trial.join("prompt.json"),
        r#"{"params":{"prompt":[{"type":"text","text":"crack it"}]}}"#,
    )
    .unwrap();
    let mut path = vec![bin];
    path.extend(env::split_paths(&env::var_os("PATH").unwrap()));
    Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../arm"))
        .args(["--arm", "north", "--account", "c", "--model", "m"])
        .args(["--effort", "medium", "--limit", "60", "--trial"])
        .arg(&trial)
        .arg("--state")
        .arg(root.join("state"))
        .arg("--cwd")
        .arg(root.join("cwd"))
        .env("PATH", env::join_paths(path).unwrap())
        .env("HOME", home)
        .env("EVAL_X_RUNNER", env!("CARGO_BIN_EXE_eval-x-runner"))
        .output()
        .unwrap()
}

/// Terminal-Bench task crack-7z-hash, 2026-11-12: north's session put the crack in the background and ended its turn on
/// "Waiting for the background john crack to complete."; the arm ended the trial there, so the session was stopped
/// before the command's end could invoke it again, and no solution file was written.
#[test]
fn a_session_waiting_on_its_own_background_command_runs_on_to_its_report() {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("north-arm-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let [trial, state, cwd] = ["trial", "state", "cwd"].map(|name| root.join(name));

    let arm = north_arm(&root);

    let log = fs::read_to_string(state.join("arm.log")).unwrap_or_default();
    assert!(
        arm.status.success(),
        "the arm failed: {}{log}",
        String::from_utf8_lossy(&arm.stderr)
    );
    assert_eq!(
        fs::read_to_string(cwd.join("solution.txt")).ok().as_deref(),
        Some("secret"),
        "the session was stopped before its background command ended and invoked it again: {log}"
    );
    let record: Value =
        serde_json::from_str(&fs::read_to_string(state.join("arm.json")).unwrap()).unwrap();
    assert_eq!(record["session_end"]["ended"], "report", "{record}");
    assert_eq!(
        record["session_end"]["turns"], 3,
        "one turn per invocation: {record}"
    );
    assert_eq!(
        record["session_end"]["nudges"], 1,
        "a turn that ended with no report and no command running is nudged once: {record}"
    );
    assert!(trial.join("done.json").exists());
    fs::remove_dir_all(&root).unwrap();
}
