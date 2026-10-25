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
