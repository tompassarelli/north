use super::*;
use crate::watch::Detail;

fn red(lanes: std::ops::Range<usize>, summary: &str) -> Alert {
    let items = lanes.map(|lane| format!("n842-very-long-lane-name-{lane}")).collect();
    Alert { key: "land:north:possible-red-main:agent_text_reads".into(), severity: Severity::Page, repo: None, owner: None, summary: summary.into(), since: 0, detail: Detail { lead: "it failed the land checks of".into(), unit: "lanes".into(), items } }
}

#[test]
fn an_alert_line_stays_under_300_characters_and_after_its_first_says_only_what_changed() {
    let summary = "north: possible red main: agent_text_reads";
    let first = news(&red(0..40, summary), None, LINE_CHARS).unwrap();
    assert!(first.chars().count() <= LINE_CHARS, "north#909: every probe's whole lane list made a 4,000-character alert: {first}");
    assert!(first.starts_with("north: possible red main: agent_text_reads; it failed the land checks of 40 lanes (n842-very-long-lane-name-0, "), "{first}");
    assert!(first.ends_with("more); all: north ops watch show land_north_possible-red-main_agent_text_reads"), "{first}");
    let told = Incident { summary: summary.into(), severity: Severity::Page, owner: None, since: 0, last_sent: 0, desktop: false, told: red(0..40, summary).detail.items, told_summary: summary.into() };
    assert_eq!(news(&red(0..40, summary), Some(&told), LINE_CHARS), None, "a still with nothing new sends nothing");
    assert_eq!(news(&red(0..41, summary), Some(&told), LINE_CHARS).unwrap(), "north: possible red main: agent_text_reads; it failed the land checks of 1 more lanes (n842-very-long-lane-name-40); all: north ops watch show land_north_possible-red-main_agent_text_reads");
    let long = "x".repeat(400);
    assert!(news(&red(0..3, &long), None, LINE_CHARS).unwrap().chars().count() <= LINE_CHARS);
}

#[test]
fn a_cleared_alert_leaves_no_detail_for_show_to_print() {
    let state = tempfile::tempdir().unwrap();
    let incidents = state.path().join("incidents.json");
    let standing = Alert { severity: Severity::Info, ..red(0..2, "north: possible red main: agent_text_reads") };
    let detail = detail_file(state.path(), &detail_name(&standing.key));
    deliver(state.path(), &incidents, std::slice::from_ref(&standing), &[], 60, &Fleet::default());
    assert!(detail.exists(), "a standing alert keeps its lane list for north ops watch show");
    deliver(state.path(), &incidents, &[], &[], 120, &Fleet::default());
    assert!(!detail.exists(), "on 2026-11-11 north ops watch show still printed a possible red main the watch had cleared, because the cleared alert's detail file stayed");
}
