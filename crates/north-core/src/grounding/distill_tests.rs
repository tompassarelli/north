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

#[test]
fn a_line_tom_quotes_from_the_reply_before_is_not_read_as_his_words() {
    let reply = "The alternative is staying here: you keep every feature, but get no resume.";
    let typed = "The alternative is staying here: you keep every feature, but get no resume.\n\nis this ready yet?";
    assert_eq!(typed_text(&own_words(typed, Some(reply))), "is this ready yet?");
    assert_eq!(classify(&typed_text(&own_words(typed, Some(reply))), Some(reply)).0, None);
}

#[test]
fn an_agent_sentence_tom_pastes_with_you_keep_is_not_a_correction() {
    assert_ne!(classify("you keep every feature, but get no resume or failover", None).0, Some(Kind::Correction));
    assert_eq!(classify("you keep asking me for decisions you could make", None).0, Some(Kind::Correction));
}
