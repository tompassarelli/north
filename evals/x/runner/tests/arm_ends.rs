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

const HELD: &str = r#"#!/bin/sh
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
    echo due > "$fleet/$name/hold"
    "$0" host "$name" "$cwd" > /dev/null 2>&1 &
    echo $! > "$fleet/$name/pid" ;;
  "host "*)
    session=$fleet/$2
    frame() { printf '%s\n' "$1" >> "$session/g1/stream.jsonl"; }
    frame '{"type":"system","subtype":"init"}'
    frame '{"type":"user","message":{"content":"segment the cells"}}'
    frame '{"type":"result","subtype":"success","result":"Done: the script saved 48 masks."}'
    echo idle > "$session/status"
    REAL_SLEEP 0.5
    echo held > "$session/hold"
    REAL_SLEEP 0.5
    frame '{"type":"system","subtype":"init"}'
    REAL_SLEEP 0.3
    [ -e "$session/stopped" ] || printf named > "$3/solution.txt"
    frame '{"type":"result","subtype":"success","result":"Done: the call with named flags printed 48 masks."}' ;;
  "fleet show")
    printf '{"status":"%s","done_hold":"%s","conversation":"c1"}\n' "$(cat "$fleet/$3/status")" "$(cat "$fleet/$3/hold")" ;;
  "fleet send") printf '%s\n' "$6" >> "$fleet/$5/inbox" ;;
  "fleet stop")
    : > "$fleet/$3/stopped"
    kill -KILL "$(cat "$fleet/$3/pid")" 2> /dev/null
    echo exited > "$fleet/$3/status" ;;
esac
exit 0
"#;

const T3: &str = r#"#!/bin/sh
fake=$HOME/fake
transcript=$CLAUDE_CONFIG_DIR/projects/p/s.jsonl
case "$1 $2" in
  "serve --base-dir")
    for root; do :; done
    MKDIR -p "$fake" "$CLAUDE_CONFIG_DIR/projects/p"
    echo "Connection string: none"
    until [ -e "$fake/started" ]; do REAL_SLEEP 0.05; done
    printf '%s\n' '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"Command running in background with ID: b1"}]},"toolUseResult":{"backgroundTaskId":"b1"}}' >> "$transcript"
    echo 1 > "$fake/runs"
    echo null > "$fake/active"
    : > "$fake/ready"
    REAL_SLEEP 1
    echo '"r2"' > "$fake/active"
    echo 2 > "$fake/runs"
    printf '%s\n' '{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\n<task-id>b1</task-id>\n<status>completed</status>\n</task-notification>"}' >> "$transcript"
    REAL_SLEEP 0.3
    printf secret > "$root/solution.txt"
    echo null > "$fake/active"
    exec REAL_SLEEP 600 ;;
  "project add") ;;
  "thread start")
    MKDIR -p "$fake"
    : > "$fake/started"
    until [ -e "$fake/ready" ]; do REAL_SLEEP 0.05; done
    echo '{"threadId":"th1","runId":"r1","status":"completed","reply":"Waiting for the background crack."}' ;;
  "thread show")
    read -r runs < "$fake/runs"
    read -r active < "$fake/active"
    echo "{\"thread\":{\"activeRunId\":$active,\"runCount\":$runs,\"pendingRequestCount\":0},\"messages\":[]}" ;;
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

fn plain_arm(root: &Path) -> Output {
    let [bin, trial, home] = ["bin", "trial", "home"].map(|name| root.join(name));
    for directory in [&bin, &trial, &home, &root.join("cwd")] {
        fs::create_dir_all(directory).unwrap();
    }
    let t3 = bin.join("t3");
    executable(
        &t3,
        &T3.replace("REAL_SLEEP", &on_path("sleep").display().to_string())
            .replace("MKDIR", &on_path("mkdir").display().to_string()),
    );
    executable(&bin.join("claude"), "#!/bin/sh\nexit 0\n");
    executable(&bin.join("sleep"), "#!/bin/sh\nexit 0\n");
    fs::write(root.join("token"), "token").unwrap();
    fs::write(
        trial.join("prompt.json"),
        r#"{"params":{"prompt":[{"type":"text","text":"crack it"}]}}"#,
    )
    .unwrap();
    let mut path = vec![bin];
    path.extend(env::split_paths(&env::var_os("PATH").unwrap()));
    Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../arm"))
        .args(["--arm", "t3", "--account", "c", "--model", "m"])
        .args(["--effort", "medium", "--limit", "60", "--trial"])
        .arg(&trial)
        .arg("--state")
        .arg(root.join("state"))
        .arg("--cwd")
        .arg(root.join("cwd"))
        .env("PATH", env::join_paths(path).unwrap())
        .env("HOME", home)
        .env("T3_RUNTIME", t3)
        .env("EVAL_T_TOKEN_FILE", root.join("token"))
        .env("EVAL_X_RUNNER", env!("CARGO_BIN_EXE_eval-x-runner"))
        .output()
        .unwrap()
}

fn north_arm(root: &Path, host: &str) -> Output {
    let [bin, trial, home] = ["bin", "trial", "home"].map(|name| root.join(name));
    for directory in [&bin, &trial, &home, &root.join("cwd")] {
        fs::create_dir_all(directory).unwrap();
    }
    executable(
        &bin.join("north"),
        &host.replace("REAL_SLEEP", &on_path("sleep").display().to_string()),
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

    let arm = north_arm(&root, HOST);

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

/// Terminal-Bench task crack-7z-hash, 2026-11-12: T3 Code's session put the crack in the background and its first run
/// ended on "I'll report back when it completes"; the arm ended the trial there and stopped T3 Code, so the wake run that
/// T3 Code starts when the background command ends never happened, and no solution file was written.
#[test]
fn a_t3_code_session_waiting_on_its_own_background_command_runs_on_to_the_wake_run() {
    let root = env::temp_dir().join(format!("eval-x-plain-arm-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let [trial, state, cwd] = ["trial", "state", "cwd"].map(|name| root.join(name));

    let arm = plain_arm(&root);

    let log = fs::read_to_string(state.join("arm.log")).unwrap_or_default();
    assert!(
        arm.status.success(),
        "the arm failed: {}{log}",
        String::from_utf8_lossy(&arm.stderr)
    );
    assert_eq!(
        fs::read_to_string(cwd.join("solution.txt")).ok().as_deref(),
        Some("secret"),
        "T3 Code was stopped before its background command ended and woke the session: {log}"
    );
    let record: Value =
        serde_json::from_str(&fs::read_to_string(state.join("arm.json")).unwrap()).unwrap();
    assert_eq!(record["session_end"]["ended"], "idle", "{record}");
    assert_eq!(
        record["session_end"]["turns"], 2,
        "one run per invocation: {record}"
    );
    assert_eq!(record["session_end"]["nudges"], 0, "{record}");
    assert!(trial.join("done.json").exists());
    fs::remove_dir_all(&root).unwrap();
}

/// Terminal-Bench task sam-cell-seg, 2026-11-12 and 2026-11-13: the session's first Done said its script saved 48 masks
/// on a call it chose itself, and the grader's named flags made the script exit 2; north now holds that Done once, so the
/// trial must run on to the report after it, not end on the held one.
#[test]
fn a_session_whose_first_done_north_holds_runs_on_to_the_report_after_it() {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("north-held-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let [state, cwd] = ["state", "cwd"].map(|name| root.join(name));

    let arm = north_arm(&root, HELD);

    let log = fs::read_to_string(state.join("arm.log")).unwrap_or_default();
    assert!(
        arm.status.success(),
        "the arm failed: {}{log}",
        String::from_utf8_lossy(&arm.stderr)
    );
    assert_eq!(
        fs::read_to_string(cwd.join("solution.txt")).ok().as_deref(),
        Some("named"),
        "the trial ended on the Done north held: {log}"
    );
    let record: Value =
        serde_json::from_str(&fs::read_to_string(state.join("arm.json")).unwrap()).unwrap();
    assert_eq!(record["session_end"]["ended"], "report", "{record}");
    assert_eq!(record["session_end"]["turns"], 2, "{record}");
    assert_eq!(record["session_end"]["holds"], 1, "{record}");
    assert_eq!(record["session_end"]["nudges"], 0, "{record}");
    fs::remove_dir_all(&root).unwrap();
}
