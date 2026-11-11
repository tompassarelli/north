use super::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3600.0;

fn rules() -> Rules {
    Rules { reserve: 5.0, floor_week: 5.0, floor_five_hour: 10.0, charge: 1.0, five_hour_burn_min: 15.0, week_burn_h: 6.0, reading_max_h: 2.0, urgent_limit: 2 }
}

fn account(name: &str, five_hour: f64, week: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + 4.0 * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + 100.0 * HOUR) }), counted: true }
}

/// Tom revised rule 1 on 2026-11-07: one reserve pooled across accounts, so one account may be drained
/// to its provider limit while workers keep starting on the others until the pool reaches the reserve.
#[test]
fn the_pooled_reserve_stops_workers_only_when_the_pool_reaches_it_while_one_account_is_drained() {
    let mut view = View { at: NOW, accounts: vec![account("claude:a", 100.0, 100.0), account("claude:b", 0.0, 80.0)], working: 0, burns: Vec::new(), keyed: Vec::new(), operational: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
    assert_eq!(view.admit(&rules(), "claude:b", Starter::Worker, Placed::default()), Ok(()));
    view.accounts[1] = account("claude:b", 0.0, 90.0);
    let refused = view.admit(&rules(), "claude:b", Starter::Worker, Placed::default()).unwrap_err();
    assert_eq!(refused.rule, Rule::Reserve, "{refused}");
    assert_eq!(view.admit(&rules(), "claude:b", Starter::Reserved, Placed::default()), Ok(()), "the proxy, routines and Tom may use the reserve");
}

/// The 2026-11-07 failure: an account partway through its week with a fresh five-hour window was
/// allowed no workers while ready boxes waited; no count stops workers, only the pool does.
#[test]
fn twelve_concurrent_workers_start_on_an_account_with_room_and_only_the_pool_stops_the_next() {
    let view = View { at: NOW, accounts: vec![account("claude:c", 0.0, 74.0)], working: 3, burns: Vec::new(), keyed: Vec::new(), operational: Vec::new(), drain: false };
    for workers in 0..20 {
        assert_eq!(view.admit(&rules(), "claude:c", Starter::Worker, Placed { workers }), Ok(()), "worker {}", workers + 4);
    }
    let refused = view.admit(&rules(), "claude:c", Starter::Worker, Placed { workers: 20 }).unwrap_err();
    assert_eq!(refused.rule, Rule::Reserve, "{refused}");
}

/// north#1000 let a worker start with 3% of the week left; north#1049 reserves the last 5% of the
/// week and 10% of the five-hour window for the privacy judge, lander and canaries, so a worker
/// stops there while a reserved starter still uses it.
#[test]
fn a_worker_stops_at_the_operational_floor_while_the_judge_lander_and_canaries_may_use_it() {
    let mut view = View { at: NOW, accounts: vec![account("claude:a", 0.0, 93.9), account("claude:b", 10.0, 50.0)], working: 0, burns: Vec::new(), keyed: Vec::new(), operational: Vec::new(), drain: false };
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()), Ok(()), "6.1% of the week left covers this worker's 1-point charge and stays above the 5% floor");
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

/// Tom's drain word (2026-11-09) turns the pooled reserve off; the provider limit and the
/// per-account operational floor stay.
#[test]
fn drain_skips_the_reserve_but_not_the_provider_limit_or_the_floor() {
    let tight = Rules { charge: 6.0, ..rules() };
    let view = View { at: NOW, accounts: vec![account("claude:a", 10.0, 94.0), account("claude:b", 10.0, 100.0)], working: 0, burns: Vec::new(), keyed: Vec::new(), operational: Vec::new(), drain: false };
    assert_eq!(view.admit(&tight, "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Reserve));
    let drained = View { drain: true, ..view.clone() };
    assert_eq!(drained.admit(&tight, "claude:a", Starter::Worker, Placed::default()), Ok(()));
    assert_eq!(drained.admit(&tight, "claude:b", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
    let floor = View { drain: true, accounts: vec![account("claude:a", 10.0, 96.0)], ..view };
    assert_eq!(floor.admit(&tight, "claude:a", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Floor));
}

/// north#1049 reserves capacity for the privacy judge, lander and canaries: an account the policy declares
/// the operational reserve starts no worker or subagent, and its week stays out of the pool.
#[test]
fn a_worker_launch_refuses_the_declared_operational_reserve_while_operational_callers_use_it() {
    let view = View { at: NOW, accounts: vec![account("claude:a", 0.0, 10.0), account("claude:e", 0.0, 0.0)], operational: vec!["claude:e".into()], ..View::default() };
    for starter in [Starter::Worker, Starter::Subagent] {
        let refused = view.admit(&rules(), "claude:e", starter, Placed::default()).unwrap_err();
        assert_eq!(refused.rule, Rule::Operational, "{refused}");
    }
    assert_eq!(view.admit(&rules(), "claude:e", Starter::Reserved, Placed::default()), Ok(()));
    assert_eq!(view.admit(&rules(), "claude:a", Starter::Worker, Placed::default()), Ok(()));
    assert_eq!(view.pool(), 90.0, "the reserve's week is not the workers' pool");
}

/// 2026-11-11 6:00 to 6:35 AM +07: spine fixes queued urgent on an account over its week projection, behind a governor meant to pace ordinary work.
#[test]
fn an_urgent_launch_passes_a_holding_governor_up_to_its_limit() {
    let view = View { at: NOW, accounts: vec![account("claude:d", 0.0, 94.0), account("claude:e", 0.0, 0.0)], ..View::default() };
    let held = |starter| view.admit(&rules(), "claude:d", starter, Placed { workers: 1 }).map_err(|refusal| refusal.rule);
    assert_eq!(held(Starter::Worker), Err(Rule::Burn), "the governor holds a normal launch");
    assert_eq!(held(Starter::Urgent { running: 0 }), Ok(()), "the first urgent launch passes it");
    assert_eq!(held(Starter::Urgent { running: 1 }), Ok(()), "the second passes it");
    assert_eq!(held(Starter::Urgent { running: 2 }), Err(Rule::Burn), "a third queues like any other launch");
    let floor = View { accounts: vec![account("claude:d", 0.0, 96.0), account("claude:e", 0.0, 0.0)], ..view };
    assert_eq!(floor.admit(&rules(), "claude:d", Starter::Urgent { running: 0 }, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Floor), "the operational floor still holds an urgent launch");
}
