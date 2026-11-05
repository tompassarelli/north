use super::*;
use crate::usage::decide::{Action, Assessment, Assessments, Scope, assess};

const WEEK_RESET: i64 = NOW.0 + 100_000;
const FIVE_RESET: i64 = NOW.0 + 9_000;

fn ws(name: &str) -> Workstream {
    Workstream(name.to_owned())
}

fn worker(name: &str, workstream: &str, box_id: &str, spent: f64, p90: f64) -> Session {
    Session {
        workstream: Some(ws(workstream)),
        box_id: Some(BoxId(box_id.to_owned())),
        expected: Expected { p50: Usd(p90 / 2.0), p90: Usd(p90), minutes: 40.0, source: EstimateSource::Cell("cell".to_owned()) },
        active_minutes: 40.0,
        ..session(name, "a", p90 / 2.0, Some(spent))
    }
}

fn open_a() -> Fixture {
    Fixture::accounts(&["a", "b"]).open("a", 10.0, WEEK_RESET, 0.0, FIVE_RESET).open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
}

fn two_ticks(fixture: &Fixture) -> Assessments {
    let first = assess(&fixture.world(), &Assessments::default());
    assess(&fixture.world(), &first)
}

fn workstream_state(assessments: &Assessments, name: &str) -> Assessment {
    assessments.spend[&Scope::Workstream(ws(name))].confirmed.clone()
}

fn box_state(assessments: &Assessments, name: &str) -> Assessment {
    assessments.spend[&Scope::Box(BoxId(name.to_owned()))].confirmed.clone()
}

#[test]
fn spend_row_1_a_box_with_no_estimate_or_a_zero_p90_is_unestimated_and_never_trips() {
    let mut none = worker("w", "n#1", "b1", 100.0, 2.0);
    none.expected.source = EstimateSource::None;
    let zero = worker("z", "n#2", "b2", 100.0, 0.0);
    let fixture = open_a().running(none).running(zero);
    let answer = assess(&fixture.world(), &Assessments::default());
    assert_eq!(box_state(&answer, "b1"), Assessment::Unestimated);
    assert_eq!(box_state(&answer, "b2"), Assessment::Unestimated);
}

#[test]
fn spend_row_1_a_workstream_with_only_a_lead_has_no_boxes_and_is_unestimated() {
    let lead = Session { standing: Standing::Lead, ..worker("lead", "n#3", "b3", 50.0, 2.0) };
    let answer = assess(&open_a().running(lead).world(), &Assessments::default());
    assert_eq!(workstream_state(&answer, "n#3"), Assessment::Unestimated);
    assert!(!answer.spend.contains_key(&Scope::Box(BoxId("b3".to_owned()))));
}

#[test]
fn spend_row_2_a_recorded_clear_below_four_times_its_p90_is_cleared_and_its_p90_is_in_force() {
    let mut fixture = open_a().running(worker("w", "n#4", "b4", 9.0, 2.0));
    fixture.settings.clears.push(Clear { workstream: ws("n#4"), p90: Usd(5.0), by: "brief".to_owned() });
    let answer = assess(&fixture.world(), &Assessments::default());
    assert_eq!(workstream_state(&answer, "n#4"), Assessment::Cleared { by: "brief".to_owned() });
    assert_eq!(answer.spend[&Scope::Workstream(ws("n#4"))].p90, Usd(5.0));
    fixture.sessions[0].spent = Some(Usd(20.0));
    let answer = two_ticks(&fixture);
    assert_eq!(workstream_state(&answer, "n#4"), Assessment::Tripped);
}

#[test]
fn spend_row_3_a_trip_needs_four_times_p90_at_two_consecutive_ticks() {
    let fixture = open_a().running(worker("w", "n#5", "b5", 8.0, 2.0));
    let first = assess(&fixture.world(), &Assessments::default());
    assert_eq!(workstream_state(&first, "n#5"), Assessment::Within);
    let second = assess(&fixture.world(), &first);
    assert_eq!(workstream_state(&second, "n#5"), Assessment::Tripped);
    assert!(second.spend[&Scope::Workstream(ws("n#5"))].changed);
    assert_eq!(second.trips(), vec![Trip { workstream: ws("n#5"), spent: Usd(8.0), p90: Usd(2.0) }]);
    let third = assess(&fixture.world(), &second);
    assert!(!third.spend[&Scope::Workstream(ws("n#5"))].changed);
}

#[test]
fn spend_row_4_two_times_p90_warns() {
    let answer = two_ticks(&open_a().running(worker("w", "n#6", "b6", 4.0, 2.0)));
    assert_eq!(workstream_state(&answer, "n#6"), Assessment::Warn);
}

#[test]
fn spend_row_5_half_the_p50_spent_in_under_a_quarter_of_the_minutes_is_velocity() {
    let fast = Session { active_minutes: 5.0, ..worker("w", "n#7", "b7", 1.0, 2.0) };
    assert_eq!(workstream_state(&two_ticks(&open_a().running(fast)), "n#7"), Assessment::Velocity);
}

#[test]
fn spend_row_6_otherwise_within() {
    assert_eq!(workstream_state(&two_ticks(&open_a().running(worker("w", "n#8", "b8", 1.0, 2.0))), "n#8"), Assessment::Within);
}

#[test]
fn a_condition_true_at_only_one_tick_keeps_the_previous_assessment() {
    let calm = open_a().running(worker("w", "n#9", "b9", 1.0, 2.0));
    let settled = two_ticks(&calm);
    let spike = Fixture { sessions: vec![worker("w", "n#9", "b9", 9.0, 2.0)], ..calm.clone() };
    let spiked = assess(&spike.world(), &settled);
    assert_eq!(workstream_state(&spiked, "n#9"), Assessment::Within);
    let back = assess(&calm.world(), &spiked);
    assert_eq!(workstream_state(&back, "n#9"), Assessment::Within);
}

#[test]
fn a_follows_chain_is_one_box_and_a_workstream_sums_its_boxes_without_the_lead() {
    let fixture = open_a()
        .running(worker("w1", "n#10", "b10", 1.0, 2.0))
        .running(worker("w2", "n#10", "b10", 2.0, 2.0))
        .running(worker("w3", "n#10", "b11", 3.0, 4.0))
        .running(Session { standing: Standing::Lead, ..worker("lead", "n#10", "b12", 50.0, 9.0) });
    let answer = assess(&fixture.world(), &Assessments::default());
    let one_box = &answer.spend[&Scope::Box(BoxId("b10".to_owned()))];
    assert_eq!((one_box.spent, one_box.p90), (Usd(3.0), Usd(2.0)));
    let whole = &answer.spend[&Scope::Workstream(ws("n#10"))];
    assert_eq!((whole.spent, whole.p90), (Usd(6.0), Usd(6.0)));
    assert!(!answer.spend.contains_key(&Scope::Box(BoxId("b12".to_owned()))));
}

#[test]
fn toms_sessions_are_never_assessed_parked_or_handed_over() {
    let mut toms = worker("toms", "n#11", "b13", 99.0, 1.0);
    toms.owner = Owner::Tom;
    toms.account = label("c");
    let fixture = Fixture::accounts(&["a", "b", "c"])
        .open("a", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("c", 100.0, WEEK_RESET, 0.0, FIVE_RESET)
        .running(toms);
    let answer = two_ticks(&fixture);
    assert!(answer.spend.is_empty() && answer.actions.is_empty() && answer.all_out.is_none());
}

#[test]
fn work_row_2_a_session_on_a_paused_account_parks_once_with_the_resume_time() {
    let fixture = Fixture::accounts(&["a", "b"])
        .open("a", 10.0, WEEK_RESET, 100.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .running(worker("w", "n#12", "b14", 0.0, 2.0));
    let answer = assess(&fixture.world(), &Assessments::default());
    assert_eq!(answer.actions, vec![(Name("w".to_owned()), Action::Park { resumes_at: Some(Unix(FIVE_RESET)) })]);
    let parked = Fixture { sessions: vec![Session { parked_since: Some(Unix(NOW.0 - 5)), ..worker("w", "n#12", "b14", 0.0, 2.0) }], ..fixture };
    assert!(assess(&parked.world(), &answer).actions.is_empty());
}

#[test]
fn work_row_1_a_lead_paused_ten_minutes_is_handed_over_before_any_worker() {
    let lead = Session { standing: Standing::Lead, parked_since: Some(Unix(NOW.0 - 601)), ..worker("lead", "n#13", "b15", 0.0, 2.0) };
    let fixture = Fixture::accounts(&["a", "b"])
        .open("a", 10.0, WEEK_RESET, 100.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .running(worker("aaa", "n#13", "b16", 0.0, 2.0))
        .running(lead);
    let answer = assess(&fixture.world(), &Assessments::default());
    let names: Vec<&str> = answer.actions.iter().map(|(name, _)| name.0.as_str()).collect();
    assert_eq!(names, ["lead", "aaa"]);
    assert_eq!(answer.actions[0].1, Action::HandOver { to: label("b") });
    assert!(matches!(answer.actions[1].1, Action::Park { .. }));
}

#[test]
fn work_row_1_a_childless_proxy_on_a_paused_account_is_handed_over_at_once() {
    let proxy = Session { standing: Standing::Proxy, ..worker("proxy", "n#14", "b17", 0.0, 2.0) };
    let fixture = Fixture::accounts(&["a", "b"]).open("a", 10.0, WEEK_RESET, 100.0, FIVE_RESET).open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET).running(proxy);
    assert_eq!(assess(&fixture.world(), &Assessments::default()).actions, vec![(Name("proxy".to_owned()), Action::HandOver { to: label("b") })]);
}

#[test]
fn work_row_1_a_lead_paused_under_ten_minutes_is_not_handed_over() {
    let lead = Session { standing: Standing::Lead, parked_since: Some(Unix(NOW.0 - 599)), ..worker("lead", "n#14", "b17", 0.0, 2.0) };
    let fixture = Fixture::accounts(&["a", "b"]).open("a", 10.0, WEEK_RESET, 100.0, FIVE_RESET).open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET).running(lead);
    assert!(assess(&fixture.world(), &Assessments::default()).actions.is_empty());
}

#[test]
fn work_row_3_a_week_out_or_scope_out_session_is_handed_over_when_place_finds_another_account() {
    let week_out = Fixture::accounts(&["a", "b"])
        .open("a", 100.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 0.0, FIVE_RESET)
        .running(worker("w", "n#15", "b18", 0.0, 2.0));
    assert_eq!(assess(&week_out.world(), &Assessments::default()).actions, vec![(Name("w".to_owned()), Action::HandOver { to: label("b") })]);
    let scope_out = Fixture {
        limits: vec![limit("a", Window::Scoped(ModelFamily::Opus), Some(WEEK_RESET), NOW.0 - 5)],
        ..open_a().running(worker("w", "n#15", "b18", 0.0, 2.0))
    };
    assert_eq!(assess(&scope_out.world(), &Assessments::default()).actions, vec![(Name("w".to_owned()), Action::HandOver { to: label("b") })]);
    let nowhere = Fixture::accounts(&["a"]).open("a", 100.0, WEEK_RESET, 0.0, FIVE_RESET).running(worker("w", "n#15", "b18", 0.0, 2.0));
    assert!(assess(&nowhere.world(), &Assessments::default()).actions.is_empty());
}

#[test]
fn work_row_4_every_account_out_for_every_family_in_flight_tells_tom_once_with_the_earliest_resume() {
    let fixture = Fixture::accounts(&["a", "b"])
        .open("a", 100.0, WEEK_RESET, 0.0, FIVE_RESET)
        .open("b", 10.0, WEEK_RESET, 100.0, FIVE_RESET - 500)
        .running(worker("w", "n#16", "b19", 0.0, 2.0));
    let first = assess(&fixture.world(), &Assessments::default());
    let all_out = first.all_out.clone().expect("all out");
    assert_eq!((all_out.earliest_resume, all_out.announce), (Some(Unix(FIVE_RESET - 500)), true));
    assert!(!assess(&fixture.world(), &first).all_out.expect("still out").announce);
    let recovered = assess(&open_a().running(worker("w", "n#16", "b19", 0.0, 2.0)).world(), &first);
    assert!(recovered.all_out.is_none());
}

#[test]
fn work_row_4_a_scope_out_for_a_family_not_in_flight_does_not_make_every_account_out() {
    let sonnet = Session { family: ModelFamily::Sonnet, ..worker("w", "n#17", "b20", 0.0, 2.0) };
    let fixture = Fixture {
        limits: vec![
            limit("a", Window::Scoped(ModelFamily::Opus), Some(WEEK_RESET), NOW.0 - 5),
            limit("b", Window::Scoped(ModelFamily::Opus), Some(WEEK_RESET), NOW.0 - 5),
        ],
        ..open_a().running(sonnet)
    };
    assert!(assess(&fixture.world(), &Assessments::default()).all_out.is_none());
}
