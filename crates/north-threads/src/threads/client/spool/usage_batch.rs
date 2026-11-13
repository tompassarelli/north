use std::collections::HashMap;

use serde_json::Value;

fn rows(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split(|byte| *byte == b'\n').filter(|line| !line.trim_ascii().is_empty())
}

/// Whether every row of the spooled batch `data` is a usage row.
pub(super) fn only_usage(data: &[u8]) -> bool {
    rows(data).all(|line| serde_json::from_slice::<Value>(line).is_ok_and(|row| row["type"] == "usage"))
}

/// The usage `batches` as one ingest, a window row that matches the last one kept for its account and window in every field but its reading time taking that row's place, so the ledger keeps the latest reading of each unchanged window.
pub(super) fn batch(batches: &[&[u8]]) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = Vec::new();
    let mut kept: HashMap<(String, String, String), (usize, Value)> = HashMap::new();
    for line in batches.iter().flat_map(|data| rows(data)) {
        let Ok(mut row) = serde_json::from_slice::<Value>(line) else {
            lines.push(line);
            continue;
        };
        if row["kind"] != "window" {
            lines.push(line);
            continue;
        }
        let key = (row["provider"].to_string(), row["account"].to_string(), row["name"].to_string());
        if let Some(fields) = row.as_object_mut() {
            fields.remove("ts");
        }
        match kept.get(&key) {
            Some((slot, reading)) if *reading == row => lines[*slot] = line,
            _ => {
                kept.insert(key, (lines.len(), row));
                lines.push(line);
            }
        }
    }
    let mut out = Vec::new();
    for line in lines {
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}
