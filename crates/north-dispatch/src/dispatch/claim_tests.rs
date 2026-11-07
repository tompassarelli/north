#[test]
fn a_claim_by_a_parked_fleet_session_holds_its_item_as_threads_claim_refuses_on_it_while_its_launch_no_longer_does() {
    let fleets = vec![("nexus".to_owned(), serde_json::json!([{"name": "north-112-d1230", "status": "parked"}, {"name": "north-88-d0131", "status": "exited"}]))];
    let listing = serde_json::json!([
        {"item": "north#112", "owner": "north-112-d1230", "standing": "live"},
        {"item": "north#88", "owner": "north-88-d0131", "standing": "live"},
    ]);
    let held: Vec<String> = super::survey::claim_rows(&listing, "gex45", &super::fleets::exited_names(&fleets)).into_iter().map(|(item, _)| item.to_string()).collect();
    assert_eq!(held, ["north#112"]);
    assert_eq!(super::fleets::stopped_names(&fleets).iter().map(String::as_str).collect::<Vec<_>>(), ["north-112-d1230", "north-88-d0131"]);
}

#[test]
fn a_hand_launch_whose_brief_opens_with_its_issue_holds_it_so_dispatch_refuses_north_855_d2226_beside_frontier_run() {
    use crate::fleet::model::{LaunchRequest, SessionRecord};
    use north_providers::providers::claude::fleet::Claude;
    use crate::fleet::runtime::Runtime;
    let prompt = "\nCategory: feature\nTier: sonnet-high\nArm: context-300k\nSpec: exact\nScope: module\nSurface: ts\nVerify: local-test\n\nnorth#855: run the frontier scoreboard's headline. Read docs/rationale/frontier.md and north#730's first comment first.\nRepository: north\n";
    let request = LaunchRequest {
        verification: false,
        name: "frontier-run".into(),
        provider: Claude.provider(),
        account: "b".into(),
        cwd: "/home/tom/code/north/main".into(),
        model: None,
        effort: None,
        prompt: prompt.into(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: crate::fleet::prompt::PromptMode::North,
        idle_exit_minutes: None,
        parent: Some("proxy".into()),
        task_ref: None,
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: None,
        resume: None,
        compact_window: None,
        slots: Default::default(),
    };
    let record = SessionRecord::launched(&request, None, None, false, 1_794_090_424_000);
    let laptop = serde_json::json!([{"name": record.name, "status": "working", "tasks": record.tasks().collect::<Vec<_>>()}]);
    let sessions: Vec<_> = super::fleets::running_sessions(&laptop).into_iter().map(|session| (session, "whiterabbit".to_owned())).collect();

    let held = super::fleets::held(&[super::IssueRef::new("north", 855)], &sessions, &std::collections::BTreeMap::new());

    assert_eq!(held.map(|(_, holder)| holder), Some("fleet session frontier-run on whiterabbit".to_owned()));
}
