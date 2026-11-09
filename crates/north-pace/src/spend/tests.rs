use super::tripwire::{self, Burst, Sample, Spender};
use super::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3600.0;

fn rules() -> Rules {
    Rules { reserve: 5.0, charge: 1.0, cap: 12, fleet_trip: Burst { points: 30.0, minutes: 15.0 }, session_trip: Burst { points: 10.0, minutes: 10.0 }, reading_max_h: 2.0 }
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

/// 2026-11-09: workers were refused on b and c at 3% and 1% of their week by a per-account cover while the pool sat above its reserve.
#[test]
fn a_worker_starts_on_an_account_with_3_percent_of_its_week_left_while_the_pool_is_above_the_reserve_and_stops_at_it() {
    let mut view = View { at: NOW, accounts: vec![account("claude:a", 10.0, 97.0), account("claude:b", 10.0, 50.0)], working: 0, trips: Vec::new(), keyed: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()), Ok(()));
    view.accounts[1] = account("claude:b", 10.0, 93.0);
    let refused = view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()).unwrap_err();
    assert_eq!(refused.rule, Rule::Reserve, "{refused}");
    assert!(refused.line.contains("worker session") && !refused.to_string().contains("turn"), "{refused}");
}

/// Tom's drain word (2026-11-09) turns the reserve off; the provider limit and the cap stay.
#[test]
fn drain_skips_the_reserve_but_not_the_provider_limit_or_the_cap() {
    let view = View { at: NOW, accounts: vec![account("claude:a", 10.0, 99.0), account("claude:b", 10.0, 100.0)], working: 0, trips: Vec::new(), keyed: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Reserve));
    let drained = View { drain: true, ..view };
    assert_eq!(drained.admit(&rules(), "claude:a", Starter::Worker, Placed::default()), Ok(()));
    assert_eq!(drained.admit(&rules(), "claude:b", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
    assert_eq!(drained.admit(&rules(), "claude:a", Starter::Worker, Placed { workers: 12 }).map_err(|refusal| refusal.rule), Err(Rule::Cap));
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
