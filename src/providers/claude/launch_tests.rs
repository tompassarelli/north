use std::path::PathBuf;

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
    let account = Account { provider: Claude.provider(), label: "a".into(), home: PathBuf::from("/tmp/claude-a") };
    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();
    assert!(spec.env.contains(&("NORTH_FLEET_SESSION".to_owned(), "worker".into())), "{:?}", spec.env);
}

#[test]
fn an_account_at_claudes_default_home_launches_on_its_own_config_and_any_other_on_its_home() {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let mut account = Account { provider: Claude.provider(), label: "a".into(), home: home.join(".claude") };
    let config = "CLAUDE_CONFIG_DIR".to_owned();
    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();
    assert!(spec.env.iter().all(|(name, _)| name != &config), "{:?}", spec.env);
    assert!(spec.env_remove.contains(&config), "{:?}", spec.env_remove);

    account.home = PathBuf::from("/tmp/claude-a");
    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();
    assert!(spec.env.contains(&(config.clone(), "/tmp/claude-a".into())), "{:?}", spec.env);
    assert!(!spec.env_remove.contains(&config), "{:?}", spec.env_remove);
}
