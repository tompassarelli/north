use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::claude_stream::{RateLimitEvent, RateWindow};

const SOURCE: &str = "claude stream-json rate_limit_event";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Window {
    FiveHour,
    SevenDay,
}

impl Window {
    fn name(self) -> &'static str {
        match self {
            Self::FiveHour => "session",
            Self::SevenDay => "weekly_all",
        }
    }

    fn minutes(self) -> u64 {
        match self {
            Self::FiveHour => 300,
            Self::SevenDay => 10080,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Observation {
    observed_at: u64,
    used_pct: i64,
    resets_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UsageStream {
    account: String,
    windows: BTreeMap<Window, Observation>,
}

impl UsageStream {
    pub(crate) fn new(account: &str) -> Self {
        Self { account: account.to_owned(), windows: BTreeMap::new() }
    }

    pub(crate) fn observe(&mut self, event: &RateLimitEvent, observed_at: u64) -> bool {
        let windows = &event.rate_limit_info.unified_windows;
        let mut changed = false;
        for (window, reading) in [(Window::FiveHour, &windows.five_hour), (Window::SevenDay, &windows.seven_day)] {
            let Some(reading) = reading else { continue };
            if self.windows.get(&window).is_some_and(|known| known.observed_at > observed_at) {
                continue;
            }
            let next = observation(reading, observed_at);
            if self.windows.get(&window) != Some(&next) {
                self.windows.insert(window, next);
                changed = true;
            }
        }
        changed
    }

    pub(crate) fn rows(&self) -> Vec<Value> {
        self.windows.iter().map(|(window, observation)| self.row(*window, observation)).collect()
    }

    fn row(&self, window: Window, observation: &Observation) -> Value {
        let mut row = Map::new();
        row.insert("provider".into(), json!("claude"));
        row.insert("account".into(), json!(self.account));
        row.insert("source".into(), json!(SOURCE));
        row.insert("ts".into(), json!(utc(observation.observed_at as i64)));
        row.insert("kind".into(), json!("window"));
        row.insert("name".into(), json!(window.name()));
        row.insert("used_pct".into(), json!(observation.used_pct));
        row.insert("window_min".into(), json!(window.minutes()));
        if let Some(resets_at) = observation.resets_at {
            row.insert("resets_at".into(), json!(utc(resets_at)));
        }
        row.insert("amount".into(), Value::Null);
        Value::Object(row)
    }
}

fn observation(reading: &RateWindow, observed_at: u64) -> Observation {
    Observation {
        observed_at,
        used_pct: (reading.utilization * 100.0).round() as i64,
        resets_at: reading.resets_at,
    }
}

fn utc(unix_seconds: i64) -> String {
    let days = unix_seconds.div_euclid(86_400);
    let seconds = unix_seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude_stream::{StreamFrame, decode_frame};

    const FOLD: &str = include_str!("fixtures/claude-stream/fold.jsonl");

    fn event(line: &str) -> RateLimitEvent {
        match decode_frame(line).unwrap() {
            StreamFrame::RateLimit(event) => event,
            other => panic!("not a rate_limit_event: {other:?}"),
        }
    }

    fn recorded() -> RateLimitEvent {
        event(FOLD.lines().find(|line| line.contains("\"rate_limit_event\"")).unwrap())
    }

    fn row<'a>(rows: &'a [Value], name: &str) -> &'a Value {
        rows.iter().find(|row| row["name"] == name).unwrap()
    }

    #[test]
    fn usage_stream_recorded_frame_converts_to_window_rows() {
        let mut usage = UsageStream::new("a");
        assert!(usage.observe(&recorded(), 1_792_040_000));
        let rows = usage.rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            row(&rows, "session"),
            &json!({"provider": "claude", "account": "a", "source": SOURCE, "ts": "2026-10-15T04:53:20Z",
                    "kind": "window", "name": "session", "used_pct": 69, "window_min": 300,
                    "resets_at": "2026-10-15T07:30:00Z", "amount": null})
        );
        assert_eq!(row(&rows, "weekly_all")["used_pct"], 18);
        assert_eq!(row(&rows, "weekly_all")["window_min"], 10080);
        assert_eq!(row(&rows, "weekly_all")["resets_at"], "2026-10-22T02:00:00Z");
    }

    #[test]
    fn usage_stream_older_observation_does_not_overwrite_newer() {
        let mut usage = UsageStream::new("a");
        let newer = event(r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.75,"resetsAt":1792049400}}}}"#);
        assert!(usage.observe(&newer, 200));
        usage.observe(&recorded(), 100);
        assert_eq!(row(&usage.rows(), "session")["used_pct"], 75);
    }

    #[test]
    fn usage_stream_leaves_fields_the_frame_omits_absent() {
        let mut usage = UsageStream::new("a");
        usage.observe(&event(r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.4}}}}"#), 100);
        let rows = usage.rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].get("resets_at").is_none());
    }
}
