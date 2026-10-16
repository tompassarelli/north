use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;
use time::{Date, Duration, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset, Weekday};

static ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:.(\d{2})(?::?(\d{2})(?::?(\d{2})(?:[.,](\d{1,9}))?)?)?)?(Z|[+-]\d{2}(?::?\d{2}(?::?\d{2})?)?)?$").unwrap()
});
static DAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Moment(pub OffsetDateTime);

fn offset(zone: &str) -> Option<UtcOffset> {
    if zone == "Z" {
        return Some(UtcOffset::UTC);
    }
    let sign: i32 = if zone.starts_with('-') { -1 } else { 1 };
    let digits = zone[1..].replace(':', "");
    let part = |i: usize| {
        digits
            .get(i..i + 2)
            .map_or(Some(0), |p| p.parse::<i32>().ok())
    };
    UtcOffset::from_whole_seconds(sign * (part(0)? * 3600 + part(2)? * 60 + part(4)?)).ok()
}

impl Moment {
    pub fn parse(text: &str) -> Option<Self> {
        let c = ISO.captures(text.trim())?;
        let number = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<u32>().ok());
        let month = Month::try_from(number(2)? as u8).ok()?;
        let date = Date::from_calendar_date(c[1].parse().ok()?, month, number(3)? as u8).ok()?;
        let nanos = c
            .get(7)
            .map_or(Some(0), |m| format!("{:0<9}", m.as_str()).parse().ok())?;
        let clock =
            Time::from_hms_nano(number(4)? as u8, number(5)? as u8, number(6)? as u8, nanos)
                .ok()?;
        let zone = c
            .get(8)
            .map_or(Some(UtcOffset::UTC), |m| offset(m.as_str()))?;
        Some(Self(
            PrimitiveDateTime::new(date, clock).assume_offset(zone),
        ))
    }

    pub fn of(value: &Value) -> Option<Self> {
        match value {
            Value::String(text) => Self::parse(text),
            _ => None,
        }
    }

    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    pub fn seconds_since(self, earlier: Self) -> f64 {
        (self.0 - earlier.0).as_seconds_f64()
    }

    pub fn plus_seconds(self, seconds: f64) -> Self {
        Self(self.0 + Duration::seconds_f64(seconds))
    }

    pub fn date(self) -> Date {
        self.0.date()
    }

    pub fn iso(self) -> String {
        let at = self.0;
        let micros = at.microsecond();
        let fraction = if micros == 0 {
            String::new()
        } else {
            format!(".{micros:06}")
        };
        let seconds = at.offset().whole_seconds();
        let sign = if seconds < 0 { '-' } else { '+' };
        let (h, m, s) = (
            seconds.abs() / 3600,
            seconds.abs() / 60 % 60,
            seconds.abs() % 60,
        );
        let zone = if s == 0 {
            format!("{sign}{h:02}:{m:02}")
        } else {
            format!("{sign}{h:02}:{m:02}:{s:02}")
        };
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{fraction}{zone}",
            at.year(),
            u8::from(at.month()),
            at.day(),
            at.hour(),
            at.minute(),
            at.second()
        )
    }

    pub fn local_clock(self) -> String {
        let local = crate::foundation::clock::Timestamp::from_unix_seconds(self.0.unix_timestamp());
        local.local_hour_minute()
    }

    pub fn eastern(self) -> Self {
        let utc = self.0.to_offset(UtcOffset::UTC);
        Self(utc.to_offset(eastern_offset(utc)))
    }

    pub fn eastern_wall(wall: PrimitiveDateTime) -> Self {
        let daylight = wall.assume_offset(UtcOffset::from_hms(-4, 0, 0).unwrap());
        if eastern_offset(daylight) == daylight.offset() {
            return Self(daylight);
        }
        Self(wall.assume_offset(UtcOffset::from_hms(-5, 0, 0).unwrap()))
    }

    pub fn wall(self) -> PrimitiveDateTime {
        PrimitiveDateTime::new(self.0.date(), self.0.time())
    }
}

fn sunday(year: i32, month: Month, nth: u8) -> Date {
    let first = Date::from_calendar_date(year, month, 1).unwrap();
    let ahead = (7 - first.weekday().number_days_from_sunday()) % 7;
    first + Duration::days(i64::from(ahead) + 7 * i64::from(nth - 1))
}

fn eastern_offset(at: OffsetDateTime) -> UtcOffset {
    let year = at.to_offset(UtcOffset::UTC).year();
    let start = sunday(year, Month::March, 2)
        .with_hms(7, 0, 0)
        .unwrap()
        .assume_utc();
    let end = sunday(year, Month::November, 1)
        .with_hms(6, 0, 0)
        .unwrap()
        .assume_utc();
    let hours = if start <= at && at < end { -4 } else { -5 };
    UtcOffset::from_hms(hours, 0, 0).unwrap()
}

pub fn days_to_friday(day: Weekday) -> i64 {
    (4 - i64::from(day.number_days_from_monday()) + 7) % 7
}

pub fn target_open(target: &Value, now: Moment) -> bool {
    let truthy = match target {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    };
    if !truthy {
        return false;
    }
    if let Value::String(text) = target
        && DAY.is_match(text)
        && let Some(day) = Moment::parse(text)
    {
        return day.date() >= now.date();
    }
    Moment::of(target).is_none_or(|when| when >= now)
}

pub fn python_g(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let exponent = value.abs().log10().floor() as i32;
    if !(-4..6).contains(&exponent) {
        let text = format!("{value:.5e}");
        let (mantissa, power) = text.split_once('e').unwrap();
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let power: i32 = power.parse().unwrap();
        return format!(
            "{mantissa}e{}{:02}",
            if power < 0 { '-' } else { '+' },
            power.abs()
        );
    }
    let digits = (5 - exponent).max(0) as usize;
    let text = format!("{value:.digits$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

pub fn python_str(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}
