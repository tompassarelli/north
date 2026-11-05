#[test]
fn a_cross_project_scan_starts_a_token_file_account_through_its_credential_reader() {
    let profile = std::path::PathBuf::from("/profiles/claude-b");
    let token = crate::fleet::model::Account { provider: crate::fleet::model::ProviderName::new("claude"), label: "b".into(), home: profile.clone(), token_file: Some("/tokens/claude-b".into()) };
    let words = super::cross_project::account_words(Some(&profile), &[token], crate::providers::claude::invocation);
    let fast = words.iter().position(|word| word == "CLAUDE_CODE_DISABLE_FAST_MODE=1").unwrap();
    assert!(words[..fast].contains(&"CLAUDE_CONFIG_DIR=/profiles/claude-b".to_owned()), "{words:?}");
    assert_eq!(words[fast + 1..].first().map(String::as_str), Some("/bin/sh"), "{words:?}");
    assert_eq!(words.last().map(String::as_str), Some("/tokens/claude-b"), "{words:?}");
    let unlisted = super::cross_project::account_words(Some(std::path::Path::new("/profiles/claude-x")), &[], crate::providers::claude::invocation);
    assert_eq!(unlisted, ["CLAUDE_CONFIG_DIR=/profiles/claude-x", "CLAUDE_CODE_DISABLE_FAST_MODE=1"]);
}

