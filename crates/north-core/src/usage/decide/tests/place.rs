use super::*;
use crate::usage::decide::{Decision, Note, Refusal, State, Tier, Warning, explain, place};

const WEEK_RESET: i64 = NOW.0 + 100_000;
const FIVE_RESET: i64 = NOW.0 + 9_000;

fn placed(decision: &Decision) -> &str {
    match decision {
        Decision::Place { account, .. } => &account.0,
        Decision::Refuse(refusal) => panic!("refused: {refusal:?}"),
    }
}

fn refused(decision: Decision) -> Refusal {
    match decision {
        Decision::Refuse(refusal) => refusal,
        Decision::Place { account, .. } => panic!("placed on {}", account.0),
    }
}

fn two_open() -> Fixture {
    Fixture::accounts(&["a", "b"]).open("a", 10.0, WEEK_RESET + 500, 0.0, FIVE_RESET).open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
}

fn trip(workstream: &str) -> Trip {
    Trip { workstream: Workstream(workstream.to_owned()), spent: Usd(9.0), p90: Usd(2.0) }
}

#[test]
fn row_1_refuses_a_request_on_a_tripped_workstream_even_when_accounts_are_open() {
    let mut fixture = two_open();
    fixture.tripped.push(trip("north#88"));
    let request = Request { workstream: Some(Workstream("north#88".to_owned())), ..request() };
    assert!(matches!(refused(place(&fixture.world(), &request)), Refusal::Tripped { spent: Usd(9.0), .. }));
    let other = Request { workstream: Some(Workstream("north#89".to_owned())), ..request };
    assert_eq!(placed(&place(&fixture.world(), &other)), "b");
}

#[test]
fn row_2_refuses_a_pin_that_names_no_account_of_the_provider() {
    let request = Request { pin: Some(label("z")), ..request() };
    assert_eq!(refused(place(&two_open().world(), &request)), Refusal::UnknownAccount { pin: label("z") });
}

#[test]
fn row_3_places_a_placeable_pin_and_notes_which_account_ranks_first() {
    let request = Request { pin: Some(label("a")), ..request() };
    let decision = place(&two_open().world(), &request);
    let Decision::Place { account, notes, .. } = decision else { panic!("refused") };
    assert_eq!(account, label("a"));
    assert_eq!(notes, vec![Note::NotFirst { first: label("b") }]);
    let first = Request { pin: Some(label("b")), ..request };
    let Decision::Place { notes, .. } = place(&two_open().world(), &first) else { panic!("refused") };
    assert!(notes.is_empty());
}

#[test]
fn row_4_refuses_a_pin_that_is_not_placeable_and_names_what_could_take_it() {
    let mut fixture = two_open().open("c", 100.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.accounts.push(account("c", &["gex45"]));
    let request = Request { pin: Some(label("c")), ..request() };
    let Refusal::Pinned { account, state, first, .. } = refused(place(&fixture.world(), &request)) else { panic!("wrong refusal") };
    assert_eq!((account, first), (label("c"), Some(label("b"))));
    assert!(matches!(state, State::WeekOut { .. }));
}

#[test]
fn row_4_refuses_a_pin_that_is_placeable_but_not_signed_in_on_the_request_host() {
    let mut fixture = two_open();
    fixture.accounts[0].signed_in = [Host("laptop".to_owned())].into();
    let request = Request { pin: Some(label("a")), host: Some(Host("gex45".to_owned())), ..request() };
    assert!(matches!(refused(place(&fixture.world(), &request)), Refusal::Pinned { block: None, .. }));
}

#[test]
fn row_5_places_the_first_account_by_tier_and_order_restricted_to_the_request_host() {
    let mut fixture = two_open();
    assert_eq!(placed(&place(&fixture.world(), &request())), "b");
    fixture.accounts[1].signed_in = [Host("laptop".to_owned())].into();
    let on_gex45 = Request { host: Some(Host("gex45".to_owned())), ..request() };
    assert_eq!(placed(&place(&fixture.world(), &on_gex45)), "a");
}

#[test]
fn row_5_carries_the_tier_2_warning_in_the_decision() {
    let fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 90.0, FIVE_RESET);
    let request = Request { expected: expected(20.0), ..request() };
    let Decision::Place { tier, notes, .. } = place(&fixture.world(), &request) else { panic!("refused") };
    assert_eq!(tier, Tier::Two);
    assert!(matches!(notes[..], [Note::Warning(Warning::MayPause { .. })]));
}

#[test]
fn a_placeable_account_elsewhere_but_none_on_the_host_refuses_naming_the_first() {
    let mut fixture = two_open();
    fixture.accounts.iter_mut().for_each(|account| account.signed_in = [Host("laptop".to_owned())].into());
    let request = Request { host: Some(Host("gex45".to_owned())), ..request() };
    assert_eq!(refused(place(&fixture.world(), &request)), Refusal::NotOnHost { host: Host("gex45".to_owned()), first: label("b") });
}

#[test]
fn row_6_refuses_all_paused_with_the_earliest_resume_when_every_account_is_out() {
    let fixture = Fixture::accounts(&["a", "b", "c"])
        .open("a", 100.0, WEEK_RESET + 700, 0.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 100.0, FIVE_RESET + 50)
        .open("c", 10.0, WEEK_RESET, 100.0, FIVE_RESET);
    assert_eq!(refused(place(&fixture.world(), &request())), Refusal::AllPaused { family: ModelFamily::Opus, earliest_resume: Some(Unix(FIVE_RESET)) });
}

#[test]
fn row_7_refuses_for_the_reserve_when_an_open_account_does_not_pass_the_gate() {
    let mut fixture = Fixture::accounts(&["a"]).open("a", 90.0, WEEK_RESET, 0.0, FIVE_RESET);
    fixture.settings.reserve.insert(Provider::Claude, Percent(20.0));
    assert!(matches!(refused(place(&fixture.world(), &request())), Refusal::Reserve(shortfall) if shortfall.reserve == 20.0 && shortfall.free == 10.0));
}

#[test]
fn row_7_refuses_for_extra_usage_when_the_only_open_account_would_pause_into_paid_spend() {
    let mut fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 90.0, FIVE_RESET);
    fixture.accounts[0].extra_usage_enabled = Some(true);
    let request = Request { expected: expected(20.0), ..request() };
    assert_eq!(refused(place(&fixture.world(), &request)), Refusal::ExtraUsage { accounts: vec![label("a")] });
}

#[test]
fn row_8_refuses_none_readable_with_every_accounts_state_when_a_reserve_blocks_unread_accounts() {
    let mut fixture = Fixture::accounts(&["a", "b"]);
    fixture.settings.reserve.insert(Provider::Claude, Percent(5.0));
    let Refusal::NoneReadable { states } = refused(place(&fixture.world(), &request())) else { panic!("wrong refusal") };
    assert_eq!(states.iter().map(|(label, _)| label.0.as_str()).collect::<Vec<_>>(), ["a", "b"]);
}

#[test]
fn a_provider_with_no_accounts_refuses_none_readable() {
    assert_eq!(refused(place(&Fixture::default().world(), &request())), Refusal::NoneReadable { states: Vec::new() });
}

#[test]
fn a_scope_out_for_another_family_does_not_stop_this_family() {
    let fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 0.0, FIVE_RESET).limited(limit(
        "a",
        Window::Scoped(ModelFamily::Opus),
        Some(WEEK_RESET),
        NOW.0 - 5,
    ));
    assert_eq!(placed(&place(&fixture.world(), &Request { family: ModelFamily::Sonnet, ..request() })), "a");
    assert!(matches!(refused(place(&fixture.world(), &request())), Refusal::AllPaused { .. }));
}

fn clock(at: Unix) -> String {
    format!("t{}", at.0 - NOW.0)
}

#[test]
fn explain_prints_one_line_from_the_decisions_inputs() {
    let fixture = two_open();
    assert_eq!(explain(&place(&fixture.world(), &request()), &clock), "place b: tier 1, week 10%, 5h 0%");
    let pinned = Request { pin: Some(label("a")), ..request() };
    assert_eq!(explain(&place(&fixture.world(), &pinned), &clock), "place a: tier 1, week 10%, 5h 0%; b ranks first");
}

#[test]
fn explain_names_the_warning_and_the_refusal_inputs() {
    let fixture = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 90.0, FIVE_RESET);
    let request = Request { expected: expected(20.0), ..request() };
    assert_eq!(explain(&place(&fixture.world(), &request), &clock), "place a: tier 2, week 10%, 5h 90%; may pause before it finishes (by t1800), back t9000");
    let full = Fixture::accounts(&["a"]).open("a", 10.0, WEEK_RESET, 100.0, FIVE_RESET);
    assert_eq!(explain(&place(&full.world(), &request), &clock), "refuse: every account is out for Opus, earliest resume t9000");
}
