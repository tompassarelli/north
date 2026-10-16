use std::path::PathBuf;

use serde_json::{Value, json};

use super::super::dividend::{self, Verdict};
use super::super::facts::{Facts, Landing};
use super::*;

const EXPECTATIONS: &str = include_str!("../../../harness/ops/expectations.toml");
const NOW: i64 = 1_792_143_600;

fn expectations() -> Vec<Expectation> {
    load(EXPECTATIONS).expect("harness/ops/expectations.toml loads")
}

fn expectation(id: &str) -> Expectation {
    expectations().into_iter().find(|expectation| expectation.id == id).unwrap_or_else(|| panic!("no expectation {id}"))
}

fn fixture(id: &str) -> Option<Value> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("src/ops/expect/fixtures/{id}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn facts(value: Value) -> Facts {
    serde_json::from_value(value).expect("facts decode")
}

#[test]
fn every_expectation_names_an_owner_and_a_fix_and_has_a_breach_fixture_that_fires() {
    for expectation in expectations() {
        assert!(!expectation.owner.is_empty() && !expectation.fix.is_empty(), "{} needs an owner and a fix", expectation.id);
        let fixture = fixture(&expectation.id).unwrap_or_else(|| panic!("{} has no breach fixture", expectation.id));
        assert_eq!(fixture["expectation"], expectation.id.as_str());
        let outcome = evaluate(&expectation, &facts(fixture["facts"].clone()));
        assert_eq!(outcome.state, State::Breached, "{}", outcome.line());
    }
}

#[test]
fn an_unknown_metric_or_a_broken_file_is_refused() {
    let broken = EXPECTATIONS.replacen("release_items_closed", "release_items_shut", 1);
    assert!(load(&broken).unwrap_err().contains("unknown metric release_items_shut"));
    assert!(load("[[expectation]]\nid = 1").is_err());
}

fn landing(repo: &str, at: i64) -> Landing {
    Landing { at, repo: repo.into(), sha: format!("{repo}{at}"), committed_at: None, issue_refs: vec![] }
}

#[test]
fn healthy_facts_hold() {
    let cloud = facts(json!({"now": NOW, "cloud": [{"id": "s", "status": "running", "placed_at": NOW - 600, "title": "box"}]}));
    assert_eq!(evaluate(&expectation("cloud-lane"), &cloud).state, State::Holds);
    let lock = facts(json!({"now": NOW, "locks": [{"repo": "north", "holder": {"pid": 1, "dir": "/x", "session": null}, "held_since": NOW - 300, "waiters": []}]}));
    assert_eq!(evaluate(&expectation("landing-lock"), &lock).state, State::Holds);
    let mut game = Facts { now: NOW, ..Facts::default() };
    game.landings = vec![landing("smashcraft", NOW - 60), landing("wisp", NOW - 120), landing("smashcraft", NOW - 180), landing("north", NOW - 240)];
    assert_eq!(evaluate(&expectation("game-share"), &game).value, Some(0.75));
    assert_eq!(evaluate(&expectation("game-share"), &game).state, State::Holds);
    let beats = facts(json!({"now": NOW, "checks": (0..288).map(|n| NOW - n * 300).collect::<Vec<_>>()}));
    assert_eq!(evaluate(&expectation("alerts"), &beats).state, State::Holds);
}

#[test]
fn missing_sources_are_unknown_not_breached_and_an_expired_expectation_is_inactive() {
    let blind = Facts { now: NOW, ..Facts::default() };
    for id in ["cloud-lane", "release-closed", "laptop-share", "account-exhaustion", "game-share"] {
        assert_eq!(evaluate(&expectation(id), &blind).state, State::Unknown, "{id}");
    }
    let after_credit = Facts { now: 1_794_000_000, cloud: Some(vec![]), ..Facts::default() };
    assert_eq!(evaluate(&expectation("cloud-lane"), &after_credit).state, State::Inactive);
}

#[test]
fn an_idle_account_with_no_weekly_room_is_not_counted_against_balance() {
    let pace = json!({"accounts": {
        "claude:a": {"windows": [{"window": "5h", "used_pct": 90.0}, {"window": "week", "used_pct": 50.0}]},
        "claude:b": {"windows": [{"window": "5h", "used_pct": 0.0}, {"window": "week", "used_pct": 99.0}]}
    }});
    let facts = Facts { now: NOW, pace: Some(pace), ..Facts::default() };
    assert_eq!(evaluate(&expectation("account-balance"), &facts).state, State::Holds);
}

#[test]
fn a_dividend_reports_seven_daily_values_and_decays_when_today_breaches() {
    let bet = super::super::facts::BetIssue { reference: "north#112".into(), title: "dividends".into(), open: true, closed_at: None, size: None, dividends: vec!["cloud-lane".into()] };
    let mut facts = Facts { now: NOW, cloud: Some(vec![]), bet_issues: Some(vec![bet]), ..Facts::default() };
    let placed = |back: i64| super::super::facts::CloudSession { id: format!("s{back}"), status: "idle".into(), placed_at: NOW - back * 86_400 - 60, title: String::new() };
    facts.cloud = Some((1..=6).map(placed).collect());
    let decayed = dividend::evaluate(&expectation("cloud-lane"), &facts);
    assert_eq!(decayed.verdict, Verdict::Decayed);
    assert_eq!(decayed.daily, vec![Some(1.0), Some(1.0), Some(1.0), Some(1.0), Some(1.0), Some(1.0), Some(0.0)]);
    assert_eq!(decayed.issues, vec!["north#112".to_owned()]);
    facts.cloud.as_mut().unwrap().push(placed(0));
    assert_eq!(dividend::evaluate(&expectation("cloud-lane"), &facts).verdict, Verdict::Paying);
}
