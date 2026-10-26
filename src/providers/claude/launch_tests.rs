use std::path::PathBuf;

use crate::fleet::model::{Effort, SessionRecord};
use crate::fleet::prompt::PromptMode;

use super::*;

fn request() -> LaunchRequest {
    LaunchRequest {
        verification: false,
        name: "worker".into(),
        provider: Claude.provider(),
        account: "a".into(),
        cwd: PathBuf::from("/tmp"),
        model: None,
        effort: None,
        prompt: String::new(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: PromptMode::North,
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        placement: None,
        machine: None,
        capacity_need: None,
    }
}

#[test]
fn the_host_names_its_session_to_the_commands_it_runs() {
    let account = Account { provider: Claude.provider(), label: "a".into(), home: PathBuf::from("/tmp/claude-a"), token_file: None };
    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();
    assert!(spec.env.contains(&("NORTH_FLEET_SESSION".to_owned(), "worker".into())), "{:?}", spec.env);
}

#[test]
fn an_account_at_claudes_default_home_launches_on_its_own_config_and_any_other_on_its_home() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let mut account = Account { provider: Claude.provider(), label: "a".into(), home: home.join(".claude"), token_file: None };
    let config = "CLAUDE_CONFIG_DIR".to_owned();
    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();
    assert!(spec.env.iter().all(|(name, _)| name != &config), "{:?}", spec.env);
    assert!(spec.env_remove.contains(&config), "{:?}", spec.env_remove);

    account.home = PathBuf::from("/tmp/claude-a");
    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();
    assert!(spec.env.contains(&(config.clone(), "/tmp/claude-a".into())), "{:?}", spec.env);
    assert!(!spec.env_remove.contains(&config), "{:?}", spec.env_remove);
}

#[test]
fn a_token_account_reads_its_token_into_the_cli_environment_and_never_into_argv_or_the_spec_env() {
    let scratch = tempfile::tempdir().unwrap();
    let token = scratch.path().join("token");
    std::fs::write(&token, "sk-test-value\n").unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: Some(token) };

    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();

    assert!(spec.argv.iter().all(|arg| !arg.contains("sk-test-value")), "{:?}", spec.argv);
    assert!(spec.env.iter().all(|(_, value)| !value.to_string_lossy().contains("sk-test-value")), "{:?}", spec.env);
    assert!(spec.env_remove.contains(&"CLAUDE_CODE_OAUTH_TOKEN".to_owned()), "{:?}", spec.env_remove);
    let probe = [&spec.argv[..4], &["sh".into(), "-c".into(), "printf %s \"$CLAUDE_CODE_OAUTH_TOKEN\"".into()]].concat();
    let output = std::process::Command::new(&probe[0]).args(&probe[1..]).env_remove("CLAUDE_CODE_OAUTH_TOKEN").output().unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "sk-test-value");
}

#[test]
fn a_launch_at_high_effort_runs_claude_at_high_effort_and_its_revive_keeps_it() {
    let account = Account { provider: Claude.provider(), label: "a".into(), home: PathBuf::from("/tmp/claude-a"), token_file: None };
    let request = LaunchRequest { effort: Some(Effort::parse("high").unwrap()), ..request() };
    let effort = |argv: &[String]| argv.windows(2).any(|pair| pair == ["--effort", "high"]);

    let launched = Claude.host(&account, &request, Opening::Fresh(None), "").unwrap();
    let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "effort": request.effort})).unwrap();
    let revived = Claude.host(&account, &record.relaunch_request(), Opening::Resume("c"), "").unwrap();

    assert!(effort(&launched.argv), "{:?}", launched.argv);
    assert!(effort(&revived.argv), "{:?}", revived.argv);
    assert!(Effort::parse("extreme").is_err());
}
