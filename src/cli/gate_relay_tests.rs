use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::tom_turn;
use crate::cli::fleet::typed_at;
use crate::fleet::gates::{Authority, GateBook, GateRequest, Relay, TypedTurn, answer_gate, relay_gate};
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
    let mut seen = std::collections::HashSet::new();
    for (number, row) in rows.iter().enumerate() {
        let id = 10_519_700 + number as i64;
        lines.push_str(&format!("{row}\n"));
        let text = row["message"]["content"].as_str().or_else(|| row["attachment"]["prompt"].as_str()).unwrap();
        db.execute("INSERT INTO body VALUES (?1, ?2)", (id, text)).unwrap();
        db.execute("INSERT INTO msg VALUES (?1, 1, ?2, ?3, 'user', NULL, ?1, ?4)", (id, number as i64 + 1, row["timestamp"].as_str().unwrap(), i64::from(!seen.insert(text.to_owned())))).unwrap();
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
        typed("2026-11-07T01:30:00Z", "2"),
        typed("2026-11-07T09:40:00Z", "2"),
    ];
    let index = convo(dir.path(), &rows);
    let home = Path::new("/home/tom");
    let opened_ms = 1_794_016_800_000;
    let book = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let ask = GateRequest { verification: false, issue: None, box_text: None, session: "north-88-d1828".into(), question: "Upload the capture?".into(), options: vec!["encrypt it".into(), "keep it local".into()], recommend: 2, default: 0, default_after_ms: 0 };
    let gate = book.open_gate(&ask, opened_ms).unwrap();

    for (cited, why) in [(10_519_702, "not typed by a person"), (10_519_703, "not typed by a person"), (10_519_704, "not typed by a person"), (10_519_799, "not in the convo store")] {
        let refused = tom_turn(&index, home, CONVERSATION, cited).unwrap_err().user_message();
        assert!(refused.contains(why), "turn {cited}: {refused}");
    }
    assert!(tom_turn(&index, home, "another-conversation", 10_519_701).unwrap_err().user_message().contains("not in this session's own conversation"));

    let turn = tom_turn(&index, home, CONVERSATION, 10_519_701).unwrap();
    assert_eq!(turn.text, "2. keep it local");
    let mut delivered = Vec::new();
    let closure = relay_gate(&book, gate, 2, Relay { turn: &turn, relayer: "proxy", parent: None }, opened_ms + 7 * 3_600_000, |session, text| {
        delivered.push((session.to_owned(), text.to_owned()));
        Ok(())
    })
    .unwrap();
    assert_eq!(closure.by.label(), "tom");
    assert!(delivered[0].1.contains("Tom answered: keep it local\nrelayed by proxy from convo turn 10519701"), "{delivered:?}");

    let bare = book.open_gate(&ask, opened_ms).unwrap();
    let relay = |number: usize, cited: i64| {
        let turn = tom_turn(&index, home, CONVERSATION, cited).unwrap();
        relay_gate(&book, bare, number, Relay { turn: &turn, relayer: "proxy", parent: None }, opened_ms + 7 * 3_600_000, |_, _| Ok(()))
    };
    assert!(relay(2, 10_519_705).unwrap_err().user_message().contains("typed before the gate opened"));
    assert!(relay(1, 10_519_706).unwrap_err().user_message().contains("names option 2, not 1"));
    assert_eq!(relay(2, 10_519_706).unwrap().by.label(), "tom");
}

#[test]
fn the_coordinator_window_closes_its_own_defaulted_gate_from_a_turn_tom_typed_after_it_opened() {
    let dir = tempfile::tempdir().unwrap();
    let queued = |at: &str, text: &str| json!({"type": "attachment", "timestamp": at, "attachment": {"type": "queued_command", "prompt": text, "origin": {"kind": "human"}}});
    let notice = json!({"type": "user", "origin": {"kind": "task-notification"}, "timestamp": "2026-11-09T17:06:00Z", "message": {"content": "<task-notification>\n<status>completed</status>\n</task-notification>"}});
    let rows = [
        typed("2026-11-09T16:50:00Z", "file the ladder question as a gate"),
        delivered("2026-11-09T17:02:00Z", "[from north-1071-a] Done: dispatch alerts page"),
        typed("2026-11-09T17:04:00Z", "1 on both"),
        queued("2026-11-09T17:05:00Z", "and keep going"),
        notice,
    ];
    let index = convo(dir.path(), &rows);
    let opened_ms = 1_794_243_600_000;
    let book = Ledger::open(&dir.path().join("ledger.db")).unwrap();
    let ask = |question: &str| GateRequest { verification: false, issue: None, box_text: None, session: "proxy".into(), question: question.into(), options: vec!["keep the ladder".into(), "route to Fable".into()], recommend: 2, default: 2, default_after_ms: 8 * 3_600_000 };
    let (direct, relayed) = (book.open_gate(&ask("Ladder?"), opened_ms).unwrap(), book.open_gate(&ask("Filter?"), opened_ms).unwrap());

    let at_ms = typed_at(&dir.path().join(format!("{CONVERSATION}.jsonl"))).expect("Tom's queued message opens the window's current turn");
    let typed = TypedTurn { conversation: CONVERSATION.into(), at_ms };
    let closure = answer_gate(&book, direct, 1, None, Authority { caller: Some("proxy"), parent: None, typed: Some(&typed) }, opened_ms + 600_000, |session, _| {
        assert_eq!(session, "proxy");
        Ok(())
    })
    .unwrap();
    assert_eq!(closure.by.label(), "tom");

    let turn = tom_turn(&index, Path::new("/home/tom"), CONVERSATION, 10_519_702).unwrap();
    let parent_refused = relay_gate(&book, relayed, 1, Relay { turn: &turn, relayer: "proxy", parent: Some("lead") }, opened_ms + 600_000, |_, _| panic!("a gate its asker's parent answers took a relay")).unwrap_err().user_message();
    assert!(parent_refused.contains("parent lead"), "{parent_refused}");
    let closure = relay_gate(&book, relayed, 1, Relay { turn: &turn, relayer: "proxy", parent: None }, opened_ms + 600_000, |_, text| {
        assert!(text.contains("Tom answered: keep the ladder\nrelayed by proxy from convo turn 10519702"), "{text}");
        Ok(())
    })
    .unwrap();
    assert_eq!(closure.by.label(), "tom");
}
