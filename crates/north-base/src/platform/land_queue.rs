use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

const GITHUB_SSH: [&str; 2] = ["git@github.com:", "ssh://git@github.com/"];

/// The queue a repository's landings wait in, keyed by its GitHub identity rather than the spelling of its origin URL, so hosts that clone it over ssh and over https share one queue.
pub fn key(origin: &str) -> String {
    Sha256::digest(canonical(origin).as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

fn canonical(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    let origin = origin.strip_suffix(".git").unwrap_or(origin);
    match GITHUB_SSH.iter().find_map(|prefix| origin.strip_prefix(prefix)) {
        Some(path) => format!("https://github.com/{path}.git"),
        None => format!("{origin}.git"),
    }
}

pub fn directory(state_home: &Path, origin: &str) -> PathBuf {
    state_home.join("north/land-queue").join(key(origin))
}

/// Seconds since a ticket was queued, read from its `queued_at` field, else the file's mtime; never from the file-name id, which jump-the-queue tickets set deliberately old.
pub fn ticket_age(ticket: &Path, now: f64) -> Option<f64> {
    let queued = fs::read_to_string(ticket).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).and_then(|ticket| ticket["queued_at"].as_str().and_then(rfc3339_seconds));
    let at = queued.or_else(|| fs::metadata(ticket).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|since| since.as_secs_f64()))?;
    Some((now - at).max(0.0))
}

pub fn rfc3339_seconds(text: &str) -> Option<f64> {
    let (date, clock) = text.strip_suffix('Z')?.split_once('T')?;
    let mut day = date.splitn(3, '-').map(str::parse::<i32>);
    let (year, month, day) = (day.next()?.ok()?, day.next()?.ok()?, day.next()?.ok()?);
    let (whole, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut parts = whole.splitn(3, ':').map(str::parse::<u8>);
    let (hour, minute, second) = (parts.next()?.ok()?, parts.next()?.ok()?, parts.next()?.ok()?);
    let date = time::Date::from_calendar_date(year, time::Month::try_from(u8::try_from(month).ok()?).ok()?, u8::try_from(day).ok()?).ok()?;
    let at = date.with_hms(hour, minute, second).ok()?.assume_utc().unix_timestamp() as f64;
    let fraction = if fraction.is_empty() { 0.0 } else { format!("0.{fraction}").parse::<f64>().ok()? };
    Some(at + fraction)
}
