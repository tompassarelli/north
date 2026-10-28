use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, params, params_from_iter};

use super::blake::text_hash;
use super::failure::Result;
use super::records::Meta;

const SCHEMA: &str = "
CREATE TABLE meta(k TEXT PRIMARY KEY, v TEXT);
CREATE TABLE files(
  id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, dev INTEGER, ino INTEGER,
  size INTEGER NOT NULL DEFAULT 0, dsize INTEGER NOT NULL DEFAULT 0,
  zst INTEGER NOT NULL DEFAULT 0, mtime REAL, head TEXT,
  off INTEGER NOT NULL DEFAULT 0, nline INTEGER NOT NULL DEFAULT 0,
  provider TEXT, account TEXT, project TEXT, session_id TEXT, agent TEXT,
  title TEXT, cwd TEXT, branch TEXT, first_ts TEXT, last_ts TEXT,
  nmsg INTEGER NOT NULL DEFAULT 0);
CREATE INDEX files_session ON files(session_id);
CREATE TABLE msg(
  id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL, line INTEGER NOT NULL,
  ts TEXT, role TEXT, tool TEXT, body INTEGER NOT NULL,
  dup INTEGER NOT NULL DEFAULT 0);
CREATE INDEX msg_file ON msg(file_id, line);
CREATE INDEX msg_ts ON msg(ts);
CREATE INDEX msg_body ON msg(body);
CREATE TABLE body(id INTEGER PRIMARY KEY, h INTEGER NOT NULL UNIQUE,
  first INTEGER NOT NULL, text TEXT NOT NULL);
CREATE VIRTUAL TABLE ftx USING fts5(text, content='', contentless_delete=1,
  tokenize='porter unicode61 remove_diacritics 2');
";

pub(super) struct Pending {
    pub line: i64,
    pub ts: Option<String>,
    pub role: String,
    pub tool: Option<String>,
    pub text: String,
}

pub(super) fn connect(db: &Path, state: &Path, readonly: bool) -> Result<Connection> {
    let con = if readonly {
        Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?
    } else {
        std::fs::create_dir_all(state)?;
        let con = Connection::open(db)?;
        let present: Option<i64> = con.query_row("SELECT 1 FROM sqlite_master WHERE name = 'ftx'", [], |row| row.get(0)).ok();
        if present.is_none() {
            con.execute_batch(&format!("BEGIN;{SCHEMA}COMMIT;"))?;
        }
        con.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        con.execute_batch("PRAGMA synchronous=NORMAL")?;
        con
    };
    con.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA cache_size=-64000")?;
    Ok(con)
}

pub(super) fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

/// Drops a file's messages; texts it introduced move to their next occurrence.
pub(super) fn forget(con: &Connection, file: i64) -> Result<()> {
    let firsts: Vec<(i64, i64)> = con
        .prepare("SELECT m.id, m.body FROM msg m JOIN body b ON b.id = m.body AND b.first = m.id WHERE m.file_id = ?")?
        .query_map([file], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (id, _) in &firsts {
        con.execute("DELETE FROM ftx WHERE rowid = ?", [id])?;
    }
    con.execute("DELETE FROM msg WHERE file_id = ?", [file])?;
    for (_, body) in firsts {
        let next: Option<i64> = con.query_row("SELECT MIN(id) FROM msg WHERE body = ?", [body], |row| row.get(0))?;
        match next {
            None => {
                con.execute("DELETE FROM body WHERE id = ?", [body])?;
            }
            Some(next) => {
                con.execute("UPDATE body SET first = ? WHERE id = ?", [next, body])?;
                con.execute("UPDATE msg SET dup = 0 WHERE id = ?", [next])?;
                con.execute("INSERT INTO ftx(rowid, text) SELECT ?, text FROM body WHERE id = ?", [next, body])?;
            }
        }
    }
    con.execute("UPDATE files SET off = 0, nline = 0, nmsg = 0 WHERE id = ?", [file])?;
    Ok(())
}

/// Inserts pending rows; each distinct text is indexed once, at its first occurrence.
pub(super) fn store(con: &Connection, file: i64, pending: &[Pending], mut next_id: i64) -> Result<i64> {
    let hashes: Vec<i64> = pending.iter().map(|row| text_hash(&row.text)).collect();
    let mut known = std::collections::HashMap::new();
    let mut unique = hashes.clone();
    unique.sort_unstable();
    unique.dedup();
    for part in unique.chunks(500) {
        let mut statement = con.prepare(&format!("SELECT h, id FROM body WHERE h IN ({})", placeholders(part.len())))?;
        for row in statement.query_map(params_from_iter(part), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))? {
            let (hash, id) = row?;
            known.insert(hash, id);
        }
    }
    let mut message = con.prepare_cached("INSERT INTO msg(id, file_id, line, ts, role, tool, body, dup) VALUES(?,?,?,?,?,?,?,?)")?;
    let mut body = con.prepare_cached("INSERT INTO body(id, h, first, text) VALUES(?,?,?,?)")?;
    let mut fts = con.prepare_cached("INSERT INTO ftx(rowid, text) VALUES(?,?)")?;
    for (row, hash) in pending.iter().zip(hashes) {
        let id = next_id;
        next_id += 1;
        let known_body = known.get(&hash).copied();
        let body_id = known_body.unwrap_or(id);
        message.execute(params![id, file, row.line, row.ts, row.role, row.tool, body_id, i64::from(body_id != id)])?;
        if known_body.is_none() {
            known.insert(hash, id);
            body.execute(params![id, hash, id, row.text])?;
            fts.execute(params![id, row.text])?;
        }
    }
    con.execute("INSERT OR REPLACE INTO meta(k, v) VALUES('next_id', ?)", [next_id.to_string()])?;
    Ok(next_id)
}

pub(super) struct Checkpoint<'a> {
    pub file: i64,
    pub off: u64,
    pub line: i64,
    pub meta: &'a Meta,
    pub hint: &'a str,
    pub first_ts: &'a Option<String>,
    pub last_ts: &'a Option<String>,
    pub count: usize,
}

pub(super) fn slug(cwd: Option<&str>) -> String {
    cwd.map_or_else(String::new, |cwd| cwd.replace('/', "-"))
}

/// Checkpoints a file's offset with the metadata and count read up to it.
pub(super) fn save(con: &Connection, at: &Checkpoint) -> Result<()> {
    let meta = at.meta;
    let project = Some(slug(meta.cwd.as_deref())).filter(|slug| !slug.is_empty()).unwrap_or_else(|| at.hint.to_owned());
    con.execute(
        "UPDATE files SET off=?, nline=?, session_id=COALESCE(?, session_id), agent=COALESCE(?, agent), \
         title=COALESCE(?, title, ?), cwd=COALESCE(?, cwd), branch=COALESCE(?, branch), \
         project=COALESCE(NULLIF(?, ''), project), first_ts=COALESCE(first_ts, ?), \
         last_ts=COALESCE(?, last_ts), nmsg=nmsg+? WHERE id=?",
        params_from_iter([
            Value::Integer(at.off as i64),
            Value::Integer(at.line),
            opt(&meta.session_id),
            opt(&meta.agent),
            opt(&meta.title),
            opt(&meta.slug),
            opt(&meta.cwd),
            opt(&meta.branch),
            Value::Text(project),
            opt(at.first_ts),
            opt(at.last_ts),
            Value::Integer(at.count as i64),
            Value::Integer(at.file),
        ]),
    )?;
    Ok(())
}

fn opt(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::Text)
}
