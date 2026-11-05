use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;

use super::routing::{alert, routing_plan};
use super::tests::{Fake, places};
use crate::ledger::closed::{ArmBox, ClosedReport};

const NOW: f64 = 1_793_300_000.0;

fn ledger(path: &Path, sonnet: usize, opus: usize) {
    let connection = Connection::open(path).unwrap();
    connection.execute("CREATE TABLE runs(category TEXT, arm TEXT)", []).unwrap();
    for (arm, runs) in [("sonnet-high", sonnet), ("worker", opus)] {
        for _ in 0..runs {
            connection.execute("INSERT INTO runs VALUES ('feature', ?1)", [arm]).unwrap();
        }
    }
}

fn report(arms: &[&str]) -> ClosedReport {
    let boxes = arms.iter().flat_map(|arm| (0..4).map(move |number| ArmBox { item: format!("north#{arm}-{number}"), tier: String::new(), arm: (*arm).into(), usd: 8.0, category: "feature".into(), closed: true, reworked: false }));
    ClosedReport { skipped: Vec::new(), closed: 0, cells: Vec::new(), arms: boxes.collect(), spend_usd: 0.0, leads_usd: 0.0 }
}

fn run(sonnet: usize, opus: usize, closed: Result<ClosedReport, String>) -> Vec<String> {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    ledger(&places.ledger, sonnet, opus);
    let sent = RefCell::new(Vec::new());
    let reply = |argv: &[&str]| {
        if argv.first() == Some(&"supply-alert") {
            sent.borrow_mut().push(argv[1..].join("|"));
            return Some(String::new());
        }
        None
    };
    alert(&places, &Fake(&reply), &BTreeMap::new(), || closed, NOW).unwrap();
    sent.into_inner()
}

#[test]
fn full_arms_with_an_unreadable_closed_report_raise_one_supply_alert_naming_the_category_and_the_input() {
    let sent = run(20, 20, Err("gh issue list tompassarelli/galileo: not found".into()));
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("normal|Tier routing of feature still alternates") && sent[0].contains("north ledger summary --since 30d is unread: gh issue list"), "{sent:?}");
}

#[test]
fn full_arms_with_a_report_that_prices_both_arms_raise_nothing() {
    assert!(run(20, 20, Ok(report(&["sonnet-high", "worker"]))).is_empty());
}

#[test]
fn full_arms_whose_report_prices_only_one_arm_name_the_unpriced_one_and_short_arms_raise_nothing() {
    let sent = run(20, 20, Ok(report(&["sonnet-high"])));
    assert!(sent.len() == 1 && sent[0].contains("prices worker"), "{sent:?}");
    assert!(run(20, 14, Err("unread".into())).is_empty());
}

#[test]
fn the_alert_repeats_after_an_hour_not_before_and_clears_when_the_category_routes() {
    let stalled = [("feature".to_owned(), "unread".to_owned())];
    let (mut state, mut sent) = (BTreeMap::new(), Vec::new());
    for minute in 0..=125 {
        let (next, actions) = routing_plan(&stalled, &state, NOW + f64::from(minute) * 60.0);
        state = next;
        for action in actions {
            state.get_mut(&action.repo).unwrap().sent.insert(action.key, NOW + f64::from(minute) * 60.0);
            sent.push(minute);
        }
    }
    assert_eq!(sent, [0, 59, 118]);
    let (cleared, actions) = routing_plan(&[], &state, NOW + 8000.0);
    assert!(cleared.is_empty() && actions.is_empty());
}
