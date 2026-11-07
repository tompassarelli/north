//! The box ETA of each brief's facets over a ledger's box chains or over given ledger rows.

use serde_json::{Value, json};

use super::boxes::{BoxRun, box_chains, box_duration};
use super::read::box_rows;
use super::verbs::latest_calibration;
use crate::foundation::clock::Timestamp;
use super::Knobs;
use crate::threads::runs::LABEL_CATEGORY;
use crate::threads::runs::brief::Fields;
use crate::threads::verb::{Failure, open};

fn text(row: &Value, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_owned)
}

pub fn box_run(row: &Value) -> Option<BoxRun> {
    if row.get("role").and_then(Value::as_str).is_some_and(|role| role != "worker") {
        return None;
    }
    Some(BoxRun {
        agent: text(row, "agent").unwrap_or_default(),
        item: text(row, "item")?,
        category: text(row, "category").unwrap_or_default(),
        outcome: text(row, "outcome")?,
        staffed: text(row, "staffed"),
        started: text(row, "started"),
        ended: text(row, "ended").unwrap_or_default(),
        landed: row.get("landed").and_then(Value::as_i64).is_some_and(|landed| landed != 0),
        landed_at: text(row, "landed_at"),
        cost_usd: row.get("cost_usd").and_then(Value::as_f64),
        box_name: text(row, "brief_text").as_deref().map(super::boxes::box_name).unwrap_or_default(),
    })
}

/// The category a brief's facets name, else the first of LABEL_CATEGORY whose `Label: NAME` line they carry.
fn category_of(facets: &str) -> Option<String> {
    let fields = Fields::read(facets);
    fields.known_category().map(str::to_owned).or_else(|| {
        let labels: Vec<&str> = facets.lines().filter_map(|line| line.strip_prefix("Label: ")).filter(|label| !label.contains(char::is_whitespace)).collect();
        LABEL_CATEGORY.iter().find(|(label, _)| labels.contains(label)).map(|(_, category)| (*category).to_owned())
    })
}

/// The box ETA of each facet text, over `rows` when given, else over the ledger at `database`, which also gives the scale.
pub fn box_durations(database: &std::path::Path, now: Timestamp, facets: &[String], rows: Option<&[Value]>) -> Result<Value, Failure> {
    let knobs = Knobs::registry();
    let (runs, scale) = match rows {
        Some(rows) => (rows.iter().filter_map(box_run).collect(), (None, None)),
        None => {
            let db = open(database, false)?;
            let scale = latest_calibration(&db, &knobs, now, "box", "all", true).map(|(value, label)| (value, Some(label))).unwrap_or((None, Some("uncalibrated: no calibration table".to_owned())));
            (box_rows(&db)?, scale)
        }
    };
    let chains = box_chains(&knobs, &runs);
    let boxes: Vec<Value> = facets
        .iter()
        .map(|facets| {
            let (minutes, n, source) = box_duration(&knobs, &chains, category_of(facets).as_deref());
            json!({"minutes": minutes, "n": n, "source": source})
        })
        .collect();
    Ok(json!({"scale": scale.0, "label": scale.1, "boxes": boxes}))
}
