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
    Fleet,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Reach {
    Human,
    Fleet,
}

const DELIVERED: [&str; 6] = ["[from ", "[gate ", "[urgent message", "[Request interrupted", "Stop hook feedback", "<"];

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

fn origin(row: &Value) -> Option<&Value> {
    row.get("origin").filter(|origin| !origin.is_null()).or_else(|| row.get("attachment").and_then(|attachment| attachment.get("origin")))
}

struct Indexed {
    id: i64,
    at: String,
    text: String,
    cwd: String,
    opening: bool,
}

fn label(row: &Value, indexed: &Indexed, home: &str, reach: Reach) -> Option<Label> {
    let at_home = indexed.cwd == home;
    let origin = origin(row).filter(|origin| !origin.is_null());
    if origin.is_some_and(|origin| origin.is_object() && origin.get("kind").and_then(Value::as_str) == Some("human")) {
        return (at_home || reach == Reach::Fleet).then_some(Label::Exact);
    }
    let flagged = |key: &str| row.get(key).is_some_and(truthy);
    if origin.is_some() || flagged("isMeta") || flagged("isCompactSummary") {
        return None;
    }
    let sent = row.get("promptSource").and_then(Value::as_str) == Some("sdk") && !DELIVERED.iter().any(|prefix| indexed.text.trim_start().starts_with(prefix));
    if reach == Reach::Fleet && sent && !indexed.opening {
        return Some(Label::Fleet);
    }
    at_home.then_some(Label::Heuristic)
}

type TextsByFile = BTreeMap<String, BTreeMap<i64, Indexed>>;

fn indexed_texts(index: &Path, home: &Path, reach: Reach, start: &str, end: &str) -> rusqlite::Result<TextsByFile> {
    let db = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut query = db.prepare(
        "SELECT f.path,f.cwd,m.line,m.id,m.ts,b.text,m.line=(SELECT MIN(first.line) FROM msg first WHERE first.file_id=f.id AND first.role='user' AND first.tool IS NULL AND first.dup=0) \
         FROM files f JOIN msg m INDEXED BY msg_file ON m.file_id=f.id JOIN body b ON b.id=m.body \
         WHERE f.provider='anthropic' AND f.agent IS NULL AND (f.cwd=?1 OR ?2) AND m.role='user' AND m.tool IS NULL AND m.dup=0 AND m.ts>=?3 AND m.ts<=?4",
    )?;
    let mut grouped: TextsByFile = BTreeMap::new();
    let rows = query.query_map((home.to_string_lossy(), reach == Reach::Fleet, start, end), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(2)?, Indexed { cwd: row.get(1)?, id: row.get(3)?, at: row.get(4)?, text: row.get(5)?, opening: row.get(6)? }))
    })?;
    for row in rows {
        let (path, line, indexed) = row?;
        grouped.entry(path).or_default().insert(line, indexed);
    }
    Ok(grouped)
}

pub fn typed_messages(index: &Path, home: &Path, reach: Reach, start: i64, end: i64) -> Vec<Typed> {
    if !index.is_file() {
        return Vec::new();
    }
    let (start, end) = (Timestamp::from_unix_seconds(start).iso(), Timestamp::from_unix_seconds(end).iso());
    let Ok(grouped) = indexed_texts(index, home, reach, &start, &end) else { return Vec::new() };
    let home = home.to_string_lossy();
    let mut found: Vec<(Label, Typed)> = Vec::new();
    for (name, wanted) in &grouped {
        let (Ok(file), Some(last)) = (fs::File::open(name), wanted.keys().next_back().copied()) else { continue };
        for (number, line) in BufReader::new(file).split(b'\n').enumerate().take(usize::try_from(last).unwrap_or(0)) {
            let Some(indexed) = wanted.get(&(number as i64 + 1)) else { continue };
            let Some(row) = line.ok().and_then(|line| serde_json::from_slice::<Value>(&line).ok()) else { continue };
            if let (Some(label), Some(at)) = (label(&row, indexed, &home, reach), Timestamp::parse_rfc3339(&indexed.at)) {
                found.push((label, Typed { id: indexed.id, session: name.clone(), at: at.unix_seconds(), text: indexed.text.clone() }));
            }
        }
    }
    let cutoff = found.iter().filter(|(label, _)| *label == Label::Exact).map(|(_, typed)| typed.at).min().unwrap_or(i64::MAX);
    let mut typed: Vec<Typed> = found.into_iter().filter(|(label, typed)| *label != Label::Heuristic || typed.at < cutoff).map(|(_, typed)| typed).collect();
    typed.sort_by_key(|typed| (typed.at, typed.id));
    typed
}
