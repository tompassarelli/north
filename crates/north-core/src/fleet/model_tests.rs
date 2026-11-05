use super::{LaunchRequest, ProviderName};

#[test]
fn a_session_launched_without_task_holds_the_issue_its_briefs_first_item_line_names() {
    let request = |prompt: &str, task: Option<&str>| LaunchRequest {
        verification: false,
        name: "w39-laptop".into(),
        provider: ProviderName::new("claude"),
        account: "a".into(),
        cwd: "/tmp".into(),
        model: None,
        effort: None,
        prompt: prompt.into(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: Default::default(),
        idle_exit_minutes: None,
        parent: None,
        task_ref: task.map(str::to_owned),
        placement: None,
        machine: None,
        capacity_need: None,
        role: None,
    };
    assert_eq!(request("Item: wisp#39 relay NAT run\nCategory: native-check\nItem: wisp#40", None).task().as_deref(), Some("wisp#39"));
    assert_eq!(request("Category: tooling\n  Item: tompassarelli/makgora#342: dash attack", None).task().as_deref(), Some("tompassarelli/makgora#342"));
    assert_eq!(request("Item: makgora#342", Some("makgora#7")).task().as_deref(), Some("makgora#7"), "--task wins");
    assert_eq!(request("Item: north dispatch places almost nothing\nFollows: north#12", None).task(), None, "an Item line naming no issue holds none");
    assert_eq!(request("Item: #342 dash attack", None).task(), None, "a bare #N names no repository");
}

#[test]
fn a_session_launched_without_task_holds_the_issue_the_brief_its_prompt_points_at_names() {
    let directory = tempfile::tempdir().unwrap();
    let brief = directory.path().join("brief-w63.md");
    std::fs::write(&brief, "Category: bug\nItem: wisp#63 relay rebuild\nItem: wisp#64\n").unwrap();
    let request = |prompt: String, task: Option<&str>| LaunchRequest {
        verification: false,
        name: "w63-laptop".into(),
        provider: ProviderName::new("claude"),
        account: "a".into(),
        cwd: "/tmp".into(),
        model: None,
        effort: None,
        prompt,
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: Default::default(),
        idle_exit_minutes: None,
        parent: None,
        task_ref: task.map(str::to_owned),
        placement: None,
        machine: None,
        capacity_need: None,
        role: None,
    };
    let brief = brief.display();
    assert_eq!(request(format!("read and do the box brief at {brief}"), None).task().as_deref(), Some("wisp#63"));
    assert_eq!(request(format!("Read `{brief}`."), None).task().as_deref(), Some("wisp#63"));
    assert_eq!(request(format!("read {brief}"), Some("wisp#7")).task().as_deref(), Some("wisp#7"), "--task wins");
    assert_eq!(request(format!("Item: north#5\nread {brief}"), None).task().as_deref(), Some("north#5"), "an inline Item line wins");
    assert_eq!(request(format!("read {}/missing.md", directory.path().display()), None).task(), None, "a missing brief holds none");
    let text = directory.path().join("brief.txt");
    std::fs::write(&text, "Item: wisp#63\n").unwrap();
    assert_eq!(request(format!("read {}", text.display()), None).task(), None, "only a .md brief is read");
    assert_eq!(request("read brief-w63.md".into(), None).task(), None, "a relative path is not read");
    assert_eq!(request(format!("read ./{}", brief.to_string().trim_start_matches('/')), None).task(), None, "a dot-relative path is not read");
}

#[test]
fn every_issue_the_first_item_line_names_is_held_up_to_a_word_that_marks_the_rest_as_context() {
    use super::item_issues;
    assert_eq!(item_issues("Category: bug\nItem: north#306, north#314 and #315 close the duplicates\nItem: north#9"), ["north#306", "north#314", "north#315"]);
    assert_eq!(item_issues("Item: close north#306 (see north#12)"), ["north#306"]);
    assert_eq!(item_issues("Item: fix north#7 follows north#6"), ["north#7"]);
    assert_eq!(item_issues("Item: tompassarelli/makgora#342 and north#9, north#9"), ["tompassarelli/makgora#342", "north#9"]);
    assert!(item_issues("Item: #342 dash attack").is_empty());
    assert!(item_issues("Done when: north#1").is_empty());
}

#[test]
fn a_launch_records_the_role_it_was_given_else_a_worker_for_a_task_and_a_lead_for_any_other() {
    use super::{Role, SessionRecord};
    let request = |prompt: &str, task: Option<&str>, role: Option<Role>| LaunchRequest {
        verification: false,
        name: "proxy".into(),
        provider: ProviderName::new("claude"),
        account: "a".into(),
        cwd: "/tmp".into(),
        model: None,
        effort: None,
        prompt: prompt.into(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: Default::default(),
        idle_exit_minutes: None,
        parent: None,
        task_ref: task.map(str::to_owned),
        placement: None,
        machine: None,
        capacity_need: None,
        role,
    };
    let recorded = |request: LaunchRequest| SessionRecord::launched(&request, None, None, false, 1).role;
    assert_eq!(recorded(request("take Tom's messages", None, Some(Role::Proxy))), Some(Role::Proxy));
    assert_eq!(recorded(request("go", Some("north#112"), None)), Some(Role::Worker), "--task makes a worker");
    assert_eq!(recorded(request("Item: north#112 proxy role R1", None, None)), Some(Role::Worker), "an Item line makes a worker");
    assert_eq!(recorded(request("staff the lead-fleet boxes", None, None)), Some(Role::Lead));
    let proxy = SessionRecord::launched(&request("go", None, Some(Role::Proxy)), None, None, false, 1);
    let stored: SessionRecord = serde_json::from_value(serde_json::to_value(&proxy).unwrap()).unwrap();
    assert_eq!((stored.role(), stored.relaunch_request().role()), (Role::Proxy, Role::Proxy), "a revive or hand-over keeps the role");
    assert!(Role::parse("boss").is_err());
}

#[test]
fn a_record_from_before_roles_reads_its_role_from_its_name_parent_and_task() {
    use super::{Role, SessionRecord};
    let legacy = |name: &str, parent: Option<&str>, task: Option<&str>| {
        let record: SessionRecord = serde_json::from_value(serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "parent": parent, "task_ref": task})).unwrap();
        record.role()
    };
    assert_eq!(legacy("proxy", None, None), Role::Proxy);
    assert_eq!(legacy("lead-fleet", None, None), Role::Lead);
    assert_eq!(legacy("lf-r1", Some("lead-fleet"), None), Role::Worker);
    assert_eq!(legacy("lf-r1", None, Some("north#112")), Role::Worker);
}
