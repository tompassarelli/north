use std::time::Duration;

use serde_json::json;

use super::overrun::{Overrun, Reader, Usual};

fn command() -> Vec<String> {
    ["cargo", "nextest", "run", "-p", "north-fleet"].map(str::to_owned).to_vec()
}

#[test]
fn a_lease_past_its_usual_tells_its_session_then_its_parent_and_a_thin_signature_measures_against_its_deadline() {
    let measured = Usual::of(Some(&json!({"basis": "key", "runs": 12, "wallSeconds": 40.0})), Some(600));
    let mut overrun = Overrun::watch(Some("w1".to_owned()), &command(), measured).expect("a fleet session with a measured signature is watched");
    assert_eq!(overrun.due(), Some(Duration::from_secs(120)));
    let (reader, line) = overrun.next(Duration::from_secs(120)).expect("the session line");
    assert_eq!(reader, Reader::Session);
    assert!(line.contains("has run 2 min") && line.contains("usually takes 40 s (median of 12 runs)") && line.contains("3x"), "{line}");
    assert_eq!(overrun.due(), Some(Duration::from_secs(240)));
    let (reader, line) = overrun.next(Duration::from_secs(240)).expect("the parent line");
    assert_eq!(reader, Reader::Parent);
    assert!(line.contains("w1's") && line.contains("has run 4 min") && line.contains("6x"), "{line}");
    assert_eq!(overrun.due(), None);

    let thin = Usual::of(Some(&json!({"basis": "unknown", "runs": 2, "wallSeconds": null})), Some(600));
    let mut overrun = Overrun::watch(Some("w1".to_owned()), &command(), thin).expect("a thin signature with a deadline is watched");
    assert_eq!(overrun.due(), Some(Duration::from_secs(300)));
    let (_, line) = overrun.next(Duration::from_secs(300)).expect("the thin session line");
    assert!(line.contains("too few runs") && line.contains("(2 of 5)") && line.contains("50% of its declared deadline of 10 min"), "{line}");
    assert_eq!(overrun.due(), Some(Duration::from_secs(480)));
    assert!(Overrun::watch(Some("w1".to_owned()), &command(), Usual::of(None, None)).is_none());
}
