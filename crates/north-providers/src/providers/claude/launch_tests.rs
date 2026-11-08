use std::path::PathBuf;

use crate::fleet::prompt::PromptMode;

use super::*;

const HOOKED: &str = r#"{"hooks": {"PreToolUse": [{}]}}"#;

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
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: None,
        resume: None,
        compact_window: None,
        slots: Default::default(),
    }
}

#[test]
fn a_token_account_reads_its_token_into_the_cli_environment_and_never_into_argv_or_the_spec_env() {
    let scratch = tempfile::tempdir().unwrap();
    let token = scratch.path().join("token");
    std::fs::write(&token, "sk-test-value\n").unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: Some(token), api: None };

    let spec = Claude.host(&account, &request(), Opening::Fresh(None), "").unwrap();

    assert!(spec.argv.iter().all(|arg| !arg.contains("sk-test-value")), "{:?}", spec.argv);
    assert!(spec.env.iter().all(|(_, value)| !value.to_string_lossy().contains("sk-test-value")), "{:?}", spec.env);
    assert!(spec.env_remove.contains(&"CLAUDE_CODE_OAUTH_TOKEN".to_owned()), "{:?}", spec.env_remove);
    let probe = [&spec.argv[..4], &["sh".into(), "-c".into(), "printf %s \"$CLAUDE_CODE_OAUTH_TOKEN\"".into()]].concat();
    let output = std::process::Command::new(&probe[0]).args(&probe[1..]).env_remove("CLAUDE_CODE_OAUTH_TOKEN").output().unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "sk-test-value");
}


#[test]
fn a_brief_on_a_context_arm_launches_claude_with_that_auto_compact_window_and_an_unarmed_brief_keeps_the_settings_window() {
    let scratch = tempfile::tempdir().unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: None, api: None };
    let window = |request: &LaunchRequest| {
        let spec = Claude.host(&account, request, Opening::Fresh(None), "").unwrap();
        spec.env.into_iter().find(|(name, _)| name == "CLAUDE_CODE_AUTO_COMPACT_WINDOW").map(|(_, value)| value.to_string_lossy().into_owned())
    };
    let armed = LaunchRequest { prompt: "Item: north#636\nTier: worker\nArm: context-300k\nGoal: x".into(), ..request() };
    let relaunched = LaunchRequest { compact_window: Some(200_000), ..request() };

    assert_eq!(window(&armed).as_deref(), Some("300000"));
    assert_eq!(window(&relaunched).as_deref(), Some("200000"));
    assert_eq!(window(&LaunchRequest { prompt: "Arm: effort-high".into(), ..request() }), None);
}

#[test]
fn a_profile_without_the_harness_settings_is_named_unguarded_so_its_launch_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("profiles/claude-api");
    std::fs::create_dir_all(&home).unwrap();
    let account = Account { provider: Claude.provider(), label: "api".into(), home: home.clone(), token_file: None, api: None };
    let here = |path: &std::path::Path| std::fs::read_to_string(path).map_or(Seen::Absent, Seen::Text);

    assert!(Claude.unguarded(&account, &here).is_some_and(|reason| reason.contains("no settings.json")));
    std::fs::write(home.join("settings.json"), HOOKED).unwrap();
    assert_eq!(Claude.unguarded(&account, &here), None);

    let remote = scratch.path().join("remote");
    let there = |path: &std::path::Path| std::fs::read_to_string(remote.join(path.strip_prefix("/").unwrap_or(path))).map_or(Seen::Absent, Seen::Text);
    assert!(Claude.unguarded(&account, &there).is_some_and(|reason| reason.contains("no settings.json")));
    let linked = remote.join(home.strip_prefix("/").unwrap());
    std::fs::create_dir_all(&linked).unwrap();
    std::fs::write(linked.join("settings.json"), HOOKED).unwrap();
    assert_eq!(Claude.unguarded(&account, &there), None);
}

#[test]
fn a_profile_whose_settings_register_no_hooks_is_named_unguarded_too() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("profiles/claude-api");
    std::fs::create_dir_all(&home).unwrap();
    let account = Account { provider: Claude.provider(), label: "api".into(), home: home.clone(), token_file: None, api: None };
    let here = |path: &std::path::Path| std::fs::read_to_string(path).map_or(Seen::Absent, Seen::Text);

    for settings in ["{}", r#"{"hooks": {}}"#, "not json"] {
        std::fs::write(home.join("settings.json"), settings).unwrap();
        assert!(Claude.unguarded(&account, &here).is_some_and(|reason| reason.contains("settings.json that")), "{settings}");
    }
    std::fs::write(home.join("settings.json"), r#"{"hooks": {"PreToolUse": [{}]}}"#).unwrap();
    assert_eq!(Claude.unguarded(&account, &here), None);
    assert_eq!(Claude.unguarded(&account, &|_| Seen::Unreachable), None);
}
