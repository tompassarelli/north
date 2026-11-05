use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::boxes::{Boxes, Standing, count, standing, unticked, window_opens};
use super::host::Host;
use super::measure::issue_activity;
use super::render::format_row;
use super::tests::{Fake, at};
use super::throughput::{Inputs, throughput};

const OBSERVED: &str = "Context.\n- [x] The recording lands.\n- [ ] Box counts report waiting boxes apart, after 2026-11-06 over 7 d of `north flow`.\n";
const WORKABLE: &str = "- [ ] The box count prints workable and waiting counts.\n";
const NESTED: &str = "- [ ] Parent box that can close today.\n  - [ ] Nested box (waits: 2026-11-12, 7 d of samples)\n    * [ ] Deeper box (decision: Tom: which account pays)\n";
const GATED: &str = "- [ ] Refusals over the 7-day window (not before 2026-11-05T18:00Z)\n";
const TOM: &str = "Intro.\n- [ ] Ship it (decision: Tom: rename or keep)\n- [ ] Read after 2026-11-20 (waits: 2026-11-20, a fortnight)\n";

fn now() -> f64 {
    at("2026-11-05T12:00:00Z")
}

#[test]
fn an_after_window_waits_through_its_date_and_opens_the_next_midnight() {
    assert_eq!(window_opens("after 2026-11-06"), Some(at("2026-11-07T00:00:00Z") as i64));
    assert_eq!(standing("- [ ] count after 2026-11-06", now()), Standing::Waiting);
    assert_eq!(standing("- [ ] count after 2026-11-06", at("2026-11-06T23:59:00Z")), Standing::Waiting);
    assert_eq!(standing("- [ ] count after 2026-11-06", at("2026-11-07T00:00:00Z")), Standing::Workable);
    assert_eq!(standing("- [ ] count after 2026-11-04", now()), Standing::Workable, "a window already open is workable");
}

#[test]
fn a_not_before_window_waits_until_its_minute() {
    assert_eq!(window_opens("(not before 2026-11-05T18:00Z)"), Some(at("2026-11-05T18:00:00Z") as i64));
    assert_eq!(standing(GATED, now()), Standing::Waiting);
    assert_eq!(standing(GATED, at("2026-11-05T18:00:00Z")), Standing::Workable);
    assert_eq!(standing("- [ ] read (not before 2026-11-06)", now()), Standing::Waiting);
}

#[test]
fn a_waits_marker_holds_its_box_through_the_date_it_names() {
    let line = "- [ ] Nested box (waits: 2026-11-12, 7 d of samples)";
    assert_eq!(window_opens(line), Some(at("2026-11-13T00:00:00Z") as i64));
    assert_eq!(standing(line, now()), Standing::Waiting);
    assert_eq!(standing(line, at("2026-11-12T23:00:00Z")), Standing::Waiting);
    assert_eq!(standing(line, at("2026-11-13T00:00:00Z")), Standing::Workable);
    assert_eq!(standing("- [ ] stale (waits: 2026-11-01, done waiting)", now()), Standing::Workable);
    assert_eq!(standing("- [ ] prose that waits: 2026-11-12 without a marker", now()), Standing::Workable, "only the parenthesised marker holds a box");
}

#[test]
fn a_decision_marker_counts_its_box_as_a_decision_for_tom_whatever_its_date() {
    assert_eq!(standing("- [ ] Ship it (decision: Tom: rename or keep)", now()), Standing::Decision);
    assert_eq!(standing("- [ ] Pick (Decision: tom: a or b) after 2026-11-06", now()), Standing::Decision);
    assert_eq!(standing("- [ ] A decision: Tom made it already", now()), Standing::Workable);
}

#[test]
fn every_unticked_line_is_a_box_nested_ones_included_and_an_issue_with_no_workable_box_is_held() {
    assert_eq!(unticked(NESTED).count(), 3);
    assert_eq!(unticked("- [x] done\n- [X] done\n1. [ ] numbered\n").count(), 1);
    let bodies = [OBSERVED, WORKABLE, NESTED, GATED, TOM, "No boxes at all.\n"].map(String::from);
    assert_eq!(count(&bodies, now()), Boxes { workable: 2, waiting: 4, decision: 2, held_issues: 3 });
}

#[test]
fn north_flow_prints_workable_waiting_and_decision_box_counts_and_needs_a_rate_only_for_issues_with_a_workable_box() {
    let claims = BTreeMap::new();
    let read = |boxes| throughput(&Inputs { repo: "north", now: now(), open_count: Some(30), boxes, closed: &[], main_log: &[], landings: &[], pushes: &[], target: Some(now() + 6.0 * 3600.0), dead_lanes: &[], claims: &claims, pending_since: &[] });
    let split = read(Some(Boxes { workable: 182, waiting: 40, decision: 4, held_issues: 6 }));
    assert_eq!((split.workable_boxes, split.waiting_boxes, split.decision_boxes), (Some(182), Some(40), Some(4)));
    assert_eq!(split.needed_per_hour, Some(24.0 / 6.0));
    let unread = read(None);
    assert_eq!((unread.workable_boxes, unread.waiting_boxes, unread.decision_boxes, unread.needed_per_hour), (None, None, None, Some(5.0)));
    let mut row = json!({"repo": "north", "pending": 0, "landed_per_hour": 1, "push_to_land_minutes": 5, "push_to_land_p90_minutes": 9});
    row.as_object_mut().unwrap().extend(serde_json::to_value(&split).unwrap().as_object().unwrap().clone());
    row["target"] = Value::Null;
    let line = format_row(&row, Some(&[]));
    assert!(line.contains(", boxes workable 182 waiting 40 decision 4"), "{line}");
    row.as_object_mut().unwrap().extend(serde_json::to_value(&unread).unwrap().as_object().unwrap().clone());
    row["target"] = Value::Null;
    assert!(format_row(&row, Some(&[])).contains(", boxes workable n/a waiting n/a decision n/a"));
}

#[test]
fn issue_activity_pages_every_open_body_and_counts_its_boxes() {
    let reply = |input: &str| -> Value {
        let query = serde_json::from_str::<Value>(input).unwrap()["query"].as_str().unwrap().to_owned();
        if query.contains("search(") {
            return json!({"data": {"repository": {"issues": {"totalCount": 3}, "sized": {"nodes": []}}, "search": {"nodes": []}}});
        }
        let (bodies, next) = if query.contains("after: \"page-2\"") { (vec![WORKABLE], false) } else { (vec![OBSERVED, NESTED], true) };
        let nodes: Vec<Value> = bodies.into_iter().map(|body| json!({"body": body})).collect();
        json!({"data": {"repository": {"issues": {"pageInfo": {"hasNextPage": next, "endCursor": "page-2"}, "nodes": nodes}}}})
    };
    let (open, boxes, closed) = issue_activity(&GraphqlFake(&reply), "tompassarelli/north", now());
    assert_eq!((open, boxes, closed.map(|closed| closed.len())), (Some(3), Some(Boxes { workable: 2, waiting: 2, decision: 1, held_issues: 1 }), Some(0)));
}

struct GraphqlFake<'a>(&'a dyn Fn(&str) -> Value);

impl Host for GraphqlFake<'_> {
    fn output(&self, argv: &[&str], cwd: Option<&std::path::Path>, input: Option<&str>, limit: std::time::Duration) -> Option<std::process::Output> {
        let said = |_: &[&str]| Some((self.0)(input.unwrap_or_default()).to_string());
        Fake(&said).output(argv, cwd, input, limit)
    }
}
