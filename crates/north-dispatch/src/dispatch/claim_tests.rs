#[test]
fn a_parked_fleet_session_holds_its_item_and_box_as_its_threads_claim_does_so_dispatch_staffs_none_beside_it() {
    let fleets = vec![("nexus".to_owned(), serde_json::json!([{"name": "north-112-d1230", "status": "parked"}, {"name": "north-88-d0131", "status": "exited"}]))];
    let listing = serde_json::json!([
        {"item": "north#112", "owner": "north-112-d1230", "standing": "live"},
        {"item": "north#88", "owner": "north-88-d0131", "standing": "live"},
    ]);
    let held: Vec<String> = super::survey::claim_rows(&listing, "gex45", &super::fleets::stopped_names(&fleets)).into_iter().map(|(item, _)| item.to_string()).collect();
    assert_eq!(held, ["north#112"]);
    assert_eq!(super::fleets::stopped_names(&fleets).iter().map(String::as_str).collect::<Vec<_>>(), ["north-88-d0131"], "a parked session's launch still holds its item");

    let listed = serde_json::json!([
        {"name": "north-1036-d0225", "status": "parked", "tasks": ["north#1036"], "box": "a fleet session's build goes to the build host"},
        {"name": "north-1041-d0225", "status": "parked", "tasks": ["north#1038", "north#1041"]},
    ]);
    let sessions: Vec<_> = super::fleets::running_sessions(&listed).into_iter().map(|session| (session, "nexus".to_owned())).collect();
    let (_, holds) = super::fleets::tally(&sessions);
    let boxes = super::fleets::box_tally(&sessions);
    assert_eq!(holds.get(&super::IssueRef::new("north", 1041)).map(String::as_str), Some("fleet session north-1041-d0225 on nexus"), "a parked session's north fleet hold holds its issue: {holds:?}");
    let slug = crate::threads::claims::box_slug("a fleet session's build goes to the build host").expect("a box slug");
    assert!(boxes.get(&super::IssueRef::new("north", 1036)).is_some_and(|held| held.contains_key(&slug)), "{boxes:?}");
    let (hosts, accounts) = super::tests::fleet();
    let mut surveyed = super::tests::survey(vec![super::tests::issue("north", 1041, "- [ ] a\n- [ ] b", &[])], super::plan::Cloud::Unreachable(String::new()), hosts, accounts);
    surveyed.holds = holds;
    assert!(super::decide(&surveyed, "test").launches.is_empty(), "dispatch staffs no second worker beside a parked session");
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
        route: None,
            };
    let record = SessionRecord::launched(&request, None, None, false, 1_794_090_424_000);
    let laptop = serde_json::json!([{"name": record.name, "status": "working", "tasks": record.tasks().collect::<Vec<_>>()}]);
    let sessions: Vec<_> = super::fleets::running_sessions(&laptop).into_iter().map(|session| (session, "whiterabbit".to_owned())).collect();

    let held = super::fleets::held(&[super::IssueRef::new("north", 855)], &sessions, &std::collections::BTreeMap::new());

    assert_eq!(held.map(|(_, holder)| holder), Some("fleet session frontier-run on whiterabbit".to_owned()));
}
