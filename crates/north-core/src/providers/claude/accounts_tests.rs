use std::fs;
use std::path::Path;

use serde_json::json;

use super::{Profiles, discover};
use crate::error::NorthError;
use crate::fleet::model::ProviderName;

fn login(home: &Path, account: &str) {
    fs::create_dir_all(home).unwrap();
    fs::write(home.join(".claude.json"), json!({"oauthAccount": {"accountUuid": account}}).to_string()).unwrap();
}

fn profile(state: &Path, label: &str, account: &str) {
    let home = state.join(format!("profiles/claude-{label}"));
    login(&home, account);
    fs::write(home.join(".credentials.json"), "{}").unwrap();
}

fn labels(roster: &crate::fleet::roster::Roster) -> (Vec<&str>, Vec<(&str, &str)>) {
    let listed = roster.accounts.iter().map(|account| account.label.as_str()).collect();
    let duplicates = roster.duplicates.iter().map(|duplicate| (duplicate.account.label.as_str(), duplicate.of.as_str())).collect();
    (listed, duplicates)
}

#[test]
fn a_label_logged_into_another_labels_account_is_omitted_and_its_launch_refused_naming_both() {
    let state = tempfile::tempdir().unwrap();
    profile(state.path(), "a", "account-one");
    profile(state.path(), "b", "account-one");
    profile(state.path(), "c", "account-two");
    let claude = ProviderName::new("claude");

    let roster = discover(&claude, state.path(), &Profiles::default());

    assert_eq!(labels(&roster), (vec!["a", "c"], vec![("b", "a")]));
    match roster.account(&claude, "b") {
        Err(NorthError::Refused(why)) => assert!(why.starts_with("claude:b is logged in to the same account as claude:a"), "{why}"),
        other => panic!("launching the duplicate label was not refused: {other:?}"),
    }
    assert_eq!(roster.account(&claude, "c").unwrap().label, "c");

    let token = state.path().join("claude-tokens/claude-c");
    fs::create_dir_all(token.parent().unwrap()).unwrap();
    fs::write(&token, "setup-token").unwrap();
    let gone = state.path().join("claude-tokens/claude-a");
    let homes = ["a", "c"].map(|label| (label.to_owned(), state.path().join(format!("profiles/claude-{label}")))).to_vec();
    let launched = discover(&claude, state.path(), &Profiles { homes, tokens: vec![("c".into(), token.clone()), ("a".into(), gone)], ..Profiles::default() });
    assert_eq!(launched.account(&claude, "c").unwrap().token_file.as_deref(), Some(token.as_path()), "a host that holds the setup token launches on it, not on the login beside it");
    assert!(launched.account(&claude, "a").is_err(), "a declared token that cannot be read refuses the account instead of falling back to its login");
}

