use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset, Weekday};

use crate::error::{NorthError, NorthResult};

const NANOS_PER_SECOND: i128 = 1_000_000_000;
const NANOS_PER_MILLI: i128 = 1_000_000;
const SECONDS_PER_DAY: i64 = 86_400;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct Timestamp(i128);

impl Timestamp {
    pub fn now() -> Self {
        Self::from(SystemTime::now())
    }

    pub fn from_unix_seconds(seconds: i64) -> Self {
        Self(i128::from(seconds) * NANOS_PER_SECOND)
    }

    pub fn from_unix_millis(millis: i64) -> Self {
        Self(i128::from(millis) * NANOS_PER_MILLI)
    }

    pub fn from_unix_seconds_f64(seconds: f64) -> Self {
        Self((seconds * 1e9) as i128)
    }

    pub fn from_unix_millis_f64(millis: f64) -> Self {
        Self::from_unix_millis(millis.trunc() as i64)
    }

    pub fn unix_seconds(self) -> i64 {
        self.0.div_euclid(NANOS_PER_SECOND) as i64
    }

    pub fn unix_millis(self) -> i64 {
        self.0.div_euclid(NANOS_PER_MILLI) as i64
    }

    pub fn unix_nanos(self) -> i128 {
        self.0
    }

    pub fn unix_seconds_f64(self) -> f64 {
        self.unix_seconds() as f64 + self.0.rem_euclid(NANOS_PER_SECOND) as f64 / 1e9
    }

    pub fn unix_days(self) -> i64 {
        self.unix_seconds().div_euclid(SECONDS_PER_DAY)
    }

    pub fn local_day_start_ms(self) -> u64 {
        let offset = local_offset_seconds();
        let local_day = (self.unix_seconds() + offset).div_euclid(SECONDS_PER_DAY);
        u64::try_from((local_day * SECONDS_PER_DAY - offset) * 1000).unwrap_or(0)
    }

    fn moment(self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp_nanos(self.0).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    fn date_and_minute(self) -> String {
        let at = self.moment();
        format!("{:04}-{:02}-{:02}T{:02}:{:02}", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute())
    }

    pub fn iso_date(self) -> String {
        let at = self.moment();
        format!("{:04}-{:02}-{:02}", at.year(), u8::from(at.month()), at.day())
    }

    pub fn iso_minute(self) -> String {
        format!("{}Z", self.date_and_minute())
    }

    pub fn iso(self) -> String {
        format!("{}:{:02}Z", self.date_and_minute(), self.moment().second())
    }

    pub fn iso_millis(self) -> String {
        let at = self.moment();
        format!("{}:{:02}.{:03}Z", self.date_and_minute(), at.second(), at.millisecond())
    }

    pub fn iso_micros_utc_offset(self) -> String {
        let at = self.moment();
        format!("{}:{:02}.{:06}+00:00", self.date_and_minute(), at.second(), at.microsecond())
    }

    pub fn local_hour_minute(self) -> String {
        let minutes = (self.unix_seconds() + local_offset_seconds()).div_euclid(60);
        format!("{:02}:{:02}", minutes.div_euclid(60).rem_euclid(24), minutes.rem_euclid(60))
    }

    pub fn day_and_minute(self) -> String {
        let at = self.moment();
        format!("{:02} {} {:02}:{:02}Z", at.day(), MONTHS[usize::from(u8::from(at.month())) - 1], at.hour(), at.minute())
    }

    pub fn parse(text: &str) -> NorthResult<Self> {
        let invalid = || NorthError::Usage(format!("Expected an ISO time like 2026-10-15T15:20:00Z, got {text:?}"));
        let bytes = text.as_bytes();
        if bytes.len() < 17 || bytes[4] != b'-' || bytes[7] != b'-' || !matches!(bytes[10], b'T' | b't' | b' ') || bytes[13] != b':' {
            return Err(invalid());
        }
        let number = |range: std::ops::Range<usize>| -> NorthResult<i64> {
            let part = text.get(range).ok_or_else(invalid)?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            part.parse().map_err(|_| invalid())
        };
        let (year, month, day, hour, minute) = (number(0..4)?, number(5..7)?, number(8..10)?, number(11..13)?, number(14..16)?);
        let mut rest = &text[16..];
        let mut second = 0;
        if let Some(after) = rest.strip_prefix(':') {
            second = number(17..19)?;
            rest = after.get(2..).ok_or_else(invalid)?;
            if let Some(fraction) = rest.strip_prefix('.') {
                let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
                if digits == 0 {
                    return Err(invalid());
                }
                rest = &fraction[digits..];
            }
        }
        let offset = match rest {
            "Z" | "z" => 0,
            _ if rest.len() == 6 && matches!(rest.as_bytes()[0], b'+' | b'-') && rest.as_bytes()[3] == b':' => {
                let start = text.len() - 5;
                let value = number(start..start + 2)? * 3600 + number(start + 3..start + 5)? * 60;
                if rest.starts_with('-') { -value } else { value }
            }
            _ => return Err(invalid()),
        };
        if hour > 23 || minute > 59 || second > 60 {
            return Err(invalid());
        }
        let date = calendar_date(year, month, day).ok_or_else(invalid)?;
        Ok(Self::from_unix_seconds(unix_days(date) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second - offset))
    }

    pub fn parse_loose(text: &str) -> Option<Self> {
        let pattern = cached(&LOOSE, r"^([0-9]{4})-([0-9]{2})-([0-9]{2})(?:T([0-9]{2}):([0-9]{2})(?::([0-9]{2})(?:\.([0-9]{1,9}))?)?(Z|[+-][0-9]{2}:[0-9]{2})?)?$");
        let c = pattern.captures(text)?;
        let num = |i: usize| c.get(i).map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(0));
        let (year, month, day, hour, minute, second) = (num(1), num(2), num(3), num(4), num(5), num(6));
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 24 || minute > 59 || second > 59 {
            return None;
        }
        let fraction = c.get(7).map_or(0.0, |m| format!("0.{}", m.as_str()).parse::<f64>().unwrap_or(0.0));
        let offset = match c.get(8).map(|m| m.as_str()) {
            Some(zone) if zone.len() == 6 => {
                let sign = if zone.starts_with('-') { -1 } else { 1 };
                sign * (zone[1..3].parse::<i64>().ok()? * 60 + zone[4..6].parse::<i64>().ok()?)
            }
            _ => 0,
        };
        let seconds = rolling_days(year, month, day)? * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second - offset * 60;
        Some(Self::from_unix_millis(seconds * 1000 + (fraction * 1000.0).trunc() as i64))
    }

    pub fn parse_rfc3339(text: &str) -> Option<Self> {
        let pattern = cached(&RFC3339, r"^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:\d{2})$");
        let c = pattern.captures(text.trim())?;
        let number = |i| c.get(i)?.as_str().parse::<u8>().ok();
        let date = Date::from_calendar_date(c[1].parse().ok()?, Month::try_from(number(2)?).ok()?, number(3)?).ok()?;
        let nanos = c.get(7).map_or(Some(0), |v| format!("{:0<9}", v.as_str()).parse::<u32>().ok())?;
        let clock = Time::from_hms_nano(number(4)?, number(5)?, number(6)?, nanos).ok()?;
        let zone = &c[8];
        let offset = if zone == "Z" {
            UtcOffset::UTC
        } else {
            let sign: i8 = if zone.starts_with('-') { -1 } else { 1 };
            UtcOffset::from_hms(sign * zone[1..3].parse::<i8>().ok()?, sign * zone[4..6].parse::<i8>().ok()?, 0).ok()?
        };
        Some(Self(PrimitiveDateTime::new(date, clock).assume_offset(offset).unix_timestamp_nanos()))
    }

    pub fn parse_publish_time(text: &str) -> Option<Self> {
        let pattern = cached(&PUBLISH, r"\A(\d{4})-?(\d{2})-?(\d{2}).(\d{2}):?(\d{2})(?::?(\d{2})(?:[.,](\d+))?)?([+-])(\d{2}):?(\d{2})(?::?(\d{2})(?:[.,](\d+))?)?\z");
        let c = pattern.captures(text)?;
        let number = |i| c.get(i).map_or(Some(0), |v| v.as_str().parse::<i32>().ok());
        let date = Date::from_calendar_date(number(1)?, Month::try_from(number(2)? as u8).ok()?, number(3)? as u8).ok()?;
        let clock = Time::from_hms(number(4)? as u8, number(5)? as u8, number(6)? as u8).ok()?;
        if number(9)? >= 24 || number(10)? >= 60 || number(11)? >= 60 {
            return None;
        }
        let micros = |i| c.get(i).map_or(Some(0), |v| fraction_micros(v.as_str()));
        let offset = i128::from(number(9)? * 3600 + number(10)? * 60 + number(11)?) * NANOS_PER_SECOND + micros(12)? * 1000;
        let local = i128::from(date.with_time(clock).assume_utc().unix_timestamp()) * NANOS_PER_SECOND + micros(7)? * 1000;
        Some(Self(if &c[8] == "+" { local - offset } else { local + offset }))
    }

    pub fn parse_python_datetime(value: &str) -> Option<Self> {
        let pattern = cached(&PYTHON_DATETIME, r"\A(\d{4}-\d{2}-\d{2}|\d{8}|\d{4}-W\d{2}(?:-\d)?|\d{4}W\d{2}\d?)(?:(?s:.)(.+))?\z");
        let m = pattern.captures(value)?;
        let mut nanos = parse_python_date(&m[1])?.midnight().assume_utc().unix_timestamp_nanos();
        let Some(clock) = m.get(2).map(|m| m.as_str()) else {
            return Some(Self(nanos));
        };
        if let Some(zulu) = clock.strip_suffix('Z') {
            nanos += python_clock_nanos(zulu)?;
        } else if let Some(index) = clock.find(['+', '-']) {
            nanos += python_clock_nanos(&clock[..index])?;
            let offset = python_clock_nanos(&clock[index + 1..])?;
            nanos -= if clock.as_bytes()[index] == b'-' { -offset } else { offset };
        } else {
            nanos += python_clock_nanos(clock)?;
        }
        Some(Self(nanos))
    }
}

impl From<SystemTime> for Timestamp {
    fn from(time: SystemTime) -> Self {
        Self(time.duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos() as i128))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.iso())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

pub fn parse_day_number(text: &str) -> Option<i64> {
    let mut parts = text.splitn(3, '-');
    let year: i64 = parts.next().filter(|part| part.len() == 4)?.parse().ok()?;
    let month: i64 = parts.next().filter(|part| part.len() == 2)?.parse().ok()?;
    let day: i64 = parts.next().filter(|part| part.len() == 2)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    rolling_days(year, month, day)
}

pub fn parse_python_date(value: &str) -> Option<Date> {
    if !value.is_ascii() {
        return None;
    }
    let week_pattern = cached(&PYTHON_WEEK, r"\A(\d{4})(?:-W(\d{2})(?:-(\d))?|W(\d{2})(\d)?)\z");
    if let Some(m) = week_pattern.captures(value) {
        let year = m[1].parse().ok()?;
        if !(1..=9999).contains(&year) {
            return None;
        }
        let week = m.get(2).or(m.get(4))?.as_str().parse().ok()?;
        let weekday = match m.get(3).or(m.get(5)).map_or("1", |s| s.as_str()) {
            "1" => Weekday::Monday,
            "2" => Weekday::Tuesday,
            "3" => Weekday::Wednesday,
            "4" => Weekday::Thursday,
            "5" => Weekday::Friday,
            "6" => Weekday::Saturday,
            "7" => Weekday::Sunday,
            _ => return None,
        };
        return Date::from_iso_week_date(year, week, weekday).ok().filter(|d| (1..=9999).contains(&d.year()));
    }
    let compact;
    let value = if cached(&EIGHT_DIGITS, r"\A\d{8}\z").is_match(value) {
        compact = format!("{}-{}-{}", &value[..4], &value[4..6], &value[6..]);
        &compact
    } else {
        value
    };
    let m = cached(&PYTHON_DAY, r"\A(\d{4})-(\d{2})-(\d{2})\z").captures(value)?;
    let year = m[1].parse().ok()?;
    if !(1..=9999).contains(&year) {
        return None;
    }
    Date::from_calendar_date(year, Month::try_from(m[2].parse::<u8>().ok()?).ok()?, m[3].parse().ok()?).ok()
}

fn local_offset_seconds() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        let output = Command::new("date").arg("+%z").output().ok();
        output.and_then(|output| offset_seconds(String::from_utf8_lossy(&output.stdout).trim())).unwrap_or(0)
    })
}

fn offset_seconds(zone: &str) -> Option<i64> {
    let (sign, digits) = zone.split_at_checked(1)?;
    let sign = match sign {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };
    let (hours, minutes) = digits.split_at_checked(2)?;
    Some(sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60))
}

pub fn local_today() -> Date {
    Command::new("date")
        .arg("+%F")
        .output()
        .ok()
        .and_then(|o| parse_python_date(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_else(|| OffsetDateTime::now_utc().date())
}

static LOOSE: OnceLock<Regex> = OnceLock::new();
static RFC3339: OnceLock<Regex> = OnceLock::new();
static PUBLISH: OnceLock<Regex> = OnceLock::new();
static PYTHON_DATETIME: OnceLock<Regex> = OnceLock::new();
static PYTHON_WEEK: OnceLock<Regex> = OnceLock::new();
static PYTHON_DAY: OnceLock<Regex> = OnceLock::new();
static PYTHON_CLOCK: OnceLock<Regex> = OnceLock::new();
static PYTHON_COMPACT_CLOCK: OnceLock<Regex> = OnceLock::new();
static EIGHT_DIGITS: OnceLock<Regex> = OnceLock::new();

fn cached(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("clock pattern"))
}

fn unix_days(date: Date) -> i64 {
    i64::from(date.to_julian_day() - OffsetDateTime::UNIX_EPOCH.date().to_julian_day())
}

fn calendar_date(year: i64, month: i64, day: i64) -> Option<Date> {
    let month = Month::try_from(u8::try_from(month).ok()?).ok()?;
    Date::from_calendar_date(i32::try_from(year).ok()?, month, u8::try_from(day).ok()?).ok()
}

fn rolling_days(year: i64, month: i64, day: i64) -> Option<i64> {
    calendar_date(year, month, 1).map(|first| unix_days(first) + day - 1)
}

fn fraction_micros(digits: &str) -> Option<i128> {
    let micros: String = digits.chars().chain(std::iter::repeat('0')).take(6).collect();
    micros.parse().ok()
}

fn python_clock_nanos(text: &str) -> Option<i128> {
    if !text.is_ascii() {
        return None;
    }
    let c = cached(&PYTHON_CLOCK, r"\A(\d{2})(?::(\d{2})(?::(\d{2}))?)?(?:[.,](\d+))?\z")
        .captures(text)
        .or_else(|| cached(&PYTHON_COMPACT_CLOCK, r"\A(\d{2})(?:(\d{2})(?:(\d{2}))?)?(?:[.,](\d+))?\z").captures(text))?;
    let part = |i| c.get(i).map_or(Some(0), |v| v.as_str().parse::<u8>().ok());
    let (hour, minute, second) = (part(1)?, part(2)?, part(3)?);
    if hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    let micros = c.get(4).map_or(Some(0), |v| fraction_micros(v.as_str()))?;
    Some((i128::from(hour) * 3600 + i128::from(minute) * 60 + i128::from(second)) * NANOS_PER_SECOND + micros * 1000)
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
