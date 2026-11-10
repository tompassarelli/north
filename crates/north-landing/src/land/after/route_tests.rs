use std::cell::RefCell;

use super::*;

#[derive(Default)]
struct Fake {
    sent: RefCell<Vec<(String, String, String)>>,
}

impl Courier for Fake {
    fn here(&self, session: &str, message: &str) -> bool {
        self.sent.borrow_mut().push(("here".to_owned(), session.to_owned(), message.to_owned()));
        false
    }

    fn on(&self, host: &str, session: &str, message: &str) -> bool {
        self.sent.borrow_mut().push((host.to_owned(), session.to_owned(), message.to_owned()));
        true
    }
}

#[test]
fn a_landing_notice_reaches_a_session_on_another_host() {
    let hosts: Vec<String> = ["gex45", "cpx62", "cpx62b"].map(str::to_owned).into();
    let courier = Fake::default();

    assert!(deliver(&courier, &hosts, "cpx62b-n1081-lag", "landed n1081-runtime-lag-json as abc"));
    assert!(deliver(&courier, &hosts, "cpx62-n902-registry-gate", "landed b as def"));
    assert!(!deliver(&courier, &hosts, "land-lead", "conflict"));
    assert!(!deliver(&courier, &hosts, "cpx62b", "no dash"));

    let sent = courier.sent.borrow();
    let told: Vec<(&str, &str)> = sent.iter().map(|(host, session, _)| (host.as_str(), session.as_str())).collect();
    assert_eq!(told, [("cpx62b", "cpx62b-n1081-lag"), ("cpx62", "cpx62-n902-registry-gate"), ("here", "land-lead"), ("here", "cpx62b")]);
    assert_eq!(sent[0].2, "landed n1081-runtime-lag-json as abc");
}
