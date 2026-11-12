use std::sync::LazyLock;

use regex::Regex;

const DAY_MS: u64 = 86_400_000;

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
