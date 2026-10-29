use crate::error::NorthError;

use super::rows::{Scenarios, VariantRow};

pub(crate) const TRIALS: usize = 3;
const ALPHA: f64 = 0.05;

#[derive(Debug, PartialEq)]
pub(crate) enum Verdict {
    Drop,
    Gain,
    Same,
}

fn comb(n: usize, k: usize) -> f64 {
    let k = k.min(n - k.min(n));
    (1..=k).fold(1.0, |acc, i| acc * (n - k + i) as f64 / i as f64)
}

fn fisher_tail(a: usize, n1: usize, c: usize, n2: usize, upper: bool) -> f64 {
    let (k, total) = (a + c, n1 + n2);
    let (lo, hi) = (k.saturating_sub(n2), k.min(n1));
    let weight = |x: usize| comb(k, x) * comb(total - k, n1 - x);
    let tail: f64 = if upper { (a..=hi).map(weight).sum() } else { (lo..=a).map(weight).sum() };
    tail / comb(total, n1)
}

pub(crate) fn compare(point: (f64, usize), candidate: (f64, usize)) -> Verdict {
    let ((p, n1), (q, n2)) = (point, candidate);
    if n1 == 0 || n2 == 0 {
        return Verdict::Same;
    }
    let (a, c) = ((p * n1 as f64).round_ties_even() as usize, (q * n2 as f64).round_ties_even() as usize);
    if fisher_tail(a, n1, c, n2, true) < ALPHA {
        return Verdict::Drop;
    }
    if fisher_tail(a, n1, c, n2, false) < ALPHA {
        return Verdict::Gain;
    }
    Verdict::Same
}

pub(crate) fn covered(scenarios: &Scenarios) -> usize {
    scenarios.values().filter(|cell| cell[1] >= TRIALS).count()
}

pub(crate) fn flips(point: &Scenarios, candidate: &Scenarios) -> Vec<String> {
    candidate
        .iter()
        .filter(|(scenario, [passed, n])| {
            *n >= TRIALS && *passed == 0 && point.get(*scenario).is_some_and(|top| top[1] >= TRIALS && top[0] == top[1])
        })
        .map(|(scenario, _)| scenario.clone())
        .collect()
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

pub(crate) fn operating_point(rows: &[VariantRow]) -> Result<&VariantRow, NorthError> {
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
