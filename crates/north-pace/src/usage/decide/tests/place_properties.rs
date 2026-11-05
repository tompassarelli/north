use proptest::prelude::*;

use super::properties::{FAMILIES, NAMES, fixture};
use super::*;
use crate::usage::decide::{Decision, Gate, Tier, place};

fn request_strategy() -> impl Strategy<Value = Request> {
    (
        prop::sample::select(FAMILIES.to_vec()),
        prop::sample::select(vec![0.0, 5.0, 40.0, 120.0]),
        prop::sample::select(vec![Owner::Fleet, Owner::Tom]),
        prop::option::of(prop::sample::select(NAMES.to_vec())),
    )
        .prop_map(|(family, p50, owner, pin)| Request { family, owner, expected: expected(p50), pin: pin.map(label), ..request() })
}

fn setup() -> impl Strategy<Value = (Fixture, Request)> {
    (
        fixture(),
        request_strategy(),
        prop::sample::select(vec![None, Some(30.0), Some(80.0)]),
        prop::collection::vec((prop::sample::select(NAMES.to_vec()), 0.0..60.0f64), 0..3),
        prop::collection::vec((0.0..100.0f64, 0.0..100.0f64), 2),
    )
        .prop_map(|(mut fixture, request, reserve, running, opened)| {
            for (index, (week, five_hour)) in opened.into_iter().enumerate() {
                let name = NAMES[index];
                fixture.readings.push(fresh(1_000 + 2 * index as i64, name, Window::Week, week, Some(NOW.0 + 50_000)));
                fixture.readings.push(fresh(1_001 + 2 * index as i64, name, Window::FiveHour, five_hour, Some(NOW.0 + 9_000)));
            }
            if let Some(reserve) = reserve {
                fixture.settings.reserve.insert(Provider::Claude, Percent(reserve));
            }
            for (index, (name, p50)) in running.into_iter().enumerate() {
                fixture.sessions.push(session(&format!("s{index}"), name, p50, None));
            }
            (fixture, request)
        })
}

fn tier_of(decision: &Decision) -> Option<Tier> {
    match decision {
        Decision::Place { tier, .. } => Some(*tier),
        Decision::Refuse(_) => None,
    }
}

proptest! {
    // KEEP(mutant): a placement under a reserve passes the reserve gate (kills a placer that skips the reserve check)
    #[test]
    fn prop_reserve_a_placement_under_a_reserve_leaves_at_least_the_reserve_free((fixture, request) in setup()) {
        let world = fixture.world();
        if let Decision::Place { account, gate, .. } = place(&world, &request) {
            let applies = request.owner != Owner::Tom && fixture.settings.reserve.get(&Provider::Claude).is_some_and(|percent| percent.0 > 0.0);
            if applies {
                let passed = matches!(gate, Gate::Pass { .. });
                prop_assert!(passed, "{account:?}");
            }
        }
    }

    // KEEP(mutant): an account with extra usage enabled is never placed where it may pause into paid spend (money guard)
    #[test]
    fn prop_extra_usage_an_enabled_account_is_never_placed_in_tier_2_or_3((mut fixture, request) in setup()) {
        fixture.accounts.iter_mut().for_each(|account| account.extra_usage_enabled = Some(true));
        let decision = place(&fixture.world(), &request);
        let allowed = matches!(tier_of(&decision), None | Some(Tier::One));
        prop_assert!(allowed);
    }
}
