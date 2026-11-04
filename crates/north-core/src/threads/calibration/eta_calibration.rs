//! `north threads eta-calibration`: how far the ETA stated at launch lands from the measured active minutes of runs that reported Done.
use std::collections::BTreeMap;

use super::read::raw_runs;
use super::verbs::since_start;
use super::{median, tier_key, tier_rank};
use crate::threads::verb::{Args, Context, Failure, Outcome, open, table};

const DEFAULT_WINDOW: &str = "7d";
const WITHIN: (f64, f64) = (0.5, 2.0);

/// One finished worker run that stated an ETA and recorded active minutes.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Sample {
    pub category: String,
    pub tier: String,
    pub predicted: f64,
    pub actual: f64,
}

impl Sample {
    fn error(&self) -> f64 {
        (self.actual / self.predicted).ln().abs()
    }

    fn within(&self) -> bool {
        (WITHIN.0..=WITHIN.1).contains(&(self.actual / self.predicted))
    }
}

pub(super) fn render(samples: &[Sample]) -> String {
    let mut cells: BTreeMap<(&str, usize, &str), Vec<&Sample>> = BTreeMap::new();
    for sample in samples {
        cells.entry((&sample.category, tier_rank(&sample.tier), &sample.tier)).or_default().push(sample);
    }
    let mut rows = vec![["category", "tier", "n", "median_abs_ln_error", "within_0.5_to_2x"].map(str::to_owned).to_vec()];
    for ((category, _, tier), group) in &cells {
        let errors: Vec<f64> = group.iter().map(|sample| sample.error()).collect();
        let within = group.iter().filter(|sample| sample.within()).count();
        rows.push(vec![
            (*category).to_owned(),
            (*tier).to_owned(),
            group.len().to_string(),
            median(&errors).map_or_else(|| "-".to_owned(), |error| format!("{error:.2}")),
            format!("{}%", within * 100 / group.len()),
        ]);
    }
    table(&rows)
}

fn samples(context: &Context, since: &str) -> Result<Vec<Sample>, Failure> {
    let db = open(context.database, false)?;
    let runs = raw_runs(&db, "outcome = 'done' AND ended >= ?1 AND eta_min > 0 AND active_min > 0", &[&since])?;
    Ok(runs
        .into_iter()
        .filter_map(|raw| Some(Sample { category: raw.cells[0].clone(), tier: tier_key(&raw.tier), predicted: raw.eta_min?, actual: raw.active_min? }))
        .collect())
}

pub(super) fn run(argv: &[String], context: &Context) -> Outcome {
    let args = Args::parse(argv, &["--since"], &[])?;
    let window = args.get("--since").unwrap_or(DEFAULT_WINDOW);
    let found = samples(context, &since_start(window, context.now))?;
    if found.is_empty() {
        return Err(Failure::Refused(format!("no worker run in the last {window} has both an ETA and active minutes")));
    }
    Ok(format!("eta calibration, last {window}: stated ETA against measured active minutes of runs that reported Done\n{}", render(&found)))
}
