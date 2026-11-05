use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::host::Host;
use super::measure::issue_activity;
use super::render::format_row;
use super::tests::{Fake, at};
use super::throughput::{Inputs, throughput};
use super::windows::{waiting, waits, window_opens};

const OBSERVED: &str = "Context.\n- [x] The recording lands.\n- [ ] Box counts report waiting boxes apart, after 2026-11-06 over 7 d of `north flow`.\n";
const WORKABLE: &str = "- [ ] The box count prints workable and waiting counts.\n";
const MIXED: &str = "- [ ] Observed after 2026-11-06.\n- [ ] Code that can land today.\n";
const GATED: &str = "- [ ] Refusals over the 7-day window (not before 2026-11-05T18:00Z)\n";

#[test]
fn a_box_with_an_after_window_waits_until_the_day_after_its_date_and_a_workable_line_makes_its_issue_workable() {
    let now = at("2026-11-05T12:00:00Z");
    assert_eq!(window_opens("after 2026-11-06"), Some(at("2026-11-07T00:00:00Z") as i64));
    assert_eq!(window_opens("(not before 2026-11-05T18:00Z)"), Some(at("2026-11-05T18:00:00Z") as i64));
    assert!(waits(OBSERVED, now));
    assert!(waits(OBSERVED, at("2026-11-06T23:59:00Z")));
    assert!(!waits(OBSERVED, at("2026-11-07T00:00:00Z")), "the box is workable once its window opens");
    assert!(!waits(WORKABLE, now));
    assert!(!waits(MIXED, now), "an issue with one box that can close today is workable");
    assert!(waits(GATED, now) && !waits(GATED, at("2026-11-05T18:00:00Z")));
    assert!(!waits("- [x] Done after 2026-11-06.\n", now), "a ticked box waits on nothing");
    let bodies = [OBSERVED, WORKABLE, MIXED, GATED].map(String::from);
    assert_eq!(waiting(&bodies, now), 2);
}

#[test]
fn north_flow_prints_workable_and_waiting_box_counts_and_needs_a_closing_rate_only_for_workable_boxes() {
    let now = at("2026-11-05T12:00:00Z");
    let claims = BTreeMap::new();
    let read = |waiting| throughput(&Inputs { repo: "north", now, open_count: Some(226), waiting, closed: &[], main_log: &[], landings: &[], pushes: &[], target: Some(now + 6.0 * 3600.0), dead_lanes: &[], claims: &claims, pending_since: &[] });
    let split = read(Some(44));
    assert_eq!((split.workable_boxes, split.waiting_boxes), (Some(182), Some(44)));
    assert_eq!(split.needed_per_hour, Some(182.0 / 6.0));
    let unread = read(None);
    assert_eq!((unread.workable_boxes, unread.waiting_boxes, unread.needed_per_hour), (Some(226), None, Some(226.0 / 6.0)));
    let mut row = json!({"repo": "north", "pending": 0, "landed_per_hour": 1, "push_to_land_minutes": 5, "push_to_land_p90_minutes": 9});
    row.as_object_mut().unwrap().extend(serde_json::to_value(&split).unwrap().as_object().unwrap().clone());
    row["target"] = Value::Null;
    let line = format_row(&row, Some(&[]));
    assert!(line.contains(", boxes workable 182 waiting 44"), "{line}");
    row["waiting_boxes"] = Value::Null;
    assert!(format_row(&row, Some(&[])).contains(", boxes workable 182 waiting n/a"));
}

#[test]
fn issue_activity_pages_every_open_body_and_counts_a_fixture_box_with_an_after_window_as_waiting() {
    let now = at("2026-11-05T12:00:00Z");
    let reply = |input: &str| -> Value {
        let query = serde_json::from_str::<Value>(input).unwrap()["query"].as_str().unwrap().to_owned();
        if query.contains("search(") {
            return json!({"data": {"repository": {"issues": {"totalCount": 3}, "sized": {"nodes": []}}, "search": {"nodes": []}}});
        }
        let (bodies, next) = if query.contains("after: \"page-2\"") { (vec![WORKABLE], false) } else { (vec![OBSERVED, MIXED], true) };
        let nodes: Vec<Value> = bodies.into_iter().map(|body| json!({"body": body})).collect();
        json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": next, "endCursor": "page-2"}, "nodes": nodes}}}})
    };
    let host = GraphqlFake(&reply);
    let (open, waiting, closed) = issue_activity(&host, "tompassarelli/north", now);
    assert_eq!((open, waiting, closed.map(|closed| closed.len())), (Some(3), Some(1), Some(0)));
}

struct GraphqlFake<'a>(&'a dyn Fn(&str) -> Value);

impl Host for GraphqlFake<'_> {
    fn output(&self, argv: &[&str], cwd: Option<&std::path::Path>, input: Option<&str>, limit: std::time::Duration) -> Option<std::process::Output> {
        let said = |_: &[&str]| Some((self.0)(input.unwrap_or_default()).to_string());
        Fake(&said).output(argv, cwd, input, limit)
    }
}
