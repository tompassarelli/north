use super::tier::Tier;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const FRESH_HOURS: u64 = 36;

pub fn routes(path: &Path, now: SystemTime) -> BTreeMap<String, Tier> {
    let Ok(written) = std::fs::metadata(path).and_then(|meta| meta.modified()) else { return BTreeMap::new() };
    if now.duration_since(written).is_ok_and(|age| age > Duration::from_secs(FRESH_HOURS * 3_600)) {
        return BTreeMap::new();
    }
    let cells: BTreeMap<String, Value> = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    cells.iter().filter_map(|(cell, value)| Some((cell.strip_prefix("route ")?.to_owned(), Tier::parse(value.as_str()?)?))).collect()
}

pub fn current() -> BTreeMap<String, Tier> {
    crate::ops::tune::cells_file().map_or_else(|_| BTreeMap::new(), |path| routes(&path, SystemTime::now()))
}
