//! Whether an open box can close today or waits on a dated observation window, such as
//! `after 2026-11-06` or `(not before 2026-10-26T05:13Z)` on every unticked Done-when line.
use std::sync::LazyLock;

use regex::Regex;

const DAY: i64 = 86_400;

static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-*] \[ \]").unwrap());
static WINDOW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(after|not before)\s+(\d{4})-(\d{2})-(\d{2})(?:T(\d{2}):(\d{2})Z)?").unwrap());

fn opens_at(found: &regex::Captures) -> Option<i64> {
    let number = |index: usize| found.get(index).map(|part| part.as_str().parse::<u8>().ok());
    let date = time::Date::from_calendar_date(found[2].parse().ok()?, time::Month::try_from(number(3)??).ok()?, number(4)??).ok()?;
    match (number(5), number(6)) {
        (Some(hour), Some(minute)) => Some(date.with_time(time::Time::from_hms(hour?, minute?, 0).ok()?).assume_utc().unix_timestamp()),
        _ => {
            let midnight = date.midnight().assume_utc().unix_timestamp();
            Some(if found[1].eq_ignore_ascii_case("after") { midnight + DAY } else { midnight })
        }
    }
}

/// The epoch a box line's observation window opens, the latest when it names several.
pub fn window_opens(line: &str) -> Option<i64> {
    WINDOW.captures_iter(line).filter_map(|found| opens_at(&found)).max()
}

/// Whether every unticked box in this body waits on a window that has not opened by `now`.
pub fn waits(body: &str, now: f64) -> bool {
    let mut unticked = body.lines().filter(|line| UNTICKED.is_match(line)).peekable();
    unticked.peek().is_some() && unticked.all(|line| window_opens(line).is_some_and(|opens| opens as f64 > now))
}

/// How many of these open issue bodies wait on an observation window that has not opened by `now`.
pub fn waiting(bodies: &[String], now: f64) -> u64 {
    bodies.iter().filter(|body| waits(body, now)).count() as u64
}
