//! Number and time formats of the Python table: `{:g}`, whole minutes until a time, and the strftime shapes.
use time::OffsetDateTime;

use crate::foundation::clock::Timestamp;

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const NANOS: i128 = 1_000_000_000;

pub(super) fn g(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-4..6).contains(&exponent) {
        return format!("{}e{}{:02}", trimmed(mantissa), if exponent < 0 { '-' } else { '+' }, exponent.abs());
    }
    let decimals = usize::try_from(5 - exponent).unwrap_or(0);
    trimmed(&format!("{value:.decimals$}"))
}

fn trimmed(text: &str) -> String {
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text.to_owned() }
}

pub(super) fn parse(text: &str) -> Result<Timestamp, String> {
    Timestamp::parse_rfc3339(text).ok_or_else(|| format!("unreadable time {text:?}"))
}

pub(super) fn until(at: Option<&str>, now: Timestamp) -> Result<String, String> {
    match at.filter(|text| !text.is_empty()) {
        Some(text) => Ok(span(minutes_between(now, parse(text)?))),
        None => Ok("-".into()),
    }
}

pub(super) fn span(minutes: i64) -> String {
    if minutes >= 1440 {
        format!("{}d{}h", minutes.div_euclid(1440), minutes.rem_euclid(1440).div_euclid(60))
    } else {
        format!("{}h{:02}m", minutes.div_euclid(60), minutes.rem_euclid(60))
    }
}

pub(super) fn minutes_between(from: Timestamp, to: Timestamp) -> i64 {
    let micros = (to.unix_nanos() - from.unix_nanos()).div_euclid(1000);
    (micros as f64 / 1e6 / 60.0).floor() as i64
}

pub(super) fn civil(at: Timestamp) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp_nanos(at.unix_nanos()).unwrap_or(OffsetDateTime::UNIX_EPOCH)
}

pub(super) fn month(at: &OffsetDateTime) -> &'static str {
    MONTHS[usize::from(u8::from(at.month())) - 1]
}

pub(super) fn day(at: Timestamp) -> String {
    let at = civil(at);
    format!("{:04}-{:02}-{:02}", at.year(), u8::from(at.month()), at.day())
}

pub(super) fn minute(at: Timestamp) -> String {
    let moment = civil(at);
    format!("{}T{:02}:{:02}Z", day(at), moment.hour(), moment.minute())
}

pub(super) fn second(at: Timestamp) -> String {
    let moment = civil(at);
    format!("{}T{:02}:{:02}:{:02}Z", day(at), moment.hour(), moment.minute(), moment.second())
}

pub(super) fn isoformat(at: Timestamp) -> String {
    let micros = at.unix_nanos().rem_euclid(NANOS) / 1000;
    let fraction = if micros == 0 { String::new() } else { format!(".{micros:06}") };
    format!("{}{fraction}Z", second(at).trim_end_matches('Z'))
}

pub(super) fn after_hours(at: Timestamp, hours: f64) -> Timestamp {
    Timestamp::from_unix_nanos(at.unix_nanos() + (hours * 3.6e9).round() as i128 * 1000)
}

pub(super) fn before(at: Timestamp, seconds: i64) -> Timestamp {
    Timestamp::from_unix_nanos(at.unix_nanos() - i128::from(seconds) * NANOS)
}

pub(super) fn minutes_precise(from: Timestamp, to: Timestamp) -> f64 {
    (to.unix_nanos() - from.unix_nanos()) as f64 / 1e9 / 60.0
}

pub(super) fn midnight(year: u16, month: u8, day: u8) -> Result<i128, String> {
    let month = time::Month::try_from(month).map_err(|error| error.to_string())?;
    let date = time::Date::from_calendar_date(i32::from(year), month, day).map_err(|error| error.to_string())?;
    Ok(date.midnight().assume_utc().unix_timestamp_nanos())
}
