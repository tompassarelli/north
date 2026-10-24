mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const NOW: &str = "2026-10-16T12:00:00Z";
const CODE_ONLY: &str = "Item: north#12 tidy placement\nSurface: rust\nNeeds: code\nDone when: `cargo test` exits 0\n";
const WARCRAFT: &str = "Item: smashcraft#4 LAN check\nSurface: lua\nNeeds: warcraft\n";
const NATIVE: &str = "Item: smashcraft#207 remote lag\nSurface: lua\nNeeds: native\n";

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn window(name: &str, used: f64, burn: f64, resets_at: &str) -> serde_json::Value {
    serde_json::json!({"window": name, "used_pct": used, "burn_pct_h": burn, "resets_at": resets_at})
}

/// A HOME with South's placement policy, the credit, pace-control's gate, two Claude profiles and
/// shims for the scope launcher and the Claude runtime, which records the prompt it was sent.
fn fixture(root: &Path) {
    let op = root.join("op");
    let agents = root.join(".local/state/agents");
    fs::create_dir_all(&op).unwrap();
    fs::create_dir_all(&agents).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(op.join("orchestration.toml"), "[placement]\ncloud_credit = \"cloud-credit\"\nprivate_repos = [\"galileo\"]\n[hosts.here]\nserves = [\"warcraft\", \"native\"]\n").unwrap();
    fs::write(op.join("costs.toml"), "[[service]]\nid = \"cloud-credit\"\ninitial = 250.0\nexpires = 2026-11-05T07:59:00Z\n").unwrap();
    let gate = serde_json::json!({"claude": {"controls": {
        "claude:a": {"windows": [window("5h", 35.0, 10.0, "2026-10-16T16:00:00Z"), window("week", 40.0, 1.0, "2026-10-20T00:00:00Z")]},
        "claude:c": {"windows": [window("5h", 42.0, 86.0, "2026-10-16T16:00:00Z"), window("week", 20.0, 1.0, "2026-10-20T00:00:00Z")]},
    }}});
    fs::write(agents.join("usage-gate.json"), gate.to_string()).unwrap();
    for label in ["a", "c"] {
        let profile = root.join(format!(".local/state/north/profiles/claude-{label}"));
        fs::create_dir_all(&profile).unwrap();
        fs::write(profile.join(".credentials.json"), "{}").unwrap();
    }
    executable(&root.join("bin/setsid"), "#!/bin/sh\nexec \"$@\"\n");
    executable(&root.join("bin/systemd-run"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n");
    executable(&root.join("bin/claude"), &format!("#!/bin/sh\nprintf '%s' \"${{RAN_VIA_SSH:-}}\" > \"$PLACEMENT_FIXTURE/ran-via\"\nprintf '%s' \"${{AGENT_CAPACITY_NEED:-}}\" > \"$PLACEMENT_FIXTURE/need\"\ntee \"$PLACEMENT_FIXTURE/prompt\" | timeout 20 {} --trace fold \"$@\"\n", support::fake_claude()));
}

fn north(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(args)
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("AGENTS_ORCHESTRATION", root.join("op/orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("PLACEMENT_NOW", NOW)
        .env("RESIDENT_CAP_HOST", "here")
        .env("CLAUDE_RUNTIME", root.join("bin/claude"))
        .env("PLACEMENT_FIXTURE", root)
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
        .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .output()
        .unwrap()
}

fn launch(root: &Path, name: &str, brief: &str, flags: &[&str]) -> Output {
    let path = root.join(format!("{name}.md"));
    fs::write(&path, brief).unwrap();
    let path = path.display().to_string();
    let mut args = vec!["fleet", "launch", "--provider", "claude", "--name", name, "--parent", "none", "--system-prompt", &path];
    args.extend_from_slice(flags);
    args.push("start the box");
    north(root, &args)
}

#[test]
fn fleet_launch_refuses_off_placement_and_records_the_override_reason() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root);

    let cloud = launch(root, "cloudy", CODE_ONLY, &["--account", "a"]);
    let said = String::from_utf8_lossy(&cloud.stderr);
    assert!(!cloud.status.success(), "a code-only public box launched locally without --local");
    assert!(said.contains("ssh -tt nexus 'cd /tmp/cloud && claude --cloud \"$(cat cloudy.md)\"'"), "{said}");
    assert!(said.contains("Add the cloud safety line") && said.contains("--local \"<reason>\""), "{said}");
    assert!(said.contains("claude:c unavailable: runaway: 5h 42%"), "{said}");
    assert!(!root.join(".local/state/north/fleet/cloudy").exists(), "the refused launch left a session");

    let runaway = launch(root, "native", WARCRAFT, &["--account", "c"]);
    let said = String::from_utf8_lossy(&runaway.stderr);
    assert!(!runaway.status.success(), "a launch onto a runaway account started without --pin");
    assert!(said.contains("claude:c unavailable: runaway") && said.contains("--account a"), "{said}");

    let local = launch(root, "kept", CODE_ONLY, &["--account", "a", "--local", "x"]);
    assert!(local.status.success(), "{}", String::from_utf8_lossy(&local.stderr));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(root.join("prompt")).unwrap_or_default().contains("start the box") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(fs::read_to_string(root.join("prompt")).unwrap_or_default().contains("start the box"));
    let listed = north(root, &["fleet", "list"]);
    let lines = String::from_utf8_lossy(&listed.stdout);
    let kept = lines.lines().find(|line| line.starts_with("kept\t")).unwrap_or_default();
    assert!(kept.ends_with("\tlocal: x"), "{lines}");
}

#[test]
fn fleet_launch_runs_a_box_placed_on_a_machine_through_its_ssh_destination() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root);
    let orchestration = root.join("op/orchestration.toml");
    let policy = fs::read_to_string(&orchestration).unwrap().replace("[placement]\n", "[placement]\norder = [\"far\"]\n");
    fs::write(&orchestration, format!("{policy}[hosts.far]\nresident_cap = 2\nssh = \"tom@far\"\n")).unwrap();
    fs::write(root.join("hosts.json"), r#"{"far": {"online": true, "live": 1, "accounts": ["a"]}}"#).unwrap();
    executable(&root.join("bin/ssh"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do last=$1; shift; done\nshift\nexport RAN_VIA_SSH=\"$last\"\nPATH=\"$PLACEMENT_FIXTURE/bin:$PATH\" exec sh -c \"$*\"\n");
    let path = root.join("private.md");
    fs::write(&path, "Item: galileo#3 private change\nSurface: rust\n").unwrap();
    let launch = |name: &str| {
        let path = path.display().to_string();
        let placed = Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["fleet", "launch", "--provider", "claude", "--name", name, "--parent", "none", "--system-prompt", &path, "--account", "a", "start the box"])
            .current_dir(root)
            .env("HOME", root)
            .env("XDG_STATE_HOME", root.join(".local/state"))
            .env("AGENTS_ORCHESTRATION", &orchestration)
            .env("AGENTS_OPERATOR_DIR", root.join("op"))
            .env("PLACEMENT_NOW", NOW)
            .env("RESIDENT_CAP_HOST", "here")
            .env("PLACEMENT_HOSTS", root.join("hosts.json"))
            .env("CLAUDE_RUNTIME", root.join("bin/claude"))
            .env("PLACEMENT_FIXTURE", root)
            .env("NORTH_FLEET_SESSION", "")
            .env_remove("NORTH_FLEET_TEST")
            .env("PATH", format!("{}:{}", root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .output()
            .unwrap();
        let said = String::from_utf8_lossy(&placed.stderr).into_owned();
        assert!(placed.status.success(), "{said}");
        said
    };

    let said = launch("far-box");
    assert!(said.contains("placement: target far claude:a") && said.contains("far available: 1/2 resident CLIs"), "{said}");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(root.join("prompt")).unwrap_or_default().contains("start the box") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(fs::read_to_string(root.join("prompt")).unwrap_or_default().contains("start the box"), "the CLI behind ssh got the prompt: {}", fs::read_to_string(root.join(".local/state/north/fleet/far-box/g1/stderr.log")).unwrap_or_default());
    assert_eq!(fs::read_to_string(root.join("ran-via")).unwrap_or_default(), "tom@far", "the CLI ran behind ssh to the machine");
    let listed = north(root, &["fleet", "list"]);
    let lines = String::from_utf8_lossy(&listed.stdout);
    assert!(lines.lines().any(|line| line.starts_with("far-box\t") && line.contains("\tfar:/")), "{lines}");

    let said = launch("kept-box");
    assert!(said.contains("far full: 2/2 resident CLIs") && said.contains("placement: target here claude:a"), "a live session on the machine counts against its cap: {said}");
    for name in ["far-box", "kept-box"] {
        north(root, &["fleet", "stop", name]);
    }
}

#[test]
fn a_label_logged_in_to_another_labels_account_is_omitted_from_accounts_and_refused_at_launch() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root);
    for label in ["a", "c"] {
        let login = serde_json::json!({"oauthAccount": {"accountUuid": "0d4b6f2e-shared-login"}});
        fs::write(root.join(format!(".local/state/north/profiles/claude-{label}/.claude.json")), login.to_string()).unwrap();
    }
    let listed = north(root, &["fleet", "accounts"]);
    let lines = String::from_utf8_lossy(&listed.stdout);
    assert!(lines.contains("claude\ta\n") && !lines.contains("claude\tc"), "{lines}");
    assert!(String::from_utf8_lossy(&listed.stderr).contains("omitted: claude:c is logged in to the same account as claude:a"), "{lines}");

    let pinned = launch(root, "shared", WARCRAFT, &["--account", "c", "--pin", "drain c"]);
    let said = String::from_utf8_lossy(&pinned.stderr);
    assert!(!pinned.status.success(), "a label sharing another label's login launched");
    assert!(said.contains("claude:c is logged in to the same account as claude:a"), "{said}");
    assert!(!root.join(".local/state/north/fleet/shared").exists(), "the refused launch left a session");
    let tested = launch(root, "shared-test", WARCRAFT, &["--account", "c", "--test"]);
    let said = String::from_utf8_lossy(&tested.stderr);
    assert!(!tested.status.success() && said.contains("claude:c is logged in to the same account as claude:a"), "a --test launch skipped the login check: {said}");
}

#[test]
fn a_box_that_needs_native_on_smashcraft_starts_with_that_capacity_need() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root);
    let launched = launch(root, "lagged", NATIVE, &["--account", "a"]);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(root.join("prompt")).unwrap_or_default().contains("start the box") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(fs::read_to_string(root.join("need")).unwrap_or_default(), "native:smashcraft");
    north(root, &["fleet", "stop", "lagged"]);
}
