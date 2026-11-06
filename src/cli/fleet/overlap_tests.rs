use super::*;

#[test]
fn a_second_launch_for_an_item_with_a_different_brief_never_reports_the_open_box_as_its_own() {
    let first = "Item: north#112\nstop-ask-guard\n\nFollows: `north#200 (lane lane-a changes x.rs)`\n";
    let second = "Item: north#112\nlf-prune-b\n\nFollows: `north#201 (lane lane-b changes x.rs)`\n";
    let boxes = serde_json::json!([{"id": 337, "item": "north#112", "state": "queued", "follows": "north#200 (lane lane-a changes x.rs)", "brief": first}]);
    assert_eq!(same_box(&boxes, "north#112", second), None);
    assert_eq!(same_box(&boxes, "north#112", first).as_deref(), Some("337"));
    assert_eq!(same_box(&boxes, "north#113", first), None);
}
