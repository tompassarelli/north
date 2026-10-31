use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use serde_json::json;

use crate::foundation::clock::Timestamp;

pub(crate) struct Ingest {
    pub(crate) started: Timestamp,
    pub(crate) acquired: Option<Timestamp>,
    pub(crate) ended: Timestamp,
    pub(crate) exit: i32,
}

pub(crate) fn append(path: &Path, ingest: &Ingest) -> io::Result<()> {
    let millis = |from: Timestamp, to: Timestamp| (to.unix_millis() - from.unix_millis()).max(0);
    let line = json!({
        "pid": std::process::id(),
        "started": ingest.started.iso_millis(),
        "ended": ingest.ended.iso_millis(),
        "waited_ms": ingest.acquired.map_or_else(|| millis(ingest.started, ingest.ended), |acquired| millis(ingest.started, acquired)),
        "held_ms": ingest.acquired.map(|acquired| millis(acquired, ingest.ended)),
        "total_ms": millis(ingest.started, ingest.ended),
        "exit": ingest.exit,
    });
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}
