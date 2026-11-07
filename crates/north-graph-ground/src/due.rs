//! The dated check an unticked box waits for: `(not before 2026-11-08T00:00Z)`, or a start or review date such as
//! `after 2026-11-08` or `reviewed on 2026-11-23`. Dispatch and `north graph ready` both hold a box until it comes due,
//! so a box whose only remaining check is a measurement over time is not staffed before then (north#706).
use std::sync::LazyLock;

use north_base::foundation::clock::Timestamp;
use regex::Regex;

static DATED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:not before|on or after|after|from|reviewed on|review on|due on|due)\s+(\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}Z)?)").unwrap_or_else(|error| panic!("{error}"))
});

/// The latest date `text` names its check for, as unix seconds and as written.
pub fn due(text: &str) -> Option<(i64, String)> {
    DATED.captures_iter(text).filter_map(|found| {
        let written = found.get(1)?.as_str();
        Some((Timestamp::parse_loose(written)?.unix_seconds(), written.to_owned()))
    }).max()
}

/// The date `text` names its check for while that date is still ahead of `now`.
pub fn pending(text: &str, now: i64) -> Option<(i64, String)> {
    due(text).filter(|(at, _)| *at > now)
}
