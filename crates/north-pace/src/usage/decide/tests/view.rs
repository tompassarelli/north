use super::*;
use crate::usage::decide::{FRESH_FOR, StaleWhy, View, view};

fn week(fixture: &Fixture) -> View {
    view(&fixture.world(), &label("a"), Window::Week)
}

#[test]
fn row_1_a_limit_event_resetting_after_now_limits_the_window_over_a_fresh_reading() {
    let fixture = Fixture::signed_in("a")
        .with(fresh(1, "a", Window::Week, 10.0, Some(NOW.0 + 9_000)))
        .limited(limit("a", Window::Week, Some(NOW.0 + 3_000), NOW.0 - 4_000))
        .limited(limit("a", Window::Week, Some(NOW.0 + 5_000), NOW.0 - 5_000))
        .limited(limit("a", Window::Week, Some(NOW.0 - 1), NOW.0 - 2));
    assert_eq!(week(&fixture), View::Limited { resets_at: Some(Unix(NOW.0 + 5_000)) });
}

#[test]
fn row_2_a_limit_event_without_reset_newer_than_every_reading_limits_the_window() {
    let newer = Fixture::signed_in("a").with(fresh(1, "a", Window::Week, 10.0, Some(NOW.0 + 9_000))).limited(limit("a", Window::Week, None, NOW.0 - 30));
    assert_eq!(week(&newer), View::Limited { resets_at: None });
    let older = Fixture::signed_in("a").with(fresh(1, "a", Window::Week, 10.0, Some(NOW.0 + 9_000))).limited(limit("a", Window::Week, None, NOW.0 - 60));
    assert!(matches!(week(&older), View::Fresh { .. }));
}

#[test]
fn row_3_a_window_with_no_reading_is_never_read() {
    let fixture = Fixture::signed_in("a")
        .with(fresh(1, "a", Window::FiveHour, 10.0, Some(NOW.0 + 9_000)))
        .with(fresh(2, "b", Window::Week, 10.0, Some(NOW.0 + 9_000)))
        .limited(limit("a", Window::Week, Some(NOW.0 - 10), NOW.0 - 100));
    assert_eq!(week(&fixture), View::NeverRead);
}

#[test]
fn row_4_a_newest_reading_whose_reset_has_passed_is_stale() {
    let fixture = Fixture::signed_in("a").with(fresh(1, "a", Window::Week, 10.0, Some(NOW.0))).with(reading(
        2,
        "a",
        Window::Week,
        10.0,
        Some(NOW.0 + 9_000),
        NOW.0 - 100,
        Source::Header,
    ));
    assert_eq!(week(&fixture), View::Stale { read_at: Unix(NOW.0 - 60), why: StaleWhy::ResetSinceRead });
}

#[test]
fn row_5_a_newest_reading_older_than_fresh_for_either_way_is_stale() {
    for read_at in [NOW.0 - FRESH_FOR.0 - 1, NOW.0 + FRESH_FOR.0 + 1] {
        let fixture = Fixture::signed_in("a").with(reading(1, "a", Window::Week, 10.0, None, read_at, Source::Cli));
        assert_eq!(week(&fixture), View::Stale { read_at: Unix(read_at), why: StaleWhy::Old });
    }
    let edge = Fixture::signed_in("a").with(reading(1, "a", Window::Week, 10.0, None, NOW.0 - FRESH_FOR.0, Source::Cli));
    assert!(matches!(week(&edge), View::Fresh { .. }));
}

#[test]
fn row_6_the_newest_reading_by_time_then_used_then_reset_then_source_then_row_is_fresh() {
    let fixture = Fixture::signed_in("a")
        .with(reading(1, "a", Window::Week, 90.0, Some(NOW.0 + 9_000), NOW.0 - 70, Source::Stream))
        .with(reading(2, "a", Window::Week, 20.0, Some(NOW.0 + 9_000), NOW.0 - 10, Source::Web))
        .with(reading(3, "a", Window::Week, 30.0, Some(NOW.0 + 8_000), NOW.0 - 10, Source::Web))
        .with(reading(4, "a", Window::Week, 30.0, Some(NOW.0 + 9_000), NOW.0 - 10, Source::Web))
        .with(reading(5, "a", Window::Week, 30.0, Some(NOW.0 + 9_000), NOW.0 - 10, Source::Cli))
        .with(reading(6, "a", Window::Week, 30.0, Some(NOW.0 + 9_000), NOW.0 - 10, Source::Cli));
    let expected = View::Fresh { used: Percent(30.0), resets_at: Some(Unix(NOW.0 + 9_000)), read_at: Unix(NOW.0 - 10), source: Source::Cli };
    assert_eq!(week(&fixture), expected);
}
