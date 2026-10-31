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
    };
    assert_eq!(request("Item: wisp#39 relay NAT run\nCategory: native-check\nItem: wisp#40", None).task().as_deref(), Some("wisp#39"));
    assert_eq!(request("Category: tooling\n  Item: tompassarelli/smashcraft#342: dash attack", None).task().as_deref(), Some("tompassarelli/smashcraft#342"));
    assert_eq!(request("Item: smashcraft#342", Some("smashcraft#7")).task().as_deref(), Some("smashcraft#7"), "--task wins");
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
    assert_eq!(item_issues("Item: tompassarelli/smashcraft#342 and north#9, north#9"), ["tompassarelli/smashcraft#342", "north#9"]);
    assert!(item_issues("Item: #342 dash attack").is_empty());
    assert!(item_issues("Done when: north#1").is_empty());
}
