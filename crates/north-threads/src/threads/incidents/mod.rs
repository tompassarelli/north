//! `north incident record|guard|import|report`: one row per failure the moment a lander verdict, a
//! withdrawal or a detector finding names it, the guard that closes each failure class, and the
//! weekly reading `north flow` prints. The tables are created here, on first write; the reader in
//! `report` tolerates a ledger that has none yet.
pub mod corrections;
pub mod report;

use std::io::{Read, Write};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

use super::verb::{Args, Context, Failure, Outcome, immediate, open};
use crate::foundation::clock::Timestamp;

pub const VERBS: [&str; 1] = ["incident"];

pub const HUMANS: [&str; 2] = ["proxy", "tom"];

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS incidents(
  id INTEGER PRIMARY KEY,
  key TEXT NOT NULL UNIQUE CHECK(length(key) > 0),
  class TEXT NOT NULL CHECK(length(class) > 0),
  noticed_by TEXT NOT NULL CHECK(length(noticed_by) > 0),
  repo TEXT NOT NULL DEFAULT '',
  subject TEXT NOT NULL DEFAULT '',
  started_at TEXT,
  detected_at TEXT NOT NULL,
  fixed_at TEXT,
  guard TEXT NOT NULL DEFAULT '',
  recurred_after_guard INTEGER NOT NULL DEFAULT 0 CHECK(recurred_after_guard IN (0, 1)),
  detail TEXT NOT NULL DEFAULT ''
) STRICT;
CREATE INDEX IF NOT EXISTS incidents_detected ON incidents(detected_at);
CREATE TABLE IF NOT EXISTS incident_guards(
  class TEXT PRIMARY KEY CHECK(length(class) > 0),
  guard TEXT NOT NULL CHECK(length(guard) > 0),
  landed_at TEXT NOT NULL,
  issue TEXT NOT NULL DEFAULT ''
) STRICT;";

const USAGE: &str = "usage: north incident record --key K --class C --noticed-by WHO [--repo R] [--subject S] [--started-at EPOCH] [--detected-at EPOCH] [--detail TEXT]\n       north incident guard --class C --guard REF --landed-at EPOCH [--issue REPO#N]\n       north incident import < ROWS.jsonl\n       north incident report [--days N] [--since EPOCH --until EPOCH]";

pub struct Incident {
    pub key: String,
    pub class: String,
    pub noticed_by: String,
    pub repo: String,
    pub subject: String,
    pub started_at: Option<String>,
    pub detected_at: String,
    pub detail: String,
}

pub struct Guard {
    pub class: String,
    pub guard: String,
    pub landed_at: String,
    pub issue: String,
}

#[derive(Deserialize)]
struct Imported {
    key: String,
    class: String,
    noticed_by: String,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    subject: String,
    started_at: Option<String>,
    detected_at: String,
    #[serde(default)]
    detail: String,
    guard: Option<String>,
    guard_landed_at: Option<String>,
    #[serde(default)]
    issue: String,
}

pub fn run(argv: &[String], context: &Context, stdin: &mut dyn Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = match argv.get(1).map(String::as_str) {
        Some("record") => record(&argv[2..], context),
        Some("guard") => guard(&argv[2..], context),
        Some("import") => import(context, stdin),
        Some("report") => report::verb(&argv[2..], context),
        _ => Err(Failure::Usage(USAGE.into())),
    };
    super::verb::report(&argv[0], result, out, err)
}

fn time(args: &Args, flag: &str) -> Result<Option<String>, Failure> {
    let Some(text) = args.get(flag) else { return Ok(None) };
    let seconds: f64 = text.parse().map_err(|_| Failure::Usage(format!("{flag} {text} is not epoch seconds")))?;
    Ok(Some(Timestamp::from_unix_seconds_f64(seconds).iso()))
}

fn iso(text: &str, field: &str) -> Result<String, Failure> {
    Timestamp::parse_python_datetime(text).map(Timestamp::iso).ok_or_else(|| Failure::Usage(format!("{field} {text} is not an ISO time")))
}

fn required(args: &Args, flag: &str) -> Result<String, Failure> {
    args.get(flag).filter(|value| !value.is_empty()).map(str::to_owned).ok_or_else(|| Failure::Usage(format!("{flag} is required")))
}

fn write(context: &Context, statement: impl FnOnce(&Connection) -> Result<String, Failure>) -> Outcome {
    if context.replica {
        return Err(Failure::Refused("this ledger is a replica; incidents are written on the ledger host".into()));
    }
    let mut db = open(context.database, true)?;
    let tx = immediate(&mut db)?;
    tx.execute_batch(SCHEMA)?;
    let text = statement(&tx)?;
    tx.commit()?;
    Ok(text)
}

/// Inserts one incident once by key, naming the class's guard when it had landed before the
/// incident was detected, which marks the row a recurrence after the guard.
pub fn insert(db: &Connection, incident: &Incident) -> rusqlite::Result<bool> {
    let landed: Option<(String, String)> = db
        .query_row("SELECT guard, landed_at FROM incident_guards WHERE class = ?1", [&incident.class], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional()?;
    let held = landed.filter(|(_, landed_at)| *landed_at <= incident.detected_at);
    let inserted = db.execute(
        "INSERT INTO incidents(key, class, noticed_by, repo, subject, started_at, detected_at, guard, recurred_after_guard, detail) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(key) DO NOTHING",
        params![
            incident.key,
            incident.class,
            incident.noticed_by,
            incident.repo,
            incident.subject,
            incident.started_at,
            incident.detected_at,
            held.as_ref().map_or("", |(guard, _)| guard.as_str()),
            i64::from(held.is_some()),
            incident.detail
        ],
    )?;
    Ok(inserted == 1)
}

/// Creates the tables if they are missing, then inserts the incident once by key.
pub fn record_once(db: &Connection, incident: &Incident) -> rusqlite::Result<bool> {
    db.execute_batch(SCHEMA)?;
    insert(db, incident)
}

/// Names the guard that closes a class and marks every open earlier incident of it fixed then.
pub fn set_guard(db: &Connection, guard: &Guard) -> rusqlite::Result<usize> {
    db.execute(
        "INSERT INTO incident_guards(class, guard, landed_at, issue) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(class) DO UPDATE SET guard = excluded.guard, landed_at = excluded.landed_at, issue = excluded.issue",
        params![guard.class, guard.guard, guard.landed_at, guard.issue],
    )?;
    db.execute(
        "UPDATE incidents SET fixed_at = ?1, guard = ?2 WHERE class = ?3 AND fixed_at IS NULL AND detected_at <= ?1",
        params![guard.landed_at, guard.guard, guard.class],
    )
}

fn record(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--key", "--class", "--noticed-by", "--repo", "--subject", "--started-at", "--detected-at", "--detail"], &[])?;
    let incident = Incident {
        key: required(&args, "--key")?,
        class: required(&args, "--class")?,
        noticed_by: required(&args, "--noticed-by")?,
        repo: args.get("--repo").unwrap_or_default().to_owned(),
        subject: args.get("--subject").unwrap_or_default().to_owned(),
        started_at: time(&args, "--started-at")?,
        detected_at: time(&args, "--detected-at")?.unwrap_or_else(|| context.now.iso()),
        detail: args.get("--detail").unwrap_or_default().to_owned(),
    };
    write(context, |db| Ok(if insert(db, &incident)? { format!("incident {}: recorded\n", incident.key) } else { format!("incident {}: already recorded\n", incident.key) }))
}

fn guard(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--class", "--guard", "--landed-at", "--issue"], &[])?;
    let guard = Guard {
        class: required(&args, "--class")?,
        guard: required(&args, "--guard")?,
        landed_at: time(&args, "--landed-at")?.ok_or_else(|| Failure::Usage("--landed-at is required".into()))?,
        issue: args.get("--issue").unwrap_or_default().to_owned(),
    };
    write(context, |db| Ok(format!("incident guard {} for {}: {} open incidents fixed\n", guard.guard, guard.class, set_guard(db, &guard)?)))
}

fn import(context: &Context, stdin: &mut dyn Read) -> Outcome {
    let mut text = String::new();
    stdin.read_to_string(&mut text).map_err(|error| Failure::Refused(error.to_string()))?;
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate().filter(|(_, line)| !line.trim().is_empty()) {
        let row: Imported = serde_json::from_str(line).map_err(|error| Failure::Usage(format!("line {}: {error}", number + 1)))?;
        rows.push(row);
    }
    write(context, |db| {
        let (mut inserted, mut guards) = (0, 0);
        for row in &rows {
            let incident = Incident {
                key: row.key.clone(),
                class: row.class.clone(),
                noticed_by: row.noticed_by.clone(),
                repo: row.repo.clone(),
                subject: row.subject.clone(),
                started_at: row.started_at.as_deref().map(|text| iso(text, "started_at")).transpose()?,
                detected_at: iso(&row.detected_at, "detected_at")?,
                detail: row.detail.clone(),
            };
            inserted += usize::from(insert(db, &incident)?);
            if let (Some(guard), Some(landed_at)) = (&row.guard, &row.guard_landed_at) {
                set_guard(db, &Guard { class: row.class.clone(), guard: guard.clone(), landed_at: iso(landed_at, "guard_landed_at")?, issue: row.issue.clone() })?;
                guards += 1;
            }
        }
        Ok(format!("incident import: {inserted} of {} rows recorded, {guards} guards set\n", rows.len()))
    })
}
