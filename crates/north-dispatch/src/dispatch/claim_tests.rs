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
