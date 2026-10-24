mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

const BRIEF: &str = "Item: smashcraft#4 LAN check\nSurface: lua\nNeeds: warcraft\n";

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture(root: &Path) {
    let op = root.join("op");
    let agents = root.join(".local/state/agents");
    let profile = root.join(".local/state/north/profiles/claude-a");
    for directory in [&op, &agents, &profile, &root.join("bin")] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(op.join("orchestration.toml"), "[placement]\nprivate_repos = []\n[hosts.here]\nserves = [\"warcraft\"]\n").unwrap();
    fs::write(profile.join(".credentials.json"), "{}").unwrap();
    let windows = serde_json::json!([{"window": "5h", "used_pct": 10.0, "burn_pct_h": 1.0, "resets_at": "2026-10-16T16:00:00Z"}]);
    fs::write(agents.join("usage-gate.json"), serde_json::json!({"claude": {"controls": {"claude:a": {"windows": windows}}}}).to_string()).unwrap();
    executable(&root.join("bin/setsid"), "#!/bin/sh\nexec \"$@\"\n");
    executable(&root.join("bin/systemd-run"), "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n");
    executable(&root.join("bin/systemctl"), "#!/bin/sh\n[ \"$*\" = '--user show-environment' ] || exit 0\nprintf 'HOME=%s\\nPATH=%s/.nix-profile/bin:/run/wrappers/bin\\n' \"$HOME\" \"$HOME\"\n");
    let timeout = Command::new("sh").args(["-c", "command -v timeout"]).output().unwrap();
    let timeout = String::from_utf8_lossy(&timeout.stdout).trim().to_owned();
    executable(&root.join("bin/claude"), &format!("#!/bin/sh\nprintf '%s\\n' \"$PATH\" > \"$SESSION_PATH_FIXTURE/path\"\nexec {timeout} 20 {} --trace fold \"$@\"\n", support::fake_claude()));
}

fn launch(root: &Path) -> Output {
    let brief = root.join("box.md");
    fs::write(&brief, BRIEF).unwrap();
    Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["fleet", "launch", "--provider", "claude", "--account", "a", "--name", "narrow", "--parent", "none", "--system-prompt"])
        .arg(&brief)
        .arg("start the box")
        .current_dir(root)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join(".local/state"))
        .env("AGENTS_ORCHESTRATION", root.join("op/orchestration.toml"))
        .env("AGENTS_OPERATOR_DIR", root.join("op"))
        .env("PLACEMENT_NOW", "2026-10-16T12:00:00Z")
        .env("RESIDENT_CAP_HOST", "here")
        .env("CLAUDE_RUNTIME", root.join("bin/claude"))
        .env("SESSION_PATH_FIXTURE", root)
        .env("NORTH_FLEET_SESSION", "")
        .env_remove("NORTH_FLEET_TEST")
        .env("PATH", root.join("bin"))
        .output()
        .unwrap()
}

#[test]
fn a_session_launched_from_a_minimal_path_gets_the_login_path() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    fixture(root);
    let launched = launch(root);
    assert!(launched.status.success(), "{}", String::from_utf8_lossy(&launched.stderr));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("path").is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let path = fs::read_to_string(root.join("path")).unwrap_or_default();
    let expected = format!("{home}/.nix-profile/bin:/run/wrappers/bin:{home}/bin\n", home = root.display());
    assert_eq!(path, expected);
}
