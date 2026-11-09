use super::tier::Route;
use crate::foundation::clock::Timestamp;
use north_graph_loops::{Cells, Learned};
use std::path::PathBuf;
use std::time::SystemTime;

pub const FRESH_HOURS: u64 = 36;
pub const WRITER: &str = "tune";

/// North tune's cells as dispatch reads them from the graph, each through its decision's rule under
/// harness/graph-rules/, with the date of the tick that told them, or none once that tick is older
/// than `FRESH_HOURS`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tuned {
    cells: Cells,
    date: String,
}

/// A number one tune cell decided and the date the tick wrote it.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub value: f64,
    pub date: String,
}

impl Tuned {
    pub fn from_learned(learned: Learned, now: SystemTime) -> Self {
        let now = now.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX));
        let fresh = |at: &i64| now.saturating_sub(*at) <= i64::try_from(FRESH_HOURS * 3_600).unwrap_or(i64::MAX);
        match learned.at.filter(fresh) {
            Some(at) => Self { cells: learned.cells, date: Timestamp::from_unix_seconds(at).iso_date() },
            None => Self::default(),
        }
    }

    pub fn current() -> Self {
        north_graph_loops::read(&north_graph_loops::graph_path(), WRITER).map_or_else(|_| Self::default(), |learned| Self::from_learned(learned, SystemTime::now()))
    }

    /// The `<name> <family> <category>` cell for the route's tier family and category.
    pub fn figure(&self, name: &str, route: &Route) -> Option<Figure> {
        let value = self.cells.get(&format!("{name} {} {}", route.tier.family(), route.category))?.as_f64()?;
        Some(Figure { value, date: self.date.clone() })
    }
}

pub fn directory() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/state/north/tune"))
}
