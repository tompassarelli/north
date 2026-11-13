/// north#1239: every one-shot ask tried each account at its limit again before the one with room, 5.5 s of an 8.3 s ask.
#[test]
fn a_second_judge_ask_skips_accounts_found_at_their_limit_and_starts_claude_once() {
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
    let profiles = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let labels: Vec<String> = ["a", "b", "c"].map(str::to_owned).to_vec();
    for label in &labels {
        std::fs::create_dir(profiles.path().join(format!("claude-{label}"))).unwrap();
    }
    for expected in [3, 1] {
        let mut starts = 0;
        let mut attempt = |dir: Option<&std::path::Path>| {
            starts += 1;
            let answers = dir.is_some_and(|dir| dir.ends_with("claude-c"));
            let stdout = if answers { "yes" } else { "You've hit your limit · resets 3pm (UTC)" }.to_owned();
            super::oneshot::Reply { code: Some(i32::from(!answers)), stdout, stderr: String::new() }
        };
        let mut limited = super::oneshot::Limited::read(state.path(), 1_792_000_000);
        let asked = super::oneshot::ask(profiles.path(), &mut limited, &labels, &mut attempt, &mut |warning| panic!("{warning}"));
        assert!(matches!(asked, super::oneshot::Asked::Answer(ref answer) if answer == "yes"));
        assert_eq!(starts, expected);
    }
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
