use proptest::prelude::*;

use super::*;
use crate::usage::decide::{FRESH_FOR, State, View, state, view};

pub(super) const NAMES: [&str; 2] = ["a", "b"];
pub(super) const FAMILIES: [ModelFamily; 2] = [ModelFamily::Opus, ModelFamily::Sonnet];
pub(super) const WINDOWS: [Window; 4] = [Window::FiveHour, Window::Week, Window::Scoped(ModelFamily::Opus), Window::Scoped(ModelFamily::Sonnet)];

pub(super) fn window() -> impl Strategy<Value = Window> {
    prop::sample::select(WINDOWS.to_vec())
}

pub(super) fn source() -> impl Strategy<Value = Source> {
    prop::sample::select(vec![Source::Stream, Source::Header, Source::Cli, Source::Web])
}

pub(super) fn resets_at() -> impl Strategy<Value = Option<i64>> {
    prop::option::of(NOW.0 - 1_000..NOW.0 + 20_000)
}

pub(super) fn readings() -> impl Strategy<Value = Vec<Reading>> {
    let one = (
        prop::sample::select(NAMES.to_vec()),
        window(),
        prop::sample::select(vec![0.0, 40.0, 99.0, 100.0, 104.0]),
        resets_at(),
        NOW.0 - 2_000..NOW.0 + 100,
        source(),
    );
    prop::collection::vec(one, 0..12).prop_map(|rows| {
        rows.into_iter()
            .enumerate()
            .map(|(row, (name, window, used, resets_at, read_at, source))| reading(row as i64, name, window, used, resets_at, read_at, source))
            .collect()
    })
}

pub(super) fn limits() -> impl Strategy<Value = Vec<LimitEvent>> {
    let one = (prop::sample::select(NAMES.to_vec()), window(), resets_at(), NOW.0 - 2_000..NOW.0 + 100);
    prop::collection::vec(one.prop_map(|(name, window, resets_at, at)| limit(name, window, resets_at, at)), 0..3)
}

pub(super) fn fixture() -> impl Strategy<Value = Fixture> {
    (readings(), limits(), any::<bool>()).prop_map(|(readings, limits, b_signed_in)| Fixture {
        accounts: vec![account("a", &["gex45"]), account("b", if b_signed_in { &["laptop"] } else { &[] })],
        readings,
        limits,
        ..Fixture::default()
    })
}

fn answers(fixture: &Fixture) -> Vec<(Vec<View>, Vec<State>)> {
    let world = fixture.world();
    NAMES
        .iter()
        .map(|name| {
            let views = WINDOWS.iter().map(|window| view(&world, &label(name), *window)).collect();
            let states = FAMILIES.iter().map(|family| state(&world, &label(name), *family)).collect();
            (views, states)
        })
        .collect()
}

fn badness(state: State) -> u8 {
    match state {
        State::Open { .. } => 0,
        State::Unknown { .. } => 1,
        State::SignedOut | State::WeekOut { .. } | State::ScopeOut { .. } | State::Paused { .. } => 2,
    }
}

fn badness_of(fixture: &Fixture) -> Vec<u8> {
    let world = fixture.world();
    NAMES.iter().flat_map(|name| FAMILIES.iter().map(move |family| badness(state(&world, &label(name), *family)))).collect()
}

fn never_better(before: &Fixture, after: &Fixture) -> Result<(), TestCaseError> {
    for (old, new) in badness_of(before).into_iter().zip(badness_of(after)) {
        prop_assert!(new >= old, "moved from badness {old} to {new}");
    }
    Ok(())
}

proptest! {
    #[test]
    fn prop_determinism_any_permutation_of_the_slices_gives_the_same_views_and_states(
        (fixture, shuffled) in fixture().prop_flat_map(|fixture| {
            let parts = (
                Just(fixture.accounts.clone()).prop_shuffle(),
                Just(fixture.readings.clone()).prop_shuffle(),
                Just(fixture.limits.clone()).prop_shuffle(),
            );
            (Just(fixture), parts.prop_map(|(accounts, readings, limits)| Fixture { accounts, readings, limits, ..Fixture::default() }))
        }),
    ) {
        prop_assert_eq!(answers(&fixture), answers(&shuffled));
    }

    #[test]
    fn prop_override_a_limit_resetting_after_now_closes_its_window_whatever_the_readings(
        mut fixture in fixture(),
        window in window(),
        ahead in 1..20_000i64,
        at in NOW.0 - 2_000..NOW.0 + 100,
    ) {
        fixture.limits.push(limit("a", window, Some(NOW.0 + ahead), at));
        let world = fixture.world();
        prop_assert!(matches!(view(&world, &label("a"), window), View::Limited { resets_at: Some(_) }), "unexpected answer");
        let closed = FAMILIES.iter().map(|family| state(&world, &label("a"), *family));
        for (family, state) in FAMILIES.iter().zip(closed) {
            let applies = !matches!(window, Window::Scoped(scope) if scope != *family);
            prop_assert!(!applies || badness(state) == 2, "{window:?} limited but {family:?} is {state:?}");
        }
    }

    #[test]
    fn prop_exhaustion_first_a_fresh_week_at_100_is_week_out_whatever_the_five_hour_view(
        mut fixture in fixture(),
        resets in prop::option::of(NOW.0 + 1..NOW.0 + 20_000),
    ) {
        fixture.readings.push(reading(99, "a", Window::Week, 100.0, resets, NOW.0 + 200, Source::Header));
        let world = fixture.world();
        for family in FAMILIES {
            prop_assert!(matches!(state(&world, &label("a"), family), State::WeekOut { .. }), "unexpected answer");
        }
    }

    #[test]
    fn prop_stale_only_a_recent_unreset_reading_is_fresh_and_only_a_fresh_week_is_open(fixture in fixture()) {
        let world = fixture.world();
        for name in NAMES {
            for window in WINDOWS {
                if let View::Fresh { read_at, resets_at, .. } = view(&world, &label(name), window) {
                    prop_assert!((NOW.0 - read_at.0).abs() <= FRESH_FOR.0);
                    prop_assert!(resets_at.is_none_or(|resets_at| resets_at > NOW));
                }
            }
            let week = view(&world, &label(name), Window::Week);
            for family in FAMILIES {
                if let State::Open { .. } = state(&world, &label(name), family) {
                    prop_assert!(matches!(week, View::Fresh { .. }), "unexpected answer");
                }
            }
        }
    }

    #[test]
    fn prop_scope_one_familys_scoped_window_never_changes_another_familys_state(
        fixture in fixture(),
        extra in readings(),
        extra_limits in limits(),
    ) {
        let opus_only = |window: &Window| *window == Window::Scoped(ModelFamily::Opus);
        let mut changed = fixture.clone();
        changed.readings.retain(|reading| !opus_only(&reading.window));
        changed.limits.retain(|limit| !opus_only(&limit.window));
        changed.readings.extend(extra.into_iter().filter(|reading| opus_only(&reading.window)).map(|reading| Reading { row: RowId(reading.row.0 + 100), ..reading }));
        changed.limits.extend(extra_limits.into_iter().filter(|limit| opus_only(&limit.window)));
        for name in NAMES {
            prop_assert_eq!(state(&fixture.world(), &label(name), ModelFamily::Sonnet), state(&changed.world(), &label(name), ModelFamily::Sonnet));
        }
    }

    #[test]
    fn prop_monotonic_raising_used_of_a_window_never_moves_an_account_to_a_better_state(
        fixture in fixture(),
        window in window(),
        raise in 0.0..60.0f64,
    ) {
        let mut raised = fixture.clone();
        for reading in raised.readings.iter_mut().filter(|reading| reading.window == window) {
            reading.used = Percent(reading.used.0 + raise);
        }
        never_better(&fixture, &raised)?;
    }

    #[test]
    fn prop_monotonic_adding_a_limit_event_never_moves_an_account_to_a_better_state(
        fixture in fixture(),
        added in limits(),
    ) {
        let mut limited = fixture.clone();
        limited.limits.extend(added);
        never_better(&fixture, &limited)?;
    }

    #[test]
    fn prop_monotonic_aging_a_window_not_shown_full_never_moves_an_account_to_a_better_state(
        fixture in fixture(),
        name in prop::sample::select(NAMES.to_vec()),
        window in window(),
        age in 0..3_000i64,
    ) {
        let shown_full = matches!(view(&fixture.world(), &label(name), window), View::Fresh { used, .. } if used.0 >= 100.0);
        prop_assume!(!shown_full);
        let mut aged = fixture.clone();
        for reading in aged.readings.iter_mut().filter(|reading| reading.window == window && reading.account == label(name)) {
            reading.read_at = Unix(reading.read_at.0 - age);
        }
        never_better(&fixture, &aged)?;
    }
}
