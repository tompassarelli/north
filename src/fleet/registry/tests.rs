use std::fs;
use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::error::NorthError;
use crate::fleet::model::Machine;
use crate::fleet::prompt::PromptMode;
use crate::providers::claude::fleet::Claude;

fn fleet(home: &Path, profiles: &str) -> Fleet {
    let local = home.join(".local/state/north/profiles/claude-a");
    fs::create_dir_all(&local).unwrap();
    fs::write(local.join(".credentials.json"), "{}").unwrap();
    fs::write(local.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"local-login"}}"#).unwrap();
    let policy = crate::pace::target::home_policy_path(home);
    fs::create_dir_all(policy.parent().unwrap()).unwrap();
    let here = crate::platform::host::name();
    fs::write(&policy, format!("[accounts.claude.profiles.a]\n\"{here}\" = {{ path = \"~/.local/state/north/profiles/claude-a\" }}\n{profiles}")).unwrap();
    let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Claude)]), ledger).unwrap()
}

fn on_far() -> LaunchRequest {
    LaunchRequest {
        verification: false,
        name: "worker".into(),
        provider: Claude.provider(),
        account: "a".into(),
        cwd: "/tmp".into(),
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
        machine: Some(Machine { name: "far".into(), ssh: "tom@far".into() }),
        capacity_need: None,
    }
}

fn remote_command(fleet: &Fleet) -> String {
    let request = on_far();
    let account = fleet.account(&request).unwrap();
    let record = SessionRecord { machine: request.machine.clone(), ..serde_json::from_value(serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null})).unwrap() };
    let spec = remote::placed(Claude.host(&account, &request, Opening::Fresh(None), "").unwrap(), &record).unwrap();
    spec.argv.last().unwrap().clone()
}

#[test]
fn a_session_placed_on_another_machine_runs_on_that_machines_declared_token_whatever_this_hosts_login() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "far = { path = \"~/far-a\", token_file = \"~/tokens/claude-a\" }\n");

    let command = remote_command(&fleet);

    let far_home = home.path().join("far-a").display().to_string();
    let far_token = home.path().join("tokens/claude-a").display().to_string();
    assert!(command.contains(&format!("'--setenv=CLAUDE_CONFIG_DIR={far_home}'")), "{command}");
    assert!(command.contains("CLAUDE_CODE_OAUTH_TOKEN=$(cat -- \"$0\")") && command.contains(&format!("'{far_token}'")), "{command}");
    assert!(!command.contains("profiles/claude-a"), "{command}");
}

#[test]
fn a_session_placed_on_another_machine_without_a_declared_token_runs_on_that_machines_declared_home() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "far = { path = \"/srv/far-a\" }\n");

    let command = remote_command(&fleet);

    assert!(command.contains("'--setenv=CLAUDE_CONFIG_DIR=/srv/far-a'"), "{command}");
    assert!(!command.contains("CLAUDE_CODE_OAUTH_TOKEN") && !command.contains("profiles/claude-a"), "{command}");
}

#[test]
fn a_label_with_no_profile_for_the_named_machine_is_refused_naming_its_owners() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "near = { path = \"~/near-a\" }\n");

    match fleet.account(&on_far()) {
        Err(NorthError::Refused(why)) => assert!(why.contains("not far") && why.contains("near"), "{why}"),
        other => panic!("a label undeclared for far was placed there: {other:?}"),
    }
}

fn chain(fleet: &Fleet, links: &[(&str, Option<&str>)]) {
    for (name, parent) in links {
        let session = fleet.root.join(name);
        fs::create_dir_all(&session).unwrap();
        let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "parent": parent});
        fs::write(session.join("session.json"), record.to_string()).unwrap();
    }
}

fn launch_refusal(fleet: &Fleet, caller: &str) -> Option<String> {
    match fleet.admit_launch_from(Some(caller)) {
        Ok(()) => None,
        Err(NorthError::Refused(why)) => Some(why),
        Err(other) => panic!("{caller} was refused for the wrong reason: {other:?}"),
    }
}

#[test]
fn a_launch_from_a_session_two_levels_below_the_lead_is_refused_naming_its_depth() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("lead", None), ("worker", Some("lead")), ("judge", Some("worker"))]);

    let why = launch_refusal(&fleet, "judge").expect("a session two levels below the lead launched");

    assert!(why.contains("judge is 2 levels below the lead"), "{why}");
}

#[test]
fn a_launch_from_the_lead_or_one_level_below_it_is_admitted() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("lead", None), ("worker", Some("lead")), ("judge", Some("worker"))]);

    assert_eq!(launch_refusal(&fleet, "lead"), None);
    assert_eq!(launch_refusal(&fleet, "worker"), None);
    assert_eq!(launch_refusal(&fleet, "unrecorded"), None);
}

#[test]
fn a_session_whose_parent_record_is_gone_still_counts_that_parent_as_a_level() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("orphan", Some("forgotten-lead")), ("grandchild", Some("orphan"))]);

    assert_eq!(launch_refusal(&fleet, "orphan"), None);
    assert!(launch_refusal(&fleet, "grandchild").is_some());
}

#[test]
fn a_parent_cycle_is_refused_instead_of_walked_forever() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path(), "");
    chain(&fleet, &[("a", Some("b")), ("b", Some("a"))]);

    assert!(launch_refusal(&fleet, "a").is_some());
}
