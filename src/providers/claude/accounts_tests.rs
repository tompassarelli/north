use std::fs;
use std::path::Path;

use serde_json::json;

use super::{Profiles, discover, profiles};
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
    match roster.account(&claude, "b", None) {
        Err(NorthError::Refused(why)) => assert!(why.starts_with("claude:b is logged in to the same account as claude:a"), "{why}"),
        other => panic!("launching the duplicate label was not refused: {other:?}"),
    }
    assert_eq!(roster.account(&claude, "c", None).unwrap().label, "c");
}

#[test]
fn the_declared_label_keeps_its_login_even_from_a_home_outside_the_fleet_profiles() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join(".local/state/north");
    profile(&state, "a", "account-one");
    profile(&state, "c", "account-two");
    login(&home.path().join(".claude-outside"), "account-two");
    let policy = "[accounts.claude]\nlabels = { a2cd0f1e = \"x\" }\nprofiles = { b = { here = { path = \"~/.claude-outside\" } }, a = { elsewhere = { path = \"/nowhere\" }, here = {} } }\n";
    let mut declared = profiles(&policy.parse().unwrap(), home.path(), "here");
    assert_eq!(declared.homes, [("b".to_owned(), home.path().join(".claude-outside"))]);

    let roster = discover(&ProviderName::new("claude"), &state, &declared);
    assert_eq!(labels(&roster), (vec!["a"], vec![("c", "b")]));

    let two = super::identity(&state.join("profiles/claude-c"), &declared.default_home).unwrap();
    declared.keepers.insert(two, "c".into());
    assert_eq!(labels(&discover(&ProviderName::new("claude"), &state, &declared)), (vec!["a", "c"], vec![]));
}

#[test]
fn a_login_at_a_home_declared_for_this_host_is_listed_there_and_still_refused_as_a_duplicate() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join(".local/state/north");
    let default = home.path().join(".claude");
    fs::create_dir_all(&default).unwrap();
    fs::write(default.join(".credentials.json"), "{}").unwrap();
    fs::write(home.path().join(".claude.json"), json!({"oauthAccount": {"accountUuid": "account-one"}}).to_string()).unwrap();
    profile(&state, "a", "account-two");
    profile(&state, "b", "account-one");
    let policy = "[accounts.claude]\nprofiles = { a = { here = { path = \"~/.claude\" }, elsewhere = { path = \"/nowhere\" } } }\n";
    let declared = profiles(&policy.parse().unwrap(), home.path(), "here");

    let roster = discover(&ProviderName::new("claude"), &state, &declared);

    assert_eq!(labels(&roster), (vec!["a"], vec![("b", "a")]));
    assert_eq!(roster.accounts[0].home, default);
}
