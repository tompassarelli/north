use std::fs;
use std::process::Command;

#[test]
fn config_agents_sync_creates_each_declared_profile_missing_on_this_host() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    fs::create_dir_all(home.join(".claude/skills")).unwrap();
    fs::write(home.join(".claude/settings.json"), "{}").unwrap();
    let policy = scratch.path().join("orchestration.toml");
    fs::write(&policy, "[accounts.claude.profiles.b]\nlaunchhost = { path = \"~/profiles/claude-b\" }\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["config", "agents", "sync"])
        .env("HOME", &home)
        .env("AGENTS_ORCHESTRATION", &policy)
        .env("AGENTS_OPERATOR_DIR", scratch.path())
        .env("RESIDENT_CAP_HOST", "launchhost")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let profile = home.join("profiles/claude-b");
    assert!(stdout.contains(&format!("Claude profile b: harness projected to {}", profile.display())), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(fs::read_link(profile.join("settings.json")).unwrap(), home.join(".claude/settings.json"));
    assert_eq!(fs::read_link(profile.join("skills")).unwrap(), home.join(".claude/skills"));
}
