use super::*;

#[test]
fn a_replay_change_trailer_names_a_known_decision_and_a_reason() {
    assert_eq!(declared_kind("Replay-change: land.failed lanes need a cached clippy verdict"), Some(Ok(("failed".to_owned(), "lanes need a cached clippy verdict".to_owned()))));
    assert_eq!(declared_kind("Follows: abc"), None);
    assert!(declared_kind("Replay-change: land.failed").is_some_and(|parsed| parsed.is_err()));
    assert!(declared_kind("Replay-change: clippy lanes need a verdict").is_some_and(|parsed| parsed.is_err()));
    assert!(refuse_malformed("Subject\n\nReplay-change: land.landed\n").unwrap_err().contains("gives no reason"));
    assert!(refuse_malformed("Subject\n\nReplay-change: land.landed red-fix lanes now land\n").is_ok());
}
