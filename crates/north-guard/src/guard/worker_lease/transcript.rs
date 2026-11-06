use crate::foundation::clock::Timestamp;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

const LINE_LIMIT: u64 = 1 << 20;
const HEAD_LINES: usize = 8;

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
