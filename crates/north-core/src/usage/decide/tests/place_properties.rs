use proptest::prelude::*;

use super::properties::{FAMILIES, NAMES, fixture};
use super::*;
use crate::usage::decide::{Decision, Gate, Tier, Verdict, place, rank};

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

fn badness(decision: &Decision) -> u8 {
    match tier_of(decision) {
        Some(Tier::One) => 0,
        Some(Tier::Two) => 1,
        Some(Tier::Three) => 2,
        None => 3,
    }
}

proptest! {
    #[test]
    fn prop_determinism_any_permutation_of_the_slices_gives_the_same_decision((fixture, request) in setup()) {
        let shuffled = Fixture {
            accounts: fixture.accounts.iter().rev().cloned().collect(),
            readings: fixture.readings.iter().rev().cloned().collect(),
            limits: fixture.limits.iter().rev().cloned().collect(),
            sessions: fixture.sessions.iter().rev().cloned().collect(),
            ..fixture.clone()
        };
        prop_assert_eq!(place(&fixture.world(), &request), place(&shuffled.world(), &request));
    }

    #[test]
    fn prop_soundness_a_placement_is_in_a_tier_for_the_family_and_tier_3_needs_no_reserve((fixture, request) in setup()) {
        let world = fixture.world();
        let reserve = fixture.settings.reserve.get(&Provider::Claude).map_or(0.0, |percent| percent.0);
        if let Decision::Place { account, tier, .. } = place(&world, &request) {
            let ranked = rank(&world, &request);
            let ranked_tier = ranked.entry(&account).map(|entry| entry.verdict);
            let consistent = ranked_tier.is_some_and(|verdict| matches!(verdict, Verdict::Placeable { tier: found, .. } if found == tier));
            prop_assert!(consistent);
            prop_assert!(tier != Tier::Three || reserve == 0.0 || request.owner == Owner::Tom);
            prop_assert!(account_of(&fixture, &account).extra_usage_enabled != Some(true) || tier == Tier::One);
        }
    }

    #[test]
    fn prop_reserve_with_none_set_place_refuses_only_when_no_account_is_placeable((fixture, request) in setup()) {
        let mut fixture = fixture;
        fixture.settings.reserve.clear();
        let request = Request { pin: None, workstream: None, ..request };
        let world = fixture.world();
        let any_placeable = rank(&world, &request).placeable().next().is_some();
        prop_assert_eq!(any_placeable, tier_of(&place(&world, &request)).is_some());
    }

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

    #[test]
    fn prop_warn_never_hold_a_tier_1_or_2_account_and_an_untripped_workstream_always_place((fixture, request) in setup()) {
        let request = Request { pin: None, ..request };
        let world = fixture.world();
        let some_tier_1_or_2 = rank(&world, &request).placeable().any(|entry| matches!(entry.verdict, Verdict::Placeable { tier: Tier::One | Tier::Two, .. }));
        prop_assert!(!some_tier_1_or_2 || tier_of(&place(&world, &request)).is_some());
    }

    #[test]
    fn prop_extra_usage_an_enabled_account_is_never_placed_in_tier_2_or_3((mut fixture, request) in setup()) {
        fixture.accounts.iter_mut().for_each(|account| account.extra_usage_enabled = Some(true));
        let decision = place(&fixture.world(), &request);
        let allowed = matches!(tier_of(&decision), None | Some(Tier::One));
        prop_assert!(allowed);
    }

    #[test]
    fn prop_monotonic_raising_the_reserve_or_adding_in_flight_spend_never_improves_the_decision(
        (fixture, request) in setup(),
        raise in 0.0..60.0f64,
        extra in 0.0..80.0f64,
    ) {
        let before = badness(&place(&fixture.world(), &request));
        let mut raised = fixture.clone();
        let current = raised.settings.reserve.get(&Provider::Claude).map_or(0.0, |percent| percent.0);
        raised.settings.reserve.insert(Provider::Claude, Percent((current + raise).min(100.0)));
        prop_assert!(badness(&place(&raised.world(), &request)) >= before, "reserve");
        let mut busier = fixture;
        busier.sessions.push(session("late", "a", extra, None));
        prop_assert!(badness(&place(&busier.world(), &request)) >= before, "in flight");
    }
}

fn account_of<'a>(fixture: &'a Fixture, name: &Label) -> &'a Account {
    fixture.accounts.iter().find(|account| &account.label == name).expect("account")
}
