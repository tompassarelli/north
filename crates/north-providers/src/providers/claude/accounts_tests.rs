use std::fs;
use std::path::Path;

use serde_json::json;

use super::{Profiles, discover};
use north_foundation::error::NorthError;
use north_fleet_supervisor::fleet::model::ProviderName;

fn login(home: &Path, account: &str) {
    fs::create_dir_all(home).unwrap();
    fs::write(home.join(".claude.json"), json!({"oauthAccount": {"accountUuid": account}}).to_string()).unwrap();
}

fn profile(state: &Path, label: &str, account: &str) {
    let home = state.join(format!("profiles/claude-{label}"));
    login(&home, account);
    fs::write(home.join(".credentials.json"), "{}").unwrap();
}

fn labels(roster: &north_fleet_supervisor::fleet::roster::Roster) -> (Vec<&str>, Vec<(&str, &str)>) {
    let listed = roster.accounts.iter().map(|account| account.label.as_str()).collect();
    let duplicates = roster.duplicates.iter().map(|duplicate| (duplicate.account.label.as_str(), duplicate.of.as_str())).collect();
    (listed, duplicates)
}


#[test]
fn a_launch_on_the_api_pool_with_every_org_spent_is_refused_naming_each_org_and_when_it_reopens() {
    let state = tempfile::tempdir().unwrap();
    let key = state.path().join("keys/a");
    fs::create_dir_all(key.parent().unwrap()).unwrap();
    fs::write(&key, "sk-ant").unwrap();
    let marks = state.path().join("api-exhausted");
    fs::create_dir_all(&marks).unwrap();
    fs::write(marks.join("o1"), "n-a 2026-11-07 until=2099-12-01\n").unwrap();
    fs::write(marks.join("o2"), "n-b 2026-11-07\n").unwrap();
    let org = |name: &str, id: &str| super::Org { name: name.into(), id: id.into(), emails: Vec::new(), keys: vec![key.clone()], expires: None };
    let claude = ProviderName::new("claude");
    let profiles = Profiles { api: vec![("api".into(), state.path().join("profiles/claude-api"))], orgs: vec![org("zenith", "o1"), org("ember", "o2")], ..Profiles::default() };

    match discover(&claude, state.path(), &profiles).account(&claude, "api") {
        Err(NorthError::Refused(why)) => assert_eq!(why, "every API org is spent: zenith until 2099-12-01, ember until its next credit is declared"),
        other => panic!("a launch on a spent pool was not refused as spent: {other:?}"),
    }
}

#[test]
fn an_account_its_client_refused_is_not_offered_and_its_launch_refused_with_the_refusal_until_cleared_though_its_conversation_still_carries_off() {
    let state = tempfile::tempdir().unwrap();
    profile(state.path(), "b", "account-b");
    profile(state.path(), "e", "account-e");
    let claude = ProviderName::new("claude");
    let marks = north_fleet_supervisor::fleet::refusals::directory(state.path());
    let text = "Your organization has disabled Claude subscription access for Claude Code";
    north_fleet_supervisor::fleet::refusals::mark(&marks, "b", "ds-landerE", "2026-11-10T01:20:00Z", text).unwrap();

    let roster = discover(&claude, state.path(), &Profiles::default());

    assert_eq!(labels(&roster).0, vec!["e"], "on 2026-11-10 claude:b was offered as available though its client refused it");
    match roster.account(&claude, "b") {
        Err(NorthError::Refused(why)) => assert!(why.contains(text), "{why}"),
        other => panic!("launching the refused account was not refused: {other:?}"),
    }
    assert_eq!(roster.profile(&claude, "b").ok().map(|account| account.label), Some("b".to_owned()), "on 2026-11-13 a revive onto claude:a could not carry a session off claude:e while e's client refused it");
    north_fleet_supervisor::fleet::refusals::clear(&marks, "b").unwrap();
    assert_eq!(labels(&discover(&claude, state.path(), &Profiles::default())).0, vec!["b", "e"]);
}
