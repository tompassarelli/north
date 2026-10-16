use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const NOW: &str = "2026-10-16T12:00:00Z";
const CODE_ONLY: &str = "Item: north#12 tidy placement\nSurface: rust\nNeeds: code\nDone when: `cargo test` exits 0\n";
const WARCRAFT: &str = "Item: smashcraft#4 LAN check\nSurface: lua\nNeeds: warcraft\n";

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
    fs::write(op.join("orchestration.toml"), "[placement]\ncloud_credit = \"claude-a-cloud-credit\"\nprivate_repos = [\"galileo\"]\n").unwrap();
    fs::write(op.join("costs.toml"), "[[service]]\nid = \"claude-a-cloud-credit\"\ninitial = 250.0\nexpires = 2026-11-05T07:59:00Z\n").unwrap();
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
    executable(&root.join("bin/claude"), "#!/bin/sh\nhead -n 1 > \"$PLACEMENT_FIXTURE/prompt\"\nsleep 1\n");
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
    while !root.join("prompt").is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(fs::read_to_string(root.join("prompt")).unwrap_or_default().contains("start the box"));
    let listed = north(root, &["fleet", "list"]);
    let lines = String::from_utf8_lossy(&listed.stdout);
    let kept = lines.lines().find(|line| line.starts_with("kept\t")).unwrap_or_default();
    assert!(kept.ends_with("\tlocal: x"), "{lines}");
}
