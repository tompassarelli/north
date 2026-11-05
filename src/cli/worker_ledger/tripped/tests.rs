use super::line_for;
use crate::usage::breaker::trips_in;

const LEDGER: &str = concat!(
    r#"{"at_ms":1,"kind":"trip","scope":"workstream:north#385","spent":1.9,"p50":0.2,"p90":0.37,"by":null}"#,
    "\n",
    r#"{"at_ms":2,"kind":"warn","scope":"workstream:north#88","spent":9.0,"p50":2.0,"p90":4.0,"by":null}"#,
    "\n",
);

#[test]
fn recommend_prints_the_tripped_state_of_the_briefs_workstream() {
    let line = line_for("Item: north#385 (box: x)\nCategory: feature", &trips_in(LEDGER)).unwrap();
    assert!(line.starts_with("tripped: north#385 spent $1.90 against p90 $0.37"), "{line}");
}

#[test]
fn recommend_prints_nothing_for_an_untripped_cleared_or_unnamed_workstream() {
    assert_eq!(line_for("Item: north#88\nCategory: feature", &trips_in(LEDGER)), None);
    assert_eq!(line_for("Category: feature", &trips_in(LEDGER)), None);
    let cleared = format!("{LEDGER}{}\n", r#"{"at_ms":3,"kind":"clear","scope":"workstream:north#385","spent":1.9,"p50":0.2,"p90":1.0,"by":"tom"}"#);
    assert_eq!(line_for("Item: north#385", &trips_in(&cleared)), None);
}
