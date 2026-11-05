use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::foundation::clock::Timestamp;
use crate::ledger::outcome::attention::Typed;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Label {
    Exact,
    Heuristic,
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn human(row: &Value) -> bool {
    row.get("origin").and_then(|origin| origin.get("kind")).and_then(Value::as_str) == Some("human")
}

fn label(row: &Value) -> Option<Label> {
    if human(row) && row["origin"].is_object() {
        return Some(Label::Exact);
    }
    let flagged = |key: &str| row.get(key).is_some_and(truthy);
    (row.get("origin").is_none_or(Value::is_null) && !flagged("isMeta") && !flagged("isCompactSummary")).then_some(Label::Heuristic)
}

type TextsByFile = BTreeMap<String, BTreeMap<i64, (i64, String, String)>>;

fn indexed_texts(index: &Path, home: &Path, start: &str, end: &str) -> rusqlite::Result<TextsByFile> {
    let db = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut query = db.prepare(
        "SELECT f.path,m.line,m.id,m.ts,b.text FROM files f JOIN msg m INDEXED BY msg_file ON m.file_id=f.id JOIN body b ON b.id=m.body \
         WHERE f.provider='anthropic' AND f.agent IS NULL AND f.cwd=? AND m.role='user' AND m.tool IS NULL AND m.dup=0 AND m.ts>=? AND m.ts<=?",
    )?;
    let mut grouped: TextsByFile = BTreeMap::new();
    let rows = query.query_map((home.to_string_lossy(), start, end), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)))?;
    for row in rows {
        let (path, line, id, at, text) = row?;
        grouped.entry(path).or_default().insert(line, (id, at, text));
    }
    Ok(grouped)
}

pub(super) fn typed_messages(index: &Path, home: &Path, start: i64, end: i64) -> Vec<Typed> {
    if !index.is_file() {
        return Vec::new();
    }
    let (start, end) = (Timestamp::from_unix_seconds(start).iso(), Timestamp::from_unix_seconds(end).iso());
    let Ok(grouped) = indexed_texts(index, home, &start, &end) else { return Vec::new() };
    let mut found: Vec<(Label, Typed)> = Vec::new();
    for (name, wanted) in &grouped {
        let (Ok(file), Some(last)) = (fs::File::open(name), wanted.keys().next_back().copied()) else { continue };
        for (number, line) in BufReader::new(file).split(b'\n').enumerate().take(usize::try_from(last).unwrap_or(0)) {
            let Some((id, at, text)) = wanted.get(&(number as i64 + 1)) else { continue };
            let Some(row) = line.ok().and_then(|line| serde_json::from_slice::<Value>(&line).ok()) else { continue };
            if let (Some(label), Some(at)) = (label(&row), Timestamp::parse_rfc3339(at)) {
                found.push((label, Typed { id: *id, session: name.clone(), at: at.unix_seconds(), text: text.clone() }));
            }
        }
    }
    let cutoff = found.iter().filter(|(label, _)| *label == Label::Exact).map(|(_, typed)| typed.at).min().unwrap_or(i64::MAX);
    let mut typed: Vec<Typed> = found.into_iter().filter(|(label, typed)| *label == Label::Exact || typed.at < cutoff).map(|(_, typed)| typed).collect();
    typed.sort_by_key(|typed| (typed.at, typed.id));
    typed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at(text: &str) -> i64 {
        Timestamp::parse_rfc3339(text).expect("fixture time").unix_seconds()
    }

    #[test]
    fn typed_messages_are_the_human_turns_of_toms_home_conversations_with_their_convo_ids_and_texts() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let transcript = dir.path().join("session.jsonl");
        let rows = [json!({"origin": {"kind": "human"}}), json!({"origin": {"kind": "agent"}}), json!({"origin": {"kind": "human"}}), json!({"isMeta": true})];
        fs::write(&transcript, rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        let index = dir.path().join("index-v4.db");
        let db = Connection::open(&index).unwrap();
        db.execute_batch(
            "CREATE TABLE files(id INTEGER,path TEXT,provider TEXT,agent TEXT,cwd TEXT,off INTEGER);\
             CREATE TABLE msg(id INTEGER,file_id INTEGER,line INTEGER,ts TEXT,role TEXT,tool TEXT,body INTEGER,dup INTEGER);CREATE INDEX msg_file ON msg(file_id,line);\
             CREATE TABLE body(id INTEGER,text TEXT);",
        )
        .unwrap();
        db.execute("INSERT INTO files VALUES(1,?,'anthropic',NULL,?,0)", (transcript.to_string_lossy(), home.to_string_lossy())).unwrap();
        db.execute("INSERT INTO files VALUES(2,?,'anthropic',NULL,'/elsewhere',0)", [transcript.to_string_lossy()]).unwrap();
        for (id, file, line, ts) in [(11, 1, 1, "2026-10-15T05:00:00Z"), (12, 1, 2, "2026-10-15T05:01:00Z"), (13, 1, 3, "2026-10-15T05:30:00Z"), (14, 1, 4, "2026-10-15T05:31:00Z"), (15, 2, 1, "2026-10-15T05:00:00Z")] {
            db.execute("INSERT INTO msg VALUES(?,?,?,?,'user',NULL,?,0)", (id, file, line, ts, id)).unwrap();
            db.execute("INSERT INTO body VALUES(?,?)", (id, format!("text {id}"))).unwrap();
        }
        drop(db);
        let (start, end) = (at("2026-10-15T00:00:00Z"), at("2026-10-16T00:00:00Z"));
        let found = typed_messages(&index, &home, start, end);
        let ids: Vec<(i64, i64, &str)> = found.iter().map(|typed| (typed.id, typed.at, typed.text.as_str())).collect();
        assert_eq!(ids, [(11, at("2026-10-15T05:00:00Z"), "text 11"), (13, at("2026-10-15T05:30:00Z"), "text 13")], "agent and meta rows and another directory's conversation are not Tom's");
        assert!(found.iter().all(|typed| typed.session == transcript.to_string_lossy()));
        assert!(typed_messages(&dir.path().join("missing.db"), &home, start, end).is_empty());
    }
}
