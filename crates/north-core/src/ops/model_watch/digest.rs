use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use regex::Regex;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::xml;
use crate::foundation::clock::Timestamp;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS watch_candidates(
 id TEXT PRIMARY KEY, source TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL,
 detail TEXT NOT NULL CHECK(json_valid(detail)), ranked INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS watch_digests(
 id INTEGER PRIMARY KEY, ts TEXT NOT NULL, body TEXT NOT NULL, sent INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS watch_digest_items(
 digest INTEGER NOT NULL REFERENCES watch_digests(id), id TEXT NOT NULL,
 source TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL, why TEXT NOT NULL,
 rank REAL NOT NULL, PRIMARY KEY(digest,id)
) STRICT;
";

const ATOM: &str = "{http://www.w3.org/2005/Atom}";

pub(super) type Rows = BTreeMap<String, (String, String)>;

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

pub(super) fn feed_rows(parser: &str, payload: &[u8]) -> Result<Rows, String> {
    let mut raw: Vec<(String, String, Value)> = Vec::new();
    match parser {
        "hn-front-page" => {
            let data: Value = serde_json::from_slice(payload).map_err(|error| error.to_string())?;
            for item in data["hits"].as_array().ok_or("hits required")?.iter().take(100) {
                let key = match &item["objectID"] {
                    Value::String(text) => text.clone(),
                    other if !other.is_null() => other.to_string(),
                    _ => return Err("objectID required".into()),
                };
                let title = item["title"].as_str().ok_or("title required")?.to_owned();
                let url = item["url"].as_str().filter(|url| !url.is_empty()).map_or_else(|| format!("https://news.ycombinator.com/item?id={key}"), str::to_owned);
                raw.push((key, title, Value::String(url)));
            }
        }
        "feed" => {
            let root = xml::parse(payload)?;
            let mut items = root.descendants_named("item");
            if items.is_empty() {
                items = root.find_all(&format!("{ATOM}entry"));
            }
            for item in items {
                let title = non_empty(item.find_text("title")).or_else(|| non_empty(item.find_text(&format!("{ATOM}title")))).unwrap_or_default();
                let link = item.find(&format!("{ATOM}link"));
                let url = non_empty(item.find_text("link")).or_else(|| link.and_then(|link| link.attribute("href").map(str::to_owned))).unwrap_or_default();
                let key = non_empty(item.find_text("guid")).or_else(|| non_empty(item.find_text(&format!("{ATOM}id")))).unwrap_or_else(|| url.clone());
                if !title.is_empty() && !url.is_empty() && !key.is_empty() {
                    raw.push((key, title, Value::String(url)));
                }
            }
        }
        _ => return Err("unknown digest feed parser".into()),
    }
    let mut rows = Rows::new();
    for (key, title, url) in raw {
        let url = url.as_str().unwrap_or_default();
        if url.starts_with("http://") || url.starts_with("https://") {
            rows.insert(key, (truncate(&squash(&title), 300), url.to_owned()));
        }
    }
    Ok(rows)
}

pub(super) fn observe(db: &Connection, source: &str, rows: &Rows, ts: &str) -> Result<usize, String> {
    let name = format!("digest:{source}");
    let sql = |error: rusqlite::Error| error.to_string();
    let previous: BTreeSet<String> = db
        .prepare("SELECT key FROM watch_snapshot WHERE source=?")
        .and_then(|mut statement| statement.query_map([&name], |row| row.get(0))?.collect())
        .map_err(sql)?;
    let mut added = 0;
    db.execute_batch("BEGIN").map_err(sql)?;
    let outcome = (|| {
        for (key, (title, url)) in rows {
            if !previous.is_empty() && !previous.contains(key) {
                let digest = Sha256::digest(format!("{source}:{key}").as_bytes());
                let identity = format!("n{}", digest.iter().take(8).map(|byte| format!("{byte:02x}")).collect::<String>());
                let detail = json!({"title": title, "url": url}).to_string();
                added += db.execute("INSERT OR IGNORE INTO watch_candidates(id,source,title,url,detail) VALUES(?,?,?,?,?)", params![identity, source, title, url, detail])?;
            }
        }
        db.execute("DELETE FROM watch_snapshot WHERE source=?", [&name])?;
        db.execute("INSERT INTO watch_snapshot VALUES(?,?,?,?)", params![name, "@baseline", "{}", ts])?;
        for (key, (title, url)) in rows {
            db.execute("INSERT OR REPLACE INTO watch_snapshot VALUES(?,?,?,?)", params![name, key, json!({"title": title, "url": url}).to_string(), ts])?;
        }
        Ok::<(), rusqlite::Error>(())
    })();
    match outcome {
        Ok(()) => db.execute_batch("COMMIT").map_err(sql)?,
        Err(error) => {
            let _ = db.execute_batch("ROLLBACK");
            return Err(error.to_string());
        }
    }
    Ok(added)
}

pub(super) fn feedback_rows(path: &Path) -> Result<Vec<Value>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    text.lines().filter(|line| !line.trim().is_empty()).map(|line| serde_json::from_str(line).map_err(|error| error.to_string())).collect()
}

fn terms(text: &str) -> BTreeSet<String> {
    let pattern = Regex::new(r"[a-z0-9]{3,}").expect("term pattern");
    pattern.find_iter(&text.to_lowercase()).map(|found| found.as_str().to_owned()).collect()
}

pub(super) fn pre_score(source: &str, title: &str, feedback: &[Value]) -> f64 {
    let mine = terms(title);
    let mut latest: BTreeMap<&str, &Value> = BTreeMap::new();
    for row in feedback {
        if let Some(id) = row["id"].as_str() {
            latest.insert(id, row);
        }
    }
    let mut score = 0.0;
    for row in latest.values() {
        let shared = mine.intersection(&terms(row["title"].as_str().unwrap_or_default())).count();
        if source == row["source"].as_str().unwrap_or_default() && shared > 0 {
            let weight = if row["feedback"].as_str() == Some("noise") { -0.25 } else { 0.1 };
            score += weight * f64::min(1.0, shared as f64 / 2.0);
        }
    }
    score.clamp(-0.75, 0.3)
}

pub(super) struct Kept {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) url: String,
    pub(super) why: String,
}

pub(super) fn kept(db: &Connection, days: i64, now: Timestamp) -> Result<Vec<Kept>, String> {
    let sql = |error: rusqlite::Error| error.to_string();
    let last: Option<(i64, String)> = db
        .query_row("SELECT id,ts FROM watch_digests WHERE sent=1 ORDER BY id DESC LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?)))
        .map(Some)
        .or_else(|error| if matches!(error, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(error) })
        .map_err(sql)?;
    let Some((digest, ts)) = last else { return Ok(Vec::new()) };
    let sent = Timestamp::parse(&ts).map_err(|error| error.to_string())?;
    if (now.unix_seconds() - sent.unix_seconds()).div_euclid(86_400) >= days {
        return Ok(Vec::new());
    }
    db.prepare("SELECT id,title,url,why FROM watch_digest_items WHERE digest=? ORDER BY rank DESC,id")
        .and_then(|mut statement| {
            statement.query_map([digest], |row| Ok(Kept { id: row.get(0)?, title: row.get(1)?, url: row.get(2)?, why: row.get(3)? }))?.collect()
        })
        .map_err(sql)
}

pub(super) fn record_feedback(db: &Connection, identity: &str, label: &str, path: &Path, now: Timestamp) -> Result<(), String> {
    let item: Option<(String, String)> = db
        .query_row("SELECT source,title FROM watch_digest_items WHERE id=? ORDER BY digest DESC LIMIT 1", [identity], |row| Ok((row.get(0)?, row.get(1)?)))
        .map(Some)
        .or_else(|error| if matches!(error, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(error) })
        .map_err(|error| error.to_string())?;
    let (source, title) = item.ok_or("unknown digest item ID")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut stream = fs::OpenOptions::new().create(true).append(true).open(path).map_err(|error| error.to_string())?;
    let line = json!({"id": identity, "source": source, "title": title, "feedback": label, "ts": now.iso_micros_utc_offset()});
    writeln!(stream, "{line}").map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| error.to_string())
}
