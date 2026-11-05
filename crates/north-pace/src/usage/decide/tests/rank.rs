use super::*;
use crate::usage::decide::{Block, State, Tier, Verdict, Warning, rank};

const WEEK_RESET: i64 = NOW.0 + 100_000;
const FIVE_RESET: i64 = NOW.0 + 9_000;

fn order(fixture: &Fixture, request: &Request) -> Vec<(String, Option<Tier>)> {
    rank(&fixture.world(), request)
        .entries
        .into_iter()
        .map(|entry| {
            let tier = match entry.verdict {
                Verdict::Placeable { tier, .. } => Some(tier),
                Verdict::Blocked(_) => None,
            };
            (entry.account.0, tier)
        })
        .collect()
}

fn verdict_of(fixture: &Fixture, request: &Request, name: &str) -> Verdict {
    rank(&fixture.world(), request).entry(&label(name)).expect("entry").verdict
}

#[test]
fn tier_1_is_an_open_account_whose_five_hour_window_fits_the_box_with_no_warning() {
    let fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 50.0, FIVE_RESET);
    let request = Request { expected: expected(50.0), ..request() };
    assert!(matches!(verdict_of(&fixture, &request, "a"), Verdict::Placeable { tier: Tier::One, warning: None, .. }));
}

#[test]
fn tier_2_is_an_open_account_whose_window_does_not_fit_and_it_warns_when_the_box_may_pause() {
    let fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 50.0, FIVE_RESET);
    let request = Request { expected: expected(51.0), ..request() };
    let finish_by = Unix(NOW.0 + 30 * 60);
    assert!(matches!(
        verdict_of(&fixture, &request, "a"),
        Verdict::Placeable { tier: Tier::Two, warning: Some(Warning::MayPause { finish_by: by, resumes_at: Some(resumes) }), .. } if by == finish_by && resumes == Unix(FIVE_RESET)
    ));
}

#[test]
fn tier_3_is_an_unread_account_with_reserve_zero_and_it_warns_unread() {
    let fixture = Fixture::accounts(&["a"]);
    assert!(matches!(verdict_of(&fixture, &request(), "a"), Verdict::Placeable { tier: Tier::Three, warning: Some(Warning::Unread { read_at: None }), .. }));
}

#[test]
fn an_unread_account_is_blocked_while_a_reserve_applies_and_placeable_when_toms_or_crossed() {
    let mut fixture = Fixture::accounts(&["a"]);
    fixture.settings.reserve.insert(Provider::Claude, Percent(10.0));
    assert!(matches!(verdict_of(&fixture, &request(), "a"), Verdict::Blocked(Block::Unread { .. })));
    let toms = Request { owner: Owner::Tom, ..request() };
    assert!(matches!(verdict_of(&fixture, &toms, "a"), Verdict::Placeable { tier: Tier::Three, .. }));
}

#[test]
fn a_closed_account_is_blocked_with_its_state() {
    let fixture = Fixture::accounts(&["a", "b"]).open("a", 100.0, WEEK_RESET, 0.0, FIVE_RESET).open("b", 10.0, WEEK_RESET, 100.0, FIVE_RESET);
    assert!(matches!(verdict_of(&fixture, &request(), "a"), Verdict::Blocked(Block::Closed(State::WeekOut { .. }))));
    assert!(matches!(verdict_of(&fixture, &request(), "b"), Verdict::Blocked(Block::Closed(State::Paused { .. }))));
}

#[test]
fn an_account_with_extra_usage_enabled_is_never_placed_where_it_may_pause_or_unread() {
    let mut fixture = Fixture::accounts(&["a", "b"]).open("a", 10.0, WEEK_RESET, 90.0, FIVE_RESET);
    fixture.accounts[0].extra_usage_enabled = Some(true);
    fixture.accounts[1].extra_usage_enabled = Some(true);
    let request = Request { expected: expected(20.0), ..request() };
    assert_eq!(verdict_of(&fixture, &request, "a"), Verdict::Blocked(Block::ExtraUsage));
    assert_eq!(verdict_of(&fixture, &request, "b"), Verdict::Blocked(Block::ExtraUsage));
}

#[test]
fn tier_1_accounts_order_by_soonest_week_reset_with_none_last_then_label() {
    let fixture = Fixture::accounts(&["a", "b", "c", "d"])
        .open("a", 10.0, WEEK_RESET + 500, 0.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("c", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .with(fresh(50, "d", Window::Week, 10.0, None))
        .with(fresh(51, "d", Window::FiveHour, 0.0, Some(FIVE_RESET)));
    let names: Vec<String> = order(&fixture, &request()).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["b", "c", "a", "d"]);
}

#[test]
fn tier_2_orders_by_soonest_five_hour_reset_and_follows_tier_1_and_precedes_tier_3_and_blocked() {
    let fixture = Fixture::accounts(&["a", "b", "c", "d", "e"])
        .open("a", 10.0, WEEK_RESET, 90.0, FIVE_RESET + 600)
        .open("b", 10.0, WEEK_RESET, 90.0, FIVE_RESET)
        .open("c", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("d", 100.0, WEEK_RESET, 0.0, FIVE_RESET);
    let request = Request { expected: expected(20.0), ..request() };
    assert_eq!(
        order(&fixture, &request),
        [
            ("c".to_owned(), Some(Tier::One)),
            ("b".to_owned(), Some(Tier::Two)),
            ("a".to_owned(), Some(Tier::Two)),
            ("e".to_owned(), Some(Tier::Three)),
            ("d".to_owned(), None),
        ]
    );
}

#[test]
fn in_flight_work_on_the_account_counts_against_the_five_hour_fit() {
    let fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 50.0, FIVE_RESET).running(session("working", "a", 30.0, Some(5.0)));
    let request = Request { expected: expected(30.0), ..request() };
    assert!(matches!(verdict_of(&fixture, &request, "a"), Verdict::Placeable { tier: Tier::Two, .. }));
    let parked = Fixture { sessions: vec![Session { parked_since: Some(NOW), ..session("working", "a", 30.0, Some(10.0)) }], ..fixture };
    assert!(matches!(verdict_of(&parked, &request, "a"), Verdict::Placeable { tier: Tier::One, .. }));
}

#[test]
fn a_reserve_shortfall_blocks_an_open_account() {
    let mut fixture = Fixture::accounts(&["a"]).open("a", 90.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.settings.reserve.insert(Provider::Claude, Percent(20.0));
    assert!(matches!(verdict_of(&fixture, &request(), "a"), Verdict::Blocked(Block::Reserve(_))));
}
