use serde_json::{Value, json};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const NAMED: &str = "north/watch/named.jsonl";

pub struct Named {
    pub began: u64,
    pub summaries: Vec<String>,
}

fn path(state: &Path) -> PathBuf {
    state.join(NAMED)
}

fn append(state: &Path, line: &Value) {
    let path = path(state);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

pub fn begin(state: &Path, now: u64) {
    if !path(state).exists() {
        append(state, &json!({ "began": now }));
    }
}

pub fn record(state: &Path, now: u64, summary: &str) {
    begin(state, now);
    append(state, &json!({ "at": now, "summary": summary }));
}

pub fn read(state: &Path) -> Option<Named> {
    let text = std::fs::read_to_string(path(state)).ok()?;
    let rows: Vec<Value> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let began = rows.iter().find_map(|row| row["began"].as_u64())?;
    let summaries = rows.iter().filter_map(|row| row["summary"].as_str().map(str::to_owned)).collect();
    Some(Named { began, summaries })
}

pub fn run_time(text: &str) -> Option<u64> {
    let number = |range: std::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let month = time::Month::try_from(u8::try_from(month).ok()?).ok()?;
    let date = time::Date::from_calendar_date(i32::try_from(year).ok()?, month, u8::try_from(day).ok()?).ok()?;
    let days = i64::from(date.to_julian_day()) - 2_440_588;
    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}
