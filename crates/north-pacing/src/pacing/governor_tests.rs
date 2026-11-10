use crate::spend::tripwire::{Burn, Burst, Sample};
use crate::spend::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_794_300_000.0;
const HOUR: f64 = 3600.0;

fn rules() -> Rules {
    Rules { reserve: 5.0, floor_week: 5.0, floor_five_hour: 10.0, charge: 1.0, fleet_trip: Burst { points: 30.0, minutes: 15.0 }, session_trip: Burst { points: 10.0, minutes: 10.0 }, week_burn_h: 6.0, reading_max_h: 2.0 }
}

fn account(name: &str, five_hour: f64, five_hour_resets: f64, week: f64, week_resets: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + five_hour_resets * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + week_resets * HOUR) }), counted: true }
}

fn burn(name: &str, five_hour_per_hour: f64, week_per_hour: f64) -> Burn {
    Burn { account: name.into(), at: NOW, five_hour: five_hour_per_hour / HOUR, week: week_per_hour / HOUR, five_hour_per_week: 0.0 }
}

fn refusal(view: &View, name: &str, placed: Placed) -> Option<crate::spend::Refusal> {
    view.admit(&rules(), name, Starter::Worker, placed).err()
}

/// The 2026-11-10 count cap refused launches while accounts had week to spare: admission reads each account's measured burn, projects it to the next five-hour and weekly reset with the new worker's charge, and refuses only the account whose projection leaves the approved budget.
#[test]
fn admission_is_one_burn_governor_per_account() {
    let hot = burn("claude:a", 20.0, 0.0);
    let calm = burn("claude:b", 2.0, 0.1);
    let view = View { at: NOW, accounts: vec![account("claude:a", 40.0, 3.0, 10.0, 100.0), account("claude:b", 40.0, 3.0, 10.0, 100.0)], burns: vec![hot, calm], ..View::default() };

    let refused = refusal(&view, "claude:a", Placed::default()).expect("40% at 20 points an hour for 3 hours reaches 100%, over the 90% budget");
    assert_eq!(refused.rule, Rule::Burn, "{refused}");
    for named in ["claude:a", "100%", "five-hour", "its reset in 3.0 h", "20.0 points an hour", "over the 90%"] {
        assert!(refused.line.contains(named), "{named} missing from {refused}");
    }
    assert_eq!(refusal(&view, "claude:b", Placed::default()), None, "the other account's burn is its own: 40% plus 6 points reaches 46%");
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Reserved, Placed::default()), Ok(()), "the proxy, routines and Tom are not governed");

    let weekly = View { at: NOW, accounts: vec![account("claude:c", 0.0, 4.0, 10.0, 100.0)], burns: vec![burn("claude:c", 0.0, 1.0)], ..View::default() };
    let refused = refusal(&weekly, "claude:c", Placed::default()).expect("10% plus 1 point an hour for 100 hours reaches 111%");
    assert!(refused.rule == Rule::Burn && refused.line.contains("week window") && refused.line.contains("111%") && refused.line.contains("its reset in 100.0 h"), "{refused}");
    assert_eq!(refusal(&View { drain: true, ..weekly }, "claude:c", Placed::default()), None, "drain skips the week's projection and keeps the floor");

    let charged = View { at: NOW, accounts: vec![account("claude:d", 0.0, 4.0, 90.0, 100.0), account("claude:g", 0.0, 4.0, 0.0, 100.0)], burns: vec![Burn { five_hour_per_week: 1.0, ..burn("claude:d", 0.0, 0.0) }], ..View::default() };
    assert_eq!(refusal(&charged, "claude:d", Placed::default()), None, "90% plus this worker's 1 point stays inside the 95% week budget");
    assert_eq!(refusal(&charged, "claude:d", Placed { workers: 4 }).map(|refused| refused.rule), None, "95% is the budget itself");
    let refused = refusal(&charged, "claude:d", Placed { workers: 5 }).expect("six workers' charges reach 96%");
    assert!(refused.rule == Rule::Burn && refused.line.contains("96%") && refused.line.contains("6.0 points for this worker"), "{refused}");

    let week_first = View { at: NOW, accounts: vec![account("claude:e", 75.0, 1.0, 40.0, 1.5)], burns: vec![hot_five_hour("claude:e")], ..View::default() };
    assert_eq!(refusal(&week_first, "claude:e", Placed::default()), None, "a week that resets within an hour after the five-hour window discards what that window would keep");
    let week_later = View { at: NOW, accounts: vec![account("claude:e", 75.0, 1.0, 40.0, 3.0)], burns: vec![hot_five_hour("claude:e")], ..View::default() };
    assert!(refusal(&week_later, "claude:e", Placed::default()).is_some(), "a later week resets nothing the window would spend");

    let before = [Sample { at: NOW - 15.0 * 60.0, used_pct: 30.0, resets_at: Some(NOW + HOUR) }, Sample { at: NOW, used_pct: 40.0, resets_at: Some(NOW + HOUR) }];
    let measured = crate::spend::tripwire::burn(&rules(), "claude:f", &before, &before, 2.0, NOW);
    assert_eq!((measured.five_hour * HOUR, measured.five_hour_per_week), (40.0, 2.0), "ten points in 15 minutes is 40 an hour");
}

fn hot_five_hour(name: &str) -> Burn {
    burn(name, 20.0, 0.0)
}
