use std::time::{SystemTime, UNIX_EPOCH};

use crate::cli::Settings;
use crate::error::{Result, RunError};

pub fn refuse_environment<I>(names: I) -> Result<()>
where
    I: IntoIterator<Item = String>,
{
    let offending: Vec<String> = names
        .into_iter()
        .filter(|name| {
            let upper = name.to_ascii_uppercase();
            upper.starts_with("ANTHROPIC_") || upper.contains("API_KEY") || upper.contains("APIKEY")
        })
        .collect();
    if offending.is_empty() {
        Ok(())
    } else {
        Err(RunError::Refused(format!(
            "API key or ANTHROPIC_ variables are set ({}); unset them before a run",
            offending.join(", ")
        )))
    }
}

pub fn one_minute_load(loadavg: &str) -> Option<f64> {
    loadavg.split_whitespace().next()?.parse().ok()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Headroom {
    pub five_hour_used: f64,
    pub week_used: f64,
    pub reading_at: u64,
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Go(Headroom),
    Wait(String),
}

pub struct Snapshot {
    pub at: u64,
    pub line: Option<String>,
}

pub fn snapshot(report: &str, account: &str) -> Result<Snapshot> {
    let value: serde_json::Value = serde_json::from_str(report)?;
    let at = value
        .get("at")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_utc)
        .ok_or_else(|| RunError::Failed("the account report has no readable time".to_owned()))?;
    let prefix = format!("claude:{account} ");
    let line = value
        .get("plan")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("line").and_then(serde_json::Value::as_str))
        .find(|line| line.starts_with(&prefix))
        .map(str::to_owned);
    Ok(Snapshot { at, line })
}

pub fn decide(snapshot: &Snapshot, now: u64, settings: &Settings) -> Verdict {
    if now.saturating_sub(snapshot.at) > settings.max_reading_age_s {
        return Verdict::Wait("the account reading is stale".to_owned());
    }
    let Some(line) = &snapshot.line else {
        return Verdict::Wait("the account has no line in the report".to_owned());
    };
    let (Some(five_hour_used), Some(week_used)) =
        (percent_after(line, "5h "), percent_after(line, "week "))
    else {
        return Verdict::Wait(format!("the reading is missing: {line}"));
    };
    if 100.0 - five_hour_used < settings.min_five_hour_headroom {
        return Verdict::Wait(format!(
            "five-hour headroom is under {}%",
            settings.min_five_hour_headroom
        ));
    }
    if 100.0 - week_used < settings.min_week_headroom {
        return Verdict::Wait(format!(
            "weekly headroom is under {}%",
            settings.min_week_headroom
        ));
    }
    Verdict::Go(Headroom {
        five_hour_used,
        week_used,
        reading_at: snapshot.at,
    })
}

fn percent_after(text: &str, label: &str) -> Option<f64> {
    let start = text.find(label)? + label.len();
    let rest = &text[start..];
    let end = rest.find('%')?;
    let number = &rest[..end];
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    number.parse().ok()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub fn parse_utc(text: &str) -> Option<u64> {
    let (date, time) = text.strip_suffix('Z')?.split_once('T')?;
    let mut day = date.split('-').map(str::parse::<i64>);
    let year = day.next()?.ok()?;
    let month = day.next()?.ok()?;
    let mday = day.next()?.ok()?;
    let mut clock = time.split(':').map(str::parse::<i64>);
    let hour = clock.next()?.ok()?;
    let minute = clock.next()?.ok()?;
    let second = clock.next()?.ok()?;
    let days = days_from_civil(year, month, mday);
    u64::try_from(days * 86_400 + hour * 3_600 + minute * 60 + second).ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPORT: &str = r#"{"at":"2026-11-12T10:44:06Z","plan":[
        {"line":"claude:d available: 5h 16%, week 99% (1% headroom)","reason":"deadline"},
        {"line":"claude:a available: 5h unread, week 48% (52% headroom)","reason":"deadline"}]}"#;

    fn settings() -> Settings {
        Settings::parse(&["plan".to_owned()]).expect("defaults parse")
    }

    #[test]
    fn a_missing_five_hour_reading_waits_and_a_week_at_its_limit_waits() {
        let now = parse_utc("2026-11-12T10:44:30Z").expect("time parses");
        let a = snapshot(REPORT, "a").expect("report parses");
        assert!(
            matches!(decide(&a, now, &settings()), Verdict::Wait(reason) if reason.contains("missing"))
        );
        let d = snapshot(REPORT, "d").expect("report parses");
        assert!(
            matches!(decide(&d, now, &settings()), Verdict::Wait(reason) if reason.contains("weekly"))
        );
    }

    #[test]
    fn an_old_reading_waits_for_a_fresh_one() {
        let later = parse_utc("2026-11-12T11:44:06Z").expect("time parses");
        let d = snapshot(REPORT, "d").expect("report parses");
        assert!(
            matches!(decide(&d, later, &settings()), Verdict::Wait(reason) if reason.contains("stale"))
        );
    }
}
