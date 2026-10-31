//! `north threads box-eta`: the box ETA of each brief's facets over a ledger's box chains or over ledger rows on stdin.
use std::io::Read;

use serde_json::{Value, json};

use super::boxes::{BoxRun, box_chains, box_duration};
use super::read::box_rows;
use super::verbs::latest_calibration;
use super::Knobs;
use crate::threads::runs::LABEL_CATEGORY;
use crate::threads::runs::brief::Fields;
use crate::threads::verb::{Context, Failure, Outcome, open};

fn text(row: &Value, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn box_run(row: &Value) -> Option<BoxRun> {
    if row.get("role").and_then(Value::as_str).is_some_and(|role| role != "worker") {
        return None;
    }
    Some(BoxRun {
        item: text(row, "item")?,
        category: text(row, "category").unwrap_or_default(),
        outcome: text(row, "outcome")?,
        staffed: text(row, "staffed"),
        started: text(row, "started"),
        ended: text(row, "ended").unwrap_or_default(),
        landed: row.get("landed").and_then(Value::as_i64).is_some_and(|landed| landed != 0),
        landed_at: text(row, "landed_at"),
        cost_usd: row.get("cost_usd").and_then(Value::as_f64),
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

pub(super) fn run(context: &Context, stdin: &mut dyn Read) -> Outcome {
    let mut input = String::new();
    stdin.read_to_string(&mut input).map_err(|error| Failure::Refused(format!("box-eta cannot read stdin: {error}")))?;
    let request: Value = serde_json::from_str(&input).map_err(|error| Failure::Usage(format!("box-eta reads {{\"facets\": [...], \"rows\": [...]}} on stdin: {error}")))?;
    let facets: Vec<&str> = request.get("facets").and_then(Value::as_array).ok_or_else(|| Failure::Usage("box-eta needs a facets list".into()))?.iter().filter_map(Value::as_str).collect();
    let knobs = Knobs::registry();
    let (runs, scale) = match request.get("rows").and_then(Value::as_array) {
        Some(rows) => (rows.iter().filter_map(box_run).collect(), (None, None)),
        None => {
            let db = open(context.database, false)?;
            let scale = latest_calibration(&db, &knobs, context.now, "box", "all", true).map(|(value, label)| (value, Some(label))).unwrap_or((None, Some("uncalibrated: no calibration table".to_owned())));
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
    Ok(format!("{}\n", json!({"scale": scale.0, "label": scale.1, "boxes": boxes})))
}
