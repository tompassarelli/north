use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

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

const HUMAN: i64 = 1;
const ORIGIN: i64 = 2;
const INJECTED: i64 = 4;
const SDK: i64 = 8;

fn said(row: &Value) -> i64 {
    let origin = origin(row).filter(|origin| !origin.is_null());
    let human = origin.is_some_and(|origin| origin.is_object() && origin.get("kind").and_then(Value::as_str) == Some("human"));
    let flagged = |key: &str| row.get(key).is_some_and(truthy);
    let sdk = row.get("promptSource").and_then(Value::as_str) == Some("sdk");
    [(human, HUMAN), (origin.is_some(), ORIGIN), (flagged("isMeta") || flagged("isCompactSummary"), INJECTED), (sdk, SDK)].into_iter().filter(|(on, _)| *on).map(|(_, bit)| bit).sum()
}

fn label(said: i64, indexed: &Indexed, home: &str, reach: Reach) -> Option<Label> {
    let at_home = indexed.cwd == home;
    if said & HUMAN != 0 {
        return Some(Label::Exact);
    }
    if said & (ORIGIN | INJECTED) != 0 {
        return None;
    }
    let sent = said & SDK != 0 && !DELIVERED.iter().any(|prefix| indexed.text.trim_start().starts_with(prefix));
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
    attested(index, &grouped, home, reach).into_iter().next()
}

pub fn typed_messages(index: &Path, home: &Path, reach: Reach, start: i64, end: i64) -> Vec<Typed> {
    if !index.is_file() {
        return Vec::new();
    }
    let pick = Pick::Window { start: Timestamp::from_unix_seconds(start).iso(), end: Timestamp::from_unix_seconds(end).iso() };
    let Ok(grouped) = indexed_texts(index, &pick, false) else { return Vec::new() };
    attested(index, &grouped, home, reach)
}

const SEEN: &str = "typed-said.db";

type Seen = BTreeMap<i64, (i64, i64)>;

fn seen_store(index: &Path) -> Option<Connection> {
    let db = Connection::open(index.with_file_name(SEEN)).ok()?;
    db.busy_timeout(Duration::from_secs(5)).ok()?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS said(path TEXT NOT NULL, line INTEGER NOT NULL, said INTEGER NOT NULL, next INTEGER NOT NULL, PRIMARY KEY(path, line)) WITHOUT ROWID").ok()?;
    Some(db)
}

fn seen_in(store: Option<&Connection>, path: &str) -> Seen {
    let Some(mut query) = store.and_then(|db| db.prepare_cached("SELECT line, said, next FROM said WHERE path=?1").ok()) else { return Seen::new() };
    query.query_map([path], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?)))).map(|rows| rows.flatten().collect()).unwrap_or_default()
}

fn read_unseen(path: &str, wanted: &BTreeMap<i64, Indexed>, seen: &mut Seen) -> Vec<(i64, i64, i64)> {
    let unseen: Vec<i64> = wanted.keys().copied().filter(|line| !seen.contains_key(line)).collect();
    let (Some(&first), Some(&last)) = (unseen.first(), unseen.last()) else { return Vec::new() };
    let (start_line, start) = seen.range(..first).next_back().map_or((1, 0), |(line, (_, next))| (line + 1, *next));
    let Ok(mut file) = fs::File::open(path) else { return Vec::new() };
    if !u64::try_from(start).is_ok_and(|start| file.seek(SeekFrom::Start(start)).is_ok()) {
        return Vec::new();
    }
    let mut fresh = Vec::new();
    let mut offset = start;
    for (number, line) in (start_line..=last).zip(BufReader::new(file).split(b'\n')) {
        let Ok(line) = line else { break };
        offset = offset.saturating_add(i64::try_from(line.len() + 1).unwrap_or(i64::MAX));
        if wanted.contains_key(&number)
            && let Ok(row) = serde_json::from_slice::<Value>(&line)
        {
            let said = said(&row);
            seen.insert(number, (said, offset));
            fresh.push((number, said, offset));
        }
    }
    fresh
}

fn remember(store: Option<&Connection>, path: &str, fresh: &[(i64, i64, i64)]) {
    let Some(db) = store.filter(|_| !fresh.is_empty()) else { return };
    let Ok(transaction) = db.unchecked_transaction() else { return };
    if let Ok(mut insert) = transaction.prepare_cached("INSERT OR REPLACE INTO said VALUES (?1, ?2, ?3, ?4)") {
        for (line, said, next) in fresh {
            let _ = insert.execute((path, line, said, next));
        }
    }
    let _ = transaction.commit();
}

fn attested(index: &Path, grouped: &TextsByFile, home: &Path, reach: Reach) -> Vec<Typed> {
    let home = home.to_string_lossy();
    let store = seen_store(index);
    let mut found: Vec<(Label, Typed)> = Vec::new();
    for (name, wanted) in grouped {
        let mut seen = seen_in(store.as_ref(), name);
        let fresh = read_unseen(name, wanted, &mut seen);
        remember(store.as_ref(), name, &fresh);
        for (line, indexed) in wanted {
            let Some((said, _)) = seen.get(line) else { continue };
            if let (Some(label), Some(at)) = (label(*said, indexed, &home, reach), Timestamp::parse_rfc3339(&indexed.at)) {
                found.push((label, Typed { id: indexed.id, session: name.clone(), at: at.unix_seconds(), text: indexed.text.clone() }));
            }
        }
    }
    let cutoff = found.iter().filter(|(label, _)| *label == Label::Exact).map(|(_, typed)| typed.at).min().unwrap_or(i64::MAX);
    let mut typed: Vec<Typed> = found.into_iter().filter(|(label, typed)| *label != Label::Heuristic || typed.at < cutoff).map(|(_, typed)| typed).collect();
    typed.sort_by_key(|typed| (typed.at, typed.id));
    typed
}
