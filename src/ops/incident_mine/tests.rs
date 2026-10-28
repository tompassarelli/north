use serde_json::json;

use super::classify::{Message, assistant_candidate, excerpt, user_candidate};
use super::{Row, line, split};

fn message(provider: &str, agent: Option<&str>, text: &str) -> Message {
    serde_json::from_value(json!({
        "id": 7, "provider": provider, "session": "s", "project": null, "agent": agent,
        "ts": "2026-10-28T00:00:00Z", "role": "user", "text": text,
    }))
    .unwrap()
}

#[test]
fn a_typed_pushback_is_a_tom_typed_candidate_and_a_neutral_message_is_not() {
    assert_eq!(user_candidate(&message("claude", None, "no, that is wrong")), Some(("tom-typed", "pushback")));
    assert_eq!(user_candidate(&message("claude", None, "please run the build")), None);
    assert_eq!(user_candidate(&message("claude", None, &format!("wrong {}", "x".repeat(4000)))), None);
}

#[test]
fn a_relayed_message_is_classified_by_who_it_quotes() {
    assert_eq!(
        user_candidate(&message("claude", None, "From plato: Tom was frustrated with the wait")),
        Some(("relayed-tom", "pushback"))
    );
    assert_eq!(
        user_candidate(&message("claude", None, "From plato: you skipped the test")),
        Some(("relayed-lead", "correction"))
    );
    assert_eq!(user_candidate(&message("claude", Some("a1"), "you skipped the test")), None);
    assert_eq!(user_candidate(&message("claude", None, "Item: x\nTom was frustrated")), None);
    assert_eq!(user_candidate(&message("claude", None, &format!("From a: Tom was angry {}", "x".repeat(2500)))), None);
}

#[test]
fn a_long_codex_message_counts_as_relayed() {
    let text = format!("wrong {}", "x".repeat(700));
    assert_eq!(user_candidate(&message("claude", None, &text)), Some(("tom-typed", "pushback")));
    assert_eq!(user_candidate(&message("codex", None, &text)), Some(("relayed-lead", "correction")));
}

#[test]
fn assistant_flags_follow_the_first_matching_pattern() {
    for (text, flag) in [
        ("Not done: build failed", "not-done"),
        ("done\nBlocked: waiting", "blocked"),
        ("this was the second failed fix", "fix-chain"),
        ("I reverted it", "revert"),
        ("you're right, my mistake", "admission"),
    ] {
        assert_eq!(assistant_candidate(&message("claude", None, text)), Some(("assistant", flag)), "{text}");
    }
    assert_eq!(assistant_candidate(&message("claude", None, "all good")), None);
}

#[test]
fn an_excerpt_collapses_whitespace_and_cuts_at_the_limit_with_an_ellipsis() {
    assert_eq!(excerpt("  a \n\t b  ", 10), "a b");
    assert_eq!(excerpt("abcdef", 6), "abcdef");
    assert_eq!(excerpt("abcdefg", 6), "abcde…");
}

#[test]
fn a_row_is_written_with_python_separators_and_unescaped_text() {
    let row = Row {
        reference: json!(7),
        provider: "claude".to_owned(),
        session: "s".to_owned(),
        project: None,
        ts: "t".to_owned(),
        role: "user".to_owned(),
        source: "tom-typed",
        flag: "pushback",
        text: "é — \"q\"".to_owned(),
    };
    assert_eq!(
        line(&row),
        r#"{"ref": 7, "provider": "claude", "session": "s", "project": null, "ts": "t", "role": "user", "source": "tom-typed", "flag": "pushback", "text": "é — \"q\""}"#
    );
}

#[test]
fn rows_split_into_at_most_the_requested_chunks_without_reordering() {
    let lines: Vec<String> = (0..10).map(|index| format!("{index}{}", "x".repeat(9))).collect();
    let groups = split(&lines, 3);
    assert_eq!(groups.iter().map(|group| group.len()).collect::<Vec<_>>(), [3, 3, 4]);
    assert_eq!(groups.concat(), lines);
    assert_eq!(split(&lines, 1).len(), 1);
    assert!(split(&[], 6).is_empty());
}
