//! `north threads calibrate`: predicted against actual per provider, optionally appended to the calibration table.
use rusqlite::{Connection, params};

use super::forward::{Figure, Row};
use super::read::{box_rows, calibration_runs};
use super::verbs::since_start;
use super::{Knobs, Run};
use super::boxes::box_chains;
use super::forward::{calibrate_box, calibrate_eta, calibrate_eta_pred, calibrate_tier, calibrate_usd};
use crate::threads::verb::{Args, Context, Failure, Outcome, immediate, open, table};

const GOAL_KINDS: [&str; 2] = ["goal_eta", "goal_actual"];

struct Recorded {
    kind: &'static str,
    row: Row,
    source: String,
}

fn windowed(since: &str) -> bool {
    since.len() > 1 && since.ends_with(['d', 'h']) && since[..since.len() - 1].chars().all(|c| c.is_ascii_digit())
}

type Calibrator = fn(&Knobs, &[Run], &str, &str) -> Vec<Row>;

/// A calibration-table row ready to insert.
struct Stored {
    kind: String,
    cell: String,
    n: i64,
    p25: Option<f64>,
    median: Option<f64>,
    p75: Option<f64>,
    value: Option<f64>,
    source: String,
}

fn insert(db: &mut Connection, at: &str, rows: &[Stored]) -> Result<(), Failure> {
    let tx = immediate(db)?;
    for Stored { kind, cell, n, p25, median, p75, value, source } in rows {
        tx.execute("INSERT INTO calibration(at, kind, cell, n, p25, median, p75, value, source) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![at, kind, cell, n, p25, median, p75, value, source])?;
    }
    tx.commit()?;
    Ok(())
}

fn goal_row(text: &str, context: &Context, at: &str) -> Outcome {
    let refused = |why: String| Failure::Refused(format!("calibrate --goal-row takes a JSON object with kind, cell, n, value and source: {why}"));
    let row: serde_json::Value = serde_json::from_str(text).map_err(|error| refused(error.to_string()))?;
    let field = |name: &str| row.get(name).ok_or_else(|| refused(format!("missing {name}")));
    let kind = field("kind")?.as_str().unwrap_or_default().to_owned();
    let cell = field("cell")?.as_str().ok_or_else(|| refused("cell is not text".into()))?.to_owned();
    let n = field("n")?.as_i64().ok_or_else(|| refused("n is not an integer".into()))?;
    let source = field("source")?.as_str().ok_or_else(|| refused("source is not text".into()))?.to_owned();
    let value = match field("value")? {
        serde_json::Value::Null => None,
        number if number.is_number() => number.as_f64(),
        _ => return Err(Failure::Refused(format!("calibrate --goal-row kind must be one of {} with a numeric value", GOAL_KINDS.join(", ")))),
    };
    if !GOAL_KINDS.contains(&kind.as_str()) {
        return Err(Failure::Refused(format!("calibrate --goal-row kind must be one of {} with a numeric value", GOAL_KINDS.join(", "))));
    }
    insert(&mut open(context.database, true)?, at, &[Stored { kind: kind.clone(), cell: cell.clone(), n, p25: None, median: None, p75: None, value, source }])?;
    Ok(format!("{kind} {cell} recorded at {at}\n"))
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| format!("{value:?}"))
}

fn json_row(recorded: &Recorded) -> String {
    let row = &recorded.row;
    let value = match row.value {
        Some(Figure::Whole(whole)) => whole.to_string(),
        Some(Figure::Real(real)) => number(Some(real)),
        None => "null".to_owned(),
    };
    let text = |text: &str| serde_json::to_string(text).expect("text encodes");
    format!(
        "{{\"kind\": {}, \"cell\": {}, \"n\": {}, \"p25\": {}, \"median\": {}, \"p75\": {}, \"value\": {value}, \"source\": {}}}",
        text(recorded.kind), text(&row.cell), row.n, number(row.p25), number(row.median), number(row.p75), text(&recorded.source)
    )
}

pub(super) fn run(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--since", "--goal-row"], &["--json", "--write"])?;
    let write = args.has("--write");
    if write && context.replica {
        return Err(Failure::Refused("calibrate --write needs the live ledger; this is the read-only snapshot".into()));
    }
    let end = context.now.iso();
    if let Some(text) = args.get("--goal-row") {
        if !write {
            return Err(Failure::Refused("calibrate --goal-row needs --write".into()));
        }
        return goal_row(text, context, &end);
    }
    let since = args.get("--since").unwrap_or("7d");
    let start = since_start(since, context.now);
    let knobs = Knobs::registry();
    let db = open(context.database, false)?;
    let runs = calibration_runs(&db)?;
    let chains = box_chains(&knobs, &box_rows(&db)?);
    let window = if windowed(since) { since.to_owned() } else { format!("since {since}") };
    let mut rows = Vec::new();
    let kinds: [(&str, Calibrator); 4] =
        [("eta", calibrate_eta), ("eta_pred", calibrate_eta_pred), ("usd", calibrate_usd), ("tier", calibrate_tier)];
    for (kind, calibrate) in kinds {
        rows.extend(calibrate(&knobs, &runs, &start, &end).into_iter().map(|row| Recorded { kind, source: format!("measured n={} {window}", row.n), row }));
    }
    rows.extend(calibrate_box(&knobs, &chains, &start, &end).into_iter().map(|row| Recorded { kind: "box", source: format!("measured n={} chains {window}", row.n), row }));
    drop(db);
    if write {
        let values: Vec<Stored> = rows
            .iter()
            .map(|r| Stored { kind: r.kind.to_owned(), cell: r.row.cell.clone(), n: r.row.n as i64, p25: r.row.p25, median: r.row.median, p75: r.row.p75, value: r.row.value.map(Figure::real), source: r.source.clone() })
            .collect();
        insert(&mut open(context.database, true)?, &end, &values)?;
    }
    if args.has("--json") {
        let listed: Vec<String> = rows.iter().map(json_row).collect();
        return Ok(format!("{{\"at\": \"{end}\", \"since\": {}, \"rows\": [{}]}}\n", serde_json::to_string(&start).expect("text encodes"), listed.join(", ")));
    }
    let fixed = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"));
    let mut lines = vec![["kind", "cell", "n", "p25", "median", "p75", "value"].map(str::to_owned).to_vec()];
    lines.extend(rows.iter().map(|r| vec![r.kind.to_owned(), r.row.cell.clone(), r.row.n.to_string(), fixed(r.row.p25), fixed(r.row.median), fixed(r.row.p75), fixed(r.row.value.map(Figure::real))]));
    Ok(table(&lines))
}
