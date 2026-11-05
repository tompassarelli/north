use crate::foundation::clock::Timestamp;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const WINDOWS: [u64; 2] = [256 << 10, 4 << 20];
const LINE_LIMIT: u64 = 1 << 20;
const HEAD_LINES: usize = 8;
const USAGE_KEYS: [&str; 3] = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"];

pub(super) fn context_tokens(path: &Path) -> i64 {
    tail_tokens(path).unwrap_or(0)
}

fn tail_tokens(path: &Path) -> Option<i64> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    for window in WINDOWS {
        let start = size.saturating_sub(window);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let mut lines = split_lines(&bytes);
        if start > 0 && !lines.is_empty() {
            lines.remove(0);
        }
        for raw in lines.iter().rev() {
            if let Some(total) = usage_total(raw)? {
                return Some(total);
            }
        }
        if start == 0 {
            break;
        }
    }
    Some(0)
}

fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut begin = 0;
    let mut index = 0;
    while index < bytes.len() {
        let width = match bytes[index] {
            b'\n' => 1,
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\r' => 1,
            _ => 0,
        };
        if width > 0 {
            lines.push(&bytes[begin..index]);
            index += width;
            begin = index;
        } else {
            index += 1;
        }
    }
    if begin < bytes.len() {
        lines.push(&bytes[begin..]);
    }
    lines
}

fn usage_total(raw: &[u8]) -> Option<Option<i64>> {
    if !raw.windows(7).any(|piece| piece == b"\"usage\"") {
        return Some(None);
    }
    let Ok(entry) = serde_json::from_slice::<Value>(raw) else {
        return Some(None);
    };
    if entry["type"] != "assistant" {
        return Some(None);
    }
    let usage = &entry["message"]["usage"];
    let mut total = 0;
    for key in USAGE_KEYS {
        total = i64::saturating_add(total, whole(&usage[key])?);
    }
    Some((total != 0).then_some(total))
}

fn whole(value: &Value) -> Option<i64> {
    match value {
        Value::Null | Value::Bool(false) => Some(0),
        Value::Bool(true) => Some(1),
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().filter(|float| float.is_finite()).map(|float| float as i64)),
        Value::String(text) if text.is_empty() => Some(0),
        Value::String(text) => text.trim().parse().ok(),
        Value::Array(items) if items.is_empty() => Some(0),
        Value::Object(items) if items.is_empty() => Some(0),
        _ => None,
    }
}

pub(super) fn started_at(path: &Path) -> Option<f64> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    for _ in 0..HEAD_LINES {
        let mut raw = Vec::new();
        if (&mut reader).take(LINE_LIMIT).read_until(b'\n', &mut raw).ok()? == 0 {
            break;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(&raw) else {
            continue;
        };
        if let Some(stamp) = entry["timestamp"].as_str() {
            let parsed = Timestamp::parse_python_datetime(&stamp.replace('Z', "+00:00"))?;
            return Some(parsed.unix_seconds_f64());
        }
    }
    None
}
