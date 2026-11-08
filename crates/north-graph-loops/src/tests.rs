use north_graph::Graph;
use serde_json::json;

use crate::{Cells, read_from, tell_to as tell, ticks_from};

#[test]
fn a_tick_that_moves_a_cell_leaves_dispatch_only_the_new_value_and_the_new_tick() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first: Cells = [("eta_minutes opus feature", json!(92.0)), ("budget_usd opus feature", json!(12.67)), ("effort feature", json!("high"))].into_iter().map(|(cell, value)| (cell.to_owned(), value)).collect();
    let empty = read_from(&graph, "tune").unwrap();
    tell(&mut graph, "tune", &empty, &first, 1_000).unwrap();
    let told = read_from(&graph, "tune").unwrap();
    assert_eq!((told.cells.clone(), told.at), (first.clone(), Some(1_000)));
    let mut second = first;
    second.insert("eta_minutes opus feature".into(), json!(141.0));
    second.remove("effort feature");
    tell(&mut graph, "tune", &told, &second, 2_000).unwrap();
    let told = read_from(&graph, "tune").unwrap();
    assert_eq!((told.cells, told.at), (second, Some(2_000)));
}

#[test]
fn a_finished_lease_leaves_admission_its_signature_s_newest_estimate_and_the_capacity_tick() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first: crate::Estimate = [("runs", 4.0), ("cores", 6.5), ("memory-mb", 9100.0), ("wall-s", 310.0)].into_iter().map(|(measure, value)| (measure.to_owned(), value)).collect();
    crate::tell_estimate_to(&mut graph, "a1", &first, 1_000).unwrap();
    crate::tell_estimate_to(&mut graph, "b2", &first, 1_500).unwrap();
    let mut second = first.clone();
    second.insert("cores".into(), 7.0);
    second.remove("wall-s");
    crate::tell_estimate_to(&mut graph, "a1", &second, 2_000).unwrap();
    let told = crate::estimates_from(&graph).unwrap();
    assert_eq!(told.get("a1"), Some(&second));
    assert_eq!(told.get("b2"), Some(&first));
    assert_eq!(ticks_from(&graph).unwrap().get("capacity"), Some(&2_000));
    assert!(crate::tell_estimate_to(&mut graph, "a1", &[("flavour".to_owned(), 1.0)].into(), 3_000).is_err());
}

#[test]
fn a_reaper_tick_leaves_the_fleet_only_the_sessions_it_still_holds_and_the_new_dead_letter_count() {
    let mut graph = Graph::open_in_memory().unwrap();
    let verdicts = |held: &[(&str, &str)]| held.iter().map(|(name, verdict)| ((*name).to_owned(), (*verdict).to_owned())).collect();
    let first = crate::Retirement { verdicts: verdicts(&[("n1", "idle"), ("n2", "parked")]), dead_letters_24h: Some(3) };
    crate::tell_retirement_to(&mut graph, &first, 1_000).unwrap();
    assert_eq!(crate::retirement_from(&graph).unwrap(), first);
    let second = crate::Retirement { verdicts: verdicts(&[("n2", "stale")]), dead_letters_24h: Some(0) };
    crate::tell_retirement_to(&mut graph, &second, 2_000).unwrap();
    assert_eq!(crate::retirement_from(&graph).unwrap(), second);
    assert_eq!(ticks_from(&graph).unwrap().get("fleet"), Some(&2_000));
}
