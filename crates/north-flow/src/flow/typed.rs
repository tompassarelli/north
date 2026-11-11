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

/// Human takes only turns attested as a person's: a CLI turn marked human-origin, or a fleet prompt sent with no sender prefix
/// after the opening, in any cwd. Fleet also takes the unmarked turns of conversations opened in the home directory.
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
        return Some(Label::Exact);
    }
    let flagged = |key: &str| row.get(key).is_some_and(truthy);
    if origin.is_some() || flagged("isMeta") || flagged("isCompactSummary") {
        return None;
    }
    let sent = row.get("promptSource").and_then(Value::as_str) == Some("sdk") && !DELIVERED.iter().any(|prefix| indexed.text.trim_start().starts_with(prefix));
    if sent && !indexed.opening {
        return Some(Label::Fleet);
    }
    (at_home && reach == Reach::Fleet).then_some(Label::Heuristic)
}

type TextsByFile = BTreeMap<String, BTreeMap<i64, Indexed>>;

enum Pick {
    Window { start: String, end: String },
    Turn(i64),
}

fn indexed_texts(index: &Path, pick: &Pick, repeats: bool) -> rusqlite::Result<TextsByFile> {
    let db = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let (start, end, turn, from) = match pick {
        Pick::Window { start, end } => (start.as_str(), end.as_str(), None, "files f JOIN msg m INDEXED BY msg_file ON m.file_id=f.id"),
        Pick::Turn(id) => ("", "9999", Some(*id), "msg m JOIN files f ON m.file_id=f.id"),
    };
    let mut query = db.prepare(&format!(
        "SELECT f.path,f.cwd,m.line,m.id,m.ts,b.text,m.line=(SELECT MIN(first.line) FROM msg first WHERE first.file_id=f.id AND first.role IN ('user','sent') AND first.tool IS NULL AND (?4 OR first.dup=0)) \
         FROM {from} JOIN body b ON b.id=m.body \
         WHERE f.provider='anthropic' AND f.agent IS NULL AND m.role IN ('user','sent') AND m.tool IS NULL AND (?4 OR m.dup=0) AND m.ts>=?1 AND m.ts<=?2 AND (?3 IS NULL OR m.id=?3)"
    ))?;
    let mut grouped: TextsByFile = BTreeMap::new();
    let rows = query.query_map((start, end, turn, repeats), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(2)?, Indexed { cwd: row.get(1)?, id: row.get(3)?, at: row.get(4)?, text: row.get(5)?, opening: row.get(6)? }))
    })?;
    for row in rows {
        let (path, line, indexed) = row?;
        grouped.entry(path).or_default().insert(line, indexed);
    }
    Ok(grouped)
}

pub fn user_turn_stored(index: &Path, id: i64) -> bool {
    let Ok(db) = Connection::open_with_flags(index, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return false };
    db.query_row("SELECT 1 FROM msg WHERE id=?1 AND role IN ('user','sent') AND tool IS NULL", [id], |_| Ok(())).is_ok()
}

/// The typed turn with this id, found even when its text was typed before: a repeat such as a bare option number is the answer itself.
pub fn typed_turn(index: &Path, home: &Path, reach: Reach, id: i64) -> Option<Typed> {
    if !index.is_file() {
        return None;
    }
    let grouped = indexed_texts(index, &Pick::Turn(id), true).ok()?;
    attested(&grouped, home, reach).into_iter().next()
}

pub fn typed_messages(index: &Path, home: &Path, reach: Reach, start: i64, end: i64) -> Vec<Typed> {
    if !index.is_file() {
        return Vec::new();
    }
    let pick = Pick::Window { start: Timestamp::from_unix_seconds(start).iso(), end: Timestamp::from_unix_seconds(end).iso() };
    let Ok(grouped) = indexed_texts(index, &pick, false) else { return Vec::new() };
    attested(&grouped, home, reach)
}

fn attested(grouped: &TextsByFile, home: &Path, reach: Reach) -> Vec<Typed> {
    let home = home.to_string_lossy();
    let mut found: Vec<(Label, Typed)> = Vec::new();
    for (name, wanted) in grouped {
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
