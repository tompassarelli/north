use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::tom_turn;
use crate::fleet::gates::{GateBook, GateRequest, relay_gate};
use crate::ledger::Ledger;

const CONVERSATION: &str = "1f79cfb7-4cc6-4879-9916-81371e094167";

fn convo(dir: &Path, rows: &[Value]) -> std::path::PathBuf {
    let transcript = dir.join(format!("{CONVERSATION}.jsonl"));
    let index = dir.join("index.db");
    let db = Connection::open(&index).unwrap();
    db.execute_batch(
        "CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT, provider TEXT, agent TEXT, cwd TEXT);
         CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INTEGER, line INTEGER, ts TEXT, role TEXT, tool TEXT, body INTEGER, dup INTEGER DEFAULT 0);
         CREATE INDEX msg_file ON msg(file_id, line);
         CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT);",
    )
    .unwrap();
    db.execute("INSERT INTO files VALUES (1, ?1, 'anthropic', NULL, '/home/tom/code/north/main')", [transcript.to_string_lossy()]).unwrap();
    let mut lines = String::new();
    for (number, row) in rows.iter().enumerate() {
        let id = 10_519_700 + number as i64;
        lines.push_str(&format!("{row}\n"));
        db.execute("INSERT INTO body VALUES (?1, ?2)", (id, row["message"]["content"].as_str().unwrap())).unwrap();
        db.execute("INSERT INTO msg VALUES (?1, 1, ?2, ?3, 'user', NULL, ?1, 0)", (id, number as i64 + 1, row["timestamp"].as_str().unwrap())).unwrap();
    }
    fs::write(transcript, lines).unwrap();
    index
}

fn typed(at: &str, text: &str) -> Value {
    json!({"type": "user", "promptSource": "typed", "origin": {"kind": "human"}, "timestamp": at, "message": {"content": text}})
}

fn delivered(at: &str, text: &str) -> Value {
    json!({"type": "user", "promptSource": "system", "isMeta": true, "origin": {"kind": "peer", "from": "unknown"}, "timestamp": at, "message": {"content": format!("Another Claude session sent a message:\n{text}")}})
}

#[test]
fn a_proxy_window_relays_an_answer_tom_typed_hours_earlier_from_a_turn_a_fleet_message_opened() {
    let dir = tempfile::tempdir().unwrap();
    let rows = [
        typed("2026-11-07T01:00:00Z", "You are the proxy."),
        typed("2026-11-07T03:10:00Z", "2. keep it local"),
        json!({"type": "user", "timestamp": "2026-11-07T04:00:00Z", "message": {"content": "This session is being continued from a previous conversation. 1. encrypt it"}}),
        delivered("2026-11-07T09:30:00Z", "[from service:north-fleet] gate 12 still waits on Tom"),
        delivered("2026-11-07T09:31:00Z", "[from north-88-d1828] 1"),
    ];
    let index = convo(dir.path(), &rows);
    let home = Path::new("/home/tom");
    let opened_ms = 1_794_016_800_000;
    let book = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let ask = GateRequest { verification: false, issue: None, box_text: None, session: "north-88-d1828".into(), question: "Upload the capture?".into(), options: vec!["encrypt it".into(), "keep it local".into()], recommend: 2, default: 0, default_after_ms: 0 };
    let gate = book.open_gate(&ask, opened_ms).unwrap();

    for (cited, why) in [(10_519_702, "not a human turn"), (10_519_703, "not a human turn"), (10_519_704, "not a human turn")] {
        let refused = tom_turn(&index, home, CONVERSATION, opened_ms, cited).unwrap_err().user_message();
        assert!(refused.contains(why), "turn {cited}: {refused}");
    }
    assert!(tom_turn(&index, home, "another-conversation", opened_ms, 10_519_701).unwrap_err().user_message().contains("not in this session's own conversation"));

    let turn = tom_turn(&index, home, CONVERSATION, opened_ms, 10_519_701).unwrap();
    assert_eq!(turn.text, "2. keep it local");
    let mut delivered = Vec::new();
    let closure = relay_gate(&book, gate, 2, &turn, "proxy", opened_ms + 7 * 3_600_000, |session, text| {
        delivered.push((session.to_owned(), text.to_owned()));
        Ok(())
    })
    .unwrap();
    assert_eq!(closure.by.label(), "tom");
    assert!(delivered[0].1.contains("Tom answered: keep it local\nrelayed by proxy from convo turn 10519701"), "{delivered:?}");
}
