use std::path::Path;
use std::process::Command;

use serde_json::Value;
use time::{Date, Duration, OffsetDateTime};

use super::die::Die;

const DAY: i64 = 86_400;

pub enum Zone {
    Fixed(i64),
    Named(Option<String>),
}

pub(super) struct Window {
    pub(super) zone: Zone,
    pub(super) open: Option<String>,
    pub(super) close: Option<String>,
}

impl Zone {
    pub fn named(name: Option<&str>) -> Self {
        match name {
            Some(name) => fixed(name).map_or_else(|| Self::Named(Some(name.to_owned())), Self::Fixed),
            None => Self::Named(None),
        }
    }

    fn offset(&self, unix: i64) -> i64 {
        match self {
            Self::Fixed(seconds) => *seconds,
            Self::Named(name) => date(name.as_deref(), &format!("@{unix}"), "+%z").and_then(|text| parse_offset(&text)).unwrap_or(0),
        }
    }

    pub fn day(&self, unix: i64) -> Date {
        OffsetDateTime::from_unix_timestamp(unix + self.offset(unix)).map_or(Date::MIN, OffsetDateTime::date)
    }

    pub(super) fn wall(&self, day: Date, hour: i64, minute: i64) -> i64 {
        let naive = day.midnight().assume_utc().unix_timestamp() + hour * 3600 + minute * 60;
        match self {
            Self::Fixed(seconds) => naive - seconds,
            Self::Named(name) => {
                let text = format!("{} {hour:02}:{minute:02}:00", day);
                date(name.as_deref(), &text, "+%s").and_then(|out| out.parse().ok()).unwrap_or(naive)
            }
        }
    }

    pub fn hhmm(&self, unix: i64) -> String {
        let minutes = (unix + self.offset(unix)).div_euclid(60);
        format!("{:02}:{:02}", minutes.div_euclid(60).rem_euclid(24), minutes.rem_euclid(60))
    }

    pub fn at_today(&self, unix: i64, hhmm: &str) -> Option<i64> {
        let (hour, minute) = parse_hhmm(hhmm)?;
        Some(self.wall(self.day(unix), hour, minute))
    }

    pub(super) fn at_yesterday(&self, unix: i64, hhmm: &str) -> Option<i64> {
        let (hour, minute) = parse_hhmm(hhmm)?;
        Some(self.wall(self.day(unix).checked_sub(Duration::days(1))?, hour, minute))
    }

    pub(super) fn at_tomorrow(&self, unix: i64, hhmm: &str) -> Option<i64> {
        let (hour, minute) = parse_hhmm(hhmm)?;
        Some(self.wall(self.day(unix).checked_add(Duration::days(1))?, hour, minute))
    }
}

pub(super) fn parse_hhmm(text: &str) -> Option<(i64, i64)> {
    let (hour, minute) = text.split_once(':')?;
    let number = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit()).then(|| part.parse::<i64>().ok()).flatten();
    let (hour, minute) = (number(hour)?, number(minute)?);
    (hour < 24 && minute < 60).then_some((hour, minute))
}

fn fixed(name: &str) -> Option<i64> {
    let name = name.strip_prefix("Etc/").unwrap_or(name);
    if matches!(name, "UTC" | "GMT" | "Zulu" | "UCT" | "Universal" | "GMT0") {
        return Some(0);
    }
    let rest = name.strip_prefix("GMT")?;
    let sign = match rest.as_bytes().first()? {
        b'+' => -1,
        b'-' => 1,
        _ => return None,
    };
    let hours: i64 = rest[1..].parse().ok().filter(|hours| *hours <= 14)?;
    Some(sign * hours * 3600)
}

fn parse_offset(text: &str) -> Option<i64> {
    let sign = match text.get(..1)? {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let digits = text.get(1..5)?;
    Some(sign * (digits.get(..2)?.parse::<i64>().ok()? * 3600 + digits.get(2..)?.parse::<i64>().ok()? * 60))
}

fn date(zone: Option<&str>, at: &str, format: &str) -> Option<String> {
    let mut command = Command::new("date");
    if let Some(zone) = zone {
        command.env("TZ", zone);
    }
    let output = command.args(["-d", at, format]).output().ok().filter(|output| output.status.success())?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Window {
    pub(super) fn read(path: &Path) -> Self {
        let document = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).unwrap_or(Value::Null);
        let text = |key: &str| document.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned);
        Self { zone: Zone::named(text("tz").as_deref()), open: text("open"), close: text("close") }
    }

    pub(super) fn last_edge(&self, unix: i64) -> Result<Option<(i64, &'static str)>, Die> {
        let (Some(open), Some(close)) = (&self.open, &self.close) else {
            return Ok(None);
        };
        let mut edges = Vec::new();
        for (hhmm, target) in [(open, "available"), (close, "unavailable")] {
            let today = self.zone.at_today(unix, hhmm).ok_or_else(|| Die::new(1, format!("the window time {hhmm} is not HH:MM")))?;
            let edge = if today > unix { self.zone.at_yesterday(unix, hhmm).unwrap_or(today - DAY) } else { today };
            edges.push((edge, target));
        }
        Ok(edges.into_iter().max())
    }

    pub(super) fn until_at(&self, hhmm: &str, unix: i64) -> Result<i64, Die> {
        let bad = || Die::new(2, format!("until wants HH:MM, got {hhmm}"));
        let today = self.zone.at_today(unix, hhmm).ok_or_else(bad)?;
        if today > unix { Ok(today) } else { self.zone.at_tomorrow(unix, hhmm).ok_or_else(bad) }
    }
}
