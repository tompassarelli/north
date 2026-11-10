//! Plain-word times for the default `north fleet usage` table: spans, reading ages and local 12-hour clock times.
use time::{OffsetDateTime, UtcOffset};

use crate::foundation::clock::Timestamp;

pub(super) const GAP: &str = "   ";
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

pub(super) fn span(minutes: i64) -> String {
    let minutes = minutes.max(0);
    let (days, hours, rest) = (minutes / 1440, minutes % 1440 / 60, minutes % 60);
    match (days, hours) {
        (0, 0) => format!("{rest}m"),
        (0, _) => format!("{hours}h {rest:02}m"),
        _ => format!("{days}d {hours}h"),
    }
}

pub(super) fn ago(minutes: f64) -> String {
    let whole = minutes.max(0.0).floor() as i64;
    match whole {
        0 => "read just now".to_owned(),
        1..60 => format!("read {whole} min ago"),
        60..2880 => counted(whole / 60, "hour"),
        _ => counted(whole / 1440, "day"),
    }
}

fn counted(count: i64, unit: &str) -> String {
    format!("read {count} {unit}{} ago", if count == 1 { "" } else { "s" })
}

pub(super) fn clock(at: Timestamp, offset_seconds: i64) -> String {
    let offset = i32::try_from(offset_seconds).ok().and_then(|seconds| UtcOffset::from_whole_seconds(seconds).ok()).unwrap_or(UtcOffset::UTC);
    let moment = OffsetDateTime::from_unix_timestamp_nanos(at.unix_nanos()).unwrap_or(OffsetDateTime::UNIX_EPOCH).to_offset(offset);
    let hour = moment.hour();
    let weekday = WEEKDAYS[usize::from(moment.weekday().number_days_from_monday())];
    format!("{weekday} {}:{:02} {}", (hour + 11) % 12 + 1, moment.minute(), if hour < 12 { "AM" } else { "PM" })
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The month and day of an ISO date such as `2026-12-08`, as `Dec 8`; anything else comes back unchanged.
pub(super) fn day(date: &str) -> String {
    let mut parts = date.get(..10).unwrap_or(date).split('-').skip(1).map(|part| part.parse::<usize>().ok());
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(month @ 1..=12), Some(day @ 1..=31)) => format!("{} {day}", MONTHS[month - 1]),
        _ => date.to_owned(),
    }
}

/// `table` as lines with every column padded to its widest cell, the first at least `first_width` wide.
pub(super) fn grid(table: &[Vec<String>], first_width: usize) -> Vec<String> {
    spaced(table, first_width, GAP)
}

/// `grid` with `gap` between the columns.
pub(super) fn spaced(table: &[Vec<String>], first_width: usize, gap: &str) -> Vec<String> {
    let width = |cell: &String| cell.chars().count();
    let mut widths = vec![first_width];
    for row in table {
        widths.resize(widths.len().max(row.len()), 0);
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(width(cell));
        }
    }
    let padded = |cell: &String, column: usize| format!("{cell}{}{gap}", " ".repeat(widths[column] - width(cell)));
    table.iter().map(|row| row.iter().enumerate().map(|(column, cell)| padded(cell, column)).collect::<String>().trim_end().to_owned()).collect()
}

/// `clock`, without the weekday when `at` falls on the same local day as `now`.
pub(super) fn clock_near(at: Timestamp, now: Timestamp, offset_seconds: i64) -> String {
    let offset = i32::try_from(offset_seconds).ok().and_then(|seconds| UtcOffset::from_whole_seconds(seconds).ok()).unwrap_or(UtcOffset::UTC);
    let local = |moment: Timestamp| OffsetDateTime::from_unix_timestamp_nanos(moment.unix_nanos()).unwrap_or(OffsetDateTime::UNIX_EPOCH).to_offset(offset);
    let shown = clock(at, offset_seconds);
    if local(at).date() == local(now).date() { shown.split_once(' ').map_or(shown.clone(), |(_, time)| time.to_owned()) } else { shown }
}
