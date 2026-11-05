#![cfg(test)]

#[test]
fn a_configured_client_cap_prints_its_number_and_an_unconfigured_host_prints_nothing() {
    assert_eq!(super::command::limit_text(Some(6)), "6");
    assert_eq!(super::command::limit_text(None), "");
}
