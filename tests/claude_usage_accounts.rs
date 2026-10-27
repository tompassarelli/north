use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const LIB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/harness/lib");

fn host() -> String {
    String::from_utf8(Command::new("hostname").output().unwrap().stdout).unwrap().trim().to_owned()
}

fn python(home: &Path, policy: &Path, config_dir: Option<&Path>, arguments: &[&str]) -> String {
    let mut command = Command::new("python3");
    command.arg("-I").args(arguments).env("HOME", home).env("AGENTS_ORCHESTRATION", policy).env_remove("CLAUDE_CONFIG_DIR");
    if let Some(dir) = config_dir {
        command.env("CLAUDE_CONFIG_DIR", dir);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn policy(home: &Path, labels: &str, profiles: &str) -> std::path::PathBuf {
    let path = home.join("orchestration.toml");
    fs::write(&path, format!("[accounts.claude]\nlabels = {{ {labels} }}\nprofiles = {{ {profiles} }}\n")).unwrap();
    path
}

#[test]
fn an_upgraded_account_reports_the_plan_of_its_refreshed_organization_tier_not_its_sign_in_credential() {
    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join("profile-c");
    fs::create_dir_all(&profile).unwrap();
    let identity = format!("{:x}", Sha256::digest(b"uuid-c"));
    fs::write(profile.join(".claude.json"), json!({"oauthAccount": {"accountUuid": "uuid-c", "organizationRateLimitTier": "default_claude_max_20x"}}).to_string()).unwrap();
    fs::write(profile.join(".credentials.json"), json!({"claudeAiOauth": {"subscriptionType": "max", "rateLimitTier": "default_claude_max_5x"}}).to_string()).unwrap();
    let policy = policy(home.path(), &format!("\"{}\" = \"c\"", &identity[..8]), &format!("c = {{ \"{}\" = {{ path = \"{}\" }} }}", host(), profile.display()));
    let usage = python(home.path(), &policy, None, &[concat!(env!("CARGO_MANIFEST_DIR"), "/harness/lib/usage_claude.py"), "--no-refresh"]);
    let plan = usage.lines().map(|line| serde_json::from_str::<Value>(line).unwrap()).find(|row| row["name"] == "plan").unwrap();
    assert_eq!((plan["account"].as_str(), plan["source"].as_str()), (Some("c"), Some("max-20x")));
}

#[test]
fn a_session_on_a_token_only_profile_is_the_account_this_host_declares_for_that_profile() {
    let home = tempfile::tempdir().unwrap();
    let (token_home, other) = (home.path().join("profile-d"), home.path().join("elsewhere"));
    fs::create_dir_all(&token_home).unwrap();
    fs::create_dir_all(&other).unwrap();
    let policy = policy(home.path(), "", &format!("d = {{ \"{}\" = {{ path = \"{}\", token_file = \"{}/token-d\" }} }}", host(), token_home.display(), home.path().display()));
    let script = format!("import sys; sys.path.insert(0, '{LIB}'); import usage; print(usage.local_account('claude'))");
    assert_eq!(python(home.path(), &policy, Some(&token_home), &["-c", &script]).trim(), "d");
    assert_eq!(python(home.path(), &policy, Some(&other), &["-c", &script]).trim(), "None");
}
