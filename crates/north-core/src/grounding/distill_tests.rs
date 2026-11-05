use super::*;

#[test]
fn rid_and_iso_match_the_typescript_forms() {
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(rid("convo:sess:/t/a.jsonl:3"), format!("L{}", &sha1_hex(b"convo:sess:/t/a.jsonl:3")[..10]));
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso(), "2026-10-15T10:52:55Z");
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso_millis(), "2026-10-15T10:52:55.752Z");
}
