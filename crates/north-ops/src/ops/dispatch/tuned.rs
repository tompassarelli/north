use super::tier::{Effort, Route, Tier};
use crate::foundation::clock::Timestamp;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub const FRESH_HOURS: u64 = 36;

/// The cells file as dispatch reads it: every cell the last tick wrote and the date it wrote them,
/// or none once the file is older than `FRESH_HOURS`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tuned {
    cells: BTreeMap<String, Value>,
    date: String,
}

/// A number one tune cell decided and the date the tick wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub value: f64,
    pub date: String,
}

impl Tuned {
    pub fn read(path: &Path, now: SystemTime) -> Self {
        let Ok(written) = std::fs::metadata(path).and_then(|meta| meta.modified()) else { return Self::default() };
        if now.duration_since(written).is_ok_and(|age| age > Duration::from_secs(FRESH_HOURS * 3_600)) {
            return Self::default();
        }
        let cells = std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let seconds = written.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX));
        Self { cells, date: Timestamp::from_unix_seconds(seconds).iso_date() }
    }

    pub fn current() -> Self {
        crate::ops::tune::cells_file().map_or_else(|_| Self::default(), |path| Self::read(&path, SystemTime::now()))
    }

    pub fn routes(&self) -> BTreeMap<String, Tier> {
        self.cells.iter().filter_map(|(cell, value)| Some((cell.strip_prefix("route ")?.to_owned(), Tier::parse(value.as_str()?)?))).collect()
    }

    pub fn efforts(&self) -> BTreeMap<String, Effort> {
        self.cells.iter().filter_map(|(cell, value)| Some((cell.strip_prefix("effort ")?.to_owned(), Effort::parse(value.as_str()?)?))).collect()
    }

    /// The `<name> <family> <category>` cell for the route's tier family and category.
    pub fn figure(&self, name: &str, route: &Route) -> Option<Figure> {
        let value = self.cells.get(&format!("{name} {} {}", route.tier.family(), route.category))?.as_f64()?;
        Some(Figure { value, date: self.date.clone() })
    }
}

pub fn current() -> BTreeMap<String, Tier> {
    Tuned::current().routes()
}

pub fn current_efforts() -> BTreeMap<String, Effort> {
    Tuned::current().efforts()
}
