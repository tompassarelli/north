//! Plain-word times for the default `north usage` table: spans, reading ages and local 12-hour clock times.
use time::{OffsetDateTime, UtcOffset};

use crate::foundation::clock::Timestamp;

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
