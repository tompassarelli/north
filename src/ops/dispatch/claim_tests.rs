#[test]
fn a_claim_by_a_fleet_session_that_no_host_runs_holds_nothing_and_a_hand_made_one_still_does() {
    let fleets = vec![
        ("gex45".to_owned(), serde_json::json!([{"name": "sc-9-a", "status": "exited"}, {"name": "sc-10-a", "status": "working"}, {"name": "sc-11-a", "status": "exited"}])),
        ("nexus".to_owned(), serde_json::json!([{"name": "sc-11-a", "status": "idle"}])),
    ];
    let stopped = super::fleets::stopped_names(&fleets);
    assert_eq!(stopped.iter().map(String::as_str).collect::<Vec<_>>(), ["sc-9-a"]);
    let listing = serde_json::json!([
        {"item": "smashcraft#9", "owner": "sc-9-a", "standing": "live"},
        {"item": "smashcraft#10", "owner": "sc-10-a", "standing": "live"},
        {"item": "smashcraft#11", "owner": "sc-11-a", "standing": "live"},
        {"item": "smashcraft#12", "owner": "tom", "standing": "live"},
    ]);
    let held: Vec<String> = super::survey::claim_rows(&listing, "gex45", &stopped).into_iter().map(|(item, _)| item.to_string()).collect();
    assert_eq!(held, ["smashcraft#10", "smashcraft#11", "smashcraft#12"]);
}
