//! The goal ETA the planner last recorded for a repository's current target.
use std::time::Duration;

use serde_json::Value;

use super::host::{Places, seconds};
use crate::platform::sqlite::{self, Access};

/// Epoch of the newest `goal_eta` prediction recorded for this repository's current target.
pub(crate) fn goal_finish(places: &Places, repo: &str, target: Option<f64>) -> Option<f64> {
    let target = target?;
    let rows = sqlite::rows(&places.ledger, Access::ReadOnly, Duration::from_secs(10), "SELECT at, kind, cell, source FROM calibration WHERE kind = 'goal_eta'").unwrap_or_default();
    rows.iter()
        .filter(|row| row["cell"].as_str() == Some(repo))
        .filter_map(|row| {
            let detail: Value = serde_json::from_str(row["source"].as_str()?).ok()?;
            if seconds(detail["target"].as_str()?)? != target {
                return None;
            }
            Some((row["at"].as_str()?.to_owned(), seconds(detail["predicted_finish"].as_str()?)?))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, finish)| finish)
}
