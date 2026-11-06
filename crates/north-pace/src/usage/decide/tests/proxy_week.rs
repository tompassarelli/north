use super::*;
use crate::usage::decide::assess;
use crate::usage::decide::Action;

fn proxy_on(account: &str) -> Session {
    Session { standing: Standing::Proxy, ..session("proxy", account, 0.0, None) }
}

fn week_and_five_hour(row: i64, name: &str, week: f64) -> [Reading; 2] {
    [fresh(row, name, Window::Week, week, Some(NOW.0 + 6 * 3600)), fresh(row + 1, name, Window::FiveHour, 20.0, Some(NOW.0 + 3600))]
}

#[test]
fn the_proxy_hands_over_when_its_week_reaches_the_ceiling_and_another_account_has_week_headroom() {
    let mut fixture = Fixture {
        accounts: vec![account("a", &["laptop"]), account("b", &["laptop"]), account("c", &["laptop"])],
        readings: [week_and_five_hour(1, "a", 96.0), week_and_five_hour(3, "b", 97.0), week_and_five_hour(5, "c", 40.0)].concat(),
        sessions: vec![proxy_on("a"), session("worker", "a", 0.0, None)],
        ..Fixture::default()
    };
    let actions = assess(&fixture.world(), &Default::default()).actions;
    assert_eq!(actions, vec![(Name("proxy".to_owned()), Action::HandOver { to: label("c") })]);

    fixture.readings = [week_and_five_hour(1, "a", 96.0), week_and_five_hour(3, "b", 97.0), week_and_five_hour(5, "c", 99.0)].concat();
    assert!(assess(&fixture.world(), &Default::default()).actions.is_empty());
}
