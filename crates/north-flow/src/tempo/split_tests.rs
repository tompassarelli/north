use super::{Stamped, leaves};

fn span(id: &str, parent: Option<&str>, name: &str, start: f64, end: f64) -> Stamped {
    Stamped { id: id.to_owned(), parent: parent.map(str::to_owned), trace: "enqueue:head".to_owned(), name: name.to_owned(), start, end }
}

#[test]
fn a_stage_with_steps_counts_its_steps_and_not_its_whole_once_more() {
    let spans = [
        span("stage", None, "queue checks", 0.0, 10.0),
        span("lease", Some("stage"), "queue checks / lease", 0.0, 4.0),
        span("command", Some("stage"), "queue checks / command", 4.0, 10.0),
        span("ticket", None, "ticket", 10.0, 10.5),
    ];
    let parts = leaves(&spans);
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.get("queue checks"), None);
    assert_eq!(parts.get("queue checks / lease"), Some(&4.0));
    assert_eq!(parts.get("queue checks / command"), Some(&6.0));
    assert_eq!(parts.get("ticket"), Some(&0.5));
}
