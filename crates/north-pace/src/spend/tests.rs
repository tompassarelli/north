use super::tripwire::{self, Burst, Sample, Spender};
use super::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3600.0;

fn rules() -> Rules {
    Rules { reserve: 5.0, floor_week: 5.0, floor_five_hour: 10.0, charge: 1.0, cap: 12, fleet_trip: Burst { points: 30.0, minutes: 15.0 }, session_trip: Burst { points: 10.0, minutes: 10.0 }, reading_max_h: 2.0 }
}

fn account(name: &str, five_hour: f64, week: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + 4.0 * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + 100.0 * HOUR) }), counted: true }
}

/// Tom revised rule 1 on 2026-11-07: one reserve pooled across accounts, so one account may be drained
/// to its provider limit while workers keep starting on the others until the pool reaches the reserve.
#[test]
fn the_pooled_reserve_stops_workers_only_when_the_pool_reaches_it_while_one_account_is_drained() {
    let mut view = View { at: NOW, accounts: vec![account("claude:a", 100.0, 100.0), account("claude:b", 0.0, 80.0)], working: 0, trips: Vec::new(), keyed: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
    assert_eq!(view.admit(&rules(), "claude:b", Starter::Worker, Placed::default()), Ok(()));
    view.accounts[1] = account("claude:b", 0.0, 90.0);
    let refused = view.admit(&rules(), "claude:b", Starter::Worker, Placed::default()).unwrap_err();
    assert_eq!(refused.rule, Rule::Reserve, "{refused}");
    assert_eq!(view.admit(&rules(), "claude:b", Starter::Reserved, Placed::default()), Ok(()), "the proxy, routines and Tom may use the reserve");
}

/// The 2026-11-07 failure: an account partway through its week with a fresh five-hour window was
/// allowed no workers while ready boxes waited; it must start workers until the fleet cap alone stops it.
#[test]
fn an_account_at_74_percent_of_its_week_with_a_fresh_five_hour_window_starts_workers_up_to_the_cap() {
    let view = View { at: NOW, accounts: vec![account("claude:c", 0.0, 74.0)], working: 3, trips: Vec::new(), keyed: Vec::new(), drain: false };
    for workers in 0..9 {
        assert_eq!(view.admit(&rules(), "claude:c", Starter::Worker, Placed { workers }), Ok(()), "worker {}", workers + 4);
    }
    let refused = view.admit(&rules(), "claude:c", Starter::Worker, Placed { workers: 9 }).unwrap_err();
    assert_eq!(refused.rule, Rule::Cap, "{refused}");
}

#[test]
fn the_tripwire_fires_on_a_planted_burst_and_names_the_top_spender() {
    let samples = [Sample { at: NOW - 14.0 * 60.0, used_pct: 20.0, resets_at: Some(NOW + HOUR) }, Sample { at: NOW, used_pct: 55.0, resets_at: Some(NOW + HOUR) }];
    let spenders = [Spender { session: "runaway".into(), account: "claude:c".into(), spent: vec![(NOW - 14.0 * 60.0, 1.0), (NOW, 31.0)] }];
    let trip = tripwire::trip(&rules(), "claude:c", &samples, &spenders, 1.0, NOW).expect("a 35-point rise in 14 minutes trips");
    assert!(trip.line.contains("runaway"), "{}", trip.line);
    let view = View { at: NOW, accounts: vec![account("claude:c", 55.0, 50.0)], working: 0, trips: vec![trip], keyed: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:c", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Tripwire));
    let calm = [Sample { at: NOW - 14.0 * 60.0, used_pct: 20.0, resets_at: Some(NOW + HOUR) }, Sample { at: NOW, used_pct: 45.0, resets_at: Some(NOW + HOUR) }];
    assert_eq!(tripwire::trip(&rules(), "claude:c", &calm, &[], 1.0, NOW), None);
}

/// north#1000 let a worker start with 3% of the week left; north#1049 reserves the last 5% of the
/// week and 10% of the five-hour window for the privacy judge, lander and canaries, so a worker
/// stops there while a reserved starter still uses it.
#[test]
fn a_worker_stops_at_the_operational_floor_while_the_judge_lander_and_canaries_may_use_it() {
    let mut view = View { at: NOW, accounts: vec![account("claude:a", 0.0, 94.9), account("claude:b", 10.0, 50.0)], working: 0, trips: Vec::new(), keyed: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()), Ok(()), "5.1% of the week is still above the 5% floor");
    view.accounts[0] = account("claude:a", 0.0, 95.0);
    let week = view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()).unwrap_err();
    assert_eq!(week.rule, Rule::Floor, "{week}");
    assert!(week.line.contains("week 5% left, inside the 5% floor") && week.line.contains("privacy judge, lander and canaries"), "{week}");
    view.accounts[0] = account("claude:a", 90.0, 50.0);
    let five_hour = view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()).unwrap_err();
    assert_eq!(five_hour.rule, Rule::Floor, "{five_hour}");
    assert!(five_hour.line.contains("five-hour 10% left, inside the 10% floor"), "{five_hour}");
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Reserved, Placed::default()), Ok(()), "the judge, lander and canaries keep the floor");
}

/// Tom's drain word (2026-11-09) turns the pooled reserve off; the provider limit, the cap and the
/// per-account operational floor stay.
#[test]
fn drain_skips_the_reserve_but_not_the_provider_limit_the_cap_or_the_floor() {
    let tight = Rules { charge: 6.0, ..rules() };
    let view = View { at: NOW, accounts: vec![account("claude:a", 10.0, 94.0), account("claude:b", 10.0, 100.0)], working: 0, trips: Vec::new(), keyed: Vec::new(), drain: false };
    assert_eq!(view.admit(&tight, "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Reserve));
    let drained = View { drain: true, ..view.clone() };
    assert_eq!(drained.admit(&tight, "claude:a", Starter::Worker, Placed::default()), Ok(()));
    assert_eq!(drained.admit(&tight, "claude:b", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
    assert_eq!(drained.admit(&tight, "claude:a", Starter::Worker, Placed { workers: 12 }).map_err(|refusal| refusal.rule), Err(Rule::Cap));
    let floor = View { drain: true, accounts: vec![account("claude:a", 10.0, 96.0)], ..view };
    assert_eq!(floor.admit(&tight, "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Floor));
}

/// The 2026-11-08 failure: a session trip on b priced before the 2:05 AM recalibration held every b launch until 2:22 AM.
#[test]
fn a_session_trip_priced_before_a_recalibration_is_lifted_at_the_new_price_and_prints_its_price() {
    let spenders = [Spender { session: "north-bars-lead".into(), account: "claude:b".into(), spent: vec![(NOW - 10.0 * 60.0, 10.0), (NOW, 13.4)] }];
    let stale = tripwire::trip(&rules(), "claude:b", &[], &spenders, 3.529, NOW).expect("12 points at the stale rate trips");
    assert!(stale.line.contains("at 3.529 points per dollar"), "{}", stale.line);
    let later = NOW + 120.0;
    assert_eq!(tripwire::trip(&rules(), "claude:b", &[], &spenders, 0.465, later), None);
    let held = tripwire::judge(std::slice::from_ref(&stale), Vec::new(), later, &rules(), |_| 3.529);
    assert_eq!(held.current, vec![stale.clone()], "an unconfirmed trip at the same price holds its window");
    let aged = tripwire::judge(std::slice::from_ref(&stale), Vec::new(), NOW + 11.0 * 60.0, &rules(), |_| 3.529);
    assert!(aged.current.is_empty(), "on 2026-11-08 n882-text's session trip held every b start from 9:17 to 9:32 AM, the fleet's 15 min, after its own 10 min burst: {:?}", aged.current);
    let lifted = tripwire::judge(&[stale], Vec::new(), later, &rules(), |_| 0.465);
    assert!(lifted.current.is_empty(), "{:?}", lifted.current);
}
