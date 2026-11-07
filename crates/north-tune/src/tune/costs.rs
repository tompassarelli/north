//! The cost per closed box of each staffed cell as each tick read it, one row per cell per day, so
//! a tick can print today's figure beside the last earlier day's for the same cell.
use crate::ledger::ClosedReport;
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
pub struct Cost {
    pub day: String,
    pub cell: String,
    pub usd: f64,
    pub closed: usize,
}

impl Cost {
    pub fn read(row: &Value) -> Option<Self> {
        Some(Self { day: row["day"].as_str()?.to_owned(), cell: row["cell"].as_str()?.to_owned(), usd: row["usd_per_closed"].as_f64()?, closed: usize::try_from(row["closed"].as_u64()?).ok()? })
    }

    fn record(&self, at: &str) -> Value {
        json!({"at": at, "day": self.day, "cell": self.cell, "usd_per_closed": self.usd, "closed": self.closed})
    }
}

/// What a tick adds to the costs: the lines comparing today's figures with the last earlier day's,
/// and the rows of the cells with no row yet for today.
pub struct Today {
    pub lines: Vec<String>,
    pub records: Vec<Value>,
}

pub fn today(week: &ClosedReport, earlier: &[Cost], at: &str) -> Today {
    let day = at.get(..10).unwrap_or(at).to_owned();
    let mut found = Today { lines: Vec::new(), records: Vec::new() };
    for cell in week.cells.iter().filter(|cell| cell.staffed && cell.closed > 0) {
        let name = format!("{} {}", cell.tier, cell.category);
        let now = Cost { day: day.clone(), cell: name.clone(), usd: cell.usd_per_closed(), closed: cell.closed };
        let rows = || earlier.iter().filter(|row| row.cell == name);
        if !rows().any(|row| row.day == day) {
            found.records.push(now.record(at));
        }
        if let Some(was) = rows().filter(|row| row.day < day).max_by(|a, b| a.day.cmp(&b.day)) {
            found.lines.push(format!("cost per closed box {name}: ${:.2} (n {}) today, ${:.2} (n {}) on {}", now.usd, now.closed, was.usd, was.closed, was.day));
        }
    }
    found
}
