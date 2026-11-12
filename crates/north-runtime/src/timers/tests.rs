#[test]
fn a_roster_account_ask_reaches_its_program_through_env() {
    let profile = std::path::PathBuf::from("/profiles/claude-b");
    let held = tempfile::tempdir().unwrap();
    let token_file = held.path().join("claude-b");
    std::fs::write(&token_file, "secret\n").unwrap();
    let token = crate::fleet::model::Account { provider: crate::fleet::model::ProviderName::new("claude"), label: "b".into(), home: profile.clone(), token_file: Some(token_file.clone()), api: None };
    let words = super::oneshot::account_words(Some(&profile), &[token], crate::providers::claude::invocation);
    let fast = words.iter().position(|word| word == "CLAUDE_CODE_DISABLE_FAST_MODE=1").unwrap();
    assert!(words[..fast].contains(&"CLAUDE_CONFIG_DIR=/profiles/claude-b".to_owned()), "{words:?}");
    assert_eq!(words[fast + 1..].first().map(String::as_str), Some("/bin/sh"), "{words:?}");
    assert_eq!(words.last().map(String::as_str), Some(token_file.to_str().unwrap()), "{words:?}");
    let unlisted = super::oneshot::account_words(Some(std::path::Path::new("/profiles/claude-x")), &[], crate::providers::claude::invocation);
    assert_eq!(unlisted, ["CLAUDE_CONFIG_DIR=/profiles/claude-x", "CLAUDE_CODE_DISABLE_FAST_MODE=1"]);
    assert!(words.contains(&"-u".to_owned()), "{words:?}");
    let ask = super::oneshot::Ask { model: "haiku", effort: "low", system: "s", input: "i" };
    let args = super::oneshot::once_args("true", &words, &ask, &[]);
    let status = std::process::Command::new("env").args(&args).status().unwrap();
    assert!(status.success(), "env {args:?}: {status}");
}


/// north#960: timers that start a fleet session started one while the proxy had dispatch paused.
#[test]
fn a_timer_that_launches_a_fleet_session_starts_none_while_dispatch_is_paused() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join(".local/state/north");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join(north_base::dispatch_pause::RECORD), "account b is for lander and graph work\n").unwrap();

    let fleet = super::fleet_session::Fleet { south: home.path().join("south"), home: home.path().to_owned() };
    let launch = super::fleet_session::Launch { name: "watch-review-x", account: "b", cwd: home.path(), brief: "brief", model: None };
    let held = fleet.launch(&launch).unwrap_err();
    assert!(matches!(&held, super::fleet_session::LaunchError::Held(reason) if super::fleet_session::held(reason) && reason.contains("account b is for lander")), "{held:?}");

    let paths = super::skill_prune::Paths { handoffs: home.path().join("handoffs"), operator: home.path().join("operator"), home: home.path().to_owned() };
    assert_eq!(super::skill_prune::prune(&paths), Ok(None));
    assert!(!paths.handoffs.exists());
}
