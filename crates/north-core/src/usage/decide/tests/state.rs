use super::*;
use crate::usage::decide::{FRESH_FOR, State, state};

const OPUS: Window = Window::Scoped(ModelFamily::Opus);

fn opus(fixture: &Fixture) -> State {
    state(&fixture.world(), &label("a"), ModelFamily::Opus)
}

fn old(row: i64, window: Window, used: f64) -> Reading {
    reading(row, "a", window, used, Some(NOW.0 + 9_000), NOW.0 - FRESH_FOR.0 - 1, Source::Web)
}

#[test]
fn row_1_an_account_signed_in_on_no_host_is_signed_out_whatever_its_readings() {
    let mut fixture = Fixture::signed_in("a").with(fresh(1, "a", Window::Week, 100.0, None));
    fixture.accounts[0].signed_in.clear();
    assert_eq!(opus(&fixture), State::SignedOut);
    let missing = Fixture::signed_in("b").with(fresh(1, "a", Window::Week, 10.0, None));
    assert_eq!(opus(&missing), State::SignedOut);
}

#[test]
fn row_2_a_limited_or_full_fresh_week_is_out_before_scope_five_hour_or_staleness() {
    let full = Fixture::signed_in("a")
        .with(fresh(1, "a", Window::Week, 100.0, Some(NOW.0 + 7_000)))
        .with(fresh(2, "a", OPUS, 100.0, Some(NOW.0 + 8_000)))
        .with(old(3, Window::FiveHour, 10.0));
    assert_eq!(opus(&full), State::WeekOut { resets_at: Some(Unix(NOW.0 + 7_000)) });
    let limited = Fixture::signed_in("a").with(old(1, Window::Week, 20.0)).limited(limit("a", Window::Week, Some(NOW.0 + 6_000), NOW.0 - 5)).limited(limit(
        "a",
        Window::FiveHour,
        Some(NOW.0 + 600),
        NOW.0 - 5,
    ));
    assert_eq!(opus(&limited), State::WeekOut { resets_at: Some(Unix(NOW.0 + 6_000)) });
}

#[test]
fn row_3_a_limited_or_full_scoped_week_for_the_family_is_out_before_five_hour_or_an_unread_week() {
    let full =
        Fixture::signed_in("a").with(fresh(1, "a", OPUS, 100.0, Some(NOW.0 + 8_000))).limited(limit("a", Window::FiveHour, Some(NOW.0 + 600), NOW.0 - 5));
    assert_eq!(opus(&full), State::ScopeOut { family: ModelFamily::Opus, resets_at: Some(Unix(NOW.0 + 8_000)) });
    let limited = Fixture::signed_in("a").with(old(1, Window::Week, 20.0)).limited(limit("a", OPUS, None, NOW.0 - 5));
    assert_eq!(opus(&limited), State::ScopeOut { family: ModelFamily::Opus, resets_at: None });
}

#[test]
fn row_4_a_limited_or_full_five_hour_window_pauses_before_an_unread_week() {
    let limited = Fixture::signed_in("a").limited(limit("a", Window::FiveHour, Some(NOW.0 + 600), NOW.0 - 5));
    assert_eq!(opus(&limited), State::Paused { resumes_at: Some(Unix(NOW.0 + 600)) });
    let full = Fixture::signed_in("a").with(old(1, Window::Week, 20.0)).with(fresh(2, "a", Window::FiveHour, 100.0, Some(NOW.0 + 900)));
    assert_eq!(opus(&full), State::Paused { resumes_at: Some(Unix(NOW.0 + 900)) });
}

#[test]
fn row_5_a_never_read_or_stale_week_is_unknown() {
    let never = Fixture::signed_in("a").with(fresh(1, "a", Window::FiveHour, 10.0, Some(NOW.0 + 900)));
    assert_eq!(opus(&never), State::Unknown { window: Window::Week, read_at: None });
    let stale = Fixture::signed_in("a").with(old(1, Window::Week, 20.0)).with(old(2, OPUS, 20.0));
    assert_eq!(opus(&stale), State::Unknown { window: Window::Week, read_at: Some(Unix(NOW.0 - FRESH_FOR.0 - 1)) });
}

#[test]
fn row_6_a_never_read_or_stale_five_hour_window_beside_a_fresh_week_is_open_and_not_started() {
    let expected =
        State::Open { five_hour: Percent(0.0), week: Percent(40.0), five_hour_resets: None, week_resets: Some(Unix(NOW.0 + 9_000)), not_started: true };
    let never = Fixture::signed_in("a").with(fresh(1, "a", Window::Week, 40.0, Some(NOW.0 + 9_000)));
    assert_eq!(opus(&never), expected);
    let stale = Fixture::signed_in("a").with(fresh(1, "a", Window::Week, 40.0, Some(NOW.0 + 9_000))).with(old(2, Window::FiveHour, 70.0));
    assert_eq!(opus(&stale), expected);
}

#[test]
fn row_7_fresh_windows_under_100_are_open_with_their_readings_and_a_stale_scope_never_closes() {
    let fixture = Fixture::signed_in("a")
        .with(fresh(1, "a", Window::Week, 40.0, Some(NOW.0 + 9_000)))
        .with(fresh(2, "a", Window::FiveHour, 99.5, Some(NOW.0 + 900)))
        .with(old(3, OPUS, 100.0));
    let expected = State::Open {
        five_hour: Percent(99.5),
        week: Percent(40.0),
        five_hour_resets: Some(Unix(NOW.0 + 900)),
        week_resets: Some(Unix(NOW.0 + 9_000)),
        not_started: false,
    };
    assert_eq!(opus(&fixture), expected);
}
