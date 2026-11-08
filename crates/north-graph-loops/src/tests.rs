use north_graph::Graph;
use serde_json::json;

use crate::{Cells, read_from, tell_to as tell};

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
