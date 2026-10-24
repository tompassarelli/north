mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

const BRIEF: &str = "Item: smashcraft#4 LAN check\nSurface: lua\nNeeds: warcraft\n";

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture(root: &Path, trace: &str) {
    let op = root.join("op");
    let agents = root.join(".local/state/agents");
    let profile = root.join(".local/state/north/profiles/claude-a");
    for directory in [&op, &agents, &profile, &root.join("bin"), &root.join("units")] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(op.join("orchestration.toml"), "[placement]\nprivate_repos = []\n[hosts.here]\nserves = [\"warcraft\"]\n").unwrap();
    fs::write(profile.join(".credentials.json"), "{}").unwrap();
    let windows = serde_json::json!([{"window": "5h", "used_pct": 10.0, "burn_pct_h": 1.0, "resets_at": "2026-10-16T16:00:00Z"}]);
    fs::write(agents.join("usage-gate.json"), serde_json::json!({"claude": {"controls": {"claude:a": {"windows": windows}}}}).to_string()).unwrap();
    let setsid = Command::new("sh").args(["-c", "command -v setsid"]).output().unwrap();
    executable(&root.join("bin/setsid"), &format!("#!/bin/sh\nexec {} \"$@\"\n", String::from_utf8_lossy(&setsid.stdout).trim()));
    executable(&root.join("bin/systemd-run"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do [ \"$1\" = --unit ] && unit=$2; shift; done\nshift\necho $$ > \"$FLEET_LAUNCH_FIXTURE/units/$unit\"\nexec \"$@\"\n");
    executable(&root.join("bin/systemctl"), "#!/bin/sh\n[ \"$1 $2\" = '--user stop' ] || exit 0\nunit=\"$FLEET_LAUNCH_FIXTURE/units/$3\"\n[ -f \"$unit\" ] || { echo \"Failed to stop $3: Unit $3 not loaded.\" >&2; exit 5; }\nkill -TERM -- \"-$(cat \"$unit\")\" 2>/dev/null\nrm -f \"$unit\"\n");
    executable(&root.join("bin/claude"), &format!("#!/bin/sh\nexec {} --trace {trace} \"$@\"\n", support::fake_claude()));
}

fn north(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("AGENTS_ORCHESTRATION", root.join("op/orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("PLACEMENT_NOW", "2026-10-16T12:00:00Z")
        .env("RESIDENT_CAP_HOST", "here")
        .env("CLAUDE_RUNTIME", root.join("bin/claude"))
        .env("FLEET_LAUNCH_FIXTURE", root)
        .env("FAKE_CLAUDE_HOOK_MS", "1500")
        .env("FAKE_CLAUDE_SIGNED_OUT_MARK", root.join("signed-out"))
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap()
}

fn launch(root: &Path) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--system-prompt", &brief, "start the box"])
}

fn shown(root: &Path) -> Value {
    let show = north(root, &["fleet", "show", "narrow", "--json"]);
    assert!(show.status.success(), "{}", String::from_utf8_lossy(&show.stderr));
    serde_json::from_slice(&show.stdout).unwrap()
}

#[test]
fn a_launch_returns_once_its_prompt_has_started_the_first_turn_even_behind_slow_session_hooks() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "slow-hooks");
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    let show = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert_eq!(show["status"], "working", "{show}");
    assert!(show.to_string().contains("start the box"), "{show}");
}

#[test]
fn a_launch_whose_first_turn_answers_not_logged_in_restarts_once_and_records_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "signed-out-once");
    let launched = launch(root);
    let show = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert_eq!(show["status"], "working", "{show}");
    let noted = fs::read_to_string(root.join(".local/state/north/fleet/.signed-out-launches.jsonl")).unwrap_or_default();
    let rows: Vec<Value> = noted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), 1, "{noted}");
    assert_eq!((rows[0]["session"].as_str(), rows[0]["account"].as_str()), (Some("narrow"), Some("a")));
}

#[test]
fn sending_to_and_forgetting_a_session_whose_scope_is_gone_say_nothing_about_stopping_it() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    let session = root.join(".local/state/north/fleet/narrow");
    let unit = fs::read_to_string(session.join("host.scope")).unwrap();
    let pid: i32 = fs::read_to_string(session.join("host.pid")).unwrap().trim().parse().unwrap();
    let stopped = Command::new(root.join("bin/systemctl")).args(["--user", "stop", unit.trim()]).env("FLEET_LAUNCH_FIXTURE", root).status().unwrap();
    assert!(stopped.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while Path::new(&format!("/proc/{pid}")).exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let sent = north(root, &["fleet", "send", "narrow", "carry on"]);
    let forgot = north(root, &["fleet", "forget", "narrow"]);
    for (verb, output) in [("send", &sent), ("forget", &forgot)] {
        let said = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{verb}: {said}");
        assert!(!said.contains("Failed to stop"), "{verb}: {said}");
    }
}
