//! The incident reading over a window: incidents by class, how many a detector noticed before a
//! person did, the median minutes to detect and to fix, and the classes that recurred after their
//! guard landed.
use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::HUMANS;
use crate::foundation::clock::Timestamp;
use crate::threads::verb::{Args, Context, Failure, Outcome, open};

pub const WEEK: f64 = 7.0 * 86_400.0;

pub struct Recurrence {
    pub class: String,
    pub guard: String,
    pub issue: String,
    pub key: String,
    pub detected_at: String,
    pub detail: String,
}

#[derive(Default)]
pub struct Reading {
    pub label: String,
    pub total: usize,
    pub by_class: BTreeMap<String, usize>,
    pub detector_first: usize,
    pub detect_minutes: Vec<f64>,
    pub fix_minutes: Vec<f64>,
    pub recurred: Vec<Recurrence>,
}

fn seconds(text: &str) -> Option<f64> {
    Timestamp::parse_python_datetime(text).map(Timestamp::unix_seconds_f64)
}

fn median(samples: &[f64]) -> Option<f64> {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let length = sorted.len();
    match length {
        0 => None,
        _ if length % 2 == 1 => Some(sorted[length / 2]),
        _ => Some(f64::midpoint(sorted[length / 2 - 1], sorted[length / 2])),
    }
}

fn present(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row("SELECT count(*) = 2 FROM sqlite_master WHERE type = 'table' AND name IN ('incidents', 'incident_guards')", [], |row| row.get(0))
}

/// The reading of every incident detected in `[since, until)`; none when the ledger has no incidents table.
pub fn read(db: &Connection, since: f64, until: f64, label: &str) -> rusqlite::Result<Option<Reading>> {
    if !present(db)? {
        return Ok(None);
    }
    let (from, to) = (Timestamp::from_unix_seconds_f64(since).iso(), Timestamp::from_unix_seconds_f64(until).iso());
    let mut statement = db.prepare(
        "SELECT i.key, i.class, i.noticed_by, i.started_at, i.detected_at, i.fixed_at, i.guard, i.recurred_after_guard, i.detail, coalesce(g.issue, '') \
         FROM incidents i LEFT JOIN incident_guards g ON g.class = i.class WHERE i.detected_at >= ?1 AND i.detected_at < ?2 ORDER BY i.detected_at, i.id",
    )?;
    let mut reading = Reading { label: label.to_owned(), ..Reading::default() };
    let rows = statement.query_map([&from, &to], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
        ))
    })?;
    for row in rows {
        let (key, class, noticed_by, started_at, detected_at, fixed_at, guard, recurred, detail, issue) = row?;
        reading.total += 1;
        *reading.by_class.entry(class.clone()).or_default() += 1;
        reading.detector_first += usize::from(!HUMANS.contains(&noticed_by.as_str()));
        let detected = seconds(&detected_at);
        if let (Some(started), Some(detected)) = (started_at.as_deref().and_then(seconds), detected) {
            reading.detect_minutes.push((detected - started).max(0.0) / 60.0);
        }
        if let (Some(fixed), Some(detected)) = (fixed_at.as_deref().and_then(seconds), detected) {
            reading.fix_minutes.push((fixed - detected).max(0.0) / 60.0);
        }
        if recurred == 1 {
            reading.recurred.push(Recurrence { class, guard, issue, key, detected_at, detail });
        }
    }
    Ok(Some(reading))
}

/// The reading of the seven days before `now` in the ledger at `ledger`.
pub fn week(ledger: &Path, now: f64) -> Option<Reading> {
    let db = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    read(&db, now - WEEK, now + 1.0, "7d").ok().flatten()
}

fn minutes(samples: &[f64]) -> String {
    median(samples).map_or_else(|| "n/a (n=0)".into(), |minutes| format!("{minutes:.0} min (n={})", samples.len()))
}

impl Reading {
    /// The lines `north flow` and `north incident report` print.
    pub fn lines(&self) -> Vec<String> {
        let label = &self.label;
        let share = if self.total == 0 { "n/a".into() } else { format!("{:.0}%", 100.0 * self.detector_first as f64 / self.total as f64) };
        let classes = if self.by_class.is_empty() { "none".into() } else { self.by_class.iter().map(|(class, count)| format!("{class} {count}")).collect::<Vec<_>>().join(", ") };
        let mut lines = vec![
            format!("incidents {label}: {} by class: {classes}", self.total),
            format!("incidents {label}: detector-first {} of {} ({share}); median to detect {}, to fix {}", self.detector_first, self.total, minutes(&self.detect_minutes), minutes(&self.fix_minutes)),
        ];
        let mut recurred: BTreeMap<&str, (&str, usize)> = BTreeMap::new();
        for recurrence in &self.recurred {
            recurred.entry(&recurrence.class).or_insert((&recurrence.guard, 0)).1 += 1;
        }
        lines.push(if recurred.is_empty() {
            format!("incidents {label}: no class recurred after its guard landed")
        } else {
            format!("incidents {label}: recurred after their guard landed: {}", recurred.iter().map(|(class, (guard, count))| format!("{class} {count} (guard {guard})")).collect::<Vec<_>>().join(", "))
        });
        lines
    }
}

fn epoch(args: &Args, flag: &str) -> Result<Option<f64>, Failure> {
    args.get(flag).map(|text| text.parse::<f64>().map_err(|_| Failure::Usage(format!("{flag} {text} is not epoch seconds")))).transpose()
}

pub fn verb(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--days", "--since", "--until"], &[])?;
    let now = context.now.unix_seconds_f64();
    let (since, until, label) = match (epoch(&args, "--since")?, epoch(&args, "--until")?) {
        (Some(since), Some(until)) => (since, until, format!("{} to {}", Timestamp::from_unix_seconds_f64(since).iso(), Timestamp::from_unix_seconds_f64(until).iso())),
        (None, None) => {
            let days: f64 = args.get("--days").map_or(Ok(7.0), |text| text.parse().map_err(|_| Failure::Usage(format!("--days {text} is not a number"))))?;
            (now - days * 86_400.0, now + 1.0, format!("{days}d"))
        }
        _ => return Err(Failure::Usage("--since and --until go together".into())),
    };
    let db = open(context.database, false)?;
    let reading = read(&db, since, until, &label)?.unwrap_or(Reading { label, ..Reading::default() });
    Ok(reading.lines().into_iter().map(|line| line + "\n").collect())
}
