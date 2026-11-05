use super::*;

#[test]
fn rid_and_iso_match_the_typescript_forms() {
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(rid("convo:sess:/t/a.jsonl:3"), format!("L{}", &sha1_hex(b"convo:sess:/t/a.jsonl:3")[..10]));
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso(), "2026-10-15T10:52:55Z");
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso_millis(), "2026-10-15T10:52:55.752Z");
}

#[test]
fn a_fleet_message_relayed_into_a_session_is_never_read_as_tom_typing() {
    assert!(is_skip_message("[from proxy] Fact, no order: the lane is innocent."));
    assert!(is_skip_message("[from service:north-land] landed w230"));
    assert!(!is_skip_message("from now on file every plan as an issue"));
}
