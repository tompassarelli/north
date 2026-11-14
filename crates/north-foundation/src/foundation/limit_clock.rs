use std::sync::LazyLock;

use regex::Regex;

const DAY_MS: u64 = 86_400_000;

static REPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*you.ve hit your\b([^\n]*?)\blimit").expect("usage limit reply pattern"));
static DATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bresets\s+([a-z]{3})[a-z]*\s+(\d{1,2}),?\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)").expect("limit date pattern"));
const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

static CLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bresets\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)\s*\(utc\)").expect("limit clock pattern"));

/// The next time after `now_ms` that a usage limit message's `resets 3pm (UTC)` clock names.
pub fn reset_after(text: &str, now_ms: u64) -> Option<u64> {
    let found = CLOCK.captures(text)?;
    let (hour, minute): (u64, u64) = (found[1].parse().ok()?, found.get(2).map_or(Some(0), |minute| minute.as_str().parse().ok())?);
    if !(1..=12).contains(&hour) || minute > 59 {
        return None;
    }
    let pm = found[3].eq_ignore_ascii_case("pm");
    let in_day = ((hour % 12) + if pm { 12 } else { 0 }) * 3_600_000 + minute * 60_000;
    let today = now_ms - now_ms % DAY_MS + in_day;
    Some(if today > now_ms { today } else { today + DAY_MS })
}

/// What a provider's usage-limit reply names between `hit your` and `limit`, lowercased: `fable` for a model's weekly limit, `weekly`, or nothing for the five-hour one.
pub fn limit_reply(text: &str) -> Option<String> {
    REPLY.captures(text).map(|found| found[1].trim().to_ascii_lowercase())
}

/// The time a usage limit reply's `resets Nov 19, 9am` names, read as UTC, in the year of `now_ms` or the next one.
pub fn reset_on(text: &str, now_ms: u64) -> Option<u64> {
    let found = DATED.captures(text)?;
    let month = MONTHS.iter().position(|name| found[1].eq_ignore_ascii_case(name))? as i64 + 1;
    let (day, hour): (i64, u64) = (found[2].parse().ok()?, found[3].parse().ok()?);
    let minute: u64 = found.get(4).map_or(Some(0), |minute| minute.as_str().parse().ok())?;
    if !(1..=31).contains(&day) || !(1..=12).contains(&hour) || minute > 59 {
        return None;
    }
    let in_day = ((hour % 12) + if found[5].eq_ignore_ascii_case("pm") { 12 } else { 0 }) * 3_600_000 + minute * 60_000;
    let year = civil_year(now_ms / DAY_MS);
    [year, year + 1].into_iter().filter_map(|year| u64::try_from(days_from_civil(year, month, day)).ok().map(|days| days * DAY_MS + in_day)).find(|at| *at > now_ms)
}

fn civil_year(days: u64) -> i64 {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(month >= 10)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
