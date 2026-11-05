use proptest::prelude::*;

use super::*;
use crate::usage::decide::{State, state};

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
    // KEEP(mutant): raising a window's used percent never opens an account (kills an exhaustion check that reads usage the wrong way)
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
}
