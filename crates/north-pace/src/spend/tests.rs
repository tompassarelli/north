use super::{Account, Rule, Starter, View, Window};

const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3600.0;

fn account(name: &str, five_hour: f64, week: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + 4.0 * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + 100.0 * HOUR) }), counted: true }
}

/// 2026-11-11: normal launches queued behind the burn governor's projection while the provider had room; nothing short of the provider's own limit holds a worker.
#[test]
fn a_worker_starts_on_an_account_with_provider_room_whatever_its_use_projects_to() {
    let view = View { at: NOW, accounts: vec![account("claude:d", 99.0, 99.0), account("claude:e", 100.0, 10.0)], working: 40, ..View::default() };
    assert_eq!(view.admit("claude:d", Starter::Worker), Ok(()));
    assert_eq!(view.admit("claude:e", Starter::Worker).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
}

/// north#1049 reserves capacity for the privacy judge, lander and canaries: an account the policy declares
/// the operational reserve starts no worker or subagent, and its week stays out of the pool.
#[test]
fn a_worker_launch_refuses_the_declared_operational_reserve_while_operational_callers_use_it() {
    let view = View { at: NOW, accounts: vec![account("claude:a", 0.0, 10.0), account("claude:e", 0.0, 0.0)], operational: vec!["claude:e".into()], ..View::default() };
    let refused = view.admit("claude:e", Starter::Worker).unwrap_err();
    assert_eq!(refused.rule, Rule::Operational, "{refused}");
    assert_eq!(view.admit("claude:e", Starter::Reserved), Ok(()));
    assert_eq!(view.admit("claude:a", Starter::Worker), Ok(()));
    assert_eq!(view.pool(), 90.0, "the reserve's week is not the workers' pool");
}
