//! The harness lanes of the week: each worker landing on north's main in the last 7 days, whether its commit message
//! names the `Expected effect:` it should have on finished boxes per unit of usage, and the incidents it caused within
//! 48 h: the rework judge's revert, fix forward or red-main culprit, and the incident rows whose detail names its sha.
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;
use std::process::Command;

use rusqlite::Connection;

use super::Window;
use crate::foundation::clock::Timestamp;
use crate::threads::calibration::rework::{Cause, REWORK_WINDOW_S, Rework, Sources};

const REPO: &str = "north";
const EFFECT: &str = "Expected effect:";
const SHA_CHARS: usize = 9;
const SUBJECT_CHARS: usize = 90;

/// One harness lane of the week with the incidents it caused.
pub struct Lane {
    pub sha: String,
    pub subject: String,
    pub effect: bool,
    pub caused: Vec<String>,
}

fn messages(checkout: &Path, since: i64) -> BTreeMap<String, String> {
    let output = Command::new("git").arg("-C").arg(checkout).args(["log", "origin/main", &format!("--since=@{since}"), "--format=%H%x1f%B%x1e"]).output();
    let text = output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned()).unwrap_or_default();
    text.split('\x1e').filter_map(|entry| entry.trim_start().split_once('\x1f')).map(|(sha, body)| (sha.to_owned(), body.to_owned())).collect()
}

fn incidents(db: &Connection, window: Window) -> rusqlite::Result<Vec<(i64, String, String)>> {
    let present: bool = db.query_row("SELECT count(*) = 1 FROM sqlite_master WHERE type = 'table' AND name = 'incidents'", [], |row| row.get(0))?;
    if !present {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare("SELECT detected_at, class, subject || ' ' || detail FROM incidents")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
    Ok(rows
        .filter_map(Result::ok)
        .filter_map(|(at, class, text)| {
            let at = at.parse::<f64>().ok().map(|at| at as i64).or_else(|| Timestamp::parse_loose(&at).map(Timestamp::unix_seconds))?;
            (at >= window.start && at < window.end + REWORK_WINDOW_S).then_some((at, class, text))
        })
        .collect())
}

/// The harness lanes that landed in `window`, newest first.
pub fn lanes(db: &Connection, sources: &Sources, window: Window) -> Result<Vec<Lane>, String> {
    let rework = Rework::read(db, sources).map_err(|why| format!("rework: {why}"))?;
    let checkout = sources.code_root.clone().unwrap_or_default().join(REPO).join("main");
    let bodies = messages(&checkout, window.start);
    let rows = incidents(db, window).map_err(|why| why.to_string())?;
    let mut landed: BTreeMap<&str, (i64, Option<String>)> = BTreeMap::new();
    for judged in rework.judged.values().filter(|judged| judged.repo == REPO && window.holds(judged.at)) {
        let reworked = judged.cause.map(|cause| {
            let how = match cause {
                Cause::Reverted => "reverted",
                Cause::FixedForward => "fixed forward",
                Cause::Culprit => "red-main culprit",
            };
            judged.trigger.as_ref().map_or_else(|| how.to_owned(), |trigger| format!("{how} by {}", trigger.sha.chars().take(SHA_CHARS).collect::<String>()))
        });
        let entry = landed.entry(judged.sha.as_str()).or_insert((judged.at, None));
        if entry.1.is_none() {
            entry.1 = reworked;
        }
    }
    let mut found: Vec<(i64, Lane)> = landed
        .into_iter()
        .map(|(sha, (at, reworked))| {
            let body = bodies.get(sha).map_or("", String::as_str);
            let short: String = sha.chars().take(SHA_CHARS).collect();
            let named = rows.iter().filter(|(when, _, text)| *when >= at && *when < at + REWORK_WINDOW_S && text.contains(&short)).map(|(_, class, _)| format!("incident {class}"));
            let caused = reworked.into_iter().chain(named).collect();
            let subject = body.lines().next().unwrap_or_default().chars().take(SUBJECT_CHARS).collect();
            (at, Lane { sha: short, subject, effect: body.contains(EFFECT), caused })
        })
        .collect();
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    Ok(found.into_iter().map(|(_, lane)| lane).collect())
}

/// The week's harness lanes: a count line, then each lane that caused an incident within 48 h.
pub fn render(lanes: &[Lane]) -> String {
    let mut out = String::new();
    let causing: Vec<&Lane> = lanes.iter().filter(|lane| !lane.caused.is_empty()).collect();
    let effect = lanes.iter().filter(|lane| lane.effect).count();
    let _ = writeln!(out, "harness lanes, last 7 days: {} landed on north, {effect} name an expected effect, {} caused an incident within 48 h", lanes.len(), causing.len());
    for lane in causing {
        let _ = writeln!(out, "  - {} {}{}: {}", lane.sha, lane.subject, if lane.effect { "" } else { " (no expected effect)" }, lane.caused.join(", "));
    }
    out
}
