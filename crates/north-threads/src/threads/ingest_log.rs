use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

pub struct Ingest {
    pub started: Timestamp,
    pub acquired: Option<Timestamp>,
    pub ended: Timestamp,
    pub exit: i32,
    pub rows: BTreeMap<String, usize>,
    pub holder: Option<String>,
}

/// How many rows of each type a batch carries, a row without a type counted as a run, as the ingest records it.
pub fn rows(batch: &[u8]) -> BTreeMap<String, usize> {
    let mut rows = BTreeMap::new();
    for line in batch.split(|byte| *byte == b'\n').filter(|line| line.iter().any(|byte| !byte.is_ascii_whitespace())) {
        let kind = match serde_json::from_slice::<Value>(line) {
            Ok(row) => row.get("type").and_then(Value::as_str).unwrap_or("run").to_owned(),
            Err(_) => "unreadable".to_owned(),
        };
        *rows.entry(kind).or_insert(0) += 1;
    }
    rows
}

pub fn append(path: &Path, ingest: &Ingest) -> io::Result<()> {
    let millis = |from: Timestamp, to: Timestamp| (to.unix_millis() - from.unix_millis()).max(0);
    let line = json!({
        "pid": std::process::id(),
        "started": ingest.started.iso_millis(),
        "ended": ingest.ended.iso_millis(),
        "waited_ms": ingest.acquired.map_or_else(|| millis(ingest.started, ingest.ended), |acquired| millis(ingest.started, acquired)),
        "held_ms": ingest.acquired.map(|acquired| millis(acquired, ingest.ended)),
        "total_ms": millis(ingest.started, ingest.ended),
        "exit": ingest.exit,
        "rows": ingest.rows,
        "holder": ingest.holder,
    });
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}
