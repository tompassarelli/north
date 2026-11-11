use super::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3600.0;

fn rules() -> Rules {
    Rules { reserve: 5.0, floor_week: 5.0, floor_five_hour: 10.0, charge: 1.0, five_hour_burn_min: 15.0, week_burn_h: 6.0, reading_max_h: 2.0, urgent_limit: 2 }
}

fn account(name: &str, five_hour: f64, week: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + 4.0 * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + 100.0 * HOUR) }), counted: true }
}

/// 2026-11-11: normal launches queued behind the burn governor's projection while the provider had room; nothing short of the provider's own limit holds a worker.
#[test]
fn a_worker_starts_on_an_account_with_provider_room_whatever_its_use_projects_to() {
    let view = View { at: NOW, accounts: vec![account("claude:d", 99.0, 99.0), account("claude:e", 100.0, 10.0)], working: 40, ..View::default() };
    for workers in [0, 50] {
        assert_eq!(view.admit(&rules(), "claude:d", Starter::Worker, Placed { workers }), Ok(()));
    }
    assert_eq!(view.admit(&rules(), "claude:e", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit));
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

