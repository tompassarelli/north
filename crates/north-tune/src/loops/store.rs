use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

/// Where a closed loop's last update shows: a record file under the state root, or the newest tick its
/// writer told graph.db.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Store {
    Record(String),
    Graph { writer: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    At(SystemTime),
    Never,
    Unread(String),
}

impl Store {
    pub fn missing(&self) -> String {
        match self {
            Self::Record(record) => format!("no {record}"),
            Self::Graph { writer } => format!("no tick by writer {writer} in graph.db"),
        }
    }
}

/// A graph loop's reading: the newest tick its writer told, from the ticks graph.db holds.
pub fn graph_reading(ticks: &Result<BTreeMap<String, i64>, String>, writer: &str) -> Reading {
    match ticks {
        Err(why) => Reading::Unread(why.clone()),
        Ok(ticks) => ticks.get(writer).and_then(|at| u64::try_from(*at).ok()).map_or(Reading::Never, |at| Reading::At(SystemTime::UNIX_EPOCH + Duration::from_secs(at))),
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
