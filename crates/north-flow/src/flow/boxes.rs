//! Open boxes, each unticked `- [ ]` line of an open issue (nested ones included), split by whether
//! it can close today, waits on a dated observation window (`after 2026-11-06`,
//! `(not before 2026-10-26T05:13Z)`, `(waits: 2026-11-12, ...)`), or waits on a decision for Tom
//! (`(decision: Tom: ...)`).
use std::sync::LazyLock;

use regex::Regex;

use crate::platform::cleanup;

const DAY: i64 = 86_400;

static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:[-*+]|\d+[.)]) \[ \]").unwrap());
static WINDOW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:\b(after|not before)\s+|\(\s*(waits):\s*)(\d{4})-(\d{2})-(\d{2})(?:T(\d{2}):(\d{2})Z)?").unwrap());
static DECISION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\(\s*decision:\s*Tom\b").unwrap());

/// Where one open box stands today.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Standing {
    Workable,
    Waiting,
    Decision,
}

/// The open boxes of a repository's open issues, by standing, and how many open issues hold no
/// workable box.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Boxes {
    pub workable: u64,
    pub waiting: u64,
    pub decision: u64,
    pub held_issues: u64,
}

fn opens_at(found: &regex::Captures) -> Option<i64> {
    let number = |index: usize| found.get(index).map(|part| part.as_str().parse::<u8>().ok());
    let date = time::Date::from_calendar_date(found[3].parse().ok()?, time::Month::try_from(number(4)??).ok()?, number(5)??).ok()?;
    match (number(6), number(7)) {
        (Some(hour), Some(minute)) => Some(date.with_time(time::Time::from_hms(hour?, minute?, 0).ok()?).assume_utc().unix_timestamp()),
        _ => {
            let midnight = date.midnight().assume_utc().unix_timestamp();
            let through_the_day = found.get(1).is_some_and(|word| word.as_str().eq_ignore_ascii_case("after")) || found.get(2).is_some();
            Some(if through_the_day { midnight + DAY } else { midnight })
        }
    }
}

/// The epoch a box line's observation window opens, the latest when it names several.
pub fn window_opens(line: &str) -> Option<i64> {
    WINDOW.captures_iter(line).filter_map(|found| opens_at(&found)).max()
}

/// Where this unticked box line stands at `now`.
pub fn standing(line: &str, now: f64) -> Standing {
    if DECISION.is_match(line) {
        Standing::Decision
    } else if window_opens(line).is_some_and(|opens| opens as f64 > now) {
        Standing::Waiting
    } else {
        Standing::Workable
    }
}

/// Every unticked box line of this issue body.
pub fn unticked(body: &str) -> impl Iterator<Item = &str> {
    body.lines().filter(|line| UNTICKED.is_match(line) && !cleanup::is_box(line))
}

/// The open boxes across these open issue bodies, by standing at `now`.
pub fn count(bodies: &[String], now: f64) -> Boxes {
    let mut boxes = Boxes::default();
    for body in bodies {
        let mut workable = false;
        let mut any = false;
        for line in unticked(body) {
            any = true;
            match standing(line, now) {
                Standing::Workable => {
                    workable = true;
                    boxes.workable += 1;
                }
                Standing::Waiting => boxes.waiting += 1,
                Standing::Decision => boxes.decision += 1,
            }
        }
        boxes.held_issues += u64::from(any && !workable);
    }
    boxes
}
