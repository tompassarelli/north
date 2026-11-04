use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

pub type Moment = OffsetDateTime;

static ISO: OnceLock<Regex> = OnceLock::new();

pub fn parse(text: &str) -> Option<Moment> {
    let pattern = ISO.get_or_init(|| {
        Regex::new(r"\A(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2})(?:[.,](\d{1,9}))?)?(?:(Z)|([+-])(\d{2}):?(\d{2})(?::?(\d{2}))?)\z").expect("iso pattern")
    });
    let found = pattern.captures(text.trim())?;
    let number = |index: usize| found.get(index).map_or(Some(0), |part| part.as_str().parse::<u32>().ok());
    let date = Date::from_calendar_date(number(1)? as i32, Month::try_from(number(2)? as u8).ok()?, number(3)? as u8).ok()?;
    let nanos = found.get(7).map_or(Some(0), |part| format!("{:0<9}", part.as_str()).parse::<u32>().ok())?;
    let clock = Time::from_hms_nano(number(4)? as u8, number(5)? as u8, number(6)? as u8, nanos).ok()?;
    let offset = if found.get(8).is_some() {
        0
    } else {
        let sign = if &found[9] == "-" { -1 } else { 1 };
        sign * (number(10)? as i32 * 3600 + number(11)? as i32 * 60 + number(12)? as i32)
    };
    Some(PrimitiveDateTime::new(date, clock).assume_offset(UtcOffset::from_whole_seconds(offset).ok()?))
}

pub fn iso(at: Moment) -> String {
    let at = at.to_offset(UtcOffset::UTC);
    let base = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second());
    match at.microsecond() {
        0 => format!("{base}Z"),
        micros => format!("{base}.{micros:06}Z"),
    }
}

#[derive(Clone, Debug)]
pub enum Zone {
    Fixed(i32),
    Table { transitions: Vec<(i64, i32)>, initial: i32, rule: Option<Rule> },
}

#[derive(Clone, Debug)]
pub struct Rule {
    standard: i32,
    daylight: Option<(i32, Change, Change)>,
}

#[derive(Clone, Copy, Debug)]
struct Change {
    month: u8,
    week: u8,
    weekday: u8,
    seconds: i64,
}

const ZONE_DIRS: [&str; 4] = ["/etc/zoneinfo", "/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo"];

impl Zone {
    pub fn named(name: &str) -> Option<Self> {
        let name = name.strip_prefix(':').unwrap_or(name);
        let mut dirs: Vec<String> = std::env::var("TZDIR").ok().into_iter().collect();
        dirs.extend(ZONE_DIRS.iter().map(|dir| (*dir).to_owned()));
        let found = if name.starts_with('/') {
            Self::file(Path::new(name))
        } else if name.contains("..") {
            None
        } else {
            dirs.iter().find_map(|dir| Self::file(&Path::new(dir).join(name)))
        };
        found.or_else(|| matches!(name, "UTC" | "Etc/UTC" | "GMT" | "Etc/GMT" | "Zulu").then_some(Self::Fixed(0)))
    }

    pub fn local(now: Moment) -> Self {
        let zone = Self::file(Path::new("/etc/localtime")).unwrap_or(Self::Fixed(0));
        Self::Fixed(zone.offset_at(now.unix_timestamp()))
    }

    fn file(path: &Path) -> Option<Self> {
        tzif(&std::fs::read(path).ok()?)
    }

    pub fn offset_at(&self, unix: i64) -> i32 {
        match self {
            Self::Fixed(offset) => *offset,
            Self::Table { transitions, initial, rule } => {
                let index = transitions.partition_point(|(at, _)| *at <= unix);
                match (index, rule) {
                    (0, _) => *initial,
                    (index, Some(rule)) if index == transitions.len() => rule.offset_at(unix),
                    (index, _) => transitions[index - 1].1,
                }
            }
        }
    }

    pub fn localize(&self, at: Moment) -> Moment {
        at.to_offset(UtcOffset::from_whole_seconds(self.offset_at(at.unix_timestamp())).unwrap_or(UtcOffset::UTC))
    }

    pub fn resolve(&self, wall: PrimitiveDateTime) -> Moment {
        let naive = wall.assume_utc().unix_timestamp();
        let before = self.offset_at(naive - 86_400);
        let after = self.offset_at(naive + 86_400);
        let valid = |offset: i32| self.offset_at(naive - i64::from(offset)) == offset;
        let chosen = if valid(before) || !valid(after) { before } else { after };
        wall.assume_offset(UtcOffset::from_whole_seconds(chosen).unwrap_or(UtcOffset::UTC))
    }
}

impl Rule {
    fn offset_at(&self, unix: i64) -> i32 {
        let Some((daylight, start, end)) = self.daylight else { return self.standard };
        let year = OffsetDateTime::from_unix_timestamp(unix + i64::from(self.standard)).map_or(1970, |at| at.year());
        let begins = start.local_seconds(year) - i64::from(self.standard);
        let ends = end.local_seconds(year) - i64::from(daylight);
        let summer = if begins < ends { begins <= unix && unix < ends } else { !(ends <= unix && unix < begins) };
        if summer { daylight } else { self.standard }
    }
}

impl Change {
    fn local_seconds(self, year: i32) -> i64 {
        let Ok(month) = Month::try_from(self.month) else { return 0 };
        let Ok(first) = Date::from_calendar_date(year, month, 1) else { return 0 };
        let shift = (i64::from(self.weekday) - i64::from(first.weekday().number_days_from_sunday())).rem_euclid(7);
        let mut day = 1 + shift + (i64::from(self.week) - 1) * 7;
        while day > i64::from(time::util::days_in_month(month, year)) {
            day -= 7;
        }
        first.midnight().assume_utc().unix_timestamp() + (day - 1) * 86_400 + self.seconds
    }
}

fn tzif(bytes: &[u8]) -> Option<Zone> {
    let counts = |at: usize| -> Option<[usize; 6]> {
        let header = bytes.get(at..at + 44).filter(|header| header.starts_with(b"TZif"))?;
        let mut counts = [0; 6];
        for (index, count) in counts.iter_mut().enumerate() {
            *count = u32::from_be_bytes(header[20 + index * 4..24 + index * 4].try_into().ok()?) as usize;
        }
        Some(counts)
    };
    let [isut, isstd, leap, times, types, chars] = counts(0)?;
    if bytes[4] == 0 {
        return None;
    }
    let second = 44 + times * 5 + types * 6 + chars + leap * 8 + isstd + isut;
    let [isut, isstd, leap, times, types, chars] = counts(second)?;
    let data = second + 44;
    let read = |at: usize, width: usize| bytes.get(at..at + width);
    let offsets: Vec<i32> = (0..types)
        .map(|index| read(data + times * 9 + index * 6, 4).map(|raw| i32::from_be_bytes(raw.try_into().unwrap_or_default())))
        .collect::<Option<_>>()?;
    let transitions = (0..times)
        .map(|index| {
            let at = i64::from_be_bytes(read(data + index * 8, 8)?.try_into().ok()?);
            Some((at, *offsets.get(usize::from(*read(data + times * 8 + index, 1)?.first()?))?))
        })
        .collect::<Option<Vec<_>>>()?;
    let footer = data + times * 9 + types * 6 + chars + leap * 12 + isstd + isut;
    let rule = bytes.get(footer..).and_then(|tail| std::str::from_utf8(tail).ok()).and_then(|tail| posix(tail.trim()));
    Some(Zone::Table { transitions, initial: *offsets.first()?, rule })
}

fn posix(text: &str) -> Option<Rule> {
    let mut rest = text;
    zone_name(&mut rest)?;
    let standard = -clock_seconds(&mut rest)? as i32;
    if rest.is_empty() {
        return Some(Rule { standard, daylight: None });
    }
    zone_name(&mut rest)?;
    let daylight = if rest.starts_with(',') { standard + 3600 } else { -clock_seconds(&mut rest)? as i32 };
    let mut parts = rest.strip_prefix(',')?.split(',');
    let start = change(parts.next()?)?;
    let end = change(parts.next()?)?;
    Some(Rule { standard, daylight: Some((daylight, start, end)) })
}

fn zone_name(rest: &mut &str) -> Option<()> {
    let length = if let Some(quoted) = rest.strip_prefix('<') {
        quoted.find('>')? + 2
    } else {
        rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len())
    };
    (length >= 3).then(|| *rest = &rest[length..])
}

fn clock_seconds(rest: &mut &str) -> Option<i64> {
    let length = rest.find(|c: char| !(c.is_ascii_digit() || matches!(c, ':' | '+' | '-'))).unwrap_or(rest.len());
    let (text, tail) = rest.split_at(length);
    *rest = tail;
    let (sign, digits) = match text.as_bytes().first()? {
        b'-' => (-1, &text[1..]),
        b'+' => (1, &text[1..]),
        _ => (1, text),
    };
    let mut total = 0;
    for (index, part) in digits.split(':').enumerate() {
        total += part.parse::<i64>().ok()? * [3600, 60, 1].get(index)?;
    }
    Some(sign * total)
}

fn change(text: &str) -> Option<Change> {
    let (date, at) = text.split_once('/').map_or((text, None), |(date, at)| (date, Some(at)));
    let mut fields = date.strip_prefix('M')?.split('.').map(|field| field.parse::<u8>().ok());
    let (month, week, weekday) = (fields.next()??, fields.next()??, fields.next()??);
    let seconds = match at {
        Some(mut at) => clock_seconds(&mut at)?,
        None => 7200,
    };
    (weekday <= 6 && (1..=12).contains(&month) && (1..=5).contains(&week)).then_some(Change { month, week, weekday, seconds })
}
