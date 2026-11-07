use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde_json::json;

use super::typed::{Reach, typed_messages};

fn index(dir: &Path, rows: &[(&str, &str)]) -> std::path::PathBuf {
    let path = dir.join("index.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT, provider TEXT, agent TEXT, cwd TEXT);
         CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INTEGER, line INTEGER, ts TEXT, role TEXT, tool TEXT, body INTEGER, dup INTEGER DEFAULT 0);
         CREATE INDEX msg_file ON msg(file_id, line);
         CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT);",
    )
    .unwrap();
    let transcript = dir.join("proxy.jsonl");
    let mut lines = String::new();
    db.execute("INSERT INTO files VALUES (1, ?1, 'anthropic', NULL, '/work/north')", [transcript.to_string_lossy()]).unwrap();
    for (number, (text, stamp)) in rows.iter().enumerate() {
        let row = json!({"type": "user", "promptSource": "sdk", "timestamp": stamp, "message": {"content": text}});
        lines.push_str(&format!("{row}\n"));
        db.execute("INSERT INTO body VALUES (?1, ?2)", (number as i64, text)).unwrap();
        db.execute("INSERT INTO msg VALUES (?1, 1, ?2, ?3, 'user', NULL, ?1, 0)", (number as i64, number as i64 + 1, stamp)).unwrap();
    }
    fs::write(transcript, lines).unwrap();
    path
}

#[test]
fn what_tom_types_in_a_fleet_hosted_conversation_counts_but_deliveries_and_the_opening_prompt_do_not() {
    let dir = tempfile::tempdir().unwrap();
    let rows = [
        ("You are the proxy for this fleet.", "2026-11-07T04:00:00Z"),
        ("stop the paid pass now", "2026-11-07T04:10:00Z"),
        ("[from service:north-fleet] landed abc", "2026-11-07T04:11:00Z"),
        ("[gate 4] Tom answered: yes", "2026-11-07T04:12:00Z"),
        ("fix the harness gap again", "2026-11-07T04:20:00Z"),
    ];
    let index = index(dir.path(), &rows);
    let home = Path::new("/home/tom");
    let texts = |reach| typed_messages(&index, home, reach, 1_700_000_000, 2_000_000_000).into_iter().map(|typed| typed.text).collect::<Vec<_>>();
    assert_eq!(texts(Reach::Fleet), ["stop the paid pass now", "fix the harness gap again"]);
    assert_eq!(texts(Reach::Human), texts(Reach::Fleet), "a fleet prompt Tom sent with no sender prefix is attested in any cwd");
}
