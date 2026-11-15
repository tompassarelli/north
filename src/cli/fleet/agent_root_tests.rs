use std::path::Path;

use super::*;

const OPERATOR: &str = "/home/tom";

#[test]
fn a_host_with_no_agent_login_keeps_the_brief_as_it_is() {
    assert_eq!(login_home("cpx62", Path::new(OPERATOR)), None);
    assert_eq!(login_home("tom@cpx62", Path::new(OPERATOR)), None);
    assert_eq!(login_home("@cpx62", Path::new(OPERATOR)), None);
    assert_eq!(login_home("tom@cpx62", Path::new("")), None, "an unset HOME made the operator's own login count as an agent's");
    assert_eq!(login_home("../etc@cpx62", Path::new(OPERATOR)), None, "a login naming no plain user name gave a home outside /home");
}
