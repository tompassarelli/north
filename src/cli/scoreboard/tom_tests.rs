use super::*;

fn turn(first_line: i64, end_line: Option<i64>) -> CoordinatorTurn {
    CoordinatorTurn { at: 0, tom: true, first_line, end_line, recommends: false, turn: None }
}

#[test]
fn a_closed_turn_is_parsed_once_and_not_reread_north_1553() {
    let dir = tempfile::tempdir().unwrap();
    let transcript = dir.path().join("t.jsonl");
    let lines = [
        r#"{"type":"user","uuid":"a","message":{"content":"first"}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"one"}]}}"#,
        r#"{"type":"user","uuid":"b","message":{"content":"second"}}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"two"}]}}"#,
    ];
    fs::write(&transcript, lines.join("\n") + "\n").unwrap();
    let path = transcript.to_str().unwrap();
    let store = parsed_store(&dir.path().join("index.db"));
    let mut first = [turn(1, Some(3)), turn(3, None)];
    parse_turns(store.as_ref(), path, &mut first);
    assert_eq!(first[1].turn.as_ref().and_then(|turn| turn.reply.as_deref()), Some("two"));
    let replies = |turns: &[CoordinatorTurn]| turns.iter().map(|coordinator| coordinator.turn.as_ref().and_then(|turn| turn.reply.clone())).collect::<Vec<_>>();
    fs::write(&transcript, "").unwrap();
    let mut again = [turn(1, Some(3))];
    parse_turns(store.as_ref(), path, &mut again);
    assert_eq!(replies(&again), vec![Some("one".to_owned())]);
}
