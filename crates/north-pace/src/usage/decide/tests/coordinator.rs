use super::super::work::actions;
use super::super::Action;
use super::*;

fn proxy(account: &str) -> Session {
    Session {
        name: Name("proxy".into()),
        owner: Owner::Fleet,
        account: label(account),
        family: ModelFamily::Fable,
        standing: Standing::Proxy,
        parked_since: None,
    }
}

fn meters(a: (f64, f64), b: (f64, f64)) -> Vec<Reading> {
    let reset = Some(NOW.0 + 3600);
    let read_at = NOW.0 - 60;
    vec![
        reading(0, "a", Window::FiveHour, 0.0, reset, read_at, Source::Header),
        reading(1, "a", Window::Week, a.1, reset, read_at, Source::Header),
        reading(2, "a", Window::Scoped(ModelFamily::Fable), a.0, reset, read_at, Source::Header),
        reading(3, "b", Window::FiveHour, 0.0, reset, read_at, Source::Header),
        reading(4, "b", Window::Week, b.1, reset, read_at, Source::Header),
        reading(5, "b", Window::Scoped(ModelFamily::Fable), b.0, reset, read_at, Source::Header),
    ]
}

fn hands_over(fixture: Fixture) -> Vec<(Name, Action)> {
    actions(&fixture.world())
}

#[test]
fn the_coordinator_hands_over_before_its_fable_meter_reaches_the_floor() {
    let fixture = Fixture { accounts: vec![account("a", &["host"]), account("b", &["host"])], readings: meters((99.0, 10.0), (40.0, 10.0)), sessions: vec![proxy("a")], ..Fixture::default() };
    assert_eq!(hands_over(fixture), [(Name("proxy".into()), Action::HandOver { to: label("b"), family: None })]);
}

#[test]
fn with_no_fable_headroom_left_the_coordinator_hands_over_to_opus_high() {
    let fixture = Fixture { accounts: vec![account("a", &["host"]), account("b", &["host"])], readings: meters((99.0, 90.0), (99.0, 10.0)), sessions: vec![proxy("a")], ..Fixture::default() };
    assert_eq!(hands_over(fixture), [(Name("proxy".into()), Action::HandOver { to: label("b"), family: Some(ModelFamily::Opus) })]);
}
