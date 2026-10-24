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
    north_on(root, "here", args)
}

fn north_on(root: &Path, host: &str, args: &[&str]) -> Output {
    north_as(root, host, &[], args)
}

fn north_as(root: &Path, host: &str, sender: &[(&str, &str)], args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("AGENTS_ORCHESTRATION", root.join("op/orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("PLACEMENT_NOW", "2026-10-16T12:00:00Z")
        .env("RESIDENT_CAP_HOST", host)
        .env("CLAUDE_RUNTIME", root.join("bin/claude"))
        .env("FLEET_LAUNCH_FIXTURE", root)
        .env("PLACEMENT_HOSTS", root.join("placement-hosts.json"))
        .env("FAKE_CLAUDE_HOOK_MS", "1500")
        .env("FAKE_CLAUDE_SIGNED_OUT_MARK", root.join("signed-out"))
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .envs(sender.iter().copied())
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap()
}

fn launch(root: &Path) -> Output {
    launch_on(root, "here")
}

fn launch_on(root: &Path, host: &str) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    let brief = brief.display().to_string();
    north_on(root, host, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--system-prompt", &brief, "start the box"])
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
    let noted = fs::read_to_string(root.join(".local/state/north/fleet/.credential-failures.jsonl")).unwrap_or_default();
    let rows: Vec<Value> = noted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), 1, "{noted}");
    assert_eq!((rows[0]["session"].as_str(), rows[0]["account"].as_str()), (Some("narrow"), Some("a")));
}

#[test]
fn a_launch_whose_first_turn_answers_oauth_session_expired_is_recorded_and_refused_naming_the_account_without_a_restart() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "oauth-expired");
    let launched = launch(root);
    north(root, &["fleet", "forget", "narrow"]);
    let said = String::from_utf8_lossy(&launched.stderr);
    assert!(!launched.status.success(), "{said}");
    assert!(said.contains("narrow on claude:a answered its first turn with 'OAuth session expired'"), "{said}");
    let noted = fs::read_to_string(root.join(".local/state/north/fleet/.credential-failures.jsonl")).unwrap_or_default();
    let rows: Vec<Value> = noted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(rows.len(), 1, "{noted}");
    assert_eq!((rows[0]["account"].as_str(), rows[0]["failure"].as_str()), (Some("a"), Some("oauth-expired")));
}

#[test]
fn a_host_holding_a_copy_of_another_hosts_login_refuses_to_launch_it_naming_the_owner_while_the_owner_launches() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let policy = root.join("op/orchestration.toml");
    let declared = fs::read_to_string(&policy).unwrap() + "[hosts.copy]\nserves = [\"warcraft\"]\n[accounts.claude]\nprofiles = { a = { here = {} } }\n";
    fs::write(&policy, declared).unwrap();

    let copy = launch_on(root, "copy");
    let listed = north_on(root, "copy", &["fleet", "accounts"]);
    let said = String::from_utf8_lossy(&copy.stderr);
    assert!(!copy.status.success(), "{said}");
    assert!(said.contains("claude:a's login belongs to here"), "{said}");
    assert!(!north_on(root, "copy", &["fleet", "show", "narrow"]).status.success(), "the copy host started a session on a");
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("claude\ta"), "{}", String::from_utf8_lossy(&listed.stdout));

    let owner = launch(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert!(owner.status.success(), "{}", String::from_utf8_lossy(&owner.stderr));
}

#[test]
fn a_launch_whose_cwd_names_another_machine_runs_there_on_a_label_only_that_machine_owns() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let policy = root.join("op/orchestration.toml");
    let declared = fs::read_to_string(&policy).unwrap() + "[hosts.far]\nssh = \"far\"\nresident_cap = 4\nserves = []\n[accounts.claude]\nprofiles = { a = { far = {} } }\n";
    fs::write(&policy, declared).unwrap();
    fs::write(root.join("placement-hosts.json"), serde_json::json!({"far": {"online": true, "live": 0, "accounts": ["a"]}}).to_string()).unwrap();
    executable(&root.join("bin/ssh"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec sh -c \"$*\"\n");
    let checkout = root.join("far-checkout");
    fs::create_dir_all(&checkout).unwrap();
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();

    let cwd = format!("far:{}", checkout.display());
    let launched = north(root, &["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--cwd", &cwd, "--system-prompt", &brief.display().to_string(), "start the box"]);
    let show = shown(root);
    north(root, &["fleet", "forget", "narrow"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    assert_eq!((show["machine"].as_str(), show["status"].as_str()), (Some("far"), Some("working")), "{show}");
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

#[test]
fn a_send_names_its_sender_in_the_receivers_transcript_and_warns_a_second_sender_about_the_first() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));

    let first = north_as(root, "here", &[("CLAUDE_CODE_SESSION_ID", "28c1c1d1")], &["fleet", "send", "narrow", "staff every ready item"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut shown = String::new();
    while !shown.contains("[from 28c1c1d1] staff every ready item") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        shown = String::from_utf8_lossy(&north(root, &["fleet", "show", "narrow"]).stdout).into_owned();
    }
    let second = north_as(root, "here", &[("NORTH_FLEET_SESSION", "proxy")], &["fleet", "send", "narrow", "launch nothing on a"]);
    let warned = String::from_utf8_lossy(&second.stderr).into_owned();
    north(root, &["fleet", "forget", "narrow"]);

    assert!(first.status.success() && second.status.success(), "{}{warned}", String::from_utf8_lossy(&first.stderr));
    assert!(!String::from_utf8_lossy(&first.stderr).contains("also messaged"), "{}", String::from_utf8_lossy(&first.stderr));
    assert!(shown.contains("user: [from 28c1c1d1] staff every ready item"), "{shown}");
    assert!(warned.contains("28c1c1d1 also messaged narrow 0m ago"), "{warned}");
}

#[test]
fn a_letter_relayed_from_another_host_names_its_sender_and_host_and_records_them() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root, "fold");
    let far = root.join("far");
    fs::create_dir_all(far.join(".local/state")).unwrap();
    executable(&root.join("bin/ssh"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nHOME=\"$FAR_HOME\" XDG_STATE_HOME=\"$FAR_HOME/.local/state\" exec sh -c \"$1\"\n");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("bin/north")).unwrap();
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));

    let far_home = far.to_str().unwrap();
    let far_state = format!("{far_home}/.local/state");
    north_as(root, "here", &[("HOME", far_home), ("XDG_STATE_HOME", &far_state)], &["fleet", "outbox", "take", "--for", "here"]);
    let posted = north_as(root, "here", &[("HOME", far_home), ("XDG_STATE_HOME", &far_state), ("NORTH_FLEET_SESSION", "wisp-lead")], &["fleet", "send", "narrow", "rolled back"]);
    let relayed = north_as(root, "here", &[("FAR_HOME", far_home)], &["fleet", "relay", "far"]);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut shown = String::new();
    while !shown.contains("rolled back") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
        shown = String::from_utf8_lossy(&north(root, &["fleet", "show", "narrow"]).stdout).into_owned();
    }
    let record: Value = serde_json::from_slice(&fs::read(root.join(".local/state/north/fleet/narrow/session.json")).unwrap()).unwrap();
    north(root, &["fleet", "forget", "narrow"]);

    assert!(String::from_utf8_lossy(&posted.stderr).contains("no session named narrow here; posted "), "{}", String::from_utf8_lossy(&posted.stderr));
    assert!(relayed.status.success(), "{}{}", String::from_utf8_lossy(&relayed.stdout), String::from_utf8_lossy(&relayed.stderr));
    let host = record["last_sender"]["name"].as_str().and_then(|name| name.strip_prefix("wisp-lead@")).unwrap_or_else(|| panic!("{record}"));
    assert!(shown.contains(&format!("user: [from wisp-lead@{host}] rolled back")), "{shown}");
}
