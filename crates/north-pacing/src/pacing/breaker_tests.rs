use crate::spend::tripwire::{self, Burst, Sample, judge};
use crate::spend::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_794_300_000.0;
const MINUTE: f64 = 60.0;
const HOUR: f64 = 3600.0;

fn rules() -> Rules {
    Rules { reserve: 5.0, floor_week: 5.0, floor_five_hour: 10.0, charge: 1.0, fleet_trip: Burst { points: 30.0, minutes: 15.0 }, session_trip: Burst { points: 10.0, minutes: 10.0 }, reading_max_h: 2.0 }
}

fn account(name: &str, five_hour: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + HOUR) }), week: Some(Window { used_pct: 10.0, resets_at: Some(NOW + 100.0 * HOUR) }), counted: true }
}

fn readings(from: f64, to: f64) -> [Sample; 2] {
    [Sample { at: NOW - 15.0 * MINUTE, used_pct: from, resets_at: Some(NOW + HOUR) }, Sample { at: NOW, used_pct: to, resets_at: Some(NOW + HOUR) }]
}

/// A 25-point rise in 15 minutes is under the 30-point burst tripwire but, held for the hour left to the reset, reaches 100%.
#[test]
fn a_circuit_breaker_trips_an_account_whose_fifteen_minute_burn_would_exhaust_its_window() {
    let rules = rules();
    let trip = tripwire::trip(&rules, "claude:a", &readings(30.0, 55.0), &[], 0.0, NOW).expect("25 points in 15 minutes reaches 155% by the reset");
    assert!(trip.line.contains("circuit breaker"), "{}", trip.line);
    assert_eq!(tripwire::trip(&rules, "claude:b", &readings(30.0, 35.0), &[], 0.0, NOW), None, "5 points in 15 minutes reaches 55%");

    let view = View { at: NOW, accounts: vec![account("claude:a", 55.0), account("claude:b", 35.0)], trips: vec![trip.clone()], ..View::default() };
    assert_eq!(view.admit(&rules, "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Tripwire));
    assert_eq!(view.admit(&rules, "claude:b", Starter::Worker, Placed::default()), Ok(()), "only the tripped account refuses");

    let opened = judge(&[], vec![trip.clone()], NOW, &rules, |_| 0.0);
    assert_eq!(opened.new, vec![trip]);
    assert!(opened.closed.is_empty());

    let held = judge(&opened.current, Vec::new(), NOW + 5.0 * MINUTE, &rules, |_| 0.0);
    assert_eq!(held.current.len(), 1, "the breaker holds until the burst leaves its window");
    assert!(held.closed.is_empty());

    let later = NOW + 20.0 * MINUTE;
    let closed = judge(&opened.current, Vec::new(), later, &rules, |_| 0.0);
    assert!(closed.current.is_empty(), "the breaker closes once the burn has dropped back");
    assert_eq!(closed.closed.len(), 1);
    assert_eq!(closed.closed[0].closed_at, Some(later));
    assert!(judge(&closed.closed, Vec::new(), later + MINUTE, &rules, |_| 0.0).closed.is_empty(), "a close is logged once");
}
