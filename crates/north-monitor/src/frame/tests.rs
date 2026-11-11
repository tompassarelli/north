use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;

use super::{View, draw};
use crate::budget::Budgets;
use crate::snapshot::Snapshot;

#[test]
fn an_over_budget_value_renders_red_and_a_stale_source_says_stale() {
    let snapshot: Snapshot = serde_json::from_str(include_str!("fixtures/over-budget-and-stale.json")).unwrap();
    let budgets = Budgets::defaults();
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| draw(frame, &View { snapshot: &snapshot, budgets: &budgets, offset_s: 0, mode: "test" })).unwrap();
    let buffer = terminal.backend().buffer();
    let rows: Vec<Vec<&str>> = (0..40).map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect()).collect();
    let (y, x) = rows
        .iter()
        .enumerate()
        .find_map(|(y, row)| row.concat().find("DEPTH 14").map(|byte| (y, row.concat()[..byte].chars().count())))
        .expect("the queue box shows the depth of 14 tickets");
    let depth = [&buffer[(x as u16 + 6, y as u16)], &buffer[(x as u16 + 7, y as u16)]];
    assert!(depth.iter().all(|cell| cell.fg == Color::Red), "a depth of 14 is past the red budget of 10");
    assert!(rows.iter().any(|row| row.concat().contains("north fleet list STALE 2m00s")), "a fleet reading 120 s old is past its 10 s bound");
    assert!(rows[0].concat().contains("landing queue depth 14"), "the status line names the red value first");
}
