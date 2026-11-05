use super::*;
use crate::usage::decide::free::gate;
use crate::usage::decide::{Gate, Shortfall, free};

const WEEK_RESET: i64 = NOW.0 + 100_000;
const FIVE_RESET: i64 = NOW.0 + 9_000;

fn reserve(percent: f64) -> Settings {
    Settings { reserve: [(Provider::Claude, Percent(percent))].into(), ..Settings::default() }
}

fn gate_for(fixture: &Fixture, request: &Request, name: &str) -> Gate {
    let world = fixture.world();
    let account = fixture.accounts.iter().find(|account| account.label == label(name)).expect("account");
    gate(&world, request, account, &free(&world, Provider::Claude))
}

#[test]
fn free_is_the_weight_averaged_week_remaining_of_fresh_accounts() {
    let fixture = Fixture::accounts(&["a", "b"]).open("a", 40.0, WEEK_RESET, 0.0, FIVE_RESET).open("b", 100.0, WEEK_RESET, 0.0, FIVE_RESET);
    let answer = free(&fixture.world(), Provider::Claude);
    assert_eq!(answer.percent, 30.0);
    assert!(answer.unread.is_empty());
}

#[test]
fn free_counts_a_stale_or_unread_account_as_zero_and_names_it() {
    let mut fixture = Fixture::accounts(&["a", "b", "c"]).open("a", 0.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture = fixture.with(reading(90, "b", Window::Week, 0.0, Some(WEEK_RESET), NOW.0 - 5_000, Source::Header));
    let answer = free(&fixture.world(), Provider::Claude);
    assert_eq!(answer.percent, 100.0 / 3.0);
    assert_eq!(answer.unread, vec![label("b"), label("c")]);
}

#[test]
fn free_counts_a_limited_account_as_zero_without_calling_it_unread() {
    let fixture = Fixture::accounts(&["a", "b"]).open("a", 0.0, WEEK_RESET, 0.0, FIVE_RESET).limited(limit("b", Window::Week, Some(WEEK_RESET), NOW.0 - 10));
    let answer = free(&fixture.world(), Provider::Claude);
    assert_eq!(answer.percent, 50.0);
    assert!(answer.unread.is_empty());
}

#[test]
fn free_ignores_other_providers_accounts() {
    let mut fixture = Fixture::accounts(&["a", "g"]).open("a", 20.0, WEEK_RESET, 0.0, FIVE_RESET).open("g", 0.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.accounts[1].provider = Provider::Gemini;
    assert_eq!(free(&fixture.world(), Provider::Claude).percent, 80.0);
}

#[test]
fn in_flight_is_the_unspent_expected_cost_in_week_points_and_skips_parked_and_toms_sessions() {
    let mut parked = session("parked", "a", 50.0, None);
    parked.parked_since = Some(Unix(NOW.0 - 100));
    let mut toms = session("toms", "a", 50.0, None);
    toms.owner = Owner::Tom;
    let mut fixture = Fixture::accounts(&["a", "b"])
        .open("a", 0.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("b", 0.0, WEEK_RESET, 0.0, FIVE_RESET)
        .running(session("working", "a", 10.0, Some(4.0)))
        .running(session("overspent", "b", 5.0, Some(9.0)))
        .running(parked)
        .running(toms);
    fixture.accounts[0].rates.week = Rate::Account { points_per_usd: 2.0, n: 30 };
    assert_eq!(free(&fixture.world(), Provider::Claude).in_flight, 6.0);
}

#[test]
fn gate_row_1_passes_for_toms_own_requests() {
    let mut fixture = Fixture::accounts(&["a"]).open("a", 99.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.settings = reserve(50.0);
    let request = Request { owner: Owner::Tom, expected: expected(30.0), ..request() };
    assert_eq!(gate_for(&fixture, &request, "a"), Gate::Pass { crossed: false });
}

#[test]
fn gate_row_2_passes_when_the_reserve_is_zero() {
    let fixture = Fixture::accounts(&["a"]).open("a", 99.0, WEEK_RESET, 0.0, FIVE_RESET);
    let request = Request { expected: expected(30.0), ..request() };
    assert_eq!(gate_for(&fixture, &request, "a"), Gate::Pass { crossed: false });
}

#[test]
fn gate_row_3_passes_while_a_crossing_is_active_and_not_after() {
    let mut fixture = Fixture::accounts(&["a"]).open("a", 99.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.settings = reserve(50.0);
    let request = Request { expected: expected(30.0), ..request() };
    fixture.settings.crossings.push(Crossing { provider: Provider::Claude, until: Unix(NOW.0 + 1), gate: "g1".to_owned() });
    assert_eq!(gate_for(&fixture, &request, "a"), Gate::Pass { crossed: true });
    fixture.settings.crossings[0].until = NOW;
    assert!(matches!(gate_for(&fixture, &request, "a"), Gate::Reserve(_)));
}

#[test]
fn gate_row_4_passes_when_free_less_in_flight_less_this_box_still_meets_the_reserve() {
    let mut fixture = Fixture::accounts(&["a", "b"]).open("a", 0.0, WEEK_RESET, 0.0, FIVE_RESET).open("b", 0.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.settings = reserve(80.0);
    fixture.sessions.push(session("working", "a", 20.0, None));
    let fits = Request { expected: expected(20.0), ..request() };
    assert_eq!(gate_for(&fixture, &fits, "a"), Gate::Pass { crossed: false });
}

#[test]
fn gate_row_5_reports_the_shortfall_inputs() {
    let mut fixture = Fixture::accounts(&["a", "b"]).open("a", 0.0, WEEK_RESET, 0.0, FIVE_RESET).open("b", 0.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.settings = reserve(80.0);
    fixture.sessions.push(session("working", "a", 20.0, None));
    let over = Request { expected: expected(21.0), ..request() };
    assert_eq!(gate_for(&fixture, &over, "a"), Gate::Reserve(Shortfall { free: 100.0, in_flight: 10.0, request: 10.5, reserve: 80.0 }));
}
