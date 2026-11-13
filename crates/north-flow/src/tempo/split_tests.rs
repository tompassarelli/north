use super::{Stamped, leaves};
use crate::tempo::landing::percentiles;

fn span(id: &str, parent: Option<&str>, name: &str, start: f64, end: f64) -> Stamped {
    Stamped { id: id.to_owned(), parent: parent.map(str::to_owned), trace: "enqueue:head".to_owned(), name: name.to_owned(), start, end }
}

#[test]
fn a_stage_with_steps_counts_its_steps_and_not_its_whole_once_more() {
    let spans = [
        span("setup", None, "before enqueue", -1.0, 0.0),
        span("stage", None, "queue checks", 0.0, 440.0),
        span("lease", Some("stage"), "queue checks / lease", 0.0, 4.0),
        span("command", Some("stage"), "queue checks / command", 4.0, 10.0),
        span("rest", Some("stage"), "queue checks / not in a step", 10.0, 440.0),
        span("ticket", None, "ticket", 440.0, 440.5),
    ];
    let parts = leaves(&spans, 441.5);
    assert_eq!(parts.len(), 5);
    assert_eq!(parts.get("queue checks"), None);
    assert_eq!(parts.get("unspanned"), None);
    assert_eq!(parts.get("before enqueue"), Some(&1.0));
    assert_eq!(parts.get("queue checks / lease"), Some(&4.0));
    assert_eq!(parts.get("queue checks / command"), Some(&6.0));
    assert_eq!(parts.get("queue checks / not in a step"), Some(&430.0));
    assert_eq!(parts.get("ticket"), Some(&0.5));
}

#[test]
fn the_part_medians_of_overlapping_checks_sum_to_the_call_median() {
    let lanes = [
        (12.0, vec![span("a", None, "check a", 0.0, 6.0), span("b", None, "check b", 0.0, 6.0), span("c", None, "ticket", 8.0, 10.0)]),
        (12.0, vec![span("a", None, "check a", 1.0, 7.0), span("b", None, "check b", 2.0, 8.0), span("c", None, "ticket", 9.0, 11.0)]),
        (12.0, vec![span("a", None, "check a", 0.0, 5.0), span("b", None, "check b", 0.0, 5.0), span("c", None, "ticket", 10.0, 12.0)]),
    ];
    let per_lane: Vec<_> = lanes.iter().map(|(seconds, spans)| leaves(spans, *seconds)).collect();
    let names: std::collections::BTreeSet<&String> = per_lane.iter().flat_map(|lane| lane.keys()).collect();
    let sum: f64 = names
        .into_iter()
        .map(|name| percentiles(&per_lane.iter().map(|lane| lane.get(name).copied().unwrap_or(0.0)).collect::<Vec<f64>>()).0.unwrap_or(0.0))
        .sum();
    assert!((sum - 12.0).abs() <= 1.2, "part medians sum to {sum} s against a 12 s call");
    assert!(per_lane[0].contains_key("unspanned"));
}
