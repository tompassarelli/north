use crate::error::NorthError;

use super::rows::VariantRow;
pub use crate::platform::trials::{TRIALS, Verdict, compare, flips};
use crate::platform::trials::Scenarios;

pub fn covered(scenarios: &Scenarios) -> usize {
    scenarios.values().filter(|cell| cell[1] >= TRIALS).count()
}

fn regressed(best: &VariantRow, row: &VariantRow) -> bool {
    if !flips(&best.scenarios, &row.scenarios).is_empty() {
        return true;
    }
    row.scenarios.iter().any(|(scenario, [passed, n])| {
        best.scenarios.get(scenario).is_some_and(|top| {
            top[1] != 0 && *n != 0 && compare((top[0] as f64 / top[1] as f64, top[1]), (*passed as f64 / *n as f64, *n)) == Verdict::Drop
        })
    })
}

pub fn operating_point(rows: &[VariantRow]) -> Result<&VariantRow, NorthError> {
    let scored: Vec<&VariantRow> = rows.iter().filter(|row| row.conformance.is_some() && row.n != 0).collect();
    let key = |row: &VariantRow| (row.conformance.unwrap_or(0.0), row.n);
    let best = scored
        .iter()
        .copied()
        .reduce(|best, row| if key(row).partial_cmp(&key(best)) == Some(std::cmp::Ordering::Greater) { row } else { best })
        .ok_or_else(|| NorthError::Usage("policy-replay: no variant produced a scored run".into()))?;
    scored
        .into_iter()
        .filter(|row| compare(key(best), key(row)) != Verdict::Drop && !regressed(best, row))
        .min_by(|a, b| {
            (a.startup_tokens, -a.conformance.unwrap_or(0.0))
                .partial_cmp(&(b.startup_tokens, -b.conformance.unwrap_or(0.0)))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .ok_or_else(|| NorthError::Usage("policy-replay: no variant produced a scored run".into()))
}
